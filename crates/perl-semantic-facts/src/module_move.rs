//! Pure planning contract for a conventional source-backed module move.
//!
//! This module consumes already-authorized canonical facts.  It does not read a
//! workspace, inspect disk, parse source, or produce protocol edit types.

use crate::{AnchorId, EntityId, FileId, OccurrenceId, OccurrenceKind, SourceGeneration};
use serde::{Deserialize, Serialize};

/// The target form supplied by a move request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleMoveTarget {
    /// A Perl package/module name such as `Old::Name`.
    Package(String),
    /// A workspace-relative source path such as `lib/New/Name.pm`.
    RelativePath(String),
}

/// Canonical source facts admitted by the conventional move profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveSource {
    /// Workspace identity.
    pub workspace: String,
    /// Authorized source-root identity.
    pub root: String,
    /// Canonical source file identity.
    pub file_id: FileId,
    /// Workspace-relative source path.
    pub relative_path: String,
    /// Canonical source URI.
    pub source_uri: String,
    /// Primary source-backed package.
    pub package: String,
    /// Canonical module identity.
    pub module: String,
    /// Current source generation; unknown generations cannot authorize edits.
    pub generation: SourceGeneration,
    /// Whether the source is an editable workspace file.
    pub editable: bool,
    /// Whether this is generated/vendor/staged/external source.
    pub restricted: bool,
    /// Number of primary packages proven in the source file.
    pub primary_package_count: u32,
    /// Whether the canonical occurrence denominator is complete for this profile.
    pub occurrences_complete: bool,
}

/// One canonical semantic occurrence admitted to the move denominator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveOccurrence {
    /// Source file containing the occurrence.
    pub file_id: FileId,
    /// Stable semantic occurrence identity.
    pub occurrence_id: OccurrenceId,
    /// Exact source anchor identity.
    pub anchor_id: AnchorId,
    /// Entity whose identity is embedded in the occurrence.
    pub entity_id: EntityId,
    /// Semantic occurrence kind.
    pub kind: OccurrenceKind,
    /// Exact old source text at the anchor.
    pub old_text: String,
    /// Exact byte range of `old_text`.
    pub start_byte: u32,
    /// Exclusive end byte of `old_text`.
    pub end_byte: u32,
    /// Generation of the file containing this occurrence.
    pub file_generation: SourceGeneration,
    /// True when the occurrence is not fully statically known.
    pub dynamic: bool,
    /// True when the occurrence is not current with the source.
    pub stale: bool,
    /// True when the producer cannot prove this projection is supported.
    pub unsupported: bool,
}

/// A planned exact source edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveEdit {
    /// Source document identity.
    pub file_id: FileId,
    /// Source generation precondition.
    pub generation: SourceGeneration,
    /// Semantic occurrence identity.
    pub occurrence_id: OccurrenceId,
    /// Semantic occurrence kind.
    pub kind: OccurrenceKind,
    /// Entity identity anchor.
    pub entity_id: EntityId,
    /// Exact anchor identity.
    pub anchor_id: AnchorId,
    /// Exact old text precondition.
    pub old_text: String,
    /// Replacement text.
    pub new_text: String,
    /// Inclusive start byte.
    pub start_byte: u32,
    /// Exclusive end byte.
    pub end_byte: u32,
}

/// The non-text resource transition required by the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveResourceTransition {
    /// Existing source path.
    pub source_path: String,
    /// Target source path.
    pub target_path: String,
    /// Existing module identity.
    pub source_module: String,
    /// Target module identity.
    pub target_module: String,
}

/// Why a module move cannot be authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleMoveBlocker {
    /// The source identity is incomplete or contradictory.
    InvalidSource,
    /// The source generation is unknown or an occurrence is from another generation.
    StaleOrUnknownGeneration,
    /// A target package/path is invalid or escapes its root.
    UnsafeTarget,
    /// The target already has a resource.
    TargetCollision,
    /// The source has multiple primary packages.
    AmbiguousSourcePackage,
    /// A relevant occurrence is dynamic.
    DynamicBoundary,
    /// A relevant occurrence is not fully supported.
    UnsupportedProjection,
    /// The source package declaration is not present in the denominator.
    MissingPackageDeclaration,
    /// An occurrence range is not exactly the byte length of its old text.
    InvalidAnchor,
    /// The admitted semantic denominator is incomplete.
    IncompleteOccurrences,
    /// The target does not remain in the same authorized root.
    CrossRoot,
}

/// Whether the pure plan can be materialized as a complete operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleMoveDisposition {
    /// Every required member is present and current.
    Complete,
    /// The operation must be refused with its blockers.
    Blocked,
}

/// Pure, deterministic module-move plan.  It contains no mutable workspace state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMovePlan {
    /// Plan schema version.
    pub schema_version: u16,
    /// Source identity used to build the plan.
    pub source: ModuleMoveSource,
    /// Derived resource transition.
    pub resource: ModuleMoveResourceTransition,
    /// Complete ordered semantic edit set when accepted.
    pub edits: Vec<ModuleMoveEdit>,
    /// Typed refusal reasons, empty for a complete plan.
    pub blockers: Vec<ModuleMoveBlocker>,
    /// Plan disposition.
    pub disposition: ModuleMoveDisposition,
    /// Deterministic identity over the full plan payload.
    pub fingerprint: String,
}

