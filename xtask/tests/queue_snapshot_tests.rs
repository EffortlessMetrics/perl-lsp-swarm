// Integration test: `expect()` carries the assertion message on fixture and
// CLI-output parsing. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
use assert_cmd::cargo::cargo_bin_cmd;

fn fixture(path: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(manifest).join("tests").join("fixtures").join(path).display().to_string()
}

#[test]
fn queue_snapshot_from_fixture_derives_distinct_mergeability_buckets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let out = temp.path().join("snapshot.json");

    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.args([
        "queue",
        "snapshot",
        "--fixture",
        fixture("queue-snapshot/snapshot-fixture.json").as_str(),
        "--out",
        out.display().to_string().as_str(),
    ])
    .assert()
    .success();

    let rendered = std::fs::read_to_string(out).expect("read snapshot");
    let snapshot: serde_json::Value = serde_json::from_str(&rendered).expect("parse snapshot");
    let buckets = snapshot.get("buckets").expect("buckets");

    assert_eq!(snapshot.get("default_branch").and_then(serde_json::Value::as_str), Some("main"));
    let legacy_main_sha =
        snapshot.get("master_sha").and_then(serde_json::Value::as_str).expect("legacy master_sha");
    assert!(!legacy_main_sha.is_empty(), "legacy master_sha must remain populated");
    assert_eq!(buckets.get("mergeable_clean"), Some(&serde_json::json!([1])));
    assert_eq!(buckets.get("ci_green"), Some(&serde_json::json!([1])));
    assert!(buckets.get("merge_ready").is_none(), "retired merge_ready bucket must be absent");
    assert!(
        buckets.get("needs_builder_fix").is_none(),
        "retired needs_builder_fix bucket must be absent"
    );
    assert!(
        buckets.get("needs_diff_fix").is_none(),
        "retired needs_diff_fix bucket must be absent"
    );
    assert!(
        buckets.get("diff_audited_waiting_ci").is_none(),
        "retired diff_audited_waiting_ci bucket must be absent"
    );
    assert_eq!(buckets.get("conflicting"), Some(&serde_json::json!([2])));
    assert_eq!(buckets.get("unknown_not_proven"), Some(&serde_json::json!([3])));
    assert_eq!(buckets.get("pending_or_unclassified"), Some(&serde_json::json!([])));
    assert!(buckets.get("stale_or_dirty").is_none());
    assert!(buckets.get("blocked_unknown").is_none());
}
