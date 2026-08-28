//! Deterministic seedable property harness for formatter safety invariants
//! (#10301).
//!
//! The module owns four concerns:
//!
//! 1. an admitted safe-subset family registry where every family must carry
//!    at least one generator/mutator disposition (FPH-001);
//! 2. pure deterministic generators that turn `(seed, index)` into structured
//!    subjects, targets, and configuration profiles — never raw bytes, never
//!    executed Perl (FPH-001/FPH-007);
//! 3. the mandatory invariant checker consuming only the canonical typed
//!    production APIs (`format_document_typed` / `format_range_typed`) plus
//!    the independent byte-edit oracle `apply_edits_exact` (FPH-002..FPH-006);
//! 4. a fail-closed dormant-disposition registry for gated invariants whose
//!    oracles do not exist on today's tree (FPH-008).
//!
//! Authority boundary: this module never references the subprocess-backed
//! compatibility adapter, never spawns processes, never reads a clock, and
//! never applies expected bytes through production edit derivation.
//!
//! The cargo-fuzz target `fuzz/fuzz_targets/perl_tidy_formatter.rs` includes
//! this file verbatim via `#[path]` and drives the same checker from
//! structured byte mutations.

use perl_lsp_perltidy::native::{
    BracePlacement, EditSpec, FinalNewline, FormatContext, FormatDisposition,
    FormatLineEndingDisposition, FormatReasonCode, KeywordSpacing, NativeFormatter,
    PositionEncoding, TextRange, apply_edits_exact,
};

/// Schema version stamped into every generated case and receipt.
pub const HARNESS_SCHEMA_VERSION: u32 = 1;

/// Hard byte bound for any generated subject.
pub const MAX_SUBJECT_BYTES: usize = 4096;

/// Hard bound on the number of edits one produced plan may carry.
pub const MAX_PLAN_EDITS: usize = 64;

/// Hard bound on composed source lines per generated subject.
pub const MAX_SUBJECT_LINES: usize = 8;

/// Indentation prefixes used by the indent mutator.
const INDENTS: [&str; 3] = ["", "  ", "\t"];

/// Trailing comment suffixes used by the trailing-comment mutator.
const COMMENTS: [&str; 3] = ["", " # note", " # keep 1"];

/// Admitted construct family identifiers.
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
/// dispositions. A family with no disposition must fail the suite (FPH-001),
/// and each disposition here is deterministically exercised by
/// [`generate_case`] so registration can never outpace wiring.
#[derive(Debug, Clone)]
pub struct FamilyRecord {
    /// Admitted family.
    pub family: Family,
    /// Whether formatting this family renders closing-brace lines (which the
    /// formatter's already-formatted classification does not admit yet).
    pub renders_closed_blocks: bool,
    /// Required generator/mutator dispositions for this family.
    pub dispositions: &'static [&'static str],
}

struct FamilyVariants {
    family: Family,
    renders_closed_blocks: bool,
    /// Unformatted (compact) one-liner forms.
    compact: &'static [&'static str],
    /// Already-formatted forms; line-level families only. Block families have
    /// no single-line already-formatted rendering, so they reuse `compact`.
    spaced: &'static [&'static str],
}

