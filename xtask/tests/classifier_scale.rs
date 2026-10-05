//! P3 classifier-scale bench (#17154).
//!
//! The lane-termination classifier (`scripts/ci/classify-ripr-lane-termination`)
//! caps every log scan at 16 MiB (`MAX_BYTES_DEFAULT`) and fails closed on a
//! truncated scan: a capped scan can never rule out a genuine gap receipt past
//! the cut, so it reports `ripr-failure` no matter which markers the scanned
//! prefix holds. This bench proves the full-scan side of that contract at
//! scale: a realistic lane log just under the cap, with the terminal receipt
//! line as its LAST line, still classifies correctly — and quickly — through
//! the real script in log-file mode, exactly as `ripr.yml` invokes it.
//!
//! Harness (unix; mirrors `ripr_e1_gap_precision_recall.rs` Harness 2):
//! synthesize a deterministic ~16 MiB lane log (timestamped cargo/test filler
//! plus one terminal `quality gate failed; see receipt ...` line), stage it in
//! a hermetic temp dir, and run the real bash classifier over it 3 times,
//! timing each spawn-to-output wall interval. Every run must report
//! `classification=ripr-failure` with `gap_receipt_matches=1`,
//! `partial_read=false`, `bytes_scanned` equal to the staged file length, and
//! zero teardown counters. The counters — not just the class — prove the scan
//! reached the final line, because a truncated scan would also report
//! `ripr-failure` but with `gap_receipt_matches=0`. The slowest of the 3 runs
//! must stay under `SCALE_WALL_BOUND`. The classifier is bash-only, so
//! non-unix runs report SKIP (CI enforces the unix path).
//!
//! Bound provenance: 3 measured unix runs of 54.6 ms, 52.1 ms, 45.4 ms
//! (16,773,286-byte log, 205,177 lines; WSL2 Ubuntu on the dev host,
//! 2026-10-05) ×4 headroom → 218.4 ms, rounded up to the 250 ms
//! `SCALE_WALL_BOUND`. The slowest run seen across all bench executions
//! (60.2 ms, during mutant A's kill run) still clears the bound at 4.2×, and
//! a 10x regression (~550 ms) exceeds it loudly.
//!
//! Mutants: 2 attempted, 2 kills — shrinking the bound to 1 ms fails at the
//! slowest-run assertion (`took 60.2ms, bound is 1ms`), and truncating the
//! staged log mid-body fails at the `gap_receipt_matches` pin (`left: "0",
//! right: "1"`) while the class coincidentally stays `ripr-failure`,
//! proving the counters — not just the class — discriminate a lost receipt.
//! Both mutants were reverted after their kill runs.
//!
//! NOT_PROVEN boundaries: absolute CI-runner speed (the ×4 headroom absorbs
//! machine skew; the bound is documented, not adaptive), api-evidence and
//! stdin modes at scale (log-file mode is the gate's hot path), and over-cap
//! fail-closed at scale (E1's `mutant-truncated-log-pass` pins the truncation
//! rule at small scale).

// The bench prints its per-run score lines; the workspace-wide print denial
// is a production-code rule.
#![allow(clippy::print_stdout)]

use anyhow::Result;
#[cfg(unix)]
use anyhow::{Context, bail};
#[cfg(unix)]
use perl_tdd_support::must_some_with;
#[cfg(unix)]
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    time::{Duration, Instant},
};

/// Byte cap the classifier enforces on every log scan, mirrored from
/// `MAX_BYTES_DEFAULT` in `scripts/ci/classify-ripr-lane-termination`.
#[cfg(unix)]
const CLASSIFIER_CAP_BYTES: u64 = 16 * 1024 * 1024;

/// Margin between the synthesized lane log and the classifier cap: the log
/// must be a genuine ~16 MiB full-scan workout while staying strictly under
/// the cap so `partial_read` stays `false`.
#[cfg(unix)]
const LANE_LOG_CAP_MARGIN_BYTES: u64 = 4096;

/// Timed classification runs per bench execution. Three runs separate a
/// one-off scheduling spike from a real regression: every run must classify
/// correctly, and the slowest must clear the bound.
#[cfg(unix)]
const SCALE_RUNS: u32 = 3;

/// Slowest single full-scan classification the bench tolerates: 4× the slowest
/// measured unix run (54.6 ms → 218.4 ms, rounded up to 250 ms), so ordinary
/// machine skew passes with room while a 10x regression (~550 ms) fails
/// loudly. See the module docs for the measured runs.
#[cfg(unix)]
const SCALE_WALL_BOUND: Duration = Duration::from_millis(250);

#[cfg(unix)]
fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask manifest must be nested under repo root")
}

