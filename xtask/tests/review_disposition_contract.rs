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
    if [[ -n "${GH_STUB_MARKERS_JSON:-}" ]]; then
      # Multi-marker seed: array of {version, marker} pairs, each emitted as
      # its own comment so the disposition script sees v1 and v2 envelopes
      # on the same thread fixture (#15282).
      jq -cn --argjson resolved "$GH_STUB_RESOLVED" --argjson markers "$GH_STUB_MARKERS_JSON" '
        $markers
        | map({body: ("prior note\n<!-- disposition:" + .version + " " + .marker + " -->")})
        as $nodes
        | {data:{node:{isResolved:$resolved, comments:{nodes:$nodes}}}}'
    elif [[ -n "${GH_STUB_MARKER:-}" ]]; then
      jq -cn --argjson resolved "$GH_STUB_RESOLVED" --arg version "${GH_STUB_MARKER_VERSION:-v1}" --arg marker "$GH_STUB_MARKER" \
        '{data:{node:{isResolved:$resolved,comments:{nodes:[{body:("prior note\n<!-- disposition:" + $version + " " + $marker + " -->")}]}}}}'
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

/// `<!-- disposition:v[0-9]+ ... -->` markers that the script does not support
/// (e.g. a future v2 envelope) must be detected as malformed and force a fail-
/// closed path; previously the v1 literal in the filter silently dropped v2
/// markers, which let the script post duplicate replies and resolve threads
/// that already carried an authoritative v2 disposition (#15282).
#[cfg(unix)]
fn run_live_with_envelopes(
    root: &Path,
    class: &str,
    extra: &[&str],
    initially_resolved: bool,
    envelopes: &[(&str, &str)],
) -> Result<(Output, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let stub_bin = write_gh_stub(temp.path())?;
    let log = temp.path().join("gh.log");

    let mut path = OsString::from(stub_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").ok_or("PATH is unavailable")?);

    let markers_json = serde_json::Value::Array(
        envelopes
            .iter()
            .map(|(version, marker)| serde_json::json!({"version": version, "marker": marker}))
            .collect(),
    )
    .to_string();

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
        .env("GH_STUB_RESOLVED", if initially_resolved { "true" } else { "false" })
        .env("GH_STUB_MARKERS_JSON", markers_json);

    let output = command.output()?;
    let calls = fs::read_to_string(&log).unwrap_or_default();
    Ok((output, calls))
}

#[cfg(unix)]
#[test]
fn unsupported_marker_versions_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;

    let v1_marker = serde_json::json!({
        "v": 1,
        "class": "fixed",
        "thread_id": "PRRT_test",
        "by": "reviewer",
        "head": "0123456789012345678901234567890123456789",
        "evidence": {"commit": "abc1234"},
        "thread_transition": "resolve",
    })
    .to_string();
    let v2_marker = serde_json::json!({
        "v": 2,
        "class": "fixed",
        "thread_id": "PRRT_test",
        "by": "reviewer",
        "head": "0123456789012345678901234567890123456789",
        "evidence": {"commit": "abc1234"},
        "thread_transition": "resolve",
    })
    .to_string();

    // A v2-only fixture must refuse mutation: the script has no envelope for
    // v2 yet, so any v2 marker is unsupported.
    let (v2_only, v2_only_calls) = run_live_with_envelopes(
        &root,
        "fixed",
        &["--commit", "abc1234"],
        false,
        &[("v2", &v2_marker)],
    )?;
    assert!(
        !v2_only.status.success(),
        "a v2-only fixture must fail closed: {}",
        output_text(&v2_only)
    );
    assert!(
        output_text(&v2_only).contains("malformed disposition marker"),
        "expected the v2 rejection error to mention malformed markers: {}",
        output_text(&v2_only)
    );
    assert!(
        !v2_only_calls.contains("addPullRequestReviewThreadReply"),
        "v2-only fixture must not produce a duplicate reply"
    );
    assert!(
        !v2_only_calls.contains("resolveReviewThread"),
        "v2-only fixture must not resolve the thread"
    );

    // A mixed v1+v2 fixture must also fail closed: the v2 envelope is
    // unsupported even if a sibling v1 marker would otherwise match.
    let (mixed, mixed_calls) = run_live_with_envelopes(
        &root,
        "fixed",
        &["--commit", "abc1234"],
        false,
        &[("v1", &v1_marker), ("v2", &v2_marker)],
    )?;
    assert!(
        !mixed.status.success(),
        "a mixed v1+v2 fixture must fail closed (the v2 marker is unsupported): {}",
        output_text(&mixed)
    );
    assert!(
        output_text(&mixed).contains("malformed disposition marker"),
        "expected the mixed-fixture rejection to mention malformed markers: {}",
        output_text(&mixed)
    );
    assert!(
        !mixed_calls.contains("addPullRequestReviewThreadReply"),
        "mixed v1+v2 must not produce a reply (the v2 marker must gate the mutation)"
    );

    // Regression: a v1-only fixture that matches the supplied class+evidence
    // must still match the existing idempotent re-apply path; the broadened
    // filter must not break existing v1 behavior.
    let (v1_reapply, v1_reapply_calls) = run_live_with_envelopes(
        &root,
        "fixed",
        &["--commit", "abc1234"],
        false,
        &[("v1", &v1_marker)],
    )?;
    assert!(
        v1_reapply.status.success(),
        "v1-only matching fixture must succeed: {}",
        output_text(&v1_reapply)
    );
    assert!(
        output_text(&v1_reapply).contains("matching disposition already exists"),
        "v1 matching marker must still drive the idempotent reapply path: {}",
        output_text(&v1_reapply)
    );
    assert!(
        !v1_reapply_calls.contains("addPullRequestReviewThreadReply"),
        "v1-only matching fixture must not produce a duplicate reply"
    );

    Ok(())
}

