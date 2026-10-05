//! C1 oracle-recall bench (#17154).
//!
//! Each fixture under `benchmarks/ripr/oracles/` pairs a `.t` file containing
//! labeled assertion calls with a `.expected.json` naming the expected oracle
//! `(call, kind, strength)` triples plus the expected test framework and
//! runner hints. This test stages each fixture alone under a fresh
//! `target/ripr-c1/<stem>/t/` root, builds the packet through the real
//! `build_ripr_facts_packet`, and scores recall = |expected ∩ actual| /
//! |expected| over the `oracles[]` triples attributed to that file's test.
//!
//! Threshold (C1): per-fixture recall >= 0.95 AND overall recall >= 0.95.
//! Additionally every fixture asserts exact oracle counts (a `done_testing` /
//! `diag` / `note` call must never emit an oracle), the test fact's framework
//! + runner hints, and a resolving `prove t/<file>` verify command.
//!
//! Sources are `.t` (not `.pm`): the oracle emitter scans `<root>/t` only, so
//! `.pm` fixtures would score a dishonest zero.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must_some_with, must_with};

/// C1 bar from #17154: oracle recall vs labeled truth must be >= 95%.
const MIN_RECALL: f64 = 0.95;

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// Labeled C1 fixture stems. `<stem>.t` and `<stem>.expected.json` live in
/// `benchmarks/ripr/oracles/`; the `.t` is staged under `<root>/t/` because
/// the oracle emitter scans `t/**/*.t`.
const FIXTURES: &[&str] = &[
    "exact_assertions",
    "predicate_assertions",
    "smoke_assertions",
    "mention_assertions",
    "exception_assertions",
    "fatal_assertions",
    "warn_assertions",
    "test2_bundle",
    "mixed_assertions",
    "done_testing_only",
];

/// One labeled oracle: (assertion call name, oracle kind, oracle strength).
type OracleTriple = (String, String, String);

fn fixture_dir() -> String {
    format!("{}/../../benchmarks/ripr/oracles", env!("CARGO_MANIFEST_DIR"))
}

fn packet_for_staged_root(root: &str) -> serde_json::Value {
    must_with(
        build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root,
            base: Some("origin/main"),
            head: Some("HEAD"),
            fact_classes: ALL_FACT_CLASSES,
            diff: None,
        }),
        format!("assemble C1 packet for staged root {root}"),
    )
}

/// The assertion call name carried in an `oracle_id` of the
/// `oracle:<relative>:<name>:<start>-<end>` shape the emitter writes.
fn call_name_from_oracle_id(oracle_id: &str, stem: &str) -> String {
    must_some_with(
        oracle_id.split(':').nth(2),
        format!("C1 fixture `{stem}`: oracle_id `{oracle_id}` carries a call name"),
    )
    .to_owned()
}

/// Multiset overlap between labeled and emitted triples: each expected triple
/// consumes at most one equal actual triple, so duplicates count honestly.
fn count_matched_oracle_triples(expected: &[OracleTriple], actual: &[OracleTriple]) -> usize {
    let mut remaining: Vec<&OracleTriple> = actual.iter().collect();
    let mut matched = 0usize;
    for want in expected {
        if let Some(position) = remaining.iter().position(|got| *got == want) {
            remaining.remove(position);
            matched += 1;
        }
    }
    matched
}

