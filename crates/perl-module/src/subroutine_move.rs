//! Pure planning contract for moving one ordinary source-backed named package
//! subroutine to another module (#2990).
//!
//! Consumes already-authorized canonical facts.  Does not read a workspace,
//! inspect disk, parse source, or produce protocol edit types.
//!
//! **Admitted callable family (initial profile):**
//! - one named package subroutine (`sub name { ... }`) in an editable source file;
//! - one primary source-backed package in that file;
//! - target is an existing editable module file under the same authorized root;
//! - call sites are exact unqualified call projections rewritten to
//!   `Target::name(...)`;
//! - the definition is removed from the source file and appended to the target.
//!
//! Import/export setup, POD preservation, methods, anonymous subs, and generated
//! members are out of scope for this profile and safe-refuse.

use crate::path::is_lookup_safe_module_name;
use perl_semantic_facts::{
    AnchorId, EntityId, FileId, OccurrenceId, OccurrenceKind, SourceGeneration,
};
use serde::{Deserialize, Serialize};

/// Current plan schema version.  A plan carrying any other version is refused.
pub const SUBROUTINE_MOVE_SCHEMA_VERSION: u16 = 1;

/// The admitted current generation of one file the plan depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveFileGeneration {
    /// Canonical file identity.
    pub file_id: FileId,
    /// The generation the producer proves is current for that file.
    pub generation: SourceGeneration,
}

/// Canonical source facts for the subroutine's current file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveSource {
    /// Workspace identity.
    pub workspace: String,
    /// Authorized source-root identity.
    pub root: String,
    /// Source file identity.
    pub file_id: FileId,
    /// Workspace-relative source path.
    pub relative_path: String,
    /// Primary source-backed package in the file.
    pub package: String,
    /// Current source generation for the source file.
    pub generation: SourceGeneration,
    /// Whether the source is an editable workspace file.
    pub editable: bool,
    /// Whether this is generated/vendor/staged/external source.
    pub restricted: bool,
    /// Whether the callable occurrence denominator is complete for this profile.
    pub occurrences_complete: bool,
    /// Bare subroutine name without sigil or package prefix.
    pub callable_name: String,
}

/// Canonical target module facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveTarget {
    /// Target module/package name.
    pub module: String,
    /// Workspace-relative path to the target module file.
    pub relative_path: String,
    /// Target file identity.
    pub file_id: FileId,
    /// Current source generation for the target file.
    pub generation: SourceGeneration,
    /// Whether the target file is editable.
    pub editable: bool,
    /// Whether the target is restricted/generated/external.
    pub restricted: bool,
}

/// One canonical semantic occurrence admitted to the move denominator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveOccurrence {
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
    /// Producer stale flag — additional refusal input.
    pub stale: bool,
    /// True when the producer cannot prove this projection is supported.
    pub unsupported: bool,
}

/// A planned exact source replacement edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveEdit {
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
    /// Replacement text (empty means delete the anchored span).
    pub new_text: String,
    /// Inclusive start byte.
    pub start_byte: u32,
    /// Exclusive end byte.
    pub end_byte: u32,
}

/// Append the moved subroutine body to the target module file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMoveAppend {
    /// Target document identity.
    pub file_id: FileId,
    /// Generation precondition for the target document.
    pub generation: SourceGeneration,
    /// Byte offset where the subroutine text is appended.
    pub insertion_byte: u32,
    /// Exact subroutine source text to append (including leading newline when required).
    pub text: String,
}

/// Why a subroutine move cannot be authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SubroutineMoveBlocker {
    /// Source identity is incomplete or inadmissible for this profile.
    InvalidSource,
    /// Target identity is incomplete or inadmissible.
    InvalidTarget,
    /// Generation is unknown or an occurrence is off its file's current generation.
    StaleOrUnknownGeneration,
    /// Missing or contradictory generation evidence for a depended-on file.
    MissingFileGeneration,
    /// Target path escapes its authorized root.
    CrossRoot,
    /// A relevant occurrence is dynamic.
    DynamicBoundary,
    /// A relevant occurrence is unsupported for this profile.
    UnsupportedProjection,
    /// Anchor range/text mismatch.
    InvalidAnchor,
    /// Two admitted occurrences share an identity in one file.
    DuplicateOccurrence,
    /// Planned edits overlap in one file.
    OverlappingEdits,
    /// Denominator incomplete.
    IncompleteOccurrences,
    /// No definition occurrence for the callable in the source file.
    MissingCallableDefinition,
    /// More than one definition occurrence for the callable in the source file.
    AmbiguousCallableDefinition,
    /// Definition text does not match the admitted callable name/profile.
    CallableDefinitionMismatch,
}

