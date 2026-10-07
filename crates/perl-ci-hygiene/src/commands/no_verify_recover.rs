//! Required-tier re-cover for hook-skipped pushes (`--no-verify`, #17430 / trap T9).
//!
//! `git commit --no-verify` / `git push --no-verify` skip the installed hooks
//! entirely, so anything the hooks would have refused sails silently. Mandatory
//! hook provisioning (#17406/#17414/#17426) closes the "hooks never installed"
//! half; this gate closes the other half: it re-runs hook policy over the
//! pushed range of every PR, so `--no-verify` only skips the local run, never
//! the policy.
//!
//! V1 policy is commit attribution: no in-range commit may carry a placeholder
//! git identity. That is the pre-commit hook's first refusal
//! ([`crate::git_hooks::pre_commit_hook_script`]), and on current main it is
//! the only hook-enforced policy with zero CI coverage — the pre-push
//! proof-scope policy is already re-covered by the existing required gates,
//! while secret shapes (#17428) and ref rules (#17427) do not exist as hook
//! policy yet. When those hooks land, their server-side re-scan plugs into
//! this same pushed-range gate; the range resolution and reporting below are
//! deliberately policy-agnostic.
//!
//! The hook reads live `git config user.name` / `user.email`, which a normal
//! commit records as both author and committer. History can only show the
//! recorded fields, so the gate flags a placeholder value in any of the four
//! slots (author name/email, committer name/email) with the hook's OR
//! semantics: one matching slot is enough. That is a slight superset of the
//! hook — a `git -c user.name=… commit` evades the config read but not the
//! recorded history — and the superset direction is the safe one: a flagged
//! value proves the commit was produced under a placeholder identity.
//!
//! [`scan_commit_attribution`] is a pure function over `git log` text;
//! [`check`] is the thin shell that resolves the pushed range and reports.
//!
//! Hook-policy parity is pinned by
//! [`tests::deny_list_matches_pre_commit_hook_policy`]: the deny lists below
//! must equal, exactly, the literals the generated pre-commit script refuses.
//! Editing one side without the other fails that test.

use color_eyre::eyre::{Result, eyre};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::process::Command;

use crate::{GREEN, NC, RED, YELLOW};
use serde_json::Value as JsonValue;

/// Placeholder author/committer names the pre-commit hook refuses.
///
/// Must stay identical to the `$GIT_USER_NAME` literals in
/// [`crate::git_hooks::pre_commit_hook_script`]; the parity test enforces it.
pub(crate) const PLACEHOLDER_IDENTITY_NAMES: [&str; 2] =
    ["Codex Release Validation", "xtask hook tests"];

/// Placeholder author/committer emails the pre-commit hook refuses.
///
/// Must stay identical to the `$GIT_USER_EMAIL` literals in
/// [`crate::git_hooks::pre_commit_hook_script`]; the parity test enforces it.
pub(crate) const PLACEHOLDER_IDENTITY_EMAILS: [&str; 2] =
    ["codex-release-validation@example.invalid", "xtask@example.invalid"];

/// Which recorded identity slot carried a placeholder value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributionSlot {
    /// Commit author name (`%an`).
    AuthorName,
    /// Commit author email (`%ae`).
    AuthorEmail,
    /// Committer name (`%cn`).
    CommitterName,
    /// Committer email (`%ce`).
    CommitterEmail,
}

impl fmt::Display for AttributionSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            AttributionSlot::AuthorName => "author name",
            AttributionSlot::AuthorEmail => "author email",
            AttributionSlot::CommitterName => "committer name",
            AttributionSlot::CommitterEmail => "committer email",
        };
        write!(f, "{name}")
    }
}

/// One in-range commit carrying a placeholder git identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributionFinding {
    /// Full commit SHA, so the finding names the exact commit to rewrite.
    pub(crate) sha: String,
    /// Which identity slot matched.
    pub(crate) slot: AttributionSlot,
    /// The placeholder value that matched.
    pub(crate) value: String,
    /// Commit subject, so the finding is recognizable without a lookup.
    pub(crate) subject: String,
}

