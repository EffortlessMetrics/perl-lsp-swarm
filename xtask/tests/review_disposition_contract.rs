//! Contract tests for truthful review-thread disposition transitions (#13342).

use std::error::Error;
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Output};

use serde_json::Value;

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

    let not_proven =
        run_dry(&root, "not-proven", &["--argument", "Current evidence is incomplete."])?;
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
    if [[ -n "${GH_STUB_MARKER:-}" ]]; then
      jq -cn --argjson resolved "$GH_STUB_RESOLVED" --arg marker "$GH_STUB_MARKER" \
        '{data:{node:{isResolved:$resolved,comments:{nodes:[{body:("prior note\n<!-- disposition:v1 " + $marker + " -->")}]}}}}'
    else
      printf '{"data":{"node":{"isResolved":%s,"comments":{"nodes":[]}}}}\n' "$GH_STUB_RESOLVED"
    fi
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
    preseed_marker: Option<&str>,
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
        .env("GH_STUB_RESOLVED", if initially_resolved { "true" } else { "false" });
    if let Some(marker) = preseed_marker {
        command.env("GH_STUB_MARKER", marker);
    }

    let output = command.output()?;
    let calls = fs::read_to_string(log)?;
    Ok((output, calls))
}

#[cfg(unix)]
#[test]
fn terminal_dispositions_resolve_and_live_blockers_do_not() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;

    let (fixed, fixed_calls) = run_live(&root, "fixed", &["--commit", "abc1234"], false, None)?;
    assert!(fixed.status.success(), "fixed disposition failed: {}", output_text(&fixed));
    assert!(fixed_calls.contains("addPullRequestReviewThreadReply"));
    assert!(fixed_calls.lines().any(|line| line.contains(" resolveReviewThread")));
    assert!(!fixed_calls.contains("unresolveReviewThread"));

    let (additive, additive_calls) = run_live(
        &root,
        "post-merge-follow-up",
        &["--issue", "13342", "--argument", "The current claim is satisfied; this is additive."],
        false,
        None,
    )?;
    assert!(additive.status.success(), "post-merge follow-up failed: {}", output_text(&additive));
    assert!(additive_calls.lines().any(|line| line.contains(" resolveReviewThread")));

    let (prerequisite, prerequisite_calls) = run_live(
        &root,
        "blocked-by-prerequisite",
        &["--issue", "13342", "--argument", "The defect remains until the prerequisite lands."],
        true,
        None,
    )?;
    assert!(
        prerequisite.status.success(),
        "prerequisite disposition failed: {}",
        output_text(&prerequisite)
    );
    let reopen_position = prerequisite_calls
        .find("unresolveReviewThread")
        .ok_or("prerequisite blocker did not reopen the thread")?;
    let reply_position = prerequisite_calls
        .find("addPullRequestReviewThreadReply")
        .ok_or("prerequisite blocker did not post its disposition")?;
    assert!(
        reopen_position < reply_position,
        "the thread must reopen before evidence posting can fail"
    );
    assert!(!prerequisite_calls.lines().any(|line| line.contains(" resolveReviewThread")));
    assert!(output_text(&prerequisite).contains("reopened thread"));

    // A live blocker on an unresolved thread still enforces the final state
    // after posting, closing the window where a concurrent resolve would hide
    // the blocker while the command reports success.
    let (current, current_calls) = run_live(
        &root,
        "current-blocker",
        &["--argument", "The candidate still contains the defect."],
        false,
        None,
    )?;
    assert!(
        current.status.success(),
        "current blocker disposition failed: {}",
        output_text(&current)
    );
    let current_reply = current_calls
        .find("addPullRequestReviewThreadReply")
        .ok_or("current blocker did not post its disposition")?;
    let current_enforce = current_calls
        .rfind("unresolveReviewThread")
        .ok_or("current blocker did not enforce the final unresolved state")?;
    assert!(
        current_reply < current_enforce,
        "the unresolved state must be enforced after the reply is posted"
    );
    assert!(!current_calls.lines().any(|line| line.contains(" resolveReviewThread")));
    assert!(output_text(&current).contains("kept thread PRRT_test unresolved"));

    // not-proven is non-terminal: it must never resolve the thread.
    let (not_proven, not_proven_calls) = run_live(
        &root,
        "not-proven",
        &["--argument", "Current evidence is incomplete."],
        false,
        None,
    )?;
    assert!(
        not_proven.status.success(),
        "not-proven disposition failed: {}",
        output_text(&not_proven)
    );
    assert!(!not_proven_calls.lines().any(|line| line.contains(" resolveReviewThread")));
    assert!(output_text(&not_proven).contains("kept thread PRRT_test unresolved"));

    // Re-applying a matching keep-open disposition to a resolved thread is
    // idempotent: no duplicate reply, and the falsely-clean thread is reopened.
    let matching_marker = serde_json::json!({
        "v": 1,
        "class": "current-blocker",
        "thread_id": "PRRT_test",
        "by": "reviewer",
        "head": "0123456789012345678901234567890123456789",
        "evidence": {"argument": "The candidate still contains the defect."},
        "thread_transition": "keep_open",
    })
    .to_string();
    let (reapply, reapply_calls) = run_live(
        &root,
        "current-blocker",
        &["--argument", "The candidate still contains the defect."],
        true,
        Some(&matching_marker),
    )?;
    assert!(reapply.status.success(), "re-apply disposition failed: {}", output_text(&reapply));
    assert!(
        output_text(&reapply).contains("matching disposition already exists"),
        "a matching marker must suppress the duplicate reply: {}",
        output_text(&reapply)
    );
    assert!(!reapply_calls.contains("addPullRequestReviewThreadReply"));
    assert!(reapply_calls.contains("unresolveReviewThread"));
    assert!(!reapply_calls.lines().any(|line| line.contains(" resolveReviewThread")));
    assert!(output_text(&reapply).contains("reopened thread"));

    Ok(())
}

