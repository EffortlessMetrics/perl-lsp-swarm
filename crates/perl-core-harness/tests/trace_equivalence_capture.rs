//! Hermetic paired exact-process proof for the ordinary/instrumented
//! equivalence receipt (#12286).
//!
//! The ordinary side runs the landed #12283 capture route against the
//! observe-fixture stand-in for upstream `t/TEST`; the instrumented side
//! runs the landed #12285 route against the trace-fixture on an identical
//! prepared-tree layout. The comparator must grant exactly the bounded
//! transfer relation for the clean pair and must type drift as a measured
//! difference or non-comparability, never as hidden parity.
//!
//! The pinned real-tree pair (prepared upstream `t/TEST` at perl-5.42.2)
//! remains explicitly unproven until both sides are captured on a real
//! prepared tree; nothing here fabricates it.

use color_eyre::eyre::{Result, bail};
use perl_core_harness::artifacts::CaptureLimits;
use perl_core_harness::invocation_trace::{
    EXACT_PATCH_SCHEMA_VERSION, ExactPatchOp, ExactPatchSpec, InstrumentedObservation,
    ObserveInvocationsConfig,
};
use perl_core_harness::observed_discovery::capture::ObserveDiscoveryConfig;
use perl_core_harness::observed_discovery::{RunnerKind, observe_discovery};
use perl_core_harness::trace_equivalence::{
    EquivalenceVerdict, TraceEquivalenceInput, build_trace_equivalence, validate_trace_equivalence,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ORDINARY_ARTIFACT: &str = "#!./perl\n# hermetic stand-in for the pinned upstream t/TEST\n";
const ORDINARY_ANCHOR: &str = "# hermetic stand-in for the pinned upstream t/TEST";
const INSTRUMENTATION_ID: &str = "trace-equivalence-test-1";

fn observe_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-core-harness-observe-fixture"))
}

fn trace_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-core-harness-trace-fixture"))
}

fn matrix_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.ci/perl-core-harness/upstream-targets-5.42.2.v1")
}

fn limits() -> CaptureLimits {
    CaptureLimits { deadline: Duration::from_mins(1), cancel_file: None }
}

fn prepared_tree(root: &Path, marker: &str, mode: &str) -> Result<PathBuf> {
    let tree = root.join("prepared-perl");
    let t_dir = tree.join("t");
    fs::create_dir_all(t_dir.join("base"))?;
    fs::create_dir_all(t_dir.join("comp"))?;
    fs::create_dir_all(t_dir.join("run"))?;
    for member in ["if.t", "cond.t"] {
        fs::write(t_dir.join("base").join(member), "1;\n")?;
    }
    fs::write(t_dir.join("comp").join("require.t"), "1;\n")?;
    fs::write(t_dir.join("run").join("exit.t"), "1;\n")?;
    fs::write(t_dir.join("TEST"), ORDINARY_ARTIFACT)?;
    fs::write(t_dir.join(marker), mode)?;
    Ok(tree)
}

fn ordinary_side(
    root: &Path,
    mode: &str,
) -> Result<perl_core_harness::observed_discovery::model::UpstreamDiscoveryReceiptV1> {
    let tree = prepared_tree(root, ".observe-fixture-mode", mode)?;
    let config = ObserveDiscoveryConfig {
        matrix: matrix_path(),
        target_id: "component_base".to_string(),
        runner: RunnerKind::Test,
        perl_tree: tree,
        host_perl: observe_fixture(),
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        output: root.join("ordinary-receipt.json"),
        limits: limits(),
    };
    observe_discovery(&config)
}

