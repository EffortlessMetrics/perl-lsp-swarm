use super::{ActiveMutation, DoctorHeading, SOURCE_PATHS, SourceDigest};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const EXPECTED_HEADINGS: [(u32, &str); 7] = [
    (1, "core.bare = true corruption (#3205)"),
    (2, "Stale local branches (upstream gone)"),
    (3, "Worktree file leaks"),
    (4, "Orphaned worktree directories"),
    (5, "pre-push hook installed"),
    (6, "Workspace clean"),
    (7, "Current checkout is fast-forward-able with remote default branch."),
];

pub(super) struct SourceFacts {
    pub headings: Vec<DoctorHeading>,
    pub active_mutations: Vec<ActiveMutation>,
    pub sources: BTreeMap<String, SourceDigest>,
}

pub(super) fn inspect_sources(root: &Path) -> Result<SourceFacts> {
    let mut texts = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for relative in SOURCE_PATHS {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        texts.insert(relative, text);
        sources.insert(
            relative.to_string(),
            SourceDigest {
                path: relative.to_string(),
                sha256: digest,
            },
        );
    }

    let justfile = texts
        .get("justfile")
        .context("justfile missing from source map")?;
    let doctor = doctor_block(justfile)?;
    let headings = doctor_headings(&doctor)?;
    require_markers(
        &doctor,
        &[
            "common_dir=$(git rev-parse --git-common-dir",
            "config --local --get core.bare",
            "config --local --unset core.bare",
            "Auto-fixed: unset core.bare",
            "Stale local branches (upstream gone)",
            "git branch -D <branch>",
            "worktree file leaks",
            "main_dirty_set[\"$path\"]=1",
            "orphaned worktree directories",
            "git worktree prune; rm -rf <dir>",
            "pre-push hook installed",
            "pre-push hook installed but stale",
            "cargo xtask ci-hygiene install-githooks",
            "--untracked-files=no",
            "workspace has $dirty_count uncommitted changes",
            "HEAD is $behind commits behind $default_remote_ref",
            "Fix: git pull --ff-only",
            "could not resolve default remote branch",
            "git remote set-head origin -a && git fetch origin",
            "echo \"$issues issues found, $fixed auto-fixed\"",
            "exit 0",
        ],
        "doctor",
    )?;
    require_absent(
        &doctor,
        &[
            "refs/heads/origin/",
            "symbolic-ref --quiet HEAD",
            "writer-collision",
            "gh pr list",
            "unpushed_commits",
            "@{u}",
        ],
        "doctor",
    )?;
    require_markers(
        justfile,
        &[
            "ready: doctor pr-fast",
            "Workspace is ready to push (doctor + pr-fast passed)",
            "pr-fast: _check-tools-basic",
            "cargo xtask check-toolchain",
            "cargo xtask gates \"${args[@]}\"",
        ],
        "justfile",
    )?;

    let writer = texts
        .get("xtask/src/tasks/writer_admission.rs")
        .context("writer admission source missing")?;
    require_markers(
        writer,
        &[
            "check_shadow_ref(snapshot)",
            "check_symbolic_head(snapshot)",
            "check_branch_worktree_mapping(snapshot)",
            "check_dirty_unpushed(snapshot, config)",
            "check_disk_capacity(snapshot, config)",
            "check_writer_collision(snapshot)",
            "Advisory-first: `run` always returns `Ok(())`",
            "disk-capacity",
            "floor_gb",
        ],
        "writer-admission",
    )?;
    let devex = texts
        .get("xtask/src/tasks/devex_doctor.rs")
        .context("devex doctor source missing")?;
    require_markers(
        devex,
        &[
            "check_command(\"cargo\"",
            "check_command_optional(\"just\"",
            "check_command_optional(\"nix\"",
            "check_command_optional(\"cargo-audit\"",
            "check_pre_push_hook();",
            "check_pre_commit_hook();",
            "check_build_storage(&root);",
            "pre-commit hook missing or not executable",
        ],
        "devex-doctor",
    )?;
    let worktrees = texts
        .get("xtask/src/tasks/worktrees.rs")
        .context("worktree source missing")?;
    require_markers(
        worktrees,
        &[
            "Dry-run report",
            "args([\"worktree\", \"prune\"])",
            "PrStatus::Unknown",
        ],
        "worktree-cleanup",
    )?;
    let hooks = texts
        .get("crates/perl-ci-hygiene/src/cli.rs")
        .context("ci-hygiene CLI source missing")?;
    require_markers(hooks, &["InstallGithooks", "CheckGithooks"], "ci-hygiene")?;
    let storage = texts
        .get("scripts/storage-doctor")
        .context("storage doctor source missing")?;
    require_markers(
        storage,
        &[
            "repo-local target dirs",
            "repo-local target dir exceeds 1G",
            "sccache --show-stats",
        ],
        "storage-doctor",
    )?;

    let active_mutations = detect_mutations(&doctor)?;
    if active_mutations.len() != 1 || active_mutations[0].owned_by != "core-bare" {
        bail!("doctor mutation denominator changed: {active_mutations:?}");
    }
    Ok(SourceFacts {
        headings,
        active_mutations,
        sources,
    })
}

