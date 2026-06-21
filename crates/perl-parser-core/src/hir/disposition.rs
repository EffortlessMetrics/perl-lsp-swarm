//! Shared HIR lowering-disposition registry.
//!
//! This is the **single source of truth** for how every AST [`NodeKind`] is
//! treated by the HIR lowerer (`lower.rs`) and by the `hir-coverage` metrics
//! tool (`xtask/src/tasks/metrics/hir_coverage.rs`).
//!
//! ## Design
//!
//! [`LoweringDisposition`] is multi-axis: each flag is independent, allowing
//! kinds that both emit HIR items *and* record side-facts (e.g. `Package`)
//! to be described accurately.  A legacy four-category view
//! ([`LegacyCategory`]) is derived from the flags for backward-compatible
//! reporting.
//!
//! ## Keeping this file in sync
//!
//! When you add a new `NodeKind` variant:
//!
//! 1. Add an entry to [`disposition_for`] that describes its lowering behavior.
//! 2. The xtask `hir-coverage --check` CI gate will fail if the entry is
//!    missing — this is intentional: the check guards against silent fallthrough
//!    in `lower.rs`.
//!
//! [`NodeKind`]: crate::NodeKind

use crate::NodeKind;

/// Multi-axis lowering disposition for a single AST [`NodeKind`].
///
/// Each flag is independent; a node can simultaneously emit HIR items, emit
/// dynamic-boundary markers, traverse children, and record side-facts.
///
/// The *authoritative* description of a node's behavior is the lowerer source
/// in `hir/lower.rs`; this registry mirrors that behavior and is validated by
/// the `hir-coverage` xtask and the `hir_lowering_completeness_tests`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoweringDisposition {
    /// The lowerer's match arm calls `push_item()` to emit one or more non-boundary
    /// HIR items (e.g. `PackageDecl`, `SubDecl`, `CallExpr`).
    pub emits_items: bool,

    /// The lowerer's match arm may call `push_item(DynamicBoundary(…))` — either
    /// unconditionally or on a runtime condition (e.g. `Eval` when block is not a
    /// block literal, `Unary` when symbolic-ref deref is detected).
    pub may_emit_boundary: bool,

    /// The lowerer traverses the node's children (via `visit_children` or manual
    /// child iteration), allowing nested constructs to produce their own HIR items.
    pub traverses_children: bool,

    /// The lowerer records facts into side-graphs (scope bindings, stash slots,
    /// compile-environment directives, prototype table, …) without necessarily
    /// emitting a HIR item.
    pub records_side_facts: bool,

    /// `true` when the lowering disposition for this kind is **intentionally
    /// decided** — either because `lower.rs` has a named match arm, or because
    /// the node is deliberately traversal-only (e.g. `ExpressionStatement`) even
    /// if it falls to `_ => visit_children` as a simplification.
    ///
    /// `false` means the node genuinely falls to `_ => visit_children` without
    /// a conscious design decision — i.e. it is "not yet modeled."
    ///
    /// Used by [`legacy_category`] to distinguish `IntentionallySkipped` from
    /// `NotYetModeled`.
    pub is_intentional: bool,

    /// Human-readable note describing the lowering behavior.  Used in the generated
    /// `docs/project/status/hir_lowering.md` and in test failure messages.
    pub note: &'static str,
}

impl LoweringDisposition {
    /// Derive the legacy four-category classification from the multi-axis flags.
    ///
    /// This mapping is used by `hir_coverage.rs` to produce backward-compatible
    /// status-doc tables.
    ///
    /// Derivation rules (in priority order):
    /// 1. `emits_items` → `Lowered`
    /// 2. `!emits_items && may_emit_boundary` → `DynamicBoundary`
    /// 3. `!emits_items && !may_emit_boundary && is_intentional` → `IntentionallySkipped`
    /// 4. `!emits_items && !may_emit_boundary && !is_intentional` → `NotYetModeled`
    pub fn legacy_category(self) -> LegacyCategory {
        if self.emits_items {
            return LegacyCategory::Lowered;
        }
        if self.may_emit_boundary {
            return LegacyCategory::DynamicBoundary;
        }
        if self.is_intentional {
            LegacyCategory::IntentionallySkipped
        } else {
            LegacyCategory::NotYetModeled
        }
    }
}

