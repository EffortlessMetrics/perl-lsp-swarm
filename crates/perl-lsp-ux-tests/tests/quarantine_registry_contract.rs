//! Regression guard for the editor UX quarantine registry.
//!
//! Two surfaces are enforced here:
//!
//! 1. Scenario 14 rows carry terminal (or honestly unproven) dispositions with
//!    executable replacement mappings.
//! 2. Every `verified` disposition is bound to an exact `verified_sha` plus the
//!    replacement-source git blob at that commit, and a drift negative control
//!    re-verifies that binding: it fails when the recorded sha is fabricated or
//!    pruned in a full-history clone, when the sha does not carry the recorded
//!    blob, or when the quarantined artifact has drifted since verification.
//! 3. Every `verified` disposition names the concrete `verification_pr` that
//!    carried the evidence run, and all verified Scenario 14 rows share one
//!    (verification_pr, verified_sha, artifact blob) evidence event, so stale
//!    or free-floating provenance cannot hide behind the sha↔blob checks.
//! 4. Every `active` row carries the bounded review contract of #10015: a
//!    current issue, a named owner, and a bounded `expires_after_days` — no
//!    indefinite, ownerless quarantine may re-enter the registry silently.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SCENARIO_SOURCE: &str = "crates/perl-lsp-ux-tests/tests/ux_scenario_14_inc_conformance.rs";

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git(root: &Path, args: &[&str]) -> io::Result<Result<String, String>> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() { Ok(Ok(stdout)) } else { Ok(Err(stderr)) }
}

