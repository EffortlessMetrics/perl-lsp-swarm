use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use xtask::claude_plugin_governance::{
    CLAUDE_PLUGIN_SLUG, PluginPackageIdentity, inspect_plugin_package, validate_package_transition,
};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn identity(version: &str, tree: &str) -> PluginPackageIdentity {
    PluginPackageIdentity {
        slug: CLAUDE_PLUGIN_SLUG.to_string(),
        version: version.to_string(),
        tree_digest: tree.to_string(),
        manifest_digest: "sha256:manifest".to_string(),
        lsp_digest: "sha256:lsp".to_string(),
        inventory_digest: "sha256:inventory".to_string(),
        inventory: vec![".claude-plugin/plugin.json".to_string(), ".lsp.json".to_string()],
    }
}

fn write_fixture(root: &Path, version: &str, extra: Option<(&str, &str)>) -> Result<()> {
    fs::create_dir_all(root.join(".claude-plugin"))?;
    fs::write(
        root.join(".claude-plugin/plugin.json"),
        format!(r#"{{"name":"perl-lsp-rs","version":"{version}"}}"#),
    )?;
    fs::write(root.join(".lsp.json"), r#"{"perl":{"command":"perllsp"}}"#)?;
    if let Some((path, content)) = extra {
        let path = root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
    }
    Ok(())
}

#[test]
fn current_plugin_has_deterministic_complete_tree_identity() -> Result<()> {
    let root = repository_root()?.join("integrations/claude-code/plugins/perl-lsp-rs");
    let first = inspect_plugin_package(&root)?;
    let second = inspect_plugin_package(&root)?;

    ensure!(first == second, "same plugin tree produced different package identity");
    ensure!(first.slug == CLAUDE_PLUGIN_SLUG);
    ensure!(first.version == "0.1.0");
    ensure!(first.tree_digest.starts_with("sha256:"));
    ensure!(first.tree_digest.len() == 71);
    ensure!(first.manifest_digest.len() == 71);
    ensure!(first.lsp_digest.len() == 71);
    ensure!(first.inventory_digest.len() == 71);

    let json_a = serde_json::to_string_pretty(&first.to_json())?;
    let json_b = serde_json::to_string_pretty(&second.to_json())?;
    ensure!(json_a == json_b, "machine-readable package identity is not deterministic");
    Ok(())
}

#[test]
fn changed_tree_requires_plugin_version_advance() -> Result<()> {
    let previous = identity("0.1.0", "sha256:tree-a");
    let unchanged_version = identity("0.1.0", "sha256:tree-b");
    let advanced_version = identity("0.1.1", "sha256:tree-b");

    ensure!(validate_package_transition(&previous, &unchanged_version).is_err());
    validate_package_transition(&previous, &advanced_version)?;
    Ok(())
}

#[test]
fn unchanged_tree_may_keep_version_but_version_cannot_regress() -> Result<()> {
    let previous = identity("0.2.0", "sha256:same-tree");
    validate_package_transition(&previous, &identity("0.2.0", "sha256:same-tree"))?;
    validate_package_transition(&previous, &identity("0.2.1", "sha256:same-tree"))?;
    ensure!(
        validate_package_transition(&previous, &identity("0.1.9", "sha256:same-tree")).is_err()
    );
    Ok(())
}

#[test]
fn inspection_covers_every_installable_byte() -> Result<()> {
    let temp = tempdir()?;
    write_fixture(temp.path(), "0.1.0", None)?;
    let before = inspect_plugin_package(temp.path())?;

    fs::write(temp.path().join("README.md"), "changed package byte")?;
    let after = inspect_plugin_package(temp.path())?;
    ensure!(before.tree_digest != after.tree_digest);
    ensure!(before.inventory_digest != after.inventory_digest);
    ensure!(validate_package_transition(&before, &after).is_err());
    Ok(())
}

#[test]
fn inspection_rejects_mcp_and_bundled_binary_surfaces() -> Result<()> {
    let temp = tempdir()?;
    write_fixture(temp.path(), "0.1.0", Some((".mcp.json", "{}")))?;
    ensure!(inspect_plugin_package(temp.path()).is_err());

    let temp = tempdir()?;
    write_fixture(temp.path(), "0.1.0", Some(("bin/perllsp", "not actually a binary")))?;
    ensure!(inspect_plugin_package(temp.path()).is_err());
    Ok(())
}