#[test]
fn oracle_recall_meets_c1_threshold() {
    let dir = fixture_dir();
    let mut total_expected = 0usize;
    let mut total_matched = 0usize;
    let mut report = String::from("fixture | expected | actual | recall\n");

    for stem in FIXTURES {
        let source = must_with(
            std::fs::read_to_string(format!("{dir}/{stem}.t")),
            format!("read C1 fixture {stem}.t"),
        );
        let expected_raw = must_with(
            std::fs::read_to_string(format!("{dir}/{stem}.expected.json")),
            format!("read C1 expectation {stem}.expected.json"),
        );
        let expected_json: serde_json::Value =
            must_with(serde_json::from_str(&expected_raw), format!("parse {stem}.expected.json"));
        let expected_framework = must_some_with(
            expected_json["framework"].as_str(),
            format!("{stem}.expected.json names a framework"),
        )
        .to_owned();
        let expected_hints: Vec<String> = must_some_with(
            expected_json["runner_hints"].as_array(),
            format!("{stem}.expected.json names runner_hints"),
        )
        .iter()
        .filter_map(|hint| hint.as_str().map(String::from))
        .collect();
        let expected: Vec<OracleTriple> = must_some_with(
            expected_json["expected_oracles"].as_array(),
            format!("{stem}.expected.json names expected_oracles"),
        )
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let call = must_some_with(
                entry["call"].as_str(),
                format!("{stem}.expected.json oracle {index} names a call"),
            )
            .to_owned();
            let kind = must_some_with(
                entry["kind"].as_str(),
                format!("{stem}.expected.json oracle {index} names a kind"),
            )
            .to_owned();
            let strength = must_some_with(
                entry["strength"].as_str(),
                format!("{stem}.expected.json oracle {index} names a strength"),
            )
            .to_owned();
            (call, kind, strength)
        })
        .collect();

        // Pid-namespaced: two concurrent `cargo test` invocations must never
        // share a fixture tree (#17272 lesson — one invocation's setup/teardown
        // `remove_dir_all` could otherwise remove another's fixture mid-read).
        let root = format!("target/ripr-c1/{stem}-pid{}", std::process::id());
        let _ = std::fs::remove_dir_all(&root);
        must_with(std::fs::create_dir_all(format!("{root}/t")), format!("mkdir {root}/t"));
        must_with(
            std::fs::write(format!("{root}/t/{stem}.t"), &source),
            format!("stage {stem} fixture"),
        );
        let packet = packet_for_staged_root(&root);
        let _ = std::fs::remove_dir_all(&root);

        let relative = format!("t/{stem}.t");
        let test_id = format!("test:{relative}");
        let test_fact = must_some_with(
            must_some_with(
                packet["tests"].as_array(),
                format!("C1 fixture `{stem}`: packet carries tests[]"),
            )
            .iter()
            .find(|test| test["test_id"].as_str() == Some(test_id.as_str())),
            format!("C1 fixture `{stem}`: tests[] holds {test_id}"),
        );
        assert_eq!(
            test_fact["framework"].as_str(),
            Some(expected_framework.as_str()),
            "C1 fixture `{stem}`: framework must be `{expected_framework}`"
        );
        let actual_hints: Vec<&str> = must_some_with(
            test_fact["runner_hints"].as_array(),
            format!("C1 fixture `{stem}`: test fact carries runner_hints"),
        )
        .iter()
        .filter_map(|hint| hint.as_str())
        .collect();
        let expected_hint_strs: Vec<&str> = expected_hints.iter().map(String::as_str).collect();
        assert_eq!(
            actual_hints, expected_hint_strs,
            "C1 fixture `{stem}`: runner_hints must be {expected_hint_strs:?}"
        );

        let actual: Vec<OracleTriple> = must_some_with(
            packet["oracles"].as_array(),
            format!("C1 fixture `{stem}`: packet carries oracles[]"),
        )
        .iter()
        .filter(|oracle| oracle["test_id"].as_str() == Some(test_id.as_str()))
        .enumerate()
        .map(|(index, oracle)| {
            let oracle_id = must_some_with(
                oracle["oracle_id"].as_str(),
                format!("C1 fixture `{stem}`: oracle {index} carries an oracle_id"),
            );
            let kind = must_some_with(
                oracle["kind"].as_str(),
                format!("C1 fixture `{stem}`: oracle {index} carries a kind"),
            )
            .to_owned();
            let strength = must_some_with(
                oracle["strength"].as_str(),
                format!("C1 fixture `{stem}`: oracle {index} carries a strength"),
            )
            .to_owned();
            (call_name_from_oracle_id(oracle_id, stem), kind, strength)
        })
        .collect();

        // Exact-count precision: diagnostics (`diag`/`note`) and structure
        // (`done_testing`) must never emit oracles, so extras fail as loudly
        // as misses.
        assert_eq!(
            actual.len(),
            expected.len(),
            "C1 fixture `{stem}`: emitted {} oracles, labeled {} (extras mean a \
             non-assertion call leaked into oracles[])\n{report}",
            actual.len(),
            expected.len()
        );

        // The staged `.t` file is a prove target: one verify command must run
        // `prove` against it and resolve to its test fact.
        let prove_target = must_some_with(
            packet["verify_commands"].as_array(),
            format!("C1 fixture `{stem}`: packet carries verify_commands[]"),
        )
        .iter()
        .find(|command| {
            command["test_id"].as_str() == Some(test_id.as_str())
                && command["argv"] == serde_json::json!(["prove", relative.as_str()])
        });
        assert!(
            prove_target.is_some(),
            "C1 fixture `{stem}`: verify_commands[] must hold `prove {relative}` for {test_id}"
        );

        if expected.is_empty() {
            report.push_str(&format!("{stem} | [] | [] | n/a (negative control)\n"));
            continue;
        }
        let matched = count_matched_oracle_triples(&expected, &actual);
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
            "C1 recall {recall:.2} for fixture `{stem}` is below the {MIN_RECALL} bar; \
             missing triples: {:?}\n{report}",
            expected.iter().filter(|want| !actual.contains(want)).collect::<Vec<_>>()
        );
    }

    #[allow(clippy::cast_precision_loss)]
    let overall = total_matched as f64 / total_expected as f64;
    report.push_str(&format!("overall | | | {total_matched}/{total_expected} = {overall:.2}\n"));
    assert!(
        overall >= MIN_RECALL,
        "C1 overall recall {overall:.2} ({total_matched}/{total_expected}) is below the {MIN_RECALL} bar\n{report}"
    );
}
