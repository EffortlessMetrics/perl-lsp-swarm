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
fn registry_packet_targets_only_the_existing_perl_entry() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text =
        fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/registry/manifest.toml"))?;
    let manifest: toml::Value = toml::from_str(&text)?;
    let extension = manifest
        .get("extension")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("missing extension section"))?;
    assert_eq!(extension.get("id").and_then(toml::Value::as_str), Some("perl"));
    assert_eq!(
        extension.get("submodule_path").and_then(toml::Value::as_str),
        Some("extensions/perl")
    );
    assert_eq!(
        extension.get("submodule_remote").and_then(toml::Value::as_str),
        Some("https://github.com/tree-sitter-perl/zed-perl.git")
    );
    let changed: Vec<&str> = manifest
        .get("submission")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("expected_changed_paths"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("missing expected changed paths"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(changed, vec!["extensions/perl", "extensions.toml"]);
    Ok(())
}

#[test]
fn registry_packet_cannot_invent_the_future_merge() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text =
        fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/registry/manifest.toml"))?;
    let manifest: toml::Value = toml::from_str(&text)?;
    assert_eq!(
        manifest.get("status").and_then(toml::Value::as_str),
        Some("blocked_pending_upstream_merge")
    );
    assert_eq!(manifest.get("ready").and_then(toml::Value::as_bool), Some(false));
    let extension = manifest
        .get("extension")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("missing extension section"))?;
    assert_eq!(extension.get("new_version").and_then(toml::Value::as_str), Some(""));
    assert_eq!(extension.get("new_commit").and_then(toml::Value::as_str), Some(""));
    let body = fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/registry/pr-body.md"))?;
    assert!(body.contains("[BLOCKED:"));
    Ok(())
}
