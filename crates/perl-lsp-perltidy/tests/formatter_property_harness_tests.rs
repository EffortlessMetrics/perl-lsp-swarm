//! Property/fuzz harness proof for formatter safety invariants (#10301).
//!
//! Rows FPH-001..FPH-010 from `.spec/10301-formatter-property-fuzz-harness/`.
//! The shared invariant core lives in
//! `tests/support/formatter_property_harness/` and is consumed by the
//! property-tier decoder/replay controls. The checker binds only canonical
//! production APIs (`format_*_typed`) and its independent strict byte-edit
//! applicator; it never reuses production edit application, never spawns a
//! process, and never reads a clock. #10301 remains open; this branch lands a
//! bounded subset. The four committed replay controls are predetermined
//! decoder vectors covering valid, invalidation, and index >= 16 paths through
//! `case_from_fuzz_input`; no runtime fuzzing campaign has run, so crash-derived
//! corpus evidence is not claimed.
//!
//! Determinism: every case is a pure function of `(seed, index)`; receipts are
//! normalized and digested without wall-clock input. Boundedness is asserted
//! per case (`MAX_SUBJECT_BYTES`, `MAX_PLAN_EDITS`, `MAX_SUBJECT_LINES`).
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, FileFailurePersistence, RngAlgorithm};

#[path = "support/formatter_property_harness/mod.rs"]
mod formatter_property_harness;

use formatter_property_harness::{
    DormantStatus, FUZZ_INDEX_SPACE, Family, GeneratedCase, HARNESS_SCHEMA_VERSION, LineEndingKind,
    MAX_PLAN_EDITS, MAX_SUBJECT_BYTES, MAX_SUBJECT_LINES, apply_plan_strict,
    body_line_endings_preserved, case_from_fuzz_input, convention_present_in_bytes,
    dormant_registry, family_registry, generate_case, generate_case_neutral_control,
    generate_invalidation_case, record_for, run_case, variants_for,
};
use perl_lsp_perltidy::native::{FinalNewline, TextEdit, TextPosition, TextRange};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const REGRESSION_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/formatter_property_harness_tests.proptest-regressions"
);

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

const PINNED_REPLAY_CONTROLS: [(u64, u8); 4] = [
    (0x0000_0000_0000_0042, 0x2a),
    (0x0102_0304_0506_0708, 0xa5),
    (0xdead_beef_cafe_babe, 0x80),
    (0x0123_4567_89ab_cdef, 0x3f),
];

/// Independent FPH-001 catalog, maintained in this test surface only. The
/// harness module's registry, variant table, and `Family::ALL` are all
/// compared against these literals in both directions, so deleting a family,
/// a disposition, or a table row — or renaming a family — cannot keep the
/// suite green by self-validating a reduced registry.
const PINNED_FAMILY_COUNT: usize = 10;
const PINNED_FAMILY_NAMES: [&str; PINNED_FAMILY_COUNT] = [
    "lexical_declaration",
    "plain_assignment",
    "return_statement",
    "loop_control",
    "module_surface",
    "conditional_block",
    "loop_block",
    "for_each_block",
    "c_style_for_block",
    "subroutine_block",
];
/// 3 + 2 + 2 + 2 + 2 + 3 + 2 + 2 + 2 + 2 registered dispositions.
const PINNED_DISPOSITION_TOTAL: usize = 22;

const EXTERNAL_ORACLE_BANNED_TOKENS: [&str; 14] = [
    "PerlTidyFormatter",
    "with_os_runtime",
    "run_command",
    "std::process",
    "process::Command",
    "Command::new",
    "std::thread",
    "thread::spawn",
    "Instant",
    "SystemTime",
    "apply_edits_exact",
    "EditSpec",
    "PositionEncoding",
    "edit_application",
];

const HARNESS_FORBIDDEN_TOKENS: [&str; 9] = [
    ".unwrap()",
    ".expect(",
    "todo!",
    "unimplemented!",
    "unreachable!",
    "dbg!",
    "unsafe",
    "assert!",
    "assert_eq!",
];

/// Reason classes that legitimately carry no plan (every stable reason except
/// the two success classes).
const REFUSAL_REASON_CLASSES: [&str; 9] = [
    "formatter_disabled",
    "unsupported_syntax",
    "literal_preservation_unsupported",
    "source_parse_error",
    "formatted_output_parse_error",
    "unsafe_range",
    "stale_source",
    "invalid_configuration",
    "instrument_failure",
];

fn harness_proptest_config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSION_FILE))),
        rng_algorithm: RngAlgorithm::ChaCha,
        ..ProptestConfig::default()
    }
}

