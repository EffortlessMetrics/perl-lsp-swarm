//! Pure planning contract for a conventional source-backed module move.
//!
//! This module consumes already-authorized canonical facts.  It does not read a
//! workspace, inspect disk, parse source, or produce protocol edit types.
//!
//! Three propositions are kept apart and checked against each other:
//!
//! * **Currentness** is per file.  A cross-file reference lives in its own
//!   document with its own generation, so currentness is proven against an
//!   admitted per-file generation snapshot, never against the moved file's
//!   generation.
//! * **A refused occurrence contributes no edit,** so `edits` never carries a
//!   member the plan also refuses.  A plan blocked for a whole-plan reason may
//!   still list the edits of its acceptable occurrences; those are diagnostic,
//!   and [`ModuleMovePlan::is_complete`] is the only authorization.
//! * **A complete plan is a checked property, not a tag.**  [`ModuleMovePlan`]
//!   is publicly constructible and `Deserialize`, so
//!   [`ModuleMovePlan::is_complete`] re-derives the invariants and the
//!   fingerprint rather than trusting `disposition`.

use crate::{AnchorId, EntityId, FileId, OccurrenceId, OccurrenceKind, SourceGeneration};
use serde::{Deserialize, Serialize};

/// Current plan schema version.  A plan carrying any other version is refused.
pub const MODULE_MOVE_SCHEMA_VERSION: u16 = 1;

/// The target form supplied by a move request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleMoveTarget {
    /// A Perl package/module name such as `Old::Name`.
    Package(String),
    /// A workspace-relative source path such as `lib/New/Name.pm`.
    RelativePath(String),
}

/// The admitted current generation of one file the plan depends on.
///
/// Every occurrence file needs an entry.  Absence is not "current"; it is an
/// explicit missing-evidence state and blocks the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveFileGeneration {
    /// Canonical file identity.
    pub file_id: FileId,
    /// The generation the producer proves is current for that file.
    pub generation: SourceGeneration,
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
    /// True when the producer marks the occurrence not current.
    ///
    /// This is the producer's own claim.  It is an additional refusal input,
    /// never a substitute for the generation comparison.
    pub stale: bool,
    /// True when the producer cannot prove this projection is supported.
    pub unsupported: bool,
}

/// A planned exact source edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMoveEdit {
    /// Source document identity.
    pub file_id: FileId,
    /// Source generation precondition for that document.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModuleMoveBlocker {
    /// The source identity is incomplete or contradictory.
    InvalidSource,
    /// A generation is unknown, or an occurrence is not at its file's current generation.
    StaleOrUnknownGeneration,
    /// No known current generation is admitted for a file the plan depends on,
    /// or the admitted snapshot contradicts itself.
    MissingFileGeneration,
    /// A target package/path is invalid or escapes its root.
    UnsafeTarget,
    /// The target already has a resource.
    TargetCollision,
    /// The source has multiple primary packages, or more than one primary
    /// package declaration was admitted for it.
    AmbiguousSourcePackage,
    /// A relevant occurrence is dynamic.
    DynamicBoundary,
    /// A relevant occurrence is not fully supported.
    UnsupportedProjection,
    /// The moved file's own package declaration is not present in the denominator.
    MissingPackageDeclaration,
    /// An occurrence range is not exactly the byte length of its old text.
    InvalidAnchor,
    /// An anchor contains the source identity at more than one substitution site.
    AmbiguousAnchor,
    /// Two admitted occurrences share an occurrence or anchor identity in one file.
    DuplicateOccurrence,
    /// Two planned edits in one file cover overlapping bytes.
    OverlappingEdits,
    /// The admitted semantic denominator is incomplete.
    IncompleteOccurrences,
    /// The target does not remain in the same authorized root.
    CrossRoot,
}

impl ModuleMoveBlocker {
    /// Stable wire tag, independent of `Debug` formatting.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::InvalidSource => "invalid-source",
            Self::StaleOrUnknownGeneration => "stale-or-unknown-generation",
            Self::MissingFileGeneration => "missing-file-generation",
            Self::UnsafeTarget => "unsafe-target",
            Self::TargetCollision => "target-collision",
            Self::AmbiguousSourcePackage => "ambiguous-source-package",
            Self::DynamicBoundary => "dynamic-boundary",
            Self::UnsupportedProjection => "unsupported-projection",
            Self::MissingPackageDeclaration => "missing-package-declaration",
            Self::InvalidAnchor => "invalid-anchor",
            Self::AmbiguousAnchor => "ambiguous-anchor",
            Self::DuplicateOccurrence => "duplicate-occurrence",
            Self::OverlappingEdits => "overlapping-edits",
            Self::IncompleteOccurrences => "incomplete-occurrences",
            Self::CrossRoot => "cross-root",
        }
    }
}

/// Whether the pure plan can be materialized as a complete operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleMoveDisposition {
    /// Every required member is present and current.
    Complete,
    /// The operation must be refused with its blockers.
    Blocked,
}

impl ModuleMoveDisposition {
    /// Stable wire tag, independent of `Debug` formatting.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

/// Why a plan value cannot be trusted as a checked plan.
///
/// A plan can arrive by deserialization or public field construction, so no
/// caller may read `disposition` as authorization without clearing these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModuleMoveInvalidPlan {
    /// The plan does not carry [`MODULE_MOVE_SCHEMA_VERSION`].
    UnknownSchemaVersion,
    /// `disposition` and `blockers` disagree about refusal.
    DispositionDisagreesWithBlockers,
    /// Blockers are not in canonical sorted, deduplicated order.
    BlockersNotCanonical,
    /// A complete plan carries no edit.
    CompletePlanWithoutEdits,
    /// An edit's range, text, or generation is malformed.
    MalformedEdit,
    /// Edits are not in canonical order.
    EditsNotCanonical,
    /// Two edits in one file share an occurrence or anchor identity.
    DuplicateEdit,
    /// Two edits in one file cover overlapping bytes.
    OverlappingEdits,
    /// The resource transition does not follow from the source identity.
    ResourceTransitionInconsistent,
    /// A complete plan does not rename the moved file's own package declaration.
    MissingPackageDeclaration,
    /// The source identity would not have been eligible to authorize any edit.
    SourceNotEligible,
    /// A complete plan does not record the target resource as proven absent.
    TargetWasNotProvenAbsent,
    /// The retained generation snapshot is missing, non-canonical, self-
    /// contradictory, or disagrees with the source generation.
    GenerationEvidenceUnusable,
    /// An edit is not at the admitted current generation of its own file.
    EditIsNotAtItsFilesCurrentGeneration,
    /// An edit is not the boundary-clean substitution of the source identity by
    /// the target module that it claims to be.
    EditIsNotTheIdentitySubstitution,
    /// The fingerprint does not identify this payload.
    FingerprintMismatch,
}

/// Pure, deterministic module-move plan.  It contains no mutable workspace state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMovePlan {
    /// Plan schema version.
    pub schema_version: u16,
    /// Source identity used to build the plan.
    pub source: ModuleMoveSource,
    /// The admitted current generation of every file the plan depends on.
    ///
    /// Retained so a plan can re-derive its own generation binding: without it
    /// a reconstructed plan has no evidence that a dependent edit was current.
    /// Contradicted files are absent, never guessed.
    pub current_generations: Vec<ModuleMoveFileGeneration>,
    /// Whether the producer proved the target resource did not already exist.
    ///
    /// Retained for the same reason: collision-freedom is part of a complete
    /// plan's contract, so the evidence must survive construction.
    pub target_was_absent: bool,
    /// Derived resource transition.
    pub resource: ModuleMoveResourceTransition,
    /// Ordered semantic edit set.
    ///
    /// A blocked plan may still carry the edits of its individually acceptable
    /// occurrences; those are diagnostic, never authorization. Gate every read
    /// on [`Self::is_complete`] — a non-empty `edits` is not a licence to apply.
    pub edits: Vec<ModuleMoveEdit>,
    /// Typed refusal reasons, empty for a complete plan.
    pub blockers: Vec<ModuleMoveBlocker>,
    /// Plan disposition.  Not authorization on its own — see [`Self::is_complete`].
    pub disposition: ModuleMoveDisposition,
    /// Deterministic identity over every load-bearing field of the plan.
    pub fingerprint: String,
}

