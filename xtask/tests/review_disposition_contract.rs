//! Contract tests for truthful review-thread disposition transitions (#13342).

use std::error::Error;
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Output};

fn project_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn disposition_script(root: &Path) -> PathBuf {
    root.join("scripts/reviews/disposition")
}

#[test]
fn disposition_vocabulary_separates_terminal_and_live_findings() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let disposition = fs::read_to_string(disposition_script(&root))?;
    let threads = fs::read_to_string(root.join("scripts/reviews/threads"))?;

    for terminal in ["fixed", "refuted", "superseded", "post-merge-follow-up"] {
        assert!(
            disposition.contains(&format!("{terminal})")),
            "terminal disposition class missing: {terminal}"
        );
    }
    for non_terminal in ["current-blocker", "blocked-by-prerequisite", "not-proven"] {
        assert!(
            disposition.contains(&format!("{non_terminal})")),
            "non-terminal disposition class missing: {non_terminal}"
        );
    }

    assert!(
        disposition.contains("class=follow-up is ambiguous"),
        "the legacy ambiguous follow-up class must fail closed"
    );
    assert!(
        disposition.contains("thread_transition:$transition"),
        "markers must retain the intended thread transition"
    );
    assert!(
        threads.contains("Resolving classes:") && threads.contains("Keep-open classes:"),
        "thread enumeration must teach the terminal/non-terminal split"
    );
    assert!(
        threads.contains("The legacy class 'follow-up' is rejected"),
        "agent-facing guidance must not retain the ambiguous class"
    );

    Ok(())
}

#[cfg(unix)]
fn run_dry(root: &Path, class: &str, extra: &[&str]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new("bash");
    command
        .arg(disposition_script(root))
        .args([
            "--pr",
            "42",
            "--thread",
            "PRRT_test",
            "--class",
            class,
            "--reply",
            "Disposition proof",
            "--dry-run",
        ])
        .args(extra);
    Ok(command.output()?)
}

#[cfg(unix)]
fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(unix)]
#[test]
fn dry_run_requires_explicit_follow_up_semantics() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;

    let ambiguous = run_dry(&root, "follow-up", &["--issue", "13342"])?;
    assert!(!ambiguous.status.success());
    assert!(output_text(&ambiguous).contains("class=follow-up is ambiguous"));

    let additive = run_dry(
        &root,
        "post-merge-follow-up",
        &[
            "--issue",
            "13342",
            "--argument",
            "The current claim is already satisfied; this work is additive.",
        ],
    )?;
    assert!(
        additive.status.success(),
        "post-merge follow-up dry run failed: {}",
        output_text(&additive)
    );
    let additive_text = output_text(&additive);
    assert!(additive_text.contains(r#""thread_transition":"resolve""#));
    assert!(additive_text.contains("resolveReviewThread(PRRT_test)"));

    let prerequisite = run_dry(
        &root,
        "blocked-by-prerequisite",
        &[
            "--issue",
            "13342",
            "--argument",
            "The defect remains on this candidate until the prerequisite lands.",
        ],
    )?;
    assert!(
        prerequisite.status.success(),
        "prerequisite blocker dry run failed: {}",
        output_text(&prerequisite)
    );
    let prerequisite_text = output_text(&prerequisite);
    assert!(prerequisite_text.contains(r#""thread_transition":"keep_open""#));
    assert!(prerequisite_text.contains("keep thread unresolved"));
    assert!(!prerequisite_text.contains("--- then: resolveReviewThread"));

    let not_proven = run_dry(
        &root,
        "not-proven",
        &["--argument", "Current evidence is incomplete."],
    )?;
    assert!(not_proven.status.success());
    assert!(output_text(&not_proven).contains(r#""thread_transition":"keep_open""#));

    Ok(())
}

#[cfg(unix)]
fn write_gh_stub(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("bin");
    fs::create_dir_all(&bin)?;
    let gh = bin.join("gh");
    fs::write(
        &gh,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_STUB_LOG"

case "$*" in
  *"query(\$threadId: ID!)"*)
    printf '{"data":{"node":{"isResolved":%s,"comments":{"nodes":[]}}}}\n' "$GH_STUB_RESOLVED"
    ;;
  *"addPullRequestReviewThreadReply"*)
    printf '%s\n' '{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"C1"}}}}'
    ;;
  *"unresolveReviewThread"*)
    printf '%s\n' '{"data":{"unresolveReviewThread":{"thread":{"id":"T1","isResolved":false}}}}'
    ;;
  *"resolveReviewThread"*)
    printf '%s\n' '{"data":{"resolveReviewThread":{"thread":{"id":"T1","isResolved":true}}}}'
    ;;
  *)
    printf 'unexpected gh invocation: %s\n' "$*" >&2
    exit 1
    ;;
esac
"#,
    )?;
    let mut permissions = fs::metadata(&gh)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gh, permissions)?;
    Ok(bin)
}

