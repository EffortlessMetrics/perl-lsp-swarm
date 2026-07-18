//! Keep release narrative docs synchronized from one source of truth.
//!
//! The workspace version is read from `Cargo.toml` and the published crate
//! surface count is derived from `[workspace.metadata.publish.allow]`.
//! This keeps active narrative docs aligned with the true release surface without
//! hand-editing version/count literals.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

/// Values used to hydrate release narrative files.
#[derive(Debug)]
struct ReleaseSurface {
    version: String,
    published_crate_count: usize,
    shipped_date: String,
    prior_version: String,
    prior_shipped_date: String,
    next_version: String,
}

/// Synchronize active release docs from workspace-derived values.
pub fn run(write: bool) -> Result<()> {
    let root = project_root()?;
    let surface = collect_release_surface(&root)?;

    let mut changed_files: Vec<(PathBuf, String)> = Vec::new();

    apply_sync(
        &root.join("README.md"),
        |content| sync_readme(content, &surface),
        &mut changed_files,
    )?;
    apply_sync(
        &root.join("docs/project/CURRENT_STATUS.md"),
        |content| sync_current_status(content, &surface),
        &mut changed_files,
    )?;
    apply_sync(
        &root.join("docs/project/ROADMAP.md"),
        |content| sync_roadmap(content, &surface),
        &mut changed_files,
    )?;
    apply_sync(
        &root.join("docs/project/status/index.md"),
        |content| sync_status_index(content, &surface),
        &mut changed_files,
    )?;
    apply_sync(
        &root.join("docs/project/status/release.md"),
        |content| sync_release_notes(content, &surface),
        &mut changed_files,
    )?;

    if changed_files.is_empty() {
        println!("Release docs are in sync.");
        return Ok(());
    }

    if write {
        for (path, content) in &changed_files {
            fs::write(path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            println!("Updated {}", path.display());
        }
        println!("Synced {} file(s) from workspace metadata.", changed_files.len());
        return Ok(());
    }

    for (path, _) in &changed_files {
        eprintln!("{} is out of date", path.display());
    }
    bail!("{} release doc file(s) out of date; rerun with --write", changed_files.len());
}

fn apply_sync<F>(path: &Path, mut sync: F, changed_files: &mut Vec<(PathBuf, String)>) -> Result<()>
where
    F: FnMut(&str) -> Result<String>,
{
    let current =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let updated = sync(&current)?;
    if updated != current {
        changed_files.push((path.to_path_buf(), updated));
    }
    Ok(())
}

fn collect_release_surface(root: &Path) -> Result<ReleaseSurface> {
    let cargo_toml = root.join("Cargo.toml");
    let raw = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;
    let parsed: Value = toml::from_str(&raw).context("parsing Cargo.toml")?;

    let version = parsed
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|pkg| pkg.get("version"))
        .and_then(toml::Value::as_str)
        .map(|v| v.to_string())
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("Cargo.toml is missing [workspace.package].version")
        })?;

    let publish_allow = parsed
        .get("workspace")
        .and_then(|workspace| workspace.get("metadata"))
        .and_then(|metadata| metadata.get("publish"))
        .and_then(|publish| publish.get("allow"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("Cargo.toml is missing [workspace.metadata.publish.allow]")
        })?;

    let published_crate_count = publish_allow.len();
    if published_crate_count == 0 {
        bail!("publish allowlist is empty");
    }

    let (shipped_date, prior_version, prior_shipped_date) = collect_release_dates(root, &version)?;
    let next_version = next_minor_version(&version)?;

    Ok(ReleaseSurface {
        version,
        published_crate_count,
        shipped_date,
        prior_version,
        prior_shipped_date,
        next_version,
    })
}

fn collect_release_dates(root: &Path, version: &str) -> Result<(String, String, String)> {
    let changelog_path = root.join("CHANGELOG.md");
    let changelog = fs::read_to_string(&changelog_path)
        .with_context(|| format!("reading {}", changelog_path.display()))?;
    let mut releases = changelog.lines().filter_map(parse_changelog_release);
    let Some((current_version, shipped_date)) =
        releases.find(|(candidate, _)| *candidate == version)
    else {
        bail!("CHANGELOG.md: release heading for v{version} not found");
    };
    let Some((prior_version, prior_shipped_date)) = releases.next() else {
        bail!("CHANGELOG.md: prior release heading after v{current_version} not found");
    };
    Ok((shipped_date.to_string(), prior_version.to_string(), prior_shipped_date.to_string()))
}