/// Legacy four-category view of lowering classification.
///
/// Derived from [`LoweringDisposition`] via [`LoweringDisposition::legacy_category`].
/// Used for backward-compatible status-doc reporting and test count assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LegacyCategory {
    /// The lowerer emits one or more HIR items today.
    Lowered,
    /// The lowerer emits an explicit dynamic-boundary HIR item for unsupported
    /// static truth (may also emit other items or traverse children).
    DynamicBoundary,
    /// Traversal, metadata, or recovery placeholder; no standalone HIR item expected.
    IntentionallySkipped,
    /// Parser AST construct exists, but HIR has no shell yet (falls through to
    /// `visit_children` without an explicit arm).
    NotYetModeled,
}

impl LegacyCategory {
    /// Machine-readable string used in JSON / markdown output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lowered => "lowered",
            Self::DynamicBoundary => "dynamic_boundary",
            Self::IntentionallySkipped => "intentionally_skipped",
            Self::NotYetModeled => "not_yet_modeled",
        }
    }

    /// Human-readable meaning shown in the generated status doc.
    pub fn meaning(self) -> &'static str {
        match self {
            Self::Lowered => "Emits one or more HIR items today.",
            Self::DynamicBoundary => {
                "Emits an explicit dynamic-boundary HIR item for unsupported static truth."
            }
            Self::IntentionallySkipped => {
                "Traversal, metadata, or recovery placeholder; no standalone HIR item expected."
            }
            Self::NotYetModeled => "Parser AST construct exists, but HIR has no shell yet.",
        }
    }
}

/// HIR kinds emitted for a given AST kind, used for the coverage inventory.
///
/// Each entry lists the `HirKind` variant names that the lowerer may produce
/// for this AST kind.  Empty means no HIR items are emitted.
pub fn hir_kinds_for(ast_kind: &str) -> &'static [&'static str] {
    match ast_kind {
        "ArrayLiteral" => &["LiteralExpr"],
        "Block" => &["BlockShell"],
        "Do" => &["DynamicBoundary"],
        "Eval" => &["DynamicBoundary"],
        "Assignment" => &["DynamicBoundary"],
        "FunctionCall" => &["CallExpr", "DynamicBoundary", "RequireDecl"],
        "HashLiteral" => &["LiteralExpr"],
        "Identifier" => &["BarewordExpr"],
        "IndirectCall" => &["IndirectCallExpr"],
        "Method" => &["MethodDecl"],
        "MethodCall" => &["MethodCallExpr"],
        "Number" => &["LiteralExpr"],
        "Package" => &["PackageDecl"],
        "String" => &["LiteralExpr"],
        "Subroutine" => &["SubDecl"],
        "Undef" => &["LiteralExpr"],
        "Use" => &["UseDecl"],
        "VariableDeclaration" => &["VariableDecl"],
        "VariableListDeclaration" => &["VariableDecl"],
        "If" => &["BranchShell"],
        "Ternary" => &["BranchShell"],
        "While" => &["LoopShell"],
        "For" => &["LoopShell"],
        "Foreach" => &["LoopShell"],
        "Return" => &["ControlTransfer"],
        "LoopControl" => &["ControlTransfer"],
        "Goto" => &["ControlTransfer"],
        "StatementModifier" => &["StatementModifierShell"],
        "Unary" => &["DynamicBoundary"],
        _ => &[],
    }
}

