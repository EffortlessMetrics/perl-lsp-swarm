//! Exact-tree test panic-family debt denominator (#13397).
//!
//! Generated observation over current source, the panic-identity registry, and
//! source declarations. Not a second allowlist or lint policy.

mod check;
mod discover;
mod join;
mod model;
mod projection;
mod topology;
mod vocabulary;

pub use check::{CheckRequest, CheckResult, check_inventory};
pub use model::{
    ClippyObservation, ClippyTargetObservation, ClippyTargetStatus, DebtRow, DebtStatus,
    Entrypoint, Instrument, InstrumentStatus, Inventory, InventoryRequest, OwnerState, SCHEMA,
    TargetKind,
};
pub use projection::{canonical_json, render_human, semantic_delta};

use color_eyre::eyre::{Result, WrapErr, eyre};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Default machine artifact path relative to the repository root.
pub const DEFAULT_JSON_PATH: &str = "target/policy/test_panic_family_debt.v1.json";
/// Default human projection path relative to the repository root.
pub const DEFAULT_MARKDOWN_PATH: &str = "target/policy/test_panic_family_debt.v1.md";

/// Build the exact-tree denominator for `root`.
pub fn build_inventory(request: InventoryRequest<'_>) -> Result<Inventory> {
    let vocabulary =
        vocabulary::load(request.root, request.lint_ledger_path, request.lint_catalog_dir)?;
    let topology = topology::discover(request.root, &vocabulary)?;
    let discovered = discover::scan(request.root, &topology, &vocabulary)?;
    join::join(request, topology, discovered)
}

/// CLI: generate machine and human projections. Returns the stdout summary.
pub fn run_inventory(
    root: &Path,
    json_path: Option<PathBuf>,
    markdown_path: Option<PathBuf>,
) -> Result<String> {
    let inventory = build_inventory(InventoryRequest { root, ..InventoryRequest::default() })?;
    let json_path = json_path.unwrap_or_else(|| root.join(DEFAULT_JSON_PATH));
    let markdown_path = markdown_path.unwrap_or_else(|| root.join(DEFAULT_MARKDOWN_PATH));
    write_artifact(&json_path, &canonical_json(&inventory)?)?;
    write_artifact(&markdown_path, render_human(&inventory).as_bytes())?;
    Ok(format!(
        "test_panic_family_debt.v1: files={} entrypoints={} rows={} instruments_not_proven={} json={} markdown={}",
        inventory.population.files.len(),
        inventory.population.entrypoints.len(),
        inventory.rows.len(),
        inventory
            .instruments
            .iter()
            .filter(|instrument| instrument.status == InstrumentStatus::NotProven)
            .count(),
        json_path.display(),
        markdown_path.display()
    ))
}

/// CLI: re-derive the denominator and fail on integrity or identity drift.
pub fn run_check(
    root: &Path,
    artifact: Option<PathBuf>,
    baseline: Option<PathBuf>,
    clippy_observation: Option<PathBuf>,
    owner_state: Option<PathBuf>,
) -> Result<CheckResult> {
    let clippy = clippy_observation.as_deref().map(load_clippy_observation).transpose()?;
    let owners = owner_state.as_deref().map(load_owner_state).transpose()?;
    let current = build_inventory(InventoryRequest {
        root,
        clippy_observation: clippy.as_ref(),
        owner_state: owners.as_ref(),
        ..InventoryRequest::default()
    })?;
    check_inventory(CheckRequest {
        root,
        current: &current,
        artifact: artifact.as_deref(),
        baseline: baseline.as_deref(),
    })
}

/// Format check findings for stdout.
pub fn format_check_result(result: &CheckResult) -> String {
    let mut out = format!(
        "test_panic_family_debt.v1 check: ok={} findings={}",
        result.ok,
        result.findings.len()
    );
    for finding in &result.findings {
        out.push_str("\n- ");
        out.push_str(finding);
    }
    out
}

/// CLI: render the human projection for the current tree.
pub fn run_report(root: &Path, json_path: Option<PathBuf>) -> Result<String> {
    let inventory = build_inventory(InventoryRequest { root, ..InventoryRequest::default() })?;
    if let Some(path) = json_path {
        write_artifact(&path, &canonical_json(&inventory)?)?;
    }
    Ok(render_human(&inventory))
}

fn load_clippy_observation(path: &Path) -> Result<ClippyObservation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading clippy observation {}", path.display()))?;
    serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing clippy observation {}", path.display()))
}

fn load_owner_state(path: &Path) -> Result<OwnerState> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading owner-state {}", path.display()))?;
    serde_json::from_str(&raw).wrap_err_with(|| format!("parsing owner-state {}", path.display()))
}

fn write_artifact(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).wrap_err_with(|| format!("creating {}", parent.display()))?;
    }
    let mut file =
        fs::File::create(path).wrap_err_with(|| format!("creating {}", path.display()))?;
    file.write_all(bytes).wrap_err_with(|| format!("writing {}", path.display()))?;
    Ok(())
}

impl Default for InventoryRequest<'_> {
    fn default() -> Self {
        Self {
            root: Path::new("."),
            registry_path: None,
            lint_ledger_path: None,
            lint_catalog_dir: None,
            clippy_observation: None,
            owner_state: None,
            repository_commit: None,
        }
    }
}

impl InventoryRequest<'_> {
    pub(crate) fn registry_path(&self) -> PathBuf {
        self.registry_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.join("ci/panic_test_identities.json"))
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Collapse `.` / `..` without touching the filesystem.
///
/// Returns `None` when a `..` would escape past the path's own prefix or root.
pub(crate) fn lexically_normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => return None,
            },
            component => out.push(component.as_os_str()),
        }
    }
    Some(out)
}

/// Repository-relative slash path. `..` components are collapsed so
/// `#[path = "../src/foo.rs"]` joins the same identity as Cargo's `src/foo.rs`.
///
/// Paths that escape `root` after collapse are an error; callers must not mint a
/// second `../` identity.
pub(crate) fn repo_relative_path(path: &Path, root: &Path) -> Result<String, String> {
    let normalized = lexically_normalize(path)
        .ok_or_else(|| format!("{} escapes its prefix via ..", path.display()))?;
    let stripped = normalized
        .strip_prefix(root)
        .map_err(|_| format!("{} is not under {}", normalized.display(), root.display()))?;
    if stripped.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err(format!("{} retains .. after lexical collapse", path.display()));
    }
    Ok(stripped.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn normalize_path(path: &Path, root: &Path) -> String {
    repo_relative_path(path, root).unwrap_or_else(|_| {
        path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    })
}

pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|err| eyre!("reading {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_artifact_paths_and_inventory_request_are_named() {
        let request = InventoryRequest { root: Path::new("."), ..InventoryRequest::default() };
        assert_eq!(DEFAULT_JSON_PATH, "target/policy/test_panic_family_debt.v1.json");
        assert_eq!(DEFAULT_MARKDOWN_PATH, "target/policy/test_panic_family_debt.v1.md");
        assert!(request.registry_path().ends_with("ci/panic_test_identities.json"));
        assert_eq!(normalize_path(Path::new("/tmp/a/b.rs"), Path::new("/tmp/a")), "b.rs");
        assert_eq!(
            normalize_path(Path::new("/tmp/a/tests/../src/foo.rs"), Path::new("/tmp/a")),
            "src/foo.rs"
        );
        assert!(repo_relative_path(Path::new("/tmp/outside.rs"), Path::new("/tmp/a")).is_err());
        assert!(lexically_normalize(Path::new("../outside.rs")).is_none());
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }
}
