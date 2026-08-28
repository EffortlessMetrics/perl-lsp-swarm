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
            other => panic!("{test} has unknown verification_state `{other}`"),
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
    for entry in ledger["entries"].as_array().expect("entries array") {
        if entry["evidence"]["verification_state"] != "verified" {
            continue;
        }
        let test = entry["test"].as_str().expect("test name");
        let verified_sha = entry["evidence"]["verified_sha"].as_str().expect("verified sha");
        let recorded_blob =
            entry["evidence"]["verified_artifact_blob"].as_str().expect("recorded blob");

        let blob_at_sha =
            match git(&root, &["rev-parse", &format!("{verified_sha}:{SCENARIO_SOURCE}")])? {
                Ok(blob) => Some(blob),
                Err(_) => None,
            };

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
fn drift_negative_control_fails_on_tampered_bindings() {
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
    .expect_err("drifted artifact must fail");
    assert!(err.contains("drifted"), "{err}");

    // 3. Fabricated sha in a full-history clone fails.
    let err = check_verified_binding("t", SHA, BLOB, BLOB, None, true)
        .expect_err("unresolvable sha in full history must fail");
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
    .expect_err("sha-blob mismatch must fail");
    assert!(err.contains("mis-bound") || err.contains("fabricated"), "{err}");

    // 5. Null/classification escapes fail the format gate.
    let err = check_verified_binding("t", "null", BLOB, BLOB, Some(BLOB), true)
        .expect_err("non-40-hex sha must fail");
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
    .expect_err("shallow clones must still detect artifact drift");
    assert!(err.contains("drifted"), "{err}");
}