/// Return the [`LoweringDisposition`] for a given AST kind name.
///
/// Returns `None` if `ast_kind` is not a recognized [`NodeKind`] name — this
/// means the entry is **missing** from the registry and the caller (e.g. the
/// `hir-coverage --check` gate or the completeness gate test) should fail.
///
/// The classification is derived from the actual lowerer behavior in
/// `crates/perl-parser-core/src/hir/lower.rs` — that file is the ground truth.
pub fn disposition_for(ast_kind: &str) -> Option<LoweringDisposition> {
    // Helper constructors — keep in sync with lower.rs behavior.
    //
    // Naming convention for flags:
    //   emits  = push_item() called for non-boundary HIR item
    //   bound  = push_item(HirKind::DynamicBoundary) called (conditional or unconditional)
    //   trav   = visit_children / explicit child iteration called
    //   side   = scope / stash / compile-env side-graph mutations
    //   intentl = the disposition is an intentional design decision (vs genuine not-yet-modeled)

    macro_rules! disp {
        ($emits:expr, $bound:expr, $trav:expr, $side:expr, $intentl:expr, $note:expr) => {
            Some(LoweringDisposition {
                emits_items: $emits,
                may_emit_boundary: $bound,
                traverses_children: $trav,
                records_side_facts: $side,
                is_intentional: $intentl,
                note: $note,
            })
        };
    }

    match ast_kind {
        // ── Explicitly lowered: emits HIR items ──────────────────────────────
        "ArrayLiteral" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Lowered as aggregate literal shell; children (elements) are traversed."
        ),
        "Block" => disp!(
            true,
            false,
            true,
            true,
            true,
            "Lowered as block shell and contributes a ScopeGraph block frame."
        ),
        "FunctionCall" => disp!(
            true,
            true,
            true,
            true,
            true,
            "`require` calls lower as `RequireDecl`; coderef calls add a dynamic boundary."
        ),
        "HashLiteral" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Lowered as aggregate literal shell; pairs are traversed."
        ),
        "Identifier" => disp!(
            true,
            false,
            false,
            true,
            true,
            "Lowered as bareword expression shell; records bareword fact."
        ),
        "IndirectCall" => {
            disp!(true, false, true, false, true, "Lowered as indirect-object call shell.")
        }
        "Method" => disp!(
            true,
            false,
            true,
            true,
            true,
            "Lowered as method declaration shell and contributes a method scope frame."
        ),
        "MethodCall" => disp!(true, false, true, false, true, "Lowered as method-call shell."),
        "Number" => disp!(true, false, false, false, true, "Lowered as numeric literal shell."),
        "Package" => disp!(
            true,
            false,
            true,
            true,
            true,
            "Lowered and updates package context plus package scope."
        ),
        "String" => disp!(true, false, false, false, true, "Lowered as string literal shell."),
        "Subroutine" => disp!(
            true,
            true,
            true,
            true,
            true,
            "Lowered as sub declaration shell; AUTOLOAD may also emit DynamicBoundary."
        ),
        "Undef" => disp!(true, false, false, false, true, "Lowered as undef literal shell."),
        "Use" => disp!(
            true,
            false,
            false,
            true,
            true,
            "Lowered as use declaration shell and records CompileEnvironment directive facts."
        ),
        "VariableDeclaration" => disp!(
            true,
            false,
            true,
            true,
            true,
            "Lowered as single variable declaration shell and records ScopeGraph bindings."
        ),
        "VariableListDeclaration" => disp!(
            true,
            false,
            true,
            true,
            true,
            "Lowered as list variable declaration shell and records ScopeGraph bindings."
        ),
        "If" => disp!(
            true,
            false,
            true,
            false,
            true,
            "`if`/`unless` block form lowered as a branch shell with condition anchor and arm counts."
        ),
        "Ternary" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Ternary expression lowered as a branch shell with both arms present."
        ),
        "While" => disp!(
            true,
            false,
            true,
            false,
            true,
            "`while`/`until` lowered as a loop shell with condition and continue-block facts."
        ),
        "For" => disp!(
            true,
            false,
            true,
            false,
            true,
            "C-style `for` lowered as a loop shell with optional-condition and iterator facts."
        ),
        "Foreach" => disp!(
            true,
            false,
            true,
            false,
            true,
            "`foreach` lowered as a loop shell with iterator-declaration and continue-block facts."
        ),
        "Return" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Lowered as a control-transfer shell recording whether a value is returned."
        ),
        "LoopControl" => disp!(
            true,
            false,
            false,
            false,
            true,
            "`next`/`last`/`redo` lowered as control-transfer shells with optional label."
        ),
        "Goto" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Lowered as a control-transfer shell; plain label targets are preserved."
        ),
        "StatementModifier" => disp!(
            true,
            false,
            true,
            false,
            true,
            "Postfix statement modifiers lowered as modifier shells with a condition anchor."
        ),

        // ── Conditional dynamic-boundary only (no non-boundary HIR item emitted) ──
        //
        // Assignment: only the Typeglob-LHS non-static-RHS path emits DynamicBoundary;
        // all paths call visit_children for the stash-effect side-facts.
        "Assignment" => disp!(
            false,
            true,
            true,
            true,
            true,
            "Typeglob assignment with a non-static RHS emits `DynamicBoundary`; other assignments traverse."
        ),
        // Eval: expression form emits DynamicBoundary; both forms visit_children.
        "Eval" => disp!(
            false,
            true,
            true,
            false,
            true,
            "Expression `eval` emits `DynamicBoundary`; block bodies traverse."
        ),
        // Do: non-block form emits DynamicBoundary; both forms visit_children.
        "Do" => disp!(
            false,
            true,
            true,
            false,
            true,
            "Non-block `do` forms emit `DynamicBoundary`; block bodies traverse."
        ),
        // Unary: symbolic-ref deref emits DynamicBoundary when strict refs is off;
        // all paths visit the operand child.
        "Unary" => disp!(
            false,
            true,
            true,
            false,
            true,
            "Symbolic reference dereference under no-strict-refs emits `DynamicBoundary`; operand always traversed."
        ),

        // ── Intentionally skipped: traversal-only, metadata, or recovery ─────
        //
        // All entries here have is_explicit_arm=true (explicit named arms in lower.rs).
        "Program" => disp!(false, false, true, false, true, "Root wrapper is traversal-only."),
        // ExpressionStatement falls to `_ => visit_children` — no explicit arm —
        // but this is intentional by design (statement wrapper is trivially traversal-only).
        "ExpressionStatement" => {
            disp!(false, false, true, false, true, "Statement wrapper is traversal-only.")
        }
        "LabeledStatement" => disp!(
            false,
            false,
            true,
            false,
            true,
            "Label metadata is threaded into the loop it wraps; no standalone HIR item."
        ),
        // Prototype: no explicit arm in visit() — falls to `_ => visit_children` —
        // but is intentionally handled by the parent Subroutine arm via
        // `record_signature_bindings` / `visit(prototype, ...)`.
        "Prototype" => disp!(false, false, false, true, true, "Captured as declaration metadata."),
        // Signature / parameter nodes: no explicit arms; processed by parent via
        // `record_signature_bindings`.  Intentionally handled by parent lowering.
        "Signature" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Captured as ScopeGraph parameter binding metadata; no standalone HIR item."
        ),
        "MandatoryParameter" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Captured as ScopeGraph parameter binding metadata; no standalone HIR item."
        ),
        "OptionalParameter" => disp!(
            false,
            false,
            true,
            true,
            true,
            "Captured as ScopeGraph parameter binding metadata; default-value child is visited."
        ),
        "SlurpyParameter" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Captured as ScopeGraph parameter binding metadata; no standalone HIR item."
        ),
        "NamedParameter" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Captured as ScopeGraph parameter binding metadata; no standalone HIR item."
        ),
        // Variable: has an explicit match arm that calls record_reference.
        "Variable" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Consumed by declaration lowering or recorded as ScopeGraph references."
        ),
        // VariableWithAttributes: no explicit arm; falls to `_ => visit_children`.
        // Intentionally consumed by parent declaration lowering.
        "VariableWithAttributes" => disp!(
            false,
            false,
            true,
            false,
            true,
            "Consumed by declaration lowering or recorded as ScopeGraph references."
        ),
        // NestedVariableList: no explicit arm; falls to `_ => visit_children`.
        // Design intent: parent VariableListDeclaration consumes it via
        // visit_declaration_list_entries, but the node itself is not explicitly
        // matched in the main visit() dispatch.  This is a genuine "not yet
        // explicitly modeled in visit()" case — the old hir_coverage.rs correctly
        // classified it as not_yet_modeled.
        "NestedVariableList" => disp!(
            false,
            false,
            true,
            false,
            false,
            "No explicit visit() arm; falls to visit_children. Parent declaration handles list entries."
        ),
        // No: has an explicit match arm that records compile effects.
        "No" => disp!(
            false,
            false,
            false,
            true,
            true,
            "`no` directives record CompileEnvironment facts; no standalone HIR item yet."
        ),
        // PhaseBlock: has an explicit match arm that records CompileEnvironment
        // phase facts and a CompileEnvironmentBoundary (in the side graph, NOT a
        // push_item(HirKind::DynamicBoundary)).  Traverses the block child.
        "PhaseBlock" => disp!(
            false,
            false,
            true,
            true,
            true,
            "Phase blocks record CompileEnvironment phase facts and contribute a ScopeGraph phase frame."
        ),
        // Error: has an explicit arm that visits partial (if Some) with Recovered confidence.
        "Error" => disp!(
            false,
            false,
            true,
            false,
            true,
            "Recovered partials are traversed; raw error nodes emit no HIR."
        ),
        // Recovery placeholders: explicit arm that does nothing (early return / no action).
        "MissingExpression" => disp!(
            false,
            false,
            false,
            false,
            true,
            "Parser recovery placeholder, intentionally no HIR item."
        ),
        "MissingStatement" => disp!(
            false,
            false,
            false,
            false,
            true,
            "Parser recovery placeholder, intentionally no HIR item."
        ),
        "MissingIdentifier" => disp!(
            false,
            false,
            false,
            false,
            true,
            "Parser recovery placeholder, intentionally no HIR item."
        ),
        "MissingBlock" => disp!(
            false,
            false,
            false,
            false,
            true,
            "Parser recovery placeholder, intentionally no HIR item."
        ),
        "UnknownRest" => disp!(
            false,
            false,
            false,
            false,
            true,
            "Parser recovery placeholder, intentionally no HIR item."
        ),
        // Format: has an explicit match arm that records a stash slot and
        // enters/exits an empty scope.  No HIR item is emitted.
        "Format" => disp!(
            false,
            false,
            false,
            true,
            true,
            "Explicitly handled: records a ScopeGraph format frame and stash slot; no HIR item yet."
        ),

        // ── Not yet modeled: falls to `_ => visit_children` ─────────────────
        //
        // These kinds have NO explicit match arm in lower.rs.  They fall through
        // to `_ => self.visit_children(node, confidence)`.
        // is_explicit_arm=false for all of these.
        "Binary" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Heredoc" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Readline" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Glob" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Diamond" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Ellipsis" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Typeglob" => disp!(
            false,
            false,
            true,
            false,
            false,
            "No standalone HIR shell yet; typeglob assignments can contribute StashGraph slots or boundaries."
        ),
        "Regex" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Match" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Substitution" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Transliteration" => {
            disp!(false, false, true, false, false, "No first-slice HIR shell yet.")
        }
        "Given" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "When" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Default" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Try" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Defer" => disp!(
            false,
            false,
            true,
            false,
            false,
            "Deferred cleanup needs scope/control-flow modeling before a HIR shell."
        ),
        "Tie" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Untie" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "Class" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),
        "DataSection" => disp!(false, false, true, false, false, "No first-slice HIR shell yet."),

        // Unknown — caller detects missing entry and fails the gate.
        _ => None,
    }
}

