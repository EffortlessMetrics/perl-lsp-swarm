//! Typed discovery and inventory for executable install and package surfaces.
//!
//! This layer is diagnostic. It records repository truth without changing route
//! status, public guidance, release topology, or channel state.

use clap::Parser;
use color_eyre::eyre::{bail, eyre, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use walkdir::{DirEntry, WalkDir};

const REGISTRY_SCHEMA: &str = "install_surface_registry.v1";
const REPORT_SCHEMA: &str = "install_surface_inventory.v1";
const MAX_DISCOVERY_FILE_BYTES: u64 = 1_048_576;

#[derive(Debug, Parser)]
#[command(
    name = "install-surface-inventory",
    about = "Discover and report executable install/package surfaces"
)]
struct Cli {
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
    #[arg(long, default_value = "policy/install-surface-registry.toml")]
    registry: PathBuf,
    #[arg(
        long,
        default_value = "target/receipts/install-surface-inventory.json"
    )]
    output: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema_version: String,
    pub surfaces: Vec<SurfaceRow>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRow {
    pub surface_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<String>,
    pub surface_kind: SurfaceKind,
    pub producer: String,
    pub consumer: String,
    pub target_channel: String,
    pub installed_product_unit: ProductUnit,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_refs: Vec<String>,
    pub topology_relationship: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_currentness_owner: Option<String>,
    pub publication_stage: String,
    pub active_user_reachability: String,
    pub disposition: Disposition,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_condition: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    StandaloneInstaller,
    PackageSource,
    PackageMetadata,
    PublisherWorkflow,
    ReleaseWorkflow,
    SetupAction,
    ManagedDownloader,
    Documentation,
    Specification,
    Generator,
    Validator,
    HistoricalFixture,
    Other,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProductUnit {
    Server,
    ServerDapPair,
    ManagedServerDapPair,
    EditorPackage,
    PackageSource,
    DocumentationOnly,
    NotApplicable,
    Unknown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    CanonicalGenerated,
    ActiveOwnedChannelSource,
    CandidateOnly,
    DeferredScaffold,
    HistoricalFixture,
    Retired,
    NeedsDisposition,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredSurface {
    pub suggested_surface_id: String,
    pub path: String,
    pub surface_kind: SurfaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_surface_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FindingCode {
    UnregisteredSurface,
    StaleRegistryPath,
    MissingConsumer,
    MissingAuthority,
    DuplicateChannelProductRole,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: FindingCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: String,
    pub registry_schema_version: String,
    pub registry_path: String,
    pub registered_surface_count: usize,
    pub discovered_surface_count: usize,
    pub disposition_counts: BTreeMap<String, usize>,
    pub surface_kind_counts: BTreeMap<String, usize>,
    pub discovered_surfaces: Vec<DiscoveredSurface>,
    pub findings: Vec<Finding>,
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let root = cli
        .repo_root
        .canonicalize()
        .wrap_err_with(|| format!("canonicalize repository root {}", cli.repo_root.display()))?;
    let registry_path = under_root(&root, &cli.registry);
    let output_path = under_root(&root, &cli.output);
    let registry = load_registry(&registry_path)?;
    let report = evaluate(&root, &registry, relative_display(&root, &registry_path)?)?;
    write_report(&output_path, &report)?;
    println!(
        "install surface inventory: {} registered, {} discovered, {} finding(s)",
        report.registered_surface_count,
        report.discovered_surface_count,
        report.findings.len()
    );
    println!("receipt: {}", relative_display(&root, &output_path)?);
    Ok(())
}

fn under_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn relative_display(root: &Path, path: &Path) -> Result<String> {
    normalize(
        path.strip_prefix(root)
            .wrap_err_with(|| format!("{} is outside repository root", path.display()))?,
    )
}

fn load_registry(path: &Path) -> Result<Registry> {
    let source = fs::read_to_string(path)
        .wrap_err_with(|| format!("read install surface registry {}", path.display()))?;
    let registry: Registry = toml::from_str(&source)
        .wrap_err_with(|| format!("parse install surface registry {}", path.display()))?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_registry(registry: &Registry) -> Result<()> {
    if registry.schema_version != REGISTRY_SCHEMA {
        bail!(
            "unsupported registry schema {:?}; expected {:?}",
            registry.schema_version,
            REGISTRY_SCHEMA
        );
    }
    if registry.surfaces.is_empty() {
        bail!("install surface registry must contain at least one row");
    }
    let mut ids = BTreeSet::new();
    for row in &registry.surfaces {
        validate_id(&row.surface_id)?;
        if !ids.insert(row.surface_id.as_str()) {
            bail!("duplicate install surface id {:?}", row.surface_id);
        }
        match (&row.path, &row.external_identity) {
            (Some(path), None) => validate_path(path)?,
            (None, Some(identity)) if !identity.trim().is_empty() => {}
            (Some(_), Some(_)) => bail!(
                "surface {:?} must use path or external_identity, not both",
                row.surface_id
            ),
            _ => bail!(
                "surface {:?} must define path or external_identity",
                row.surface_id
            ),
        }
        if row.owner.trim().is_empty() {
            bail!("surface {:?} has an empty owner", row.surface_id);
        }
        if matches!(
            row.disposition,
            Disposition::DeferredScaffold | Disposition::NeedsDisposition
        ) && row
            .exit_condition
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            bail!("surface {:?} requires an exit_condition", row.surface_id);
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        bail!("invalid install surface id {id:?}");
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    let parsed = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || parsed.is_absolute()
        || parsed
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::RootDir))
    {
        bail!("install surface path must be normalized and repository-relative: {path:?}");
    }
    Ok(())
}

fn evaluate(root: &Path, registry: &Registry, registry_path: String) -> Result<Report> {
    let discovered = discover(root)?;
    let rows_by_path: BTreeMap<&str, &SurfaceRow> = registry
        .surfaces
        .iter()
        .filter_map(|row| row.path.as_deref().map(|path| (path, row)))
        .collect();
    let mut discovered_surfaces = Vec::with_capacity(discovered.len());
    let mut findings = Vec::new();

    for (path, kind) in discovered {
        let registered = rows_by_path.get(path.as_str()).copied();
        if registered.is_none() {
            findings.push(Finding {
                code: FindingCode::UnregisteredSurface,
                surface_id: None,
                path: Some(path.clone()),
                detail: "discovered install/package-looking surface has no registry row".to_string(),
            });
        }
        discovered_surfaces.push(DiscoveredSurface {
            suggested_surface_id: suggested_id(kind, &path),
            path,
            surface_kind: kind,
            registered_surface_id: registered.map(|row| row.surface_id.clone()),
        });
    }

    for row in &registry.surfaces {
        if let Some(path) = row.path.as_deref() {
            if !root.join(path).is_file() {
                findings.push(Finding {
                    code: FindingCode::StaleRegistryPath,
                    surface_id: Some(row.surface_id.clone()),
                    path: Some(path.to_string()),
                    detail: "registry row points to a path that is not a regular file".to_string(),
                });
            }
        }
        if row.consumer.trim().is_empty() {
            findings.push(row_finding(
                FindingCode::MissingConsumer,
                row,
                "registry row does not name a consumer",
            ));
        }
        if row.authority_refs.is_empty()
            && !matches!(
                row.disposition,
                Disposition::HistoricalFixture | Disposition::Retired
            )
        {
            findings.push(row_finding(
                FindingCode::MissingAuthority,
                row,
                "current or unresolved row does not name a structured authority",
            ));
        }
    }
    findings.extend(duplicate_roles(registry));
    discovered_surfaces.sort();
    findings.sort();

    Ok(Report {
        schema_version: REPORT_SCHEMA.to_string(),
        registry_schema_version: registry.schema_version.clone(),
        registry_path,
        registered_surface_count: registry.surfaces.len(),
        discovered_surface_count: discovered_surfaces.len(),
        disposition_counts: count_rows(&registry.surfaces, |row| enum_key(row.disposition)),
        surface_kind_counts: count_rows(&registry.surfaces, |row| enum_key(row.surface_kind)),
        discovered_surfaces,
        findings,
    })
}

fn row_finding(code: FindingCode, row: &SurfaceRow, detail: &str) -> Finding {
    Finding {
        code,
        surface_id: Some(row.surface_id.clone()),
        path: row.path.clone(),
        detail: detail.to_string(),
    }
}

fn duplicate_roles(registry: &Registry) -> Vec<Finding> {
    let mut roles: BTreeMap<(String, ProductUnit), Vec<&SurfaceRow>> = BTreeMap::new();
    for row in &registry.surfaces {
        if matches!(
            row.disposition,
            Disposition::CanonicalGenerated | Disposition::ActiveOwnedChannelSource
        ) {
            roles
                .entry((row.target_channel.clone(), row.installed_product_unit))
                .or_default()
                .push(row);
        }
    }
    let mut findings = Vec::new();
    for ((channel, unit), mut rows) in roles {
        if rows.len() < 2 {
            continue;
        }
        rows.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
        findings.push(Finding {
            code: FindingCode::DuplicateChannelProductRole,
            surface_id: None,
            path: None,
            detail: format!(
                "active rows claim channel {channel:?} and product unit {unit:?}: {}",
                rows.iter()
                    .map(|row| row.surface_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    findings
}

fn count_rows<F>(rows: &[SurfaceRow], key: F) -> BTreeMap<String, usize>
where
    F: Fn(&SurfaceRow) -> String,
{
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(key(row)).or_insert(0) += 1;
    }
    counts
}

fn enum_key<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn discover(root: &Path) -> Result<BTreeMap<String, SurfaceKind>> {
    let mut discovered = BTreeMap::new();
    for path in tracked_paths(root)? {
        let absolute = root.join(&path);
        let metadata = fs::metadata(&absolute)
            .wrap_err_with(|| format!("stat tracked path {path}"))?;
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_DISCOVERY_FILE_BYTES {
            if let Some(kind) = classify_path(&path) {
                discovered.insert(path, kind);
            }
            continue;
        }
        let content = fs::read_to_string(&absolute).unwrap_or_default();
        if let Some(kind) = classify_path(&path).or_else(|| classify_content(&path, &content)) {
            discovered.insert(path, kind);
        }
    }
    Ok(discovered)
}

fn tracked_paths(root: &Path) -> Result<Vec<String>> {
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
    {
        if output.status.success() {
            let mut paths = output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|bytes| !bytes.is_empty())
                .map(|bytes| {
                    String::from_utf8(bytes.to_vec())
                        .map_err(|error| eyre!("git ls-files emitted non-UTF-8 path: {error}"))
                })
                .collect::<Result<Vec<_>>>()?;
            paths.sort();
            paths.dedup();
            return Ok(paths);
        }
    }

    let mut paths = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
    {
        let entry = entry.wrap_err("walk repository for install surfaces")?;
        if entry.file_type().is_file() {
            paths.push(normalize(
                entry
                    .path()
                    .strip_prefix(root)
                    .wrap_err("strip repository root from discovered path")?,
            )?);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn should_descend(entry: &DirEntry) -> bool {
    entry.depth() == 0
        || !matches!(
            entry.file_name().to_str(),
            Some(".git" | "target" | "node_modules" | ".worktrees")
        )
}

fn normalize(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(
                value
                    .to_str()
                    .ok_or_else(|| eyre!("path is not valid UTF-8: {}", path.display()))?,
            ),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("path is not repository-relative: {}", path.display())
            }
        }
    }
    Ok(parts.join("/"))
}

fn classify_path(path: &str) -> Option<SurfaceKind> {
    let lower = path.to_ascii_lowercase();
    let name = Path::new(path)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(lower.as_str(), "install.sh" | "install.ps1" | "scripts/install.sh") {
        return Some(SurfaceKind::StandaloneInstaller);
    }
    if lower.starts_with("distribution/") || lower.starts_with("formula/") {
        return Some(SurfaceKind::PackageSource);
    }
    if lower.starts_with("xtask/src/tasks/install_surface_") {
        return Some(SurfaceKind::Validator);
    }
    if lower.starts_with(".github/workflows/") && any(&name, WORKFLOW_TOKENS) {
        return Some(if any(&name, PUBLISH_TOKENS) {
            SurfaceKind::PublisherWorkflow
        } else {
            SurfaceKind::ReleaseWorkflow
        });
    }
    if lower.starts_with(".github/actions/") && any(&lower, ACTION_TOKENS) {
        return Some(SurfaceKind::SetupAction);
    }
    if lower.starts_with("scripts/") && any(&name, SCRIPT_TOKENS) {
        return Some(if any(&name, GENERATOR_TOKENS) {
            SurfaceKind::Generator
        } else if any(&name, VALIDATOR_TOKENS) {
            SurfaceKind::Validator
        } else {
            SurfaceKind::PackageSource
        });
    }
    if lower.starts_with("vscode-extension/") && any(&name, MANAGED_TOKENS) {
        return Some(SurfaceKind::ManagedDownloader);
    }
    None
}

fn classify_content(path: &str, content: &str) -> Option<SurfaceKind> {
    let path = path.to_ascii_lowercase();
    let content = content.to_ascii_lowercase();
    if path.ends_with("cargo.toml") && content.contains("[package.metadata.binstall]") {
        return Some(SurfaceKind::PackageMetadata);
    }
    if path.ends_with(".md")
        && (path.starts_with("docs/")
            || path.starts_with("book/")
            || path.ends_with("readme.md"))
        && INSTALL_COMMANDS.iter().any(|needle| content.contains(needle))
    {
        return Some(if path.starts_with("docs/specs/") {
            SurfaceKind::Specification
        } else {
            SurfaceKind::Documentation
        });
    }
    if EXECUTABLE_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
        && (content.contains("perllsp") || content.contains("perl-lsp"))
        && INSTALL_WORDS.iter().any(|needle| content.contains(needle))
    {
        return Some(SurfaceKind::Other);
    }
    None
}

fn any(value: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| value.contains(token))
}

fn suggested_id(kind: SurfaceKind, path: &str) -> String {
    format!("{}.{}", enum_key(kind), path)
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                character
            } else {
                '.'
            }
        })
        .collect::<String>()
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

fn write_report(path: &Path, report: &Report) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("receipt path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("create receipt directory {}", parent.display()))?;
    fs::write(
        path,
        serde_json::to_vec_pretty(report).wrap_err("serialize install surface inventory")?,
    )
    .wrap_err_with(|| format!("write install surface inventory {}", path.display()))
}

const WORKFLOW_TOKENS: &[&str] = &[
    "artifact", "build-vsix", "checksum", "container", "docker", "extension", "formula",
    "install", "package", "post-publish", "publish", "release", "sbom", "signed", "version",
];
const PUBLISH_TOKENS: &[&str] = &["publish", "release", "signed", "container", "docker"];
const ACTION_TOKENS: &[&str] = &["install", "package", "release", "setup", "publish"];
const SCRIPT_TOKENS: &[&str] = &[
    "artifact", "binstall", "checksum", "container", "crates-io", "homebrew", "install",
    "package", "provenance", "publish", "release", "sbom", "scoop", "topology", "winget",
];
const GENERATOR_TOKENS: &[&str] = &["generate", "render", "prepare", "prep", "inject"];
const VALIDATOR_TOKENS: &[&str] = &["check", "validate", "verify", "smoke", "audit"];
const MANAGED_TOKENS: &[&str] = &["binary", "download", "install", "managed", "package", "release"];
const EXECUTABLE_EXTENSIONS: &[&str] = &[".sh", ".ps1", ".py", ".js", ".ts", ".mjs", ".cjs", ".rb"];
const INSTALL_WORDS: &[&str] = &[
    " install", "download", "release", "publish", "package", "artifact", "homebrew", "scoop",
    "chocolatey", "winget",
];
const INSTALL_COMMANDS: &[&str] = &[
    "cargo install perllsp", "cargo binstall perllsp", "scripts/install.sh", "install.ps1",
    "brew install", "scoop install", "choco install", "winget install", "setup-perl-lsp",
    "marketplace.visualstudio.com", "open-vsx.org",
];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, path: &str, content: &str) -> Result<()> {
        let destination = root.join(path);
        let parent = destination
            .parent()
            .ok_or_else(|| eyre!("test destination has no parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(destination, content)?;
        Ok(())
    }

    fn row(id: &str, path: &str) -> String {
        format!(
            "[[surfaces]]\nsurface_id = \"{id}\"\npath = \"{path}\"\nsurface_kind = \"standalone_installer\"\nproducer = \"repository\"\nconsumer = \"public_user\"\ntarget_channel = \"github_release\"\ninstalled_product_unit = \"server_dap_pair\"\nauthority_refs = [\"issue-6067\"]\ntopology_relationship = \"checked\"\npublication_stage = \"source\"\nactive_user_reachability = \"active\"\ndisposition = \"needs_disposition\"\nowner = \"issue-9104\"\nexit_condition = \"Classify after complete inventory.\"\n"
        )
    }

    fn registry(rows: &str) -> Result<Registry> {
        let source = format!("schema_version = \"{REGISTRY_SCHEMA}\"\n\n{rows}");
        let registry: Registry = toml::from_str(&source)?;
        validate_registry(&registry)?;
        Ok(registry)
    }

    #[test]
    fn discovers_script_outside_historical_roots() -> Result<()> {
        let temp = TempDir::new()?;
        write(temp.path(), "install.sh", "#!/bin/sh\n")?;
        write(
            temp.path(),
            "tools/custom/bootstrap-release.sh",
            "#!/bin/sh\ncurl release.tar.gz\ninstall perllsp\n",
        )?;
        let report = evaluate(
            temp.path(),
            &registry(&row("bootstrap.root", "install.sh"))?,
            "policy/registry.toml".to_string(),
        )?;
        assert!(report.findings.iter().any(|finding| {
            finding.code == FindingCode::UnregisteredSurface
                && finding.path.as_deref() == Some("tools/custom/bootstrap-release.sh")
        }));
        Ok(())
    }

    #[test]
    fn reports_stale_registry_path() -> Result<()> {
        let temp = TempDir::new()?;
        let report = evaluate(
            temp.path(),
            &registry(&row("bootstrap.missing", "missing-install.sh"))?,
            "policy/registry.toml".to_string(),
        )?;
        assert!(report.findings.iter().any(|finding| {
            finding.code == FindingCode::StaleRegistryPath
                && finding.surface_id.as_deref() == Some("bootstrap.missing")
        }));
        Ok(())
    }

    #[test]
    fn discovers_documented_install_command() -> Result<()> {
        let temp = TempDir::new()?;
        write(temp.path(), "install.sh", "#!/bin/sh\n")?;
        write(
            temp.path(),
            "docs/how-to/install.md",
            "Run `cargo binstall perllsp`.",
        )?;
        let report = evaluate(
            temp.path(),
            &registry(&row("bootstrap.root", "install.sh"))?,
            "policy/registry.toml".to_string(),
        )?;
        assert!(report
            .discovered_surfaces
            .iter()
            .any(|surface| surface.path == "docs/how-to/install.md"));
        Ok(())
    }

    #[test]
    fn identical_inputs_are_deterministic() -> Result<()> {
        let temp = TempDir::new()?;
        write(temp.path(), "install.sh", "#!/bin/sh\n")?;
        let registry = registry(&row("bootstrap.root", "install.sh"))?;
        assert_eq!(
            evaluate(temp.path(), &registry, "registry.toml".to_string())?,
            evaluate(temp.path(), &registry, "registry.toml".to_string())?
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_ids() -> Result<()> {
        let source = format!(
            "schema_version = \"{REGISTRY_SCHEMA}\"\n\n{}\n{}",
            row("duplicate.id", "install.sh"),
            row("duplicate.id", "install.ps1")
        );
        let registry: Registry = toml::from_str(&source)?;
        let error = match validate_registry(&registry) {
            Ok(()) => return Err(eyre!("duplicate IDs unexpectedly passed validation")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("duplicate install surface id"));
        Ok(())
    }
}