impl ModuleMovePlan {
    /// Build a plan from canonical facts without reading or mutating anything.
    pub fn build(
        source: ModuleMoveSource,
        target: ModuleMoveTarget,
        occurrences: Vec<ModuleMoveOccurrence>,
        target_exists: bool,
    ) -> Self {
        let mut blockers = Vec::new();
        let target_package = match target {
            ModuleMoveTarget::Package(value) => value,
            ModuleMoveTarget::RelativePath(value) => {
                let prefix = format!("{}/", source.root.trim_end_matches('/'));
                value
                    .strip_prefix(&prefix)
                    .and_then(package_from_path)
                    .unwrap_or_default()
            }
        };
        let target_path = module_path(&target_package)
            .map(|path| format!("{}/{path}", source.root.trim_end_matches('/')))
            .unwrap_or_default();
        let expected_source_path = module_path(&source.module)
            .map(|path| format!("{}/{path}", source.root.trim_end_matches('/')));
        if source.workspace.trim().is_empty()
            || source.root.trim().is_empty()
            || source.source_uri.trim().is_empty()
            || source.relative_path.trim().is_empty()
            || !valid_package(&source.package)
            || !valid_package(&source.module)
            || source.package != source.module
            || expected_source_path.as_deref() != Some(source.relative_path.as_str())
            || !source.editable || source.restricted || source.primary_package_count != 1
        {
            blockers.push(if source.primary_package_count != 1 {
                ModuleMoveBlocker::AmbiguousSourcePackage
            } else {
                ModuleMoveBlocker::InvalidSource
            });
        }
        if !source.occurrences_complete {
            blockers.push(ModuleMoveBlocker::IncompleteOccurrences);
        }
        if !source.generation.is_known() || target_package.is_empty() || target_path.is_empty() {
            blockers.push(if target_package.is_empty() {
                ModuleMoveBlocker::UnsafeTarget
            } else {
                ModuleMoveBlocker::StaleOrUnknownGeneration
            });
        }
        if target_exists || target_path == source.relative_path {
            blockers.push(ModuleMoveBlocker::TargetCollision);
        }
        if !target_path.starts_with(&format!("{}/", source.root.trim_end_matches('/'))) {
            blockers.push(ModuleMoveBlocker::CrossRoot);
        }
        let mut edits = Vec::new();
        let mut has_package_declaration = false;
        for occurrence in occurrences {
            if occurrence.file_generation != source.generation {
                blockers.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
            }
            if occurrence.dynamic || occurrence.kind == OccurrenceKind::DynamicBoundary {
                blockers.push(ModuleMoveBlocker::DynamicBoundary);
            }
            if occurrence.unsupported { blockers.push(ModuleMoveBlocker::UnsupportedProjection); }
            if occurrence.kind == OccurrenceKind::Definition
                && occurrence.old_text.trim_start().starts_with("package ")
                && replace_identity(&occurrence.old_text, &source.package, &target_package).is_some()
            {
                has_package_declaration = true;
            }
            let range_len = u64::from(occurrence.end_byte)
                .saturating_sub(u64::from(occurrence.start_byte));
            if occurrence.stale
                || occurrence.old_text.is_empty()
                || occurrence.end_byte <= occurrence.start_byte
                || range_len != occurrence.old_text.len() as u64
            {
                if range_len != occurrence.old_text.len() as u64 {
                    blockers.push(ModuleMoveBlocker::InvalidAnchor);
                }
                blockers.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
            }
            if let Some(new_text) = replace_identity(&occurrence.old_text, &source.package, &target_package) {
                edits.push(ModuleMoveEdit { file_id: occurrence.file_id, generation: occurrence.file_generation,
                    occurrence_id: occurrence.occurrence_id, kind: occurrence.kind, entity_id: occurrence.entity_id,
                    anchor_id: occurrence.anchor_id, old_text: occurrence.old_text, new_text,
                    start_byte: occurrence.start_byte, end_byte: occurrence.end_byte });
            } else {
                blockers.push(ModuleMoveBlocker::UnsupportedProjection);
            }
        }
        if !has_package_declaration {
            blockers.push(ModuleMoveBlocker::MissingPackageDeclaration);
        }
        edits.sort_by_key(|edit| (edit.file_id, edit.start_byte, edit.occurrence_id));
        blockers.sort_unstable_by_key(|blocker| *blocker as u8);
        blockers.dedup();
        let resource = ModuleMoveResourceTransition { source_path: source.relative_path.clone(),
            target_path, source_module: source.module.clone(), target_module: target_package.clone() };
        let disposition = if blockers.is_empty() { ModuleMoveDisposition::Complete } else { ModuleMoveDisposition::Blocked };
        let fingerprint = crate::semantic_identity::SemanticIdentityFingerprint::new("module-move-plan-v1")
            .field("source", &source.source_uri).field("generation", &format!("{:?}", source.generation))
            .field("target", &resource.target_module).field("path", &resource.target_path)
            .field("disposition", &format!("{disposition:?}"))
            .field("edits", &format!("{edits:?}" )).field("blockers", &format!("{blockers:?}" )).finish();
        Self { schema_version: 1, source, resource, edits, blockers, disposition, fingerprint }
    }