/// Fixture GraphQL `reviewThreads` payload covering the #17095 selection matrix.
#[cfg(unix)]
fn sweep_graphql_fixture() -> Value {
    let node = |id: &str, resolved: bool, outdated: bool, author: Option<(&str, &str)>| {
        let author_json = match author {
            Some((typename, login)) => serde_json::json!({"__typename": typename, "login": login}),
            None => Value::Null,
        };
        serde_json::json!({
            "id": id,
            "isResolved": resolved,
            "isOutdated": outdated,
            "path": format!("src/{id}.rs"),
            "comments": {"nodes": [{"author": author_json, "body": "finding", "url": "https://example.invalid/t"}]}
        })
    };
    serde_json::json!({
        "pr": 42,
        "repo": "owner/repo",
        "data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
            node("PRRT_bot_outdated", false, true, Some(("Bot", "coderabbitai"))),
            node("PRRT_bot_current", false, false, Some(("Bot", "gemini-code-assist"))),
            node("PRRT_human_outdated", false, true, Some(("User", "alice"))),
            node("PRRT_resolved", true, true, Some(("Bot", "coderabbitai"))),
            node("PRRT_ghost", false, true, None),
        ]}}}}
    })
}

#[cfg(unix)]
fn sweep_threads_json_fixture() -> Value {
    let thread = |id: &str, resolved: bool, outdated: bool, typename: Option<&str>, login: &str| {
        serde_json::json!({
            "id": id,
            "isResolved": resolved,
            "isOutdated": outdated,
            "path": format!("src/{id}.rs"),
            "author": login,
            "author_typename": typename,
        })
    };
    serde_json::json!({
        "pr": 42,
        "repo": "owner/repo",
        "threads": [
            thread("PRRT_bot_outdated", false, true, Some("Bot"), "coderabbitai"),
            thread("PRRT_bot_current", false, false, Some("Bot"), "gemini-code-assist"),
            thread("PRRT_human_outdated", false, true, Some("User"), "alice"),
            thread("PRRT_resolved", true, true, Some("Bot"), "coderabbitai"),
            thread("PRRT_ghost", false, true, None, "unknown"),
        ]
    })
}

#[cfg(unix)]
const SWEEP_HEAD: &str = "0123456789012345678901234567890123456789";

