use anyhow::{Result, bail, ensure};
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::{TempDir, tempdir};
use xtask::worktree_forensic_recovery::{
    ActiveUseEvidence, ActiveUseProbe, RecoveryClassification, RecoveryPlan, TraversalLimits,
    inspect, inspect_with_limits_and_probe,
};

struct Fixture {
    _temporary: TempDir,
    repository: PathBuf,
    candidate: PathBuf,
    unselected: PathBuf,
}

impl Fixture {
    fn create() -> Result<Self> {
        let temporary = tempdir()?;
        let repository = temporary.path().join("repository");
        run_git(temporary.path(), &["init", "-q", "-b", "main", path_text(&repository)?])?;
        run_git(&repository, &["config", "user.name", "Forensic Fixture"])?;
        run_git(&repository, &["config", "user.email", "forensic@example.invalid"])?;
        fs::write(repository.join("seed.pl"), "use strict;\n")?;
        run_git(&repository, &["add", "seed.pl"])?;
        run_git(&repository, &["commit", "-q", "-m", "seed"])?;
        let candidate = repository.join(".claude/worktrees/lost");
        fs::create_dir_all(&candidate)?;
        let unselected = repository.join(".claude/worktrees/unselected");
        fs::create_dir_all(&unselected)?;
        fs::write(unselected.join("must-not-be-scanned.pl"), "unique\n")?;
        let admin = repository.join(".git/worktrees/lost");
        fs::write(candidate.join(".git"), format!("gitdir: {}\n", admin.display()))?;
        fs::write(candidate.join("unique.pl"), "our $unique = 1;\n")?;
        Ok(Self { _temporary: temporary, repository, candidate, unselected })
    }

    fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            worktrees: git_output(&self.repository, &["worktree", "list", "--porcelain", "-z"])?
                .stdout,
            refs: git_output(&self.repository, &["show-ref"])?.stdout,
            pointer: fs::read(self.candidate.join(".git"))?,
            candidate_files: file_snapshot(&self.candidate)?,
            unselected_files: file_snapshot(&self.unselected)?,
        })
    }
}

struct LinkedFixture {
    _temporary: TempDir,
    repository: PathBuf,
    candidate: PathBuf,
    administrative: PathBuf,
}

impl LinkedFixture {
    fn create(detached: bool) -> Result<Self> {
        let temporary = tempdir()?;
        let repository = temporary.path().join("repository");
        run_git(temporary.path(), &["init", "-q", "-b", "main", path_text(&repository)?])?;
        run_git(&repository, &["config", "user.name", "Forensic Fixture"])?;
        run_git(&repository, &["config", "user.email", "forensic@example.invalid"])?;
        run_git(&repository, &["config", "extensions.worktreeConfig", "true"])?;
        fs::write(repository.join("seed.pl"), "use strict;\n")?;
        run_git(&repository, &["add", "seed.pl"])?;
        run_git(&repository, &["commit", "-q", "-m", "seed"])?;

        let candidate = repository.join(".claude/worktrees/linked");
        fs::create_dir_all(candidate.parent().ok_or_else(|| {
            anyhow::anyhow!("candidate path has no parent: {}", candidate.display())
        })?)?;
        let candidate_text = path_text(&candidate)?;
        if detached {
            run_git(&repository, &["worktree", "add", "-q", "--detach", candidate_text])?;
        } else {
            run_git(
                &repository,
                &["worktree", "add", "-q", "-b", "forensic-fixture", candidate_text],
            )?;
        }
        let pointer = fs::read_to_string(candidate.join(".git"))?;
        let administrative_text = pointer
            .strip_prefix("gitdir:")
            .ok_or_else(|| anyhow::anyhow!("fixture pointer lacks gitdir record"))?
            .trim();
        let administrative = PathBuf::from(administrative_text);
        if !administrative.join("config.worktree").exists() {
            fs::write(administrative.join("config.worktree"), "[core]\n")?;
        }
        Ok(Self { _temporary: temporary, repository, candidate, administrative })
    }

