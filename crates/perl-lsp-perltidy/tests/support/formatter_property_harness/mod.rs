//! Deterministic seedable property harness for formatter safety invariants
//! (#10301).
//!
//! RED SKELETON: types and registry shape exist; generators and the invariant
//! checker are not implemented yet. Every generated case intentionally reports
//! a `harness.skeleton` violation so the acceptance properties fail for honest
//! not-yet-proven reasons before the implementation lands.
//!
//! Authority boundary: this module consumes only the canonical typed
//! production APIs (`format_document_typed` / `format_range_typed`) and the
//! independent byte-edit oracle `apply_edits_exact`. It never references the
//! subprocess adapter, never spawns processes, and never reads a clock.

use perl_lsp_perltidy::native::{
    BracePlacement, FinalNewline, KeywordSpacing, TextRange,
};

/// Schema version stamped into every generated case and receipt.
pub const HARNESS_SCHEMA_VERSION: u32 = 1;

/// Hard byte bound for any generated subject.
pub const MAX_SUBJECT_BYTES: usize = 4096;

/// Hard bound on the number of edits one produced plan may carry.
pub const MAX_PLAN_EDITS: usize = 64;

/// Hard bound on composed source lines per generated subject.
pub const MAX_SUBJECT_LINES: usize = 8;

/// Admitted construct family identifiers, ordered by registry position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `my` / `our` declarations with optional destructuring and initializer.
    LexicalDeclaration,
    /// Plain scalar assignment statements.
    PlainAssignment,
    /// `return` statements with optional value.
    ReturnStatement,
    /// `next` / `last` / `redo` loop-control statements, with optional label.
    LoopControl,
    /// `package` / `use` / `no` / `require` module-surface lines.
    ModuleSurface,
    /// `if` / `unless` one-liner blocks with optional elsif/else tails.
    ConditionalBlock,
    /// `while` / `until` one-liner blocks with optional continue tails.
    LoopBlock,
    /// `foreach` / list-`for` one-liner blocks.
    ForEachBlock,
    /// C-style `for (init; cond; update) { ... }` one-liner blocks.
    CStyleForBlock,
    /// `sub name { ... }` one-liner blocks.
    SubroutineBlock,
}

impl Family {
    /// Stable registry name for receipts and pinning.
    pub fn name(self) -> &'static str {
        match self {
            Family::LexicalDeclaration => "lexical_declaration",
            Family::PlainAssignment => "plain_assignment",
            Family::ReturnStatement => "return_statement",
            Family::LoopControl => "loop_control",
            Family::ModuleSurface => "module_surface",
            Family::ConditionalBlock => "conditional_block",
            Family::LoopBlock => "loop_block",
            Family::ForEachBlock => "for_each_block",
            Family::CStyleForBlock => "c_style_for_block",
            Family::SubroutineBlock => "subroutine_block",
        }
    }
}

/// One registry row: an admitted family plus its generator/mutator
/// dispositions. A family with no disposition must fail the suite (FPH-001).
#[derive(Debug, Clone)]
pub struct FamilyRecord {
    /// Admitted family.
    pub family: Family,
    /// Whether formatting this family renders closing-brace lines.
    pub renders_closed_blocks: bool,
    /// Required generator/mutator dispositions for this family.
    pub dispositions: &'static [&'static str],
}

/// Line-ending variants exercised across generated subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndingKind {
    /// `LF` separators only.
    Lf,
    /// `CRLF` separators only.
    Crlf,
    /// Bare `CR` separators only.
    BareCr,
    /// Mixed `LF` and `CRLF` separators.
    Mixed,
}

impl LineEndingKind {
    /// Stable receipt name.
    pub fn name(self) -> &'static str {
        match self {
            LineEndingKind::Lf => "lf",
            LineEndingKind::Crlf => "crlf",
            LineEndingKind::BareCr => "bare_cr",
            LineEndingKind::Mixed => "mixed",
        }
    }
}