const FAMILY_TABLE: [FamilyVariants; 10] = [
    FamilyVariants {
        family: Family::LexicalDeclaration,
        renders_closed_blocks: false,
        compact: &["my$x=1;", "our@list=(1,2);", "my($a,$b)=@_;"],
        spaced: &["my $x = 1;", "our @list = (1, 2);", "my ($a, $b) = @_;"],
    },
    FamilyVariants {
        family: Family::PlainAssignment,
        renders_closed_blocks: false,
        compact: &["$count=0;", "$name=\"demo\";", "$sum=$a+$b;"],
        spaced: &["$count = 0;", "$name = \"demo\";", "$sum = $a + $b;"],
    },
    FamilyVariants {
        family: Family::ReturnStatement,
        renders_closed_blocks: false,
        compact: &["return;", "return$value;", "return$a+$b;"],
        spaced: &["return;", "return $value;", "return $a + $b;"],
    },
    FamilyVariants {
        family: Family::LoopControl,
        renders_closed_blocks: false,
        compact: &["next;", "last;", "redo;", "next OUTER;"],
        spaced: &["next;", "last;", "redo;", "next OUTER;"],
    },
    FamilyVariants {
        family: Family::ModuleSurface,
        renders_closed_blocks: false,
        compact: &["package Demo;", "use strict;", "no warnings;", "require Exporter;"],
        spaced: &["package Demo;", "use strict;", "no warnings;", "require Exporter;"],
    },
    FamilyVariants {
        family: Family::ConditionalBlock,
        renders_closed_blocks: true,
        compact: &[
            "if($x){return 1;}",
            "unless($ok){next;}",
            "if($a==$b){return 1;}elsif($x){return 2;}else{return 3;}",
        ],
        spaced: &[],
    },
    FamilyVariants {
        family: Family::LoopBlock,
        renders_closed_blocks: true,
        compact: &["while($n){next;}", "until($done){last;}", "while($n){next;}continue{last;}"],
        spaced: &[],
    },
    FamilyVariants {
        family: Family::ForEachBlock,
        renders_closed_blocks: true,
        compact: &["foreach my$item(@items){return$item;}", "foreach$e(@list){next;}"],
        spaced: &[],
    },
    FamilyVariants {
        family: Family::CStyleForBlock,
        renders_closed_blocks: true,
        compact: &["for(my$i=0;$i<3;$i++){next;}", "for($i=0;$i<9;$i++){last;}"],
        spaced: &[],
    },
    FamilyVariants {
        family: Family::SubroutineBlock,
        renders_closed_blocks: true,
        compact: &["sub demo{return 1;}", "sub run{return$a+$b;}"],
        spaced: &[],
    },
];

fn variants_for(family: Family) -> &'static FamilyVariants {
    FAMILY_TABLE.iter().find(|variants| variants.family == family).unwrap_or(&FAMILY_TABLE[0])
}

