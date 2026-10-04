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
const RUN_PL: &str = "use strict;\nsub run { eval { die }; }\nrun();\n";
const EXTRA_PL: &str = "use strict;\nprint 'extra';\n";
const APP_PSGI: &str = "use strict;\nmy $app = sub { [200, [], ['ok']] };\n";

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

#[test]
fn pl_and_psgi_skips_are_named_anywhere() {
    // No scoped collector scans `.pl`/`.psgi` at all, so they are skips
    // wherever they live — even directly under `lib/`.
    let root = fresh_root("pl-psgi");
    stage_file(&root, "lib/App.pm", APP_PM);
    stage_file(&root, "lib/extra.pl", EXTRA_PL);
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "script/run.pl", RUN_PL);
    stage_file(&root, "app.psgi", APP_PSGI);
    let packet = build_packet(
        &root,
        "files,owners,tests,oracles,relations,dynamic_boundaries,verify_commands",
    );
    let _ = std::fs::remove_dir_all(&root);

    // The `eval` in script/run.pl yields no boundary (scope unchanged —
    // Option 2) — but it must no longer be a silent skip either.
    assert!(
        must_some(packet["dynamic_boundaries"].as_array())
            .iter()
            .all(|b| b["file_id"].as_str() != Some("file:script/run.pl")),
        "unscanned .pl must yield no boundary fact"
    );
    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    for skipped in ["script/run.pl", "lib/extra.pl", "app.psgi"] {
        assert!(message.contains(skipped), "message must name {skipped}; got {message:?}");
    }
    assert!(
        message.contains("appear in `files[]`"),
        "files-present packet must use the files-present wording; got {message:?}"
    );
    assert!(
        message.contains("excluded from the requested scoped facts or commands"),
        "files-present wording must name the exclusion from requested facts/commands; got {message:?}"
    );
    let refs: Vec<&str> = must_some(limitation["evidence_refs"].as_array())
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    for skipped in ["file:script/run.pl", "file:lib/extra.pl", "file:app.psgi"] {
        assert!(refs.contains(&skipped), "evidence must name skipped ids; got {refs:?}");
    }
}

#[test]
fn tests_only_request_reports_t_skips_not_pm_skips() {
    // Split gating: `.t` feeds tests, but `.pm` does not — a tests-only
    // request must not report unrelated source skips.
    let root = fresh_root("tests-only-gating");
    stage_file(&root, "lib/App.pm", APP_PM);
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "xt/extra.t", EXTRA_T);
    stage_file(&root, "script/Helper.pm", HELPER_PM);
    let packet = build_packet(&root, "tests");
    let _ = std::fs::remove_dir_all(&root);

    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    assert!(message.contains("xt/extra.t"), "tests-only must name the .t skip; got {message:?}");
    assert!(
        !message.contains("script/Helper.pm"),
        "tests-only must not report the unrelated .pm skip; got {message:?}"
    );
    let refs: Vec<&str> = must_some(limitation["evidence_refs"].as_array())
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(refs.contains(&"file:xt/extra.t"), "got {refs:?}");
    assert!(!refs.contains(&"file:script/Helper.pm"), "got {refs:?}");
}

#[test]
fn relations_request_reports_both_t_and_source_skips() {
    // Relations consume both sides (test files + source files), so a
    // relations/dynamic_boundaries request reports both skip classes.
    let root = fresh_root("relations-gating");
    stage_file(&root, "lib/App.pm", APP_PM);
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "xt/extra.t", EXTRA_T);
    stage_file(&root, "script/Helper.pm", HELPER_PM);
    let packet = build_packet(&root, "relations,dynamic_boundaries");
    let _ = std::fs::remove_dir_all(&root);

    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    assert!(message.contains("xt/extra.t"), "relations must name the .t skip; got {message:?}");
    assert!(
        message.contains("script/Helper.pm"),
        "relations must name the .pm skip; got {message:?}"
    );
    let refs: Vec<&str> = must_some(limitation["evidence_refs"].as_array())
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(refs.contains(&"file:xt/extra.t"), "got {refs:?}");
    assert!(refs.contains(&"file:script/Helper.pm"), "got {refs:?}");
}

#[test]
fn verify_only_request_names_excluded_commands() {
    // A verify-only caller requests commands, not facts — the limitation
    // must still name the out-of-scope .t as excluded from the requested
    // scoped facts or commands (PRRT_kwDOSid81M6o2Z3P).
    let root = fresh_root("verify-only");
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "xt/extra.t", EXTRA_T);
    let packet = build_packet(&root, "verify_commands");
    let _ = std::fs::remove_dir_all(&root);

    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    assert!(message.contains("xt/extra.t"), "message must name the skip; got {message:?}");
    assert!(
        message.contains("excluded from the requested scoped facts or commands"),
        "verify-only wording must name the exclusion from requested facts/commands; got {message:?}"
    );
}

#[test]
fn subset_packet_uses_files_absent_wording() {
    // A tests-only packet carries empty `files[]` — the message must not
    // claim the skips "appear in `files[]`"; the `file:` evidence refs are
    // path-derived ids, stated as such.
    let root = fresh_root("files-absent");
    stage_file(&root, "t/app.t", APP_T);
    stage_file(&root, "xt/extra.t", EXTRA_T);
    let packet = build_packet(&root, "tests");
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        must_some(packet["files"].as_array()).is_empty(),
        "tests-only packet must carry empty files[]"
    );
    let limitation = scope_limitation(&packet);
    let message = must_some(limitation["message"].as_str());
    assert!(message.contains("xt/extra.t"), "message must name the skip; got {message:?}");
    assert!(
        !message.contains("appear in `files[]`"),
        "files-absent packet must not claim files[] presence; got {message:?}"
    );
    assert!(
        message.contains("not in this packet") && message.contains("path-derived"),
        "files-absent wording must state the ids are path-derived; got {message:?}"
    );
    let refs: Vec<&str> = must_some(limitation["evidence_refs"].as_array())
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(refs.contains(&"file:xt/extra.t"), "got {refs:?}");
}
