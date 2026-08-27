//! Process-level proof for issue #10194: the PR provenance comparator must describe
//! the base tree actually incorporated into the scanned result, not a recorded
//! event-time base that lags after intervening merges.
//!
//! The fixture builds the adjudicated failure graph directly: `B0 -> B1` carries a
//! base-only action-pin movement on `main`, `H` is a candidate branched from `B0`,
//! and the scanned worktree is their simulated merge `M = merge(H, B1)`. Comparing
//! against the stale `B0` charges the base-only movement to the candidate
//! (`LEGACY_DEBT_NOT_ALLOWED_FOR_CHANGED_PIN`); comparing against
//! `--merge-base main` resolves `B1` at run time and reports zero candidate pin
//! changes. Controls keep genuine candidate-owned pin movements detected.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Real ledger subject so fixture rows mirror production: this SHA carries both a
/// legacy_debt 'v7' family row and the verified release_tag 'v7.0.1' row.
const CHECKOUT_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

#[test]
fn stale_recorded_base_charges_base_only_movement_to_candidate() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::None)?;

    let output = run_cli(
        fixture.root(),
        None,
        Some(fixture.recorded_base.as_str()),
        &[],
        &fixture.receipt_path(),
    )?;

    // This documents the defect mechanism itself: an event-time base that predates
    // the base-only movement keeps charging it, which is exactly why CI must not
    // hand this comparator to pull_request scans anymore.
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("LEGACY_DEBT_NOT_ALLOWED_FOR_CHANGED_PIN"));
    assert!(stderr.contains(".github/workflows/pin-source.yml"));
    assert!(!stderr.contains(".github/workflows/candidate-only.yml"));
    Ok(())
}

#[test]
fn merge_base_comparator_reports_zero_changes_for_innocent_candidate() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::None)?;

    let output = run_cli(fixture.root(), Some("main"), None, &[], &fixture.receipt_path())?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "innocent candidate failed against merge-base comparator:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("0 new/changed"));
    let receipt = read_receipt(&fixture)?;
    assert_eq!(receipt["passed"], Value::Bool(true));
    assert_eq!(receipt["new_or_changed_count"], Value::from(0));
    // The receipt must prove the resolved comparator, not echo the request.
    assert_eq!(receipt["base"], Value::String(fixture.merge_base_commit()?));
    Ok(())
}

#[test]
fn merge_base_comparator_still_counts_candidate_owned_pin_change() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::PromotedByCandidate)?;

    let output = run_cli(fixture.root(), Some("main"), None, &[], &fixture.receipt_path())?;

    // The promoted projection is allowed content-wise, so the scan passes, but the
    // count must move off zero: the repair may not hide real candidate-owned pin
    // changes behind the fresh comparator.
    assert_eq!(
        output.status.code(),
        Some(0),
        "candidate-owned promoted pin failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt = read_receipt(&fixture)?;
    assert_eq!(receipt["passed"], Value::Bool(true));
    assert_eq!(receipt["new_or_changed_count"], Value::from(1));
    Ok(())
}

#[test]
fn merge_base_comparator_hard_errors_on_new_unpinned_ref() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::UnpinnedByCandidate)?;

    let output = run_cli(fixture.root(), Some("main"), None, &[], &fixture.receipt_path())?;

    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MUTABLE_OR_UNSUPPORTED_ACTION_REF"));
    assert!(stderr.contains(".github/workflows/candidate-only.yml"));
    Ok(())
}

#[test]
fn subject_binding_flags_pass_on_verified_merge_ref() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::None)?;

    let output = run_cli(
        fixture.root(),
        Some("main"),
        None,
        &[
            "--expect-merge-of",
            &fixture.candidate_head,
            "--expect-origin",
            "EffortlessMetrics/perl-lsp-swarm",
        ],
        &fixture.receipt_path(),
    )?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "bound scan failed on the verified subject:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt = read_receipt(&fixture)?;
    assert_eq!(receipt["passed"], Value::Bool(true));
    assert_eq!(receipt["new_or_changed_count"], Value::from(0));
    Ok(())
}

#[test]
fn tampered_expected_head_fails_closed() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::None)?;
    let forged = "0000000000000000000000000000000000000009";

    let output = run_cli(
        fixture.root(),
        Some(fixture.main_tip.as_str()),
        None,
        &["--expect-merge-of", forged],
        &fixture.receipt_path(),
    )?;

    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not of the expected candidate"));
    Ok(())
}

#[test]
fn tampered_expected_origin_fails_closed() -> Result<()> {
    let (fixture, _outer) = scenario(PinMovement::None)?;

    let output = run_cli(
        fixture.root(),
        Some("main"),
        None,
        &["--expect-origin", "EffortlessMetrics/perl-lsp-swarm-fork"],
        &fixture.receipt_path(),
    )?;

    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not match expected repository"));
    Ok(())
}

/// What the candidate's own workflow file contributes on top of the base history.
enum PinMovement {
    /// Innocent candidate: touches nothing pin-like.
    None,
    /// Candidate rewrites its own pin to the promoted `# v7.0.1` projection.
    PromotedByCandidate,
    /// Candidate introduces a mutable ref.
    UnpinnedByCandidate,
}

struct Fixture {
    repository: PathBuf,
    receipt: PathBuf,
    /// Event-time base the PR would have recorded before any base-side movement.
    recorded_base: String,
    /// Candidate head SHA: the second parent of the simulated merge ref.
    candidate_head: String,
    /// Current base branch tip: what `origin/<base.ref>` resolves to.
    main_tip: String,
}