/// The admitted-family registry. Deleting any single disposition entry turns
/// FPH-001 red.
pub fn family_registry() -> &'static [FamilyRecord] {
    &[
        FamilyRecord {
            family: Family::LexicalDeclaration,
            renders_closed_blocks: false,
            dispositions: &[
                "generator.lexical_declaration",
                "mutator.spacing_style",
                "mutator.indent_prefix",
            ],
        },
        FamilyRecord {
            family: Family::PlainAssignment,
            renders_closed_blocks: false,
            dispositions: &["generator.plain_assignment", "mutator.trailing_comment"],
        },
        FamilyRecord {
            family: Family::ReturnStatement,
            renders_closed_blocks: false,
            dispositions: &["generator.return_statement", "mutator.spacing_style"],
        },
        FamilyRecord {
            family: Family::LoopControl,
            renders_closed_blocks: false,
            dispositions: &["generator.loop_control", "mutator.indent_prefix"],
        },
        FamilyRecord {
            family: Family::ModuleSurface,
            renders_closed_blocks: false,
            dispositions: &["generator.module_surface", "mutator.trailing_comment"],
        },
        FamilyRecord {
            family: Family::ConditionalBlock,
            renders_closed_blocks: true,
            dispositions: &[
                "generator.conditional_block",
                "mutator.keyword_gap",
                "mutator.block_tail",
            ],
        },
        FamilyRecord {
            family: Family::LoopBlock,
            renders_closed_blocks: true,
            dispositions: &["generator.loop_block", "mutator.keyword_gap"],
        },
        FamilyRecord {
            family: Family::ForEachBlock,
            renders_closed_blocks: true,
            dispositions: &["generator.for_each_block", "mutator.block_tail"],
        },
        FamilyRecord {
            family: Family::CStyleForBlock,
            renders_closed_blocks: true,
            dispositions: &["generator.c_style_for_block", "mutator.keyword_gap"],
        },
        FamilyRecord {
            family: Family::SubroutineBlock,
            renders_closed_blocks: true,
            dispositions: &["generator.subroutine_block", "mutator.block_tail"],
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
    family_registry().iter().find(|record| record.family == family).unwrap_or(&MISSING)
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

// ── Deterministic randomness ────────────────────────────────────────────────

/// SplitMix64: a pure, platform-stable mixing function so `(seed, index)` is
/// the only input to generation (FPH-007).
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn pick(&mut self, bound: usize) -> usize {
        if bound == 0 { 0 } else { (self.next_u64() % bound as u64) as usize }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }
}

// ── Generation ──────────────────────────────────────────────────────────────

/// Generate one deterministic valid case from `(seed, index)`.
///
/// `index` walks the registry so a bounded index range deterministically
/// covers every family and every registered disposition (FPH-001); the seed
/// only drives the free axis choices, keeping every draw structured.
pub fn generate_case(seed: u64, index: usize) -> GeneratedCase {
    let registry = family_registry();
    let record = &registry[index % registry.len()];
    let variants = variants_for(record.family);
    let mut rng = SplitMix64::new(seed ^ (index as u64).rotate_left(17));

    let disposition = record.dispositions[(index / registry.len()) % record.dispositions.len()];

    let variant_base = rng.pick(variants.compact.len());
    let use_spaced = !variants.spaced.is_empty() && rng.chance(50);
    let keyword_gap = variants.renders_closed_blocks && rng.chance(50);
    let indent = INDENTS[rng.pick(INDENTS.len())];
    let comment = COMMENTS[rng.pick(COMMENTS.len())];

    let line_count = if variants.renders_closed_blocks { 1 } else { 1 + rng.pick(3) };

    let mut lines: Vec<String> = Vec::with_capacity(line_count);
    for offset in 0..line_count {
        let pick = (variant_base + offset) % variants.compact.len();
        let raw = if use_spaced { variants.spaced[pick] } else { variants.compact[pick] };
        let mut line = String::from(indent);
        line.push_str(&render_keyword_gap(raw, keyword_gap));
        if offset + 1 == line_count {
            line.push_str(comment);
        }
        lines.push(line);
    }

    let line_ending = pick_line_ending(&mut rng);
    let final_newline = if rng.chance(60) {
        FinalNewline::Preserve
    } else if rng.chance(50) {
        FinalNewline::Insert
    } else {
        FinalNewline::Trim
    };
    let text = compose_text(&lines, line_ending);

    let range_eligible = !variants.renders_closed_blocks
        && line_ending != LineEndingKind::BareCr
        && final_newline == FinalNewline::Preserve;
    let target = if range_eligible && rng.chance(45) {
        TargetRequest::Range { range: full_document_range(&text) }
    } else {
        TargetRequest::Document
    };

    GeneratedCase {
        seed,
        schema_version: HARNESS_SCHEMA_VERSION,
        family: record.family,
        disposition,
        subject: Subject { text, clean_parse_by_construction: true },
        target,
        profile: Profile {
            keyword_spacing: pick_keyword_spacing(&mut rng),
            brace_placement: pick_brace_placement(&mut rng),
            final_newline,
            line_ending,
        },
        expects_refusal: false,
    }
}

/// Generate one deterministic deliberately-invalid case from `(seed, index)`.
/// Such cases must map only to typed refusals or not-proven outcomes
/// (FPH-005); they never feed the idempotence or application invariants.
pub fn generate_invalidation_case(seed: u64, index: usize) -> GeneratedCase {
    let mut rng = SplitMix64::new(seed ^ ((index as u64).rotate_left(17) ^ 0x5A5A_5A5A_5A5A_5A5A));
    let base = FAMILY_TABLE[rng.pick(4)].compact[rng.pick(3)];
    let base = base.to_string();
    let kind = index % 5;

    let (text, family, target, disposition, clean_parse) = match kind {
        0 => (
            "my $x = ;\n".to_string(),
            Family::LexicalDeclaration,
            TargetRequest::Document,
            "mutator.invalidation.truncated_initializer",
            false,
        ),
        1 => (
            format!("{base};;\n"),
            Family::LexicalDeclaration,
            TargetRequest::Document,
            "mutator.invalidation.empty_statement",
            true,
        ),
        2 => (
            format!("{base}\nmy$re=qr{{x}};\n"),
            Family::PlainAssignment,
            TargetRequest::Document,
            "mutator.invalidation.regex_injection",
            false,
        ),
        3 => (
            format!("{base}\n__END__\n"),
            Family::ModuleSurface,
            TargetRequest::Document,
            "mutator.invalidation.data_marker",
            false,
        ),
        _ => {
            let text =
                compose_text(&[base.to_string(), "my$flag=1;".to_string()], LineEndingKind::Lf);
            let target = TargetRequest::Range {
                range: TextRange::new(
                    perl_lsp_perltidy::native::TextPosition::new(1, 0),
                    perl_lsp_perltidy::native::TextPosition::new(0, 0),
                ),
            };
            (text, Family::PlainAssignment, target, "mutator.invalidation.inverted_range", true)
        }
    };

    GeneratedCase {
        seed,
        schema_version: HARNESS_SCHEMA_VERSION,
        family,
        disposition,
        subject: Subject { text, clean_parse_by_construction: clean_parse },
        target,
        profile: Profile {
            keyword_spacing: KeywordSpacing::Space,
            brace_placement: BracePlacement::SameLine,
            final_newline: FinalNewline::Preserve,
            line_ending: LineEndingKind::Lf,
        },
        expects_refusal: true,
    }
}

fn pick_line_ending(rng: &mut SplitMix64) -> LineEndingKind {
    match rng.pick(4) {
        0 => LineEndingKind::Lf,
        1 => LineEndingKind::Crlf,
        2 => LineEndingKind::BareCr,
        _ => LineEndingKind::Mixed,
    }
}

fn pick_keyword_spacing(rng: &mut SplitMix64) -> KeywordSpacing {
    if rng.chance(50) { KeywordSpacing::Space } else { KeywordSpacing::Compact }
}

fn pick_brace_placement(rng: &mut SplitMix64) -> BracePlacement {
    if rng.chance(50) { BracePlacement::SameLine } else { BracePlacement::NextLine }
}

/// Optionally widen `keyword(` to `keyword (` inside block one-liners so the
/// source-side keyword-gap mutator varies independently of the configuration
/// axis of the same name.
fn render_keyword_gap(line: &str, gap: bool) -> String {
    if !gap {
        return line.to_string();
    }
    let mut widened = line.to_string();
    for keyword in ["if", "unless", "while", "until", "foreach"] {
        widened = widened.replace(&format!("{keyword}("), &format!("{keyword} ("));
    }
    widened
}

fn compose_text(lines: &[String], ending: LineEndingKind) -> String {
    let mut text = String::new();
    for (offset, line) in lines.iter().enumerate() {
        text.push_str(line);
        match ending {
            LineEndingKind::Lf => text.push('\n'),
            LineEndingKind::Crlf => text.push_str("\r\n"),
            LineEndingKind::BareCr => {
                if offset + 1 < lines.len() {
                    text.push('\r');
                }
            }
            LineEndingKind::Mixed => {
                if offset + 1 < lines.len() {
                    if offset % 2 == 0 {
                        text.push_str("\r\n");
                    } else {
                        text.push('\n');
                    }
                }
            }
        }
    }
    text
}

/// Independent UTF-16 geometry table over the exact subject bytes: line count
/// plus per-line UTF-16 content lengths, treating `\r\n`, bare `\r`, and `\n`
/// as separators (matching the #8048 true-EOF semantics without reusing any
/// production range constructor).
fn utf16_line_table(text: &str) -> (u32, Vec<u32>) {
    let mut lengths: Vec<u32> = Vec::new();
    let mut current: u32 = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                lengths.push(current);
                current = 0;
            }
            '\n' => {
                lengths.push(current);
                current = 0;
            }
            other => current += other.len_utf16() as u32,
        }
    }
    lengths.push(current);
    (lengths.len() as u32, lengths)
}

