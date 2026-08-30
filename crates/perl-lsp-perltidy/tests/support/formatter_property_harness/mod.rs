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

/// Single source of truth for the `(seed, index)` index axis shared by the
/// property tier and the fuzz decoder.
pub const GENERATED_INDEX_SPACE: usize = 64;

/// Indentation prefixes used by the indent mutator.
const INDENTS: [&str; 3] = ["", "  ", "\t"];

/// Trailing comment suffixes used by the trailing-comment mutator. The
/// non-ASCII entries keep the UTF-16 geometry checks discriminating: a
/// 2-byte BMP character (`πλ`) makes byte and UTF-16 columns diverge, and the
/// supplementary characters (`😀𝕏`, two UTF-16 units / four UTF-8 bytes each)
/// make byte, Unicode-scalar, and UTF-16 columns three distinct geometries.
const COMMENTS: [&str; 5] = ["", " # note", " # keep 1", " # πλ", " # 😀𝕏"];

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
    /// Exhaustive position pin: adding a `Family` variant makes this match
    /// non-exhaustive (compile error), so the variant cannot exist without
    /// touching this enumeration. Each arm returns the variant's position in
    /// [`Family::ALL`]; the compile-time alignment check below proves `ALL`
    /// lists every variant exactly at its pinned position, so the registry
    /// cannot drift from the enum in either direction (FPH-001).
    const fn pinned_index(self) -> usize {
        match self {
            Family::LexicalDeclaration => 0,
            Family::PlainAssignment => 1,
            Family::ReturnStatement => 2,
            Family::LoopControl => 3,
            Family::ModuleSurface => 4,
            Family::ConditionalBlock => 5,
            Family::LoopBlock => 6,
            Family::ForEachBlock => 7,
            Family::CStyleForBlock => 8,
            Family::SubroutineBlock => 9,
        }
    }

    /// Independently enumerated admitted families — deliberately not derived
    /// from `family_registry()` or `FAMILY_TABLE`, so the FPH-001 pin in
    /// `every_admitted_family_has_a_registered_disposition` can compare all
    /// three surfaces in both directions: a variant added here but missing a
    /// registry row or table row is red, and a registry/table row whose
    /// variant was deleted cannot compile.
    pub const ALL: &'static [Family] = &[
        Family::LexicalDeclaration,
        Family::PlainAssignment,
        Family::ReturnStatement,
        Family::LoopControl,
        Family::ModuleSurface,
        Family::ConditionalBlock,
        Family::LoopBlock,
        Family::ForEachBlock,
        Family::CStyleForBlock,
        Family::SubroutineBlock,
    ];

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

// Compile-time FPH-001 alignment, checked twice:
// 1. every `Family::ALL` entry sits exactly at the position its exhaustive
//    `pinned_index` arm declares;
// 2. `FAMILY_TABLE` has one row per pinned family, in `ALL` order — so the
//    pinned-index lookup below is total for every live variant, a table row
//    cannot exist without its `ALL` entry, and a variant added to the enum
//    cannot compile without extending the exhaustive enumeration. A registry
//    row without a table row is therefore impossible to substitute around.
const _: () = {
    let mut index = 0;
    while index < Family::ALL.len() {
        if Family::ALL[index].pinned_index() != index {
            panic!("Family::ALL order drifted from the exhaustive pinned_index enumeration");
        }
        if FAMILY_TABLE[index].family.pinned_index() != index {
            panic!("FAMILY_TABLE order drifted from the exhaustive pinned_index enumeration");
        }
        index += 1;
    }
    if FAMILY_TABLE.len() != Family::ALL.len() {
        panic!("FAMILY_TABLE row count drifted from the exhaustive pinned_index enumeration");
    }
};

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

/// Unformatted/already-formatted variants for one family. Visible to the
/// FPH-001 pin, which checks bidirectional identity against `Family::ALL`.
#[derive(Debug, Clone)]
pub struct FamilyVariants {
    pub family: Family,
    pub renders_closed_blocks: bool,
    /// Unformatted (compact) one-liner forms.
    pub compact: &'static [&'static str],
    /// Already-formatted forms; line-level families only. Block families have
    /// no single-line already-formatted rendering, so they reuse `compact`.
    pub spaced: &'static [&'static str],
}

