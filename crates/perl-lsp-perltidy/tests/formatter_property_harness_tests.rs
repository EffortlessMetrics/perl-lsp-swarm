//! Property/fuzz harness proof for formatter safety invariants (#10301).
//!
//! Rows FPH-001..FPH-010 from `.spec/10301-formatter-property-fuzz-harness/`.
//! The shared invariant core lives in
//! `tests/support/formatter_property_harness/` and is consumed verbatim by the
//! cargo-fuzz target `fuzz/fuzz_targets/perl_tidy_formatter.rs`. The checker
//! binds only canonical production APIs (`format_*_typed`) and the independent
//! byte-edit oracle (`apply_edits_exact`); it never reuses production edit
//! application, never spawns a process, and never reads a clock.
//!
//! Determinism: every case is a pure function of `(seed, index)`; receipts are
//! normalized and digested without wall-clock input. Boundedness is asserted
//! per case (`MAX_SUBJECT_BYTES`, `MAX_PLAN_EDITS`, `MAX_SUBJECT_LINES`).
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, FileFailurePersistence, RngAlgorithm};

#[path = "support/formatter_property_harness/mod.rs"]
mod formatter_property_harness;

use formatter_property_harness::{
    DormantStatus, Family, GENERATED_INDEX_SPACE, GeneratedCase, HARNESS_SCHEMA_VERSION,
    LineEndingKind, MAX_PLAN_EDITS, MAX_SUBJECT_BYTES, MAX_SUBJECT_LINES, case_from_fuzz_input,
    dormant_registry, family_registry, generate_case, generate_case_neutral_control,
    generate_invalidation_case, record_for, run_case, variants_for,
};
use perl_lsp_perltidy::native::FinalNewline;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const REGRESSION_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/formatter_property_harness_tests.proptest-regressions"
);

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

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

const PINNED_HARNESS_SUPPORT_FILES: &[&str] = &["mod.rs"];

const PINNED_PANIC_COUNTS: &[(&str, usize)] = &[
    ("tests/support/formatter_property_harness/mod.rs", 3),
    ("../../fuzz/fuzz_targets/perl_tidy_formatter.rs", 1),
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
    (any::<u64>(), 0usize..GENERATED_INDEX_SPACE)
        .prop_map(|(seed, index)| generate_case(seed, index))
}

fn arb_invalidation_case() -> impl Strategy<Value = GeneratedCase> {
    (any::<u64>(), 0usize..GENERATED_INDEX_SPACE)
        .prop_map(|(seed, index)| generate_invalidation_case(seed, index))
}

fn collect_files_recursively(
    current: &Path,
    extension: Option<&str>,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(current)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files_recursively(&path, extension, files)?;
        } else if extension.is_none() || path.extension().and_then(OsStr::to_str) == extension {
            files.push(path);
        }
    }
    Ok(())
}

/// FPH policy pins: this scan is textual and deliberately scoped to the
/// harness surface; unchecked indexing is covered by the
/// `get_unchecked`/`from_raw_parts` tokens rather than a `[i]` heuristic.
#[test]
fn fph_policy_pins() -> TestResult {
    let support_root =
        PathBuf::from(format!("{MANIFEST_DIR}/tests/support/formatter_property_harness"));
    let mut support_file_paths = Vec::new();
    collect_files_recursively(&support_root, None, &mut support_file_paths)?;
    let mut support_files = Vec::new();
    for path in support_file_paths {
        support_files.push(
            path.strip_prefix(&support_root)
                .map_err(|error| format!("support path is outside root: {error}"))?
                .to_path_buf(),
        );
    }
    support_files.sort();
    let pinned_support_files: Vec<PathBuf> =
        PINNED_HARNESS_SUPPORT_FILES.iter().map(PathBuf::from).collect();
    assert_eq!(
        support_files, pinned_support_files,
        "harness support-tree inventory drifted: actual={support_files:?}, pinned={pinned_support_files:?}"
    );

    let source_root = PathBuf::from(format!("{MANIFEST_DIR}/src"));
    let tests_root = PathBuf::from(format!("{MANIFEST_DIR}/tests"));
    let mut rust_files = Vec::new();
    collect_files_recursively(&source_root, Some("rs"), &mut rust_files)?;
    collect_files_recursively(&tests_root, Some("rs"), &mut rust_files)?;
    let markers = [
        ["pub const ", "HARNESS_SCHEMA_VERSION"].concat(),
        ["pub fn ", "run_case("].concat(),
        format!("#[path = \"support/formatter_property_harness/{}\"]", "mod.rs"),
    ];
    for marker in markers {
        let mut count = 0;
        let mut locations = Vec::new();
        for path in &rust_files {
            let source = fs::read_to_string(path)?;
            let occurrences = source.matches(&marker).count();
            if occurrences > 0 {
                locations.push((path.display().to_string(), occurrences));
                count += occurrences;
            }
        }
        assert_eq!(
            count, 1,
            "marker {marker:?} must occur exactly once across src/tests; locations={locations:?}"
        );
    }

    let harness_path =
        PathBuf::from(format!("{MANIFEST_DIR}/tests/support/formatter_property_harness/mod.rs"));
    let fuzz_path =
        PathBuf::from(format!("{MANIFEST_DIR}/../../fuzz/fuzz_targets/perl_tidy_formatter.rs"));
    let banned_tokens = [
        ".unwrap(",
        ".expect(",
        "todo!",
        "unimplemented!",
        "unreachable!",
        "dbg!",
        "unsafe ",
        "get_unchecked",
        "from_raw_parts",
    ];
    for (relative_path, expected_panics) in PINNED_PANIC_COUNTS {
        let path = if relative_path.starts_with("tests/") { &harness_path } else { &fuzz_path };
        let source = fs::read_to_string(path)?;
        for token in banned_tokens {
            assert!(
                !source.contains(token),
                "forbidden token {token:?} found in {}",
                path.display()
            );
        }
        assert_eq!(
            source.matches("panic!").count(),
            *expected_panics,
            "panic! count drifted in {}",
            path.display()
        );
    }
    Ok(())
}