fn full_document_range(text: &str) -> TextRange {
    let (line_count, lengths) = utf16_line_table(text);
    let last = line_count.saturating_sub(1);
    TextRange::new(
        perl_lsp_perltidy::native::TextPosition::new(0, 0),
        perl_lsp_perltidy::native::TextPosition::new(last, lengths[last as usize]),
    )
}

fn line_in_scope(range: &TextRange, line: u32) -> bool {
    line >= range.start.line
        && (line < range.end.line || (line == range.end.line && range.end.character > 0))
}

// ── Receipt naming ──────────────────────────────────────────────────────────

fn disposition_name(disposition: FormatDisposition) -> &'static str {
    match disposition {
        FormatDisposition::Applied => "applied",
        FormatDisposition::NoChange => "no_change",
        FormatDisposition::Refused => "refused",
        FormatDisposition::FailedOrNotProven => "failed_or_not_proven",
    }
}

fn reason_name(reason: FormatReasonCode) -> &'static str {
    match reason {
        FormatReasonCode::Applied => "applied",
        FormatReasonCode::AlreadyFormatted => "already_formatted",
        FormatReasonCode::FormatterDisabled => "formatter_disabled",
        FormatReasonCode::UnsupportedSyntax => "unsupported_syntax",
        FormatReasonCode::LiteralPreservationUnsupported => "literal_preservation_unsupported",
        FormatReasonCode::SourceParseError => "source_parse_error",
        FormatReasonCode::FormattedOutputParseError => "formatted_output_parse_error",
        FormatReasonCode::UnsafeRange => "unsafe_range",
        FormatReasonCode::StaleSource => "stale_source",
        FormatReasonCode::InvalidConfiguration => "invalid_configuration",
        FormatReasonCode::InstrumentFailure => "instrument_failure",
    }
}

