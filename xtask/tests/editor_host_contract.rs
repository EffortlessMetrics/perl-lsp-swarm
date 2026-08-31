// Shared fail-closed host-execution contract (#10894).
//
// Each fixture reproduces one instrument defect the host corpus historically
// shipped independently, then proves the shared primitive in
// `xtask::editor_host` makes that defect impossible. These tests never launch
// a real editor: they exercise the execution/receipt/cleanup laws directly so
// every current and future host driver inherits them from one authority.

#![allow(dead_code)]

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use xtask::editor_client_compat::CleanupResult;
use xtask::editor_host::{
    BoundedExitClass, CleanupGuard, FacetState, FreshReceiptTarget, HostRunOutcome, PathRedaction,
    ProbeCapture, ProcessProbeLine, bound_capture, judge_cleanup, new_run_nonce, redact_bytes,
    require_executable, sha256_bytes, surviving_processes,
};

fn repo_test_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Unused today except where a platform guard needs the manifest dir root;
/// keeping one helper avoids per-test duplication.
fn _scratch_base() -> std::path::PathBuf {
    repo_test_root().join("target").join("editor-host-contract")
}

// ---------------------------------------------------------------------------
// Historical bug 1: PID sets compared lexicographically after numeric
// normalization produce false leak findings (`10` < `100` < `2`).
// ---------------------------------------------------------------------------

/// Reproduce the historical comparator: sort normalized `pid args` lines as
/// text and diff the textual sets. `pad` emulates one probe flavor emitting a
/// width-padded pid column while the other emits plain pids — identical
/// process sets, differing bytes.
fn lexicographic_survivors(
    before: &[ProcessProbeLine],
    after: &[ProcessProbeLine],
    needle: &str,
    pad: bool,
) -> Vec<String> {
    let render = |line: &ProcessProbeLine| {
        if pad {
            format!("{:>6} {}", line.pid, line.args)
        } else {
            format!("{} {}", line.pid, line.args)
        }
    };
    let mut before_lines: Vec<String> =
        before.iter().filter(|line| line.args.contains(needle)).map(&render).collect();
    let mut after_lines: Vec<String> = after
        .iter()
        .filter(|line| line.args.contains(needle))
        .map(|line| {
            // The after snapshot uses the other column convention.
            if pad {
                format!("{} {}", line.pid, line.args)
            } else {
                format!("{:>6} {}", line.pid, line.args)
            }
        })
        .collect();
    before_lines.sort();
    after_lines.sort();
    after_lines.iter().filter(|line| !before_lines.contains(line)).cloned().collect()
}