fn doctor_block(justfile: &str) -> Result<String> {
    let mut lines = justfile.lines();
    lines
        .find(|line| line.trim_end() == "doctor:")
        .context("justfile has no doctor recipe")?;
    let mut block = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        let recipe_header = line
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed.ends_with(':');
        if recipe_header {
            break;
        }
        block.push(line);
    }
    if block.is_empty() {
        bail!("doctor recipe body is empty");
    }
    Ok(block.join("\n"))
}

fn doctor_headings(doctor: &str) -> Result<Vec<DoctorHeading>> {
    let mut headings = Vec::new();
    for line in doctor.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("# Check ") else {
            continue;
        };
        let (number, title) = rest
            .split_once(':')
            .context("doctor check heading has no colon")?;
        headings.push(DoctorHeading {
            number: number
                .trim()
                .parse()
                .context("doctor check number is invalid")?,
            title: title.trim().to_string(),
        });
    }
    let expected: Vec<DoctorHeading> = EXPECTED_HEADINGS
        .iter()
        .map(|(number, title)| DoctorHeading {
            number: *number,
            title: (*title).to_string(),
        })
        .collect();
    if headings != expected {
        bail!("doctor check denominator changed: {headings:?}");
    }
    Ok(headings)
}

fn detect_mutations(doctor: &str) -> Result<Vec<ActiveMutation>> {
    let mut mutations = Vec::new();
    for line in doctor.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("echo ")
            || trimmed.starts_with("@echo ")
        {
            continue;
        }
        let kind = if trimmed.contains("git ")
            && trimmed.contains(" config ")
            && trimmed.contains(" --unset ")
        {
            Some("git_config_unset")
        } else if [
            "git reset ",
            "git clean ",
            "git checkout ",
            "git pull ",
            "git rebase ",
            "git worktree prune",
            "git worktree remove",
            "rm -rf ",
        ]
        .iter()
        .any(|marker| trimmed.contains(marker))
        {
            Some("unclassified_mutation")
        } else {
            None
        };
        if let Some(kind) = kind {
            let owned_by = if kind == "git_config_unset" && trimmed.contains("core.bare") {
                "core-bare"
            } else {
                "UNCLASSIFIED"
            };
            mutations.push(ActiveMutation {
                kind: kind.to_string(),
                line: trimmed.to_string(),
                owned_by: owned_by.to_string(),
            });
        }
    }
    if mutations
        .iter()
        .any(|mutation| mutation.owned_by == "UNCLASSIFIED")
    {
        bail!("unclassified mutation found in doctor: {mutations:?}");
    }
    Ok(mutations)
}

fn require_markers(text: &str, markers: &[&str], label: &str) -> Result<()> {
    for marker in markers {
        if !text.contains(marker) {
            bail!("{label} is missing required marker {marker:?}");
        }
    }
    Ok(())
}

fn require_absent(text: &str, markers: &[&str], label: &str) -> Result<()> {
    for marker in markers {
        if text.contains(marker) {
            bail!(
                "{label} unexpectedly contains previously omitted marker {marker:?}"
            );
        }
    }
    Ok(())
}