const REFUSAL_REASON_CLASSES: [FormatReasonCode; 9] = [
    FormatReasonCode::FormatterDisabled,
    FormatReasonCode::UnsupportedSyntax,
    FormatReasonCode::LiteralPreservationUnsupported,
    FormatReasonCode::SourceParseError,
    FormatReasonCode::FormattedOutputParseError,
    FormatReasonCode::UnsafeRange,
    FormatReasonCode::StaleSource,
    FormatReasonCode::InvalidConfiguration,
    FormatReasonCode::InstrumentFailure,
];

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

fn digest_hex(value: u64) -> String {
    format!("{value:016x}")
}

// ── The mandatory invariant checker ─────────────────────────────────────────

/// Run the full invariant checker for one case and return its receipt.
///
/// The checker computes the formatting twice from fresh formatter contexts
/// and enforces, in order: bounded generation, deterministic typed outcomes,
/// applied-plan exactness through the independent oracle, second-pass
/// idempotence, refusal plan-emptiness with exact reason classes, line-ending
/// survival, and UTF-16 geometry validity. Every failure is a typed
/// [`Violation`]; the checker never panics and never reads a clock.
pub fn run_case(case: &GeneratedCase) -> Result<CaseReceipt, Violation> {
    check_bounds(case)?;
    let source = case.subject.text.as_str();
    let config = config_for(case);

    // Two fresh formatter contexts over identical inputs (FPH-002).
    let first = NativeFormatter::new();
    let second = NativeFormatter::new();
    let typed_one = format_with(&first, case, source, &config);
    let typed_two = format_with(&second, case, source, &config);
    if typed_one.result != typed_two.result || typed_one.outcome != typed_two.outcome {
        return Err(Violation {
            rule: "determinism.typed_outcome",
            detail: format!(
                "two fresh runs of (seed {}, index of family {}) diverged",
                case.seed,
                case.family.name()
            ),
        });
    }

    let result = &typed_one.result;
    let outcome = &typed_one.outcome;
    let outcome_disposition = disposition_name(outcome.disposition);
    let outcome_reason = reason_name(outcome.reason);
    let plan_edit_count = result.edits.len();

    if plan_edit_count > MAX_PLAN_EDITS {
        return Err(Violation {
            rule: "generation.bounded_plan",
            detail: format!("plan carries {plan_edit_count} edits, bound is {MAX_PLAN_EDITS}"),
        });
    }

    let (applied_application_verified, plan_ordering_verified, utf16_geometry_verified) =
        match outcome.disposition {
            FormatDisposition::Applied => {
                if plan_edit_count == 0 {
                    return Err(Violation {
                        rule: "plan.applied_without_edits",
                        detail: "applied outcome carries an empty plan".to_string(),
                    });
                }
                check_plan_ordering_and_geometry(source, result, case)?;
                if apply_edits_independently(source, result)? == result.formatted {
                    (true, true, true)
                } else {
                    return Err(Violation {
                        rule: "plan.independent_application",
                        detail: "independently applied plan does not equal the rendered bytes"
                            .to_string(),
                    });
                }
            }
            FormatDisposition::NoChange => {
                if plan_edit_count != 0 {
                    return Err(Violation {
                        rule: "nochange.plan_empty",
                        detail: format!("no-change outcome carries {plan_edit_count} edits"),
                    });
                }
                (false, true, true)
            }
            FormatDisposition::Refused | FormatDisposition::FailedOrNotProven => {
                if plan_edit_count != 0 {
                    return Err(Violation {
                        rule: "refusal.plan_empty",
                        detail: format!(
                            "{outcome_disposition} outcome carries {plan_edit_count} edits"
                        ),
                    });
                }
                if !REFUSAL_REASON_CLASSES.contains(&outcome.reason) {
                    return Err(Violation {
                        rule: "refusal.reason_class",
                        detail: format!("{outcome_reason} is not a stable refusal class"),
                    });
                }
                (false, true, true)
            }
        };

    if case.expects_refusal
        && !matches!(
            outcome.disposition,
            FormatDisposition::Refused | FormatDisposition::FailedOrNotProven
        )
    {
        return Err(Violation {
            rule: "refusal.invalid_subject",
            detail: format!("deliberately invalid subject produced {outcome_disposition}"),
        });
    }

    // Line-ending survival: asserted for every generated convention except
    // two honest carve-outs, each a registered fail-closed dormant slot
    // (FPH-008) rather than a vacuous pass:
    //   - bare CR: today's tree drops the bare CR of a parsed single-line
    //     subject while reporting `Applied`;
    //   - CRLF-only subjects rendered through a block family: the inserted
    //     wrap lines are always LF, changing the convention set (evidence
    //     honestly reports ChangedByFormatter).
    // The Insert/Trim final-newline policies own the final terminator by
    // contract, so policy-driven `ChangedByFormatter` evidence is recorded
    // rather than treated as a violation either.
    let line_endings_preserved =
        matches!(outcome.safety.line_endings, FormatLineEndingDisposition::Preserved);
    let policy_owns_terminator = case.profile.final_newline != FinalNewline::Preserve;
    let bare_cr_subject = case.profile.line_ending == LineEndingKind::BareCr;
    let wrap_inserts_foreign_separator = record_for(case.family).renders_closed_blocks
        && (case.profile.line_ending == LineEndingKind::Crlf || !case.subject.text.contains('\n'));
    if matches!(outcome.safety.line_endings, FormatLineEndingDisposition::NotChecked) {
        return Err(Violation {
            rule: "safety.line_endings_evidence",
            detail: "line-ending evidence was not checked for a generated subject".to_string(),
        });
    }
    if !policy_owns_terminator
        && !bare_cr_subject
        && !wrap_inserts_foreign_separator
        && !line_endings_preserved
    {
        return Err(Violation {
            rule: "safety.line_endings",
            detail: format!(
                "line endings not preserved for the {} convention",
                case.profile.line_ending.name()
            ),
        });
    }

    // Mandatory second pass from a fresh context (FPH-004).
    let second_pass = if matches!(
        outcome.disposition,
        FormatDisposition::Applied | FormatDisposition::NoChange
    ) && !case.expects_refusal
    {
        let third = NativeFormatter::new();
        let typed_pass = format_with(&third, case, &result.formatted, &config);
        let pass_disposition = disposition_name(typed_pass.outcome.disposition);
        let pass_edits = typed_pass.result.edits.len();
        let bytes_stable = typed_pass.result.formatted == result.formatted;

        if pass_edits != 0 {
            return Err(Violation {
                rule: "idempotence.no_further_edits",
                detail: format!("second pass carries {pass_edits} edits"),
            });
        }
        if !bytes_stable {
            return Err(Violation {
                rule: "idempotence.stable_bytes",
                detail: "second pass rendered different bytes".to_string(),
            });
        }
        if matches!(
            typed_pass.outcome.disposition,
            FormatDisposition::Applied | FormatDisposition::FailedOrNotProven
        ) {
            return Err(Violation {
                rule: "idempotence.not_applied",
                detail: format!("second pass reported {pass_disposition}"),
            });
        }
        let record = record_for(case.family);
        // Bare-CR subjects live entirely in the dormant line-ending domain:
        // an Insert pass over bare-CR text produces `\r`-inside-`\n` lines
        // that the safe-subset line admission does not cover, so their second
        // pass may legitimately refuse instead of classify as formatted.
        let bare_cr_subject = case.profile.line_ending == LineEndingKind::BareCr;
        if !bare_cr_subject && !record.renders_closed_blocks && pass_disposition != "no_change" {
            return Err(Violation {
                rule: "idempotence.already_formatted",
                detail: format!(
                    "line-level family must classify as already-formatted, got {pass_disposition}"
                ),
            });
        }
        if !bare_cr_subject
            && record.renders_closed_blocks
            && pass_disposition != "no_change"
            && pass_disposition != "refused"
        {
            return Err(Violation {
                rule: "idempotence.stabilize_or_refuse",
                detail: format!("rendered-block family second pass reported {pass_disposition}"),
            });
        }
        Some(SecondPassObservation {
            disposition: pass_disposition,
            edit_count: pass_edits,
            bytes_stable,
        })
    } else {
        None
    };

    let target_name = match case.target {
        TargetRequest::Document => "document",
        TargetRequest::Range { .. } => "range",
    };
    let normalized = format!(
        "fph|schema={}|seed={}|family={}|disposition={}|target={}|line_ending={}|subject={}|ks={:?}|bp={:?}|fn={:?}|outcome={}/{}|plan=edits:{}|application={}|ordering={}|geometry={}|line_endings={}|second_pass={}",
        case.schema_version,
        case.seed,
        case.family.name(),
        case.disposition,
        target_name,
        case.profile.line_ending.name(),
        digest_hex(fnv1a64(source.as_bytes())),
        case.profile.keyword_spacing,
        case.profile.brace_placement,
        case.profile.final_newline,
        outcome_disposition,
        outcome_reason,
        plan_edit_count,
        applied_application_verified,
        plan_ordering_verified,
        utf16_geometry_verified,
        line_endings_preserved,
        second_pass
            .as_ref()
            .map(|pass| format!("{}/{}", pass.disposition, pass.edit_count))
            .unwrap_or_else(|| "none".to_string()),
    );

    Ok(CaseReceipt {
        seed: case.seed,
        schema_version: case.schema_version,
        family: case.family.name(),
        disposition: case.disposition,
        target: target_name,
        line_ending: case.profile.line_ending.name(),
        outcome_disposition,
        outcome_reason,
        plan_edit_count,
        applied_application_verified,
        plan_ordering_verified,
        utf16_geometry_verified,
        line_endings_preserved,
        second_pass,
        digest: format!("fph-receipt:{}", digest_hex(fnv1a64(normalized.as_bytes()))),
        normalized,
    })
}

