//! Lexical reference extraction from PIR-lowered HirFile.
//!
//! This module defines the lexical fact extractor, which walks a per-body PIR lowering
//! and collects all lexical reads and writes, grouped by body boundary to preserve scope isolation.
//!
//! The extractor is used by PR2 (#2634) for shadow comparison and later by providers for
//! navigation and reference detection. PR1 defines the core API only; no provider changes.

use crate::hir::{
    BodyOwnerKind, HirBindingId, HirBodyId, HirExpr, HirExprId, HirFile, HirKind, HirStmt,
    HirStmtId,
};
use crate::pir::lower::lower_single_body;
use crate::pir::model::{LexicalName, PirOperation, PirSourceAnchor};
use std::collections::BTreeMap;

/// Current schema version for lexical extractor receipts.
///
/// Version 3 attaches the canonical HIR [`HirBindingId`] onto each fact so
/// same-spelling nested lexicals in one body stay distinguishable. PIR
/// *operations* still do not carry that identity (#6659 item 2).
pub const LEXICAL_EXTRACTOR_RECEIPT_VERSION: u32 = 3;

/// A single lexical variable binding fact extracted from PIR.
///
/// Each fact represents one read or write of a lexical (`my`/`state`) variable,
/// anchored to its source range and discriminated by body identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LexicalBindingFact {
    /// The lexical variable (sigil + name).
    pub name: LexicalName,
    /// Whether this fact records a read or write.
    pub role: LexicalRole,
    /// Source anchor for this reference (always anchored in PR1).
    pub source_anchor: PirSourceAnchor,
    /// Body index where this binding occurs (0-based).
    pub body_idx: usize,
    /// Body owner kind (what construct owns this body).
    pub body_owner: BodyOwnerKind,
    /// Canonical HIR binding this occurrence resolves to, when the scope graph
    /// recorded one. Nested same-spelling `my $x` declarations in one body
    /// therefore stay distinct here even though PIR operations still store
    /// only sigil+name.
    pub binding: Option<HirBindingId>,
}

/// Role of a lexical binding fact (read or write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LexicalRole {
    /// Read access to a lexical variable.
    Read,
    /// Write access (declaration or assignment) to a lexical variable.
    Write,
}

/// Extraction results for a single body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BodyExtractionResult {
    /// Index of this body (0-based, matches position in HirFile::bodies).
    pub body_idx: usize,
    /// Owner of this body (ProgramRoot or Subroutine).
    pub owner: BodyOwnerKind,
    /// All lexical binding facts in this body, in lowering order.
    pub facts: Vec<LexicalBindingFact>,
    /// Count of nodes that had source anchors.
    pub anchored_node_count: usize,
    /// Total count of all nodes processed (anchored + non-anchored).
    pub total_node_count: usize,
}

/// Receipt from the lexical extractor, summarizing all bindings extracted from a file.
///
/// This is distinct from [`PirReceipt`](crate::pir::PirReceipt), which models lowering stats.
/// `LexicalExtractorReceipt` is a compiler-substrate proof surface: it records what the
/// extractor found, not what the lowerer did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LexicalExtractorReceipt {
    /// Schema version (currently 3).
    pub schema_version: u32,
    /// Per-body extraction results, in order.
    pub bodies: Vec<BodyExtractionResult>,
    /// Total count of lexical reads across all bodies.
    pub total_read_count: usize,
    /// Total count of lexical writes across all bodies.
    pub total_write_count: usize,
    /// Count of compound read-modify-write operations skipped (`Modify`,
    /// `StashModify`, `FieldModify`).
    pub skipped_node_count: usize,
    /// Count of dynamic boundaries observed while lowering lexical facts.
    pub dynamic_boundary_count: usize,
    /// Whether provider behavior changed (always false for PR1).
    pub provider_behavior_changed: bool,
}