#[cfg(unix)]
fn first_json_line(stdout: &str) -> Result<Value, Box<dyn Error>> {
    let line = stdout
        .lines()
        .find(|line| line.starts_with('{'))
        .ok_or("sweep output is missing a JSON selection object")?;
    Ok(serde_json::from_str(line)?)
}

#[cfg(unix)]
fn write_poison_gh(dir: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("bin");
    fs::create_dir_all(&bin)?;
    let log = dir.join("gh.log");
    fs::write(&log, "")?;
    let gh = bin.join("gh");
    fs::write(
        &gh,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_STUB_LOG"
echo "ERROR: gh must not be invoked against live GitHub during fixture dry-run" >&2
exit 1
"#,
    )?;
    let mut permissions = fs::metadata(&gh)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gh, permissions)?;
    Ok((bin, log))
}

#[cfg(unix)]
fn run_sweep(
    root: &Path,
    payload: &Value,
    extra: &[&str],
    poison_gh: bool,
) -> Result<(Output, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let payload_path = temp.path().join("threads.json");
    fs::write(&payload_path, serde_json::to_vec_pretty(payload)?)?;

    let log = temp.path().join("gh.log");
    fs::write(&log, "")?;
    let mut path = OsString::new();
    if poison_gh {
        let (bin, _) = write_poison_gh(temp.path())?;
        path.push(bin.as_os_str());
        path.push(":");
    } else {
        let bin = write_gh_stub(temp.path())?;
        path.push(bin.as_os_str());
        path.push(":");
    }
    path.push(std::env::var_os("PATH").ok_or("PATH is unavailable")?);

    let mut command = Command::new("bash");
    command
        .arg(disposition_script(root))
        .args(["--sweep-bot-outdated", "--pr", "42", "--threads-json"])
        .arg(&payload_path)
        .args(["--repo", "owner/repo", "--head", SWEEP_HEAD, "--by", "tester"])
        .args(extra)
        .env("PATH", path)
        .env("GH_STUB_LOG", &log)
        .env("GH_STUB_RESOLVED", "false");

    let output = command.output()?;
    let calls = fs::read_to_string(&log).unwrap_or_default();
    Ok((output, calls))
}

#[cfg(unix)]
fn assert_selection_matrix(summary: &Value) -> Result<(), Box<dyn Error>> {
    let selected: Vec<&str> = summary
        .pointer("/selected")
        .and_then(Value::as_array)
        .ok_or("selection JSON missing selected[]")?
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .collect();
    assert_eq!(
        selected,
        ["PRRT_bot_outdated"],
        "only unresolved bot+outdated threads may be selected: {summary}"
    );

    let skipped = summary
        .pointer("/skipped")
        .and_then(Value::as_array)
        .ok_or("selection JSON missing skipped[]")?;
    let reason = |id: &str| -> Result<&str, Box<dyn Error>> {
        skipped
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|row| row.get("reason").and_then(Value::as_str))
            .ok_or_else(|| format!("missing skip reason for {id} in {summary}").into())
    };
    assert_eq!(reason("PRRT_bot_current")?, "not-outdated");
    assert_eq!(reason("PRRT_human_outdated")?, "not-bot");
    assert_eq!(reason("PRRT_resolved")?, "already-resolved");
    assert_eq!(reason("PRRT_ghost")?, "missing-author");
    Ok(())
}