impl SubroutineMoveBlocker {
    /// Stable wire tag, independent of `Debug` formatting.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::InvalidSource => "invalid-source",
            Self::InvalidTarget => "invalid-target",
            Self::StaleOrUnknownGeneration => "stale-or-unknown-generation",
            Self::MissingFileGeneration => "missing-file-generation",
            Self::CrossRoot => "cross-root",
            Self::DynamicBoundary => "dynamic-boundary",
            Self::UnsupportedProjection => "unsupported-projection",
            Self::InvalidAnchor => "invalid-anchor",
            Self::DuplicateOccurrence => "duplicate-occurrence",
            Self::OverlappingEdits => "overlapping-edits",
            Self::IncompleteOccurrences => "incomplete-occurrences",
            Self::MissingCallableDefinition => "missing-callable-definition",
            Self::AmbiguousCallableDefinition => "ambiguous-callable-definition",
            Self::CallableDefinitionMismatch => "callable-definition-mismatch",
        }
    }
}

/// Whether the pure plan can be materialized as a complete operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubroutineMoveDisposition {
    /// Every required member is present and current.
    Complete,
    /// The operation must be refused with its blockers.
    Blocked,
}

impl SubroutineMoveDisposition {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubroutineMoveInvalidPlan {
    /// The plan does not carry [`SUBROUTINE_MOVE_SCHEMA_VERSION`].
    UnknownSchemaVersion,
    /// `disposition` and `blockers` disagree about refusal.
    DispositionDisagreesWithBlockers,
    /// Blockers are not in canonical sorted, deduplicated order.
    BlockersNotCanonical,
    /// A complete plan carries no edit and no append.
    CompletePlanWithoutChanges,
    /// An edit's range, text, or generation is malformed.
    MalformedEdit,
    /// Edits are not in canonical order.
    EditsNotCanonical,
    /// Two edits in one file share an occurrence or anchor identity.
    DuplicateEdit,
    /// Two edits in one file cover overlapping bytes.
    OverlappingEdits,
    /// Append metadata is malformed or not at the target's admitted generation.
    MalformedAppend,
    /// The source identity would not have been eligible to authorize any edit.
    SourceNotEligible,
    /// The target identity would not have been eligible.
    TargetNotEligible,
    /// Generation snapshot is missing, non-canonical, or contradictory.
    GenerationEvidenceUnusable,
    /// An edit is not at its file's current generation.
    EditIsNotAtItsFilesCurrentGeneration,
    /// An edit is not the rewrite this plan claims for that occurrence.
    EditIsNotTheExpectedRewrite,
    /// The fingerprint does not identify this payload.
    FingerprintMismatch,
    /// A complete plan is missing exactly one callable definition removal.
    MissingCallableDefinitionRemoval,
    /// A complete plan is missing the target append of the moved body.
    MissingTargetAppend,
}

/// Pure, deterministic subroutine-move plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineMovePlan {
    /// Plan schema version.
    pub schema_version: u16,
    /// Admitted source facts.
    pub source: SubroutineMoveSource,
    /// Admitted target facts.
    pub target: SubroutineMoveTarget,
    /// Admitted current generation of every file the plan depends on.
    pub current_generations: Vec<SubroutineMoveFileGeneration>,
    /// Exact replacement edits (definition removal and call-site rewrites).
    pub edits: Vec<SubroutineMoveEdit>,
    /// Append of the subroutine body to the target module file.
    pub append: Option<SubroutineMoveAppend>,
    /// Typed refusal reasons, empty for a complete plan.
    pub blockers: Vec<SubroutineMoveBlocker>,
    /// Plan disposition.  Not authorization on its own — see [`Self::is_complete`].
    pub disposition: SubroutineMoveDisposition,
    /// Deterministic identity over every load-bearing field of the plan.
    pub fingerprint: String,
}