#[cfg(unix)]
fn run_live(
    root: &Path,
    class: &str,
    extra: &[&str],
    initially_resolved: bool,
) -> Result<(Output, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let stub_bin = write_gh_stub(temp.path())?;
    let log = temp.path().join("gh.log");

    let mut path = OsString::from(stub_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").ok_or("PATH is unavailable")?);

    let mut command = Command::new("bash");
    command
        .arg(disposition_script(root))
        .args([
            "--pr",
            "42",
            "--thread",
            "PRRT_test",
            "--class",
            class,
            "--reply",
            "Disposition proof",
            "--repo",
            "owner/repo",
            "--head",
            "0123456789012345678901234567890123456789",
            "--by",
            "reviewer",
        ])
        .args(extra)
        .env("PATH", path)
        .env("GH_STUB_LOG", &log)
        .env(
            "GH_STUB_RESOLVED",
            if initially_resolved { "true" } else { "false" },
        );

    let output = command.output()?;
    let calls = fs::read_to_string(log)?;
    Ok((output, calls))
}

#[cfg(unix)]
#[test]
fn terminal_dispositions_resolve_and_live_blockers_do_not() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;

    let (fixed, fixed_calls) =
        run_live(&root, "fixed", &["--commit", "abc1234"], false)?;
    assert!(
        fixed.status.success(),
        "fixed disposition failed: {}",
        output_text(&fixed)
    );
    assert!(fixed_calls.contains("addPullRequestReviewThreadReply"));
    assert!(
        fixed_calls
            .lines()
            .any(|line| line.contains(" resolveReviewThread"))
    );
    assert!(!fixed_calls.contains("unresolveReviewThread"));

    let (additive, additive_calls) = run_live(
        &root,
        "post-merge-follow-up",
        &[
            "--issue",
            "13342",
            "--argument",
            "The current claim is satisfied; this is additive.",
        ],
        false,
    )?;
    assert!(
        additive.status.success(),
        "post-merge follow-up failed: {}",
        output_text(&additive)
    );
    assert!(
        additive_calls
            .lines()
            .any(|line| line.contains(" resolveReviewThread"))
    );

    let (prerequisite, prerequisite_calls) = run_live(
        &root,
        "blocked-by-prerequisite",
        &[
            "--issue",
            "13342",
            "--argument",
            "The defect remains until the prerequisite lands.",
        ],
        true,
    )?;
    assert!(
        prerequisite.status.success(),
        "prerequisite disposition failed: {}",
        output_text(&prerequisite)
    );
    assert!(prerequisite_calls.contains("addPullRequestReviewThreadReply"));
    assert!(prerequisite_calls.contains("unresolveReviewThread"));
    assert!(
        !prerequisite_calls
            .lines()
            .any(|line| line.contains(" resolveReviewThread"))
    );
    assert!(output_text(&prerequisite).contains("reopened thread"));

    let (current, current_calls) = run_live(
        &root,
        "current-blocker",
        &["--argument", "The candidate still contains the defect."],
        false,
    )?;
    assert!(
        current.status.success(),
        "current blocker disposition failed: {}",
        output_text(&current)
    );
    assert!(current_calls.contains("addPullRequestReviewThreadReply"));
    assert!(!current_calls.contains("unresolveReviewThread"));
    assert!(
        !current_calls
            .lines()
            .any(|line| line.contains(" resolveReviewThread"))
    );
    assert!(output_text(&current).contains("kept thread PRRT_test unresolved"));

    Ok(())
}
