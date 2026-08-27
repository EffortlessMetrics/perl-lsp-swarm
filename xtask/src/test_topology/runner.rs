//! Execution driver producing structured nonzero-work receipts (#12411).
//!
//! The runner is a thin, deterministic harness around one `cargo test`
//! invocation per routed row: it renders the row's declared argv verbatim,
//! enforces the row budget with a hard kill, captures libtest summaries, and
//! writes one atomic receipt artifact per target keyed by candidate head.
//!
//! There is deliberately no retry loop: rerun-until-green laundering cannot
//! be expressed through this API, and receipts pin `retries` to zero so any
//! hand-forged positive counter is rejected at fan-in.

use crate::test_topology::model::{
    ExecutionKind, TopologyRegister, TopologyRow, RECEIPT_SCHEMA_VERSION,
};
use crate::test_topology::receipts::{
    LibTestCounters, ScopeNamespace, TestTopologyReceipt,
};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Structural retry ceiling; the engine never re-runs a failed route.
pub const RETRY_CEILING: u32 = 0;

/// Poll cadence while waiting under the row budget.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Sanitize a target id into one filesystem-safe artifact stem.
fn artifact_stem(target_id: &str) -> String {
    target_id
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' => ch,
            _ => '_',
        })
        .collect()
}

/// Receipt artifact path for one target.
pub fn receipt_path(receipts_dir: &Path, target_id: &str) -> PathBuf {
    receipts_dir.join(format!("{}.receipt.json", artifact_stem(target_id)))
}

/// Execute one active row against the current tree and persist its receipt.
///
/// The rendered command always starts at `root`. Captured stdout/stderr are
/// kept beside the receipt as `.stdout.log` / `.stderr.log`; the receipt
/// itself stays the only semantic evidence surface.
pub fn run_row(
    root: &Path,
    register: &TopologyRegister,
    row: &TopologyRow,
    head_sha: &str,
    base_sha: &str,
    namespace: ScopeNamespace,
    receipts_dir: &Path,
) -> Result<TestTopologyReceipt> {
    let Some(execution) = row.execution.clone() else {
        anyhow::bail!(
            "row {} is declared_pending and cannot execute; routing must refuse dormant targets",
            row.target_id
        );
    };
    let argv = render_argv(&execution);
    std::fs::create_dir_all(receipts_dir)
        .with_context(|| format!("create receipts directory {}", receipts_dir.display()))?;

    let stdout_path = receipts_dir
        .join(format!("{}.stdout.log", artifact_stem(&row.target_id)));
    let stderr_path = receipts_dir
        .join(format!("{}.stderr.log", artifact_stem(&row.target_id)));
    let stdout_file = std::fs::File::create(&stdout_path)
        .with_context(|| format!("create {}", stdout_path.display()))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .with_context(|| format!("create {}", stderr_path.display()))?;

    let started = Instant::now();
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .with_context(|| format!("spawn {}", argv.join(" ")))?;

    let budget = Duration::from_secs(row.budget_seconds.max(1));
    let mut timed_out = false;
    let mut status = None;
    while started.elapsed() < budget {
        match child.try_wait()? {
            Some(exit) => {
                status = Some(exit);
                break;
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    }
    if status.is_none() {
        // Hard budget enforcement: kill and reap, then report the timeout.
        let _killed = child.kill();
        status = Some(child.wait()?);
        timed_out = true;
    }
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let exit_ok = status.map(|exit| exit.success()).unwrap_or(false);

    let output =
        std::fs::read_to_string(&stdout_path).unwrap_or_default();

    let verdict = crate::test_topology::receipts::evaluate_run(
        row,
        &output,
        exit_ok,
        timed_out,
        None,
    );

    let summary_work = crate::test_topology::receipts::parse_libtest_summaries(&output)
        .map(LibTestCounters::from)
        .unwrap_or_default();

    let receipt = TestTopologyReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
        cohort: register.cohort.cohort.clone(),
        target_id: row.target_id.clone(),
        head_sha: head_sha.to_owned(),
        base_sha: base_sha.to_owned(),
        namespace: namespace.tag().to_owned(),
        route_class: row.route_class.tag().to_owned(),
        command: argv.join(" "),
        work: summary_work,
        duration_ms,
        budget_seconds: row.budget_seconds,
        retries: RETRY_CEILING,
        verdict,
    };

    write_receipt_atomic(receipts_dir, &receipt)?;
    Ok(receipt)
}

fn render_argv(execution: &ExecutionKind) -> Vec<String> {
    execution.render_argv()
}

/// Write one receipt atomically (temp file + rename) into `receipts_dir`.
pub fn write_receipt_atomic(
    receipts_dir: &Path,
    receipt: &TestTopologyReceipt,
) -> Result<()> {
    std::fs::create_dir_all(receipts_dir)?;
    let final_path = receipt_path(receipts_dir, &receipt.target_id);
    let temp_path =
        receipts_dir.join(format!(".{}.tmp", final_path.file_name().unwrap_or_default().to_string_lossy()));
    let body = serde_json::to_vec_pretty(receipt)?;
    {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&temp_path)
            .with_context(|| format!("create {}", temp_path.display()))?;
        file.write_all(&body)?;
        file.flush()?;
    }
    std::fs::rename(&temp_path, &final_path)
        .with_context(|| format!("persist receipt {}", final_path.display()))?;
    Ok(())
}

/// Execute every selected active required row of the scope.
///
/// Returns all receipts written this run. Dormant rows must have been
/// refused by routing before calling; this helper refuses them again here
/// so both layers independently fail closed.
#[allow(clippy::too_many_arguments)]
pub fn run_selected_rows(
    root: &Path,
    register: &TopologyRegister,
    rows: &[&TopologyRow],
    head_sha: &str,
    base_sha: &str,
    namespace: ScopeNamespace,
    receipts_dir: &Path,
) -> Result<Vec<TestTopologyReceipt>> {
    let mut receipts = Vec::new();
    for row in rows {
        if matches!(row.status, crate::test_topology::model::TargetStatus::DeclaredPending) {
            anyhow::bail!(
                "refusing to route declared_pending target {}",
                row.target_id
            );
        }
        receipts.push(run_row(
            root,
            register,
            row,
            head_sha,
            base_sha,
            namespace,
            receipts_dir,
        )?);
    }
    Ok(receipts)
}