fn instrumented_side(root: &Path, mode: &str) -> Result<InstrumentedObservation> {
    let tree = prepared_tree(root, ".trace-fixture-mode", mode)?;
    let spec = ExactPatchSpec {
        schema_version: EXACT_PATCH_SCHEMA_VERSION.to_string(),
        runner: "test".to_string(),
        target_artifact: "t/TEST".to_string(),
        expected_ordinary_sha256: {
            use sha2::{Digest, Sha256};
            Sha256::digest(ORDINARY_ARTIFACT.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        },
        operations: vec![ExactPatchOp {
            label: "instrument-runtests-invocation-decision".to_string(),
            anchor: ORDINARY_ANCHOR.to_string(),
            replacement: format!(
                "{ORDINARY_ANCHOR}\n# instrumented at the t/TEST runtests invocation decision seam by {INSTRUMENTATION_ID}\n"
            ),
        }],
    };
    let patch = root.join("patch-spec.json");
    fs::write(&patch, serde_json::to_vec(&spec)?)?;
    let config = ObserveInvocationsConfig {
        matrix: matrix_path(),
        target_id: "component_base".to_string(),
        runner: RunnerKind::Test,
        perl_tree: tree,
        host_perl: trace_fixture(),
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        instrumentation_id: INSTRUMENTATION_ID.to_string(),
        patch,
        output: root.join("parent-receipt.json"),
        trace_output: root.join("trace-receipt.json"),
        work_output: root.join("work-receipt.json"),
        limits: limits(),
    };
    perl_core_harness::invocation_trace::observe_invocations(&config)
}

fn paired_input(ordinary_mode: &str, instrumented_mode: &str) -> Result<TraceEquivalenceInput> {
    let ordinary_temp = tempfile::tempdir()?;
    let instrumented_temp = tempfile::tempdir()?;
    let ordinary = ordinary_side(ordinary_temp.path(), ordinary_mode)?;
    let instrumented = instrumented_side(instrumented_temp.path(), instrumented_mode)?;
    let parent = instrumented
        .parent
        .clone()
        .ok_or_else(|| color_eyre::eyre::eyre!("instrumented parent must construct"))?;
    let trace = instrumented
        .trace
        .clone()
        .ok_or_else(|| color_eyre::eyre::eyre!("instrumented trace must construct"))?;
    // Both observations are complete and their receipts are assembled in
    // memory; dropping the tempdirs here only removes the stand-in trees.
    drop(ordinary_temp);
    drop(instrumented_temp);
    Ok(TraceEquivalenceInput {
        matrix: perl_core_harness::io::read_matrix(&matrix_path())
            .map_err(|error| color_eyre::eyre::eyre!(error))?,
        ordinary: Some(ordinary),
        instrumented_parent: parent,
        trace,
        work: instrumented.work.clone(),
    })
}

#[test]
fn clean_pair_grants_bounded_transfer() -> Result<()> {
    let input = paired_input("select", "clean")?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    if receipt.payload.verdict != EquivalenceVerdict::EquivalentForDeclaredPlanes {
        bail!(
            "clean pair must be equivalent for declared planes, got {:?} with differences {:?}",
            receipt.payload.verdict,
            receipt.payload.differences
        );
    }
    validate_trace_equivalence(&receipt).map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

#[test]
fn cli_rejects_nonlexical_output_alias_without_touching_inputs() -> Result<()> {
    // Negative security case: `--output <dir>/./ordinary.json` spells the
    // ordinary receipt non-lexically. The CLI must refuse before reading or
    // deleting anything, so the input receipt survives byte-for-byte.
    let ordinary_temp = tempfile::tempdir()?;
    let instrumented_temp = tempfile::tempdir()?;
    let ordinary_value = ordinary_side(ordinary_temp.path(), "select")?;
    let instrumented = instrumented_side(instrumented_temp.path(), "clean")?;
    let ordinary_receipt = ordinary_temp.path().join("ordinary-receipt.json");
    fs::write(&ordinary_receipt, serde_json::to_vec(&ordinary_value)?)?;
    fs::write(
        instrumented_temp.path().join("parent-receipt.json"),
        serde_json::to_vec(&instrumented.parent)?,
    )?;
    fs::write(
        instrumented_temp.path().join("trace-receipt.json"),
        serde_json::to_vec(&instrumented.trace)?,
    )?;
    fs::write(
        instrumented_temp.path().join("work-receipt.json"),
        serde_json::to_vec(&instrumented.work)?,
    )?;
    let before = fs::read(&ordinary_receipt)?;
    let alias = format!("{}/./ordinary-receipt.json", ordinary_temp.path().display());
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_perl-core-harness-artifacts"))
        .args([
            "compare-equivalence",
            "--matrix",
            &matrix_path().to_string_lossy(),
            "--ordinary",
            &ordinary_receipt.to_string_lossy(),
            "--instrumented-parent",
            &instrumented_temp.path().join("parent-receipt.json").to_string_lossy(),
            "--trace",
            &instrumented_temp.path().join("trace-receipt.json").to_string_lossy(),
            "--work",
            &instrumented_temp.path().join("work-receipt.json").to_string_lossy(),
            "--output",
            &alias,
        ])
        .status()?;
    assert!(!status.success(), "aliased output must be refused");
    assert_eq!(fs::read(&ordinary_receipt)?, before, "input receipt must survive");
    Ok(())
}

#[test]
fn cli_without_ordinary_emits_not_proven_receipt() -> Result<()> {
    // The missing-pair falsifier at the CLI surface: no --ordinary must
    // still produce the typed not_proven receipt, never a silent absence
    // and never a passing relation.
    let instrumented_temp = tempfile::tempdir()?;
    let instrumented = instrumented_side(instrumented_temp.path(), "clean")?;
    fs::write(
        instrumented_temp.path().join("parent-receipt.json"),
        serde_json::to_vec(&instrumented.parent)?,
    )?;
    fs::write(
        instrumented_temp.path().join("trace-receipt.json"),
        serde_json::to_vec(&instrumented.trace)?,
    )?;
    fs::write(
        instrumented_temp.path().join("work-receipt.json"),
        serde_json::to_vec(&instrumented.work)?,
    )?;
    let output = instrumented_temp.path().join("equivalence.json");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_perl-core-harness-artifacts"))
        .args([
            "compare-equivalence",
            "--matrix",
            &matrix_path().to_string_lossy(),
            "--instrumented-parent",
            &instrumented_temp.path().join("parent-receipt.json").to_string_lossy(),
            "--trace",
            &instrumented_temp.path().join("trace-receipt.json").to_string_lossy(),
            "--work",
            &instrumented_temp.path().join("work-receipt.json").to_string_lossy(),
            "--output",
            &output.to_string_lossy(),
        ])
        .status()?;
    assert!(status.success(), "missing-pair comparison must still emit its receipt");
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&output)?)?;
    assert_eq!(receipt["payload"]["verdict"], "not_proven");
    assert_eq!(receipt["payload"]["transferable"], serde_json::json!([]));
    Ok(())
}