    /// A complete plan is the only plan eligible for materialization.
    pub const fn is_complete(&self) -> bool { matches!(self.disposition, ModuleMoveDisposition::Complete) }
}

fn valid_package(value: &str) -> bool {
    !value.is_empty() && value.split("::").all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

fn module_path(package: &str) -> Option<String> { valid_package(package).then(|| format!("{}.pm", package.replace("::", "/"))) }

fn package_from_path(path: &str) -> Option<String> {
    if path.contains("..") || !path.ends_with(".pm") { return None; }
    let stem = path.strip_suffix(".pm")?;
    let package = stem.replace('/', "::");
    valid_package(&package).then_some(package)
}

fn replace_identity(old: &str, source: &str, target: &str) -> Option<String> {
    let start = old.find(source)?;
    let end = start + source.len();
    let before = old.as_bytes().get(start.wrapping_sub(1)).copied();
    let after = old.as_bytes().get(end).copied();
    let boundary = |byte: Option<u8>| byte.is_none_or(|value| !(value.is_ascii_alphanumeric() || value == b'_'));
    (boundary(before) && boundary(after)).then(|| format!("{}{}{}", &old[..start], target, &old[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> ModuleMoveSource { ModuleMoveSource { workspace: "w".into(), root: "lib".into(), file_id: FileId(1), relative_path: "lib/Old/Name.pm".into(), source_uri: "file:///w/lib/Old/Name.pm".into(), package: "Old::Name".into(), module: "Old::Name".into(), generation: SourceGeneration::known("g1"), editable: true, restricted: false, primary_package_count: 1, occurrences_complete: true } }
    fn occurrence(text: &str, kind: OccurrenceKind) -> ModuleMoveOccurrence { ModuleMoveOccurrence { file_id: FileId(1), occurrence_id: OccurrenceId(1), anchor_id: AnchorId(2), entity_id: EntityId(3), kind, old_text: text.into(), start_byte: 0, end_byte: text.len() as u32, file_generation: SourceGeneration::known("g1"), dynamic: false, stale: false, unsupported: false } }

    #[test]
    fn plans_exact_prefix_and_preserves_imported_member() {
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![occurrence("package Old::Name", OccurrenceKind::Definition), occurrence("use Old::Name qw(run)", OccurrenceKind::Import)], false);
        assert!(plan.is_complete());
        assert_eq!(plan.edits[0].new_text, "use New::Name qw(run)");
        assert_eq!(plan.resource.target_path, "lib/New/Name.pm");
    }

    #[test]
    fn blocks_dynamic_and_generation_mismatch() {
        let mut item = occurrence("require Old::Name", OccurrenceKind::Reference);
        item.dynamic = true;
        item.file_generation = SourceGeneration::known("g2");
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![item], false);
        assert!(!plan.is_complete());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::DynamicBoundary));
        assert!(plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
    }

    #[test]
    fn refuses_same_text_in_an_unrelated_projection() {
        let mut item = occurrence("Old::Name", OccurrenceKind::Reference);
        item.unsupported = true;
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![item], false);
        assert!(!plan.is_complete());
        assert!(plan.edits.is_empty() || plan.blockers.contains(&ModuleMoveBlocker::UnsupportedProjection));
    }

    #[test]
    fn refuses_target_traversal_and_collision() {
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::RelativePath("lib/../New.pm".into()), Vec::new(), true);
        assert!(!plan.is_complete());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::UnsafeTarget));
        assert!(plan.blockers.contains(&ModuleMoveBlocker::TargetCollision));
    }

    #[test]
    fn requires_package_declaration_occurrence() {
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![occurrence("use Old::Name", OccurrenceKind::Import)], false);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration));
    }

    #[test]
    fn requires_consistent_package_and_module_identity() {
        let mut input = source();
        input.module = "Other::Name".into();
        let plan = ModuleMovePlan::build(input, ModuleMoveTarget::Package("New::Name".into()), vec![occurrence("package Old::Name", OccurrenceKind::Definition)], false);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidSource));
    }

    #[test]
    fn binds_each_occurrence_to_its_file_generation() {
        let mut item = occurrence("package Old::Name", OccurrenceKind::Definition);
        item.file_generation = SourceGeneration::known("dependent-generation");
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![item], false);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
    }

    #[test]
    fn rejects_ranges_that_do_not_equal_old_text_bytes() {
        let mut item = occurrence("package Old::Name", OccurrenceKind::Definition);
        item.end_byte -= 1;
        let plan = ModuleMovePlan::build(source(), ModuleMoveTarget::Package("New::Name".into()), vec![item], false);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidAnchor));
        assert!(!plan.is_complete());
    }
}