fn arb_valid_case() -> impl Strategy<Value = GeneratedCase> {
    (any::<u64>(), 0usize..FUZZ_INDEX_SPACE).prop_map(|(seed, index)| generate_case(seed, index))
}

fn arb_invalidation_case() -> impl Strategy<Value = GeneratedCase> {
    (any::<u64>(), 0usize..FUZZ_INDEX_SPACE)
        .prop_map(|(seed, index)| generate_invalidation_case(seed, index))
}

/// FPH-009 source pin: the harness module must never reference the subprocess
/// adapter, process spawning, or a wall clock.
#[test]
fn harness_module_does_not_reference_external_oracle() -> TestResult {
    let harness_source = fs::read_to_string(format!(
        "{MANIFEST_DIR}/tests/support/formatter_property_harness/mod.rs"
    ))?;

    for token in EXTERNAL_ORACLE_BANNED_TOKENS {
        assert!(
            !harness_source.contains(token),
            "harness module must not reference {token} (FPH-009)"
        );
    }
    Ok(())
}

/// The byte-column interpretation must not accidentally pass as UTF-16.
#[test]
fn strict_applicator_rejects_byte_offset_interpretation() -> TestResult {
    let mut selected: Option<(String, u32, u32, u32)> = None;
    'cases: for seed in 0..8_u64 {
        for index in 0..FUZZ_INDEX_SPACE {
            let subject = generate_case(seed, index).subject.text;
            let mut line = 0_u32;
            let mut byte_column = 0_u32;
            let mut utf16_column = 0_u32;
            let mut chars = subject.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '\r' {
                    if chars.peek() == Some(&'\n') {
                        let _ = chars.next();
                    }
                    line += 1;
                    byte_column = 0;
                    utf16_column = 0;
                } else if ch == '\n' {
                    line += 1;
                    byte_column = 0;
                    utf16_column = 0;
                } else if !ch.is_ascii() {
                    selected = Some((
                        subject,
                        line,
                        byte_column + ch.len_utf8() as u32,
                        utf16_column + ch.len_utf16() as u32,
                    ));
                    break 'cases;
                } else {
                    byte_column += ch.len_utf8() as u32;
                    utf16_column += ch.len_utf16() as u32;
                }
            }
        }
    }
    let (subject, line, byte_column, utf16_column) =
        selected.ok_or("generated subjects did not contain a non-ASCII character")?;
    assert_ne!(
        byte_column, utf16_column,
        "negative-control precondition requires distinct byte and UTF-16 columns"
    );

    let byte_edit = TextEdit::new(
        TextRange::new(TextPosition::new(line, byte_column), TextPosition::new(line, byte_column)),
        "X",
    );
    let utf16_edit = TextEdit::new(
        TextRange::new(
            TextPosition::new(line, utf16_column),
            TextPosition::new(line, utf16_column),
        ),
        "X",
    );
    let utf16_result = apply_plan_strict(&subject, &[utf16_edit])?;
    assert_ne!(utf16_result, subject, "UTF-16 control edit must change the subject");
    match apply_plan_strict(&subject, &[byte_edit]) {
        Err(_) => {}
        Ok(byte_result) => assert_ne!(
            byte_result, utf16_result,
            "byte-column interpretation must differ from UTF-16 application"
        ),
    }
    Ok(())
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> TestResult {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        } else {
            return Err(format!("unsupported filesystem entry under {}", root.display()).into());
        }
    }
    Ok(())
}

