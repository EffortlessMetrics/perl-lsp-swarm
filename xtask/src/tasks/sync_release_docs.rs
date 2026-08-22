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
    shipped_date: Option<String>,
    prior_version: Option<String>,
    prior_shipped_date: Option<String>,
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

fn collect_release_dates(
    root: &Path,
    version: &str,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let changelog_path = root.join("CHANGELOG.md");
    let changelog = fs::read_to_string(&changelog_path)
        .with_context(|| format!("reading {}", changelog_path.display()))?;
    let releases: Vec<(&str, &str)> =
        changelog.lines().filter_map(parse_changelog_release).collect();
    if releases.is_empty() {
        bail!("CHANGELOG.md: no release headings found");
    }

    let current_index = releases.iter().position(|(candidate, _)| *candidate == version);
    let shipped_date = current_index.and_then(|index| releases.get(index)).and_then(|(_, date)| {
        (!date.eq_ignore_ascii_case("unreleased")).then_some((*date).to_string())
    });
    let prior_index = current_index.map_or(0, |index| index.saturating_add(1));
    let (prior_version, prior_shipped_date) = releases
        .iter()
        .skip(prior_index)
        .find(|(_, date)| !date.eq_ignore_ascii_case("unreleased"))
        .map(|(prior_version, prior_shipped_date)| {
            ((*prior_version).to_string(), (*prior_shipped_date).to_string())
        })
        .map_or((None, None), |(version, date)| (Some(version), Some(date)));

    Ok((shipped_date, prior_version, prior_shipped_date))
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
    let patch = parts.next().ok_or_else(|| color_eyre::eyre::eyre!("invalid version {version}"))?;
    if patch.contains('-') {
        bail!("refusing to compute next minor of pre-release version {version}");
    }
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
        } else if let Some(updated) =
            sync_readme_published_surface_prose(line, surface.published_crate_count)
        {
            lines.push(updated);
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

    // The front-door README intentionally stopped restating the crate-count
    // receipt (#5450); its absence is current intent, not drift. A row is still
    // refreshed in place when present. The count remains owned by
    // CURRENT_STATUS.md, status/index.md, and status/release.md.
    if !release_track_seen && !verified_install_seen {
        bail!("README.md: release posture line not found");
    }
    Ok(restore_trailing_newline(content, &lines))
}