impl SubroutineMovePlan {
    /// Build a plan from canonical facts without reading or mutating anything.
    #[must_use]
    pub fn build(
        source: SubroutineMoveSource,
        target: SubroutineMoveTarget,
        occurrences: Vec<SubroutineMoveOccurrence>,
        current_generations: Vec<SubroutineMoveFileGeneration>,
        target_insertion_byte: u32,
    ) -> Self {
        let mut blockers = Vec::new();
        let root = source.root.trim_end_matches('/').to_string();

        let generations = CurrentGenerations::admit(current_generations);
        if !generations.contradicted.is_empty() {
            blockers.push(SubroutineMoveBlocker::MissingFileGeneration);
        }
        match generations.current(source.file_id) {
            None => blockers.push(SubroutineMoveBlocker::MissingFileGeneration),
            Some(current) if !current.is_known() => {
                blockers.push(SubroutineMoveBlocker::MissingFileGeneration);
            }
            Some(current) if *current != source.generation => {
                blockers.push(SubroutineMoveBlocker::StaleOrUnknownGeneration);
            }
            Some(_) => {}
        }
        match generations.current(target.file_id) {
            None => blockers.push(SubroutineMoveBlocker::MissingFileGeneration),
            Some(current) if !current.is_known() => {
                blockers.push(SubroutineMoveBlocker::MissingFileGeneration);
            }
            Some(current) if *current != target.generation => {
                blockers.push(SubroutineMoveBlocker::StaleOrUnknownGeneration);
            }
            Some(_) => {}
        }

        if !source_is_eligible(&source) {
            blockers.push(SubroutineMoveBlocker::InvalidSource);
        }
        if !target_is_eligible(&target, &root) {
            blockers.push(SubroutineMoveBlocker::InvalidTarget);
        }
        if !target.relative_path.starts_with(&format!("{root}/")) {
            blockers.push(SubroutineMoveBlocker::CrossRoot);
        }
        if !source.occurrences_complete {
            blockers.push(SubroutineMoveBlocker::IncompleteOccurrences);
        }
        if !source.generation.is_known() || !target.generation.is_known() {
            blockers.push(SubroutineMoveBlocker::StaleOrUnknownGeneration);
        }

        let mut edits = Vec::new();
        let mut definition_count = 0_u32;
        let mut definition_text: Option<String> = None;

        for occurrence in occurrences {
            let mut refusals: Vec<SubroutineMoveBlocker> = Vec::new();
            match generations.current(occurrence.file_id) {
                None => refusals.push(SubroutineMoveBlocker::MissingFileGeneration),
                Some(current) if !current.is_known() => {
                    refusals.push(SubroutineMoveBlocker::MissingFileGeneration);
                }
                Some(current) if *current != occurrence.file_generation => {
                    refusals.push(SubroutineMoveBlocker::StaleOrUnknownGeneration);
                }
                Some(_) => {}
            }
            if occurrence.stale {
                refusals.push(SubroutineMoveBlocker::StaleOrUnknownGeneration);
            }
            if occurrence.dynamic || occurrence.kind == OccurrenceKind::DynamicBoundary {
                refusals.push(SubroutineMoveBlocker::DynamicBoundary);
            }
            if occurrence.unsupported {
                refusals.push(SubroutineMoveBlocker::UnsupportedProjection);
            }
            if occurrence.old_text.is_empty()
                || occurrence.end_byte <= occurrence.start_byte
                || u64::from(occurrence.end_byte) - u64::from(occurrence.start_byte)
                    != occurrence.old_text.len() as u64
            {
                refusals.push(SubroutineMoveBlocker::InvalidAnchor);
            }

            let new_text = match occurrence.kind {
                OccurrenceKind::Definition
                    if occurrence.file_id == source.file_id
                        && is_callable_definition(&occurrence.old_text, &source.callable_name) =>
                {
                    definition_count += 1;
                    definition_text = Some(occurrence.old_text.clone());
                    Some(String::new())
                }
                OccurrenceKind::Call
                    if is_unqualified_call(&occurrence.old_text, &source.callable_name) =>
                {
                    Some(qualify_call(&occurrence.old_text, &target.module))
                }
                OccurrenceKind::Import | OccurrenceKind::Export => {
                    refusals.push(SubroutineMoveBlocker::UnsupportedProjection);
                    None
                }
                _ => {
                    refusals.push(SubroutineMoveBlocker::UnsupportedProjection);
                    None
                }
            };

            if occurrence.kind == OccurrenceKind::Definition
                && occurrence.file_id == source.file_id
                && !is_callable_definition(&occurrence.old_text, &source.callable_name)
            {
                refusals.push(SubroutineMoveBlocker::CallableDefinitionMismatch);
            }

            let refused = !refusals.is_empty();
            blockers.append(&mut refusals);
            if let (false, Some(new_text)) = (refused, new_text) {
                edits.push(SubroutineMoveEdit {
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

        match definition_count {
            0 => blockers.push(SubroutineMoveBlocker::MissingCallableDefinition),
            1 => {}
            _ => blockers.push(SubroutineMoveBlocker::AmbiguousCallableDefinition),
        }

        edits.sort_by(edit_order);
        blockers.extend(edit_set_conflicts(&edits));
        blockers.sort_unstable();
        blockers.dedup();

        let append = definition_text.map(|text| SubroutineMoveAppend {
            file_id: target.file_id,
            generation: target.generation.clone(),
            insertion_byte: target_insertion_byte,
            text,
        });

        let disposition = if blockers.is_empty() {
            SubroutineMoveDisposition::Complete
        } else {
            SubroutineMoveDisposition::Blocked
        };
        let current_generations = generations.canonical();
        let fingerprint = fingerprint_of(
            SUBROUTINE_MOVE_SCHEMA_VERSION,
            &source,
            &target,
            &current_generations,
            &edits,
            append.as_ref(),
            &blockers,
            disposition,
        );
        Self {
            schema_version: SUBROUTINE_MOVE_SCHEMA_VERSION,
            source,
            target,
            current_generations,
            edits,
            append,
            blockers,
            disposition,
            fingerprint,
        }
    }

    /// Re-derive every invariant this plan asserts about itself.
    ///
    /// # Errors
    ///
    /// Returns the first invariant the plan fails.
    pub fn validate(&self) -> Result<(), SubroutineMoveInvalidPlan> {
        if self.schema_version != SUBROUTINE_MOVE_SCHEMA_VERSION {
            return Err(SubroutineMoveInvalidPlan::UnknownSchemaVersion);
        }
        let claims_complete = matches!(self.disposition, SubroutineMoveDisposition::Complete);
        if claims_complete != self.blockers.is_empty() {
            return Err(SubroutineMoveInvalidPlan::DispositionDisagreesWithBlockers);
        }
        if !self.blockers.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(SubroutineMoveInvalidPlan::BlockersNotCanonical);
        }
        for edit in &self.edits {
            if edit.old_text.is_empty()
                || edit.end_byte <= edit.start_byte
                || u64::from(edit.end_byte) - u64::from(edit.start_byte)
                    != edit.old_text.len() as u64
                || edit.new_text == edit.old_text
                || !edit.generation.is_known()
            {
                return Err(SubroutineMoveInvalidPlan::MalformedEdit);
            }
        }
        if !self.edits.windows(2).all(|pair| edit_order(&pair[0], &pair[1]).is_lt()) {
            return Err(SubroutineMoveInvalidPlan::EditsNotCanonical);
        }
        if let Some(conflict) = edit_set_conflicts(&self.edits).first() {
            return Err(match conflict {
                SubroutineMoveBlocker::OverlappingEdits => {
                    SubroutineMoveInvalidPlan::OverlappingEdits
                }
                _ => SubroutineMoveInvalidPlan::DuplicateEdit,
            });
        }
        if let Some(append) = &self.append {
            if append.text.is_empty() || !append.generation.is_known() {
                return Err(SubroutineMoveInvalidPlan::MalformedAppend);
            }
            if append.file_id != self.target.file_id {
                return Err(SubroutineMoveInvalidPlan::MalformedAppend);
            }
        }
        if self.fingerprint
            != fingerprint_of(
                self.schema_version,
                &self.source,
                &self.target,
                &self.current_generations,
                &self.edits,
                self.append.as_ref(),
                &self.blockers,
                self.disposition,
            )
        {
            return Err(SubroutineMoveInvalidPlan::FingerprintMismatch);
        }
        let snapshot = CurrentGenerations::admit(self.current_generations.clone());
        if !snapshot.contradicted.is_empty() || snapshot.canonical() != self.current_generations {
            return Err(SubroutineMoveInvalidPlan::GenerationEvidenceUnusable);
        }
        for edit in &self.edits {
            if snapshot.current(edit.file_id) != Some(&edit.generation) {
                return Err(SubroutineMoveInvalidPlan::EditIsNotAtItsFilesCurrentGeneration);
            }
            let expected = expected_rewrite(edit, &self.source, &self.target);
            if edit.new_text != expected {
                return Err(SubroutineMoveInvalidPlan::EditIsNotTheExpectedRewrite);
            }
        }
        if !claims_complete {
            return Ok(());
        }
        if self.edits.is_empty() && self.append.is_none() {
            return Err(SubroutineMoveInvalidPlan::CompletePlanWithoutChanges);
        }
        if !source_is_eligible(&self.source) {
            return Err(SubroutineMoveInvalidPlan::SourceNotEligible);
        }
        if !target_is_eligible(&self.target, self.source.root.trim_end_matches('/')) {
            return Err(SubroutineMoveInvalidPlan::TargetNotEligible);
        }
        let definition_removals = self
            .edits
            .iter()
            .filter(|edit| {
                edit.file_id == self.source.file_id
                    && edit.kind == OccurrenceKind::Definition
                    && edit.new_text.is_empty()
                    && is_callable_definition(&edit.old_text, &self.source.callable_name)
            })
            .count();
        if definition_removals != 1 {
            return Err(SubroutineMoveInvalidPlan::MissingCallableDefinitionRemoval);
        }
        let Some(append) = &self.append else {
            return Err(SubroutineMoveInvalidPlan::MissingTargetAppend);
        };
        if snapshot.current(append.file_id) != Some(&append.generation) {
            return Err(SubroutineMoveInvalidPlan::MalformedAppend);
        }
        Ok(())
    }

    /// A checked complete plan is the only plan eligible for materialization.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self.disposition, SubroutineMoveDisposition::Complete) && self.validate().is_ok()
    }
}

struct CurrentGenerations {
    entries: Vec<SubroutineMoveFileGeneration>,
    contradicted: Vec<FileId>,
}

impl CurrentGenerations {
    fn admit(mut entries: Vec<SubroutineMoveFileGeneration>) -> Self {
        entries.sort_by(|a, b| {
            (a.file_id, generation_tag(&a.generation))
                .cmp(&(b.file_id, generation_tag(&b.generation)))
        });
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

    fn canonical(&self) -> Vec<SubroutineMoveFileGeneration> {
        self.entries
            .iter()
            .filter(|entry| !self.contradicted.contains(&entry.file_id))
            .cloned()
            .collect()
    }
}

fn edit_order(a: &SubroutineMoveEdit, b: &SubroutineMoveEdit) -> core::cmp::Ordering {
    (a.file_id, a.start_byte, a.end_byte, a.occurrence_id, a.anchor_id).cmp(&(
        b.file_id,
        b.start_byte,
        b.end_byte,
        b.occurrence_id,
        b.anchor_id,
    ))
}

fn edit_set_conflicts(edits: &[SubroutineMoveEdit]) -> Vec<SubroutineMoveBlocker> {
    let mut conflicts = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        for other in &edits[index + 1..] {
            if other.file_id != edit.file_id {
                continue;
            }
            if other.occurrence_id == edit.occurrence_id || other.anchor_id == edit.anchor_id {
                conflicts.push(SubroutineMoveBlocker::DuplicateOccurrence);
            }
            if other.start_byte < edit.end_byte && edit.start_byte < other.end_byte {
                conflicts.push(SubroutineMoveBlocker::OverlappingEdits);
            }
        }
    }
    conflicts
}

fn source_is_eligible(source: &SubroutineMoveSource) -> bool {
    let root = source.root.trim_end_matches('/');
    let expected_path = module_path(&source.package).map(|path| format!("{root}/{path}"));
    !source.workspace.trim().is_empty()
        && !root.is_empty()
        && !source.relative_path.trim().is_empty()
        && is_lookup_safe_module_name(&source.package)
        && is_lookup_safe_identifier(&source.callable_name)
        && expected_path.as_deref() == Some(source.relative_path.as_str())
        && source.editable
        && !source.restricted
        && source.occurrences_complete
        && source.generation.is_known()
}

fn target_is_eligible(target: &SubroutineMoveTarget, root: &str) -> bool {
    let expected_path = module_path(&target.module).map(|path| format!("{root}/{path}"));
    is_lookup_safe_module_name(&target.module)
        && expected_path.as_deref() == Some(target.relative_path.as_str())
        && target.editable
        && !target.restricted
        && target.generation.is_known()
}

fn module_path(package: &str) -> Option<String> {
    is_lookup_safe_module_name(package).then(|| format!("{}.pm", package.replace("::", "/")))
}

fn is_lookup_safe_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_callable_definition(text: &str, name: &str) -> bool {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("sub").filter(|r| {
        r.is_empty() || r.chars().next().is_some_and(|c| c.is_ascii_whitespace() || c == '(')
    });
    let Some(after_sub) = rest else {
        return false;
    };
    let name_start = after_sub.trim_start();
    name_start.strip_prefix(name).is_some_and(|tail| {
        tail.is_empty()
            || tail.starts_with(|c: char| c.is_ascii_whitespace() || c == '(' || c == '{')
    })
}

fn is_unqualified_call(text: &str, name: &str) -> bool {
    text.strip_prefix(name).is_some_and(|tail| tail.starts_with('(') || tail.starts_with('{'))
}

fn qualify_call(text: &str, target_module: &str) -> String {
    format!("{target_module}::{text}")
}

fn expected_rewrite(
    edit: &SubroutineMoveEdit,
    source: &SubroutineMoveSource,
    target: &SubroutineMoveTarget,
) -> String {
    if edit.kind == OccurrenceKind::Definition && edit.file_id == source.file_id {
        String::new()
    } else if edit.kind == OccurrenceKind::Call {
        qualify_call(&edit.old_text, &target.module)
    } else {
        edit.new_text.clone()
    }
}

fn fingerprint_of(
    schema_version: u16,
    source: &SubroutineMoveSource,
    target: &SubroutineMoveTarget,
    current_generations: &[SubroutineMoveFileGeneration],
    edits: &[SubroutineMoveEdit],
    append: Option<&SubroutineMoveAppend>,
    blockers: &[SubroutineMoveBlocker],
    disposition: SubroutineMoveDisposition,
) -> String {
    let mut acc = perl_semantic_facts::semantic_identity::SemanticIdentityFingerprint::new(
        "subroutine-move-plan-v1",
    )
    .field("schema-version", &schema_version.to_string())
    .field("workspace", &source.workspace)
    .field("root", &source.root)
    .field("source-file-id", &source.file_id.0.to_string())
    .field("source-relative-path", &source.relative_path)
    .field("source-package", &source.package)
    .field("source-generation", &generation_tag(&source.generation))
    .field("callable-name", &source.callable_name)
    .field("target-file-id", &target.file_id.0.to_string())
    .field("target-relative-path", &target.relative_path)
    .field("target-module", &target.module)
    .field("target-generation", &generation_tag(&target.generation))
    .field("disposition", disposition.tag())
    .field("edit-count", &edits.len().to_string());
    for edit in edits {
        acc = acc
            .field("edit-file-id", &edit.file_id.0.to_string())
            .field("edit-old-text", &edit.old_text)
            .field("edit-new-text", &edit.new_text);
    }
    if let Some(append) = append {
        acc = acc
            .field("append-file-id", &append.file_id.0.to_string())
            .field("append-byte", &append.insertion_byte.to_string())
            .field("append-text", &append.text);
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

fn generation_tag(generation: &SourceGeneration) -> String {
    match generation {
        SourceGeneration::Known(value) => format!("known:{value}"),
        SourceGeneration::Unknown => "unknown".to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE_FILE: FileId = FileId(1);
    const TARGET_FILE: FileId = FileId(2);
    const CALLER_FILE: FileId = FileId(3);

    fn source() -> SubroutineMoveSource {
        SubroutineMoveSource {
            workspace: "ws".into(),
            root: "lib".into(),
            file_id: SOURCE_FILE,
            relative_path: "lib/Source.pm".into(),
            package: "Source".into(),
            generation: SourceGeneration::known("g1"),
            editable: true,
            restricted: false,
            occurrences_complete: true,
            callable_name: "utility".into(),
        }
    }

    fn target() -> SubroutineMoveTarget {
        SubroutineMoveTarget {
            module: "Target".into(),
            relative_path: "lib/Target.pm".into(),
            file_id: TARGET_FILE,
            generation: SourceGeneration::known("g2"),
            editable: true,
            restricted: false,
        }
    }

    fn generation(file_id: FileId, tag: &str) -> SubroutineMoveFileGeneration {
        SubroutineMoveFileGeneration { file_id, generation: SourceGeneration::known(tag) }
    }

    fn def(text: &str) -> SubroutineMoveOccurrence {
        SubroutineMoveOccurrence {
            file_id: SOURCE_FILE,
            occurrence_id: OccurrenceId(1),
            anchor_id: AnchorId(10),
            entity_id: EntityId(100),
            kind: OccurrenceKind::Definition,
            old_text: text.into(),
            start_byte: 0,
            end_byte: text.len() as u32,
            file_generation: SourceGeneration::known("g1"),
            dynamic: false,
            stale: false,
            unsupported: false,
        }
    }

    fn call(file_id: FileId, text: &str, ids: u64, generation: &str) -> SubroutineMoveOccurrence {
        SubroutineMoveOccurrence {
            file_id,
            occurrence_id: OccurrenceId(ids),
            anchor_id: AnchorId(ids + 100),
            entity_id: EntityId(200),
            kind: OccurrenceKind::Call,
            old_text: text.into(),
            start_byte: 10,
            end_byte: 10 + text.len() as u32,
            file_generation: SourceGeneration::known(generation),
            dynamic: false,
            stale: false,
            unsupported: false,
        }
    }

    fn snapshot() -> Vec<SubroutineMoveFileGeneration> {
        vec![generation(SOURCE_FILE, "g1"), generation(TARGET_FILE, "g2")]
    }

    #[test]
    fn complete_plan_removes_definition_qualifies_call_and_appends_to_target() {
        let sub_text = "sub utility { return 1; }";
        let plan = SubroutineMovePlan::build(
            source(),
            target(),
            vec![def(sub_text), call(CALLER_FILE, "utility()", 4, "g3")],
            vec![
                generation(SOURCE_FILE, "g1"),
                generation(TARGET_FILE, "g2"),
                generation(CALLER_FILE, "g3"),
            ],
            120,
        );
        assert!(plan.is_complete(), "{:?}", plan.blockers);
        assert!(plan.edits.iter().any(|edit| edit.new_text.is_empty()));
        assert!(plan.edits.iter().any(|edit| edit.new_text == "Target::utility()"));
        assert_eq!(plan.append.as_ref().map(|a| a.text.as_str()), Some(sub_text));
        assert_eq!(plan.append.as_ref().map(|a| a.insertion_byte), Some(120));
    }

    #[test]
    fn dynamic_call_site_refuses_without_partial_call_edit() {
        let mut item = call(CALLER_FILE, "utility()", 4, "g3");
        item.dynamic = true;
        let plan = SubroutineMovePlan::build(
            source(),
            target(),
            vec![def("sub utility {}"), item],
            vec![
                generation(SOURCE_FILE, "g1"),
                generation(TARGET_FILE, "g2"),
                generation(CALLER_FILE, "g3"),
            ],
            0,
        );
        assert!(!plan.is_complete());
        assert!(plan.blockers.contains(&SubroutineMoveBlocker::DynamicBoundary));
        assert!(plan.edits.iter().all(|edit| edit.kind != OccurrenceKind::Call));
    }

    #[test]
    fn missing_definition_blocks_complete_plan() {
        let plan = SubroutineMovePlan::build(
            source(),
            target(),
            vec![call(CALLER_FILE, "utility()", 4, "g3")],
            snapshot(),
            0,
        );
        assert!(plan.blockers.contains(&SubroutineMoveBlocker::MissingCallableDefinition));
        assert!(!plan.is_complete());
    }
}
