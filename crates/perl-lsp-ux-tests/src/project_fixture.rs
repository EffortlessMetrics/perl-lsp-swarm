//! Shared helpers for source-backed editor UX fixture projects.

use crate::{ScenarioConfig, UxHarness};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REAL_PROJECTS_DIR: &str = "test_corpus/real_projects";
const MOJOLICIOUS_SKELETON: &str = "mojolicious_skeleton";
const DANCER2_SKELETON: &str = "dancer2_skeleton";
const CATALYST_SKELETON: &str = "catalyst_skeleton";

/// Source file loaded from a committed real-project UX fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProjectFixtureFile {
    /// Fixture-relative path using `/` separators.
    pub relative_path: String,
    /// UTF-8 source text for the fixture file.
    pub content: String,
}

impl ProjectFixtureFile {
    /// Construct one fixture file from a relative path and content.
    pub fn new(relative_path: impl Into<String>, content: impl Into<String>) -> Self {
        Self { relative_path: relative_path.into(), content: content.into() }
    }
}

/// Resolve the repository workspace root for UX fixtures at runtime.
///
/// Prefers the runtime `CARGO_MANIFEST_DIR` — cargo sets it for the test
/// process, so test binaries stay relocatable across worktrees that share one
/// target directory; a library compiled in a since-deleted worktree must not
/// redirect corpus reads at a dead path (#17176). Falls back to the
/// compile-time path when the test binary runs outside cargo.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    workspace_root_from(&manifest_dir)
}

/// Pure workspace-root walk behind [`workspace_root`], factored out so the
/// ancestor rules can be unit tested without mutating process-global
/// environment state (this crate denies `unsafe_code`, which
/// `std::env::set_var` requires in edition 2024).
fn workspace_root_from(manifest_dir: &Path) -> Result<PathBuf> {
    manifest_dir
        .ancestors()
        .find(|candidate| candidate.join("Cargo.lock").is_file())
        .map(Path::to_path_buf)
        .context("no ancestor of CARGO_MANIFEST_DIR holds a Cargo.lock workspace root")
}

/// Load all Perl source files under the Mojolicious skeleton UX fixture.
pub fn load_mojolicious_fixture_files() -> Result<Vec<ProjectFixtureFile>> {
    load_real_project_fixture_files(MOJOLICIOUS_SKELETON)
}

/// Load all Perl source files under the Dancer2 sample UX fixture.
pub fn load_dancer2_fixture_files() -> Result<Vec<ProjectFixtureFile>> {
    load_real_project_fixture_files(DANCER2_SKELETON)
}

/// Load all Perl source files under the Catalyst sample UX fixture.
pub fn load_catalyst_fixture_files() -> Result<Vec<ProjectFixtureFile>> {
    load_real_project_fixture_files(CATALYST_SKELETON)
}

/// Build a workspace-enabled scenario config seeded with fixture files.
pub fn fixture_scenario_config(files: &[ProjectFixtureFile]) -> ScenarioConfig {
    files.iter().fold(
        ScenarioConfig { timeout: Duration::from_secs(20), ..Default::default() }
            .env("PERL_LSP_WORKSPACE", "1"),
        |config, file| config.with_file(&file.relative_path, &file.content),
    )
}

/// Create a UX harness seeded with fixture files and workspace indexing enabled.
pub fn create_fixture_harness(files: &[ProjectFixtureFile]) -> Result<UxHarness> {
    UxHarness::new(fixture_scenario_config(files))
}

/// Open every fixture file in a harness.
pub fn open_all_fixture_files(harness: &UxHarness, files: &[ProjectFixtureFile]) -> Result<()> {
    for file in files {
        harness.open_file(&file.relative_path, &file.content)?;
    }
    Ok(())
}

/// Find fixture content by fixture-relative path.
pub fn fixture_content<'a>(
    files: &'a [ProjectFixtureFile],
    relative_path: &str,
) -> Result<&'a str> {
    files
        .iter()
        .find(|file| file.relative_path == relative_path)
        .map(|file| file.content.as_str())
        .with_context(|| format!("missing fixture file {relative_path}"))
}

fn load_real_project_fixture_files(fixture_name: &str) -> Result<Vec<ProjectFixtureFile>> {
    let root = workspace_root()?.join(REAL_PROJECTS_DIR).join(fixture_name);
    let mut files = Vec::new();
    collect_perl_files(&root, &root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn collect_perl_files(root: &Path, dir: &Path, files: &mut Vec<ProjectFixtureFile>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("reading an entry under {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_perl_files(root, &path, files)?;
        } else if is_perl_source(&path) {
            let relative_path = path
                .strip_prefix(root)
                .with_context(|| format!("stripping fixture root from {}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            files.push(ProjectFixtureFile { relative_path, content });
        }
    }
    Ok(())
}

fn is_perl_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "pm" | "pl" | "t"))
}

#[cfg(test)]
mod tests {
    use super::{workspace_root, workspace_root_from};
    use anyhow::{Context, Result};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn workspace_root_walks_to_nearest_cargo_lock_ancestor() -> Result<()> {
        let temp = TempDir::new().context("failed to create fixture workspace")?;
        let workspace = temp.path().join("workspace");
        let crate_dir = workspace.join("crates").join("ux-fixture");
        fs::create_dir_all(&crate_dir).context("failed to create nested crate dirs")?;
        fs::write(workspace.join("Cargo.lock"), "version = 9")
            .context("failed to write workspace Cargo.lock")?;

        let resolved = workspace_root_from(&crate_dir)
            .context("workspace-root walk failed for nested crate")?;
        assert_eq!(resolved, workspace, "walk must stop at the Cargo.lock ancestor");
        Ok(())
    }

    #[test]
    fn workspace_root_walk_fails_without_cargo_lock() -> Result<()> {
        let temp = TempDir::new().context("failed to create lockless tree")?;
        let nested = temp.path().join("crates").join("ux-fixture");
        fs::create_dir_all(&nested).context("failed to create nested crate dirs")?;

        let error = workspace_root_from(&nested)
            .err()
            .context("walk must fail when no ancestor holds a Cargo.lock")?;
        assert!(
            error.to_string().contains("Cargo.lock"),
            "failure must name the missing Cargo.lock workspace root: {error}"
        );
        Ok(())
    }

    /// Runtime contract for the shipped layout: the resolved root must be the
    /// running workspace, not a path baked in by whatever worktree last
    /// compiled this library into a shared target directory (#17176).
    #[test]
    fn workspace_root_resolves_the_running_workspace_at_runtime() -> Result<()> {
        let root = workspace_root().context("runtime workspace-root resolution failed")?;
        assert!(
            root.join("Cargo.lock").is_file(),
            "resolved workspace root lacks Cargo.lock: {}",
            root.display()
        );
        assert!(
            root.join("test_corpus").join("real_projects").is_dir(),
            "resolved workspace root lacks the real-project corpus: {}",
            root.display()
        );
        Ok(())
    }
}