fn parse_changelog_release(line: &str) -> Option<(&str, &str)> {
    let release = line.strip_prefix("## [")?;
    release.split_once("] - ")
}

fn next_minor_version(version: &str) -> Result<String> {
    let mut parts = version.split('.');
    let major = parts.next().ok_or_else(|| color_eyre::eyre::eyre!("invalid version {version}"))?;
    let minor = parts
        .next()
        .ok_or_else(|| color_eyre::eyre::eyre!("invalid version {version}"))?
        .parse::<u64>()
        .with_context(|| format!("invalid minor version in {version}"))?;
    let _patch =
        parts.next().ok_or_else(|| color_eyre::eyre::eyre!("invalid version {version}"))?;
    if parts.next().is_some() {
        bail!("invalid version {version}");
    }
    let next_minor = minor
        .checked_add(1)
        .ok_or_else(|| color_eyre::eyre::eyre!("minor version overflow in {version}"))?;
    Ok(format!("{major}.{next_minor}.0"))
}

fn sync_readme(content: &str, surface: &ReleaseSurface) -> Result<String> {
    let mut release_track_seen = false;
    let mut verified_install_seen = false;

    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("| Release track | `") {
            lines.push(format!("| Release track | `v{}` public beta |", surface.version));
            release_track_seen = true;
        } else if line.starts_with("| Published crate surface | ") {
            lines.push(format!(
                "| Published crate surface | {} crates in `[workspace.metadata.publish.allow]` |",
                surface.published_crate_count
            ));
        } else if line.starts_with("The verified GitHub `v") {
            lines.push(format!(
                "The verified GitHub `v{}` release assets are public beta. Other distribution",
                surface.version
            ));
            verified_install_seen = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !release_track_seen && !verified_install_seen {
        bail!("README.md: release posture line not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn sync_current_status(content: &str, surface: &ReleaseSurface) -> Result<String> {
    let mut version_seen = false;
    let mut train_seen = false;
    let mut count_seen = false;
    let mut milestone_seen = false;

    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("| **Workspace version line** | ") {
            lines.push(format!(
                "| **Workspace version line** | `v{}` | [`Cargo.toml`](../../Cargo.toml) |",
                surface.version
            ));
            version_seen = true;
        } else if line.starts_with("| **Current release train** | ") {
            lines.push(format!(
                "| **Current release train** | `v{}` latest public beta ({}); prior `v{}` ({}) | [CHANGELOG.md](../../CHANGELOG.md) |",
                surface.version,
                surface.shipped_date,
                surface.prior_version,
                surface.prior_shipped_date
            ));
            train_seen = true;
        } else if line.starts_with("| **Published crate surface** | ") {
            lines.push(format!(
                "| **Published crate surface** | {} crates | [`[workspace.metadata.publish.allow]`](../../Cargo.toml) |",
                surface.published_crate_count
            ));
            count_seen = true;
        } else if line.starts_with("| **Active milestone** | `") {
            lines.push(format!(
                "| **Active milestone** | `v{}` shipped public beta; `v{}` next public-beta train | [status/index.md](status/index.md) |",
                surface.version, surface.next_version
            ));
            milestone_seen = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !version_seen {
        bail!("CURRENT_STATUS.md: workspace version row not found");
    }
    if !train_seen {
        bail!("CURRENT_STATUS.md: current release train row not found");
    }
    if !count_seen {
        bail!("CURRENT_STATUS.md: published crate surface row not found");
    }
    if !milestone_seen {
        bail!("CURRENT_STATUS.md: active milestone row not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn sync_roadmap(content: &str, surface: &ReleaseSurface) -> Result<String> {
    let mut workspace_version_seen = false;
    let mut current_release_seen = false;
    let mut published_surface_seen = false;
    let mut publication_discipline_seen = false;
    let mut active_section_seen = false;
    let mut now_section_seen = false;
    let mut now_gate_seen = false;
    let mut next_header_seen = false;

    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("- Workspace version line: `v") {
            lines.push(format!("- Workspace version line: `v{}`", surface.version));
            workspace_version_seen = true;
        } else if line.starts_with("- Current release train: `v") {
            lines.push(format!(
                "- Current release train: `v{}` shipped public beta; channel receipts remain independently verified",
                surface.version
            ));
            current_release_seen = true;
        } else if line.starts_with("- Published crate surface target: ") {
            lines.push(format!(
                "- Published crate surface target: {} crates from `[workspace.metadata.publish.allow]`",
                surface.published_crate_count
            ));
            published_surface_seen = true;
        } else if line.starts_with("Publication discipline: `v") {
            lines.push(format!(
                "Publication discipline: `v{}` uses a normal SemVer package version while the human-facing product posture remains public beta, not stable/GA. See [RELEASE_HISTORY.md](../../RELEASE_HISTORY.md) for independently verified channel receipts.",
                surface.version
            ));
            publication_discipline_seen = true;
        } else if line.starts_with("## Active: Public-Alpha Release Prep (v")
            || line.starts_with("## Active: Public-Beta Release (v")
            || line.starts_with("## Active: Public-Alpha Channel Closeout (v")
        {
            lines.push(format!(
                "## Active: Public-Beta Release (v{})",
                surface.version
            ));
            active_section_seen = true;
        } else if line.starts_with("### Now (v")
            && (line.contains("public-alpha patch prep)")
                || line.contains("shipped public beta)")
                || line.contains("public-alpha channel closeout)"))
        {
            lines.push(format!("### Now (v{} shipped public beta)", surface.version));
            now_section_seen = true;
        } else if line.starts_with("- `v")
            && (line.contains("is staged as the next public-alpha patch release; run the release-prep checks before dispatching the train")
                || line.contains("is shipped public beta; keep each distribution channel pending until its receipt is verified")
                || line.contains("is the current public-alpha release line; finish receipt closeout before treating the release as fully closed"))
        {
            lines.push(format!(
                "- `v{}` is shipped public beta; keep each distribution channel pending until its receipt is verified",
                surface.version
            ));
            now_gate_seen = true;
        } else if line.starts_with("### Next (post v") {
            lines.push(format!("### Next (post v{})", surface.version));
            next_header_seen = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !workspace_version_seen {
        bail!("ROADMAP.md: workspace version line not found");
    }
    if !current_release_seen {
        bail!("ROADMAP.md: current release train line not found");
    }
    if !published_surface_seen {
        bail!("ROADMAP.md: published crate surface target line not found");
    }
    if !publication_discipline_seen {
        bail!("ROADMAP.md: publication discipline line not found");
    }
    if !active_section_seen {
        bail!("ROADMAP.md: active release heading not found");
    }
    if !now_section_seen {
        bail!("ROADMAP.md: current-release Now section not found");
    }
    if !now_gate_seen {
        bail!("ROADMAP.md: release-prep check gate line not found");
    }
    if !next_header_seen {
        bail!("ROADMAP.md: Next (post ...) heading not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn sync_status_index(content: &str, surface: &ReleaseSurface) -> Result<String> {
    let mut release_posture_seen = false;
    let mut now_section_seen = false;
    let mut now_gate_seen = false;
    let mut published_surface_bullet_seen = false;
    let mut next_section_seen = false;

    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("- **Release posture**: `v") {
            lines.push(format!(
                "- **Release posture**: `v{}` is the current workspace version and shipped public-beta release ({}); `v{}` is the next public-beta train, not a maturity promotion or version bump in this tree. The published crate surface is {} crates. See [release.md](release.md) for channel receipts.",
                surface.version,
                surface.shipped_date,
                surface.next_version,
                surface.published_crate_count
            ));
            release_posture_seen = true;
        } else if line.starts_with("**Now (active milestone: v") {
            lines.push(format!(
                "**Now (active milestone: v{} shipped public beta)**",
                surface.version
            ));
            now_section_seen = true;
        } else if line.starts_with("- Run the `v")
            && line.contains(" release-prep checks before dispatching release orchestration")
        {
            lines.push(format!(
                "- Run the `v{}` release-prep checks before dispatching release orchestration",
                surface.version
            ));
            now_gate_seen = true;
        } else if line.contains("published surface")
            && line.starts_with("- Keep the top-level README")
        {
            lines.push(format!(
                "- Keep the top-level README, status docs, and release runbooks aligned with the actual `perllsp` asset line, the `perl-lsp-rs` extension package, and the {}-crate published surface",
                surface.published_crate_count
            ));
            published_surface_bullet_seen = true;
        } else if line.starts_with("**Next (post v") || line.starts_with("**Next (v") {
            lines.push(format!("**Next (v{} public-beta train)**", surface.next_version));
            next_section_seen = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !release_posture_seen {
        bail!("status/index.md: release posture line not found");
    }
    if !now_section_seen {
        bail!("status/index.md: now section heading not found");
    }
    if !now_gate_seen {
        bail!("status/index.md: now release-prep checks line not found");
    }
    if !published_surface_bullet_seen {
        bail!("status/index.md: published-surface alignment line not found");
    }
    if !next_section_seen {
        bail!("status/index.md: next section heading not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn sync_release_notes(content: &str, surface: &ReleaseSurface) -> Result<String> {
    let mut train_seen = false;
    let mut workspace_seen = false;
    let mut surface_seen = false;
    let mut remaining_seen = false;

    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("**Current release train**: `v") {
            lines.push(format!(
                "**Current release train**: `v{}` — shipped {} as public beta",
                surface.version, surface.shipped_date
            ));
            train_seen = true;
        } else if line.starts_with("**Workspace version line**: `v") {
            lines.push(format!("**Workspace version line**: `v{}`", surface.version));
            workspace_seen = true;
        } else if line.starts_with("**Published crate surface**: ") {
            lines.push(format!(
                "**Published crate surface**: {} crates",
                surface.published_crate_count
            ));
            surface_seen = true;
        } else if line.starts_with("- Remaining work is operational: finish `v")
            && line.contains(" prep verification, then publish and record final channel receipts")
        {
            lines.push(format!(
                "- Remaining work is operational: finish `v{}` prep verification, then publish and record final channel receipts",
                surface.version
            ));
            remaining_seen = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !train_seen {
        bail!("status/release.md: current release train line not found");
    }
    if !workspace_seen {
        bail!("status/release.md: workspace version line not found");
    }
    if !surface_seen {
        bail!("status/release.md: published crate surface line not found");
    }
    if !remaining_seen {
        bail!("status/release.md: remaining blockers prep verification line not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn restore_trailing_newline(original: &str, lines: &[String]) -> String {
    let mut updated = lines.join("\n");
    if original.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_surface() -> ReleaseSurface {
        ReleaseSurface {
            version: "0.17.0".to_string(),
            published_crate_count: 32,
            shipped_date: "2026-06-28".to_string(),
            prior_version: "0.16.0".to_string(),
            prior_shipped_date: "2026-06-06".to_string(),
            next_version: "0.18.0".to_string(),
        }
    }

    #[test]
    fn sync_current_status_preserves_release_train_facts_on_first_and_second_write() -> Result<()> {
        let input = "| **Workspace version line** | `v0.16.0` | source |\n\
| **Current release train** | legacy | source |\n\
| **Published crate surface** | 31 crates | source |\n\
| **Active milestone** | `v0.17.0` preparation | source |\n";
        let expected = "| **Workspace version line** | `v0.17.0` | [`Cargo.toml`](../../Cargo.toml) |\n\
| **Current release train** | `v0.17.0` latest public beta (2026-06-28); prior `v0.16.0` (2026-06-06) | [CHANGELOG.md](../../CHANGELOG.md) |\n\
| **Published crate surface** | 32 crates | [`[workspace.metadata.publish.allow]`](../../Cargo.toml) |\n\
| **Active milestone** | `v0.17.0` shipped public beta; `v0.18.0` next public-beta train | [status/index.md](status/index.md) |\n";

        let first = sync_current_status(input, &release_surface())?;
        if first != expected {
            bail!("first current-status sync did not retain exact release train facts");
        }
        let second = sync_current_status(&first, &release_surface())?;
        if second != first {
            bail!("second current-status sync was not idempotent");
        }
        Ok(())
    }

    #[test]
    fn sync_readme_preserves_verified_install_posture_on_first_and_second_write() -> Result<()> {
        let input = "The verified GitHub `v0.16.0` release assets are public alpha. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";
        let expected = "The verified GitHub `v0.17.0` release assets are public beta. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";

        let first = sync_readme(input, &release_surface())?;
        if first != expected {
            bail!("first README sync did not retain verified install posture");
        }
        let second = sync_readme(&first, &release_surface())?;
        if second != first {
            bail!("second README sync was not idempotent");
        }
        Ok(())
    }

    #[test]
    fn sync_release_notes_preserves_shipped_date_on_first_and_second_write() -> Result<()> {
        let input = "**Current release train**: `v0.16.0` — release preparation\n\
**Workspace version line**: `v0.16.0`\n\
**Published crate surface**: 31 crates\n\
- Remaining work is operational: finish `v0.16.0` prep verification, then publish and record final channel receipts\n";
        let expected = "**Current release train**: `v0.17.0` — shipped 2026-06-28 as public beta\n\
**Workspace version line**: `v0.17.0`\n\
**Published crate surface**: 32 crates\n\
- Remaining work is operational: finish `v0.17.0` prep verification, then publish and record final channel receipts\n";

        let first = sync_release_notes(input, &release_surface())?;
        if first != expected {
            bail!("first release-notes sync did not retain the exact shipped date");
        }
        let second = sync_release_notes(&first, &release_surface())?;
        if second != first {
            bail!("second release-notes sync was not idempotent");
        }
        Ok(())
    }

    #[test]
    fn sync_status_index_preserves_release_posture_on_first_and_second_write() -> Result<()> {
        let input = "- **Release posture**: `v0.16.0` is in release preparation.\n\
**Now (active milestone: v0.16.0 release preparation)**\n\
- Run the `v0.16.0` release-prep checks before dispatching release orchestration\n\
- Keep the top-level README and status docs aligned with the 31-crate published surface\n\
**Next (post v0.16.0 release preparation)**\n";
        let expected = "- **Release posture**: `v0.17.0` is the current workspace version and shipped public-beta release (2026-06-28); `v0.18.0` is the next public-beta train, not a maturity promotion or version bump in this tree. The published crate surface is 32 crates. See [release.md](release.md) for channel receipts.\n\
**Now (active milestone: v0.17.0 shipped public beta)**\n\
- Run the `v0.17.0` release-prep checks before dispatching release orchestration\n\
- Keep the top-level README, status docs, and release runbooks aligned with the actual `perllsp` asset line, the `perl-lsp-rs` extension package, and the 32-crate published surface\n\
**Next (v0.18.0 public-beta train)**\n";

        let first = sync_status_index(input, &release_surface())?;
        if first != expected {
            bail!("first status-index sync did not retain the exact release posture facts");
        }
        let second = sync_status_index(&first, &release_surface())?;
        if second != first {
            bail!("second status-index sync was not idempotent");
        }
        Ok(())
    }

    #[test]
    fn sync_roadmap_preserves_release_posture_on_first_and_second_write() -> Result<()> {
        let input = "- Workspace version line: `v0.14.0`\n\
- Current release train: `v0.14.0` public-alpha closeout\n\
- Published crate surface target: 31 crates\n\
Publication discipline: `v0.14.0` uses a normal SemVer package version for release channels while the human-facing product posture remains public alpha.\n\
## Active: Public-Alpha Channel Closeout (v0.14.0)\n\
### Now (v0.14.0 public-alpha channel closeout)\n\
- `v0.14.0` is the current public-alpha release line; finish receipt closeout before treating the release as fully closed\n\
### Next (post v0.14.0 closeout)\n";
        let expected = "- Workspace version line: `v0.17.0`\n\
- Current release train: `v0.17.0` shipped public beta; channel receipts remain independently verified\n\
- Published crate surface target: 32 crates from `[workspace.metadata.publish.allow]`\n\
Publication discipline: `v0.17.0` uses a normal SemVer package version while the human-facing product posture remains public beta, not stable/GA. See [RELEASE_HISTORY.md](../../RELEASE_HISTORY.md) for independently verified channel receipts.\n\
## Active: Public-Beta Release (v0.17.0)\n\
### Now (v0.17.0 shipped public beta)\n\
- `v0.17.0` is shipped public beta; keep each distribution channel pending until its receipt is verified\n\
### Next (post v0.17.0)\n";

        let first = sync_roadmap(input, &release_surface())?;
        if first != expected {
            bail!("first roadmap sync did not retain release posture");
        }
        let second = sync_roadmap(&first, &release_surface())?;
        if second != first {
            bail!("second roadmap sync was not idempotent");
        }
        Ok(())
    }
}