/// Fields per `git log` record: SHA, author name/email, committer name/email,
/// subject — NUL-separated within one newline-terminated line.
const RECORD_FIELDS: usize = 6;

/// Scans NUL-separated `git log` records for placeholder git identities.
///
/// `log` is the stdout of
/// `git log --format=%H%x00%an%x00%ae%x00%cn%x00%ce%x00%s%x00 <range>`: one
/// record per line, six NUL-separated fields per record (SHA, author
/// name/email, committer name/email, subject). NUL cannot appear in a
/// recorded identity, and `%s` is single-line, so the grouping is
/// unambiguous for any git-producible output; anything else is an error,
/// never a silent skip. Findings are returned in log order (newest first,
/// as git emits).
///
/// # Errors
///
/// Returns an error when any record does not hold exactly six fields. An
/// unparseable range cannot prove a clean scan, so the gate fails closed
/// instead of reporting what it could parse.
pub(crate) fn scan_commit_attribution(log: &str) -> Result<Vec<AttributionFinding>> {
    if log.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut findings = Vec::new();
    for (index, line) in log.lines().enumerate() {
        let mut fields: Vec<&str> = line.split('\0').collect();
        // Every record ends with %x00, so a well-formed line ends with one
        // empty item; drop it before counting.
        if fields.last().is_some_and(|field| field.is_empty()) {
            fields.pop();
        }
        if fields.len() != RECORD_FIELDS {
            return Err(eyre!(
                "unparseable git log record {}: {} NUL-separated field(s), expected {}",
                index + 1,
                fields.len(),
                RECORD_FIELDS
            ));
        }
        let [sha, author_name, author_email, committer_name, committer_email, subject] =
            <[&str; RECORD_FIELDS]>::try_from(fields)
                .map_err(|error| eyre!("unparseable git log record {} ({error:?})", index + 1))?;
        for (slot, value) in [
            (AttributionSlot::AuthorName, author_name),
            (AttributionSlot::AuthorEmail, author_email),
            (AttributionSlot::CommitterName, committer_name),
            (AttributionSlot::CommitterEmail, committer_email),
        ] {
            if is_placeholder(slot, value) {
                findings.push(AttributionFinding {
                    sha: sha.to_owned(),
                    slot,
                    value: value.to_owned(),
                    subject: subject.to_owned(),
                });
            }
        }
    }
    Ok(findings)
}

/// Returns `true` when `value` in `slot` is a hook-refused placeholder.
///
/// Names and emails each match only their own deny list, mirroring the hook's
/// four independent `[ … = "…" ]` comparisons exactly (OR semantics, exact
/// equality — a substring or case variant is not a placeholder identity).
fn is_placeholder(slot: AttributionSlot, value: &str) -> bool {
    match slot {
        AttributionSlot::AuthorName | AttributionSlot::CommitterName => {
            PLACEHOLDER_IDENTITY_NAMES.contains(&value)
        }
        AttributionSlot::AuthorEmail | AttributionSlot::CommitterEmail => {
            PLACEHOLDER_IDENTITY_EMAILS.contains(&value)
        }
    }
}