    fn snapshot(&self) -> Result<LinkedSnapshot> {
        Ok(LinkedSnapshot {
            worktrees: git_output(&self.repository, &["worktree", "list", "--porcelain", "-z"])?
                .stdout,
            refs: git_output(&self.repository, &["show-ref"])?.stdout,
            head_object: git_output(&self.repository, &["cat-file", "-p", "HEAD"])?.stdout,
            object_count: git_output(&self.repository, &["count-objects", "-v"])?.stdout,
            pointer: fs::read(self.candidate.join(".git"))?,
            administrative_gitdir: fs::read(self.administrative.join("gitdir"))?,
            administrative_commondir: fs::read(self.administrative.join("commondir"))?,
            administrative_head: fs::read(self.administrative.join("HEAD"))?,
            administrative_index: fs::read(self.administrative.join("index"))?,
            administrative_config: fs::read(self.administrative.join("config.worktree"))?,
            common_config: fs::read(self.repository.join(".git/config"))?,
            administrative_files: file_snapshot(&self.administrative)?,
            candidate_files: file_snapshot(&self.candidate)?,
        })
    }
}

struct InactiveFixtureProbe;

impl ActiveUseProbe for InactiveFixtureProbe {
    fn observe(&self, _candidate: &Path, _administrative_path: &Path) -> ActiveUseEvidence {
        ActiveUseEvidence::Inactive
    }
}

fn forensic_result<T>(result: color_eyre::eyre::Result<T>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("{error:#}"))
}

fn assert_platform_plan(
    plan: &RecoveryPlan,
    unix_classification: RecoveryClassification,
    context: &str,
) -> Result<()> {
    #[cfg(windows)]
    {
        if plan.classification != unix_classification {
            ensure!(
                plan.classification == RecoveryClassification::ForensicInstrumentUnavailable,
                "{context} had an unexpected Windows classification: {plan:?}"
            );
            ensure!(
                plan.reasons.iter().any(|reason| {
                    reason.contains("stable Windows file identity is unavailable")
                }),
                "{context} lacked the Windows identity refusal reason: {plan:?}"
            );
        }
    }
    #[cfg(not(windows))]
    ensure!(
        plan.classification == unix_classification,
        "{context} had the wrong Unix classification: {plan:?}"
    );
    Ok(())
}

