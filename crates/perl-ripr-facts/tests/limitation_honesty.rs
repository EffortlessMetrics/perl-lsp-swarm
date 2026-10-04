//! E3 limitation-honesty bench (#17154).
//!
//! Hostile inputs must never be silently dropped nor misrepresented: each one
//! is either flagged with an explicit limitation/unknown signal or represented
//! accurately. Fixtures live in `benchmarks/ripr/hostile/` (the huge file is
//! generated at test time so the repo does not carry a 2MB blob):
//!
//! | fixture | honesty predicate |
//! | `unparseable.pm` | file fact present + `parse-failed:` limitation, zero owners |
//! | `empty.pm` | file fact present, empty digest, zero owners (accurate, no phantoms) |
//! | huge generated `Huge.pm` (~2MB) | file fact present, all owners extracted |
//! | `unknown_framework.t` | test fact present with framework `unknown` (not guessed) |
//! | `outside_root.diff` | `diff-file-not-found:` limitation, no phantom change |
//!
//! Threshold (E3): 100% of hostile inputs surfaced (5/5 predicates hold),
//! 0 drops (every staged input is accounted for by a file/test fact or a
//! limitation referencing it).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_some_with, must_with};

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// E3 bar from #17154: every hostile input's handling is asserted (5/5).
const EXPECTED_SURFACED: usize = 5;

/// Target size for the generated huge fixture: ~2MB of real Perl (subs with
/// comment bodies, so the parser does real work per owner).
const HUGE_TARGET_BYTES: usize = 2_000_000;

/// SHA-256 of the empty string, as the packet's digest recipe renders it.
const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn hostile_dir() -> String {
    format!("{}/../../benchmarks/ripr/hostile", env!("CARGO_MANIFEST_DIR"))
}

fn build_packet(root: &str, diff: Option<&str>) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: ALL_FACT_CLASSES,
        diff,
    }))
}

fn stage_file(root: &str, rel: &str, content: &str) {
    let path = format!("{root}/{rel}");
    if let Some(parent) = std::path::Path::new(&path).parent() {
        must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
    }
    must_with(std::fs::write(&path, content), format!("stage hostile file {rel}"));
}