fn check_bounds(case: &GeneratedCase) -> Result<(), Violation> {
    if case.subject.text.len() > MAX_SUBJECT_BYTES {
        return Err(Violation {
            rule: "generation.bounded_subject",
            detail: format!(
                "subject is {} bytes, bound is {MAX_SUBJECT_BYTES}",
                case.subject.text.len()
            ),
        });
    }
    if case.subject.text.lines().count() > MAX_SUBJECT_LINES {
        return Err(Violation {
            rule: "generation.bounded_subject",
            detail: "subject exceeds the composed line bound".to_string(),
        });
    }
    Ok(())
}

fn config_for(case: &GeneratedCase) -> perl_lsp_perltidy::native::FormatConfig {
    perl_lsp_perltidy::native::FormatConfig {
        mode: perl_lsp_perltidy::native::FormatterMode::Native,
        line_width: 100,
        indent_width: 4,
        use_tabs: false,
        final_newline: case.profile.final_newline,
        trailing_comma: perl_lsp_perltidy::native::TrailingComma::Preserve,
        brace_placement: case.profile.brace_placement,
        else_placement: perl_lsp_perltidy::native::ElsePlacement::Cuddled,
        keyword_spacing: case.profile.keyword_spacing,
    }
}

fn format_with(
    formatter: &NativeFormatter,
    case: &GeneratedCase,
    source: &str,
    config: &perl_lsp_perltidy::native::FormatConfig,
) -> perl_lsp_perltidy::native::TypedFormatResult {
    let context = FormatContext::default();
    match case.target {
        TargetRequest::Document => formatter.format_document_typed(source, config, &context),
        TargetRequest::Range { range } => {
            formatter.format_range_typed(source, range, config, &context)
        }
    }
}