#[test]
fn cli_accepts_bare_relative_output_name() -> Result<()> {
    // Regression: `--output equivalence.json` (no directory component)
    // must not abort in the alias check; the empty parent is the cwd.
    let instrumented_temp = tempfile::tempdir()?;
    let instrumented = instrumented_side(instrumented_temp.path(), "clean")?;
    fs::write(
        instrumented_temp.path().join("parent-receipt.json"),
        serde_json::to_vec(&instrumented.parent)?,
    )?;
    fs::write(
        instrumented_temp.path().join("trace-receipt.json"),
        serde_json::to_vec(&instrumented.trace)?,
    )?;
    fs::write(
        instrumented_temp.path().join("work-receipt.json"),
        serde_json::to_vec(&instrumented.work)?,
    )?;
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_perl-core-harness-artifacts"))
        .current_dir(instrumented_temp.path())
        .args([
            "compare-equivalence",
            "--matrix",
            &matrix_path().to_string_lossy(),
            "--instrumented-parent",
            &instrumented_temp.path().join("parent-receipt.json").to_string_lossy(),
            "--trace",
            &instrumented_temp.path().join("trace-receipt.json").to_string_lossy(),
            "--work",
            &instrumented_temp.path().join("work-receipt.json").to_string_lossy(),
            "--output",
            "equivalence.json",
        ])
        .status()?;
    assert!(status.success(), "bare relative output name must be accepted");
    assert!(instrumented_temp.path().join("equivalence.json").is_file());
    Ok(())
}

#[test]
fn stderr_bearing_ordinary_run_is_measured_difference() -> Result<()> {
    // Falsifier 5: instrumentation must not change ordinary stdout/stderr
    // beyond the isolated trace channel; stderr drift is measured, not hidden.
    let input = paired_input("select_stderr", "clean")?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::MeasuredDifference);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}

#[test]
fn failing_ordinary_run_is_measured_difference() -> Result<()> {
    let input = paired_input("select_fail", "clean")?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_ne!(receipt.payload.verdict, EquivalenceVerdict::EquivalentForDeclaredPlanes);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}