#[test]
fn pid_set_comparison_is_numeric_and_never_false_diffs() -> anyhow::Result<()> {
    // The same three-process set, where PIDs 2/10/100 sort differently as
    // numbers than as text.
    let before = vec![
        ProcessProbeLine { pid: 2, args: "vim /tmp/host".into() },
        ProcessProbeLine { pid: 10, args: "vim /tmp/host".into() },
        ProcessProbeLine { pid: 100, args: "perllsp --stdio".into() },
    ];
    let after = before.clone();
    // The historical comparator reports false survivors purely from the
    // column-convention difference (and lexicographic ordering of 10<100<2):
    // that is the defect this substrate exists to prevent.
    let historical_false_leak = lexicographic_survivors(&before, &after, "/tmp/host", true);
    assert!(
        !historical_false_leak.is_empty(),
        "fixture intent broken: lexicographic comparison was expected to false-diff"
    );
    let survivors = surviving_processes(&before, &after, "/tmp/host");
    assert!(survivors.is_empty(), "numeric comparison fabricated leaks: {survivors:?}");
    // Set equality survives any row order.
    let reordered = vec![after[2].clone(), after[0].clone(), after[1].clone()];
    assert!(surviving_processes(&before, &reordered, "/tmp/host").is_empty());
    // And it still finds a real survivor regardless of column ordering.
    let leaked = vec![
        ProcessProbeLine { pid: 2, args: "vim /tmp/host".into() },
        ProcessProbeLine { pid: 4242, args: "perllsp --stdio at /tmp/host".into() },
    ];
    let real = surviving_processes(&before, &leaked, "/tmp/host");
    assert_eq!(real.len(), 1);
    assert_eq!(real[0].pid, 4242);
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical bug 2: persistent receipt paths accepted by existence alone.
// ---------------------------------------------------------------------------

#[test]
fn pre_existing_receipt_is_refused_as_stale() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let receipt_path = dir.path().join("receipt.json");
    fs::write(&receipt_path, br#"{"schema_version":"editor_client_compat.v1"}"#)?;
    let error =
        FreshReceiptTarget::reserve(receipt_path.clone(), "sha256:".to_string() + &"a".repeat(64))
            .err()
            .map(|value| format!("{value:#}"))
            .ok_or_else(|| anyhow::anyhow!("reserve accepted a pre-existing receipt path"))?;
    assert!(
        error.contains("already exists") && error.contains("stale_receipt"),
        "refusal must name the stale-receipt law, got: {error}"
    );
    Ok(())
}

#[test]
fn fresh_reservation_writes_once_and_rebinds_nonce_per_run() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let receipt_path = dir.path().join("receipt.json");
    let first =
        FreshReceiptTarget::reserve(receipt_path.clone(), "sha256:".to_string() + &"a".repeat(64))?;
    let subject_digest = "sha256:".to_string() + &"b".repeat(64);
    first.write(br#"{"run":1}"#)?;
    // A second run reserving the same path hits the stale-refusal law even
    // though the file looks like a perfectly valid receipt.
    let second_error = FreshReceiptTarget::reserve(receipt_path.clone(), subject_digest)
        .err()
        .map(|value| format!("{value:#}"))
        .ok_or_else(|| anyhow::anyhow!("second reservation overtook a live receipt"))?;
    assert!(second_error.contains("already exists"));
    assert!(!first.nonce().is_empty());
    assert_eq!(first.path(), receipt_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical bug 3: raw host processes launched without a parent-owned
// hard deadline hang the proof lane forever.
// ---------------------------------------------------------------------------

fn long_sleeper_command() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("ping");
        command.args(["-n", "30", "-w", "1000", "127.0.0.1"]);
        command
    } else {
        let mut command = Command::new("sleep");
        command.arg("17");
        command
    }
}

#[test]
fn deadline_fires_and_classifies_timeout_deterministically() -> anyhow::Result<()> {
    let started = Instant::now();
    let observation = xtask::editor_host::bounded_run(
        &mut long_sleeper_command(),
        1_500,
        "contract-timeout-subject",
    )?;
    let elapsed = started.elapsed();
    assert!(observation.timed_out, "deadline did not fire");
    assert!(observation.kill_requested, "kill was not requested at the deadline");
    assert_eq!(observation.exit_class(), BoundedExitClass::TimedOut);
    assert!(!observation.orderly_success(), "a killed subject is not an orderly exit");
    assert!(
        elapsed < Duration::from_secs(12),
        "bounded_run waited far beyond its deadline: {elapsed:?}"
    );
    // Captures stay separated and bounded even for the killed subject.
    let _stdout_bound = bound_capture(&observation.stdout);
    let _stderr_bound = bound_capture(&observation.stderr);
    Ok(())
}

#[test]
fn quick_success_subject_classifies_success() -> anyhow::Result<()> {
    // Each platform gets its own real shell: `cmd` does not exist on Unix and
    // `sh` does not exist on stock Windows.
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "exit 0"]);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c").arg("true");
        command
    };
    let observation = xtask::editor_host::bounded_run(&mut command, 30_000, "contract-ok-subject")?;
    assert_eq!(observation.exit_class(), BoundedExitClass::Success);
    assert!(observation.orderly_success());
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical bug 4: cleanup represented by client events while OS processes
// stayed alive; probes unavailable silently judged pass.
// ---------------------------------------------------------------------------

#[test]
fn surviving_child_process_fails_cleanup_regardless_of_events() -> anyhow::Result<()> {
    // The pid-2 candidate is present in both captures (this run's pre-existing
    // state); only the pid-4242 candidate is new since the before-snapshot.
    let before = ProbeCapture::Captured("  2 /checkout/target/debug/perllsp --stdio\n".to_string());
    let after = ProbeCapture::Captured(
        "  2 /checkout/target/debug/perllsp --stdio\n \
             4242 /checkout/target/debug/perllsp --stdio\n"
            .to_string(),
    );
    let judgment = judge_cleanup(&before, &after, "/checkout/target/debug/perllsp", false, true);
    assert_eq!(judgment.result, CleanupResult::Fail);
    assert_eq!(judgment.survivors.len(), 1);
    assert_eq!(judgment.survivors[0].pid, 4242);
    // A client-emitted shutdown event cannot repair an observed OS leak: the
    // judgment is made purely from process-set evidence.
    assert!(judgment.detail.contains("surviving candidate process"));
    Ok(())
}