#[cfg(unix)]
fn parse_convergence_v1_markers(body: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    let output = Command::new("jq")
        .args([
            "-n",
            "--arg",
            "body",
            body,
            r#"[$body | scan("<!-- disposition:v1 (\\{.*\\}) -->") | .[0] | fromjson]"#,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("convergence marker parser failed: {}", output_text(&output)).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Selection-predicate contract over recorded GraphQL and threads-JSON payloads
/// (#17095 item 1). Never talks to a live PR: `--threads-json` plus `--dry-run`
/// and a poison `gh` stub refuse any GitHub mutation or network call.
#[cfg(unix)]
#[test]
fn sweep_selects_unresolved_bot_outdated_only() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let threads = fs::read_to_string(root.join("scripts/reviews/threads"))?;
    assert!(
        threads.contains("__typename"),
        "scripts/reviews/threads must request author __typename so Bot selection is not a login heuristic"
    );

    for payload in [sweep_graphql_fixture(), sweep_threads_json_fixture()] {
        let (output, calls) = run_sweep(&root, &payload, &["--dry-run"], true)?;
        assert!(output.status.success(), "sweep dry-run failed: {}", output_text(&output));
        assert!(calls.trim().is_empty(), "dry-run must not invoke gh (log={calls})");
        let summary = first_json_line(&output_text(&output))?;
        assert_eq!(summary.get("mode").and_then(Value::as_str), Some("sweep-bot-outdated"));
        assert_eq!(summary.get("dry_run").and_then(Value::as_bool), Some(true));
        assert_eq!(summary.get("head").and_then(Value::as_str), Some(SWEEP_HEAD));
        assert_selection_matrix(&summary)?;
    }

    Ok(())
}

#[cfg(unix)]
#[test]
fn sweep_dry_run_reply_carries_v1_marker_and_head_sha() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let (output, calls) = run_sweep(&root, &sweep_graphql_fixture(), &["--dry-run"], true)?;
    assert!(output.status.success(), "sweep dry-run failed: {}", output_text(&output));
    assert!(calls.trim().is_empty(), "dry-run invoked gh: {calls}");

    let text = output_text(&output);
    assert!(
        text.contains("Disposition: superseded"),
        "templated reply must use class superseded: {text}"
    );
    assert!(
        text.contains("no longer exists at HEAD"),
        "templated reply must note the anchored hunk is gone: {text}"
    );
    assert!(text.contains(SWEEP_HEAD), "templated reply must cite the current head SHA: {text}");
    assert!(
        text.contains("resolveReviewThread(PRRT_bot_outdated)"),
        "dry-run must plan resolve after the reply: {text}"
    );
    assert!(
        !text.contains("resolveReviewThread(PRRT_bot_current)"),
        "dry-run must not plan resolve for a current bot thread: {text}"
    );
    assert!(
        !text.contains("resolveReviewThread(PRRT_human_outdated)"),
        "dry-run must not plan resolve for a human thread: {text}"
    );

    let markers = parse_convergence_v1_markers(&text)?;
    assert_eq!(
        markers.len(),
        1,
        "exactly one selected thread must emit a parseable disposition:v1 marker: {markers:?}"
    );
    let marker = &markers[0];
    assert_eq!(marker.get("v").and_then(Value::as_u64), Some(1));
    assert_eq!(marker.get("class").and_then(Value::as_str), Some("superseded"));
    assert_eq!(marker.get("thread_id").and_then(Value::as_str), Some("PRRT_bot_outdated"));
    assert_eq!(marker.get("head").and_then(Value::as_str), Some(SWEEP_HEAD));
    assert_eq!(marker.pointer("/evidence/superseded_by").and_then(Value::as_str), Some(SWEEP_HEAD));
    assert_eq!(marker.get("thread_transition").and_then(Value::as_str), Some("resolve"));

    Ok(())
}

/// Live (non-dry-run) sweep still talks only to the gh stub: selected bot+outdated
/// threads get a reply then resolve; skipped threads are not mutated.
#[cfg(unix)]
#[test]
fn sweep_live_replies_then_resolves_only_selected_threads() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let (output, calls) = run_sweep(&root, &sweep_graphql_fixture(), &[], false)?;
    assert!(output.status.success(), "sweep live fixture run failed: {}", output_text(&output));

    let reply_count = calls.matches("addPullRequestReviewThreadReply").count();
    let resolve_count = calls
        .lines()
        .filter(|line| {
            line.contains(" resolveReviewThread") || line.contains("resolveReviewThread")
        })
        .count();
    assert_eq!(reply_count, 1, "exactly one selected thread may be replied to; log={calls}");
    assert!(resolve_count >= 1, "selected thread must be resolved after the reply; log={calls}");
    assert!(
        calls.contains("addPullRequestReviewThreadReply"),
        "convergence requires a real reply before resolve; log={calls}"
    );

    let reply_at = calls.find("addPullRequestReviewThreadReply").ok_or("missing reply")?;
    let resolve_at = calls.find("resolveReviewThread").ok_or("missing resolve")?;
    assert!(
        reply_at < resolve_at,
        "reply must precede resolve (check-pr-review-convergence-core #3732); log={calls}"
    );

    for skipped in ["PRRT_bot_current", "PRRT_human_outdated", "PRRT_resolved", "PRRT_ghost"] {
        let thread_invocations = calls.lines().filter(|line| line.contains(skipped)).count();
        assert_eq!(
            thread_invocations, 0,
            "skipped thread {skipped} must not be mutated; log={calls}"
        );
    }

    Ok(())
}