/// FPH-009 source pin: the harness module and the fuzz target must never
/// reference the subprocess adapter, process spawning, or a wall clock.
#[test]
fn harness_module_does_not_reference_external_oracle() -> TestResult {
    let harness_source = fs::read_to_string(format!(
        "{MANIFEST_DIR}/tests/support/formatter_property_harness/mod.rs"
    ))?;
    let fuzz_source = fs::read_to_string(format!(
        "{MANIFEST_DIR}/../../fuzz/fuzz_targets/perl_tidy_formatter.rs"
    ))?;

    let banned_in_harness = [
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
    ];
    for token in banned_in_harness {
        assert!(
            !harness_source.contains(token),
            "harness module must not reference {token} (FPH-009)"
        );
    }

    let banned_in_fuzz = [
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
    ];
    for token in banned_in_fuzz {
        assert!(!fuzz_source.contains(token), "fuzz target must not reference {token} (FPH-009)");
    }
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
        for index in 0..GENERATED_INDEX_SPACE {
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

/// Whether the emitted subject bytes actually contain the selected
/// line-ending convention (bare CR and mixed separators exist only between
/// lines, so the generator forces multi-line subjects for those variants).
fn convention_present_in_bytes(kind: LineEndingKind, text: &str) -> bool {
    let without_crlf = text.replace("\r\n", "");
    match kind {
        LineEndingKind::Lf => without_crlf.contains('\n'),
        LineEndingKind::Crlf => text.contains("\r\n"),
        LineEndingKind::BareCr => text.contains('\r') && !text.contains('\n'),
        LineEndingKind::Mixed => {
            text.contains("\r\n") && without_crlf.contains('\n') && !without_crlf.contains('\r')
        }
    }
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
    fn generated_cases_are_reproducible(seed in any::<u64>(), index in 0usize..GENERATED_INDEX_SPACE) {
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
    /// final-newline policies that own the final terminator by contract
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
            receipt.line_endings_preserved || bare_cr || policy_owns_terminator
                || wrap_inserts_foreign_separator
        );
        if receipt.outcome_disposition == "applied" {
            prop_assert!(receipt.utf16_geometry_verified);
        }
    }

    /// FPH-007: generation is bounded; receipts carry schema, seed, family,
    /// disposition, target, and line-ending identity and are identical for
    /// identical inputs.
    #[test]
    fn generated_case_receipt_is_deterministic_and_bounded(seed in any::<u64>(), index in 0usize..GENERATED_INDEX_SPACE) {
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
    // The rendered-block dormancy's conversion owner must outlive the claim
    // this PR closes (#10301): it points at the explicit follow-up issue.
    let rendered_block = dormant
        .iter()
        .find(|entry| entry.id == "strict_second_pass_typed_idempotence_for_rendered_blocks")
        .ok_or("rendered-block dormancy must stay registered (FPH-008)")?;
    assert_eq!(rendered_block.owning_issues, ["13205"]);
    Ok(())
}

/// FPH-010: the cargo-fuzz target drives the same invariant core from
/// structured byte mutations, is declared in the fuzz manifest with the
/// missing perltidy dependency, and one minimized committed regression entry
/// is replayed deterministically through the same core.
#[test]
fn fuzz_target_and_regression_pipeline_are_wired() -> TestResult {
    let fuzz_manifest = fs::read_to_string(format!("{MANIFEST_DIR}/../../fuzz/Cargo.toml"))?;
    assert!(
        fuzz_manifest.contains("perl-lsp-perltidy"),
        "fuzz manifest must depend on perl-lsp-perltidy (FPH-010)"
    );
    assert!(
        fuzz_manifest.contains("name = \"perl_tidy_formatter\""),
        "fuzz manifest must declare the perl_tidy_formatter target (FPH-010)"
    );

    let fuzz_source = fs::read_to_string(format!(
        "{MANIFEST_DIR}/../../fuzz/fuzz_targets/perl_tidy_formatter.rs"
    ))?;
    assert!(
        fuzz_source.contains("formatter_property_harness"),
        "fuzz target must include the shared invariant core (FPH-010)"
    );
    assert!(
        fuzz_source.contains("fuzz_target!"),
        "fuzz target must be a libfuzzer target (FPH-010)"
    );
    assert!(
        fuzz_source.contains("run_case"),
        "fuzz target must drive the shared checker (FPH-010)"
    );

    let regression_file = fs::read_to_string(REGRESSION_FILE)?;
    let mut committed_seeds = Vec::new();
    let mut fuzz_replays: Vec<(u64, u8)> = Vec::new();
    for line in regression_file.lines() {
        if let Some(rest) = line.strip_prefix("cc ") {
            let hex = rest
                .split_whitespace()
                .next()
                .ok_or("cc regression entry must carry a seed token (FPH-010)")?;
            assert_eq!(
                hex.len(),
                64,
                "committed regression seed must be 64 lowercase hex chars: {hex:?}"
            );
            assert!(
                hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "committed regression seed must be 64 lowercase hex chars: {hex:?}"
            );
            assert!(
                !committed_seeds.iter().any(|entry| entry == hex),
                "duplicate committed regression seed entry: {hex:?}"
            );
            // The harness consumes the low 128 bits' leading word; the full
            // 256-bit entry stays wire-compatible with the lexer convention.
            committed_seeds.push(hex.to_string());
        }
        // Committed fuzz crash artifacts: `seed` is the little-endian seed
        // the cargo-fuzz input carries in its first eight bytes, `selector`
        // is the ninth byte naming the case index (low six bits) and the
        // invalidation path (bit 7). Both fields are replayed through the
        // same decoder the fuzz target uses, so an invalidation-path or
        // index >= 16 crash is reconstructible — not just seeds 0..16 of the
        // valid path.
        if let Some(rest) = line.strip_prefix("# fuzz-replay seed=") {
            let (seed_hex, selector_part) = rest
                .split_once(" selector=")
                .ok_or("fuzz-replay entry must carry seed and selector (FPH-010)")?;
            assert_eq!(seed_hex.len(), 16, "fuzz-replay seed must be 16 hex chars");
            assert_eq!(selector_part.len(), 2, "fuzz-replay selector must be 2 hex chars");
            fuzz_replays
                .push((u64::from_str_radix(seed_hex, 16)?, u8::from_str_radix(selector_part, 16)?));
        }
    }
    assert!(
        !committed_seeds.is_empty(),
        "committed regression file must carry at least one cc seed entry"
    );
    // Use the first committed cc entry for the seeded 0..16 replay.
    let seed = u64::from_str_radix(&committed_seeds[0][..16], 16)?;

    for index in 0..16_usize {
        run_case(&generate_case(seed, index))?;
    }

    // Full-fidelity replay of every committed fuzz artifact through the
    // shared `(seed, selector)` decoder.
    assert!(
        fuzz_replays.len() >= 3,
        "committed fuzz-replay entries must cover the valid path, the invalidation path, and an index >= 16"
    );
    let mut replayed_invalidation = false;
    let mut replayed_high_index = false;
    for (replay_seed, selector) in fuzz_replays {
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
            .ok_or("committed fuzz-replay input must decode to a generated case")?;
        assert_eq!(case.seed, replay_seed);
        run_case(&case)?;
    }
    assert!(
        replayed_invalidation,
        "committed fuzz-replay entries must cover the invalidation path (FPH-010)"
    );
    assert!(
        replayed_high_index,
        "committed fuzz-replay entries must cover an index >= 16 (FPH-010)"
    );
    Ok(())
}