impl Fixture {
    fn root(&self) -> &Path {
        &self.repository
    }

    fn receipt_path(&self) -> &Path {
        &self.receipt
    }

    /// What the merge-base comparator resolves while the worktree sits on the
    /// simulated merge result, i.e. the base tree CI should scan against.
    fn merge_base_commit(&self) -> Result<String> {
        self.git(&["merge-base", "main", "HEAD"]).map(|value| value.trim().to_owned())
    }

    fn git(&self, arguments: &[&str]) -> Result<String> {
        git(&self.repository, arguments)
    }
}

/// Builds the full failure graph and leaves the worktree checked out at the
/// simulated merge result `M`, mirroring what `refs/pull/N/merge` holds.
fn scenario(movement: PinMovement) -> Result<(Fixture, tempfile::TempDir)> {
    let outer = tempfile::tempdir()?;
    let repository = outer.path().join("repo");
    std::fs::create_dir_all(repository.join(".github/workflows"))?;
    std::fs::create_dir_all(repository.join(".ci/policies"))?;
    git(&repository, &["init", "--initial-branch", "main"])?;
    git(&repository, &["config", "user.name", "test"])?;
    git(&repository, &["config", "user.email", "test@example.com"])?;
    git(&repository, &["config", "core.autocrlf", "false"])?;
    git(
        &repository,
        &["remote", "add", "origin", "https://github.com/EffortlessMetrics/perl-lsp-swarm.git"],
    )?;
    std::fs::write(
        repository.join(".ci/policies/action-pin-provenance.toml"),
        format!(
            "[[pin]]\naction = 'actions/checkout'\nsha = '{CHECKOUT_SHA}'\nkind = 'legacy_debt'\nvalue = 'v7'\n\n[[pin]]\naction = 'actions/checkout'\nsha = '{CHECKOUT_SHA}'\nkind = 'release_tag'\nvalue = 'v7.0.1'\n"
        ),
    )?;
    // B0: exactly one legacy-debt pin occurrence in the base.
    commit_file(
        &repository,
        ".github/workflows/pin-source.yml",
        &format!(
            "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@{CHECKOUT_SHA} # v7\n"
        ),
        "base pin",
    )?;
    let recorded_base = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

    // H: candidate branched from B0; its own file decides the movement shape.
    git(&repository, &["switch", "--create", "candidate"])?;
    let candidate_line = match movement {
        PinMovement::None => String::new(),
        PinMovement::PromotedByCandidate => {
            format!("      - uses: actions/checkout@{CHECKOUT_SHA} # v7.0.1\n")
        }
        PinMovement::UnpinnedByCandidate => "      - uses: actions/checkout@main\n".to_owned(),
    };
    commit_file(
        &repository,
        ".github/workflows/candidate-only.yml",
        &format!("jobs:\n  docs:\n    steps:\n{candidate_line}"),
        "candidate work",
    )?;

    // B1: base-only pin movement while the candidate is in flight; a second
    // identical occurrence arrives on main exactly as #12248 did.
    git(&repository, &["switch", "main"])?;
    append_file(
        &repository,
        ".github/workflows/pin-source.yml",
        &format!("      - uses: actions/checkout@{CHECKOUT_SHA} # v7\n"),
    )?;
    git(&repository, &["add", "--", ".github/workflows/pin-source.yml"])?;
    git(&repository, &["commit", "-m", "base-side pin arrival"])?;
    let main_tip = git(&repository, &["rev-parse", "main"])?.trim().to_owned();

    // M: GitHub's simulated merge ref. GitHub merges the candidate head INTO
    // the base branch, so parent1 is the incorporated base tip and parent2 is
    // the candidate head. The result hangs off a detached HEAD while the base
    // branch ref stays behind, mirroring refs/pull/N/merge vs refs/heads/main.
    let candidate_head = git(&repository, &["rev-parse", "candidate"])?.trim().to_owned();
    git(&repository, &["checkout", "--detach"])?;
    git(&repository, &["merge", "--no-ff", "-m", "simulated pull request merge", "candidate"])?;

    Ok((
        Fixture {
            repository,
            receipt: outer.path().join("action-pin-provenance.json"),
            recorded_base,
            candidate_head,
            main_tip,
        },
        outer,
    ))
}

fn commit_file(repository: &Path, path: &str, contents: &str, message: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(repository.join(parent))
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(repository.join(path), contents).with_context(|| format!("writing {path}"))?;
    git(repository, &["add", "--", path])?;
    git(repository, &["commit", "-m", message])?;
    Ok(())
}

fn append_file(repository: &Path, path: &str, contents: &str) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(repository.join(path))
        .with_context(|| format!("opening {path}"))?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

fn read_receipt(fixture: &Fixture) -> Result<Value> {
    let text =
        std::fs::read_to_string(&fixture.receipt).context("reading receipt written by the CLI")?;
    serde_json::from_str(&text).context("parsing receipt JSON")
}

fn run_cli(
    repository: &Path,
    merge_base: Option<&str>,
    base: Option<&str>,
    extra: &[&str],
    receipt: &Path,
) -> Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_action-pin-provenance"));
    command.arg("--root").arg(repository);
    if let Some(spec) = merge_base {
        command.arg("--merge-base").arg(spec);
    }
    if let Some(recorded) = base {
        command.arg("--base").arg(recorded);
    }
    command.arg("--receipt").arg(receipt);
    command.args(extra);
    command.output().context("failed to execute action-pin-provenance CLI")
}

fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git").args(arguments).current_dir(repository).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("git command returned non-UTF-8 output")
}
