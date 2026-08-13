use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

#[test]
fn submission_packet_is_explicitly_blocked() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = fs::read_to_string(
        root.join(".ci/fixtures/zed-perl-upstream/submission/manifest.toml"),
    )?;
    let manifest: toml::Value = toml::from_str(&text)?;
    assert_eq!(
        manifest.get("status").and_then(toml::Value::as_str),
        Some("blocked_pending_fan_in")
    );
    assert_eq!(
        manifest.get("ready").and_then(toml::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        manifest
            .get("submission")
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get("submission_order"))
            .and_then(toml::Value::as_str),
        Some("unresolved_pending_actual_host")
    );
    let blockers = manifest
        .get("blockers")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("missing packet blockers"))?;
    assert!(blockers.len() >= 7);
    Ok(())
}

#[test]
fn upstream_pr_body_cannot_masquerade_as_ready() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let body = fs::read_to_string(
        root.join(".ci/fixtures/zed-perl-upstream/submission/pr-body.md"),
    )?;
    assert!(body.contains("[BLOCKED:"));
    assert!(body.contains("perlnavigator-server -> Perl Navigator"));
    assert!(body.contains("perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp"));
    assert!(body.contains("perllsp              -> EffortlessMetrics/perl-lsp"));
    assert!(body.contains("perllsp --stdio"));
    Ok(())
}
