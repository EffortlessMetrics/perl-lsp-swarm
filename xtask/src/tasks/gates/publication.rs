//! Publication is committed-object consistency, never a qualification result.
//! Reuse policy membership, existing snapshot checks, and the normal gate receipt.
use std::path::Path;

use color_eyre::eyre::{Context, ContextCompat, Result, bail};

use super::{
    GateDefinition, GatePlan, GatePolicy, GateRunnerConfig, GateTier, PlannedGate, SkippedGate,
};
use crate::tasks::change_set::{self, ArtifactIdentity, DiffMode};
use crate::tasks::git_context::git_stdout_with_worktree_fallback as git;

pub(super) fn is_mechanical_gate(gate: &GateDefinition) -> bool {
    gate.tier == "commit" && gate.tags.iter().any(|tag| tag == "publication-consistency")
}

/// A clean, matching checkout binds the policy and rustfmt/config readers too.
/// Other pushed heads are refused, rather than checking this checkout for them.
pub(super) fn validate_config(root: &Path, config: &GateRunnerConfig) -> Result<()> {
    if config.staged
        || config.subject.is_some()
        || config.route_plan_path.is_some()
        || config.gate_filter.is_some()
        || config.diff_baseline.is_some()
    {
        bail!(
            "publication requires the complete native consistency role, not staged/filtered/routed proof"
        );
    }
    let expected =
        config.head_ref.as_deref().context("publication requires --head <pushed-commit>")?;
    if expected.len() != 40 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("publication --head must be the exact pushed commit SHA, not a mutable ref");
    }
    let head = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let submitted = git(root, &["rev-parse", "--verify", &format!("{expected}^{{commit}}")])?;
    if head != submitted {
        bail!(
            "publication pushed head {submitted} is not checked-out HEAD {head}; use its matching checkout"
        );
    }
    if !git(root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        bail!(
            "publication requires a clean tracked checkout to bind policy/configuration to the pushed commit"
        );
    }
    if let Some(path) = &config.gate_policy {
        let path = if path.is_absolute() { path.clone() } else { root.join(path) };
        if path != root.join(".ci/gate-policy.yaml") {
            bail!("publication uses the committed canonical gate policy only");
        }
    }
    for name in ["rustfmt.toml", ".rustfmt.toml"] {
        if root.join(name).exists() {
            git(root, &["ls-files", "--error-unmatch", "--", name]).context(
                "untracked rustfmt configuration cannot govern committed publication proof",
            )?;
        }
    }
    Ok(())
}

/// A full bound role is executable; filtered publication reruns are refused.
pub(super) fn rerun_command(base: &str, head: &str) -> String {
    format!(
        "bash scripts/cargo-admitted run --package xtask --bin xtask --locked -- gates --tier publication --base {base} --head {head} --receipt"
    )
}

pub(super) fn bind_outcome(
    outcome: crate::tasks::commit_checks::CommitCheckOutcome,
    base: &str,
    head: &str,
) -> crate::tasks::commit_checks::CommitCheckOutcome {
    use crate::tasks::commit_checks::CommitCheckOutcome;
    match outcome {
        CommitCheckOutcome::Flagged(mut report) => {
            report.rerun = rerun_command(base, head);
            CommitCheckOutcome::Flagged(report)
        }
        pass => pass,
    }
}