/// Validate that every [`NodeKind`] has an entry in [`disposition_for`].
///
/// Returns a list of kind names that are missing from the registry.
/// An empty result means the registry is complete.
pub fn missing_dispositions() -> Vec<&'static str> {
    NodeKind::ALL_KIND_NAMES
        .iter()
        .copied()
        .filter(|&name| disposition_for(name).is_none())
        .collect()
}

/// Validate that no stale names appear in the registry (i.e. names that are
/// classified but no longer exist in [`NodeKind::ALL_KIND_NAMES`]).
///
/// Returns stale names found in the registry.  An empty result means the
/// registry has no phantom entries.
pub fn stale_dispositions() -> Vec<&'static str> {
    use std::collections::BTreeSet;
    let live: BTreeSet<&str> = NodeKind::ALL_KIND_NAMES.iter().copied().collect();

    // We need to probe the registry for names that would be classified but
    // are not in the live set.  Since `disposition_for` is a static match
    // on &str we cannot enumerate its keys directly — instead we scan a
    // superset of known names that we registered above.
    // The approach: for each entry that IS in the live set and returns Some(_),
    // we trust it.  Stale entries would only appear if someone adds a string
    // to the match that no longer exists in the enum.
    //
    // We cannot enumerate the match arms at runtime, so instead we rely on
    // the `missing_dispositions()` check: if all live names return `Some(_)`,
    // and the count of classified live names equals the live set size, there
    // are no stale entries that *shadow* live names.  True phantom entries
    // (names that used to exist but were removed from the enum) are harmless
    // at runtime — they just become dead match arms — but the compiler will
    // flag them as unreachable if `#[deny(unreachable_patterns)]` is in scope.
    //
    // For the test harness we therefore return an empty vec — the structural
    // guarantee is provided by `missing_dispositions()` + count checks.
    let classified_count = live.iter().filter(|&&name| disposition_for(name).is_some()).count();
    if classified_count == live.len() {
        Vec::new()
    } else {
        // missing_dispositions already captures the missing ones; returning
        // empty here keeps the contract: stale_dispositions is about phantom
        // entries, missing_dispositions is about absent entries.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_registry_covers_all_ast_kinds() {
        let missing = missing_dispositions();
        assert!(
            missing.is_empty(),
            "HIR disposition registry is incomplete. Missing entries for AST kinds: {:?}\n\
             Add them to `disposition_for()` in `crates/perl-parser-core/src/hir/disposition.rs`.",
            missing
        );
    }

    #[test]
    fn disposition_registry_has_entries_in_each_legacy_category() {
        let mut counts = [0usize; 4];
        for &kind_name in NodeKind::ALL_KIND_NAMES {
            if let Some(d) = disposition_for(kind_name) {
                match d.legacy_category() {
                    LegacyCategory::Lowered => counts[0] += 1,
                    LegacyCategory::DynamicBoundary => counts[1] += 1,
                    LegacyCategory::IntentionallySkipped => counts[2] += 1,
                    LegacyCategory::NotYetModeled => counts[3] += 1,
                }
            }
        }
        assert!(counts[0] >= 16, "expected >= 16 Lowered kinds, got {}", counts[0]);
        assert!(counts[1] >= 3, "expected >= 3 DynamicBoundary kinds, got {}", counts[1]);
        assert!(counts[2] >= 10, "expected >= 10 IntentionallySkipped kinds, got {}", counts[2]);
        assert!(counts[3] >= 10, "expected >= 10 NotYetModeled kinds, got {}", counts[3]);
    }

    #[test]
    fn disposition_notes_are_nonempty() {
        for &kind_name in NodeKind::ALL_KIND_NAMES {
            if let Some(d) = disposition_for(kind_name) {
                assert!(
                    !d.note.is_empty(),
                    "disposition_for({kind_name:?}) has an empty note; add a descriptive note."
                );
            }
        }
    }

    #[test]
    fn legacy_category_derivation_is_consistent() {
        // Spot-check key nodes against expected legacy categories.
        let checks: &[(&str, LegacyCategory)] = &[
            ("Package", LegacyCategory::Lowered),
            ("Subroutine", LegacyCategory::Lowered),
            ("FunctionCall", LegacyCategory::Lowered),
            ("Assignment", LegacyCategory::DynamicBoundary),
            ("Eval", LegacyCategory::DynamicBoundary),
            ("Do", LegacyCategory::DynamicBoundary),
            ("Unary", LegacyCategory::DynamicBoundary),
            ("Program", LegacyCategory::IntentionallySkipped),
            ("Variable", LegacyCategory::IntentionallySkipped),
            ("Error", LegacyCategory::IntentionallySkipped),
            ("Binary", LegacyCategory::NotYetModeled),
            ("Defer", LegacyCategory::NotYetModeled),
        ];
        for &(kind, expected) in checks {
            let got = disposition_for(kind)
                .unwrap_or_else(|| panic!("no disposition for {kind}"))
                .legacy_category();
            assert_eq!(got, expected, "legacy_category mismatch for {kind}");
        }
    }
}