fn source_contains_token(source: &str, token: &str) -> bool {
    let identifier = token.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    source.match_indices(token).any(|(start, _)| {
        if !identifier {
            return true;
        }
        let before_is_identifier = source[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let end = start + token.len();
        let after_is_identifier =
            source[end..].chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        !before_is_identifier && !after_is_identifier
    })
}

/// FPH-009 policy pins: the support surface has an allowlisted inventory,
/// exactly one owner marker and checker entry point, and no forbidden
/// constructs. Unchecked indexing is not mechanically scanned: remaining
/// indexing sites are total by construction through const-asserted table
/// alignment and modulo-bounded picks. No token-level scan enforces that
/// construction guarantee, so FPH-009's index-safety clause remains partially
/// unproven.
#[test]
fn fph_policy_pins() -> TestResult {
    let support_root = Path::new(MANIFEST_DIR).join("tests/support/formatter_property_harness");
    let allowed =
        ["mod.rs", "generator.rs", "checker.rs", "strict_apply.rs", "profile.rs", "receipt.rs"];
    let mut support_files = Vec::new();
    collect_files(&support_root, &mut support_files)?;
    for path in support_files {
        let relative = path.strip_prefix(&support_root)?;
        let name = relative.to_string_lossy();
        assert!(
            allowed.contains(&name.as_ref()),
            "unsupported formatter harness support file {name}"
        );
    }

    let mut rust_files = Vec::new();
    collect_files(&Path::new(MANIFEST_DIR).join("src"), &mut rust_files)?;
    collect_files(&Path::new(MANIFEST_DIR).join("tests"), &mut rust_files)?;
    let mut marker_files = 0;
    let mut run_case_files = 0;
    let run_case_marker = format!("pub fn {}(", "run_case");
    for path in &rust_files {
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(path)?;
        if source.contains(formatter_property_harness::FPH_OWNERSHIP_MARKER) {
            marker_files += 1;
        }
        if source.contains(&run_case_marker) {
            run_case_files += 1;
        }
    }
    assert_eq!(marker_files, 1, "FPH ownership marker must occur in exactly one Rust file");
    assert_eq!(run_case_files, 1, "run_case must occur in exactly one Rust file");

    let harness_path = support_root.join("mod.rs");
    let harness_source = fs::read_to_string(&harness_path)?;
    for token in HARNESS_FORBIDDEN_TOKENS {
        assert!(
            !source_contains_token(&harness_source, token),
            "harness source contains forbidden token {token}"
        );
    }
    let panic_regions: Vec<(usize, usize)> = harness_source
        .match_indices("const _: () = {")
        .filter_map(|(start, _)| {
            harness_source[start..].find("};").map(|end| (start, start + end + 2))
        })
        .collect();
    assert_eq!(panic_regions.len(), 1, "harness must have one const alignment block");
    for (panic_start, _) in harness_source.match_indices("panic!") {
        assert!(
            panic_regions.iter().any(|(region_start, region_end)| *region_start <= panic_start
                && panic_start < *region_end),
            "harness panic must remain inside its const alignment block"
        );
    }
    assert_eq!(
        harness_source.matches("panic!").count(),
        0,
        "formatter harness must carry no panic-family exception"
    );
    Ok(())
}

/// FPH-001: every admitted family is a registry variant and every variant
/// carries at least one generator/mutator disposition; promoting a family
/// without a disposition fails the suite. The comparison is anchored to the
/// independent catalog above, not to the registry validating itself:
/// - `Family::ALL` (exhaustively pinned by `Family::pinned_index`, a compile
///   error on any new variant) must equal the pinned name catalog, so a new
///   variant cannot stay invisible;
/// - registry and variant-table rows must exist for every `ALL` entry and
///   carry that entry's identity (fail-closed lookups, no `MISSING`
///   substitutes), and vice versa;
/// - the disposition total is pinned, so deleting any single entry — even
///   with the walk re-deriving everything from the reduced registry — is red;
/// - a bounded seeded run must exercise every pinned family and disposition
///   (generators stay wired; random-byte rejection-dominant generation cannot
///   replace them).
#[test]
fn every_admitted_family_has_a_registered_disposition() -> TestResult {
    let registry = family_registry();
    assert!(!registry.is_empty(), "family registry must not be empty");
    assert_eq!(FUZZ_INDEX_SPACE, 64, "property index space must match six fuzz selector bits");
    assert_eq!(
        Family::ALL.len(),
        PINNED_FAMILY_COUNT,
        "admitted-family enumeration drifted from the pinned catalog (FPH-001)"
    );

    // Addition/deletion path: the enum enumeration, the registry, and the
    // variant table must describe exactly the pinned family set, and every
    // lookup must return the row carrying that family's own identity.
    let mut registry_names: Vec<&str> = Vec::new();
    let mut all_dispositions: Vec<&str> = Vec::new();
    for record in registry {
        let family_name = record.family.name();
        assert!(
            !record.dispositions.is_empty(),
            "family {family_name} has no generator/mutator disposition (FPH-001)"
        );
        let generator_tag = format!("generator.{family_name}");
        assert!(
            record.dispositions.contains(&generator_tag.as_str()),
            "family {family_name} lacks its {generator_tag} disposition (FPH-001)"
        );
        for disposition in record.dispositions {
            assert!(
                disposition.starts_with("generator.") || disposition.starts_with("mutator."),
                "disposition {disposition} of {family_name} is not a generator/mutator tag"
            );
            all_dispositions.push(disposition);
        }
        registry_names.push(family_name);
    }
    assert_eq!(
        all_dispositions.len(),
        PINNED_DISPOSITION_TOTAL,
        "registered disposition count drifted from the pinned catalog (FPH-001)"
    );

    for family in Family::ALL {
        let record = record_for(*family)?;
        assert_eq!(
            record.family,
            *family,
            "registry row for {} does not carry the family's own identity (FPH-001)",
            family.name()
        );
        let variants = variants_for(*family);
        assert_eq!(
            variants.family,
            *family,
            "variant-table row for {} does not carry the family's own identity (FPH-001)",
            family.name()
        );
        assert!(
            !variants.compact.is_empty(),
            "family {} has no generator variants wired (FPH-001)",
            family.name()
        );
    }
    for name in PINNED_FAMILY_NAMES {
        assert!(
            registry_names.contains(&name),
            "pinned family {name} is missing from the registry (FPH-001)"
        );
    }
    for record in registry {
        assert!(
            Family::ALL.contains(&record.family),
            "registry family {} is absent from the pinned enumeration (FPH-001)",
            record.family.name()
        );
    }

    let mut covered_families: Vec<&'static str> = Vec::new();
    let mut covered_dispositions: Vec<&str> = Vec::new();
    for seed in 0..8_u64 {
        for index in 0..FUZZ_INDEX_SPACE {
            let case = generate_case(seed, index);
            assert_eq!(case.schema_version, HARNESS_SCHEMA_VERSION);
            // The generated bytes must carry the selected line-ending
            // convention, so FPH-006's line-ending half is asserted against
            // the subject, not against generator metadata alone.
            assert!(
                convention_present_in_bytes(case.profile.line_ending, &case.subject.text),
                "subject does not contain its selected {} convention",
                case.profile.line_ending.name()
            );
            run_case(&case)?;
            if !covered_families.contains(&case.family.name()) {
                covered_families.push(case.family.name());
            }
            if !covered_dispositions.contains(&case.disposition) {
                covered_dispositions.push(case.disposition);
            }
        }
    }

    // Coverage is judged against the independent pinned catalog, not the
    // registry: a reduced registry would otherwise validate itself green.
    for name in PINNED_FAMILY_NAMES {
        assert!(
            covered_families.contains(&name),
            "pinned family {name} is registered but never generated by the bounded run (FPH-001)"
        );
    }
    for record in registry {
        assert!(
            covered_families.contains(&record.family.name()),
            "family {} is registered but never generated by the bounded run (FPH-001)",
            record.family.name()
        );
        for disposition in record.dispositions {
            assert!(
                covered_dispositions.contains(disposition),
                "disposition {disposition} is registered but never exercised (FPH-001)"
            );
        }
    }
    assert_eq!(
        covered_families.len(),
        PINNED_FAMILY_COUNT,
        "the bounded run must cover every pinned admitted family (FPH-001)"
    );
    assert!(!all_dispositions.is_empty());
    Ok(())
}

/// FPH-001 negative controls: each registered disposition must alter the
/// generated source/configuration through its named path. A disposition that
/// is deleted from the match in `generate_case` must make one of these
/// assertions fail; a receipt-only label cannot satisfy this test.
#[test]
fn generator_and_mutator_dispositions_are_observable() -> TestResult {
    let registry = family_registry();
    for (family_index, record) in registry.iter().enumerate() {
        for (slot, disposition) in record.dispositions.iter().enumerate() {
            let index = family_index + slot * registry.len();
            let mut observed_difference = false;
            for seed in 0x1318_9001..0x1318_9041 {
                let case = generate_case(seed, index);
                let control = generate_case_neutral_control(seed, index);
                if case.subject.text != control.subject.text {
                    observed_difference = true;
                    break;
                }
            }
            assert!(
                observed_difference,
                "{disposition} did not alter generated source bytes versus its neutral control"
            );

            let case = generate_case(0x1318_9001, index);
            let variants = variants_for(record.family);
            if disposition.starts_with("generator.") {
                let expected = variants.compact[family_index % variants.compact.len()];
                assert!(
                    case.subject.text.contains(expected),
                    "{disposition} did not drive its pinned generator shape for {}",
                    record.family.name()
                );
            }
            match *disposition {
                "mutator.spacing_style" => assert!(
                    variants.spaced.iter().any(|variant| case.subject.text.contains(variant)),
                    "spacing mutator did not change source bytes"
                ),
                "mutator.indent_prefix" => assert!(
                    case.subject.text.lines().all(|line| line.starts_with("  ")),
                    "indent mutator did not apply its prefix"
                ),
                "mutator.trailing_comment" => assert!(
                    case.subject.text.contains(" # note"),
                    "trailing-comment mutator did not change source bytes"
                ),
                "mutator.keyword_gap" => assert!(
                    ["if (", "unless (", "while (", "until (", "foreach (", "for ("]
                        .iter()
                        .any(|marker| case.subject.text.contains(marker)),
                    "keyword-gap mutator did not change source bytes"
                ),
                "mutator.block_tail" => assert!(
                    case.subject.text.contains(" }"),
                    "block-tail mutator did not change source bytes"
                ),
                _ => {}
            }
        }
    }
    Ok(())
}

#[test]
fn proptest_rng_algorithm_is_pinned() {
    assert_eq!(harness_proptest_config().rng_algorithm, RngAlgorithm::ChaCha);
}

#[test]
fn fuzz_decoder_covers_entire_generated_index_space() -> TestResult {
    let seed = 0x0123_4567_89ab_cdef_u64;
    for selector in 0x00_u8..=0x3f {
        let mut data = seed.to_le_bytes().to_vec();
        data.push(selector);
        let decoded = case_from_fuzz_input(&data).ok_or("valid fuzz input must decode")?;
        assert_eq!(
            decoded,
            generate_case(seed, usize::from(selector)),
            "valid selector {selector:#04x} was truncated or remapped"
        );
    }
    for selector in 0x80_u8..=0xbf {
        let mut data = seed.to_le_bytes().to_vec();
        data.push(selector);
        let decoded = case_from_fuzz_input(&data).ok_or("invalidation fuzz input must decode")?;
        assert_eq!(
            decoded,
            generate_invalidation_case(seed, usize::from(selector & 0x3f)),
            "invalidation selector {selector:#04x} was truncated or remapped"
        );
    }
    Ok(())
}

/// FPH-002: two runs of the same seed/case through fresh formatter contexts
/// produce identical typed outcomes, change summaries, and edit plans.
#[test]
fn two_fresh_runs_are_identical_typed_outcomes() -> TestResult {
    for seed in [0_u64, 7, 0xdead_beef_cafe_f00d] {
        for index in [0_usize, 21, 47] {
            let case = generate_case(seed, index);
            let first = run_case(&case)?;
            let second = run_case(&case)?;
            assert_eq!(
                first.normalized, second.normalized,
                "receipt must be identical for identical (seed, index)"
            );
            assert_eq!(first.digest, second.digest);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(harness_proptest_config())]

    /// FPH-002/FPH-007 over the drawn case space: identical inputs produce
    /// identical generated cases and identical normalized receipts.
    #[test]
    fn generated_cases_are_reproducible(seed in any::<u64>(), index in 0usize..FUZZ_INDEX_SPACE) {
        let case_a = generate_case(seed, index);
        let case_b = generate_case(seed, index);
        prop_assert_eq!(&case_a, &case_b);
        let receipt_a = run_case(&case_a).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        let receipt_b = run_case(&case_b).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        prop_assert_eq!(receipt_a.normalized, receipt_b.normalized);
        prop_assert_eq!(receipt_a.digest, receipt_b.digest);
    }

    /// FPH-003a: every applied plan, applied through the independent oracle,
    /// reproduces the rendered bytes exactly.
    #[test]
    fn applied_plan_independently_applies_to_rendered_bytes(case in arb_valid_case()) {
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        if receipt.outcome_disposition == "applied" {
            prop_assert!(receipt.applied_application_verified);
        }
    }

    /// FPH-003b: applied plans are ordered, pairwise non-overlapping, and
    /// contained in the requested target (any widening would have to be
    /// exactly recorded, and none is admitted on today's tree).
    #[test]
    fn applied_edits_are_ordered_nonoverlapping_and_target_contained(case in arb_valid_case()) {
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        if receipt.outcome_disposition == "applied" {
            prop_assert!(receipt.plan_ordering_verified);
        }
    }

    /// FPH-004: the second pass from a fresh context never re-applies and
    /// keeps the rendered bytes stable; line-level families must classify as
    /// a legitimate already-formatted no-change with zero edits. Bare-CR
    /// subjects are excluded from the strict classification: their
    /// Insert-policy renders can contain `\r`-inside-`\n` lines that the
    /// safe-subset line admission does not cover, so a typed refusal is
    /// legitimate there (registered FPH-008 dormancy).
    #[test]
    fn second_pass_is_legitimate_nochange(case in arb_valid_case()) {
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        let record = record_for(case.family)
            .map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        let bare_cr_subject = case.profile.line_ending == LineEndingKind::BareCr;
        if let Some(second) = &receipt.second_pass {
            prop_assert_ne!(second.disposition, "applied");
            prop_assert_ne!(second.disposition, "failed_or_not_proven");
            prop_assert_eq!(second.edit_count, 0);
            prop_assert!(second.bytes_stable);
            if !record.renders_closed_blocks && !bare_cr_subject {
                prop_assert_eq!(second.disposition, "no_change");
            } else {
                prop_assert!(
                    second.disposition == "no_change" || second.disposition == "refused",
                    "rendered-block families may only stabilize or refuse, got {}",
                    second.disposition
                );
            }
        }
    }

    /// FPH-005: refused/not-proven outcomes never carry a plan, and their
    /// reason is one of the stable refusal classes; deliberately invalid or
    /// recovered subjects map only to typed refusals.
    #[test]
    fn refusals_carry_no_plan_and_exact_reason_class(case in arb_invalidation_case()) {
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        prop_assert!(
            receipt.outcome_disposition == "refused"
                || receipt.outcome_disposition == "failed_or_not_proven",
            "deliberately invalid subject produced {}",
            receipt.outcome_disposition
        );
        prop_assert_eq!(receipt.plan_edit_count, 0);
        prop_assert!(
            REFUSAL_REASON_CLASSES.contains(&receipt.outcome_reason),
            "refusal reason {} is not a stable refusal class",
            receipt.outcome_reason
        );
    }

    /// FPH-006: line-ending conventions survive LF/CRLF/mixed variants and
    /// every emitted UTF-16 range is valid for the exact subject geometry
    /// (generated subjects carry non-ASCII — BMP and supplementary —
    /// content, so byte, Unicode-scalar, and UTF-16 columns are distinct
    /// geometries and a byte-based emitter fails the range checks). Honest
    /// carve-outs, each a registered fail-closed dormant slot (FPH-008)
    /// rather than a vacuous pass: bare-CR preservation, block-family
    /// subjects whose convention set contains CRLF or bare CR (inserted wrap
    /// lines and touched separators are always LF today), and the Insert/Trim
    /// final-newline policies that own only the final terminator while body
    /// separator convention integrity remains checked
    /// (`final_newline_policy_owns_terminator`).
    #[test]
    fn line_endings_and_utf16_geometry_survive_variants(case in arb_valid_case()) {
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        let bare_cr = case.profile.line_ending == LineEndingKind::BareCr;
        let policy_owns_terminator = case.profile.final_newline != FinalNewline::Preserve;
        let record = record_for(case.family)
            .map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        let wrap_inserts_foreign_separator = record.renders_closed_blocks
            && case.profile.line_ending != LineEndingKind::Lf;
        prop_assert!(
            receipt.line_endings_preserved
                || bare_cr
                || wrap_inserts_foreign_separator
                || policy_owns_terminator
                    && body_line_endings_preserved(
                        &case.subject.text,
                        &receipt.formatted,
                        case.profile.line_ending,
                    )
        );
        if receipt.outcome_disposition == "applied" {
            prop_assert!(receipt.utf16_geometry_verified);
        }
    }

    /// FPH-007: generation is bounded; receipts carry schema, seed, family,
    /// disposition, target, and line-ending identity and are identical for
    /// identical inputs.
    #[test]
    fn generated_case_receipt_is_deterministic_and_bounded(seed in any::<u64>(), index in 0usize..FUZZ_INDEX_SPACE) {
        let case = generate_case(seed, index);
        prop_assert!(case.subject.text.len() <= MAX_SUBJECT_BYTES);
        prop_assert!(case.subject.text.lines().count() <= MAX_SUBJECT_LINES);
        prop_assert_eq!(case.schema_version, HARNESS_SCHEMA_VERSION);
        let receipt = run_case(&case).map_err(|violation| TestCaseError::fail(violation.to_string()))?;
        prop_assert!(receipt.plan_edit_count <= MAX_PLAN_EDITS);
        let seed_field = format!("seed={seed}");
        let family_field = format!("family={}", case.family.name());
        prop_assert!(receipt.normalized.contains(&seed_field));
        prop_assert!(receipt.normalized.contains("schema=1"));
        prop_assert!(receipt.normalized.contains(&family_field));
        prop_assert!(!receipt.digest.is_empty());
    }
}

#[test]
fn body_separator_comparison_rejects_interior_crlf_flip() {
    assert!(!body_line_endings_preserved(
        "one\r\ntwo\r\nthree",
        "one\ntwo\r\nthree",
        LineEndingKind::Crlf,
    ));
    assert!(!body_line_endings_preserved(
        "one\rtwo\rthree",
        "one\r\ntwo\rthree",
        LineEndingKind::BareCr,
    ));
    assert!(!body_line_endings_preserved("one\r\ntwo", "onetwo", LineEndingKind::Crlf,));
    // Block expansion inserts additional LF separators without corrupting an
    // LF document, so counts and positions are intentionally not pinned.
    assert!(body_line_endings_preserved(
        "\tforeach$e(@list){next;} # 😀𝕏\n",
        "\tforeach $e (@list) {\n\t    next;\n\t} # 😀𝕏\n",
        LineEndingKind::Lf,
    ));
    assert!(body_line_endings_preserved("one\ntwo", "one\ntwo\n", LineEndingKind::Lf,));
    assert!(body_line_endings_preserved("one\ntwo\n", "one\ntwo", LineEndingKind::Lf,));
    assert!(body_line_endings_preserved("one\r\ntwo", "one\r\ntwo\r\n", LineEndingKind::Crlf,));
    assert!(body_line_endings_preserved("one\r\ntwo\r\n", "one\r\ntwo", LineEndingKind::Crlf,));
    assert!(body_line_endings_preserved("one\r\ntwo\r\n", "one\r\ntwo\r\n", LineEndingKind::Crlf,));
}

#[test]
fn convention_predicate_rejects_wrong_or_missing_separator_evidence() {
    assert!(!convention_present_in_bytes(LineEndingKind::Crlf, "one\ntwo\n"));
    assert!(!convention_present_in_bytes(LineEndingKind::Lf, "one\r\ntwo\r\n"));
    assert!(!convention_present_in_bytes(LineEndingKind::BareCr, "one\r\ntwo"));
    assert!(!convention_present_in_bytes(LineEndingKind::BareCr, "one\ntwo"));
    assert!(!convention_present_in_bytes(LineEndingKind::Mixed, "one\r\ntwo\r\n"));
    assert!(!convention_present_in_bytes(LineEndingKind::Mixed, "one\ntwo\n"));
    assert!(!convention_present_in_bytes(LineEndingKind::Mixed, "one\r\ntwo\rthree"));
    assert!(!convention_present_in_bytes(LineEndingKind::Lf, "onetwo"));

    assert!(convention_present_in_bytes(LineEndingKind::Lf, "one\ntwo\n"));
    assert!(convention_present_in_bytes(LineEndingKind::Crlf, "one\r\ntwo\r\n"));
    assert!(convention_present_in_bytes(LineEndingKind::BareCr, "one\rtwo"));
    assert!(convention_present_in_bytes(LineEndingKind::Mixed, "one\r\ntwo\n"));
}

#[test]
fn fuzz_decoder_consumes_bytes_past_the_selector() -> TestResult {
    let seed = 0x0123_4567_89ab_cdef_u64;
    let selector = 0x2a_u8;
    let mut base = seed.to_le_bytes().to_vec();
    base.push(selector);

    let decoded_a = case_from_fuzz_input(&base).ok_or("9-byte input must decode")?;
    let decoded_b = case_from_fuzz_input(&base).ok_or("9-byte input must decode")?;
    assert_eq!(decoded_a, decoded_b);
    assert_eq!(decoded_a, generate_case(seed, usize::from(selector & 0x3f)));

    let mut tail_a = base.clone();
    tail_a.push(0x00);
    let mut tail_b = base;
    tail_b.push(0x01);
    let receipt_a = run_case(&case_from_fuzz_input(&tail_a).ok_or("10-byte input A must decode")?)?;
    let receipt_b = run_case(&case_from_fuzz_input(&tail_b).ok_or("10-byte input B must decode")?)?;
    assert_ne!(receipt_a, receipt_b);
    Ok(())
}

/// FPH-008: dormant invariant slots exist as registered dispositions and fail
/// closed as not-proven on today's tree; none claims proven coverage.
#[test]
fn dormant_invariants_report_not_proven_until_dependencies_land() -> TestResult {
    let dormant = dormant_registry();
    assert!(dormant.len() >= 7, "expected the registered dormant slots to be present");
    let mut seen_ids: Vec<&str> = Vec::new();
    for entry in dormant {
        assert!(!seen_ids.contains(&entry.id), "duplicate dormant id {}", entry.id);
        seen_ids.push(entry.id);
        assert!(!entry.gate.is_empty(), "dormant slot {} must name its gate", entry.id);
        assert!(
            !entry.owning_issues.is_empty(),
            "dormant slot {} must name its owning issues",
            entry.id
        );
        assert_eq!(
            entry.status(),
            DormantStatus::NotProven,
            "dormant slot {} must fail closed on today's tree",
            entry.id
        );
    }
    for expected in [
        "cancellation_budget_interruption",
        "structural_preservation_beyond_parse_success",
        "protected_region_hash_preservation",
        "strict_second_pass_typed_idempotence_for_rendered_blocks",
        "bare_cr_line_ending_preservation",
        "wrap_line_separators_follow_source_convention",
        "final_newline_policy_owns_terminator",
    ] {
        assert!(
            seen_ids.contains(&expected),
            "dormant slot {expected} is missing from the registry"
        );
    }
    // #10301 remains open; this branch lands only a bounded subset, and the
    // rendered-block dormancy points at the explicit conversion follow-up.
    let rendered_block = dormant
        .iter()
        .find(|entry| entry.id == "strict_second_pass_typed_idempotence_for_rendered_blocks")
        .ok_or("rendered-block dormancy must stay registered (FPH-008)")?;
    assert_eq!(rendered_block.owning_issues, ["13205"]);
    Ok(())
}

/// FPH-010: predetermined replay-control vectors are replayed deterministically
/// through the property-tier decoder, covering valid and invalidation paths
/// across the generated index space. No runtime fuzzing campaign has been
/// executed, so crash-derived corpus evidence is not claimed.
#[test]
fn replay_controls_and_decoder_pipeline_are_wired() -> TestResult {
    let regression_file = fs::read_to_string(REGRESSION_FILE)?;
    let mut committed_seeds: Vec<u64> = Vec::new();
    let mut seen_cc_entries: Vec<&str> = Vec::new();
    let mut replay_controls: Vec<(u64, u8)> = Vec::new();
    for line in regression_file.lines() {
        if line.starts_with("cc") {
            let rest = line
                .strip_prefix("cc ")
                .ok_or("every regression line beginning with cc must begin with exactly `cc `")?;
            let hex = rest
                .split_whitespace()
                .next()
                .ok_or("cc regression entry must contain a 64-character seed")?;
            assert_eq!(hex.len(), 64, "cc token {hex:?} must be 64 hex chars");
            assert!(
                hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "cc token {hex:?} must contain only lowercase ASCII hex"
            );
            assert!(!seen_cc_entries.contains(&hex), "duplicate cc token {hex:?}");
            seen_cc_entries.push(hex);
            // The harness consumes the low 128 bits' leading word; the full
            // 256-bit entry stays wire-compatible with the lexer convention.
            committed_seeds.push(u64::from_str_radix(
                hex.get(..16).ok_or("committed regression seed is truncated")?,
                16,
            )?);
        }
        // Predetermined replay controls: `seed` is the little-endian seed
        // carried in the first eight bytes, `selector` is the ninth byte
        // naming the case index (low six bits) and the invalidation path
        // (bit 7). Both fields are replayed through the shared property-tier
        // decoder, covering valid, invalidation, and index >= 16 paths without
        // claiming runtime fuzzing evidence.
        if let Some(rest) = line.strip_prefix("# replay-control seed=") {
            let (seed_hex, selector_part) = rest
                .split_once(" selector=")
                .ok_or("replay-control entry must carry seed and selector (FPH-010)")?;
            assert_eq!(seed_hex.len(), 16, "replay-control seed must be 16 hex chars");
            assert_eq!(selector_part.len(), 2, "replay-control selector must be 2 hex chars");
            assert!(
                seed_hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "replay-control seed {seed_hex:?} must contain only lowercase ASCII hex"
            );
            assert!(
                selector_part
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "replay-control selector {selector_part:?} must contain only lowercase ASCII hex"
            );
            replay_controls
                .push((u64::from_str_radix(seed_hex, 16)?, u8::from_str_radix(selector_part, 16)?));
        }
    }
    assert!(!committed_seeds.is_empty(), "committed regression file must carry a cc seed entry");

    for seed in committed_seeds {
        for index in 0..16_usize {
            run_case(&generate_case(seed, index))?;
        }
    }

    assert_eq!(
        replay_controls.as_slice(),
        PINNED_REPLAY_CONTROLS.as_slice(),
        "replay-control entries must equal the pinned ordered control set"
    );
    // Full-fidelity replay of every predetermined control through the
    // shared `(seed, selector)` decoder.
    let mut replayed_invalidation = false;
    let mut replayed_high_index = false;
    for (replay_seed, selector) in replay_controls {
        let index = usize::from(selector & 0x3f);
        if selector & 0x80 != 0 {
            replayed_invalidation = true;
        }
        if index >= 16 {
            replayed_high_index = true;
        }
        let mut data = Vec::with_capacity(9);
        data.extend_from_slice(&replay_seed.to_le_bytes());
        data.push(selector);
        let case = case_from_fuzz_input(&data)
            .ok_or("replay-control input must decode to a generated case")?;
        assert_eq!(case.seed, replay_seed);
        run_case(&case)?;
    }
    assert!(
        replayed_invalidation,
        "replay-control entries must cover the invalidation path (FPH-010)"
    );
    assert!(replayed_high_index, "replay-control entries must cover an index >= 16 (FPH-010)");
    Ok(())
}