pub(super) fn plan(
    root: &Path,
    policy: &GatePolicy,
    config: &GateRunnerConfig,
) -> Result<GatePlan> {
    validate_config(root, config)?;
    let base =
        config.base_ref.as_deref().context("publication requires --base <remote-sha|auto>")?;
    let head = config.head_ref.as_deref().context("publication requires --head")?;
    let identity = ArtifactIdentity::CommitRange { base: base.to_string(), head: head.to_string() };
    let resolved = change_set::resolve_change_set_with_mode(
        identity,
        root,
        if base == "auto" { DiffMode::MergeBaseThreeDot } else { DiffMode::DirectTwoDot },
    )?;
    let head = resolved.head_sha.context("publication head did not resolve")?;
    let resolved_base = resolved.base_sha.context("publication base did not resolve")?;
    // A new branch needs an established merge base. An acquisition failure or
    // self-comparison is NOT PROVEN, never eight vacuous successful checks.
    let base = if base == "auto" {
        if resolved_base == head {
            bail!(
                "automatic publication base equals pushed head; establish the real branch baseline"
            );
        }
        git(root, &["merge-base", &resolved_base, &head])
            .context("automatic publication merge-base is unavailable")?
    } else {
        resolved_base
    };
    if config.base_ref.as_deref() == Some("auto") && base == head {
        bail!(
            "effective automatic publication merge-base equals pushed head; establish the real branch baseline"
        );
    }
    let tree = git(root, &["rev-parse", "--verify", &format!("{head}^{{tree}}")])?;
    let mut selected = Vec::new();
    let mut skipped = Vec::new();
    for gate in &policy.gates {
        if is_mechanical_gate(gate) {
            if !gate.required
                || gate.quarantine
                || !gate.command.starts_with("cargo xtask commit-check ")
            {
                bail!(
                    "publication consistency gate {} has unsupported execution/enforcement",
                    gate.name
                );
            }
            let mut gate = gate.clone();
            gate.tier = "publication".to_string();
            selected.push(PlannedGate { gate, role: super::GatePlanningRole::Static,
                reason: format!("committed consistency at {base}..{head}, tree {tree}; not product qualification") });
        } else if gate.tier == "pr_fast" || gate.name == "changie_fragment_staged" {
            skipped.push(SkippedGate { name: gate.name.clone(), role: None,
                reason: format!("DEFERRED: affected candidate/late qualification retains `{}`; not passed or inapplicable", gate.command) });
        }
    }
    if selected.is_empty() {
        bail!("publication policy selected no consistency checks");
    }
    Ok(GatePlan {
        tier: GateTier::Publication,
        base,
        scope: None,
        scope_ok: true,
        fallback_used: false,
        fallback_reason: None,
        package_args: Vec::new(),
        selected,
        skipped,
        // Existing receipt field carries the frozen snapshot; the publication tier
        // distinguishes it from an index snapshot without a second receipt format.
        staged_tree_oid: Some(tree),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};
    use tempfile::{TempDir, tempdir};

    fn fixture_git(root: &Path, args: &[&str]) -> Result<String> {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .args(args)
            .output()?;
        if !output.status.success() {
            bail!("fixture git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    fn fixture() -> Result<(TempDir, String, String)> {
        let dir = tempdir()?;
        let root = dir.path();
        fixture_git(root, &["init", "-b", "main"])?;
        fixture_git(root, &["config", "user.name", "Publication fixture"])?;
        fixture_git(root, &["config", "user.email", "fixture@example.invalid"])?;
        fixture_git(root, &["config", "commit.gpgsign", "false"])?;
        fixture_git(root, &["config", "core.hooksPath", "no-hooks"])?;
        fs::create_dir(root.join(".ci"))?;
        fs::write(
            root.join(".ci/gate-policy.yaml"),
            include_str!("../../../../.ci/gate-policy.yaml"),
        )?;
        fs::write(root.join("file.rs"), "fn example() {}\n")?;
        fixture_git(root, &["add", "."])?;
        fixture_git(root, &["commit", "-m", "base"])?;
        let base = fixture_git(root, &["rev-parse", "HEAD"])?;
        fixture_git(root, &["update-ref", "refs/remotes/origin/main", &base])?;
        fs::write(root.join("file.rs"), "fn changed() {}\n")?;
        fixture_git(root, &["add", "."])?;
        fixture_git(root, &["commit", "-m", "head"])?;
        let head = fixture_git(root, &["rev-parse", "HEAD"])?;
        Ok((dir, base, head))
    }

    fn config(base: &str, head: &str) -> GateRunnerConfig {
        GateRunnerConfig {
            tier: GateTier::Publication,
            base_ref: Some(base.into()),
            head_ref: Some(head.into()),
            ..GateRunnerConfig::default()
        }
    }

    #[test]
    fn publication_policy_selects_eight_existing_checks_and_retains_deferred_commands() -> Result<()>
    {
        let (dir, base, head) = fixture()?;
        let policy =
            super::super::load_policy_for_inspection(&dir.path().join(".ci/gate-policy.yaml"))?;
        let plan = plan(dir.path(), &policy, &config(&base, &head))?;
        let mut names: Vec<_> = plan.selected.iter().map(|gate| gate.gate.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "conflict_markers_staged",
                "forbidden_machine_paths",
                "from_raw_staged",
                "rustfmt_staged",
                "staged_config_syntax",
                "staged_exec_mode_policy",
                "staged_oversized_or_binary",
                "whitespace_check"
            ]
        );
        assert_eq!(plan.base, base);
        assert_eq!(
            plan.staged_tree_oid,
            Some(fixture_git(dir.path(), &["rev-parse", "HEAD^{tree}"])?)
        );
        for gate in policy
            .gates
            .iter()
            .filter(|gate| gate.tier == "pr_fast" || gate.name == "changie_fragment_staged")
        {
            let deferred = plan
                .skipped
                .iter()
                .find(|row| row.name == gate.name)
                .expect("retained disposition");
            assert!(deferred.reason.starts_with("DEFERRED:"));
            assert!(deferred.reason.contains(&gate.command));
        }
        Ok(())
    }

    #[test]
    fn publication_rejects_dirty_policy_and_changed_checkout() -> Result<()> {
        let (dir, base, head) = fixture()?;
        let config = config(&base, &head);
        fs::write(dir.path().join(".ci/gate-policy.yaml"), "gates: []\n")?;
        assert!(validate_config(dir.path(), &config).is_err());
        fixture_git(dir.path(), &["checkout", "--", ".ci/gate-policy.yaml"])?;
        assert!(validate_config(dir.path(), &config).is_ok());
        fs::write(dir.path().join("rustfmt.toml"), "edition = '2024'\n")?;
        assert!(validate_config(dir.path(), &config).is_err());
        fs::remove_file(dir.path().join("rustfmt.toml"))?;
        fixture_git(dir.path(), &["checkout", "--detach", &base])?;
        assert!(validate_config(dir.path(), &config).is_err());
        Ok(())
    }

    #[test]
    fn publication_rejects_missing_base_self_auto_and_missing_objects() -> Result<()> {
        let (dir, _base, head) = fixture()?;
        let policy =
            super::super::load_policy_for_inspection(&dir.path().join(".ci/gate-policy.yaml"))?;
        let mut missing = config("auto", &head);
        let mut mutable_head = config("auto", &head);
        mutable_head.head_ref = Some("HEAD".into());
        assert!(
            validate_config(dir.path(), &mutable_head).is_err(),
            "mutable heads cannot bind pushed evidence"
        );
        missing.base_ref = None;
        assert!(plan(dir.path(), &policy, &missing).is_err());
        assert!(plan(dir.path(), &policy, &config("does-not-exist", &head)).is_err());
        fixture_git(dir.path(), &["update-ref", "-d", "refs/remotes/origin/main"])?;
        assert!(
            plan(dir.path(), &policy, &config("auto", &head)).is_err(),
            "local main == HEAD is not a new-branch baseline"
        );
        Ok(())
    }

    #[test]
    fn publication_auto_base_rejects_remote_descendant_of_pushed_head() -> Result<()> {
        let (dir, _base, head) = fixture()?;
        let policy =
            super::super::load_policy_for_inspection(&dir.path().join(".ci/gate-policy.yaml"))?;
        fs::write(dir.path().join("later.rs"), "fn later() {}\n")?;
        fixture_git(dir.path(), &["add", "."])?;
        fixture_git(dir.path(), &["commit", "-m", "remote descendant"])?;
        let child = fixture_git(dir.path(), &["rev-parse", "HEAD"])?;
        fixture_git(dir.path(), &["update-ref", "refs/remotes/origin/main", &child])?;
        fixture_git(dir.path(), &["checkout", "--detach", &head])?;
        assert_ne!(child, head);
        assert_eq!(fixture_git(dir.path(), &["merge-base", &child, &head])?, head);
        assert!(
            plan(dir.path(), &policy, &config("auto", &head)).is_err(),
            "a remote descendant cannot make a new-branch publication vacuously pass"
        );
        Ok(())
    }

    #[test]
    fn publication_auto_base_rejects_unrelated_history() -> Result<()> {
        let (dir, _base, _head) = fixture()?;
        let policy =
            super::super::load_policy_for_inspection(&dir.path().join(".ci/gate-policy.yaml"))?;
        fixture_git(dir.path(), &["checkout", "--orphan", "unrelated"])?;
        fixture_git(dir.path(), &["commit", "-m", "unrelated history"])?;
        let head = fixture_git(dir.path(), &["rev-parse", "HEAD"])?;
        assert!(
            plan(dir.path(), &policy, &config("auto", &head)).is_err(),
            "failed merge-base acquisition must never fall back to a guessed baseline"
        );
        Ok(())
    }

    #[test]
    fn publication_rejects_filtered_or_weakened_policy_execution() -> Result<()> {
        let (dir, base, head) = fixture()?;
        let mut policy =
            super::super::load_policy_for_inspection(&dir.path().join(".ci/gate-policy.yaml"))?;
        let mut filtered = config(&base, &head);
        filtered.gate_filter = Some("whitespace_check".into());
        assert!(validate_config(dir.path(), &filtered).is_err());
        policy.gates.iter_mut().find(|gate| is_mechanical_gate(gate)).unwrap().required = false;
        assert!(plan(dir.path(), &policy, &config(&base, &head)).is_err());
        Ok(())
    }
}
