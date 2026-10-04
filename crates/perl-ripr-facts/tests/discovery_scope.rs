//! Discovery-scope honesty (#17259).
//!
//! `files[]` walks the whole root, but tests/oracles/verify scan only
//! `<root>/t` and boundaries/relations only `<root>/lib`. A `.t` outside `t/`
//! used to sit in `files[]` (role `test`) with silently zero test facts. Such
//! skips must surface as a `discovery-scope-split` limitation naming the
//! skipped paths; conventional `lib/`+`t/` layouts gain no new noise.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_with};

const APP_PM: &str = "package App;\nsub run { 1 }\n1;\n";
const APP_T: &str = "use Test::More;\nok(1, 'smoke');\ndone_testing();\n";
const EXTRA_T: &str = "use Test::More;\nok(1, 'extra');\ndone_testing();\n";
const HELPER_PM: &str = "package Helper;\nsub help { 1 }\n1;\n";

fn build_packet(root: &str, fact_classes: &str) -> serde_json::Value {
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
    let root = format!("target/ripr-g7/{name}");
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn limitation_ids(packet: &serde_json::Value) -> Vec<String> {
    must_some(packet["limitations"].as_array())
        .iter()
        .filter_map(|l| l["limitation_id"].as_str().map(String::from))
        .collect()
}

fn scope_limitation(packet: &serde_json::Value) -> &serde_json::Value {
    must_some(
        must_some(packet["limitations"].as_array())
            .iter()
            .find(|l| l["limitation_id"].as_str() == Some("discovery-scope-split")),
    )
}

#[test]
fn out_of_scope_files_are_limited_not_silent() {
    let root = fresh_root("split");
    stage_file(&root, "lib/App.pm", APP_PM);
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "xt/extra.t", EXTRA_T);
    stage_file(&root, "script/Helper.pm", HELPER_PM);
    let packet =
        build_packet(&root, "files,owners,tests,oracles,dynamic_boundaries,verify_commands");
    let _ = std::fs::remove_dir_all(&root);

    // The skipped .t is in files[] (role test) but yields no test fact — the
    // exact silent shape from the issue; now it must be named.
    assert!(
        must_some(packet["files"].as_array())
            .iter()
            .any(|f| f["file_id"].as_str() == Some("file:xt/extra.t")),
        "skipped .t must still appear in files[]"
    );
    assert!(
        must_some(packet["tests"].as_array())
            .iter()
            .all(|t| t["test_id"].as_str() != Some("test:xt/extra.t")),
        "skipped .t must yield no test fact (scope unchanged — Option 2)"
    );
    let ids = limitation_ids(&packet);
    assert!(ids.contains(&"discovery-scope-split".to_string()), "got {ids:?}");
    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    assert!(message.contains("xt/extra.t"), "message must name the skipped .t; got {message:?}");
    assert!(
        message.contains("script/Helper.pm"),
        "message must name the skipped .pm; got {message:?}"
    );
    let refs: Vec<&str> = must_some(limitation["evidence_refs"].as_array())
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(refs.contains(&"file:xt/extra.t"), "evidence must name skipped ids; got {refs:?}");
    assert!(refs.contains(&"file:script/Helper.pm"), "got {refs:?}");
}

#[test]
fn conventional_layout_carries_no_scope_limitation() {
    let root = fresh_root("conventional");
    stage_file(&root, "lib/App.pm", APP_PM);
    stage_file(&root, "t/app.t", APP_T);
    let packet =
        build_packet(&root, "files,owners,tests,oracles,dynamic_boundaries,verify_commands");
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        !limitation_ids(&packet).contains(&"discovery-scope-split".to_string()),
        "conventional lib/+t/ layout must gain no scope limitation; got {:?}",
        limitation_ids(&packet)
    );
}

#[test]
fn files_only_request_carries_no_scope_limitation() {
    let root = fresh_root("files-only");
    stage_file(&root, "xt/extra.t", EXTRA_T);
    let packet = build_packet(&root, "files,owners");
    let _ = std::fs::remove_dir_all(&root);

    // No scope-sensitive class requested: nothing to explain, no new noise.
    assert!(
        !limitation_ids(&packet).contains(&"discovery-scope-split".to_string()),
        "files-only request must gain no scope limitation; got {:?}",
        limitation_ids(&packet)
    );
}