impl ModuleMovePlan {
    /// Build a plan from canonical facts without reading or mutating anything.
    ///
    /// `current_generations` admits the generation the producer proves current
    /// for each file the denominator touches, including the moved file itself.
    #[must_use]
    pub fn build(
        source: ModuleMoveSource,
        target: ModuleMoveTarget,
        occurrences: Vec<ModuleMoveOccurrence>,
        current_generations: Vec<ModuleMoveFileGeneration>,
        target_exists: bool,
    ) -> Self {
        let mut blockers = Vec::new();
        let root = source.root.trim_end_matches('/').to_string();

        let generations = CurrentGenerations::admit(current_generations);
        if !generations.contradicted.is_empty() {
            blockers.push(ModuleMoveBlocker::MissingFileGeneration);
        }
        // The source file is classified exactly as any occurrence file is, so
        // one input never yields two different reasons.
        match generations.current(source.file_id) {
            None => blockers.push(ModuleMoveBlocker::MissingFileGeneration),
            Some(current) if !current.is_known() => {
                blockers.push(ModuleMoveBlocker::MissingFileGeneration);
            }
            Some(current) if *current != source.generation => {
                blockers.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
            }
            Some(_) => {}
        }

        let target_package = match target {
            ModuleMoveTarget::Package(value) => value,
            ModuleMoveTarget::RelativePath(value) => value
                .strip_prefix(&format!("{root}/"))
                .and_then(package_from_path)
                .unwrap_or_default(),
        };
        let target_path =
            module_path(&target_package).map(|path| format!("{root}/{path}")).unwrap_or_default();
        let expected_source_path = module_path(&source.module).map(|path| format!("{root}/{path}"));

        if source.workspace.trim().is_empty()
            || root.is_empty()
            || source.source_uri.trim().is_empty()
            || source.relative_path.trim().is_empty()
            || !valid_package(&source.package)
            || !valid_package(&source.module)
            || source.package != source.module
            || expected_source_path.as_deref() != Some(source.relative_path.as_str())
            || !source.editable
            || source.restricted
            || source.primary_package_count != 1
        {
            // Kept as the reported conjunction; `source_is_eligible` re-derives
            // the same requirement at the acceptance boundary.
            blockers.push(if source.primary_package_count != 1 {
                ModuleMoveBlocker::AmbiguousSourcePackage
            } else {
                ModuleMoveBlocker::InvalidSource
            });
        }
        if !source.occurrences_complete {
            blockers.push(ModuleMoveBlocker::IncompleteOccurrences);
        }
        if !source.generation.is_known() {
            blockers.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
        }
        if target_package.is_empty() || target_path.is_empty() {
            blockers.push(ModuleMoveBlocker::UnsafeTarget);
        }
        if target_exists || target_path == source.relative_path {
            blockers.push(ModuleMoveBlocker::TargetCollision);
        }
        if !target_path.starts_with(&format!("{root}/")) {
            blockers.push(ModuleMoveBlocker::CrossRoot);
        }

        let mut edits = Vec::new();
        let mut primary_declarations = 0_u32;
        for occurrence in occurrences {
            // Collected per occurrence so refusal and edit stay mutually
            // exclusive: any refusal at all withdraws this occurrence's edit.
            let mut refusals: Vec<ModuleMoveBlocker> = Vec::new();

            // Currentness is proven per occurrence file, never against the
            // moved file's generation: a cross-file reference has its own.
            match generations.current(occurrence.file_id) {
                None => refusals.push(ModuleMoveBlocker::MissingFileGeneration),
                Some(current) if !current.is_known() => {
                    refusals.push(ModuleMoveBlocker::MissingFileGeneration);
                }
                Some(current) if *current != occurrence.file_generation => {
                    refusals.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
                }
                Some(_) => {}
            }
            if occurrence.stale {
                refusals.push(ModuleMoveBlocker::StaleOrUnknownGeneration);
            }
            if occurrence.dynamic || occurrence.kind == OccurrenceKind::DynamicBoundary {
                refusals.push(ModuleMoveBlocker::DynamicBoundary);
            }
            if occurrence.unsupported {
                refusals.push(ModuleMoveBlocker::UnsupportedProjection);
            }
            if occurrence.old_text.is_empty()
                || occurrence.end_byte <= occurrence.start_byte
                || u64::from(occurrence.end_byte) - u64::from(occurrence.start_byte)
                    != occurrence.old_text.len() as u64
            {
                refusals.push(ModuleMoveBlocker::InvalidAnchor);
            }

            let sites = identity_sites(&occurrence.old_text, &source.package);
            let new_text = match sites.as_slice() {
                [] => {
                    refusals.push(ModuleMoveBlocker::UnsupportedProjection);
                    None
                }
                [start] => {
                    let new_text =
                        substitute(&occurrence.old_text, *start, &source.package, &target_package);
                    // A same-identity target substitutes nothing. The plan is
                    // already blocked as a collision; emitting an unchanged
                    // edit would make `build` produce a plan its own
                    // `validate` rejects.
                    (new_text != occurrence.old_text).then_some(new_text)
                }
                _ => {
                    // A multi-statement anchor has more than one substitution
                    // site; one exact-old-text edit cannot express it.
                    refusals.push(ModuleMoveBlocker::AmbiguousAnchor);
                    None
                }
            };

            // The declaration must be the moved file's own, bound to its file
            // identity — a dependent file's `package Old::Name` is a reference.
            if occurrence.kind == OccurrenceKind::Definition
                && occurrence.file_id == source.file_id
                && declares_identity(&occurrence.old_text, &sites)
            {
                primary_declarations += 1;
            }

            let refused = !refusals.is_empty();
            blockers.append(&mut refusals);
            if let (false, Some(new_text)) = (refused, new_text) {
                edits.push(ModuleMoveEdit {
                    file_id: occurrence.file_id,
                    generation: occurrence.file_generation,
                    occurrence_id: occurrence.occurrence_id,
                    kind: occurrence.kind,
                    entity_id: occurrence.entity_id,
                    anchor_id: occurrence.anchor_id,
                    old_text: occurrence.old_text,
                    new_text,
                    start_byte: occurrence.start_byte,
                    end_byte: occurrence.end_byte,
                });
            }
        }

        match primary_declarations {
            0 => blockers.push(ModuleMoveBlocker::MissingPackageDeclaration),
            1 => {}
            _ => blockers.push(ModuleMoveBlocker::AmbiguousSourcePackage),
        }

        edits.sort_by(edit_order);
        blockers.extend(edit_set_conflicts(&edits));
        blockers.sort_unstable();
        blockers.dedup();

        let resource = ModuleMoveResourceTransition {
            source_path: source.relative_path.clone(),
            target_path,
            source_module: source.module.clone(),
            target_module: target_package,
        };
        let disposition = if blockers.is_empty() {
            ModuleMoveDisposition::Complete
        } else {
            ModuleMoveDisposition::Blocked
        };
        let current_generations = generations.canonical();
        let fingerprint = fingerprint_of(
            MODULE_MOVE_SCHEMA_VERSION,
            &source,
            &current_generations,
            !target_exists,
            &resource,
            &edits,
            &blockers,
            disposition,
        );
        Self {
            schema_version: MODULE_MOVE_SCHEMA_VERSION,
            source,
            current_generations,
            target_was_absent: !target_exists,
            resource,
            edits,
            blockers,
            disposition,
            fingerprint,
        }
    }

    /// Re-derive every invariant this plan asserts about itself.
    ///
    /// The fields are public and the type is `Deserialize`, so a plan value is
    /// untrusted input.  This is the trust boundary.
    ///
    /// # Errors
    ///
    /// Returns the first invariant the plan fails.
    pub fn validate(&self) -> Result<(), ModuleMoveInvalidPlan> {
        if self.schema_version != MODULE_MOVE_SCHEMA_VERSION {
            return Err(ModuleMoveInvalidPlan::UnknownSchemaVersion);
        }
        let claims_complete = matches!(self.disposition, ModuleMoveDisposition::Complete);
        if claims_complete != self.blockers.is_empty() {
            return Err(ModuleMoveInvalidPlan::DispositionDisagreesWithBlockers);
        }
        if !self.blockers.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(ModuleMoveInvalidPlan::BlockersNotCanonical);
        }
        for edit in &self.edits {
            if edit.old_text.is_empty()
                || edit.end_byte <= edit.start_byte
                || u64::from(edit.end_byte) - u64::from(edit.start_byte)
                    != edit.old_text.len() as u64
                || edit.new_text == edit.old_text
                || !edit.generation.is_known()
            {
                return Err(ModuleMoveInvalidPlan::MalformedEdit);
            }
        }
        if !self.edits.windows(2).all(|pair| edit_order(&pair[0], &pair[1]).is_lt()) {
            return Err(ModuleMoveInvalidPlan::EditsNotCanonical);
        }
        if let Some(conflict) = edit_set_conflicts(&self.edits).first() {
            return Err(match conflict {
                ModuleMoveBlocker::OverlappingEdits => ModuleMoveInvalidPlan::OverlappingEdits,
                _ => ModuleMoveInvalidPlan::DuplicateEdit,
            });
        }
        if self.fingerprint
            != fingerprint_of(
                self.schema_version,
                &self.source,
                &self.current_generations,
                self.target_was_absent,
                &self.resource,
                &self.edits,
                &self.blockers,
                self.disposition,
            )
        {
            return Err(ModuleMoveInvalidPlan::FingerprintMismatch);
        }
        if !claims_complete {
            return Ok(());
        }
        if self.edits.is_empty() {
            return Err(ModuleMoveInvalidPlan::CompletePlanWithoutEdits);
        }
        let root = self.source.root.trim_end_matches('/');
        let expected_target = module_path(&self.resource.target_module)
            .map(|path| format!("{root}/{path}"))
            .unwrap_or_default();
        if self.resource.source_path != self.source.relative_path
            || self.resource.source_module != self.source.module
            || self.resource.target_path != expected_target
            || expected_target.is_empty()
            || self.resource.target_path == self.resource.source_path
            || !self.resource.target_path.starts_with(&format!("{root}/"))
        {
            return Err(ModuleMoveInvalidPlan::ResourceTransitionInconsistent);
        }
        if !source_is_eligible(&self.source) {
            return Err(ModuleMoveInvalidPlan::SourceNotEligible);
        }
        if !self.target_was_absent {
            return Err(ModuleMoveInvalidPlan::TargetWasNotProvenAbsent);
        }
        // Re-derive the generation binding the plan claims. Every edit must sit
        // at the admitted current generation of its own file, and the moved
        // file's admitted generation must be the source fact itself.
        let snapshot = CurrentGenerations::admit(self.current_generations.clone());
        if !snapshot.contradicted.is_empty()
            || snapshot.canonical() != self.current_generations
            || snapshot.current(self.source.file_id) != Some(&self.source.generation)
        {
            return Err(ModuleMoveInvalidPlan::GenerationEvidenceUnusable);
        }
        for edit in &self.edits {
            if snapshot.current(edit.file_id) != Some(&edit.generation) {
                return Err(ModuleMoveInvalidPlan::EditIsNotAtItsFilesCurrentGeneration);
            }
        }
        // The fingerprint proves the payload is self-consistent, not that it is
        // legitimate: it is computed through a public API, so anyone who edits a
        // field can recompute it. Each edit must therefore be re-derived as the
        // substitution it claims to be.
        for edit in &self.edits {
            let sites = identity_sites(&edit.old_text, &self.source.package);
            let [site] = sites.as_slice() else {
                return Err(ModuleMoveInvalidPlan::EditIsNotTheIdentitySubstitution);
            };
            if edit.new_text
                != substitute(
                    &edit.old_text,
                    *site,
                    &self.source.package,
                    &self.resource.target_module,
                )
            {
                return Err(ModuleMoveInvalidPlan::EditIsNotTheIdentitySubstitution);
            }
        }
        let declarations = self
            .edits
            .iter()
            .filter(|edit| {
                edit.file_id == self.source.file_id
                    && edit.kind == OccurrenceKind::Definition
                    && declares_identity(
                        &edit.old_text,
                        &identity_sites(&edit.old_text, &self.source.package),
                    )
            })
            .count();
        if declarations != 1 {
            return Err(ModuleMoveInvalidPlan::MissingPackageDeclaration);
        }
        Ok(())
    }

    /// A checked complete plan is the only plan eligible for materialization.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self.disposition, ModuleMoveDisposition::Complete) && self.validate().is_ok()
    }
}