/// Extract lexical binding facts from a HirFile.
///
/// Lowers each body independently and collects all `LexicalRead` and `LexicalWrite`
/// operations, preserving body boundaries so scope isolation is unambiguous.
///
/// # Arguments
///
/// * `file` - A HIR file with populated `.bodies` (from the second pass of HIR lowering)
///
/// # Returns
///
/// A receipt containing all facts grouped by body, plus summary counts.
///
/// # Invariants
///
/// - All emitted facts have `source_anchor.is_anchored() == true`
/// - `Modify`/`StashModify`/`FieldModify` operations are skipped (not counted as facts,
///   but tracked in `skipped_node_count`)
/// - `StashRead`/`StashWrite` and `FieldRead`/`FieldWrite` are ignored (not extracted in PR1)
/// - `DynamicBoundary` operations are tracked so promotion can refuse exactness honestly
/// - `provider_behavior_changed` is always `false`
/// - `total_read_count + total_write_count == sum(bodies[].facts.len())`
#[must_use]
pub fn extract_lexical_facts(file: &HirFile) -> LexicalExtractorReceipt {
    let mut bodies = Vec::new();
    let mut total_read_count = 0usize;
    let mut total_write_count = 0usize;
    let mut skipped_node_count = 0usize;
    let dynamic_boundary_count =
        file.items.iter().filter(|item| matches!(&item.kind, HirKind::DynamicBoundary(_))).count();
    let bindings_by_range = hir_bindings_by_range(file);

    for (body_idx, body) in file.bodies.iter().enumerate() {
        let owner = body.owner.clone();
        let mut facts = Vec::new();
        let mut anchored_node_count = 0usize;
        let mut total_node_count = 0usize;

        // Lower this body in isolation — fresh lowerer per body preserves scope boundaries.
        let pir_nodes = lower_single_body(body, HirBodyId(body_idx as u32), file);

        for pir_node in pir_nodes {
            total_node_count += 1;

            match &pir_node.operation {
                PirOperation::LexicalRead { name } if pir_node.source_anchor.is_anchored() => {
                    anchored_node_count += 1;
                    push_anchored_lexical_fact(
                        &mut facts,
                        name.clone(),
                        LexicalRole::Read,
                        &pir_node.source_anchor,
                        body_idx,
                        &owner,
                        &bindings_by_range,
                    );
                    total_read_count += 1;
                }
                PirOperation::LexicalWrite { name } if pir_node.source_anchor.is_anchored() => {
                    anchored_node_count += 1;
                    push_anchored_lexical_fact(
                        &mut facts,
                        name.clone(),
                        LexicalRole::Write,
                        &pir_node.source_anchor,
                        body_idx,
                        &owner,
                        &bindings_by_range,
                    );
                    total_write_count += 1;
                }
                PirOperation::Modify { .. }
                | PirOperation::StashModify { .. }
                | PirOperation::FieldModify { .. } => {
                    // Every compound read-modify-write is explicitly skipped: none is a Read
                    // or a Write in the lexical-fact model. Track them so the receipt surface
                    // is honest about what was filtered.
                    //
                    // `FieldModify` belongs here with the other two even though a field is not
                    // lexical storage, because the counter records *modifications that were
                    // filtered*, not lexical ones only. Leaving it on the wildcard arm made a
                    // method's `$n += 2` report `skipped_node_count == 0` while the identical
                    // lexical and stash statements each reported 1 (#13817).
                    skipped_node_count += 1;
                }
                // StashRead/StashWrite and FieldRead/FieldWrite are non-lexical reads and
                // writes, outside the lexical-fact model; Call, MethodCall, Assign, etc. are
                // outside PR1 scope. All silently ignored — not facts, not skipped.
                _ => {}
            }
        }

        bodies.push(BodyExtractionResult {
            body_idx,
            owner,
            facts,
            anchored_node_count,
            total_node_count,
        });
    }

    LexicalExtractorReceipt {
        schema_version: LEXICAL_EXTRACTOR_RECEIPT_VERSION,
        bodies,
        total_read_count,
        total_write_count,
        skipped_node_count,
        dynamic_boundary_count,
        provider_behavior_changed: false,
    }
}

fn push_anchored_lexical_fact(
    facts: &mut Vec<LexicalBindingFact>,
    name: LexicalName,
    role: LexicalRole,
    source_anchor: &PirSourceAnchor,
    body_idx: usize,
    owner: &BodyOwnerKind,
    bindings_by_range: &BTreeMap<(usize, usize), HirBindingId>,
) {
    facts.push(LexicalBindingFact {
        name,
        role,
        source_anchor: source_anchor.clone(),
        body_idx,
        body_owner: owner.clone(),
        binding: binding_for_anchor(bindings_by_range, source_anchor),
    });
}

/// Index canonical HIR bindings by the source range of each occurrence.
///
/// PIR lexical ops still carry only sigil+name (#6659 item 2). Join the
/// already-canonical [`HirBindingId`] at extract time so a nested same-spelling
/// `my $x` does not merge with the outer binding.
fn hir_bindings_by_range(file: &HirFile) -> BTreeMap<(usize, usize), HirBindingId> {
    let mut bindings = BTreeMap::new();
    for body in &file.bodies {
        for idx in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(idx as u32);
            let Some(HirExpr::Variable(var)) = body.expr(id) else {
                continue;
            };
            let Some(binding) = var.binding else {
                continue;
            };
            let Some(range) = body.source_map.expr_range(id) else {
                continue;
            };
            bindings.insert((range.start, range.end), binding);
        }
        for idx in 0..body.source_map.stmt_ranges.len() {
            let id = HirStmtId(idx as u32);
            let Some(HirStmt::Let { binding, binding_range, .. }) = body.stmt(id) else {
                continue;
            };
            let Some(binding) = *binding else {
                continue;
            };
            bindings.insert((binding_range.start, binding_range.end), binding);
        }
    }
    bindings
}

fn binding_for_anchor(
    bindings: &BTreeMap<(usize, usize), HirBindingId>,
    anchor: &PirSourceAnchor,
) -> Option<HirBindingId> {
    let range = anchor.range.as_ref()?;
    bindings.get(&(range.start, range.end)).copied()
}
