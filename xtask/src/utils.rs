//! Utility functions for xtask

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use color_eyre::eyre::{Context, Result, bail, eyre};

static WORKTREE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Get the explicitly selected runtime worktree, or the compiled source root.
pub fn project_root() -> Result<PathBuf> {
    if let Some(root) = WORKTREE_ROOT.get() {
        return Ok(root.clone());
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // xtask is in xtask/, so go up one level to get project root
    manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| eyre!("xtask should be in a subdirectory - invalid project structure"))
}

/// Select a runtime worktree before dispatch, aligning relative child commands.
/// Invalid explicit targets never fall back to the compiled source root.
pub fn select_worktree_root(path: &Path) -> Result<()> {
    if WORKTREE_ROOT.get().is_some() {
        bail!("runtime worktree has already been selected");
    }
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        if std::env::var_os(name).is_some() {
            bail!("--worktree-root requires {name} to be unset; refusing an ambiguous repository");
        }
    }
    let requested = path
        .canonicalize()
        .with_context(|| format!("resolving --worktree-root {}", path.display()))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&requested)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("resolving --worktree-root with git rev-parse")?;
    if !output.status.success() {
        bail!("invalid --worktree-root: {}", String::from_utf8_lossy(&output.stderr));
    }
    let text = String::from_utf8(output.stdout).context("--worktree-root Git path is not UTF-8")?;
    // Remove only Git's terminator; spaces may be part of a valid path.
    let root = PathBuf::from(text.strip_suffix('\n').unwrap_or(&text))
        .canonicalize()
        .context("canonicalizing --worktree-root Git root")?;
    if !requested.starts_with(&root) {
        bail!("--worktree-root Git root does not contain the requested directory");
    }
    let manifest: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join("Cargo.toml"))
            .context("--worktree-root requires a workspace Cargo.toml")?,
    )
    .context("parsing --worktree-root workspace Cargo.toml")?;
    let repository = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("repository"))
        .and_then(toml::Value::as_str);
    if repository != Some(env!("CARGO_PKG_REPOSITORY")) {
        bail!("--worktree-root requires the {} workspace", env!("CARGO_PKG_REPOSITORY"));
    }
    let xtask: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join("xtask/Cargo.toml"))
            .context("--worktree-root requires xtask/Cargo.toml")?,
    )
    .context("parsing --worktree-root xtask/Cargo.toml")?;
    if xtask.get("package").and_then(|package| package.get("name")).and_then(toml::Value::as_str)
        != Some("xtask")
    {
        bail!("--worktree-root requires the xtask package");
    }
    std::env::set_current_dir(&root)
        .with_context(|| format!("entering --worktree-root {}", root.display()))?;
    WORKTREE_ROOT.set(root).map_err(|_| eyre!("runtime worktree has already been selected"))
}

/// Run `cargo metadata --format-version 1` and return the raw JSON bytes.
///
/// Set `no_deps = true` to pass `--no-deps` (skips dependency resolution and
/// omits the `resolve` graph from the output — faster, but the caller will not
/// receive transitive-dep information).
///
/// Returns an error if `cargo metadata` exits non-zero.
pub fn run_cargo_metadata(no_deps: bool) -> Result<Vec<u8>> {
    run_cargo_metadata_with(None, no_deps)
}

/// Run `cargo metadata` against an explicit manifest, for fixture roots.
pub fn run_cargo_metadata_at(manifest_path: &Path, no_deps: bool) -> Result<Vec<u8>> {
    run_cargo_metadata_with(Some(manifest_path), no_deps)
}

fn run_cargo_metadata_with(manifest_path: Option<&Path>, no_deps: bool) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version", "1"]);
    if no_deps {
        cmd.arg("--no-deps");
    }
    if let Some(path) = manifest_path {
        cmd.arg("--manifest-path").arg(path);
    }
    // Without this context a missing `cargo` surfaces as a bare "program not
    // found" pointing at this line — naming neither the program nor the
    // operation (#17168). Agents and release harnesses invoke the built
    // binary directly, where cargo is not guaranteed to be on PATH.
    let output = cmd.output().with_context(|| "spawning `cargo metadata` (is `cargo` on PATH?)")?;
    if !output.status.success() {
        bail!("cargo metadata failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_cargo_metadata_at_reads_an_explicit_manifest() -> Result<()> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let raw = run_cargo_metadata_at(&manifest, true)?;
        assert!(raw.starts_with(b"{"), "cargo metadata must return JSON");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Publish allowlist helpers (shared by publish_closure, count_ratchet,
// and publish_manifest_check)
// ---------------------------------------------------------------------------

/// Workspace-level metadata shape for publish allowlist access.
///
/// Covers the `[workspace.metadata]` table only — not the full cargo metadata
/// structure.  Used by `load_publish_allowlist()` and also re-exported so
/// `publish_closure` can layer its own `FullMetadata` on top without duplicating
/// the allowlist structs.
#[derive(serde::Deserialize)]
pub struct AllowlistMetadata {
    #[serde(rename = "metadata")]
    pub workspace_metadata: Option<WorkspacePublishMeta>,
}

/// `[workspace.metadata.publish]` section.
#[derive(serde::Deserialize)]
pub struct WorkspacePublishMeta {
    pub publish: Option<AllowList>,
}

/// `[workspace.metadata.publish.allow]` list.
#[derive(serde::Deserialize)]
pub struct AllowList {
    pub allow: Option<Vec<String>>,
}

/// Load `[workspace.metadata.publish.allow]` via `cargo metadata --no-deps`.
///
/// Returns the list of allowlisted crate names.
/// Errors if the section is absent or the list is empty.
pub fn load_publish_allowlist() -> color_eyre::eyre::Result<Vec<String>> {
    use color_eyre::eyre::{bail, eyre};
    let bytes = run_cargo_metadata(true)?;
    let meta: AllowlistMetadata =
        serde_json::from_slice(&bytes).map_err(|e| eyre!("Failed to parse cargo metadata: {e}"))?;
    let allow = meta
        .workspace_metadata
        .and_then(|m| m.publish)
        .and_then(|p| p.allow)
        .ok_or_else(|| eyre!("No [workspace.metadata.publish.allow] found in Cargo.toml"))?;
    if allow.is_empty() {
        bail!("Publish allowlist is empty");
    }
    Ok(allow)
}

/// Build constrained environment variables for resource-limited CI.
/// Merges with existing RUSTFLAGS instead of overwriting.
pub fn constrained_env_vars() -> Vec<(&'static str, String)> {
    let extra_flags = "-Copt-level=2 -Ccodegen-units=1 -Cdebuginfo=0";

    // Merge with existing RUSTFLAGS
    let rustflags = match std::env::var("RUSTFLAGS") {
        Ok(prev) if !prev.trim().is_empty() => format!("{} {}", prev, extra_flags),
        _ => extra_flags.to_string(),
    };

    vec![
        ("RUSTFLAGS", rustflags),
        ("CARGO_BUILD_JOBS", "2".to_string()),
        ("RUST_TEST_THREADS", "1".to_string()),
        ("RUST_BACKTRACE", "full".to_string()),
        ("CARGO_INCREMENTAL", "0".to_string()), // Smoother memory profile
        ("CARGO_TERM_COLOR", "always".to_string()), // Nicer CI logs
    ]
}