#[cfg(unix)]
fn classifier_script() -> Result<PathBuf> {
    let script = repo_root()?.join("scripts").join("ci").join("classify-ripr-lane-termination");
    if !script.is_file() {
        bail!("classifier script is missing: {}", script.display());
    }
    Ok(script)
}

#[cfg(unix)]
fn parse_classifier_output(output: &str) -> BTreeMap<String, String> {
    // First occurrence wins, matching the self-test's `sed ... | head -1`.
    let mut fields = BTreeMap::new();
    for line in output.lines() {
        if let Some((key, value)) = line.split_once('=')
            && !key.is_empty()
            && !fields.contains_key(key)
        {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

/// Synthesize a deterministic lane log of just over `target_bytes` plus one
/// terminal receipt line. Filler rotates through timestamped cargo/test shapes
/// modeled on the `fixtures/ripr-gate/` lane logs; none of them may carry a
/// classifier marker (`quality gate failed; see receipt`, `The runner has
/// received a shutdown signal`, `Process completed with exit code 143.`, `The
/// operation was canceled`) — the harness pins every teardown counter at zero,
/// so marker-shaped filler fails the bench instead of hiding. Returns the log
/// body and its exact line count.
#[cfg(unix)]
fn synthesize_scale_lane_log(target_bytes: u64) -> (String, u64) {
    const PACKAGES: [&str; 4] = ["perl-parser", "perl-lexer", "perl-workspace", "perl-lsp-rs-core"];
    let mut body = String::with_capacity(target_bytes as usize + 1024);
    let mut lines = 0_u64;
    body.push_str(
        "2026-10-05T12:00:00Z ##[group]Run cargo xtask quality-gate --mode enforce-new-ripr\n",
    );
    lines += 1;
    let mut seq = 0_u64;
    while body.len() as u64 <= target_bytes {
        let package = PACKAGES[(seq as usize) % PACKAGES.len()];
        let minute = (seq / 60) % 60;
        let second = seq % 60;
        match seq % 4 {
            0 => {
                let _ = writeln!(
                    body,
                    "2026-10-05T12:{minute:02}:{second:02}Z test {package}::shape_{seq:06} ... ok"
                );
            }
            1 => {
                let _ = writeln!(
                    body,
                    "2026-10-05T12:{minute:02}:{second:02}Z {package}: 142 passed; 0 failed; 0 ignored; measured 0 filtered out; finished in 0.42s"
                );
            }
            2 => {
                let _ = writeln!(
                    body,
                    "2026-10-05T12:{minute:02}:{second:02}Z ##[group]Run cargo test -p {package} --lib"
                );
            }
            _ => {
                let _ = writeln!(
                    body,
                    "2026-10-05T12:{minute:02}:{second:02}Z Finished `test` profile [unoptimized + debuginfo] target(s) in 1.23s"
                );
            }
        }
        lines += 1;
        seq += 1;
    }
    // The terminal receipt line is the LAST line: the classifier must scan the
    // whole ~16 MiB to find it. The shape mirrors the `quality_gate.rs` bail
    // plus the lane timestamp prefix E1's fixtures carry.
    body.push_str(
        "2026-10-05T12:34:56Z Error: quality gate failed; see receipt target/receipts/quality/quality-gate.json and summary target/receipts/quality/quality-gate.md\n",
    );
    lines += 1;
    (body, lines)
}

/// Run the real classifier exactly as `ripr.yml` invokes it (`bash
/// scripts/ci/classify-ripr-lane-termination <logfile>`). Exit status is
/// always 0 for a performed classification; misuse (exit 64) is a harness
/// failure.
#[cfg(unix)]
fn classify_log_file(log: &Path, case: &str) -> Result<BTreeMap<String, String>> {
    use std::process::Stdio;

    let mut command = StdCommand::new("bash");
    command.arg(classifier_script()?).arg(log);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output =
        command.output().with_context(|| format!("p3 `{case}` classifier spawn failed"))?;
    if !output.status.success() {
        bail!(
            "p3 `{case}` classifier exited {} (usage error): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_classifier_output(&String::from_utf8_lossy(&output.stdout)))
}

/// Three timed full-scan classifications against one staged ~16 MiB log.
/// Every run pins the class plus the full-scan evidence counters; the slowest
/// run must clear `SCALE_WALL_BOUND`.
#[cfg(unix)]
#[allow(clippy::too_many_lines)]
fn execute_scale_matrix() -> Result<()> {
    let stage = tempfile::tempdir().context("p3 creating scale stage tempdir")?;
    let target_bytes = CLASSIFIER_CAP_BYTES - LANE_LOG_CAP_MARGIN_BYTES;
    let (body, line_count) = synthesize_scale_lane_log(target_bytes);
    let byte_count = body.len() as u64;
    // The log must hug the cap (a genuine scale workout) without touching it:
    // at or over the cap the classifier fails closed with `partial_read=true`
    // and the full-scan pins below would fail.
    let floor_bytes = CLASSIFIER_CAP_BYTES - 64 * 1024;
    assert!(
        byte_count > floor_bytes && byte_count < CLASSIFIER_CAP_BYTES,
        "p3 staged log must sit just under the {CLASSIFIER_CAP_BYTES}-byte cap, got {byte_count} bytes"
    );
    let log_path = stage.path().join("lane-scale.log");
    fs::write(&log_path, &body).with_context(|| format!("p3 writing {}", log_path.display()))?;

    let mut worst = Duration::ZERO;
    for ordinal in 1..=SCALE_RUNS {
        let started = Instant::now();
        let fields = classify_log_file(&log_path, "p3-scale")?;
        let elapsed = started.elapsed();
        worst = worst.max(elapsed);

        let classification = must_some_with(
            fields.get("classification"),
            "p3 classifier output must carry `classification`",
        )
        .to_string();
        let verdict =
            must_some_with(fields.get("verdict"), "p3 classifier output must carry `verdict`")
                .to_string();
        let gap_hits = must_some_with(
            fields.get("gap_receipt_matches"),
            "p3 classifier output must carry `gap_receipt_matches`",
        )
        .to_string();
        let shutdown_hits = must_some_with(
            fields.get("shutdown_signal_matches"),
            "p3 classifier output must carry `shutdown_signal_matches`",
        )
        .to_string();
        let term143_hits = must_some_with(
            fields.get("sigterm143_matches"),
            "p3 classifier output must carry `sigterm143_matches`",
        )
        .to_string();
        let cancel_hits = must_some_with(
            fields.get("op_cancelled_matches"),
            "p3 classifier output must carry `op_cancelled_matches`",
        )
        .to_string();
        let partial = must_some_with(
            fields.get("partial_read"),
            "p3 classifier output must carry `partial_read`",
        )
        .to_string();
        let scanned = must_some_with(
            fields.get("bytes_scanned"),
            "p3 classifier output must carry `bytes_scanned`",
        )
        .to_string();
        let scanned_lines = must_some_with(
            fields.get("log_lines_scanned"),
            "p3 classifier output must carry `log_lines_scanned`",
        )
        .to_string();

        assert_eq!(
            classification, "ripr-failure",
            "p3 run {ordinal}: terminal receipt must classify blocking red: {fields:?}"
        );
        assert_eq!(
            verdict, "ripr-failure",
            "p3 run {ordinal}: verdict alias must match: {fields:?}"
        );
        assert_eq!(
            gap_hits, "1",
            "p3 run {ordinal}: the receipt on the last line must be found (a truncated scan would report 0): {fields:?}"
        );
        assert_eq!(
            shutdown_hits, "0",
            "p3 run {ordinal}: filler must carry no shutdown marker: {fields:?}"
        );
        assert_eq!(
            term143_hits, "0",
            "p3 run {ordinal}: filler must carry no exit-143 marker: {fields:?}"
        );
        assert_eq!(
            cancel_hits, "0",
            "p3 run {ordinal}: filler must carry no cancellation marker: {fields:?}"
        );
        assert_eq!(
            partial, "false",
            "p3 run {ordinal}: the {byte_count}-byte log must scan fully under the {CLASSIFIER_CAP_BYTES}-byte cap: {fields:?}"
        );
        assert_eq!(
            scanned,
            byte_count.to_string(),
            "p3 run {ordinal}: bytes_scanned must equal the staged length (full-scan proof): {fields:?}"
        );
        assert_eq!(
            scanned_lines,
            line_count.to_string(),
            "p3 run {ordinal}: every staged line must be scanned: {fields:?}"
        );
        println!(
            "p3 run {ordinal}/{SCALE_RUNS} elapsed={elapsed:?} classification={classification} gap={gap_hits} partial={partial} | MATCH"
        );
    }
    assert!(
        worst < SCALE_WALL_BOUND,
        "p3 slowest of {SCALE_RUNS} full-scan classifications took {worst:?}, bound is {SCALE_WALL_BOUND:?} (a 10x regression must fail here; machine skew is absorbed by the x4 headroom)"
    );
    println!(
        "p3 scale | bytes={byte_count} lines={line_count} runs={SCALE_RUNS} worst={worst:?} bound={SCALE_WALL_BOUND:?} | MATCH"
    );
    Ok(())
}

#[cfg(not(unix))]
fn execute_scale_matrix() -> Result<()> {
    println!(
        "SKIP p3_classifier_scale: the lane-termination classifier is bash-only; enforced on unix CI"
    );
    Ok(())
}

#[test]
fn p3_classifier_scale_full_scan_within_bound() -> Result<()> {
    execute_scale_matrix()
}