/// Re-runs the commit-attribution hook policy over `base..HEAD`.
///
/// Returns the process exit code: `0` when no in-range commit carries a
/// placeholder identity, `1` otherwise. Uncommitted work is out of subject:
/// it has no commit attribution to scan, and the local hook owns the working
/// tree — this gate re-covers committed pushed-range policy.
///
/// # Errors
///
/// Returns an error when no usable base ref resolves in CI (fail closed: an
/// unscanned range must not report green), when an explicit `--base` does not
/// resolve, when a push payload names a `before` baseline missing locally, or
/// when `git log` cannot be executed or parsed.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        return unresolved_base_result(std::env::var_os("CI").is_some());
    };
    let merge = merge_base(repo_root, &requested_base);
    let log = read_commit_range(repo_root, &merge)?;
    let findings = scan_commit_attribution(&log)?;
    let scanned = log.lines().count();

    if findings.is_empty() {
        println!(
            "{GREEN}✅ No placeholder git identity in the pushed range{NC} \
             ({scanned} commit(s) scanned, base: {requested_base})"
        );
        return Ok(0);
    }

    let commits: BTreeSet<&str> = findings.iter().map(|finding| finding.sha.as_str()).collect();
    println!(
        "{RED}❌ no-verify re-cover: {} commit(s) in {requested_base}..HEAD carry a placeholder git identity{NC}",
        commits.len()
    );
    for finding in &findings {
        println!("  {} {} \"{}\" — {}", finding.sha, finding.slot, finding.value, finding.subject);
    }
    println!();
    println!(
        "These commits bypassed the pre-commit hook's placeholder-identity refusal \
         (e.g. via --no-verify). Fix the repo-local override first:"
    );
    println!("  git config --local --unset-all user.name");
    println!("  git config --local --unset-all user.email");
    println!(
        "then rewrite the flagged commits with a real identity \
         (git rebase -i + git commit --amend --reset-author) and push without --no-verify."
    );
    println!(
        "Re-run locally: cargo xtask ci-hygiene check-no-verify-recover --base {requested_base}"
    );
    Ok(1)
}

/// Reports an unresolvable base: a warning locally, a failure in CI.
///
/// Without a base the pushed range cannot be scanned. Locally that is a
/// warning with a naming pointer; in CI it would silently pass every
/// placeholder-identity commit, so the required gate fails closed instead
/// (mirrors the newly-added-path ratchet posture, #14688).
fn unresolved_base_result(ci: bool) -> Result<i32> {
    if ci {
        return Err(eyre!(
            "cannot resolve a merge baseline in CI; the no-verify re-cover scan did not run \
             (fetch with full history so origin/main resolves)"
        ));
    }
    println!(
        "{YELLOW}• no-verify re-cover not evaluated{NC}: no base ref resolved (tried: {}). \
         Pass --base to name one.",
        base_candidates().join(", ")
    );
    Ok(0)
}

/// Candidate base refs tried, in order, when no explicit base is supplied.
///
/// Same convention as the sibling range gates: a manual scope override, the
/// PR base ref, the main line, then the previous commit — except the
/// `HEAD~1` tail is local-only. In CI a multi-commit push with no resolvable
/// main line would otherwise scan just the last commit and report green over
/// the earlier ones; excluding the tail fails closed through
/// [`unresolved_base_result`] instead. Push runs additionally resolve the
/// event's previous tip from the `GITHUB_EVENT_PATH` payload: on a push to
/// main, checkout leaves `origin/main` at `HEAD`, so without that candidate
/// the gate would scan an empty `HEAD..HEAD` range and pass smuggled commits.
/// The payload read stays inside the gate because ci_subject forbids wiring
/// a platform scope base through the workflow.
fn base_candidates() -> Vec<String> {
    base_candidates_from(
        std::env::var("CI_SCOPE_BASE").ok(),
        std::env::var("GITHUB_BASE_REF").ok(),
        push_base_from_event(),
        std::env::var_os("CI").is_some(),
    )
}

/// Pure candidate selection behind [`base_candidates`], so tests can pin the
/// CI/local split without mutating process-global environment.
fn base_candidates_from(
    scope_base: Option<String>,
    github_base_ref: Option<String>,
    push_before: Option<String>,
    ci: bool,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(value) = scope_base {
        candidates.push(value);
    }
    if let Some(value) = github_base_ref {
        candidates.push(format!("origin/{value}"));
        candidates.push(value);
    }
    if let Some(value) = push_before {
        candidates.push(value);
    }
    candidates.extend(["origin/main".to_owned(), "main".to_owned()]);
    if !ci {
        candidates.push("HEAD~1".to_owned());
    }
    candidates
}

