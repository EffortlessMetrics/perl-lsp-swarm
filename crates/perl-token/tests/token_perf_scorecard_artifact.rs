#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

#[allow(dead_code)] // Bench-only helpers are compiled in via #[path] for path tests.
#[path = "../benches/support/perf_scorecard.rs"]
mod perf_scorecard;

use perf_scorecard::{
    PUBLISH_ENV, ScoreMetric, TRACKED_ARTIFACT_RELATIVE_PATH, is_tracked_docs_artifact,
    publish_requested_from, resolve_artifact_path, write_metric,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after epoch").as_nanos();
    let dir = std::env::temp_dir()
        .join(format!("perl-token-scorecard-{label}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn make_repo_layout(root: &Path) {
    fs::create_dir_all(root.join("docs/project/status")).expect("status dir");
    fs::create_dir_all(root.join("crates/perl-token")).expect("crate dir");
}

fn sample(name_tag: u64) -> ScoreMetric {
    ScoreMetric { iterations: 5, median_ns: name_tag, p95_ns: name_tag + 1 }
}

#[test]
fn default_path_with_cargo_target_dir_is_not_tracked_docs() {
    let root = unique_temp_dir("target-dir");
    make_repo_layout(&root);
    let target_dir = root.join("custom-target");
    let path = resolve_artifact_path(&root, Some(&target_dir), false)
        .expect("default path with CARGO_TARGET_DIR");
    assert!(
        !is_tracked_docs_artifact(&path),
        "default write must not target tracked docs: {path:?}"
    );
    assert_eq!(path, target_dir.join("token_performance_scorecard.json"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn default_path_without_cargo_target_dir_uses_repo_target() {
    let root = unique_temp_dir("repo-target");
    make_repo_layout(&root);
    let path = resolve_artifact_path(&root, None, false).expect("default path under repo target");
    assert!(
        !is_tracked_docs_artifact(&path),
        "default write must not target tracked docs: {path:?}"
    );
    assert_eq!(path, root.join("target/token_performance_scorecard.json"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn publish_flag_selects_tracked_docs_even_when_target_dir_is_set() {
    let root = unique_temp_dir("publish");
    make_repo_layout(&root);
    let target_dir = root.join("custom-target");
    let path = resolve_artifact_path(&root, Some(&target_dir), true).expect("publish path");
    assert!(
        is_tracked_docs_artifact(&path),
        "governed publication must write the tracked docs artifact: {path:?}"
    );
    assert_eq!(path, root.join(TRACKED_ARTIFACT_RELATIVE_PATH));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn publish_env_is_only_the_explicit_one_flag() {
    assert!(publish_requested_from(Some("1")));
    assert!(!publish_requested_from(None));
    assert!(!publish_requested_from(Some("")));
    assert!(!publish_requested_from(Some("true")));
    assert!(!publish_requested_from(Some("0")));
    assert_eq!(PUBLISH_ENV, "PERL_LSP_PUBLISH_TOKEN_SCORECARD");
}

#[test]
fn default_write_leaves_tracked_docs_untouched() {
    let root = unique_temp_dir("no-mutate-docs");
    make_repo_layout(&root);
    let tracked = root.join(TRACKED_ARTIFACT_RELATIVE_PATH);
    let sentinel = "{\"schema_version\":1,\"generated_at_epoch_s\":1,\"metrics\":{}}\n";
    fs::write(&tracked, sentinel).expect("write sentinel");

    let local = resolve_artifact_path(&root, None, false).expect("local path");
    write_metric(&local, "token_clone", sample(7));

    let after = fs::read_to_string(&tracked).expect("reread tracked docs");
    assert_eq!(after, sentinel, "ordinary scorecard write must not rewrite tracked docs");
    assert!(local.is_file(), "ordinary write should still emit a local artifact");
    let written = fs::read_to_string(&local).expect("read local artifact");
    assert!(written.contains("token_clone"), "local artifact must record the metric");
    assert!(!written.contains(sentinel.trim()), "local artifact is distinct from the sentinel");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn publish_write_updates_tracked_docs() {
    let root = unique_temp_dir("publish-write");
    make_repo_layout(&root);
    let tracked = root.join(TRACKED_ARTIFACT_RELATIVE_PATH);
    fs::write(&tracked, "{}\n").expect("write placeholder");

    let path = resolve_artifact_path(&root, None, true).expect("publish path");
    write_metric(&path, "token_clone", sample(9));

    let after = fs::read_to_string(&tracked).expect("reread tracked docs");
    assert!(after.contains("token_clone"), "publish write must update the tracked artifact");
    assert!(after.contains("\"median_ns\": 9"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_repo_markers_without_target_dir_does_not_invent_a_docs_path() {
    let root = unique_temp_dir("no-repo");
    assert!(resolve_artifact_path(&root, None, false).is_none());
    assert!(resolve_artifact_path(&root, None, true).is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cargo_target_dir_collision_with_tracked_docs_does_not_publish() {
    let root = unique_temp_dir("collision");
    make_repo_layout(&root);
    let colliding = root.join("docs/project/status");
    let path = resolve_artifact_path(&root, Some(&colliding), false)
        .expect("collision must fall back to repo target");
    assert!(
        !is_tracked_docs_artifact(&path),
        "CARGO_TARGET_DIR pointing at the status dir must not select tracked docs: {path:?}"
    );
    assert_eq!(path, root.join("target/token_performance_scorecard.json"));

    let relative = Path::new("docs/project/status");
    let relative_path = resolve_artifact_path(&root, Some(relative), false)
        .expect("relative collision must fall back to repo target");
    assert!(
        !is_tracked_docs_artifact(&relative_path),
        "relative CARGO_TARGET_DIR collision must not select tracked docs: {relative_path:?}"
    );
    assert_eq!(relative_path, root.join("target/token_performance_scorecard.json"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cargo_target_dir_parent_alias_does_not_publish() {
    let root = unique_temp_dir("parent-alias");
    make_repo_layout(&root);
    let aliased = root.join("docs/project/status/../status");
    let path = resolve_artifact_path(&root, Some(&aliased), false)
        .expect("parent-dir alias must fall back to repo target");
    assert!(
        !is_tracked_docs_artifact(&path),
        "CARGO_TARGET_DIR parent-dir alias must not select tracked docs: {path:?}"
    );
    assert_eq!(path, root.join("target/token_performance_scorecard.json"));

    let relative = Path::new("docs/project/status/../status");
    let relative_path = resolve_artifact_path(&root, Some(relative), false)
        .expect("relative parent-dir alias must fall back to repo target");
    assert!(!is_tracked_docs_artifact(&relative_path));
    assert_eq!(relative_path, root.join("target/token_performance_scorecard.json"));
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn cargo_target_dir_symlink_alias_does_not_publish() {
    let root = unique_temp_dir("symlink-alias");
    make_repo_layout(&root);
    let alias = root.join("alias-status");
    std::os::unix::fs::symlink(root.join("docs/project/status"), &alias)
        .expect("symlink status dir");
    let path = resolve_artifact_path(&root, Some(&alias), false)
        .expect("symlink alias must fall back to repo target");
    assert!(
        !is_tracked_docs_artifact(&path),
        "CARGO_TARGET_DIR symlink alias must not select tracked docs: {path:?}"
    );
    assert_eq!(path, root.join("target/token_performance_scorecard.json"));
    let _ = fs::remove_dir_all(&root);
}