/// Varied configuration axes for one generated case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Keyword/condition spacing axis.
    pub keyword_spacing: KeywordSpacing,
    /// Brace placement axis.
    pub brace_placement: BracePlacement,
    /// Final newline policy axis.
    pub final_newline: FinalNewline,
    /// Line-ending variant of the subject text.
    pub line_ending: LineEndingKind,
}

/// A generated subject with its construction-time validity knowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// Exact source bytes handed to the formatter.
    pub text: String,
    /// Whether the subject parses cleanly by generator construction.
    pub clean_parse_by_construction: bool,
}

/// Requested formatting target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRequest {
    /// Format the complete document.
    Document,
    /// Format one explicit UTF-16 range.
    Range { range: TextRange },
}

/// One fully described deterministic case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedCase {
    /// Seed the case was generated from.
    pub seed: u64,
    /// Harness schema version.
    pub schema_version: u32,
    /// Admitted family under test.
    pub family: Family,
    /// Registry disposition that constructed this case.
    pub disposition: &'static str,
    /// Subject text and construction validity.
    pub subject: Subject,
    /// Requested target.
    pub target: TargetRequest,
    /// Varied configuration axes.
    pub profile: Profile,
    /// Whether the case deliberately maps to a typed refusal.
    pub expects_refusal: bool,
}

/// Observation of the mandatory second pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondPassObservation {
    /// Disposition name of the second pass.
    pub disposition: &'static str,
    /// Edit count of the second pass.
    pub edit_count: usize,
    /// Whether second-pass rendered bytes equal first-pass bytes.
    pub bytes_stable: bool,
}

/// Normalized, bounded evidence record for one checked case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseReceipt {
    /// Seed the case was generated from.
    pub seed: u64,
    /// Harness schema version.
    pub schema_version: u32,
    /// Admitted family name.
    pub family: &'static str,
    /// Registry disposition name.
    pub disposition: &'static str,
    /// Target kind name.
    pub target: &'static str,
    /// Line-ending variant name.
    pub line_ending: &'static str,
    /// First-pass disposition name.
    pub outcome_disposition: &'static str,
    /// First-pass reason-class name.
    pub outcome_reason: &'static str,
    /// Number of edits in the produced plan.
    pub plan_edit_count: usize,
    /// Whether the applied plan independently rendered the exact bytes.
    pub applied_application_verified: bool,
    /// Whether plan ordering, non-overlap, and target containment held.
    pub plan_ordering_verified: bool,
    /// Whether UTF-16 ranges were valid for the exact subject geometry.
    pub utf16_geometry_verified: bool,
    /// Whether line-ending conventions were preserved.
    pub line_endings_preserved: bool,
    /// Second-pass observation.
    pub second_pass: Option<SecondPassObservation>,
    /// Canonical normalized receipt text.
    pub normalized: String,
    /// Deterministic digest over the normalized receipt.
    pub digest: String,
}

/// A violated harness invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Stable rule identifier.
    pub rule: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.rule, self.detail)
    }
}

impl std::error::Error for Violation {}

/// The admitted-family registry. RED SKELETON: dispositions are empty so
/// FPH-001 fails until the generator wiring lands.
pub fn family_registry() -> &'static [FamilyRecord] {
    &[
        FamilyRecord { family: Family::LexicalDeclaration, renders_closed_blocks: false, dispositions: &[] },
        FamilyRecord { family: Family::PlainAssignment, renders_closed_blocks: false, dispositions: &[] },
        FamilyRecord { family: Family::ReturnStatement, renders_closed_blocks: false, dispositions: &[] },
        FamilyRecord { family: Family::LoopControl, renders_closed_blocks: false, dispositions: &[] },
        FamilyRecord { family: Family::ModuleSurface, renders_closed_blocks: false, dispositions: &[] },
        FamilyRecord { family: Family::ConditionalBlock, renders_closed_blocks: true, dispositions: &[] },
        FamilyRecord { family: Family::LoopBlock, renders_closed_blocks: true, dispositions: &[] },
        FamilyRecord { family: Family::ForEachBlock, renders_closed_blocks: true, dispositions: &[] },
        FamilyRecord { family: Family::CStyleForBlock, renders_closed_blocks: true, dispositions: &[] },
        FamilyRecord { family: Family::SubroutineBlock, renders_closed_blocks: true, dispositions: &[] },
    ]
}