fn assert_platform_json(plan: &Value, unix_classification: &str, context: &str) -> Result<()> {
    #[cfg(windows)]
    {
        if plan["classification"] != unix_classification {
            ensure!(
                plan["classification"] == "FORENSIC_INSTRUMENT_UNAVAILABLE",
                "{context} had an unexpected Windows classification: {plan}"
            );
            ensure!(
                plan["reasons"].to_string().contains("stable Windows file identity is unavailable"),
                "{context} lacked the Windows identity refusal reason: {plan}"
            );
        }
    }
    #[cfg(not(windows))]
    ensure!(
        plan["classification"] == unix_classification,
        "{context} had the wrong Unix classification: {plan}"
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    worktrees: Vec<u8>,
    refs: Vec<u8>,
    pointer: Vec<u8>,
    candidate_files: BTreeMap<String, String>,
    unselected_files: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
struct LinkedSnapshot {
    worktrees: Vec<u8>,
    refs: Vec<u8>,
    head_object: Vec<u8>,
    object_count: Vec<u8>,
    pointer: Vec<u8>,
    administrative_gitdir: Vec<u8>,
    administrative_commondir: Vec<u8>,
    administrative_head: Vec<u8>,
    administrative_index: Vec<u8>,
    administrative_config: Vec<u8>,
    common_config: Vec<u8>,
    administrative_files: BTreeMap<String, String>,
    candidate_files: BTreeMap<String, String>,
}

#[test]
fn explicit_candidate_is_classified_without_broad_discovery_or_writes() -> Result<()> {
    let fixture = Fixture::create()?;
    let before = fixture.snapshot()?;
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(&fixture.repository)
        .arg("--candidate")
        .arg(&fixture.candidate)
        .arg("--json")
        .output()?;
    ensure!(
        output.status.code() == Some(2),
        "expected unsafe evidence exit 2, got {:?}",
        output.status.code()
    );
    let plan: Value = serde_json::from_slice(&output.stdout)?;
    assert_platform_json(&plan, "DIRTY_OR_INDEX_UNKNOWN", "damaged explicit candidate")?;
    #[cfg(windows)]
    if plan["classification"] == "DIRTY_OR_INDEX_UNKNOWN" {
        ensure!(
            plan["evidence"]["source_manifest"]["complete"] == true,
            "valid Windows identity did not produce a complete manifest: {plan}"
        );
        ensure!(
            plan["evidence"]["source_manifest"]["files"].to_string().contains("unique.pl"),
            "valid Windows identity did not inventory the selected candidate: {plan}"
        );
    }
    #[cfg(not(windows))]
    ensure!(
        plan["evidence"]["source_manifest"]["files"].to_string().contains("unique.pl"),
        "selected candidate was not inventoried"
    );
    ensure!(
        !plan.to_string().contains("must-not-be-scanned.pl"),
        "unselected candidate was discovered"
    );
    ensure!(
        plan["proposed_actions"].as_array().is_some_and(|values| values.is_empty()),
        "read-only slice proposed a mutation"
    );
    ensure!(
        plan["plan_digest"].as_str().is_some_and(|value| !value.is_empty()),
        "plan digest missing"
    );
    ensure!(fixture.snapshot()? == before, "forensic inspection mutated fixture state");
    Ok(())
}

#[test]
fn missing_explicit_inputs_are_rejected_by_cli() -> Result<()> {
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(".")
        .output()?;
    ensure!(output.status.code() == Some(2), "missing candidate was not a usage error");
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("--candidate"),
        "usage did not identify candidate input"
    );
    Ok(())
}

#[test]
fn top_level_plan_route_requires_explicit_inputs_and_is_read_only() -> Result<()> {
    let fixture = Fixture::create()?;
    let before = fixture.snapshot()?;
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(&fixture.repository)
        .arg("--candidate")
        .arg(&fixture.candidate)
        .arg("--json")
        .output()?;
    ensure!(output.status.code() == Some(2), "unexpected route exit: {:?}", output.status.code());
    let plan: Value = serde_json::from_slice(&output.stdout)?;
    assert_platform_json(&plan, "DIRTY_OR_INDEX_UNKNOWN", "top-level damaged candidate")?;
    ensure!(fixture.snapshot()? == before, "top-level plan route mutated fixture state");

    let missing = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(&fixture.repository)
        .output()?;
    ensure!(missing.status.code() == Some(2), "missing candidate was accepted");
    ensure!(
        String::from_utf8_lossy(&missing.stderr).contains("--candidate"),
        "missing candidate usage did not identify --candidate"
    );

    let linked = LinkedFixture::create(false)?;
    let linked_before = linked.snapshot()?;
    let production = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(&linked.repository)
        .arg("--candidate")
        .arg(&linked.candidate)
        .arg("--json")
        .output()?;
    ensure!(
        production.status.code() == Some(2),
        "production observer unexpectedly authorized clean evidence"
    );
    let production_plan: Value = serde_json::from_slice(&production.stdout)?;
    assert_platform_json(&production_plan, "NOT_PROVEN", "production observer")?;
    ensure!(linked.snapshot()? == linked_before, "top-level route mutated linked fixture state");
    Ok(())
}

#[cfg(any(windows, unix))]
#[test]
fn configured_fsmonitor_and_hooks_are_suppressed_during_observation() -> Result<()> {
    let fixture = LinkedFixture::create(false)?;
    let marker = fixture._temporary.path().join("fsmonitor-marker");
    let hook_marker = fixture._temporary.path().join("hook-marker");
    let hook = fixture._temporary.path().join("fsmonitor-spy.sh");
    let marker_text = marker.display().to_string().replace('\\', "/");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nprintf 'argv:' >> \"{marker_text}\"\nfor arg in \"$@\"; do printf ' <%s>' \"$arg\" >> \"{marker_text}\"; done\nprintf '\\n' >> \"{marker_text}\"\necho 0\n"
        ),
    )?;
    let hook_command = format!("sh {}", path_text(&hook)?.replace('\\', "/"));
    run_git(&fixture.candidate, &["config", "core.fsmonitor", hook_command.as_str()])?;

    let hooks_dir = fixture._temporary.path().join("hooks");
    fs::create_dir_all(&hooks_dir)?;
    let hook_marker_text = hook_marker.display().to_string().replace('\\', "/");
    fs::write(
        hooks_dir.join("pre-commit"),
        format!("#!/bin/sh\necho invoked >> \"{hook_marker_text}\"\n"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let hook_path = hooks_dir.join("pre-commit");
        let mut permissions = fs::metadata(&hook_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(hook_path, permissions)?;
    }
    let hooks_path = path_text(&hooks_dir)?.replace('\\', "/");
    run_git(&fixture.candidate, &["config", "core.hooksPath", hooks_path.as_str()])?;

    let configured_status = Command::new("git")
        .current_dir(&fixture.candidate)
        .args(["status", "--porcelain=v1"])
        .stdin(Stdio::null())
        .output()?;
    ensure!(
        marker.exists(),
        "configured fsmonitor spy was not armed: status={configured_status:?}"
    );
    ensure!(
        fs::read_to_string(&marker)?.contains("argv:"),
        "configured fsmonitor spy did not record argv"
    );
    fs::remove_file(&marker)?;
    run_git(&fixture.candidate, &["commit", "--allow-empty", "-m", "arm hook spy"])?;
    ensure!(hook_marker.exists(), "configured pre-commit hook spy was not armed");
    fs::remove_file(&hook_marker)?;
    if marker.exists() {
        fs::remove_file(&marker)?;
    }
    ensure!(!marker.exists(), "fsmonitor setup marker was not reset before observation");
    let before = fixture.snapshot()?;

    let _ = forensic_result(inspect_with_limits_and_probe(
        &fixture.repository,
        &fixture.candidate,
        TraversalLimits::default(),
        &InactiveFixtureProbe,
    ))?;
    ensure!(!marker.exists(), "forensic observation allowed fsmonitor mutation");
    ensure!(!hook_marker.exists(), "forensic observation allowed hook mutation");
    ensure!(fixture.snapshot()? == before, "forensic observation mutated repository state");
    Ok(())
}

#[test]
fn real_observer_reaches_clean_salvage_and_unknown_controls() -> Result<()> {
    let fixture = LinkedFixture::create(false)?;
    let clean_status = git_output(
        &fixture.candidate,
        &["status", "--porcelain=v1", "--ignored=matching", "--untracked-files=all"],
    )?;
    ensure!(
        clean_status.stdout.is_empty(),
        "linked clean fixture was not clean: {}",
        String::from_utf8_lossy(&clean_status.stdout)
    );
    let inactive = InactiveFixtureProbe;
    let clean = forensic_result(inspect_with_limits_and_probe(
        &fixture.repository,
        &fixture.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(&clean, RecoveryClassification::CleanReconstructable, "clean fixture")?;

    fs::write(fixture.candidate.join("untracked.pl"), "our $unique = 1;\n")?;
    let salvage = forensic_result(inspect_with_limits_and_probe(
        &fixture.repository,
        &fixture.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(&salvage, RecoveryClassification::SalvageRequired, "dirty fixture")?;

    let unavailable_fixture = LinkedFixture::create(false)?;
    let unavailable =
        forensic_result(inspect(&unavailable_fixture.repository, &unavailable_fixture.candidate))?;
    assert_platform_plan(&unavailable, RecoveryClassification::NotProven, "unavailable observer")?;
    Ok(())
}

#[test]
fn ignored_source_cannot_produce_clean_classification() -> Result<()> {
    let fixture = LinkedFixture::create(false)?;
    fs::create_dir(fixture.candidate.join("vendor"))?;
    fs::write(fixture.candidate.join(".gitignore"), "vendor/\n")?;
    run_git(&fixture.candidate, &["add", ".gitignore"])?;
    run_git(&fixture.candidate, &["commit", "-q", "-m", "ignore source fixture"])?;
    fs::write(fixture.candidate.join("vendor").join("ignored-source.pl"), "our $ignored = 1;\n")?;

    let plan = forensic_result(inspect_with_limits_and_probe(
        &fixture.repository,
        &fixture.candidate,
        TraversalLimits::default(),
        &InactiveFixtureProbe,
    ))?;
    assert_platform_plan(&plan, RecoveryClassification::SalvageRequired, "ignored source")?;
    #[cfg(not(windows))]
    ensure!(
        matches!(
            plan.evidence.unique_work,
            xtask::worktree_forensic_recovery::UniqueWorkEvidence::IgnoredSource { .. }
        ),
        "ignored source did not produce explicit unique-work evidence: {plan:?}"
    );
    Ok(())
}

#[test]
fn real_observer_controls_detached_lock_and_identity_absence() -> Result<()> {
    let inactive = InactiveFixtureProbe;
    let detached = LinkedFixture::create(true)?;
    let detached_plan = forensic_result(inspect_with_limits_and_probe(
        &detached.repository,
        &detached.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(
        &detached_plan,
        RecoveryClassification::DetachedOrHeadUnknown,
        "detached fixture",
    )?;

    let locked = LinkedFixture::create(false)?;
    fs::write(locked.administrative.join("index.lock"), "lock\n")?;
    let locked_plan = forensic_result(inspect_with_limits_and_probe(
        &locked.repository,
        &locked.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(&locked_plan, RecoveryClassification::ActiveOrLocked, "lock fixture")?;

    let missing_identity = LinkedFixture::create(false)?;
    fs::remove_file(missing_identity.administrative.join("commondir"))?;
    let identity_plan = forensic_result(inspect_with_limits_and_probe(
        &missing_identity.repository,
        &missing_identity.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    #[cfg(windows)]
    if identity_plan.classification == RecoveryClassification::ForensicInstrumentUnavailable {
        ensure!(
            identity_plan
                .reasons
                .iter()
                .any(|reason| reason.contains("stable Windows file identity is unavailable")),
            "missing administrative commondir had an unexplained instrumentation refusal: {identity_plan:?}"
        );
    } else {
        ensure!(
            identity_plan.classification == RecoveryClassification::IdentityConflict,
            "missing administrative commondir had an unexpected Windows classification: {identity_plan:?}"
        );
        ensure!(
            identity_plan
                .reasons
                .iter()
                .any(|reason| reason.starts_with("CANDIDATE_git-dir_IDENTITY_UNAVAILABLE:")),
            "missing administrative commondir lacked its exact candidate identity refusal reason: {identity_plan:?}"
        );
    }
    #[cfg(not(windows))]
    {
        assert_platform_plan(
            &identity_plan,
            RecoveryClassification::DirtyOrIndexUnknown,
            "missing administrative commondir",
        )?;
        ensure!(
            identity_plan.reasons.iter().any(|reason| reason == "ADMIN_COMMONDIR_UNKNOWN"),
            "missing administrative commondir lacked its exact refusal reason: {identity_plan:?}"
        );
    }

    let missing_gitdir = LinkedFixture::create(false)?;
    fs::remove_file(missing_gitdir.administrative.join("gitdir"))?;
    let missing_gitdir_plan = forensic_result(inspect_with_limits_and_probe(
        &missing_gitdir.repository,
        &missing_gitdir.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(
        &missing_gitdir_plan,
        RecoveryClassification::DirtyOrIndexUnknown,
        "missing administrative gitdir",
    )?;
    #[cfg(not(windows))]
    ensure!(
        missing_gitdir_plan.reasons.iter().any(|reason| reason == "ADMIN_GITDIR_UNKNOWN"),
        "missing administrative gitdir lacked its exact refusal reason: {missing_gitdir_plan:?}"
    );

    let mismatched_gitdir = LinkedFixture::create(false)?;
    fs::write(
        mismatched_gitdir.administrative.join("gitdir"),
        format!("{}\n", path_text(&mismatched_gitdir.repository)?),
    )?;
    let mismatched_gitdir_plan = forensic_result(inspect_with_limits_and_probe(
        &mismatched_gitdir.repository,
        &mismatched_gitdir.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    assert_platform_plan(
        &mismatched_gitdir_plan,
        RecoveryClassification::IdentityConflict,
        "mismatched administrative gitdir",
    )?;
    #[cfg(not(windows))]
    ensure!(
        mismatched_gitdir_plan
            .reasons
            .iter()
            .any(|reason| reason == "ADMIN_GITDIR_CANDIDATE_IDENTITY_CONFLICT"),
        "mismatched administrative gitdir lacked its exact conflict reason: {mismatched_gitdir_plan:?}"
    );

    let corrupt_identity = LinkedFixture::create(false)?;
    fs::write(
        corrupt_identity.administrative.join("commondir"),
        format!("{}\n", path_text(&corrupt_identity.repository)?),
    )?;
    let corrupt_plan = forensic_result(inspect_with_limits_and_probe(
        &corrupt_identity.repository,
        &corrupt_identity.candidate,
        TraversalLimits::default(),
        &inactive,
    ))?;
    #[cfg(windows)]
    assert_platform_plan(
        &corrupt_plan,
        RecoveryClassification::IdentityConflict,
        "corrupt administrative commondir",
    )?;
    #[cfg(not(windows))]
    {
        assert_platform_plan(
            &corrupt_plan,
            RecoveryClassification::IdentityConflict,
            "corrupt administrative commondir",
        )?;
        ensure!(
            corrupt_plan.reasons.iter().any(|reason| reason == "ADMIN_COMMONDIR_IDENTITY_CONFLICT"),
            "corrupt administrative commondir lacked its exact refusal reason: {corrupt_plan:?}"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_candidate_is_not_followed() -> Result<()> {
    let fixture = Fixture::create()?;
    let link = fixture.repository.join(".claude/worktrees/link");
    std::os::unix::fs::symlink(&fixture.candidate, &link)?;
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("plan")
        .arg("--repository")
        .arg(&fixture.repository)
        .arg("--candidate")
        .arg(&link)
        .arg("--json")
        .output()?;
    ensure!(output.status.code() == Some(2), "symlink candidate did not fail closed");
    let plan: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        plan["classification"] == "FORENSIC_INSTRUMENT_UNAVAILABLE",
        "symlink classification was not instrument-unavailable: {plan}"
    );
    Ok(())
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let output = git_output(root, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(Into::into)
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| anyhow::anyhow!("non-UTF-8 fixture path: {}", path.display()))
}

fn file_snapshot(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut snapshot = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            snapshot.insert(relative_key(root, &path)?, String::from("symlink"));
            continue;
        }
        if metadata.is_dir() {
            let mut children = fs::read_dir(&path)?.collect::<std::io::Result<Vec<_>>>()?;
            children.sort_by_key(|entry| entry.file_name());
            for child in children.into_iter().rev() {
                stack.push(child.path());
            }
            continue;
        }
        if metadata.is_file() {
            snapshot.insert(relative_key(root, &path)?, digest(&fs::read(&path)?));
        }
    }
    Ok(snapshot)
}

fn relative_key(root: &Path, path: &Path) -> Result<String> {
    Ok(path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