/// Sweeping a selected thread that already carries an unsupported v2 marker
/// must refuse mutation (keep #15282 fail-closed behavior).
#[cfg(unix)]
#[test]
fn sweep_unsupported_marker_versions_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let payload = serde_json::json!({
        "pr": 42,
        "repo": "owner/repo",
        "data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [{
            "id": "PRRT_test",
            "isResolved": false,
            "isOutdated": true,
            "path": "src/lib.rs",
            "comments": {"nodes": [{"author": {"__typename": "Bot", "login": "coderabbitai"}, "body": "finding"}]}
        }]}}}}
    });

    let temp = tempfile::tempdir()?;
    let payload_path = temp.path().join("threads.json");
    fs::write(&payload_path, serde_json::to_vec_pretty(&payload)?)?;
    let stub_bin = write_gh_stub(temp.path())?;
    let log = temp.path().join("gh.log");

    let mut path = OsString::from(stub_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").ok_or("PATH is unavailable")?);

    let v2_marker = serde_json::json!({
        "v": 2,
        "class": "superseded",
        "thread_id": "PRRT_test",
        "by": "tester",
        "head": SWEEP_HEAD,
        "evidence": {"superseded_by": SWEEP_HEAD},
        "thread_transition": "resolve",
    })
    .to_string();
    let markers_json = serde_json::json!([{"version": "v2", "marker": v2_marker}]).to_string();

    let output = Command::new("bash")
        .arg(disposition_script(&root))
        .args(["--sweep-bot-outdated", "--pr", "42", "--threads-json"])
        .arg(&payload_path)
        .args(["--repo", "owner/repo", "--head", SWEEP_HEAD, "--by", "tester"])
        .env("PATH", path)
        .env("GH_STUB_LOG", &log)
        .env("GH_STUB_RESOLVED", "false")
        .env("GH_STUB_MARKERS_JSON", markers_json)
        .output()?;
    let calls = fs::read_to_string(&log).unwrap_or_default();

    assert!(
        !output.status.success(),
        "sweep of a v2-marked thread must fail closed: {}",
        output_text(&output)
    );
    assert!(
        output_text(&output).contains("malformed disposition marker"),
        "expected the v2 rejection error: {}",
        output_text(&output)
    );
    assert!(
        !calls.contains("addPullRequestReviewThreadReply"),
        "v2 sweep must not post a reply; log={calls}"
    );
    assert!(
        !calls.contains("resolveReviewThread"),
        "v2 sweep must not resolve the thread; log={calls}"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn sweep_skips_already_disposed_and_does_not_reapply() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let marker = serde_json::json!({
        "v": 1,
        "class": "superseded",
        "thread_id": "PRRT_bot_outdated",
        "by": "tester",
        "head": SWEEP_HEAD,
        "evidence": {"superseded_by": SWEEP_HEAD},
        "thread_transition": "resolve",
    });
    let payload = serde_json::json!({
        "pr": 42,
        "repo": "owner/repo",
        "data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [{
            "id": "PRRT_bot_outdated",
            "isResolved": false,
            "isOutdated": true,
            "path": "src/lib.rs",
            "comments": {"nodes": [
                {"author": {"__typename": "Bot", "login": "coderabbitai"}, "body": "finding"},
                {"author": {"__typename": "User", "login": "tester"},
                 "body": format!("prior\n<!-- disposition:v1 {} -->", marker)}
            ]}
        }]}}}}
    });

    let (dry, dry_calls) = run_sweep(&root, &payload, &["--dry-run"], true)?;
    assert!(dry.status.success(), "already-disposed dry-run failed: {}", output_text(&dry));
    assert!(dry_calls.trim().is_empty(), "dry-run invoked gh: {dry_calls}");
    let summary = first_json_line(&output_text(&dry))?;
    let skipped = summary.pointer("/skipped").and_then(Value::as_array).ok_or("missing skipped")?;
    assert!(
        skipped.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some("PRRT_bot_outdated")
                && row.get("reason").and_then(Value::as_str) == Some("already-disposed")
        }),
        "reopened/already-disposed bot thread must be skipped: {summary}"
    );
    assert_eq!(summary.pointer("/selected").and_then(Value::as_array).map(Vec::len), Some(0));

    let (live, live_calls) = run_sweep(&root, &payload, &[], false)?;
    assert!(live.status.success(), "already-disposed live skip failed: {}", output_text(&live));
    assert!(
        !live_calls.contains("addPullRequestReviewThreadReply"),
        "already-disposed thread must not be replied to; log={live_calls}"
    );
    assert!(
        !live_calls.contains("resolveReviewThread"),
        "already-disposed thread must not be re-resolved; log={live_calls}"
    );

    // threads --json does not carry comment bodies; --no-reapply must still
    // refuse to re-resolve when the child observes an existing v1 marker.
    let temp = tempfile::tempdir()?;
    let payload_path = temp.path().join("threads.json");
    fs::write(&payload_path, serde_json::to_vec_pretty(&sweep_threads_json_fixture())?)?;
    let stub_bin = write_gh_stub(temp.path())?;
    let log = temp.path().join("gh.log");
    let mut path = OsString::from(stub_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").ok_or("PATH is unavailable")?);
    let output = Command::new("bash")
        .arg(disposition_script(&root))
        .args(["--sweep-bot-outdated", "--pr", "42", "--threads-json"])
        .arg(&payload_path)
        .args(["--repo", "owner/repo", "--head", SWEEP_HEAD, "--by", "tester"])
        .env("PATH", path)
        .env("GH_STUB_LOG", &log)
        .env("GH_STUB_RESOLVED", "false")
        .env("GH_STUB_MARKER", marker.to_string())
        .output()?;
    let calls = fs::read_to_string(&log).unwrap_or_default();
    assert!(output.status.success(), "threads-json reapply skip failed: {}", output_text(&output));
    assert!(
        output_text(&output).contains("skip already-disposed"),
        "expected --no-reapply skip: {}",
        output_text(&output)
    );
    assert!(
        !calls.contains("addPullRequestReviewThreadReply")
            && !calls.contains("resolveReviewThread"),
        "reopened matching marker must not be mutated; log={calls}"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn sweep_live_threads_json_requires_matching_pr() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let mut foreign = sweep_graphql_fixture();
    foreign["pr"] = serde_json::json!(99);

    let (mismatch, mismatch_calls) = run_sweep(&root, &foreign, &[], false)?;
    assert!(
        !mismatch.status.success(),
        "live sweep must refuse a foreign --threads-json pr: {}",
        output_text(&mismatch)
    );
    assert!(
        output_text(&mismatch).contains("does not match --pr"),
        "expected a PR identity error: {}",
        output_text(&mismatch)
    );
    assert!(
        !mismatch_calls.contains("addPullRequestReviewThreadReply")
            && !mismatch_calls.contains("resolveReviewThread"),
        "foreign payload must not mutate; log={mismatch_calls}"
    );

    let mut unbound = sweep_graphql_fixture();
    unbound.as_object_mut().ok_or("graphql fixture is not an object")?.remove("pr");
    let (unbound_out, unbound_calls) = run_sweep(&root, &unbound, &[], false)?;
    assert!(
        !unbound_out.status.success(),
        "live GraphQL-only --threads-json must be refused: {}",
        output_text(&unbound_out)
    );
    assert!(
        output_text(&unbound_out).contains("dry-run only"),
        "expected GraphQL-only live refusal: {}",
        output_text(&unbound_out)
    );
    assert!(
        !unbound_calls.contains("resolveReviewThread"),
        "unbound payload must not mutate; log={unbound_calls}"
    );

    Ok(())
}