/// A gated invariant whose oracle does not exist on today's tree (FPH-008).
#[derive(Debug, Clone)]
pub struct DormantInvariant {
    /// Stable identifier.
    pub id: &'static str,
    /// The in-tree mechanism that must exist before this converts.
    pub gate: &'static str,
    /// Owning issues that land the gate.
    pub owning_issues: &'static [&'static str],
}

/// Status of one dormant invariant slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DormantStatus {
    /// The property is registered but not proven on this tree.
    NotProven,
    /// The gate landed and the property is asserted for real.
    Proven,
}

impl DormantInvariant {
    /// Report the current status. Fail-closed: always not-proven today.
    pub fn status(&self) -> DormantStatus {
        DormantStatus::NotProven
    }
}

/// The dormant-invariant registry. These slots report not-proven until their
/// owning issues land the gate mechanisms.
pub fn dormant_registry() -> &'static [DormantInvariant] {
    &[
        DormantInvariant {
            id: "cancellation_budget_interruption",
            gate: "format entry points accept a cancellation/budget checkpoint input",
            owning_issues: &["7140"],
        },
        DormantInvariant {
            id: "structural_preservation_beyond_parse_success",
            gate: "structural-preservation oracle beyond clean re-parse",
            owning_issues: &["8146"],
        },
        DormantInvariant {
            id: "protected_region_hash_preservation",
            gate: "protected-region/opaque-geometry hashing consumable at the typed outcome",
            owning_issues: &["7101", "7104", "7111", "7120"],
        },
    ]
}

/// Registry row for one admitted family.
pub fn record_for(family: Family) -> &'static FamilyRecord {
    const MISSING: FamilyRecord = FamilyRecord {
        family: Family::LexicalDeclaration,
        renders_closed_blocks: false,
        dispositions: &[],
    };
    family_registry()
        .iter()
        .find(|record| record.family == family)
        .unwrap_or(&MISSING)
}

/// Generate one deterministic valid case from `(seed, index)`.
///
/// RED SKELETON: returns a single stub case; family coverage and disposition
/// wiring land with the implementation.
pub fn generate_case(seed: u64, _index: usize) -> GeneratedCase {
    GeneratedCase {
        seed,
        schema_version: HARNESS_SCHEMA_VERSION,
        family: Family::LexicalDeclaration,
        disposition: "generator.skeleton",
        subject: Subject { text: String::new(), clean_parse_by_construction: true },
        target: TargetRequest::Document,
        profile: Profile {
            keyword_spacing: KeywordSpacing::Space,
            brace_placement: BracePlacement::SameLine,
            final_newline: FinalNewline::Preserve,
            line_ending: LineEndingKind::Lf,
        },
        expects_refusal: false,
    }
}

/// Generate one deterministic deliberately-invalid case from `(seed, index)`.
/// Such cases must map only to typed refusals or not-proven outcomes.
pub fn generate_invalidation_case(seed: u64, index: usize) -> GeneratedCase {
    let mut case = generate_case(seed, index);
    case.expects_refusal = true;
    case.subject.clean_parse_by_construction = false;
    case
}

/// Run the full invariant checker for one case and return its receipt.
///
/// RED SKELETON: always reports `harness.skeleton`.
pub fn run_case(case: &GeneratedCase) -> Result<CaseReceipt, Violation> {
    let _ = case;
    Err(Violation {
        rule: "harness.skeleton",
        detail: "checker not implemented yet".to_string(),
    })
}