fn sync_readme_published_surface_prose(line: &str, published_crate_count: usize) -> Option<String> {
    let marker = "The published surface is ";
    let marker_start = line.find(marker)?;
    let count_start = marker_start + marker.len();
    let remainder = &line[count_start..];
    let (count, suffix) = remainder.split_once(" crates")?;
    if count.is_empty() || !count.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    Some(format!("{}{}{} crates{}", &line[..marker_start], marker, published_crate_count, suffix))
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
            let release_facts = match (
                surface.shipped_date.as_deref(),
                surface.prior_version.as_deref(),
                surface.prior_shipped_date.as_deref(),
            ) {
                (Some(shipped_date), Some(prior_version), Some(prior_shipped_date)) => format!(
                    "`v{}` latest public beta ({}); prior `v{}` ({})",
                    surface.version, shipped_date, prior_version, prior_shipped_date
                ),
                (Some(shipped_date), _, _) => format!(
                    "`v{}` latest public beta ({}); prior release receipt unavailable",
                    surface.version, shipped_date
                ),
                (None, Some(prior_version), Some(prior_shipped_date)) => format!(
                    "`v{}` release preparation; latest shipped public beta `v{}` ({})",
                    surface.version, prior_version, prior_shipped_date
                ),
                (None, _, _) => format!(
                    "`v{}` release preparation; shipped release receipt unavailable",
                    surface.version
                ),
            };
            lines.push(format!(
                "| **Current release train** | {release_facts} | [CHANGELOG.md](../../CHANGELOG.md) |"
            ));
            train_seen = true;
        } else if line.starts_with("| **Published crate surface** | ") {
            lines.push(format!(
                "| **Published crate surface** | {} crates | [`[workspace.metadata.publish.allow]`](../../Cargo.toml) |",
                surface.published_crate_count
            ));
            count_seen = true;
        } else if line.starts_with("| **Active milestone** | `") {
            let milestone = if surface.shipped_date.is_some() {
                format!(
                    "`v{}` shipped public beta; `v{}` next public-beta train",
                    surface.version, surface.next_version
                )
            } else {
                format!(
                    "`v{}` release preparation; `v{}` next public-beta train",
                    surface.version, surface.next_version
                )
            };
            lines.push(format!(
                "| **Active milestone** | {milestone} | [status/index.md](status/index.md) |"
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
    let mut in_active_release = false;
    let mut in_now_section = false;
    let mut in_next_section = false;

    let release_heading_marker = format!("(v{})", surface.version);
    let release_heading_indices: Vec<usize> = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let is_release_heading = line.starts_with("## Active: Public-Alpha Release Prep (v")
                || line.starts_with("## Active: Public-Beta Release (v")
                || line.starts_with("## Active: Public-Beta Release Preparation (v")
                || line.starts_with("## Active: Public-Alpha Channel Closeout (v");
            is_release_heading.then_some(index)
        })
        .collect();
    let active_heading_index = release_heading_indices
        .iter()
        .copied()
        .find(|index| {
            content
                .lines()
                .nth(*index)
                .is_some_and(|line| line.trim_end().ends_with(&release_heading_marker))
        })
        .or_else(|| release_heading_indices.first().copied());

    let mut lines: Vec<String> = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        if line.starts_with("## ") {
            let is_release_heading = line.starts_with("## Active: Public-Alpha Release Prep (v")
                || line.starts_with("## Active: Public-Beta Release (v")
                || line.starts_with("## Active: Public-Beta Release Preparation (v")
                || line.starts_with("## Active: Public-Alpha Channel Closeout (v");
            in_active_release = (is_release_heading
                && Some(line_index) == active_heading_index
                && !active_section_seen)
                || line.trim() == "## Now / Next / Later";
            in_now_section = false;
            in_next_section = false;
        } else if line.starts_with("### Now (") {
            in_now_section = true;
            in_next_section = false;
        } else if line.starts_with("### Next (") {
            in_now_section = false;
            in_next_section = true;
        }

        if line.starts_with("- Workspace version line: `v") {
            lines.push(format!("- Workspace version line: `v{}`", surface.version));
            workspace_version_seen = true;
        } else if line.starts_with("- Current release train: `v") {
            lines.push(if surface.shipped_date.is_some() {
                format!(
                    "- Current release train: `v{}` shipped public beta; channel receipts remain independently verified",
                    surface.version
                )
            } else {
                format!(
                    "- Current release train: `v{}` release preparation; shipped release receipt remains pending",
                    surface.version
                )
            });
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
        } else if in_active_release
            && !active_section_seen
            && Some(line_index) == active_heading_index
            && (line.starts_with("## Active: Public-Alpha Release Prep (v")
                || line.starts_with("## Active: Public-Beta Release (v")
                || line.starts_with("## Active: Public-Beta Release Preparation (v")
                || line.starts_with("## Active: Public-Alpha Channel Closeout (v"))
        {
            lines.push(format!(
                "## Active: Public-Beta Release{} (v{})",
                if surface.shipped_date.is_some() { "" } else { " Preparation" },
                surface.version
            ));
            active_section_seen = true;
        } else if in_active_release
            && line.starts_with("### Now (v")
            && (line.contains("public-alpha patch prep)")
                || line.contains("shipped public beta)")
                || line.contains("release preparation)")
                || line.contains("public-alpha channel closeout)"))
        {
            lines.push(if surface.shipped_date.is_some() {
                format!("### Now (v{} shipped public beta)", surface.version)
            } else {
                format!("### Now (v{} release preparation)", surface.version)
            });
            now_section_seen = true;
        } else if in_active_release
            && in_now_section
            && line.starts_with("- `v")
            && (line.contains("is staged as the next public-alpha patch release; run the release-prep checks before dispatching the train")
                || line.contains("is shipped public beta; keep each distribution channel pending until its receipt is verified")
                || line.contains("is in release preparation; run the release-prep checks before dispatching the train")
                || line.contains("is the current public-alpha release line; finish receipt closeout before treating the release as fully closed"))
        {
            lines.push(if surface.shipped_date.is_some() {
                format!(
                    "- `v{}` is shipped public beta; keep each distribution channel pending until its receipt is verified",
                    surface.version
                )
            } else {
                format!(
                    "- `v{}` is in release preparation; run the release-prep checks before dispatching the train",
                    surface.version
                )
            });
            now_gate_seen = true;
        } else if in_active_release
            && (line.starts_with("- GitHub Release and crates.io surfaces show ")
                || line.starts_with("- GitHub Release assets for `v"))
        {
            lines.push(if surface.shipped_date.is_some() {
                format!(
                    "- GitHub Release assets for `v{}` are verified; crates.io, Docker, VS Code Marketplace, Open VSX, and Homebrew remain pending/not proven until their receipts are verified",
                    surface.version
                )
            } else {
                format!(
                    "- GitHub Release, crates.io, Docker, VS Code Marketplace, Open VSX, and Homebrew surfaces for `v{}` remain pending until their release receipts are verified",
                    surface.version
                )
            });
        } else if in_active_release
            && (line.starts_with("- Public install language must say public alpha")
                || line.starts_with("- Public install language must say public beta"))
        {
            lines.push("- Public install language must say public beta and avoid stable/GA claims".to_string());
        } else if in_active_release && line.starts_with("| Version surface | ") {
            lines.push(format!(
                "| Version surface | Workspace package version, `features.toml` metadata, extension packaging, release notes, and changelog align with the current `v{}` train | [`../../Cargo.toml`](../../Cargo.toml), [`../../features.toml`](../../features.toml), [docs/releases/v{}.md](../releases/v{}.md) |",
                surface.version, surface.version, surface.version
            ));
        } else if in_active_release && line.starts_with("| Publish surface | ") {
            lines.push(format!(
                "| Publish surface | The {}-crate allowlist has dry-run or publish receipts, and deferred items have successor issues rather than silent drops | [`[workspace.metadata.publish.allow]`](../../Cargo.toml), [docs/releases/v{}.md](../releases/v{}.md) |",
                surface.published_crate_count, surface.version, surface.version
            ));
        } else if in_active_release && line.starts_with("| Install channels | ") {
            lines.push(format!(
                "| Install channels | GitHub assets, crates.io, Docker, VS Code Marketplace, Open VSX, and Homebrew each have an install/smoke receipt or an explicit pending/deferred state | [status/release.md](status/release.md), [CURRENT_STATUS.md](CURRENT_STATUS.md), [docs/releases/v{}.md](../releases/v{}.md) |",
                surface.version, surface.version
            ));
        } else if in_active_release && line.starts_with("| Public wording | ") {
            lines.push(format!(
                "| Public wording | User-facing docs call the release public beta and avoid stable/GA promises | [docs/releases/v{}.md](../releases/v{}.md), [CURRENT_STATUS.md](CURRENT_STATUS.md) |",
                surface.version, surface.version
            ));
        } else if in_now_section
            && (line.starts_with("- Reconcile the live `v")
                || line.starts_with("- Keep the verified `v"))
        {
            lines.push(format!(
                "- Keep the verified `v{}` release receipt linked to release notes, release history, generated status, and the remaining channel receipts",
                surface.version
            ));
        } else if in_next_section && line.starts_with("1. **Close release receipts first.**") {
            lines.push(format!(
                "1. **Close release receipts first.** Do not start broad feature cleanup until the `v{}` channel ledger is explicit about what shipped, what is pending, and what users should install.",
                surface.version
            ));
        } else if in_next_section && line.starts_with("5. **Burn down tracked debt by ledger.**") {
            lines.push(format!(
                "5. **Burn down tracked debt by ledger.** Use successor issues from [docs/releases/v{}.md](../releases/v{}.md) for tracked follow-up work and explicit claim boundaries.",
                surface.version, surface.version
            ));
        } else if in_next_section && line.starts_with("- Keep public-alpha release notes ") {
            lines.push("- Keep public-beta release notes concise and tied to concrete channel receipts".to_string());
        } else if in_next_section && line.starts_with("- **Distribution maturity:** ") {
            lines.push("- **Distribution maturity:** make Homebrew, Docker, crates.io, VS Code Marketplace, Open VSX, and GitHub Releases behave like one coherent public-beta install story.".to_string());
        } else if in_active_release && line.starts_with("### Next (post v") {
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

    let status_lines: Vec<&str> = content.lines().collect();
    let next_heading_indices: Vec<usize> = status_lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            (line.starts_with("**Next (post v") || line.starts_with("**Next (v")).then_some(index)
        })
        .collect();
    let active_next_index = next_heading_indices
        .iter()
        .copied()
        .find(|index| {
            status_lines[*index].trim_end()
                == format!("**Next (v{} public-beta train)**", surface.next_version)
        })
        .or_else(|| {
            next_heading_indices.iter().copied().find(|index| {
                let line = status_lines[*index].trim_end();
                let Some(suffix) = line.strip_prefix(&format!("**Next (post v{}", surface.version))
                else {
                    return false;
                };
                suffix.starts_with(')') || suffix.starts_with(' ')
            })
        })
        .or_else(|| {
            let active_now_index = status_lines.iter().position(|line| {
                line.starts_with(&format!("**Now (active milestone: v{}", surface.version))
            });
            active_now_index.and_then(|now_index| {
                next_heading_indices.iter().copied().find(|next_index| *next_index > now_index)
            })
        })
        .or_else(|| next_heading_indices.first().copied());

    let mut lines: Vec<String> = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        if line.starts_with("- **Release posture**: `v") {
            let release_facts = match (
                surface.shipped_date.as_deref(),
                surface.prior_version.as_deref(),
                surface.prior_shipped_date.as_deref(),
            ) {
                (Some(shipped_date), _, _) => format!(
                    "`v{}` is the current workspace version and shipped public-beta release ({})",
                    surface.version, shipped_date
                ),
                (None, Some(prior_version), Some(prior_shipped_date)) => format!(
                    "`v{}` is the current workspace version and release-preparation train; latest shipped public-beta release is `v{}` ({})",
                    surface.version, prior_version, prior_shipped_date
                ),
                (None, _, _) => format!(
                    "`v{}` is the current workspace version and release-preparation train",
                    surface.version
                ),
            };
            lines.push(format!(
                "- **Release posture**: {release_facts}; `v{}` is the next public-beta train, not a maturity promotion or version bump in this tree. The published crate surface is {} crates. See [release.md](release.md) for channel receipts.",
                surface.next_version, surface.published_crate_count
            ));
            release_posture_seen = true;
        } else if line.starts_with("**Now (active milestone: v") {
            let milestone = if surface.shipped_date.is_some() {
                "shipped public beta"
            } else {
                "release preparation"
            };
            lines.push(format!("**Now (active milestone: v{} {milestone})**", surface.version));
            now_section_seen = true;
        } else if (line.starts_with("- Run the `v")
            && line.contains(" release-prep checks before dispatching release orchestration"))
            || (line.starts_with("- Verify the existing `v")
                && line.contains(" release receipt and close the remaining channel receipts"))
        {
            lines.push(if surface.shipped_date.is_some() {
                format!(
                    "- Verify the existing `v{}` release receipt and close the remaining channel receipts; do not dispatch release orchestration for an already-shipped train",
                    surface.version
                )
            } else {
                format!(
                    "- Run the `v{}` release-prep checks before dispatching release orchestration",
                    surface.version
                )
            });
            now_gate_seen = true;
        } else if line.contains("published surface")
            && line.starts_with("- Keep the top-level README")
        {
            lines.push(format!(
                "- Keep the top-level README, status docs, and release runbooks aligned with the actual `perllsp` asset line, the `perl-lsp-rs` extension package, and the {}-crate published surface",
                surface.published_crate_count
            ));
            published_surface_bullet_seen = true;
        } else if !next_section_seen
            && Some(line_index) == active_next_index
            && (line.starts_with("**Next (post v") || line.starts_with("**Next (v"))
        {
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
            lines.push(if let Some(shipped_date) = surface.shipped_date.as_deref() {
                format!(
                    "**Current release train**: `v{}` — shipped {} as public beta",
                    surface.version, shipped_date
                )
            } else {
                format!(
                    "**Current release train**: `v{}` — release preparation; shipped release receipt pending",
                    surface.version
                )
            });
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
        } else if (line.starts_with("- Remaining work is operational: finish `v")
            && line.contains(" prep verification, then publish and record final channel receipts"))
            || (line.starts_with("- Remaining work is operational: verify the existing `v")
                && line.contains(" release receipt and close the remaining channel receipts"))
        {
            lines.push(if surface.shipped_date.is_some() {
                format!(
                    "- Remaining work is operational: verify the existing `v{}` release receipt and close the remaining channel receipts; do not dispatch release orchestration for an already-shipped train.",
                    surface.version
                )
            } else {
                format!(
                    "- Remaining work is operational: finish `v{}` prep verification, then publish and record final channel receipts.",
                    surface.version
                )
            });
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
            shipped_date: Some("2026-06-28".to_string()),
            prior_version: Some("0.16.0".to_string()),
            prior_shipped_date: Some("2026-06-06".to_string()),
            next_version: "0.18.0".to_string(),
        }
    }

    fn preparation_release_surface() -> ReleaseSurface {
        ReleaseSurface {
            version: "0.18.0".to_string(),
            published_crate_count: 32,
            shipped_date: None,
            prior_version: Some("0.17.0".to_string()),
            prior_shipped_date: Some("2026-06-28".to_string()),
            next_version: "0.19.0".to_string(),
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
    fn collect_release_dates_allows_unreleased_workspace_version() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("CHANGELOG.md"),
            "## [0.17.0] - 2026-06-28\n\n## [0.16.0] - Unreleased\n",
        )?;

        let (shipped_date, prior_version, prior_shipped_date) =
            collect_release_dates(dir.path(), "0.18.0")?;
        assert_eq!(shipped_date, None);
        assert_eq!(prior_version.as_deref(), Some("0.17.0"));
        assert_eq!(prior_shipped_date.as_deref(), Some("2026-06-28"));
        Ok(())
    }

    #[test]
    fn collect_release_dates_skips_unreleased_prior_entries() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("CHANGELOG.md"),
            "## [0.18.0] - Unreleased\n\n## [0.17.0] - Unreleased\n\n## [0.16.0] - 2026-06-06\n",
        )?;

        let (shipped_date, prior_version, prior_shipped_date) =
            collect_release_dates(dir.path(), "0.18.0")?;
        assert_eq!(shipped_date, None);
        assert_eq!(prior_version.as_deref(), Some("0.16.0"));
        assert_eq!(prior_shipped_date.as_deref(), Some("2026-06-06"));
        Ok(())
    }

    #[test]
    fn next_minor_version_rejects_pre_release_versions() {
        let error = next_minor_version("0.17.0-rc1").expect_err("pre-release must fail closed");
        assert!(error.to_string().contains("pre-release"));
    }

    #[test]
    fn sync_preparation_roadmap_is_idempotent() -> Result<()> {
        let input = r#"- Workspace version line: `v0.18.0`
- Current release train: `v0.18.0` release preparation
- Published crate surface target: 32 crates
Publication discipline: `v0.18.0` uses a normal SemVer package version for release channels while the human-facing product posture remains public beta.
## Active: Public-Beta Release Preparation (v0.18.0)
### Now (v0.18.0 release preparation)
- `v0.18.0` is in release preparation; run the release-prep checks before dispatching the train
### Next (post v0.18.0)
"#;

        let first = sync_roadmap(input, &preparation_release_surface())?;
        assert!(first.contains("## Active: Public-Beta Release Preparation (v0.18.0)"));
        assert!(first.contains("### Now (v0.18.0 release preparation)"));
        assert!(first.contains("v0.18.0` is in release preparation"));
        assert_eq!(sync_roadmap(&first, &preparation_release_surface())?, first);
        Ok(())
    }

    #[test]
    fn sync_readme_preserves_verified_install_posture_on_first_and_second_write() -> Result<()> {
        let input = "| Published crate surface | 30 crates in `[workspace.metadata.publish.allow]` |\n\
The verified GitHub `v0.16.0` release assets are public alpha. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";
        let expected = "| Published crate surface | 32 crates in `[workspace.metadata.publish.allow]` |\n\
The verified GitHub `v0.17.0` release assets are public beta. Other distribution\n\
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
    fn sync_readme_accepts_missing_published_surface_row_without_insertion() -> Result<()> {
        let input = "| Release track | `v0.16.0` public alpha |\n\
The verified GitHub `v0.16.0` release assets are public alpha. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";
        let expected = "| Release track | `v0.17.0` public beta |\n\
The verified GitHub `v0.17.0` release assets are public beta. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";

        let synced = sync_readme(input, &release_surface())?;
        if synced != expected {
            bail!("README sync did not refresh posture without inserting a crate-count row");
        }
        if synced.contains("| Published crate surface |") {
            bail!("README sync must not reinsert the removed crate-count row");
        }
        Ok(())
    }

    #[test]
    fn sync_readme_refreshes_published_surface_prose() -> Result<()> {
        let input = "The published surface is 33 crates, listed in `[workspace.metadata.publish.allow]` in [`Cargo.toml`](Cargo.toml).\n\
The verified GitHub `v0.16.0` release assets are public alpha. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";
        let expected = "The published surface is 32 crates, listed in `[workspace.metadata.publish.allow]` in [`Cargo.toml`](Cargo.toml).\n\
The verified GitHub `v0.17.0` release assets are public beta. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";

        assert_eq!(sync_readme(input, &release_surface())?, expected);
        Ok(())
    }

    #[test]
    fn sync_readme_published_surface_prose_is_idempotent() -> Result<()> {
        let input = "The published surface is 33 crates, listed in `[workspace.metadata.publish.allow]` in [`Cargo.toml`](Cargo.toml).\n\
The verified GitHub `v0.16.0` release assets are public alpha. Other distribution\n\
channels remain independently versioned and must be verified before editor use.\n";

        let first = sync_readme(input, &release_surface())?;
        assert_eq!(sync_readme(&first, &release_surface())?, first);
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
- Remaining work is operational: verify the existing `v0.17.0` release receipt and close the remaining channel receipts; do not dispatch release orchestration for an already-shipped train.\n";

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
- Verify the existing `v0.17.0` release receipt and close the remaining channel receipts; do not dispatch release orchestration for an already-shipped train\n\
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
    fn sync_status_index_preserves_preparation_milestone() -> Result<()> {
        let input = "- **Release posture**: `v0.17.0` shipped public beta\n\
**Now (active milestone: v0.17.0 shipped public beta)**\n\
- Run the `v0.17.0` release-prep checks before dispatching release orchestration\n\
- Keep the top-level README and status docs aligned with the 31-crate published surface\n\
**Next (post v0.17.0 release preparation)**\n";

        let first = sync_status_index(input, &preparation_release_surface())?;
        assert!(
            first.contains(
                "`v0.18.0` is the current workspace version and release-preparation train"
            )
        );
        assert!(first.contains("**Now (active milestone: v0.18.0 release preparation)**"));
        assert!(!first.contains("v0.18.0 shipped public beta"));
        assert_eq!(sync_status_index(&first, &preparation_release_surface())?, first);
        Ok(())
    }

    #[test]
    fn sync_status_index_preserves_historical_next_heading() -> Result<()> {
        let input = "- **Release posture**: `v0.16.0` is in release preparation.\n\
**Now (active milestone: v0.16.0 release preparation)**\n\
- Run the `v0.16.0` release-prep checks before dispatching release orchestration\n\
**Next (post v0.16.0 release preparation)**\n\
**Next (post v0.12.0)**\n";

        let input = format!(
            "{input}- Keep the top-level README and status docs aligned with the 31-crate published surface\n"
        );
        let synced = sync_status_index(&input, &release_surface())?;
        assert!(synced.contains("**Next (v0.18.0 public-beta train)**"));
        assert!(synced.contains("**Next (post v0.12.0)**"));
        Ok(())
    }

    #[test]
    fn sync_status_index_selects_active_next_after_historical_heading() -> Result<()> {
        let input = r#"- **Release posture**: `v0.17.0` shipped public beta
**Now (active milestone: v0.17.0 shipped public beta)**
- Verify the existing `v0.17.0` release receipt and close the remaining channel receipts
**Next (v0.18.0-rc1)**
**Next (post v0.12.0)**
**Next (post v0.17.0)**
- Keep the top-level README and status docs aligned with the 31-crate published surface
"#;

        let synced = sync_status_index(input, &release_surface())?;
        assert!(synced.contains("**Next (post v0.12.0)**"));
        assert!(synced.contains("**Next (v0.18.0-rc1)**"));
        assert!(synced.contains("**Next (v0.18.0 public-beta train)**"));
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

    #[test]
    fn sync_roadmap_rewrites_stale_active_release_facts() -> Result<()> {
        let input = r#"- Workspace version line: `v0.14.0`
- Current release train: `v0.14.0` public-alpha closeout
- Published crate surface target: 31 crates
Publication discipline: `v0.14.0` uses a normal SemVer package version for release channels while the human-facing product posture remains public alpha.
## Active: Public-Alpha Channel Closeout (v0.14.0)
- GitHub Release and crates.io surfaces show `v0.14.0` live; Docker, VS Code Marketplace, Open VSX, and Homebrew tap receipts are still tracked separately until verified
- Public install language must say public alpha, not stable/GA
| Version surface | Workspace package version, `features.toml` metadata, extension packaging, release notes, and changelog all name the same `v0.14.0` train | old |
| Publish surface | The 31-crate allowlist has dry-run or publish receipts, and deferred items have successor issues rather than silent drops | old |
| Public wording | User-facing docs call the release public alpha and avoid stable/GA promises | old |
### Now (v0.14.0 public-alpha channel closeout)
- Reconcile the live `v0.14.0` GitHub Release and crates.io surfaces with release notes, release history, generated status, and remaining channel receipts.
- `v0.14.0` is the current public-alpha release line; finish receipt closeout before treating the release as fully closed
### Next (post v0.14.0 closeout)
#### Post-Release Sequencing
1. **Close release receipts first.** Do not start broad feature cleanup until the v0.14.0 channel ledger is explicit about what shipped, what is pending, and what users should install.
5. **Burn down tracked debt by ledger.** Use successor issues from [docs/releases/v0.14.0.md](../releases/v0.14.0.md) for tracked follow-up work.
"#;

        let synced = sync_roadmap(input, &release_surface())?;
        assert!(synced.contains("GitHub Release assets for `v0.17.0` are verified"));
        assert!(synced.contains("User-facing docs call the release public beta"));
        assert!(!synced.contains("v0.14.0` live"));
        assert!(!synced.contains("public alpha, not stable/GA"));
        assert!(!synced.contains("v0.14.0 channel ledger"));
        assert!(sync_roadmap(&synced, &release_surface())? == synced);
        Ok(())
    }

    #[test]
    fn sync_roadmap_preserves_historical_preparation_sections() -> Result<()> {
        let input = "- Workspace version line: `v0.17.0`\n\
- Current release train: `v0.17.0` public-alpha closeout\n\
- Published crate surface target: 31 crates\n\
Publication discipline: `v0.17.0` uses a normal SemVer package version while the human-facing product posture remains public alpha.\n\
## Active: Public-Beta Release (v0.17.0-rc1)\n\
## Active: Public-Alpha Release Prep (v0.13.0)\n\
### Now (v0.13.0 release preparation)\n\
- `v0.13.0` is in release preparation; run the release-prep checks before dispatching the train\n\
### Next (post v0.13.0)\n\
## Active: Public-Alpha Channel Closeout (v0.17.0)\n\
### Now (v0.17.0 public-alpha channel closeout)\n\
- `v0.17.0` is the current public-alpha release line; finish receipt closeout before treating the release as fully closed\n\
### Next (post v0.17.0 closeout)\n\
## Now / Next / Later   \n\
### Now (v0.17.0 public-alpha channel closeout)\n\
- `v0.17.0` is the current public-alpha release line; finish receipt closeout before treating the release as fully closed\n\
### Next (post v0.17.0 closeout)\n";

        let synced = sync_roadmap(input, &release_surface())?;
        assert!(synced.contains("## Active: Public-Alpha Release Prep (v0.13.0)"));
        assert!(synced.contains("## Active: Public-Beta Release (v0.17.0-rc1)"));
        assert!(synced.contains("### Now (v0.17.0 shipped public beta)"));
        assert!(synced.contains("### Now (v0.13.0 release preparation)"));
        assert!(synced.contains(
            "v0.13.0` is in release preparation; run the release-prep checks before dispatching the train"
        ));
        assert!(synced.contains("### Next (post v0.13.0)"));
        assert!(synced.contains("## Active: Public-Beta Release (v0.17.0)"));
        Ok(())
    }
}
