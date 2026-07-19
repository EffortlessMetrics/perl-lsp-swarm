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
    assert_eq!(buckets.get("merge_ready"), Some(&serde_json::json!([1])));
    assert_eq!(buckets.get("ci_green"), Some(&serde_json::json!([1])));
    assert_eq!(buckets.get("conflicting"), Some(&serde_json::json!([2])));
    assert_eq!(buckets.get("unknown_not_proven"), Some(&serde_json::json!([3])));
    assert_eq!(buckets.get("pending_or_unclassified"), Some(&serde_json::json!([])));
    assert!(buckets.get("stale_or_dirty").is_none());
    assert!(buckets.get("blocked_unknown").is_none());
}
