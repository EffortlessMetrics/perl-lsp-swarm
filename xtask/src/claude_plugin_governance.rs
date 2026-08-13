use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const CLAUDE_PLUGIN_SLUG: &str = "perl-lsp-rs";
pub const PACKAGE_IDENTITY_SCHEMA: &str = "claude_plugin_package_identity.v1";

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct PluginVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl PluginVersion {
    fn parse(value: &str) -> Result<Self> {
        let parts = value.split('.').collect::<Vec<_>>();
        ensure!(parts.len() == 3, "plugin version must be semantic x.y.z: {value}");
        Ok(Self {
            major: parts[0].parse().context("invalid plugin major version")?,
            minor: parts[1].parse().context("invalid plugin minor version")?,
            patch: parts[2].parse().context("invalid plugin patch version")?,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PluginPackageIdentity {
    pub slug: String,
    pub version: String,
    pub tree_digest: String,
    pub manifest_digest: String,
    pub lsp_digest: String,
    pub inventory_digest: String,
    pub inventory: Vec<String>,
}

impl PluginPackageIdentity {
    pub fn to_json(&self) -> Value {
        json!({
            "schema_version": PACKAGE_IDENTITY_SCHEMA,
            "slug": self.slug,
            "version": self.version,
            "tree_digest": self.tree_digest,
            "manifest_digest": self.manifest_digest,
            "lsp_digest": self.lsp_digest,
            "inventory_digest": self.inventory_digest,
            "inventory": self.inventory,
        })
    }
}

pub fn inspect_plugin_package(root: &Path) -> Result<PluginPackageIdentity> {
    ensure!(root.is_dir(), "Claude plugin root is not a directory: {}", root.display());

    let files = collect_package_files(root)?;
    ensure!(
        files.contains_key(".claude-plugin/plugin.json"),
        "Claude plugin package is missing .claude-plugin/plugin.json"
    );
    ensure!(files.contains_key(".lsp.json"), "Claude plugin package is missing .lsp.json");
    ensure!(
        !files.contains_key(".mcp.json"),
        "Claude native-LSP plugin package must not add MCP"
    );
    ensure!(
        !files.keys().any(|path| path == "bin" || path.starts_with("bin/")),
        "Claude plugin package must not bundle a binary"
    );

    let manifest_path = files
        .get(".claude-plugin/plugin.json")
        .context("manifest disappeared after inventory")?;
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    let manifest = manifest.as_object().context("Claude plugin manifest must be an object")?;
    let slug = manifest
        .get("name")
        .and_then(Value::as_str)
        .context("Claude plugin manifest is missing string name")?;
    ensure!(slug == CLAUDE_PLUGIN_SLUG, "unexpected Claude plugin slug: {slug}");
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .context("Claude plugin manifest is missing string version")?;
    PluginVersion::parse(version)?;

    let lsp_path = files.get(".lsp.json").context("LSP config disappeared after inventory")?;
    let lsp_bytes = fs::read(lsp_path)?;

    let inventory = files.keys().cloned().collect::<Vec<_>>();
    let inventory_digest = digest_inventory(&inventory);
    let tree_digest = digest_tree(&files)?;

    Ok(PluginPackageIdentity {
        slug: slug.to_string(),
        version: version.to_string(),
        tree_digest,
        manifest_digest: digest_bytes(&manifest_bytes),
        lsp_digest: digest_bytes(&lsp_bytes),
        inventory_digest,
        inventory,
    })
}

pub fn validate_package_transition(
    previous: &PluginPackageIdentity,
    current: &PluginPackageIdentity,
) -> Result<()> {
    ensure!(
        previous.slug == current.slug,
        "plugin slug movement requires an explicit identity migration, not a package update"
    );

    let previous_version = PluginVersion::parse(&previous.version)?;
    let current_version = PluginVersion::parse(&current.version)?;
    ensure!(
        current_version >= previous_version,
        "Claude plugin version regressed from {} to {}",
        previous.version,
        current.version
    );

    if current.tree_digest != previous.tree_digest {
        ensure!(
            current_version > previous_version,
            "Claude plugin bytes changed under unchanged/non-increasing version {}",
            current.version
        );
    }
    Ok(())
}

fn collect_package_files(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!("Claude plugin package contains symlink: {}", entry.path().display());
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?;
        let normalized = relative.to_string_lossy().replace('\\', "/");
        ensure!(!normalized.is_empty(), "empty package path");
        ensure!(
            files.insert(normalized.clone(), entry.path().to_path_buf()).is_none(),
            "duplicate normalized package path: {normalized}"
        );
    }
    Ok(files)
}

fn digest_tree(files: &BTreeMap<String, PathBuf>) -> Result<String> {
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(path)?);
        hasher.update([0]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn digest_inventory(inventory: &[String]) -> String {
    let mut hasher = Sha256::new();
    for relative in inventory {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