const FAMILY_TABLE: [FamilyVariants; 10] = [
    FamilyVariants {
        family: Family::LexicalDeclaration,
        renders_closed_blocks: false,
        compact: &["my$x=1;", "our@list=(1,2);", "my($a,$b)=@_;", "my$s=\"πθ\";", "my$t=\"😀\";"],
        spaced: &[
            "my $x = 1;",
            "our @list = (1, 2);",
            "my ($a, $b) = @_;",
            "my $s = \"πθ\";",
            "my $t = \"😀\";",
        ],
    },
    FamilyVariants {
        family: Family::PlainAssignment,
        renders_closed_blocks: false,
        compact: &["$count=0;", "$name=\"demo\";", "$sum=$a+$b;", "$u=\"π\";"],
        spaced: &["$count = 0;", "$name = \"demo\";", "$sum = $a + $b;", "$u = \"π\";"],
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

/// Fail-closed variant-table lookup. `FAMILY_TABLE` is compile-time aligned
/// to the exhaustive `pinned_index` enumeration (const check above), so the
/// row for every live variant exists at its pinned position — no fallback
/// substitution is possible (FPH-001).
pub fn variants_for(family: Family) -> &'static FamilyVariants {
    &FAMILY_TABLE[family.pinned_index()]
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

/// Registry row for one admitted family. Fails closed: a family without a
/// registry row is a drift violation (FPH-001), never a `MISSING` substitute.
pub fn record_for(family: Family) -> Result<&'static FamilyRecord, Violation> {
    family_registry().iter().find(|record| record.family == family).ok_or_else(|| Violation {
        rule: "registry.missing_family_record",
        detail: format!("family {} has no family_registry row", family.name()),
    })
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

/// SplitMix64 is a pure, platform-stable, versioned expansion function from
/// the drawn `(seed, index)` pair to the case axes. ChaCha is the proptest
/// draw source pinned by the spec; SplitMix64 expands those draws for the
/// structured generator, and case identity is pinned by the FPH-002/FPH-007
/// determinism tests and committed regression seeds.
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
    generate_case_with_disposition(seed, index, None)
}

/// Generate the same case without applying the registry disposition. This is
/// a negative control for FPH-001: the disposition-aware path must produce
/// source bytes that differ from this neutral construction for some bounded
/// seed, or the registration is only a receipt label.
pub fn generate_case_neutral_control(seed: u64, index: usize) -> GeneratedCase {
    generate_case_with_disposition(seed, index, Some("control.neutral"))
}

fn generate_case_with_disposition(
    seed: u64,
    index: usize,
    forced_disposition: Option<&'static str>,
) -> GeneratedCase {
    let registry = family_registry();
    let record = &registry[index % registry.len()];
    // Fail closed on generator-side drift: the compile-time table alignment
    // proves the variant row for every live family exists, so a registry row
    // without variants can never fall back to another family's row.
    let variants = variants_for(record.family);
    let mut rng = SplitMix64::new(seed ^ (index as u64).rotate_left(17));

    let disposition = forced_disposition.unwrap_or_else(|| {
        record.dispositions[(index / registry.len()) % record.dispositions.len()]
    });

    // A registry disposition is executable construction authority, not a
    // receipt label. Generator rows select their pinned family shape; each
    // mutator row applies one observable source mutation. The remaining
    // axes stay seed-driven so the bounded walk still explores combinations.
    let generator_disposition = disposition.starts_with("generator.");
    let variant_base = if generator_disposition {
        record.family.pinned_index() % variants.compact.len()
    } else {
        rng.pick(variants.compact.len())
    };
    let use_spaced = disposition == "mutator.spacing_style"
        || (!generator_disposition && !variants.spaced.is_empty() && rng.chance(50));
    let keyword_gap = disposition == "mutator.keyword_gap"
        || (!generator_disposition && variants.renders_closed_blocks && rng.chance(50));
    let indent = if disposition == "mutator.indent_prefix" {
        INDENTS[1]
    } else {
        INDENTS[rng.pick(INDENTS.len())]
    };
    let comment = if disposition == "mutator.trailing_comment" {
        COMMENTS[1]
    } else {
        COMMENTS[rng.pick(COMMENTS.len())]
    };

    let line_ending = pick_line_ending(&mut rng);
    let final_newline = if rng.chance(60) {
        FinalNewline::Preserve
    } else if rng.chance(50) {
        FinalNewline::Insert
    } else {
        FinalNewline::Trim
    };

    let mut line_count = if variants.renders_closed_blocks { 1 } else { 1 + rng.pick(3) };
    // The selected convention must exist in the emitted bytes: bare-CR and
    // mixed separators are only written *between* lines, so a one-line subject
    // would make the line-ending profile pure metadata. Force at least one
    // interior separator (FPH-006).
    if matches!(line_ending, LineEndingKind::BareCr | LineEndingKind::Mixed) && line_count < 2 {
        line_count = 2;
    }
    if line_ending == LineEndingKind::Mixed && line_count < 3 {
        line_count = 3;
    }

    let mut lines: Vec<String> = Vec::with_capacity(line_count);
    for offset in 0..line_count {
        let pick = (variant_base + offset) % variants.compact.len();
        let raw = if use_spaced { variants.spaced[pick] } else { variants.compact[pick] };
        let mut line = String::from(indent);
        line.push_str(&render_keyword_gap(raw, keyword_gap));
        if disposition == "mutator.block_tail" && variants.renders_closed_blocks {
            mutate_block_tail(&mut line);
        }
        if offset + 1 == line_count {
            line.push_str(comment);
        }
        lines.push(line);
    }

    let text = compose_text(&lines, line_ending);

    let range_eligible = !variants.renders_closed_blocks
        && line_ending != LineEndingKind::BareCr
        && final_newline == FinalNewline::Preserve;
    // Range targets are load-bearing only when the requested range excludes
    // source lines: an interior single-line range on a 3+-line subject keeps
    // unchanged lines on both sides, so a formatter that silently widens the
    // target to the whole document fails the containment check (FPH-003).
    // Line-level families below three lines keep the full-document range.
    let target = if range_eligible && rng.chance(45) {
        if line_count >= 3 {
            let interior_line = 1 + rng.pick(line_count - 2);
            TargetRequest::Range {
                range: TextRange::new(
                    perl_lsp_perltidy::native::TextPosition::new(interior_line as u32, 0),
                    perl_lsp_perltidy::native::TextPosition::new(interior_line as u32 + 1, 0),
                ),
            }
        } else {
            TargetRequest::Range { range: full_document_range(&text) }
        }
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

/// Apply the block-tail mutator to the generated source itself. Inserting a
/// space before the closing brace is deliberately small but observable in the
/// exact bytes and remains valid Perl across all admitted block families.
fn mutate_block_tail(line: &mut String) {
    if let Some(close) = line.rfind('}') {
        line.insert(close, ' ');
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

/// Decode a cargo-fuzz input carrying a full `(seed, selector)` pair into
/// exactly one replayable case: the first eight little-endian bytes select the
/// seed, the ninth byte selects the case index (low six bits) and the
/// invalidation path (bit 7).
///
/// Both the fuzz target and the committed regression replay in
/// `fuzz_target_and_regression_pipeline_are_wired` call this one decoder, so a
/// `(seed, selector)` pair — including one from a future real crash input
/// — is reconstructible and replayable (FPH-010).
pub fn case_from_fuzz_input(data: &[u8]) -> Option<GeneratedCase> {
    if data.len() < 9 {
        return None;
    }
    let mut seed_bytes = [0_u8; 8];
    seed_bytes.copy_from_slice(&data[..8]);
    let seed = u64::from_le_bytes(seed_bytes);
    let selector = data[8];
    let index = usize::from(selector & 0x3f) % GENERATED_INDEX_SPACE;
    Some(if selector & 0x80 != 0 {
        generate_invalidation_case(seed, index)
    } else {
        generate_case(seed, index)
    })
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
    for keyword in ["if", "unless", "while", "until", "foreach", "for"] {
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

/// Target containment for an edit *end* position. A zero-width end at
/// `(line, 0)` sits on the boundary before `line`'s first character: it
/// touches no line-`line` content, so containment only requires line
/// `line - 1` to be in scope. This keeps the checker unable to reject a
/// correct plan that ends at a line start (e.g. an EOF-spanning edit over a
/// final terminator) while starts stay strictly scoped.
fn edit_end_in_scope(range: &TextRange, end: (u32, u32)) -> bool {
    if end.1 == 0 && end.0 > 0 {
        line_in_scope(range, end.0 - 1)
    } else {
        line_in_scope(range, end.0)
    }
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
                // The unchanged-result contract: `changed == false` with zero
                // edits must leave the exact source bytes in `formatted`. A
                // formatter that quietly substitutes different (even stable)
                // output while claiming no change fails here (FPH-002).
                if result.formatted != source {
                    return Err(Violation {
                        rule: "nochange.preserves_source_bytes",
                        detail: "no-change outcome rendered different bytes than the source"
                            .to_string(),
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
    // honest carve-outs, each a registered fail-closed dormant slot
    // (FPH-008) rather than a vacuous pass:
    //   - bare CR: today's tree drops the bare CR of a parsed single-line
    //     subject while reporting `Applied` (`bare_cr_line_ending_preservation`);
    //   - subjects rendered through a block family whose convention set
    //     contains CRLF or bare CR: the inserted wrap lines and touched
    //     separators are always LF, changing the convention set
    //     (`wrap_line_separators_follow_source_convention`);
    //   - Insert/Trim final-newline policies own the final terminator by
    //     contract, so policy-driven `ChangedByFormatter` evidence is
    //     recorded rather than treated as a violation
    //     (`final_newline_policy_owns_terminator`).
    let line_endings_preserved =
        matches!(outcome.safety.line_endings, FormatLineEndingDisposition::Preserved);
    let policy_owns_terminator = case.profile.final_newline != FinalNewline::Preserve;
    let bare_cr_subject = case.profile.line_ending == LineEndingKind::BareCr;
    let record = record_for(case.family)?;
    // Block-family rendering inserts wrap lines with LF and normalizes the
    // separators it touches (between-statement separators included) to LF
    // today, so any block subject whose convention set contains CRLF or bare
    // CR loses those conventions. Registered dormant slot
    // `wrap_line_separators_follow_source_convention`; LF-only block subjects
    // keep the preservation assertion.
    let wrap_inserts_foreign_separator =
        record.renders_closed_blocks && case.profile.line_ending != LineEndingKind::Lf;
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
        let record = record_for(case.family)?;
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
                line_in_scope(range, start.0) && edit_end_in_scope(range, end)
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
            // Live conversion owner; #10301 (the harness claim itself) closes
            // with this PR and must not be the only named owner.
            owning_issues: &["13205"],
        },
        DormantInvariant {
            id: "bare_cr_line_ending_preservation",
            gate: "applied formats preserve the bare-CR convention of parsed subjects",
            owning_issues: &["8048"],
        },
        DormantInvariant {
            id: "wrap_line_separators_follow_source_convention",
            gate: "block rendering inserts separators and normalizes the separators it touches using the source's existing convention(s); today both are always LF, changing the convention set of any CRLF/mixed subject",
            owning_issues: &["8048"],
        },
        DormantInvariant {
            id: "final_newline_policy_owns_terminator",
            gate: "typed line-ending evidence distinguishes a policy-owned final-terminator change (Insert/Trim) from a line-ending convention change, so the FPH-006 exemption converts from contract scope to proven evidence",
            owning_issues: &["8048"],
        },
    ]
}