/// The admitted per-file generation snapshot, canonicalized once.
struct CurrentGenerations {
    entries: Vec<ModuleMoveFileGeneration>,
    contradicted: Vec<FileId>,
}

impl CurrentGenerations {
    fn admit(mut entries: Vec<ModuleMoveFileGeneration>) -> Self {
        entries.sort_by(|a, b| {
            (a.file_id, generation_tag(&a.generation))
                .cmp(&(b.file_id, generation_tag(&b.generation)))
        });
        // Two entries for one file that disagree leave *that file* with no
        // current generation to compare against; that is missing evidence, not
        // a tie-break. Scoping is per file, so one contradicted document does
        // not erase the currentness evidence for every other document.
        let contradicted = entries
            .windows(2)
            .filter(|pair| {
                pair[0].file_id == pair[1].file_id && pair[0].generation != pair[1].generation
            })
            .map(|pair| pair[0].file_id)
            .collect();
        entries.dedup_by(|a, b| a.file_id == b.file_id && a.generation == b.generation);
        Self { entries, contradicted }
    }

    fn current(&self, file_id: FileId) -> Option<&SourceGeneration> {
        if self.contradicted.contains(&file_id) {
            return None;
        }
        self.entries.iter().find(|entry| entry.file_id == file_id).map(|entry| &entry.generation)
    }

    /// The canonical snapshot retained on the plan, contradictions dropped so
    /// no contradicted file appears to have a current generation.
    fn canonical(&self) -> Vec<ModuleMoveFileGeneration> {
        self.entries
            .iter()
            .filter(|entry| !self.contradicted.contains(&entry.file_id))
            .cloned()
            .collect()
    }
}

fn edit_order(a: &ModuleMoveEdit, b: &ModuleMoveEdit) -> core::cmp::Ordering {
    (a.file_id, a.start_byte, a.end_byte, a.occurrence_id, a.anchor_id).cmp(&(
        b.file_id,
        b.start_byte,
        b.end_byte,
        b.occurrence_id,
        b.anchor_id,
    ))
}

/// Conflicts that make an edit set unapplicable as one exact-old-text
/// transition.  Expects `edits` in [`edit_order`].
fn edit_set_conflicts(edits: &[ModuleMoveEdit]) -> Vec<ModuleMoveBlocker> {
    let mut conflicts = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        for other in &edits[index + 1..] {
            if other.file_id != edit.file_id {
                continue;
            }
            if other.occurrence_id == edit.occurrence_id || other.anchor_id == edit.anchor_id {
                conflicts.push(ModuleMoveBlocker::DuplicateOccurrence);
            }
            if other.start_byte < edit.end_byte && edit.start_byte < other.end_byte {
                conflicts.push(ModuleMoveBlocker::OverlappingEdits);
            }
        }
    }
    conflicts
}

/// Identity over every load-bearing field, in schema order.
///
/// Fields are mixed individually under stable labels and stable enum tags, so
/// no `Debug` formatting is load-bearing and no two distinct plans collide by
/// omission.
fn fingerprint_of(
    schema_version: u16,
    source: &ModuleMoveSource,
    current_generations: &[ModuleMoveFileGeneration],
    target_was_absent: bool,
    resource: &ModuleMoveResourceTransition,
    edits: &[ModuleMoveEdit],
    blockers: &[ModuleMoveBlocker],
    disposition: ModuleMoveDisposition,
) -> String {
    let mut acc = crate::semantic_identity::SemanticIdentityFingerprint::new("module-move-plan-v1")
        .field("schema-version", &schema_version.to_string())
        .field("workspace", &source.workspace)
        .field("root", &source.root)
        .field("source-file-id", &source.file_id.0.to_string())
        .field("source-relative-path", &source.relative_path)
        .field("source-uri", &source.source_uri)
        .field("source-package", &source.package)
        .field("source-module", &source.module)
        .field("source-generation", &generation_tag(&source.generation))
        .field("source-editable", bool_tag(source.editable))
        .field("source-restricted", bool_tag(source.restricted))
        .field("primary-package-count", &source.primary_package_count.to_string())
        .field("occurrences-complete", bool_tag(source.occurrences_complete))
        .field("target-was-absent", bool_tag(target_was_absent))
        .field("generation-count", &current_generations.len().to_string())
        .field("resource-source-path", &resource.source_path)
        .field("resource-target-path", &resource.target_path)
        .field("resource-source-module", &resource.source_module)
        .field("resource-target-module", &resource.target_module)
        .field("disposition", disposition.tag())
        .field("edit-count", &edits.len().to_string());
    for edit in edits {
        acc = acc
            .field("edit-file-id", &edit.file_id.0.to_string())
            .field("edit-generation", &generation_tag(&edit.generation))
            .field("edit-occurrence-id", &edit.occurrence_id.0.to_string())
            .field("edit-kind", occurrence_kind_tag(edit.kind))
            .field("edit-entity-id", &edit.entity_id.0.to_string())
            .field("edit-anchor-id", &edit.anchor_id.0.to_string())
            .field("edit-old-text", &edit.old_text)
            .field("edit-new-text", &edit.new_text)
            .field("edit-start-byte", &edit.start_byte.to_string())
            .field("edit-end-byte", &edit.end_byte.to_string());
    }
    for entry in current_generations {
        acc = acc
            .field("generation-file-id", &entry.file_id.0.to_string())
            .field("generation-value", &generation_tag(&entry.generation));
    }
    acc = acc.field("blocker-count", &blockers.len().to_string());
    for blocker in blockers {
        acc = acc.field("blocker", blocker.tag());
    }
    acc.finish()
}

const fn bool_tag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn generation_tag(generation: &SourceGeneration) -> String {
    match generation {
        SourceGeneration::Known(value) => format!("known:{value}"),
        SourceGeneration::Unknown => "unknown".to_string(),
    }
}

const fn occurrence_kind_tag(kind: OccurrenceKind) -> &'static str {
    match kind {
        OccurrenceKind::Definition => "definition",
        OccurrenceKind::Reference => "reference",
        OccurrenceKind::Read => "read",
        OccurrenceKind::Write => "write",
        OccurrenceKind::Call => "call",
        OccurrenceKind::MethodCall => "method-call",
        OccurrenceKind::StaticMethodCall => "static-method-call",
        OccurrenceKind::CoderefReference => "coderef-reference",
        OccurrenceKind::TypeglobReference => "typeglob-reference",
        OccurrenceKind::Import => "import",
        OccurrenceKind::Export => "export",
        OccurrenceKind::Inheritance => "inheritance",
        OccurrenceKind::RoleComposition => "role-composition",
        OccurrenceKind::GeneratedUse => "generated-use",
        OccurrenceKind::DynamicBoundary => "dynamic-boundary",
    }
}