/// A helper process sharing the candidate's path prefix must not be absorbed
/// by substring matching: needle matching holds at component boundaries only
/// (P1 review finding on #12794).
#[test]
fn decoy_prefix_process_cannot_absorb_the_candidate_needle() {
    let needle = "/checkout/target/debug/perllsp";
    let before =
        vec![ProcessProbeLine { pid: 2, args: "/checkout/target/debug/perllsp --stdio".into() }];
    let after = vec![
        ProcessProbeLine { pid: 2, args: "/checkout/target/debug/perllsp --stdio".into() },
        // Only the decoy appears after the run: it must NOT count as a leaked
        // candidate, nor hide the clean verdict behind a fabricated survivor.
        ProcessProbeLine { pid: 7, args: "/checkout/target/debug/perllsp-helper serve".into() },
    ];
    let survivors = surviving_processes(&before, &after, needle);
    assert!(
        survivors.is_empty(),
        "a prefix-sharing decoy must never match the candidate needle: {survivors:?}"
    );
}

/// Failed boundary occurrences are skipped one character at a time; the scan
/// must cross multi-byte descriptions without ever slicing mid-character
/// (reachable panic in an earlier draft advanced by raw byte offset), while a
/// clean component-bounded occurrence still matches.
#[test]
fn multibyte_descriptions_never_panic_and_component_matching_still_holds() {
    let needle = "测试";
    let decoys = vec![
        ProcessProbeLine { pid: 5, args: "αβ测试 serve".into() },
        ProcessProbeLine { pid: 6, args: "测试-后缀 --standby".into() },
    ];
    let real = vec![ProcessProbeLine { pid: 9, args: "α 测试 --stdio".into() }];
    for line in &decoys {
        assert!(
            surviving_processes(&[], std::slice::from_ref(line), needle).is_empty(),
            "decoy {line:?} must not match"
        );
    }
    let survivors = surviving_processes(&[], &real, needle);
    assert_eq!(survivors.len(), 1);
    assert_eq!(survivors[0].pid, 9);
}

#[test]
fn empty_probe_captures_are_instrument_failure_not_clean() -> anyhow::Result<()> {
    // Zero rows from a successful probe command cannot mean "no processes":
    // the live run itself is always present. Empty captures must degrade to
    // not_proven, never pass (P1 review finding on #12794).
    let error = xtask::editor_host::parse_process_snapshot("")
        .err()
        .map(|value| format!("{value:#}"))
        .ok_or_else(|| anyhow::anyhow!("an empty snapshot must be rejected"))?;
    assert!(error.contains("zero rows"), "typed empty-snapshot refusal required: {error}");

    let judgment = judge_cleanup(
        &ProbeCapture::Captured(" 1 init\n".to_string()),
        &ProbeCapture::Captured(String::new()),
        "init",
        false,
        true,
    );
    assert_eq!(judgment.result, CleanupResult::NotProven);

    let judgment = judge_cleanup(
        &ProbeCapture::Captured(String::new()),
        &ProbeCapture::Captured(" 1 init\n".to_string()),
        "init",
        false,
        true,
    );
    assert_eq!(judgment.result, CleanupResult::NotProven);
    Ok(())
}

#[test]
fn unavailable_probe_degrades_to_not_proven_never_pass() -> anyhow::Result<()> {
    let judgment = judge_cleanup(
        &ProbeCapture::Unavailable,
        &ProbeCapture::Captured(" 2 vim /tmp/host\n".to_string()),
        "/tmp/host",
        false,
        true,
    );
    assert_eq!(judgment.result, CleanupResult::NotProven);
    assert!(
        judgment.detail.contains("refused"),
        "detail must name the refusal: {}",
        judgment.detail
    );

    let failed_probe = ProbeCapture::Failed("tasklist exited 1".to_string());
    let judgment = judge_cleanup(
        &failed_probe,
        &ProbeCapture::Captured(String::new()),
        "/tmp/host",
        false,
        true,
    );
    assert_eq!(judgment.result, CleanupResult::NotProven);
    Ok(())
}