/// Previous tip of the current push, read from the platform event payload.
///
/// Returns `Some(sha)` only for `push` events whose payload carries a usable
/// `before` SHA: anything else (other events, missing/unreadable payload,
/// unparseable JSON, missing field, all-zero `before` from a new branch)
/// yields `None` and the candidate chain falls through. The payload path and
/// event name are platform-default environment, so this needs no workflow
/// wiring — which ci_subject would reject.
fn push_base_from_event() -> Option<String> {
    if std::env::var("GITHUB_EVENT_NAME").as_deref() != Ok("push") {
        return None;
    }
    let path = std::env::var_os("GITHUB_EVENT_PATH")?;
    let bytes = std::fs::read(path).ok()?;
    push_before_from_payload(&bytes)
}

/// Pure payload half of [`push_base_from_event`], so tests can pin parsing
/// without touching process-global environment or the filesystem.
fn push_before_from_payload(bytes: &[u8]) -> Option<String> {
    let payload: JsonValue = serde_json::from_slice(bytes).ok()?;
    let before = payload.get("before")?.as_str()?;
    if before.len() != 40
        || !before.bytes().all(|byte| byte.is_ascii_hexdigit())
        || before.bytes().all(|byte| byte == b'0')
    {
        return None;
    }
    Some(before.to_owned())
}