/// The receipt schema must accept every marker shape the disposition script can
/// emit, and its class enum must stay in lockstep with the script's vocabulary.
/// Platform-neutral: needs no bash, gh, or jq, so it proves the schema contract
/// on Windows too.
#[test]
fn receipt_schema_accepts_emitted_marker_shapes() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let schema: Value = serde_json::from_str(&fs::read_to_string(
        root.join(".ci/receipts/schemas/review-disposition.schema.json"),
    )?)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| format!("review-disposition schema is invalid: {error}"))?;

    let classes: Vec<&str> = schema
        .pointer("/properties/class/enum")
        .and_then(Value::as_array)
        .ok_or("schema has no class enum")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for emitted in [
        "fixed",
        "refuted",
        "superseded",
        "post-merge-follow-up",
        "current-blocker",
        "blocked-by-prerequisite",
        "not-proven",
    ] {
        assert!(classes.contains(&emitted), "schema rejects emitted class: {emitted}");
    }
    assert!(
        classes.contains(&"follow-up"),
        "legacy follow-up must remain valid so historical markers still parse"
    );

    let marker = |class: &str, transition: &str, evidence: Value| {
        serde_json::json!({
            "v": 1,
            "class": class,
            "thread_id": "PRRT_test",
            "by": "reviewer",
            "head": "0123456789012345678901234567890123456789",
            "evidence": evidence,
            "thread_transition": transition,
        })
    };

    let emitted = [
        marker("fixed", "resolve", serde_json::json!({"commit": "abc1234"})),
        marker("refuted", "resolve", serde_json::json!({"argument": "does not hold"})),
        marker("superseded", "resolve", serde_json::json!({"superseded_by": "#13366"})),
        marker(
            "post-merge-follow-up",
            "resolve",
            serde_json::json!({"issue": 13342, "argument": "claim already satisfied"}),
        ),
        marker("current-blocker", "keep_open", serde_json::json!({"argument": "defect remains"})),
        marker(
            "blocked-by-prerequisite",
            "keep_open",
            serde_json::json!({"issue": 13342, "argument": "defect remains"}),
        ),
        marker("not-proven", "keep_open", serde_json::json!({"argument": "evidence missing"})),
    ];
    for instance in &emitted {
        assert!(
            validator.is_valid(instance),
            "schema rejects a marker shape the script emits: {instance}"
        );
    }

    // Historical v1 markers predate thread_transition; they must still validate.
    let historical = serde_json::json!({
        "v": 1,
        "class": "follow-up",
        "thread_id": "PRRT_historical",
        "by": "reviewer",
        "head": "0123456789012345678901234567890123456789",
        "evidence": {"issue": 13342},
    });
    assert!(validator.is_valid(&historical), "historical v1 marker must remain valid");

    // additionalProperties stays closed against unknown envelope fields.
    let mut unknown = marker("fixed", "resolve", serde_json::json!({"commit": "abc1234"}));
    unknown["surprise"] = Value::Bool(true);
    assert!(!validator.is_valid(&unknown), "schema must stay closed to unknown fields");

    Ok(())
}
