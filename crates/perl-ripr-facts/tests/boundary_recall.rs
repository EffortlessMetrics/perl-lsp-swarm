//! E2 dynamic-boundary recall bench (#17154).
//!
//! Each fixture under `benchmarks/ripr/boundaries/` pairs a `.pm` file
//! containing one dynamic-boundary shape (or several, for `mixed.pm`) with a
//! `.expected.json` naming the expected boundary kinds. This test stages each
//! fixture alone under a fresh `target/ripr-e2/<stem>/lib/` root, builds the
//! packet, and scores recall = |expected ∩ actual| / |expected| over the
//! `dynamic_boundaries[]` kinds attributed to that file.
//!
//! Threshold (E2): per-fixture recall >= 0.90 AND overall recall >= 0.90.
//! Every single-kind fixture must therefore hit its kind exactly; the mixed
//! fixture may miss at most one of its four kinds (10% of nothing — in
//! practice the bar is 4/4, since 3/4 = 0.75 < 0.90).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use std::collections::HashSet;

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_some_with, must_with};

/// E2 bar from #17154: dynamic-boundary recall for
/// eval/AUTOLOAD/dynamic-require must be >= 90%.
const MIN_RECALL: f64 = 0.90;

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// Labeled E2 fixtures: (stem, staged file name). The `.pm` and
/// `.expected.json` live in `benchmarks/ripr/boundaries/`; the `.pm` is staged
/// under `<root>/lib/` because the boundary emitter scans `lib/**/*.pm`.
const FIXTURES: &[(&str, &str)] = &[
    ("eval_string", "EvalString.pm"),
    ("eval_spaced", "EvalSpaced.pm"),
    ("autoload", "Autoload.pm"),
    ("dynamic_require", "DynRequire.pm"),
    ("string_dispatch", "StrDispatch.pm"),
    ("isa_manipulation", "IsaManip.pm"),
    ("symbol_table", "SymTable.pm"),
    ("mixed", "Mixed.pm"),
];

fn fixture_dir() -> String {
    format!("{}/../../benchmarks/ripr/boundaries", env!("CARGO_MANIFEST_DIR"))
}

fn build_packet(root: &str) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: ALL_FACT_CLASSES,
        diff: None,
    }))
}

#[test]
fn boundary_recall_meets_e2_threshold() {
    let dir = fixture_dir();
    let mut total_expected = 0usize;
    let mut total_matched = 0usize;
    let mut report = String::from("fixture | expected | actual | recall\n");

    for (stem, file_name) in FIXTURES {
        let source = must_with(
            std::fs::read_to_string(format!("{dir}/{stem}.pm")),
            format!("read E2 fixture {stem}.pm"),
        );
        let expected_raw = must_with(
            std::fs::read_to_string(format!("{dir}/{stem}.expected.json")),
            format!("read E2 expectation {stem}.expected.json"),
        );
        let expected_json: serde_json::Value =
            must_with(serde_json::from_str(&expected_raw), format!("parse {stem}.expected.json"));
        let expected: HashSet<String> = must_some_with(
            expected_json["expected_boundary_kinds"].as_array(),
            format!("{stem}.expected.json names expected_boundary_kinds"),
        )
        .iter()
        .filter_map(|kind| kind.as_str().map(String::from))
        .collect();
        assert!(!expected.is_empty(), "{stem}.expected.json must name at least one kind");

        let root = format!("target/ripr-e2/{stem}");
        let _ = std::fs::remove_dir_all(&root);
        must_with(std::fs::create_dir_all(format!("{root}/lib")), format!("mkdir {root}/lib"));
        must_with(
            std::fs::write(format!("{root}/lib/{file_name}"), &source),
            format!("stage {stem} fixture"),
        );
        let packet = build_packet(&root);
        let _ = std::fs::remove_dir_all(&root);

        let file_id = format!("file:lib/{file_name}");
        let actual: HashSet<String> = must_some(packet["dynamic_boundaries"].as_array())
            .iter()
            .filter(|b| b["file_id"].as_str() == Some(file_id.as_str()))
            .filter_map(|b| b["kind"].as_str().map(String::from))
            .collect();
        let matched = expected.intersection(&actual).count();
        total_expected += expected.len();
        total_matched += matched;
        #[allow(clippy::cast_precision_loss)]
        let recall = matched as f64 / expected.len() as f64;
        report.push_str(&format!(
            "{stem} | {expected:?} | {actual:?} | {matched}/{} = {recall:.2}\n",
            expected.len()
        ));
        assert!(
            recall >= MIN_RECALL,
            "E2 recall {recall:.2} for fixture `{stem}` is below the {MIN_RECALL} bar; \
             missing kinds: {:?}\n{report}",
            expected.difference(&actual).collect::<Vec<_>>()
        );
    }

    #[allow(clippy::cast_precision_loss)]
    let overall = total_matched as f64 / total_expected as f64;
    report.push_str(&format!("overall | | | {total_matched}/{total_expected} = {overall:.2}\n"));
    assert!(
        overall >= MIN_RECALL,
        "E2 overall recall {overall:.2} ({total_matched}/{total_expected}) is below the {MIN_RECALL} bar\n{report}"
    );
}