/// Order, overlap, target-containment, and UTF-16 geometry checks over the
/// produced plan, implemented independently of production geometry code. Any
/// violation is returned directly; success certifies ordering and geometry.
fn check_plan_ordering_and_geometry(
    source: &str,
    result: &perl_lsp_perltidy::native::FormatResult,
    case: &GeneratedCase,
) -> Result<(), Violation> {
    let (line_count, lengths) = utf16_line_table(source);
    let mut previous_end = (0_u32, 0_u32);

    for (index, edit) in result.edits.iter().enumerate() {
        let start = (edit.range.start.line, edit.range.start.character);
        let end = (edit.range.end.line, edit.range.end.character);
        if start > end {
            return Err(Violation {
                rule: "plan.reversed_range",
                detail: format!("edit {index} has a reversed range"),
            });
        }
        if index > 0 && start < previous_end {
            return Err(Violation {
                rule: "plan.ordering_or_overlap",
                detail: format!("edit {index} overlaps or precedes its predecessor"),
            });
        }
        previous_end = end;

        if start.0 >= line_count || end.0 >= line_count {
            return Err(Violation {
                rule: "geometry.utf16_range",
                detail: format!("edit {index} targets a line beyond the subject geometry"),
            });
        }
        if start.1 > lengths[start.0 as usize] || end.1 > lengths[end.0 as usize] {
            return Err(Violation {
                rule: "geometry.utf16_range",
                detail: format!("edit {index} targets a character beyond its line"),
            });
        }

        let contained = match &case.target {
            TargetRequest::Document => true,
            TargetRequest::Range { range } => {
                line_in_scope(range, start.0) && line_in_scope(range, end.0)
            }
        };
        if !contained {
            return Err(Violation {
                rule: "plan.target_containment",
                detail: format!(
                    "edit {index} escapes the requested target and no widening is recorded"
                ),
            });
        }
    }

    Ok(())
}