#[test]
fn forced_kill_demotes_clean_process_set_to_not_proven() -> anyhow::Result<()> {
    let before = ProbeCapture::Captured(" 2 vim /tmp/host\n".to_string());
    let after = ProbeCapture::Captured(" 2 vim /tmp/host\n".to_string());
    // Clean set but no orderly exit: the driver shutdown never ran.
    let judgment = judge_cleanup(&before, &after, "/tmp/host", false, false);
    assert_eq!(judgment.result, CleanupResult::NotProven);
    assert!(judgment.detail.contains("skipped the driver shutdown path"));
    // With an orderly exit the same evidence judges clean.
    let judgment = judge_cleanup(&before, &after, "/tmp/host", false, true);
    assert_eq!(judgment.result, CleanupResult::Pass);
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical bug 5: reporter failures able to hide whether the product,
// instrument, or cleanup actually failed.
// ---------------------------------------------------------------------------

#[test]
fn reporting_failure_never_erases_product_or_instrument_disposition() -> anyhow::Result<()> {
    let outcome = HostRunOutcome {
        product: FacetState::Pass,
        instrument: FacetState::Pass,
        reporting: FacetState::Fail("receipt rename refused".into()),
        cleanup: CleanupResult::Pass,
        environment_detail: None,
    };
    let (overall, class) = outcome.judge();
    assert_eq!(overall, xtask::editor_client_compat::ObservationResult::Fail);
    assert_eq!(class, Some(xtask::editor_client_compat::FailureClass::Instrument));
    // The isolation law: facets survive the judgment unchanged, so a reporter
    // defect cannot convert an honest pass (or failure) elsewhere into silence.
    assert_eq!(outcome.product, FacetState::Pass);
    assert_eq!(outcome.instrument, FacetState::Pass);

    // A product failure outranks a reporting degradation and keeps its class.
    let outcome = HostRunOutcome {
        product: FacetState::Fail("rename cell found stale diagnostics".into()),
        instrument: FacetState::Pass,
        reporting: FacetState::NotProven("receipt writer unavailable".into()),
        cleanup: CleanupResult::Pass,
        environment_detail: None,
    };
    let (overall, class) = outcome.judge();
    assert_eq!(overall, xtask::editor_client_compat::ObservationResult::Fail);
    assert_eq!(class, Some(xtask::editor_client_compat::FailureClass::Product));
    Ok(())
}

#[test]
fn all_pass_facets_with_clean_cleanup_judge_pass() -> anyhow::Result<()> {
    let outcome = HostRunOutcome {
        product: FacetState::Pass,
        instrument: FacetState::Pass,
        reporting: FacetState::Pass,
        cleanup: CleanupResult::Pass,
        environment_detail: None,
    };
    let (overall, class) = outcome.judge();
    assert_eq!(overall, xtask::editor_client_compat::ObservationResult::Pass);
    assert_eq!(class, None);
    // A degraded cleanup facet independently blocks the overall pass.
    let degraded = HostRunOutcome { cleanup: CleanupResult::NotProven, ..outcome };
    let (overall, class) = degraded.judge();
    assert_eq!(overall, xtask::editor_client_compat::ObservationResult::NotProven);
    assert_eq!(class, Some(xtask::editor_client_compat::FailureClass::Cleanup));
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical bug 6-class: missing infrastructure reported as skipped green.
// ---------------------------------------------------------------------------

#[test]
fn missing_host_executable_is_environment_failure_never_skip() -> anyhow::Result<()> {
    let absent =
        repo_test_root().join("target").join(format!("absent-perllsp-{}.exe", new_run_nonce()));
    let error = require_executable(&absent, "host")
        .err()
        .map(|value| format!("{value:#}"))
        .ok_or_else(|| anyhow::anyhow!("require_executable accepted an absent binary"))?;
    assert!(error.contains("environment failure"), "typed env failure required: {error}");
    let outcome = HostRunOutcome::environment_unavailable(error);
    let (overall, class) = outcome.judge();
    assert_eq!(overall, xtask::editor_client_compat::ObservationResult::NotProven);
    assert_eq!(class, Some(xtask::editor_client_compat::FailureClass::Environment));
    Ok(())
}

// ---------------------------------------------------------------------------
// Interruption: cleanup executes anyway and evidence survives first.
// ---------------------------------------------------------------------------

#[test]
fn interrupted_guard_still_cleans_and_retains_diagnostic() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let evidence_root = dir.path().join("evidence");
    let scratch = dir.path().join("scratch-profile");
    fs::create_dir_all(&scratch)?;
    fs::write(scratch.join("cached-state.bin"), b"junk")?;

    let guard = CleanupGuard::new(&evidence_root);
    guard.retain_diagnostic(
        "pre-run-diagnostic.json",
        br#"{"bounded":true}"#,
        &[PathRedaction { path: scratch.to_path_buf(), token: "<SCRATCH>" }],
    )?;
    drop(guard); // interrupted: dropped without finish()

    assert!(
        evidence_root.join("pre-run-diagnostic.json").is_file(),
        "evidence retained before cleanup must survive the interruption"
    );
    let interruption = evidence_root.join("host-run-interruption.json");
    assert!(interruption.is_file(), "interruption must leave a diagnostic artifact");
    let payload = fs::read_to_string(&interruption)?;
    assert!(payload.contains("\"interrupted\": true"));

    // And the explicit finish path reports its own journal without any
    // interruption marker.
    let mut guard2 = CleanupGuard::new(dir.path().join("evidence2"));
    let scratch2 = dir.path().join("scratch2");
    fs::create_dir_all(&scratch2)?;
    guard2.register_dir(&scratch2);
    let journal = guard2.finish();
    assert!(journal.complete(), "journal recorded failures: {:?}", journal.failures);
    assert!(!journal.interrupted);
    assert!(!scratch2.exists(), "finish must remove registered scratch dirs");
    Ok(())
}

// ---------------------------------------------------------------------------
// Redaction, bounding, digest spelling.
// ---------------------------------------------------------------------------

#[test]
fn redaction_is_longest_first_and_separator_agnostic() -> anyhow::Result<()> {
    let (home, nested) = if cfg!(windows) {
        (r"C:\Users\runner".to_string(), r"C:\Users\runner\checkout".to_string())
    } else {
        ("/home/runner".to_string(), "/home/runner/checkout".to_string())
    };
    let redactions = [
        PathRedaction { path: home.clone().into(), token: "<HOME>" },
        PathRedaction { path: std::path::PathBuf::from(&nested), token: "<CHECKOUT>" },
    ];
    // Native-separator capture: the longest (nested) path must win.
    let native = nested.to_string() + if cfg!(windows) { r"\perllsp.log" } else { "/perllsp.log" };
    let text = redact_bytes(native.as_bytes(), &redactions);
    assert!(text.starts_with("<CHECKOUT>"), "longest path must win: {text}");
    assert!(!text.contains(&home));
    // Cross-separator capture (a POSIX-styled path embedded on any platform):
    // the dual-variant replacement still redacts the nested prefix.
    let posix = format!("{}/checkout/perllsp.log", home.replace('\\', "/"));
    let text = redact_bytes(posix.as_bytes(), &redactions);
    assert!(text.starts_with("<CHECKOUT>"), "slash form must redact too: {text}");
    Ok(())
}

#[test]
fn captures_are_hard_bounded_at_one_mebibyte() {
    let big = vec![b'x'; 1024 * 1024 + 7];
    assert_eq!(bound_capture(&big).len(), 1024 * 1024);
}

#[test]
fn digests_use_canonical_sha256_spelling() -> anyhow::Result<()> {
    let digest = sha256_bytes(b"perl-lsp")?;
    assert!(digest.starts_with("sha256:"));
    assert_eq!(digest.len(), "sha256:".len() + 64);
    Ok(())
}

/// Windows image names are case-insensitive end to end: a candidate path that
/// spells `PERLLSP.EXE` differently from `tasklist`'s reported image must
/// still be attributed to the run, or a surviving server reads as a clean
/// process set (P1 review finding on #12794). Non-Windows matching stays
/// exact-case, pinned by the pid test above.
#[cfg(windows)]
#[test]
fn windows_image_name_case_variance_still_matches_needle() -> anyhow::Result<()> {
    let before = vec![ProcessProbeLine { pid: 2, args: "perllsp.exe --stdio".into() }];
    let after = vec![
        ProcessProbeLine { pid: 2, args: "perllsp.exe --stdio".into() },
        ProcessProbeLine { pid: 9, args: "PERLLSP.EXE --stdio".into() },
    ];
    let survivors = surviving_processes(&before, &after, "perllsp.exe");
    assert_eq!(survivors.len(), 1, "case variance must not hide a survivor");
    assert_eq!(survivors[0].pid, 9);
    Ok(())
}