/// A Perl package name this profile will write into source.
///
/// No segment may begin with a digit: `package 123` and `package Foo::1` are
/// not valid Perl, so admitting them would plan an edit that breaks the module.
fn valid_package(value: &str) -> bool {
    !value.is_empty()
        && value.split("::").all(|part| {
            part.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

/// The byte offset of the package-name slot in a `package` declaration.
///
/// Perl separates the keyword from the name with any whitespace, so a tab or
/// newline declares a package exactly as a space does; `packageFoo` does not.
/// Returning the slot rather than a boolean is what makes
/// `package Other; Old::Name` refusable: the identity must be the name this
/// declaration introduces, not merely present somewhere in the anchor.
fn declaration_name_offset(text: &str) -> Option<usize> {
    let leading = text.len() - text.trim_start().len();
    let rest = text.trim_start().strip_prefix("package")?;
    if !rest.starts_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    let gap = rest.len() - rest.trim_start().len();
    Some(leading + "package".len() + gap)
}

/// Whether this anchor declares `identity` as its own package name, with that
/// declaration site as the anchor's single substitution site.
fn declares_identity(text: &str, sites: &[usize]) -> bool {
    declaration_name_offset(text).is_some_and(|offset| sites == [offset])
}

/// The source-identity conjunction this profile requires.
///
/// Shared by `build` and `validate` so construction and acceptance cannot
/// drift about which sources may authorize an edit.
fn source_is_eligible(source: &ModuleMoveSource) -> bool {
    let root = source.root.trim_end_matches('/');
    let expected_path = module_path(&source.module).map(|path| format!("{root}/{path}"));
    !source.workspace.trim().is_empty()
        && !root.is_empty()
        && !source.source_uri.trim().is_empty()
        && !source.relative_path.trim().is_empty()
        && valid_package(&source.package)
        && valid_package(&source.module)
        && source.package == source.module
        && expected_path.as_deref() == Some(source.relative_path.as_str())
        && source.editable
        && !source.restricted
        && source.primary_package_count == 1
        && source.occurrences_complete
        && source.generation.is_known()
}

fn module_path(package: &str) -> Option<String> {
    valid_package(package).then(|| format!("{}.pm", package.replace("::", "/")))
}

fn package_from_path(path: &str) -> Option<String> {
    if path.contains("..") || !path.ends_with(".pm") {
        return None;
    }
    let stem = path.strip_suffix(".pm")?;
    let package = stem.replace('/', "::");
    valid_package(&package).then_some(package)
}

/// Every identifier-boundary-clean start offset of `identity` within `text`.
fn identity_sites(text: &str, identity: &str) -> Vec<usize> {
    if identity.is_empty() {
        return Vec::new();
    }
    let boundary = |byte: Option<u8>| {
        byte.is_none_or(|value| !(value.is_ascii_alphanumeric() || value == b'_'))
    };
    text.match_indices(identity)
        .filter(|(start, _)| {
            let end = start + identity.len();
            // A `::`-separated identity must not be a suffix or prefix of a
            // longer package path either.
            // Both `::` and Perl's legacy `'` separator extend a package
            // identity, so a match adjacent to either is part of a longer name.
            boundary(start.checked_sub(1).and_then(|index| text.as_bytes().get(index).copied()))
                && !text[..*start].ends_with([':', '\''])
                && boundary(text.as_bytes().get(end).copied())
                && !text[end..].starts_with([':', '\''])
        })
        .map(|(start, _)| start)
        .collect()
}

fn substitute(text: &str, start: usize, identity: &str, replacement: &str) -> String {
    let end = start + identity.len();
    format!("{}{replacement}{}", &text[..start], &text[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE_FILE: FileId = FileId(1);
    const DEPENDENT_FILE: FileId = FileId(2);

    fn source() -> ModuleMoveSource {
        ModuleMoveSource {
            workspace: "w".into(),
            root: "lib".into(),
            file_id: SOURCE_FILE,
            relative_path: "lib/Old/Name.pm".into(),
            source_uri: "file:///w/lib/Old/Name.pm".into(),
            package: "Old::Name".into(),
            module: "Old::Name".into(),
            generation: SourceGeneration::known("g1"),
            editable: true,
            restricted: false,
            primary_package_count: 1,
            occurrences_complete: true,
        }
    }

    fn generation(file_id: FileId, value: &str) -> ModuleMoveFileGeneration {
        ModuleMoveFileGeneration { file_id, generation: SourceGeneration::known(value) }
    }

    /// The moved file at `g1` alone.
    fn source_only_snapshot() -> Vec<ModuleMoveFileGeneration> {
        vec![generation(SOURCE_FILE, "g1")]
    }

    /// The moved file at `g1` and a dependent file at its own `g2`.
    fn cross_file_snapshot() -> Vec<ModuleMoveFileGeneration> {
        vec![generation(SOURCE_FILE, "g1"), generation(DEPENDENT_FILE, "g2")]
    }

    fn occurrence(text: &str, kind: OccurrenceKind) -> ModuleMoveOccurrence {
        ModuleMoveOccurrence {
            file_id: SOURCE_FILE,
            occurrence_id: OccurrenceId(1),
            anchor_id: AnchorId(2),
            entity_id: EntityId(3),
            kind,
            old_text: text.into(),
            start_byte: 0,
            end_byte: text.len() as u32,
            file_generation: SourceGeneration::known("g1"),
            dynamic: false,
            stale: false,
            unsupported: false,
        }
    }

    /// The moved file's own `package Old::Name`, which every complete plan needs.
    fn declaration() -> ModuleMoveOccurrence {
        occurrence("package Old::Name", OccurrenceKind::Definition)
    }

    /// An occurrence in a different file, at that file's own generation.
    fn dependent(text: &str, kind: OccurrenceKind, ids: u64) -> ModuleMoveOccurrence {
        ModuleMoveOccurrence {
            file_id: DEPENDENT_FILE,
            occurrence_id: OccurrenceId(ids),
            anchor_id: AnchorId(ids + 100),
            entity_id: EntityId(3),
            file_generation: SourceGeneration::known("g2"),
            start_byte: 10,
            end_byte: 10 + text.len() as u32,
            ..occurrence(text, kind)
        }
    }

    fn plan(
        occurrences: Vec<ModuleMoveOccurrence>,
        generations: Vec<ModuleMoveFileGeneration>,
    ) -> ModuleMovePlan {
        ModuleMovePlan::build(
            source(),
            ModuleMoveTarget::Package("New::Name".into()),
            occurrences,
            generations,
            false,
        )
    }

    // --- positive path -----------------------------------------------------

    #[test]
    fn plans_the_declaration_and_preserves_an_imported_member() {
        let plan = plan(
            vec![declaration(), occurrence_at("use Old::Name qw(run)", 40, 4)],
            source_only_snapshot(),
        );
        assert!(plan.is_complete(), "{:?}", plan.blockers);
        assert!(plan.edits.iter().any(|edit| edit.new_text == "package New::Name"));
        assert!(plan.edits.iter().any(|edit| edit.new_text == "use New::Name qw(run)"));
        assert_eq!(plan.resource.target_path, "lib/New/Name.pm");
    }

    /// The primary multi-file case: a reference in another file, current at
    /// *that* file's generation, must plan rather than read as stale.
    #[test]
    fn a_dependent_file_reference_at_its_own_generation_is_current() {
        let plan = plan(
            vec![declaration(), dependent("use Old::Name", OccurrenceKind::Import, 4)],
            cross_file_snapshot(),
        );
        assert!(plan.is_complete(), "{:?}", plan.blockers);
        assert_eq!(plan.edits.len(), 2);
        let dependent_edit = plan
            .edits
            .iter()
            .find(|edit| edit.file_id == DEPENDENT_FILE)
            .map(|edit| (edit.new_text.as_str(), edit.generation.clone()));
        assert_eq!(dependent_edit, Some(("use New::Name", SourceGeneration::known("g2"))));
    }

    // --- per-file currentness (thread: track currentness per occurrence file)

    /// Mutation: comparing the dependent occurrence to the *moved file's*
    /// generation is exactly the defect this test exists to catch.
    #[test]
    fn a_dependent_occurrence_off_its_own_files_current_generation_is_refused() {
        let mut item = dependent("use Old::Name", OccurrenceKind::Import, 4);
        item.file_generation = SourceGeneration::known("g2-held");
        let plan = plan(vec![declaration(), item], cross_file_snapshot());
        assert!(!plan.is_complete());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
        assert_eq!(plan.edits.len(), 1, "the refused occurrence contributes no edit");
    }

    #[test]
    fn an_occurrence_file_with_no_admitted_generation_is_missing_evidence() {
        let plan = plan(
            vec![declaration(), dependent("use Old::Name", OccurrenceKind::Import, 4)],
            source_only_snapshot(),
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingFileGeneration));
        assert!(!plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
    }

    #[test]
    fn an_unknown_admitted_generation_is_not_a_current_generation() {
        let snapshot = vec![
            generation(SOURCE_FILE, "g1"),
            ModuleMoveFileGeneration {
                file_id: DEPENDENT_FILE,
                generation: SourceGeneration::Unknown,
            },
        ];
        let mut item = dependent("use Old::Name", OccurrenceKind::Import, 4);
        item.file_generation = SourceGeneration::Unknown;
        let plan = plan(vec![declaration(), item], snapshot);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingFileGeneration));
    }

    #[test]
    fn a_snapshot_that_contradicts_itself_leaves_no_current_generation() {
        let snapshot = vec![generation(SOURCE_FILE, "g1"), generation(SOURCE_FILE, "g9")];
        let plan = plan(vec![declaration()], snapshot);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingFileGeneration));
        assert!(!plan.is_complete());
    }

    /// A contradictory snapshot blocks the plan, but it must also withhold the
    /// edit: no entry in a self-contradictory snapshot is a current generation,
    /// so no occurrence in that file may be treated as current.
    #[test]
    fn a_contradicted_file_authorizes_no_edit_even_at_a_listed_generation() {
        let snapshot = vec![
            generation(SOURCE_FILE, "g1"),
            generation(DEPENDENT_FILE, "g2"),
            generation(DEPENDENT_FILE, "g9"),
        ];
        let plan = plan(
            vec![declaration(), dependent("use Old::Name", OccurrenceKind::Import, 4)],
            snapshot,
        );
        assert!(!plan.is_complete());
        assert!(
            plan.edits.iter().all(|edit| edit.file_id != DEPENDENT_FILE),
            "the contradicted file authorized an edit: {:?}",
            plan.edits
        );
        // Per-file scoping: one contradicted document does not erase the
        // currentness evidence for the source file.
        assert!(plan.edits.iter().any(|edit| edit.file_id == SOURCE_FILE));
    }

    #[test]
    fn the_source_files_admitted_generation_must_agree_with_the_source_fact() {
        let plan = plan(vec![declaration()], vec![generation(SOURCE_FILE, "g9")]);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
    }

    /// The producer's own `stale` flag is an additional refusal input, not a
    /// substitute for the generation comparison.
    #[test]
    fn a_producer_stale_flag_still_refuses_a_generation_current_occurrence() {
        let mut item = declaration();
        item.stale = true;
        let plan = plan(vec![item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::StaleOrUnknownGeneration));
        assert!(plan.edits.is_empty());
    }

    // --- primary declaration identity (thread: require the source declaration)

    #[test]
    fn a_declaration_in_a_dependent_file_does_not_satisfy_the_source_declaration() {
        let mut item = dependent("package Old::Name", OccurrenceKind::Definition, 4);
        item.old_text = "package Old::Name".into();
        item.end_byte = item.start_byte + item.old_text.len() as u32;
        let plan = plan(vec![item], cross_file_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration));
        assert!(!plan.is_complete());
    }

    #[test]
    fn an_import_only_denominator_has_no_package_declaration() {
        let plan =
            plan(vec![occurrence("use Old::Name", OccurrenceKind::Import)], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration));
    }

    #[test]
    fn an_empty_denominator_cannot_rename_the_moved_file() {
        let plan = plan(Vec::new(), source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration));
        assert!(!plan.is_complete());
    }

    #[test]
    fn two_primary_declarations_in_the_source_file_are_ambiguous() {
        let second = ModuleMoveOccurrence {
            occurrence_id: OccurrenceId(7),
            anchor_id: AnchorId(8),
            start_byte: 60,
            end_byte: 60 + "package Old::Name".len() as u32,
            ..declaration()
        };
        let plan = plan(vec![declaration(), second], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::AmbiguousSourcePackage));
    }

    // --- refusal/edit consistency (thread: unsupported occurrence still edits)

    #[test]
    fn an_unsupported_occurrence_contributes_no_edit() {
        let mut item = occurrence_at("Old::Name", 40, 4);
        item.unsupported = true;
        let plan = plan(vec![declaration(), item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::UnsupportedProjection));
        assert_eq!(plan.edits.len(), 1, "only the declaration survives");
        assert!(plan.edits.iter().all(|edit| edit.occurrence_id != OccurrenceId(4)));
    }

    #[test]
    fn a_dynamic_occurrence_contributes_no_edit() {
        let mut item = occurrence_at("require Old::Name", 40, 4);
        item.dynamic = true;
        let plan = plan(vec![declaration(), item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::DynamicBoundary));
        assert_eq!(plan.edits.len(), 1);
    }

    #[test]
    fn a_dynamic_boundary_kind_is_refused_without_the_flag() {
        let item = occurrence_at("Old::Name", 40, 4);
        let plan = plan(
            vec![
                declaration(),
                ModuleMoveOccurrence { kind: OccurrenceKind::DynamicBoundary, ..item },
            ],
            source_only_snapshot(),
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::DynamicBoundary));
        assert_eq!(plan.edits.len(), 1);
    }

    /// Every blocked plan must be internally consistent: no plan carries both
    /// a refusal and an edit for the same occurrence.
    #[test]
    fn no_refused_occurrence_ever_appears_in_the_edit_set() {
        for mutate in [
            (|item: &mut ModuleMoveOccurrence| item.unsupported = true) as fn(&mut _),
            |item: &mut ModuleMoveOccurrence| item.dynamic = true,
            |item: &mut ModuleMoveOccurrence| item.stale = true,
            |item: &mut ModuleMoveOccurrence| item.end_byte -= 1,
            |item: &mut ModuleMoveOccurrence| {
                item.file_generation = SourceGeneration::known("elsewhere");
            },
        ] {
            let mut item = occurrence_at("use Old::Name", 40, 4);
            mutate(&mut item);
            let plan = plan(vec![declaration(), item], source_only_snapshot());
            assert!(!plan.is_complete());
            assert!(
                plan.edits.iter().all(|edit| edit.occurrence_id != OccurrenceId(4)),
                "a refused occurrence produced an edit: {:?}",
                plan.blockers
            );
        }
    }

    // --- anchor geometry (thread: verify byte range covers old_text)

    #[test]
    fn a_range_that_does_not_equal_the_old_text_bytes_is_refused() {
        let mut item = declaration();
        item.end_byte -= 1;
        let plan = plan(vec![item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidAnchor));
        assert!(!plan.is_complete());
    }

    #[test]
    fn an_inverted_or_empty_range_is_refused() {
        for (start, end) in [(5_u32, 5_u32), (9, 4)] {
            let mut item = declaration();
            item.start_byte = start;
            item.end_byte = end;
            let plan = plan(vec![item], source_only_snapshot());
            assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidAnchor));
        }
    }

    // --- multi-match anchors (thread: replace_identity replaces first only)

    #[test]
    fn an_anchor_with_two_substitution_sites_is_refused_rather_than_half_edited() {
        let item = occurrence_at("package Old::Name; use Old::Name;", 40, 4);
        let plan = plan(vec![declaration(), item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::AmbiguousAnchor));
        assert!(
            plan.edits.iter().all(|edit| !edit.new_text.contains("Old::Name")),
            "no edit may leave the old identity behind"
        );
    }

    /// The declaration counter must not accept a multi-site anchor either.
    #[test]
    fn a_multi_site_declaration_anchor_does_not_satisfy_the_declaration_requirement() {
        let mut item = declaration();
        item.old_text = "package Old::Name; use Old::Name;".into();
        item.end_byte = item.old_text.len() as u32;
        let plan = plan(vec![item], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration));
    }

    #[test]
    fn a_longer_package_path_is_not_the_source_identity() {
        for text in ["use Old::Name::Deep", "use Prefix::Old::Name", "use Old::NameExtra"] {
            let item = occurrence_at(text, 40, 4);
            let plan = plan(vec![declaration(), item], source_only_snapshot());
            assert!(
                plan.blockers.contains(&ModuleMoveBlocker::UnsupportedProjection),
                "{text} was treated as the source identity"
            );
        }
    }

    // --- duplicate and overlapping edits (thread: sorting is not validation)

    #[test]
    fn two_occurrences_sharing_an_identity_in_one_file_are_refused() {
        let first = occurrence_at("use Old::Name", 40, 4);
        let second = ModuleMoveOccurrence { start_byte: 80, end_byte: 93, ..first.clone() };
        let plan = plan(vec![declaration(), first, second], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::DuplicateOccurrence));
        assert!(!plan.is_complete());
    }

    #[test]
    fn partially_overlapping_edits_in_one_file_are_refused() {
        let first = occurrence_at("use Old::Name", 40, 4);
        let second = ModuleMoveOccurrence {
            occurrence_id: OccurrenceId(5),
            anchor_id: AnchorId(6),
            start_byte: 45,
            end_byte: 58,
            ..first.clone()
        };
        let plan = plan(vec![declaration(), first, second], source_only_snapshot());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::OverlappingEdits));
        assert!(!plan.is_complete());
    }

    /// The same byte range in *different* files is not a conflict.
    #[test]
    fn identical_ranges_in_different_files_do_not_conflict() {
        let mut item = dependent("use Old::Name", OccurrenceKind::Import, 4);
        item.start_byte = 0;
        item.end_byte = "use Old::Name".len() as u32;
        let plan = plan(vec![declaration(), item], cross_file_snapshot());
        assert!(plan.is_complete(), "{:?}", plan.blockers);
    }

    // --- source and target identity ---------------------------------------

    #[test]
    fn a_package_that_disagrees_with_its_module_is_an_invalid_source() {
        let mut input = source();
        input.module = "Other::Name".into();
        let plan = ModuleMovePlan::build(
            input,
            ModuleMoveTarget::Package("New::Name".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidSource));
        assert!(!plan.is_complete());
    }

    #[test]
    fn a_path_that_disagrees_with_the_module_is_an_invalid_source() {
        let mut input = source();
        input.relative_path = "lib/Somewhere/Else.pm".into();
        let plan = ModuleMovePlan::build(
            input,
            ModuleMoveTarget::Package("New::Name".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidSource));
    }

    #[test]
    fn restricted_or_uneditable_source_cannot_authorize_edits() {
        for mutate in [
            (|input: &mut ModuleMoveSource| input.editable = false) as fn(&mut _),
            |input: &mut ModuleMoveSource| input.restricted = true,
        ] {
            let mut input = source();
            mutate(&mut input);
            let plan = ModuleMovePlan::build(
                input,
                ModuleMoveTarget::Package("New::Name".into()),
                vec![declaration()],
                source_only_snapshot(),
                false,
            );
            assert!(plan.blockers.contains(&ModuleMoveBlocker::InvalidSource));
        }
    }

    #[test]
    fn an_incomplete_denominator_blocks_even_with_a_well_formed_edit_set() {
        let mut input = source();
        input.occurrences_complete = false;
        let plan = ModuleMovePlan::build(
            input,
            ModuleMoveTarget::Package("New::Name".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::IncompleteOccurrences));
        assert!(!plan.is_complete());
    }

    #[test]
    fn refuses_target_traversal_and_collision() {
        let plan = ModuleMovePlan::build(
            source(),
            ModuleMoveTarget::RelativePath("lib/../New.pm".into()),
            vec![declaration()],
            source_only_snapshot(),
            true,
        );
        assert!(!plan.is_complete());
        assert!(plan.blockers.contains(&ModuleMoveBlocker::UnsafeTarget));
        assert!(plan.blockers.contains(&ModuleMoveBlocker::TargetCollision));
    }

    #[test]
    fn a_relative_path_target_inside_the_root_derives_its_package() {
        let plan = ModuleMovePlan::build(
            source(),
            ModuleMoveTarget::RelativePath("lib/New/Name.pm".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.is_complete(), "{:?}", plan.blockers);
        assert_eq!(plan.resource.target_module, "New::Name");
    }

    #[test]
    fn a_target_identical_to_the_source_is_a_collision() {
        let plan = ModuleMovePlan::build(
            source(),
            ModuleMoveTarget::Package("Old::Name".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::TargetCollision));
    }

    // --- checked plan acceptance (thread: forged serialized plan) ----------

    #[test]
    fn a_built_complete_plan_validates() {
        let plan = plan(vec![declaration()], source_only_snapshot());
        assert_eq!(plan.validate(), Ok(()));
        assert!(plan.is_complete());
    }

    #[test]
    fn a_built_blocked_plan_also_validates_as_an_honest_refusal() {
        let plan = plan(Vec::new(), source_only_snapshot());
        assert_eq!(plan.validate(), Ok(()));
        assert!(!plan.is_complete());
    }

    #[test]
    fn a_plan_forged_complete_through_the_public_wire_is_not_accepted()
    -> Result<(), serde_json::Error> {
        let mut forged = plan(Vec::new(), source_only_snapshot());
        forged.disposition = ModuleMoveDisposition::Complete;
        let wire = serde_json::to_string(&forged)?;
        let received: ModuleMovePlan = serde_json::from_str(&wire)?;
        assert_eq!(
            received.validate(),
            Err(ModuleMoveInvalidPlan::DispositionDisagreesWithBlockers)
        );
        assert!(!received.is_complete(), "a forged tag authorized materialization");
        Ok(())
    }

    #[test]
    fn a_forged_plan_that_also_clears_its_blockers_fails_on_its_fingerprint() {
        let mut forged = plan(Vec::new(), source_only_snapshot());
        forged.disposition = ModuleMoveDisposition::Complete;
        forged.blockers.clear();
        forged.edits = plan(vec![declaration()], source_only_snapshot()).edits;
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::FingerprintMismatch));
        assert!(!forged.is_complete());
    }

    #[test]
    fn an_unknown_schema_version_is_refused_before_anything_else() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.schema_version = 0;
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::UnknownSchemaVersion));
        assert!(!forged.is_complete());
    }

    #[test]
    fn a_complete_plan_without_edits_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits.clear();
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::CompletePlanWithoutEdits));
    }

    #[test]
    fn a_complete_plan_whose_edits_omit_the_declaration_is_refused() {
        let mut forged = plan(
            vec![declaration(), occurrence_at("use Old::Name", 40, 4)],
            source_only_snapshot(),
        );
        forged.edits.retain(|edit| edit.occurrence_id == OccurrenceId(4));
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::MissingPackageDeclaration));
    }

    #[test]
    fn a_malformed_edit_range_is_refused_even_with_a_matching_fingerprint() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].end_byte -= 1;
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::MalformedEdit));
    }

    #[test]
    fn a_no_op_edit_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].new_text = forged.edits[0].old_text.clone();
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::MalformedEdit));
    }

    #[test]
    fn an_unknown_edit_generation_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].generation = SourceGeneration::Unknown;
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::MalformedEdit));
    }

    #[test]
    fn an_out_of_order_edit_set_is_refused() {
        let mut forged = plan(
            vec![declaration(), occurrence_at("use Old::Name", 40, 4)],
            source_only_snapshot(),
        );
        forged.edits.reverse();
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::EditsNotCanonical));
    }

    #[test]
    fn a_forged_overlapping_edit_set_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        let mut clash = forged.edits[0].clone();
        clash.occurrence_id = OccurrenceId(9);
        clash.anchor_id = AnchorId(9);
        clash.start_byte += 1;
        clash.end_byte += 1;
        clash.old_text = "ackage Old::Name;".into();
        clash.new_text = "ackage New::Name;".into();
        forged.edits.push(clash);
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::OverlappingEdits));
    }

    #[test]
    fn uncanonical_blockers_are_refused() {
        let mut forged = plan(Vec::new(), source_only_snapshot());
        forged.blockers =
            vec![ModuleMoveBlocker::MissingPackageDeclaration, ModuleMoveBlocker::InvalidSource];
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::BlockersNotCanonical));
    }

    #[test]
    fn a_resource_transition_that_does_not_follow_from_the_source_is_refused() {
        for mutate in [
            (|plan: &mut ModuleMovePlan| plan.resource.target_path = "lib/Other.pm".into())
                as fn(&mut _),
            |plan: &mut ModuleMovePlan| plan.resource.source_path = "lib/Other.pm".into(),
            |plan: &mut ModuleMovePlan| plan.resource.source_module = "Other".into(),
            |plan: &mut ModuleMovePlan| plan.resource.target_module = "..".into(),
        ] {
            let mut forged = plan(vec![declaration()], source_only_snapshot());
            mutate(&mut forged);
            forged.fingerprint = rebuild_fingerprint(&forged);
            assert_eq!(
                forged.validate(),
                Err(ModuleMoveInvalidPlan::ResourceTransitionInconsistent)
            );
        }
    }

    // --- the fingerprint is not authorization (Devin review) ------------------

    /// The attack the fingerprint cannot stop: change `new_text` to anything,
    /// recompute the fingerprint through the public API, and ask for
    /// materialization. `validate()` must re-derive the substitution.
    #[test]
    fn an_unrelated_replacement_with_a_recomputed_fingerprint_is_refused() {
        for forged_text in ["package Malicious::Name", "system(\"rm -rf /\")", "package New::Nam"] {
            let mut forged = plan(vec![declaration()], source_only_snapshot());
            forged.edits[0].new_text = forged_text.into();
            forged.fingerprint = rebuild_fingerprint(&forged);
            assert_eq!(
                forged.validate(),
                Err(ModuleMoveInvalidPlan::EditIsNotTheIdentitySubstitution),
                "{forged_text} was accepted"
            );
            assert!(!forged.is_complete());
        }
    }

    /// An edit whose `old_text` no longer contains the source identity cannot
    /// be the substitution it claims, however well-formed it looks.
    #[test]
    fn an_edit_whose_old_text_lost_the_identity_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].old_text = "package Unrelated".into();
        forged.edits[0].new_text = "package Different".into();
        forged.edits[0].end_byte = forged.edits[0].start_byte + "package Unrelated".len() as u32;
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::EditIsNotTheIdentitySubstitution));
    }

    /// Source eligibility is re-derived at acceptance, not inherited from the
    /// build that produced the plan.
    #[test]
    fn an_ineligible_source_with_a_recomputed_fingerprint_is_refused() {
        let mutations: Vec<(&str, fn(&mut ModuleMovePlan))> = vec![
            ("editable", |plan| plan.source.editable = false),
            ("restricted", |plan| plan.source.restricted = true),
            ("primary package count", |plan| plan.source.primary_package_count = 2),
            ("denominator completeness", |plan| plan.source.occurrences_complete = false),
            ("generation", |plan| plan.source.generation = SourceGeneration::Unknown),
            ("workspace", |plan| plan.source.workspace = "  ".into()),
        ];
        for (label, mutate) in mutations {
            let mut forged = plan(vec![declaration()], source_only_snapshot());
            mutate(&mut forged);
            forged.fingerprint = rebuild_fingerprint(&forged);
            assert_eq!(
                forged.validate(),
                Err(ModuleMoveInvalidPlan::SourceNotEligible),
                "{label} survived acceptance"
            );
        }
    }

    /// A blocked plan may list the edits of its acceptable occurrences. That is
    /// diagnostic; it must never read as authorization.
    #[test]
    fn a_blocked_plan_with_edits_is_still_not_authorization() {
        let mut item = occurrence_at("use Old::Name", 40, 4);
        item.unsupported = true;
        let plan = plan(vec![declaration(), item], source_only_snapshot());
        assert!(!plan.edits.is_empty(), "the acceptable occurrence is still listed");
        assert!(!plan.is_complete(), "a non-empty edit set authorized a blocked plan");
    }

    // --- the plan re-derives its own generation binding (Devin review) --------

    /// The forgery the earlier `is_known()` check could not stop: move an edit
    /// to a fabricated generation and recompute the fingerprint.
    #[test]
    fn a_source_edit_off_the_source_generation_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].generation = SourceGeneration::known("fabricated");
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(
            forged.validate(),
            Err(ModuleMoveInvalidPlan::EditIsNotAtItsFilesCurrentGeneration)
        );
        assert!(!forged.is_complete());
    }

    #[test]
    fn a_dependent_edit_off_its_files_admitted_generation_is_refused() {
        let mut forged = plan(
            vec![declaration(), dependent("use Old::Name", OccurrenceKind::Import, 4)],
            cross_file_snapshot(),
        );
        assert!(forged.is_complete(), "{:?}", forged.blockers);
        let index =
            forged.edits.iter().position(|edit| edit.file_id == DEPENDENT_FILE).unwrap_or_default();
        forged.edits[index].generation = SourceGeneration::known("g9");
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(
            forged.validate(),
            Err(ModuleMoveInvalidPlan::EditIsNotAtItsFilesCurrentGeneration)
        );
    }

    /// Rewriting the retained snapshot to match a forged edit does not help:
    /// the snapshot must still agree with the source generation.
    #[test]
    fn a_snapshot_rewritten_to_excuse_a_forged_edit_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.edits[0].generation = SourceGeneration::known("fabricated");
        forged.current_generations = vec![generation(SOURCE_FILE, "fabricated")];
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::GenerationEvidenceUnusable));
    }

    #[test]
    fn a_stripped_or_contradictory_snapshot_is_unusable() {
        for mutate in [
            (|plan: &mut ModuleMovePlan| plan.current_generations.clear()) as fn(&mut _),
            |plan: &mut ModuleMovePlan| {
                plan.current_generations.push(generation(SOURCE_FILE, "g9"));
            },
        ] {
            let mut forged = plan(vec![declaration()], source_only_snapshot());
            mutate(&mut forged);
            forged.fingerprint = rebuild_fingerprint(&forged);
            assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::GenerationEvidenceUnusable));
        }
    }

    #[test]
    fn a_complete_plan_must_record_its_target_as_proven_absent() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        forged.target_was_absent = false;
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::TargetWasNotProvenAbsent));
        assert!(!forged.is_complete());
    }

    /// An unknown admitted generation for the moved file is missing evidence,
    /// and must be reported the same way it is for any occurrence file.
    #[test]
    fn an_unknown_source_generation_is_missing_evidence_not_staleness() {
        let mut input = source();
        input.generation = SourceGeneration::Unknown;
        // Deliberately no occurrences: with an empty denominator only the
        // source-file classification can raise MissingFileGeneration, so the
        // occurrence loop cannot supply it and mask a regression here.
        let plan = ModuleMovePlan::build(
            input,
            ModuleMoveTarget::Package("New::Name".into()),
            Vec::new(),
            vec![ModuleMoveFileGeneration {
                file_id: SOURCE_FILE,
                generation: SourceGeneration::Unknown,
            }],
            false,
        );
        assert!(
            plan.blockers.contains(&ModuleMoveBlocker::MissingFileGeneration),
            "an unknown source generation was not reported as missing evidence: {:?}",
            plan.blockers
        );
    }

    /// One contradicted document must not erase currentness evidence for the
    /// documents that are not contradicted.
    #[test]
    fn a_contradiction_is_scoped_to_the_file_it_contradicts() {
        let snapshot = vec![
            generation(SOURCE_FILE, "g1"),
            generation(DEPENDENT_FILE, "g2"),
            generation(DEPENDENT_FILE, "g9"),
        ];
        let plan = plan(vec![declaration()], snapshot);
        assert!(plan.blockers.contains(&ModuleMoveBlocker::MissingFileGeneration));
        assert!(
            plan.edits.iter().any(|edit| edit.file_id == SOURCE_FILE),
            "the source file lost its evidence to an unrelated contradiction"
        );
        assert!(
            plan.current_generations.iter().all(|entry| entry.file_id != DEPENDENT_FILE),
            "a contradicted file was retained as if it had a current generation"
        );
    }

    // --- Perl identifier validity (Devin review) ------------------------------

    #[test]
    fn a_package_segment_may_not_begin_with_a_digit() {
        for target in ["123", "New::123", "123::Name", "9Foo"] {
            let plan = ModuleMovePlan::build(
                source(),
                ModuleMoveTarget::Package(target.into()),
                vec![declaration()],
                source_only_snapshot(),
                false,
            );
            assert!(
                plan.blockers.contains(&ModuleMoveBlocker::UnsafeTarget),
                "{target} would have been written as a package declaration"
            );
            assert!(!plan.is_complete());
        }
    }

    #[test]
    fn an_underscore_or_letter_leading_segment_remains_valid() {
        for target in ["_Private::Name", "New::N9"] {
            let plan = ModuleMovePlan::build(
                source(),
                ModuleMoveTarget::Package(target.into()),
                vec![declaration()],
                source_only_snapshot(),
                false,
            );
            assert!(plan.is_complete(), "{target} was rejected: {:?}", plan.blockers);
        }
    }

    // --- declaration whitespace (Devin review) --------------------------------

    #[test]
    fn any_perl_whitespace_separates_the_package_keyword_from_its_name() {
        for text in ["package\tOld::Name", "package\nOld::Name", "package  Old::Name"] {
            let mut item = declaration();
            item.old_text = text.into();
            item.end_byte = text.len() as u32;
            let plan = plan(vec![item], source_only_snapshot());
            assert!(
                plan.is_complete(),
                "{text:?} was not recognized as a declaration: {:?}",
                plan.blockers
            );
        }
    }

    #[test]
    fn a_package_like_identifier_is_not_a_declaration() {
        // `packageX Old::Name` is the discriminating case: the identity is
        // boundary-clean, so only the keyword rule can reject it.
        for text in ["packageOld::Name", "repackage Old::Name", "packageX Old::Name"] {
            let mut item = declaration();
            item.old_text = text.into();
            item.end_byte = text.len() as u32;
            let plan = plan(vec![item], source_only_snapshot());
            assert!(
                plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration),
                "{text:?} was accepted as a declaration"
            );
        }
    }

    // --- fingerprint identity (thread: fingerprint omits most of the payload)

    /// Every load-bearing field must move the fingerprint.  Each closure
    /// mutates one field a `Debug`-blob fingerprint over `source_uri`,
    /// generation, target, path, disposition, edits and blockers would miss.
    #[test]
    fn every_load_bearing_field_moves_the_fingerprint() {
        let baseline = plan(vec![declaration()], source_only_snapshot());
        let mutations: Vec<(&str, fn(&mut ModuleMovePlan))> = vec![
            ("schema-version", |plan| plan.schema_version = 2),
            ("workspace", |plan| plan.source.workspace = "other".into()),
            ("root", |plan| plan.source.root = "blib".into()),
            ("file-id", |plan| plan.source.file_id = FileId(99)),
            ("relative-path", |plan| plan.source.relative_path = "lib/Other.pm".into()),
            ("package", |plan| plan.source.package = "Other".into()),
            ("module", |plan| plan.source.module = "Other".into()),
            ("editable", |plan| plan.source.editable = false),
            ("restricted", |plan| plan.source.restricted = true),
            ("primary-package-count", |plan| plan.source.primary_package_count = 2),
            ("occurrences-complete", |plan| plan.source.occurrences_complete = false),
            ("resource-source-path", |plan| plan.resource.source_path = "lib/Other.pm".into()),
            ("resource-source-module", |plan| plan.resource.source_module = "Other".into()),
            ("edit-entity-id", |plan| plan.edits[0].entity_id = EntityId(99)),
            ("edit-anchor-id", |plan| plan.edits[0].anchor_id = AnchorId(99)),
            ("edit-occurrence-id", |plan| plan.edits[0].occurrence_id = OccurrenceId(99)),
            ("edit-file-id", |plan| plan.edits[0].file_id = FileId(99)),
            ("edit-kind", |plan| plan.edits[0].kind = OccurrenceKind::Reference),
            ("edit-generation", |plan| {
                plan.edits[0].generation = SourceGeneration::known("g9");
            }),
            ("edit-new-text", |plan| plan.edits[0].new_text = "package Other".into()),
            ("edit-start-byte", |plan| plan.edits[0].start_byte = 4),
            ("target-was-absent", |plan| plan.target_was_absent = false),
            ("generation-value", |plan| {
                plan.current_generations = vec![ModuleMoveFileGeneration {
                    file_id: SOURCE_FILE,
                    generation: SourceGeneration::known("g9"),
                }];
            }),
            ("generation-file-id", |plan| {
                plan.current_generations = vec![ModuleMoveFileGeneration {
                    file_id: FileId(99),
                    generation: SourceGeneration::known("g1"),
                }];
            }),
            ("generation-count", |plan| {
                plan.current_generations.push(ModuleMoveFileGeneration {
                    file_id: FileId(99),
                    generation: SourceGeneration::known("g1"),
                });
            }),
        ];
        for (label, mutate) in mutations {
            let mut mutated = baseline.clone();
            mutate(&mut mutated);
            assert_ne!(
                rebuild_fingerprint(&mutated),
                baseline.fingerprint,
                "{label} does not reach the fingerprint"
            );
        }
    }

    #[test]
    fn the_fingerprint_is_deterministic_for_unchanged_facts() {
        let first = plan(vec![declaration()], source_only_snapshot());
        let second = plan(vec![declaration()], source_only_snapshot());
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_ne!(first.fingerprint, plan(Vec::new(), source_only_snapshot()).fingerprint);
    }

    /// Field boundaries must not be forgeable by shifting content between
    /// adjacent labelled fields.
    #[test]
    fn adjacent_field_content_cannot_shift_across_a_boundary() {
        let mut left = plan(vec![declaration()], source_only_snapshot());
        let mut right = left.clone();
        left.source.package = "Ab".into();
        left.source.module = "c".into();
        right.source.package = "A".into();
        right.source.module = "bc".into();
        assert_ne!(rebuild_fingerprint(&left), rebuild_fingerprint(&right));
    }

    #[test]
    fn a_plan_round_trips_through_json_unchanged() -> Result<(), serde_json::Error> {
        let built = plan(vec![declaration()], source_only_snapshot());
        let wire = serde_json::to_string(&built)?;
        let received: ModuleMovePlan = serde_json::from_str(&wire)?;
        assert_eq!(received, built);
        assert!(received.is_complete());
        Ok(())
    }

    // --- builder/acceptance agreement (Devin review) --------------------------

    /// The invariant behind the same-name finding, stated generally: whatever
    /// `build` produces, `validate` must accept as an honest plan. A builder
    /// that emits a plan its own acceptance boundary calls malformed has an
    /// internal contradiction, whatever the disposition.
    #[test]
    fn every_plan_the_builder_produces_validates() {
        let cases: Vec<(&str, ModuleMoveTarget, Vec<ModuleMoveOccurrence>, bool)> = vec![
            (
                "same-name target",
                ModuleMoveTarget::Package("Old::Name".into()),
                vec![declaration()],
                false,
            ),
            (
                "target exists",
                ModuleMoveTarget::Package("New::Name".into()),
                vec![declaration()],
                true,
            ),
            (
                "traversal",
                ModuleMoveTarget::RelativePath("lib/../New.pm".into()),
                vec![declaration()],
                false,
            ),
            ("digit target", ModuleMoveTarget::Package("123".into()), vec![declaration()], false),
            ("empty denominator", ModuleMoveTarget::Package("New::Name".into()), Vec::new(), false),
            (
                "unsupported occurrence",
                ModuleMoveTarget::Package("New::Name".into()),
                vec![declaration(), {
                    let mut item = occurrence_at("use Old::Name", 40, 4);
                    item.unsupported = true;
                    item
                }],
                false,
            ),
            (
                "ambiguous anchor",
                ModuleMoveTarget::Package("New::Name".into()),
                vec![declaration(), occurrence_at("package Old::Name; use Old::Name;", 40, 4)],
                false,
            ),
            (
                "happy path",
                ModuleMoveTarget::Package("New::Name".into()),
                vec![declaration(), occurrence_at("use Old::Name", 40, 4)],
                false,
            ),
        ];
        for (label, target, occurrences, target_exists) in cases {
            let plan = ModuleMovePlan::build(
                source(),
                target,
                occurrences,
                source_only_snapshot(),
                target_exists,
            );
            assert_eq!(plan.validate(), Ok(()), "{label} produced a plan validate rejects");
        }
    }

    #[test]
    fn a_same_name_target_emits_no_no_op_edit() {
        let plan = ModuleMovePlan::build(
            source(),
            ModuleMoveTarget::Package("Old::Name".into()),
            vec![declaration()],
            source_only_snapshot(),
            false,
        );
        assert!(plan.blockers.contains(&ModuleMoveBlocker::TargetCollision));
        assert!(plan.edits.is_empty(), "a no-op edit was planned: {:?}", plan.edits);
        assert_eq!(plan.validate(), Ok(()));
    }

    // --- the declaration's own name slot (Devin review) ------------------------

    /// `package Other; Old::Name` declares `Other`. Renaming the trailing
    /// reference would leave the real declaration untouched while the plan
    /// reported itself complete.
    #[test]
    fn an_identity_outside_the_declaration_slot_does_not_declare_it() {
        for text in [
            "package Other; Old::Name",
            "package Other::Thing; use Old::Name",
            "package  Other;Old::Name",
        ] {
            let mut item = declaration();
            item.old_text = text.into();
            item.end_byte = text.len() as u32;
            let plan = plan(vec![item], source_only_snapshot());
            assert!(
                plan.blockers.contains(&ModuleMoveBlocker::MissingPackageDeclaration),
                "{text:?} was accepted as declaring Old::Name"
            );
            assert!(!plan.is_complete());
        }
    }

    /// `build` now refuses to construct this, so the acceptance boundary needs
    /// its own control: a forged complete plan whose declaration edit is a
    /// well-formed substitution that does not sit in the declaration slot.
    #[test]
    fn a_forged_declaration_edit_outside_the_slot_is_refused() {
        let mut forged = plan(vec![declaration()], source_only_snapshot());
        let old_text = "package Other; Old::Name".to_string();
        let new_text = "package Other; New::Name".to_string();
        forged.edits[0].end_byte = forged.edits[0].start_byte + old_text.len() as u32;
        forged.edits[0].old_text = old_text;
        forged.edits[0].new_text = new_text;
        forged.fingerprint = rebuild_fingerprint(&forged);
        assert_eq!(forged.validate(), Err(ModuleMoveInvalidPlan::MissingPackageDeclaration));
        assert!(!forged.is_complete());
    }

    #[test]
    fn the_declaration_slot_still_accepts_real_declarations() {
        for text in ["package Old::Name", "package\tOld::Name;", "  package  Old::Name ;"] {
            let mut item = declaration();
            item.old_text = text.into();
            item.end_byte = text.len() as u32;
            let plan = plan(vec![item], source_only_snapshot());
            assert!(plan.is_complete(), "{text:?} was rejected: {:?}", plan.blockers);
        }
    }

    // --- legacy package separator (Devin review) ------------------------------

    /// Perl's `'` is a package separator, so `Old::Name'Child` is a longer
    /// identity and renaming its prefix would corrupt it.
    #[test]
    fn the_legacy_apostrophe_separator_extends_a_package_identity() {
        for text in ["use Old::Name'Child", "use Prefix'Old::Name"] {
            let item = occurrence_at(text, 40, 4);
            let plan = plan(vec![declaration(), item], source_only_snapshot());
            assert!(
                plan.blockers.contains(&ModuleMoveBlocker::UnsupportedProjection),
                "{text} was treated as the source identity"
            );
            assert!(
                plan.edits.iter().all(|edit| edit.occurrence_id != OccurrenceId(4)),
                "a longer legacy-separator identity was partially renamed"
            );
        }
    }

    // --- helpers -----------------------------------------------------------

    fn occurrence_at(text: &str, start: u32, ids: u64) -> ModuleMoveOccurrence {
        ModuleMoveOccurrence {
            occurrence_id: OccurrenceId(ids),
            anchor_id: AnchorId(ids + 100),
            start_byte: start,
            end_byte: start + text.len() as u32,
            ..occurrence(text, OccurrenceKind::Import)
        }
    }

    fn rebuild_fingerprint(plan: &ModuleMovePlan) -> String {
        fingerprint_of(
            plan.schema_version,
            &plan.source,
            &plan.current_generations,
            plan.target_was_absent,
            &plan.resource,
            &plan.edits,
            &plan.blockers,
            plan.disposition,
        )
    }
}