fn apply_edits_independently(
    source: &str,
    result: &perl_lsp_perltidy::native::FormatResult,
) -> Result<String, Violation> {
    let specs: Vec<EditSpec> = result
        .edits
        .iter()
        .map(|edit| {
            EditSpec::new(
                edit.range.start.line,
                edit.range.start.character,
                edit.range.end.line,
                edit.range.end.character,
                edit.new_text.clone(),
            )
        })
        .collect();
    apply_edits_exact(source, &specs, PositionEncoding::Utf16CodeUnits).map_err(|error| Violation {
        rule: "plan.independent_application",
        detail: format!("independent application rejected the plan: {error}"),
    })
}

// ── Dormant invariants (FPH-008) ────────────────────────────────────────────

/// A gated invariant whose oracle does not exist on today's tree. Slots fail
/// closed: they report not-proven instead of passing vacuously, and convert
/// into real assertions when their owning issues land the gate mechanism.
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
    /// The gate landed and the property is asserted for real. Constructed
    /// only when a slot's owning issue lands its in-tree gate mechanism; on
    /// today's tree no slot converts, so this variant is intentionally
    /// unconstructed.
    #[allow(dead_code)]
    Proven,
}

impl DormantInvariant {
    /// Report the current status. Fail-closed on today's tree: every slot is
    /// not-proven until its gate mechanism exists in-tree.
    pub fn status(&self) -> DormantStatus {
        DormantStatus::NotProven
    }
}

/// The dormant-invariant registry.
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
        DormantInvariant {
            id: "strict_second_pass_typed_idempotence_for_rendered_blocks",
            gate: "already-formatted classification admits rendered closing-brace lines",
            owning_issues: &["10301"],
        },
        DormantInvariant {
            id: "bare_cr_line_ending_preservation",
            gate: "applied formats preserve the bare-CR convention of parsed subjects",
            owning_issues: &["8048"],
        },
        DormantInvariant {
            id: "wrap_line_separators_follow_source_convention",
            gate: "block rendering inserts separators using the source's existing convention(s); today inserted wrap lines are always LF, changing the convention set of CRLF-only subjects",
            owning_issues: &["8048"],
        },
    ]
}