fn fresh_root(name: &str) -> String {
    let root = format!("target/ripr-e3/{name}");
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn limitation_ids(packet: &serde_json::Value) -> Vec<String> {
    must_some(packet["limitations"].as_array())
        .iter()
        .filter_map(|l| l["limitation_id"].as_str().map(String::from))
        .collect()
}

fn owners_for(packet: &serde_json::Value, file_id: &str) -> Vec<serde_json::Value> {
    must_some(packet["owners"].as_array())
        .iter()
        .filter(|o| o["file_id"].as_str() == Some(file_id))
        .cloned()
        .collect()
}

/// Every staged input must be accounted for: a `files[]`/`tests[]` fact, or a
/// limitation whose id or evidence refs name it. Returns the count surfaced.
fn assert_accounted_for(packet: &serde_json::Value, rel: &str, what: &str) {
    let file_id = format!("file:{rel}");
    let test_id = format!("test:{rel}");
    let has_file = must_some(packet["files"].as_array())
        .iter()
        .any(|f| f["file_id"].as_str() == Some(file_id.as_str()));
    let has_test = must_some(packet["tests"].as_array())
        .iter()
        .any(|t| t["test_id"].as_str() == Some(test_id.as_str()));
    let has_limitation = must_some(packet["limitations"].as_array()).iter().any(|l| {
        l["limitation_id"].as_str().is_some_and(|id| id.contains(rel))
            || l["evidence_refs"]
                .as_array()
                .is_some_and(|refs| refs.iter().any(|r| r.as_str() == Some(file_id.as_str())))
    });
    assert!(
        has_file || has_test || has_limitation,
        "E3 drop: hostile input `{rel}` ({what}) has no file/test fact and no limitation referencing it"
    );
}

#[test]
fn hostile_inputs_are_all_surfaced_and_none_dropped() {
    let dir = hostile_dir();
    let mut surfaced = 0usize;
    let mut report = String::from("fixture | outcome\n");

    // 1. Unparseable Perl: file fact present, parse-failed limitation, no owners.
    {
        let source = must_with(
            std::fs::read_to_string(format!("{dir}/unparseable.pm")),
            "read unparseable.pm fixture",
        );
        let root = fresh_root("unparseable");
        stage_file(&root, "lib/Unparseable.pm", &source);
        let packet = build_packet(&root, None);
        let _ = std::fs::remove_dir_all(&root);

        let file_id = "file:lib/Unparseable.pm";
        assert!(
            must_some(packet["files"].as_array())
                .iter()
                .any(|f| f["file_id"].as_str() == Some(file_id)),
            "unparseable.pm must still yield a file fact (fail soft, not silent)"
        );
        assert!(
            limitation_ids(&packet).iter().any(|id| id == &format!("parse-failed:{file_id}")),
            "unparseable.pm must surface a parse-failed limitation; got {:?}",
            limitation_ids(&packet)
        );
        assert!(
            owners_for(&packet, file_id).is_empty(),
            "unparseable.pm must yield zero owners (no hallucinated decls)"
        );
        assert_accounted_for(&packet, "lib/Unparseable.pm", "unparseable perl");
        surfaced += 1;
        report.push_str("unparseable.pm | file fact + parse-failed limitation, 0 owners\n");
    }

    // 2. Empty file: present, empty digest, zero owners — accurate, no phantoms.
    {
        let source =
            must_with(std::fs::read_to_string(format!("{dir}/empty.pm")), "read empty.pm fixture");
        assert!(source.is_empty(), "empty.pm fixture must be 0 bytes");
        let root = fresh_root("empty");
        stage_file(&root, "lib/Empty.pm", &source);
        let packet = build_packet(&root, None);
        let _ = std::fs::remove_dir_all(&root);

        let file_id = "file:lib/Empty.pm";
        let file = must_some_with(
            must_some(packet["files"].as_array())
                .iter()
                .find(|f| f["file_id"].as_str() == Some(file_id)),
            "empty.pm must yield a file fact (not dropped)",
        );
        assert_eq!(file["digest"], EMPTY_SHA256, "empty.pm digest must be the empty-input hash");
        assert!(
            owners_for(&packet, file_id).is_empty(),
            "empty.pm must yield zero owners (no hallucinated decls)"
        );
        assert_accounted_for(&packet, "lib/Empty.pm", "empty file");
        surfaced += 1;
        report.push_str("empty.pm | file fact + empty digest, 0 owners\n");
    }

    // 3. Huge generated file (~2MB): fully extracted, nothing truncated away.
    {
        let mut huge = String::from("package Huge;\n");
        let mut sub_count = 0usize;
        while huge.len() < HUGE_TARGET_BYTES {
            huge.push_str(&format!(
                "sub func_{sub_count} {{ # {} \n    return {sub_count};\n}}\n",
                "pad ".repeat(200)
            ));
            sub_count += 1;
        }
        assert!(
            huge.len() >= HUGE_TARGET_BYTES,
            "generated huge fixture must reach ~2MB, got {} bytes",
            huge.len()
        );
        let root = fresh_root("huge");
        stage_file(&root, "lib/Huge.pm", &huge);
        let packet = build_packet(&root, None);
        let _ = std::fs::remove_dir_all(&root);

        let file_id = "file:lib/Huge.pm";
        assert!(
            must_some(packet["files"].as_array())
                .iter()
                .any(|f| f["file_id"].as_str() == Some(file_id)),
            "huge fixture must yield a file fact (not dropped/truncated)"
        );
        let owners = owners_for(&packet, file_id);
        let subs = owners.iter().filter(|o| o["kind"].as_str() == Some("sub")).count();
        assert_eq!(
            subs, sub_count,
            "huge fixture must extract every generated sub ({sub_count}), got {subs}"
        );
        assert_accounted_for(&packet, "lib/Huge.pm", "huge file");
        surfaced += 1;
        report.push_str(&format!(
            "huge Huge.pm ({} bytes) | file fact + {subs}/{sub_count} subs\n",
            huge.len()
        ));
    }

    // 4. Unknown test framework: honest `unknown`, never a guessed framework.
    {
        let source = must_with(
            std::fs::read_to_string(format!("{dir}/unknown_framework.t")),
            "read unknown_framework.t fixture",
        );
        let root = fresh_root("unknown-fw");
        stage_file(&root, "t/custom.t", &source);
        let packet = build_packet(&root, None);
        let _ = std::fs::remove_dir_all(&root);

        let test = must_some_with(
            must_some(packet["tests"].as_array())
                .iter()
                .find(|t| t["test_id"].as_str() == Some("test:t/custom.t")),
            "unknown-framework .t must yield a test fact (not dropped)",
        );
        assert_eq!(
            test["framework"], "unknown",
            "unrecognized harness must stay framework=unknown, not a guess"
        );
        assert_accounted_for(&packet, "t/custom.t", "unknown test framework");
        surfaced += 1;
        report.push_str("unknown_framework.t | test fact with framework=unknown\n");
    }

    // 5. File outside root (via diff hunk): limitation, no phantom change.
    {
        let diff = must_with(
            std::fs::read_to_string(format!("{dir}/outside_root.diff")),
            "read outside_root.diff fixture",
        );
        let root = fresh_root("outside-root");
        stage_file(&root, "lib/App.pm", "package App;\nsub run { 1 }\n1;\n");
        let packet = build_packet(&root, Some(&diff));
        let _ = std::fs::remove_dir_all(&root);

        assert!(
            limitation_ids(&packet).iter().any(|id| id.starts_with("diff-file-not-found:")),
            "outside-root hunk must surface a diff-file-not-found limitation; got {:?}",
            limitation_ids(&packet)
        );
        assert!(
            must_some(packet["changes"].as_array())
                .iter()
                .all(|c| { c["file_id"].as_str().is_none_or(|id| !id.contains("other-root")) }),
            "outside-root hunk must not produce a phantom change fact"
        );
        surfaced += 1;
        report.push_str("outside_root.diff | diff-file-not-found limitation, 0 phantom changes\n");
    }

    assert_eq!(
        surfaced, EXPECTED_SURFACED,
        "E3 bar: 100% of hostile inputs surfaced ({EXPECTED_SURFACED}/{EXPECTED_SURFACED}), 0 drops\n{report}"
    );
}
