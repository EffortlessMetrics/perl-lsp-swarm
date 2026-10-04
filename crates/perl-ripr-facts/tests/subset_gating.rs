//! Subset-gating honesty (#17256).
//!
//! `requested_fact_classes` subsets must be honored: a subset packet carries
//! facts only from requested classes — except where referential integrity
//! trumps strict gating (the in-code rule relations already follow: a kept
//! fact's required `test_id` must resolve). `dynamic_boundaries[]` and
//! `verify_commands[]` used to leak into every subset unconditionally, with
//! verify entries carrying dangling `test_id` refs.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_some_with, must_with};

use std::collections::HashSet;

const BOUNDARY_PM: &str =
    "package Risky;\nsub run {\n    my $code = shift;\n    eval { $code->() };\n}\n1;\n";

const EV_T: &str = "use Test::More;\nok(1, 'smoke');\ndone_testing();\n";

fn build_packet_with_classes(root: &str, fact_classes: &str) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes,
        diff: None,
    }))
}

fn stage_file(root: &str, rel: &str, content: &str) {
    let path = format!("{root}/{rel}");
    if let Some(parent) = std::path::Path::new(&path).parent() {
        must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
    }
    must_with(std::fs::write(&path, content), format!("stage {rel}"));
}

fn fresh_root(name: &str) -> String {
    let root = format!("target/ripr-g3/{name}");
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn boundary_fixture() -> String {
    let root = fresh_root("boundary");
    stage_file(&root, "lib/Risky.pm", BOUNDARY_PM);
    stage_file(&root, "t/ev.t", EV_T);
    root
}

fn array_len(packet: &serde_json::Value, key: &str) -> usize {
    must_some_with(packet[key].as_array(), format!("packet[{key}] must be an array")).len()
}

#[test]
fn subset_request_carries_no_unrequested_facts() {
    let root = boundary_fixture();
    let packet = build_packet_with_classes(&root, "files,owners");
    let _ = std::fs::remove_dir_all(&root);

    // Sanity: the fixture really would emit boundaries + verify when asked.
    assert!(!must_some(packet["files"].as_array()).is_empty(), "fixture must yield files");
    assert_eq!(
        array_len(&packet, "dynamic_boundaries"),
        0,
        "files,owners subset must not leak boundaries"
    );
    assert_eq!(
        array_len(&packet, "verify_commands"),
        0,
        "files,owners subset must not leak verify commands"
    );
    assert_eq!(array_len(&packet, "tests"), 0, "nothing forces tests in a files,owners subset");
}

#[test]
fn verify_only_request_forces_referenced_tests_without_dangling_refs() {
    let root = boundary_fixture();
    let packet = build_packet_with_classes(&root, "verify_commands");
    let _ = std::fs::remove_dir_all(&root);

    let verify = must_some(packet["verify_commands"].as_array());
    assert!(!verify.is_empty(), "explicit verify_commands request must keep its facts");
    let tests = must_some(packet["tests"].as_array());
    assert!(
        !tests.is_empty(),
        "verify entries force their referenced tests (referential integrity)"
    );
    let test_ids: HashSet<&str> = tests.iter().filter_map(|t| t["test_id"].as_str()).collect();
    for entry in verify {
        let test_id =
            must_some_with(entry["test_id"].as_str(), "verify entry must carry a test_id");
        assert!(
            test_ids.contains(test_id),
            "verify test_id `{test_id}` must resolve to a test fact"
        );
    }
    // The forced tests reference `test_discovery` provenance by id — it must
    // be in the packet too.
    let prov_ids: HashSet<&str> = must_some(packet["provenance"].as_array())
        .iter()
        .filter_map(|p| p["provenance_id"].as_str())
        .collect();
    for test in tests {
        for reference in must_some(test["provenance_refs"].as_array()) {
            let id = must_some(reference.as_str());
            assert!(prov_ids.contains(id), "forced test provenance_ref `{id}` must resolve");
        }
    }
    assert_eq!(
        array_len(&packet, "dynamic_boundaries"),
        0,
        "verify_commands subset must not leak boundaries"
    );
    assert_eq!(
        array_len(&packet, "relations"),
        0,
        "verify_commands subset must not leak relations"
    );
}