fn is_40_hex(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Bind the verification provenance: a `verified` row must name the concrete
/// pull request that carried the exact-head evidence run. Without this check
/// the field is a free annotation — null, non-integer, or placeholder values
/// leave every sha↔blob check green while the recorded provenance is
/// fabricated. This detector is shared by the live contract test and the
/// provenance negative control.
fn check_verification_pr(test: &str, verification_pr: &Value) -> Result<(), String> {
    match verification_pr.as_u64() {
        Some(pr) if pr >= 1 => Ok(()),
        _ => Err(format!(
            "{test}: verified row must record the concrete verification_pr that carried the \
             evidence run, got {verification_pr}"
        )),
    }
}

/// Enforce the #10015 bounded active-row contract: an `active` quarantine must
/// name its current issue and owner and carry a bounded review expiry. Without
/// this detector an active row can re-enter the registry with
/// `expires_after_days: null` and no honest owner — the exact unbounded shape
/// #10015 forbids. Shared by the live contract test and the negative control.
fn check_active_row_bounds(
    test: &str,
    issue: &Value,
    owner: &Value,
    expires_after_days: &Value,
) -> Result<(), String> {
    if issue.as_u64().map(|issue| issue >= 1) != Some(true) {
        return Err(format!(
            "{test}: active row must name its current tracking issue, got {issue}"
        ));
    }
    if owner.as_str().is_none_or(|owner| owner.trim().is_empty()) {
        return Err(format!("{test}: active row must name a current owner, got {owner}"));
    }
    if expires_after_days.as_u64().map(|days| days >= 1) != Some(true) {
        return Err(format!(
            "{test}: active row must carry a bounded expires_after_days (integer >= 1), \
             got {expires_after_days} — no indefinite quarantine"
        ));
    }
    Ok(())
}

/// Re-verify one recorded `verified_sha` binding against the quarantined
/// artifact. This is the drift detector shared by the live contract test and
/// the negative control.
///
/// - `blob_at_verified_sha` is `Some(blob)` when git resolved the recorded
///   commit and its blob for the artifact, and `None` when the object is not
///   present in this clone.
/// - `history_available` is `false` exactly for shallow clones, where old
///   commits are legitimately absent; the deep sha→blob cross-check is then
///   skipped as a documented limitation. In a full-history clone an
///   unresolvable `verified_sha` is fabrication or pruning and must fail.
fn check_verified_binding(
    test: &str,
    verified_sha: &str,
    recorded_blob: &str,
    current_blob: &str,
    blob_at_verified_sha: Option<&str>,
    history_available: bool,
) -> Result<(), String> {
    if !is_40_hex(verified_sha) {
        return Err(format!("{test}: verified_sha `{verified_sha}` is not a 40-hex commit id"));
    }
    if !is_40_hex(recorded_blob) {
        return Err(format!(
            "{test}: verified_artifact_blob `{recorded_blob}` is not a 40-hex blob id"
        ));
    }

    match blob_at_verified_sha {
        Some(blob_at_sha) => {
            if blob_at_sha != recorded_blob {
                return Err(format!(
                    "{test}: verified_sha {verified_sha} carries artifact blob {blob_at_sha}, \
                     but the entry records {recorded_blob} — the verification binding is \
                     fabricated or mis-bound"
                ));
            }
        }
        None => {
            if history_available {
                return Err(format!(
                    "{test}: verified_sha {verified_sha} is not resolvable in a full-history \
                     clone — the recorded verification commit is fabricated, pruned, or mistyped"
                ));
            }
            // Shallow clone: the deep cross-check is impossible here; the
            // current-blob drift check below still runs everywhere.
        }
    }

    if current_blob != recorded_blob {
        return Err(format!(
            "{test}: quarantined artifact drifted since verified_sha {verified_sha} \
             (recorded blob {recorded_blob}, current blob {current_blob}) — the disposition is \
             stale; re-run the evidence lane and re-record the binding"
        ));
    }
    Ok(())
}

#[test]
fn scenario_14_quarantine_rows_have_terminal_executable_dispositions() -> TestResult {
    let root = repo_root();
    let ledger_raw = fs::read_to_string(root.join(".ci/ux-flakes.json"))?;
    let ledger: Value = serde_json::from_str(&ledger_raw)?;
    let entries = ledger["entries"]
        .as_array()
        .ok_or_else(|| invalid_data("ux-flakes entries must be an array"))?;
    let scenario_source = fs::read_to_string(root.join(SCENARIO_SOURCE))?;

    let scenario_rows: Vec<&Value> = entries
        .iter()
        .filter(|entry| {
            entry["test"]
                .as_str()
                .is_some_and(|test| test.starts_with("ux_scenario_14_inc_conformance::"))
        })
        .collect();

    assert_eq!(scenario_rows.len(), 11, "expected the 11 historical Scenario 14 rows");

    let mut verified_count = 0usize;
    let mut unverified_count = 0usize;
    // (test, verification_pr, verified_sha, artifact blob) per verified row,
    // for the single-evidence-event join asserted after the loop.
    let mut verification_events: Vec<(&str, u64, &str, &str)> = Vec::new();
    for entry in scenario_rows {
        let test = entry["test"].as_str().unwrap_or("<missing test>");
        let evidence = &entry["evidence"];

        // The verification classification must be explicit; a null verified_sha
        // without an unverified classification is the vacuous shape this guard
        // exists to reject.
        let verification_state = evidence["verification_state"]
            .as_str()
            .ok_or_else(|| invalid_data(format!("{test} is missing verification_state")))?;
        let verified_sha = evidence["verified_sha"].as_str().unwrap_or("<missing>");
        let unverified_reason = evidence["unverified_reason"].as_str();

        match verification_state {
            "verified" => {
                verified_count += 1;
                assert!(
                    is_40_hex(verified_sha),
                    "{test} claims verified without a 40-hex verified_sha, got `{verified_sha}`"
                );
                let recorded_blob =
                    evidence["verified_artifact_blob"].as_str().unwrap_or("<missing>");
                assert!(
                    is_40_hex(recorded_blob),
                    "{test} claims verified without a 40-hex verified_artifact_blob"
                );
                assert!(
                    unverified_reason.is_none(),
                    "{test} claims verified but also carries an unverified_reason"
                );
                check_verification_pr(test, &evidence["verification_pr"])
                    .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
                if let Some(pr) = evidence["verification_pr"].as_u64() {
                    verification_events.push((test, pr, verified_sha, recorded_blob));
                }
            }
            "unverified" => {
                unverified_count += 1;
                assert_eq!(
                    evidence["verified_sha"],
                    Value::Null,
                    "{test} is unverified but records a verified_sha — never fabricate one"
                );
                let reason = unverified_reason.unwrap_or_default().trim().to_owned();
                assert!(
                    reason.len() >= 20,
                    "{test} is unverified without a named reason; got `{reason}`"
                );
            }
            other => {
                return Err(format!("{test} has unknown verification_state `{other}`").into());
            }
        }

        if entry["state"] == "resolved" {
            let disposition = entry["disposition"]
                .as_str()
                .ok_or_else(|| invalid_data(format!("{test} is missing disposition")))?;
            assert!(
                matches!(
                    disposition,
                    "stabilized" | "resolved_by_intent" | "folded" | "not_proven"
                ),
                "{test} has non-terminal disposition {disposition}"
            );
            assert_ne!(entry["issue"], 7570, "{test} must not route to unrelated #7570");

            assert_eq!(
                evidence["command"], "PERL_LSP_UX_REQUIRE_BINARY=1 just ux-tests",
                "{test} must name the hard-fail verification lane"
            );
            let replacements = evidence["replacement_tests"]
                .as_array()
                .ok_or_else(|| invalid_data(format!("{test} is missing replacement_tests")))?;
            assert!(!replacements.is_empty(), "{test} must map to executable replacement coverage");
            for replacement in replacements {
                let replacement = replacement
                    .as_str()
                    .ok_or_else(|| invalid_data(format!("{test} has a non-string replacement")))?;
                assert!(
                    scenario_source.contains(&format!("fn {replacement}(")),
                    "{test} points to missing replacement test {replacement}"
                );
            }
        }
    }

    // The FindBin row is the honestly unproven one: its replacement tolerates
    // the consumer divergence it claims to guard, so it stays an active,
    // issue-owned blocker instead of a resolved disposition.
    let findbin = entries
        .iter()
        .find(|entry| {
            entry["test"].as_str()
                == Some("ux_scenario_14_inc_conformance::scenario_14_findbin_relative")
        })
        .ok_or_else(|| invalid_data("FindBin row missing from registry"))?;
    assert_eq!(findbin["state"], "active", "FindBin proof debt must stay active");
    assert_eq!(findbin["disposition"], "not_proven");
    assert_eq!(findbin["issue"], 10015, "FindBin proof debt must be issue-owned");
    assert!(
        findbin["owner"].as_str().is_some_and(|owner| !owner.is_empty()),
        "active FindBin row must name an owner"
    );
    assert_eq!(findbin["evidence"]["verification_state"], "unverified");

    assert_eq!(verified_count, 10, "exactly 10 rows carry an exact-head binding");
    assert_eq!(unverified_count, 1, "only the FindBin row is honestly unverified");

    // Durable provenance join: every verified row binds the same Scenario 14
    // artifact blob, so drift invalidates all of them at once and an honest
    // re-verification is a single evidence event. The rows must therefore
    // record one identical (verification_pr, verified_sha, artifact blob)
    // triple; per-row provenance tampering cannot hide behind the aggregate
    // sha↔blob re-verification.
    if let Some((_, first_pr, first_sha, first_blob)) = verification_events.first() {
        for (test, pr, sha, blob) in &verification_events {
            assert!(
                (pr, sha, blob) == (first_pr, first_sha, first_blob),
                "{test}: verified rows must share one exact-head evidence event; this row \
                 records (pr {pr}, sha {sha}, blob {blob}) but another row records \
                 (pr {first_pr}, sha {first_sha}, blob {first_blob})"
            );
        }
    }

    assert_eq!(ledger["summary"]["active"], 1);
    assert_eq!(ledger["summary"]["resolved"], 10);
    Ok(())
}

#[test]
fn verified_bindings_reverify_without_drift() -> TestResult {
    // Drift negative control (live half): re-verify every sampled verified_sha
    // against the quarantined artifact. Any fabricated sha, sha↔blob mismatch,
    // or post-verification artifact drift fails this test.
    let root = repo_root();
    let ledger_raw = fs::read_to_string(root.join(".ci/ux-flakes.json"))?;
    let ledger: Value = serde_json::from_str(&ledger_raw)?;

    let current_blob = match git(&root, &["hash-object", SCENARIO_SOURCE])? {
        Ok(blob) => blob,
        Err(err) => return Err(format!("git hash-object failed: {err}").into()),
    };

    let common_dir = git(&root, &["rev-parse", "--git-common-dir"])?
        .map_err(|err| format!("git rev-parse --git-common-dir failed: {err}"))?;
    // git runs with cwd = root, so a relative common dir resolves against root;
    // Path::join also accepts absolute common dirs.
    let history_available = !root.join(&common_dir).join("shallow").exists();

    let mut sampled = 0usize;
    for entry in ledger["entries"]
        .as_array()
        .ok_or_else(|| invalid_data("ux-flakes entries must be an array"))?
    {
        if entry["evidence"]["verification_state"] != "verified" {
            continue;
        }
        let test =
            entry["test"].as_str().ok_or_else(|| invalid_data("verified row missing test name"))?;
        let verified_sha = entry["evidence"]["verified_sha"]
            .as_str()
            .ok_or_else(|| invalid_data("verified row missing verified_sha"))?;
        let recorded_blob = entry["evidence"]["verified_artifact_blob"]
            .as_str()
            .ok_or_else(|| invalid_data("verified row missing verified_artifact_blob"))?;

        let blob_at_sha =
            git(&root, &["rev-parse", &format!("{verified_sha}:{SCENARIO_SOURCE}")])?.ok();

        check_verified_binding(
            test,
            verified_sha,
            recorded_blob,
            &current_blob,
            blob_at_sha.as_deref(),
            history_available,
        )
        .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
        sampled += 1;
    }

    assert!(sampled >= 10, "drift control must actually sample the verified rows, got {sampled}");
    Ok(())
}

#[test]
fn drift_negative_control_fails_on_tampered_bindings() -> TestResult {
    // Drift negative control (fault-injection half): the detector must fail on
    // each stale/fabricated shape, not just pass on the healthy ledger.
    const SHA: &str = "65f34b9061c0aab996e7f48e0efba43186d7db96";
    const BLOB: &str = "f3c571ac0c0c195ebf5c11a6d1b37480b761265c";

    // 1. Healthy binding passes.
    assert!(check_verified_binding("t", SHA, BLOB, BLOB, Some(BLOB), true).is_ok());

    // 2. Artifact drift after verification (stale disposition) fails.
    let err = check_verified_binding(
        "t",
        SHA,
        BLOB,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        Some(BLOB),
        true,
    )
    .err()
    .ok_or_else(|| invalid_data("drifted artifact must fail"))?;
    assert!(err.contains("drifted"), "{err}");

    // 3. Fabricated sha in a full-history clone fails.
    let err = check_verified_binding("t", SHA, BLOB, BLOB, None, true)
        .err()
        .ok_or_else(|| invalid_data("unresolvable sha in full history must fail"))?;
    assert!(err.contains("not resolvable"), "{err}");

    // 4. sha that does not carry the recorded blob fails.
    let err = check_verified_binding(
        "t",
        SHA,
        BLOB,
        BLOB,
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        true,
    )
    .err()
    .ok_or_else(|| invalid_data("sha-blob mismatch must fail"))?;
    assert!(err.contains("mis-bound") || err.contains("fabricated"), "{err}");

    // 5. Null/classification escapes fail the format gate.
    let err = check_verified_binding("t", "null", BLOB, BLOB, Some(BLOB), true)
        .err()
        .ok_or_else(|| invalid_data("non-40-hex sha must fail"))?;
    assert!(err.contains("40-hex"), "{err}");

    // 6. Shallow clones legitimately skip only the deep cross-check; drift
    //    detection still applies.
    assert!(check_verified_binding("t", SHA, BLOB, BLOB, None, false).is_ok());
    let err = check_verified_binding(
        "t",
        SHA,
        BLOB,
        "cccccccccccccccccccccccccccccccccccccccc",
        None,
        false,
    )
    .err()
    .ok_or_else(|| invalid_data("shallow clones must still detect artifact drift"))?;
    assert!(err.contains("drifted"), "{err}");

    Ok(())
}

#[test]
fn active_rows_carry_bounded_review_contract() -> TestResult {
    // Live half: every currently active row must satisfy the #10015 bounded
    // active-row contract. With zero active rows this is vacuously green, but
    // the detector is exercised below by fault injection so the bound cannot
    // silently rot.
    let root = repo_root();
    let ledger_raw = fs::read_to_string(root.join(".ci/ux-flakes.json"))?;
    let ledger: Value = serde_json::from_str(&ledger_raw)?;
    let summary_active = ledger["summary"]["active"].as_u64().ok_or_else(|| {
        invalid_data("ux-flakes summary.active must be an integer for the active-row bound")
    })?;

    let mut active_count = 0usize;
    for entry in ledger["entries"]
        .as_array()
        .ok_or_else(|| invalid_data("ux-flakes entries must be an array"))?
    {
        if entry["state"] != "active" {
            continue;
        }
        let test = entry["test"].as_str().unwrap_or("<missing test>");
        check_active_row_bounds(
            test,
            &entry["issue"],
            &entry["owner"],
            &entry["expires_after_days"],
        )
        .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
        active_count += 1;
    }

    // The summary is a generated projection; it must not disagree with the
    // rows the bound just walked.
    assert_eq!(
        active_count as u64, summary_active,
        "summary.active ({summary_active}) must equal the actual active row count ({active_count})"
    );
    Ok(())
}

#[test]
fn active_row_bounds_negative_control_fails_on_unbounded_rows() -> TestResult {
    // Fault-injection half: the detector must fail on each unbounded active
    // shape, not just pass on the healthy ledger.
    let issue = Value::from(10015);
    let owner = Value::from("@maintainer");
    let bounded = Value::from(30);

    // 1. A bounded, issue-owned active row passes.
    assert!(check_active_row_bounds("t", &issue, &owner, &bounded).is_ok());

    // 2. Null expiry — the indefinite quarantine #10015 forbids — fails.
    let err = check_active_row_bounds("t", &issue, &owner, &Value::Null)
        .err()
        .ok_or_else(|| invalid_data("null expiry must fail"))?;
    assert!(err.contains("expires_after_days"), "{err}");

    // 3. Zero/negative expiry bounds are placeholders, not review windows.
    for days in [Value::from(0), Value::from(-5)] {
        let err = check_active_row_bounds("t", &issue, &owner, &days)
            .err()
            .ok_or_else(|| invalid_data("non-positive expiry must fail"))?;
        assert!(err.contains("expires_after_days"), "{err}");
    }

    // 4. A non-integer expiry shape fails the bound.
    let err = check_active_row_bounds("t", &issue, &owner, &Value::from("30"))
        .err()
        .ok_or_else(|| invalid_data("string expiry must fail"))?;
    assert!(err.contains("expires_after_days"), "{err}");

    // 5. Ownerless or blank-owner active rows fail.
    for owner in [Value::Null, Value::from("")] {
        let err = check_active_row_bounds("t", &issue, &owner, &bounded)
            .err()
            .ok_or_else(|| invalid_data("missing owner must fail"))?;
        assert!(err.contains("owner"), "{err}");
    }

    // 6. Issue-less active rows fail.
    let err = check_active_row_bounds("t", &Value::Null, &owner, &bounded)
        .err()
        .ok_or_else(|| invalid_data("missing issue must fail"))?;
    assert!(err.contains("issue"), "{err}");

    Ok(())
}

#[test]
fn verification_pr_negative_control_fails_on_tampered_identity() -> TestResult {
    // Provenance negative control (fault-injection half): the detector must
    // fail on each tampered verification_pr shape, not just pass on the
    // healthy ledger. Dropping or blurring the PR identity must fail even
    // while the sha↔blob drift checks still pass.
    assert!(check_verification_pr("t", &Value::from(14393)).is_ok());

    let err = check_verification_pr("t", &Value::Null)
        .err()
        .ok_or_else(|| invalid_data("null verification_pr must fail"))?;
    assert!(err.contains("verification_pr"), "{err}");

    let err = check_verification_pr("t", &Value::from(0))
        .err()
        .ok_or_else(|| invalid_data("placeholder verification_pr must fail"))?;
    assert!(err.contains("verification_pr"), "{err}");

    let err = check_verification_pr("t", &Value::from("14393"))
        .err()
        .ok_or_else(|| invalid_data("string verification_pr must fail"))?;
    assert!(err.contains("verification_pr"), "{err}");

    Ok(())
}