/// Selects the first candidate base ref that `git` can resolve.
///
/// An explicitly requested base that does not resolve is an error: the caller
/// named a subject that does not exist. Auto-resolution finding nothing is not
/// — that is "no subject to evaluate", which [`check`] reports as an
/// unevaluated run locally and fails closed on in CI. The one exception is a
/// nonzero push `before` from the event payload: it names the exact pre-push
/// tip, so when it is missing locally the gate errors naming it instead of
/// trying further automatic candidates (a shallower branch ref would scan the
/// wrong range). A genuinely absent baseline (no/non-push event, unreadable
/// payload, all-zero `before`) stays fallthrough-eligible via `None`.
fn resolve_base(repo_root: &Path, requested: Option<&str>) -> Result<Option<String>> {
    if let Some(base) = requested {
        if ref_exists(repo_root, base) {
            return Ok(Some(base.to_owned()));
        }
        return Err(eyre!("base ref '{base}' does not resolve in {}", repo_root.display()));
    }

    let push_before = push_base_from_event();
    for candidate in base_candidates() {
        // A nonzero push `before` must exist locally, not merely parse:
        // `rev-parse --verify` echoes any well-formed full SHA (exit 0
        // without consulting the object database), so `ref_exists` cannot
        // tell a missing baseline from a present one. Probe the database
        // directly and fail closed naming the missing baseline.
        if push_before.as_deref() == Some(candidate.as_str()) {
            if object_exists(repo_root, &candidate) {
                return Ok(Some(candidate));
            }
            return Err(eyre!(
                "push baseline '{candidate}' from the GITHUB_EVENT_PATH payload does not resolve \
                 locally (fetch with full history so the pushed range can be scanned)"
            ));
        }
        if ref_exists(repo_root, &candidate) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Resolves the merge base of `base` and `HEAD`, falling back to `base` itself.
fn merge_base(repo_root: &Path, base: &str) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(["merge-base", base, "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| base.to_owned())
}

/// Returns `true` when `git rev-parse --verify` resolves `reference`.
///
/// Note: for a well-formed full SHA this is a parse check, not an existence
/// check — `rev-parse --verify` echoes the SHA with exit 0 even when the
/// object is absent. Callers that need existence (the push baseline) use
/// [`object_exists`].
fn ref_exists(repo_root: &Path, reference: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Returns `true` when `object` exists in the local object database.
fn object_exists(repo_root: &Path, object: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["cat-file", "-e", object])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Reads the pushed range as NUL-separated commit-attribution records.
///
/// The range is two-dot from the merge base to `HEAD`: only committed history
/// is in subject. `%x00` separators keep the parse unambiguous no matter what
/// the subjects contain.
fn read_commit_range(repo_root: &Path, base: &str) -> Result<String> {
    let range = format!("{base}..HEAD");
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["log", "--no-color", "--format=%H%x00%an%x00%ae%x00%cn%x00%ce%x00%s%x00", &range])
        .output()
        .map_err(|error| eyre!("failed to run `git log {range}`: {error}"))?;

    if !output.status.success() {
        return Err(eyre!(
            "`git log {range}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Placeholder values the generated pre-commit script refuses, by slot.
#[cfg(test)]
struct HookPlaceholders {
    names: BTreeSet<String>,
    emails: BTreeSet<String>,
}

/// Extracts the placeholder literals from the generated pre-commit script.
///
/// Parses the `[ "$GIT_USER_NAME" = "…" ]` / `[ "$GIT_USER_EMAIL" = "…" ]`
/// comparisons; a rewritten guard shape or a changed value in either slot
/// extracts to a different set and fails the parity test rather than
/// drifting. (A genuinely new slot variable would need a gate redesign, not
/// just a list update.)
#[cfg(test)]
fn hook_placeholders(script: &str) -> Result<HookPlaceholders> {
    let mut names = BTreeSet::new();
    let mut emails = BTreeSet::new();
    for line in script.lines() {
        // Substring match: the first comparison opens the `if` line, the rest
        // are `||` continuations.
        let (slot, rest) = if let Some(index) = line.find("[ \"$GIT_USER_NAME\" = \"") {
            (&mut names, &line[index + "[ \"$GIT_USER_NAME\" = \"".len()..])
        } else if let Some(index) = line.find("[ \"$GIT_USER_EMAIL\" = \"") {
            (&mut emails, &line[index + "[ \"$GIT_USER_EMAIL\" = \"".len()..])
        } else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            return Err(eyre!("unparseable placeholder comparison: {line}"));
        };
        let (value, trailer) = rest.split_at(end);
        if trailer != "\" ]" && trailer != "\" ] || \\" && trailer != "\" ]; then" {
            return Err(eyre!("unparseable placeholder comparison: {line}"));
        }
        slot.insert(value.to_owned());
    }
    Ok(HookPlaceholders { names, emails })
}

#[cfg(test)]
mod tests {
    use super::{
        AttributionFinding, AttributionSlot, PLACEHOLDER_IDENTITY_EMAILS,
        PLACEHOLDER_IDENTITY_NAMES, base_candidates_from, hook_placeholders,
        push_before_from_payload, scan_commit_attribution, unresolved_base_result,
    };
    use color_eyre::eyre::Result;
    use std::collections::BTreeSet;

    /// Builds one NUL-separated log record like `git log --format=…%x00` emits.
    fn record(
        sha: &str,
        author_name: &str,
        author_email: &str,
        committer_name: &str,
        committer_email: &str,
        subject: &str,
    ) -> String {
        format!(
            "{sha}\0{author_name}\0{author_email}\0{committer_name}\0{committer_email}\0{subject}\0"
        )
    }

    fn clean_record(sha: &str) -> String {
        record(
            sha,
            "Ada Lovelace",
            "ada@example.com",
            "Ada Lovelace",
            "ada@example.com",
            "a change",
        )
    }

    /// Joins records the way `git log` emits them: newline-terminated lines.
    fn join_records(records: &[String]) -> String {
        let mut text = records.join("\n");
        text.push('\n');
        text
    }

    #[test]
    fn deny_list_matches_pre_commit_hook_policy() -> Result<()> {
        // Hook-policy parity (#17430): the gate's deny lists must equal exactly
        // the literals the generated pre-commit script refuses. Editing the
        // hook's list without updating the gate (or vice versa) fails here.
        let hook = hook_placeholders(crate::git_hooks::pre_commit_hook_script())?;
        let gate_names: BTreeSet<String> =
            PLACEHOLDER_IDENTITY_NAMES.iter().map(|name| (*name).to_owned()).collect();
        let gate_emails: BTreeSet<String> =
            PLACEHOLDER_IDENTITY_EMAILS.iter().map(|email| (*email).to_owned()).collect();
        assert_eq!(hook.names, gate_names, "placeholder-name deny list drifted from the hook");
        assert_eq!(hook.emails, gate_emails, "placeholder-email deny list drifted from the hook");
        assert_eq!(hook.names.len(), 2, "expected exactly the two hook-refused names");
        assert_eq!(hook.emails.len(), 2, "expected exactly the two hook-refused emails");
        Ok(())
    }

    #[test]
    fn empty_range_scans_clean() -> Result<()> {
        assert_eq!(scan_commit_attribution("")?, vec![]);
        assert_eq!(scan_commit_attribution("  \n ")?, vec![]);
        Ok(())
    }

    #[test]
    fn clean_history_scans_clean() -> Result<()> {
        let text = join_records(&[clean_record("aaa"), clean_record("bbb")]);
        assert_eq!(scan_commit_attribution(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn placeholder_author_name_is_flagged() -> Result<()> {
        let log = record(
            "abc123",
            "xtask hook tests",
            "dev@example.com",
            "Dev Human",
            "dev@example.com",
            "bypassed commit",
        );
        assert_eq!(
            scan_commit_attribution(&log)?,
            vec![AttributionFinding {
                sha: "abc123".to_owned(),
                slot: AttributionSlot::AuthorName,
                value: "xtask hook tests".to_owned(),
                subject: "bypassed commit".to_owned(),
            }]
        );
        Ok(())
    }

    #[test]
    fn placeholder_committer_email_is_flagged() -> Result<()> {
        let log = record(
            "abc123",
            "Dev Human",
            "dev@example.com",
            "Dev Human",
            "codex-release-validation@example.invalid",
            "bypassed commit",
        );
        assert_eq!(
            scan_commit_attribution(&log)?,
            vec![AttributionFinding {
                sha: "abc123".to_owned(),
                slot: AttributionSlot::CommitterEmail,
                value: "codex-release-validation@example.invalid".to_owned(),
                subject: "bypassed commit".to_owned(),
            }]
        );
        Ok(())
    }

    #[test]
    fn every_slot_is_in_subject() -> Result<()> {
        let cases = [
            (
                record(
                    "a",
                    "Codex Release Validation",
                    "dev@example.com",
                    "Dev",
                    "dev@example.com",
                    "s",
                ),
                AttributionSlot::AuthorName,
                "Codex Release Validation",
            ),
            (
                record("b", "Dev", "xtask@example.invalid", "Dev", "dev@example.com", "s"),
                AttributionSlot::AuthorEmail,
                "xtask@example.invalid",
            ),
            (
                record("c", "Dev", "dev@example.com", "xtask hook tests", "dev@example.com", "s"),
                AttributionSlot::CommitterName,
                "xtask hook tests",
            ),
            (
                record(
                    "d",
                    "Dev",
                    "dev@example.com",
                    "Dev",
                    "codex-release-validation@example.invalid",
                    "s",
                ),
                AttributionSlot::CommitterEmail,
                "codex-release-validation@example.invalid",
            ),
        ];
        for (log, slot, value) in cases {
            let findings = scan_commit_attribution(&log)?;
            assert_eq!(findings.len(), 1, "expected one finding for {slot} = {value}");
            assert_eq!(findings[0].slot, slot);
            assert_eq!(findings[0].value, value);
        }
        Ok(())
    }

    #[test]
    fn several_matches_in_one_commit_all_report() -> Result<()> {
        let log = record(
            "abc123",
            "xtask hook tests",
            "xtask@example.invalid",
            "xtask hook tests",
            "xtask@example.invalid",
            "fully placeholder",
        );
        let findings = scan_commit_attribution(&log)?;
        assert_eq!(findings.len(), 4);
        assert!(
            findings.iter().all(|finding| finding.sha == "abc123"),
            "every finding must name the violating commit"
        );
        Ok(())
    }

    #[test]
    fn findings_follow_log_order_across_commits() -> Result<()> {
        let log = join_records(&[
            record(
                "newer",
                "xtask hook tests",
                "dev@example.com",
                "Dev",
                "dev@example.com",
                "newer",
            ),
            record("older", "Dev", "dev@example.com", "Dev", "xtask@example.invalid", "older"),
        ]);
        let findings = scan_commit_attribution(&log)?;
        assert_eq!(
            findings.iter().map(|finding| finding.sha.as_str()).collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
        Ok(())
    }

    #[test]
    fn near_matches_are_not_placeholders() -> Result<()> {
        // The hook uses exact `[ … = "…" ]` equality; the gate must too. A
        // substring, a case variant, or a name/email swap is not a refusal.
        let log = join_records(&[
            record("a", "xtask hook tests!", "dev@example.com", "Dev", "dev@example.com", "s"),
            record("b", "Xtask Hook Tests", "dev@example.com", "Dev", "dev@example.com", "s"),
            record("c", "xtask@example.invalid", "dev@example.com", "Dev", "dev@example.com", "s"),
            record("d", "Dev", "Codex Release Validation", "Dev", "dev@example.com", "s"),
        ]);
        assert_eq!(scan_commit_attribution(&log)?, vec![]);
        Ok(())
    }

    #[test]
    fn subjects_survive_verbatim_in_findings() -> Result<()> {
        let log = record(
            "abc123",
            "xtask hook tests",
            "dev@example.com",
            "Dev",
            "dev@example.com",
            "fix: \"quoted\" subject — with unicode",
        );
        let findings = scan_commit_attribution(&log)?;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject, "fix: \"quoted\" subject — with unicode");
        Ok(())
    }

    #[test]
    fn truncated_stream_fails_closed() -> Result<()> {
        let error = scan_commit_attribution("abc123\0Only Name\0")
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("truncated stream must fail"))?;
        assert!(
            error.to_string().contains("unparseable git log record 1"),
            "unexpected error: {error}"
        );
        Ok(())
    }

    #[test]
    fn unresolved_base_warns_locally_and_fails_closed_in_ci() -> Result<()> {
        assert_eq!(unresolved_base_result(false)?, 0);
        let error = unresolved_base_result(true)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("CI without a baseline must fail closed"))?;
        assert!(
            error.to_string().contains("cannot resolve a merge baseline in CI"),
            "fail-closed error must name the missing baseline: {error}"
        );
        Ok(())
    }

    #[test]
    fn ci_candidates_exclude_head_parent_so_multi_commit_pushes_fail_closed() {
        // A multi-commit push with no resolvable main line must not silently
        // narrow to the last commit: without HEAD~1 nothing resolves and the
        // gate fails closed instead of reporting green over earlier commits.
        assert_eq!(
            base_candidates_from(None, None, None, true),
            vec!["origin/main".to_owned(), "main".to_owned()]
        );
    }

    #[test]
    fn local_candidates_keep_head_parent_fallback() {
        assert_eq!(
            base_candidates_from(None, None, None, false),
            vec!["origin/main".to_owned(), "main".to_owned(), "HEAD~1".to_owned()]
        );
    }

    #[test]
    fn scope_base_leads_so_push_tip_beats_degenerate_branch_refs() {
        // On a push to main, origin/main can resolve to HEAD (empty range);
        // the event's previous tip must sort before every branch candidate.
        let candidates = base_candidates_from(
            Some("override-sha".to_owned()),
            Some("main".to_owned()),
            Some("before-sha".to_owned()),
            true,
        );
        assert_eq!(
            candidates,
            vec![
                "override-sha".to_owned(),
                "origin/main".to_owned(),
                "main".to_owned(),
                "before-sha".to_owned(),
                "origin/main".to_owned(),
                "main".to_owned(),
            ]
        );
    }

    #[test]
    fn push_before_parser_accepts_only_usable_shas() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            push_before_from_payload(format!(r#"{{"before": "{sha}", "ref": "x"}}"#).as_bytes()),
            Some(sha.to_owned())
        );
        for bad in [
            "not json",
            "{}",
            r#"{"before": 42}"#,
            r#"{"before": "short"}"#,
            r#"{"before": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"}"#,
            r#"{"before": "0000000000000000000000000000000000000000"}"#,
        ] {
            assert_eq!(
                push_before_from_payload(bad.as_bytes()),
                None,
                "payload must not yield a base: {bad}"
            );
        }
    }
}
