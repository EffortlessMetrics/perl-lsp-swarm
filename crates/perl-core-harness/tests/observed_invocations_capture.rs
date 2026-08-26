//! Hermetic exact-process proof for the instrumented effective-invocation
//! capture route (#12285).
//!
//! Every observation in this suite runs a real supervised process: the
//! `perl-core-harness-trace-fixture` binary stands in for the prepared tree's
//! host Perl executing one EXACTLY PATCHED disposable `t/TEST` copy, verifies
//! the instrumented artifact digest the route recorded, performs the upstream
//! scan/classification itself, and emits #12284 row and terminal frames to
//! the private trace channel. The route under test copies the tree, applies
//! the reviewed exact-anchor patch, supervises the process, and assembles the
//! instrumented parent discovery receipt plus the trace receipt through the
//! landed strict constructors.
//!
//! The pinned real-tree rows (prepared upstream `t/TEST` at perl-5.42.2)
//! remain explicitly unproven until a real prepared tree is captured; nothing
//! here fabricates them.

use color_eyre::eyre::{Result, bail};
use perl_core_harness::artifacts::CaptureLimits;
use perl_core_harness::invocation_trace::SourceForm;
use perl_core_harness::invocation_trace::model::{
    EffectiveInvocationField, EffectiveInvocationTraceReceiptV1, InvocationObservationState,
    TraceRowDisposition, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use perl_core_harness::invocation_trace::{
    EXACT_PATCH_SCHEMA_VERSION, ExactPatchOp, ExactPatchSpec, InstrumentationState,
    InstrumentationWorkReceiptV1, ObserveInvocationsConfig, apply_exact_patch,
    check_invocation_trace_against, observe_invocations, observe_invocations_command,
    validate_instrumentation_work,
};
use perl_core_harness::observed_discovery::model::{
    DiscoveryObservationState, UpstreamDiscoveryReceiptV1,
};
use perl_core_harness::observed_discovery::{RunnerKind, check_observed_discovery_against};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ORDINARY_ARTIFACT: &str = "#!./perl\n# hermetic stand-in for the pinned upstream t/TEST\n";
const ORDINARY_ANCHOR: &str = "# hermetic stand-in for the pinned upstream t/TEST";
const INSTRUMENTATION_ID: &str = "trace-instrument-1";

fn fixture_host_perl() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-core-harness-trace-fixture"))
}

fn matrix_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".ci/perl-core-harness/upstream-targets-5.42.2.v1")
}

fn default_limits() -> CaptureLimits {
    CaptureLimits { deadline: Duration::from_secs(30), cancel_file: None }
}

/// One hermetic prepared tree: ordinary `t/TEST` artifact, base members, and
/// the trace-fixture drift-mode marker.
fn fixture_tree(root: &Path, mode: &str, base_members: &[&str]) -> Result<PathBuf> {
    let tree = root.join("prepared-perl");
    let t_dir = tree.join("t");
    fs::create_dir_all(t_dir.join("base"))?;
    fs::create_dir_all(t_dir.join("comp"))?;
    fs::create_dir_all(t_dir.join("run"))?;
    for member in base_members {
        fs::write(t_dir.join("base").join(member), "1;\n")?;
    }
    fs::write(t_dir.join("comp").join("require.t"), "1;\n")?;
    fs::write(t_dir.join("run").join("exit.t"), "1;\n")?;
    fs::write(t_dir.join("TEST"), ORDINARY_ARTIFACT)?;
    fs::write(t_dir.join(".trace-fixture-mode"), mode)?;
    Ok(tree)
}

/// The reviewed exact-anchor patch specification for the stand-in artifact.
fn write_patch_spec(root: &Path, ordinary_bytes: &str) -> Result<PathBuf> {
    let spec = ExactPatchSpec {
        schema_version: EXACT_PATCH_SCHEMA_VERSION.to_string(),
        runner: "test".to_string(),
        target_artifact: "t/TEST".to_string(),
        expected_ordinary_sha256: sha_hex(ordinary_bytes.as_bytes()),
        operations: vec![ExactPatchOp {
            label: "instrument-runtests-invocation-decision".to_string(),
            anchor: ORDINARY_ANCHOR.to_string(),
            replacement: format!(
                "{ORDINARY_ANCHOR}\n# instrumented at the t/TEST runtests invocation decision \
                 seam by {INSTRUMENTATION_ID}\n"
            ),
        }],
    };
    let path = root.join("patch-spec.json");
    fs::write(&path, serde_json::to_vec(&spec)?)?;
    Ok(path)
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn config_for(
    tree: &Path,
    patch: &Path,
    root: &Path,
    target_id: &str,
    limits: CaptureLimits,
) -> ObserveInvocationsConfig {
    ObserveInvocationsConfig {
        matrix: matrix_path(),
        target_id: target_id.to_string(),
        runner: RunnerKind::Test,
        perl_tree: tree.to_path_buf(),
        host_perl: fixture_host_perl(),
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        instrumentation_id: INSTRUMENTATION_ID.to_string(),
        patch: patch.to_path_buf(),
        output: root.join("parent-receipt.json"),
        trace_output: root.join("trace-receipt.json"),
        work_output: root.join("work-receipt.json"),
        limits,
    }
}

fn observe_with_mode(
    target_id: &str,
    mode: &str,
    base_members: &[&str],
) -> Result<perl_core_harness::invocation_trace::InstrumentedObservation> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), mode, base_members)?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), target_id, default_limits());
    observe_invocations(&config)
}

fn load_parent(path: &Path) -> Result<UpstreamDiscoveryReceiptV1> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_trace(path: &Path) -> Result<EffectiveInvocationTraceReceiptV1> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_work(path: &Path) -> Result<InstrumentationWorkReceiptV1> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

// ---------------------------------------------------------------------------
// Positive proof: one exact disposable instrumented t/TEST subject
// ---------------------------------------------------------------------------

#[test]
fn clean_capture_emits_binding_trace_receipts() -> Result<()> {
    let observation = observe_with_mode("component_base", "clean", &["if.t", "cond.t"])?;
    let parent = observation
        .parent
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("clean capture must construct the parent"))?;
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("clean capture must construct the trace"))?;
    let work = &observation.work;

    // Parent: the same strict #12281 construction, instrumented subject.
    assert_eq!(parent.payload.state, DiscoveryObservationState::ObservedComplete);
    assert_eq!(parent.payload.subject.instrumentation_id, Some(INSTRUMENTATION_ID.to_string()));
    assert_eq!(parent.payload.invocation.argv.len(), 3);
    assert_eq!(parent.payload.invocation.argv[0], "TEST");
    assert_eq!(parent.payload.invocation.argv[1], "--dumptests");
    assert_eq!(parent.payload.invocation.argv[2], "base");
    assert_eq!(parent.payload.invocation.working_directory, "t");
    let accepted: Vec<String> = parent
        .payload
        .rows
        .iter()
        .filter(|row| row.is_accepted())
        .filter_map(|row| row.canonical_path().map(str::to_string))
        .collect();
    assert_eq!(accepted, vec!["t/base/cond.t".to_string(), "t/base/if.t".to_string()]);

    // Trace: strict decode, complete rows, exact parent binding.
    assert_eq!(trace.schema_version, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION);
    assert!(trace.payload.trace_decode.is_complete());
    assert_eq!(trace.payload.rows.len(), 2);
    for row in &trace.payload.rows {
        assert_eq!(row.state, InvocationObservationState::ObservedComplete);
        assert!(row.disposition.is_accepted());
        assert_eq!(row.fields.state_counts().observed, 17);
        assert!(matches!(
            row.projection,
            perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
        ));
        assert_eq!(
            row.subject.parent_receipt_digest, parent.payload_digest,
            "every row binds the exact parent receipt"
        );
        assert!(parent.payload.rows.iter().any(|member| member.is_accepted()
            && member.canonical_path() == Some(row.subject.parent_member_path.as_str())));
    }
    assert_eq!(trace.payload.terminal.as_ref().map(|t| t.row_count), Some(2));

    // Ordinary stdout stays byte-exact member rows: the trace channel never
    // entered the discovery stream.
    let stdout_bytes = parent
        .payload
        .stdout
        .bytes()
        .map_err(|error| color_eyre::eyre::eyre!("decoding retained stdout: {error}"))?;
    assert_eq!(stdout_bytes, b"t/base/cond.t\nt/base/if.t\n");

    // Work receipt: identities distinct, counters honest, cleanup proven.
    assert_eq!(work.payload.state, InstrumentationState::ObservedComplete);
    assert_ne!(
        work.payload.ordinary_artifact.content_sha256,
        work.payload.instrumented_artifact.content_sha256,
        "ordinary and instrumented identities stay distinct and load-bearing"
    );
    assert_eq!(
        work.payload.ordinary_artifact.content_sha256,
        sha_hex(ORDINARY_ARTIFACT.as_bytes())
    );
    let work_counters = &work.payload.work;
    assert_eq!(work_counters.instrumented_processes, 1);
    assert_eq!(work_counters.trace_rows, 2);
    assert_eq!(work_counters.complete_rows, 2);
    assert_eq!(work_counters.canonical_plan_projections, 2);
    assert_eq!(work_counters.canonical_plan_projections_accepted, 2);
    assert_eq!(work_counters.ordinary_output_contamination_count, 0);
    assert_eq!(work_counters.fields_synthesized, 0);
    assert_eq!(work_counters.direct_rows_consumed, 0);
    assert_eq!(work_counters.terminal_disagreements, 0);
    assert_eq!(work_counters.cleanup_failures, 0);
    assert!(work.payload.cleanup.is_proven());
    assert_eq!(work.payload.work.manifest_files_changed, 1);

    // Both receipts reconstruct through the landed decoders.
    let matrix = perl_core_harness::io::read_matrix(&matrix_path())?;
    check_observed_discovery_against(&matrix, parent)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    check_invocation_trace_against(parent, trace)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let spec_raw = {
        let temp = tempfile::tempdir()?;
        let path = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
        fs::read_to_string(path)?
    };
    validate_instrumentation_work(work, ORDINARY_ARTIFACT.as_bytes(), &spec_from(&spec_raw))
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

fn spec_from(raw: &str) -> ExactPatchSpec {
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("fixture spec decodes: {error}"))
}

#[test]
fn command_writes_reconstructing_receipts() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "clean", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    observe_invocations_command(&config)?;
    assert!(config.output.is_file());
    assert!(config.trace_output.is_file());
    assert!(config.work_output.is_file());
    let parent = load_parent(&config.output)?;
    let trace = load_trace(&config.trace_output)?;
    let work = load_work(&config.work_output)?;
    assert_eq!(parent.payload.state, DiscoveryObservationState::ObservedComplete);
    assert!(trace.payload.trace_decode.is_complete());
    assert_eq!(work.payload.state, InstrumentationState::ObservedComplete);
    let matrix = perl_core_harness::io::read_matrix(&matrix_path())?;
    check_observed_discovery_against(&matrix, &parent)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    check_invocation_trace_against(&parent, &trace)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    validate_instrumentation_work(
        &work,
        ORDINARY_ARTIFACT.as_bytes(),
        &spec_from(&fs::read_to_string(&patch)?),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Discriminating invocation shapes stay distinct (falsifier 6)
// ---------------------------------------------------------------------------

#[test]
fn discriminating_invocation_shapes_stay_distinct() -> Result<()> {
    let members = [
        "plain.t",
        "probe_a_taintT.t",
        "probe_b_taintt.t",
        "utf8.t",
        "init_u1.t",
        "init_u2t.t",
        "init_a.t",
        "init_nc.t",
        "chdir_probe.t",
        "with_args.t",
    ];
    let observation = observe_with_mode("component_base", "clean", &members)?;
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("capture must construct the trace"))?;
    let rows = &trace.payload.rows;
    assert_eq!(rows.len(), members.len());

    let row_for =
        |suffix: &str| -> Result<&perl_core_harness::invocation_trace::EffectiveInvocationRow> {
            rows.iter()
                .find(|row| row.subject.parent_member_path.ends_with(suffix))
                .ok_or_else(|| color_eyre::eyre::eyre!("no row for {suffix}"))
        };

    // Taint modes never collapse, and the ordered switches carry them.
    let full = row_for("probe_a_taintT.t")?;
    assert_eq!(
        full.fields.taint_mode,
        observed(perl_core_harness::invocation_trace::TaintMode::TaintMode)
    );
    assert_eq!(
        full.fields.interpreter_switches,
        observed(vec!["-I../lib".to_string(), "-I../t/lib".to_string(), "-T".to_string()])
    );
    let weak = row_for("probe_b_taintt.t")?;
    assert_eq!(
        weak.fields.taint_mode,
        observed(perl_core_harness::invocation_trace::TaintMode::TaintWarnings)
    );
    let plain = row_for("plain.t")?;
    assert_eq!(
        plain.fields.taint_mode,
        observed(perl_core_harness::invocation_trace::TaintMode::None)
    );

    // UTF/source mode distinction reaches the observed environment.
    let utf8 = row_for("utf8.t")?;
    assert_eq!(
        utf8.fields.utf8_mode,
        observed(perl_core_harness::invocation_trace::Utf8Switch::Utf8)
    );
    match &utf8.fields.environment {
        EffectiveInvocationField::Observed { value } => {
            assert!(value.variables.contains_key("PERL_UNICODE"));
        }
        other => bail!("utf8 environment must be observed, got {other:?}"),
    }
    assert_eq!(
        plain.fields.utf8_mode,
        observed(perl_core_harness::invocation_trace::Utf8Switch::None)
    );

    // TestInit classes stay distinct: U1, U2T, A, NC.
    assert_eq!(
        row_for("init_u1.t")?.fields.test_init,
        observed(perl_core_harness::invocation_trace::TestInitClass::U1)
    );
    assert_eq!(
        row_for("init_u2t.t")?.fields.test_init,
        observed(perl_core_harness::invocation_trace::TestInitClass::U2t)
    );
    assert_eq!(
        row_for("init_a.t")?.fields.test_init,
        observed(perl_core_harness::invocation_trace::TestInitClass::A)
    );
    assert_eq!(
        row_for("init_nc.t")?.fields.test_init,
        observed(perl_core_harness::invocation_trace::TestInitClass::Nc)
    );
    assert_eq!(
        plain.fields.test_init,
        observed(perl_core_harness::invocation_trace::TestInitClass::Standard)
    );

    // cwd/return-directory distinction: the chdir member runs in t/base and
    // returns to t.
    let chdir = row_for("chdir_probe.t")?;
    assert_eq!(chdir.fields.run_cwd, observed("t/base".to_string()));
    assert_eq!(chdir.fields.return_directory, observed("t".to_string()));
    assert_eq!(plain.fields.run_cwd, observed("t".to_string()));

    // Ordered include roots keep application order.
    assert_eq!(
        plain.fields.include_roots,
        observed(vec!["../lib".to_string(), "../t/lib".to_string()])
    );

    // Script arguments keep their order and never leak into other rows.
    assert_eq!(
        row_for("with_args.t")?.fields.script_arguments,
        observed(vec!["--flag".to_string(), "value".to_string()])
    );
    assert_eq!(plain.fields.script_arguments, observed(Vec::new()));

    // Source form and capture point stay observed for every row.
    for row in rows {
        assert_eq!(row.fields.source_form, observed(SourceForm::DotT));
        assert!(row.fields.capture_point.is_observed());
    }

    // The comp family carries its own include order.
    let comp = observe_with_mode("component_comp", "clean", &["if.t"])?;
    let comp_trace = comp
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("comp capture must construct the trace"))?;
    let comp_row = comp_trace
        .payload
        .rows
        .iter()
        .find(|row| row.subject.parent_member_path == "t/comp/require.t")
        .ok_or_else(|| color_eyre::eyre::eyre!("comp row missing"))?;
    assert_eq!(
        comp_row.fields.include_roots,
        observed(vec!["../lib".to_string(), "../cpan".to_string()])
    );
    Ok(())
}

fn observed<T>(value: T) -> EffectiveInvocationField<T> {
    EffectiveInvocationField::Observed { value }
}

// ---------------------------------------------------------------------------
// Determinism: repeated equivalent fixture runs, normalized receipts
// ---------------------------------------------------------------------------

#[test]
fn repeated_equivalent_runs_produce_byte_identical_normalized_receipts() -> Result<()> {
    let first = observe_with_mode("component_base", "clean", &["if.t", "cond.t"])?;
    let second = observe_with_mode("component_base", "clean", &["if.t", "cond.t"])?;
    for (a, b) in [
        (serde_json::to_value(&first.parent)?, serde_json::to_value(&second.parent)?),
        (serde_json::to_value(&first.trace)?, serde_json::to_value(&second.trace)?),
        (serde_json::to_value(&first.work)?, serde_json::to_value(&second.work)?),
    ] {
        let secrets = [
            first.work.payload.process_nonce.clone(),
            first.work.payload.trace_session_id.clone(),
            second.work.payload.process_nonce.clone(),
            second.work.payload.trace_session_id.clone(),
        ];
        assert_eq!(
            normalize_receipt(&a, &secrets),
            normalize_receipt(&b, &secrets),
            "normalized receipts must be byte-identical across equivalent runs"
        );
    }
    // Raw digests differ: the capture identities are per-run.
    assert_ne!(first.work.payload.process_nonce, second.work.payload.process_nonce);
    assert_ne!(first.work.payload.trace_session_id, second.work.payload.trace_session_id);
    Ok(())
}

/// Presentation normalization for the determinism proof: run-scoped capture
/// identities (nonce, session), derived digests (64-hex runs), and raw
/// retained stream bytes are masked; every behavior-bearing fact must match.
fn normalize_receipt(value: &serde_json::Value, secrets: &[String]) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            let mut masked = text.clone();
            for secret in secrets {
                if !secret.is_empty() {
                    masked = masked.replace(secret.as_str(), "<identity>");
                }
            }
            serde_json::Value::String(mask_hex_runs(&masked))
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items.iter().map(|item| normalize_receipt(item, secrets)).collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut normalized = serde_json::Map::new();
            for (key, entry) in map {
                if key == "bytes_hex" {
                    normalized
                        .insert(key.clone(), serde_json::Value::String("<bytes>".to_string()));
                } else {
                    normalized.insert(key.clone(), normalize_receipt(entry, secrets));
                }
            }
            serde_json::Value::Object(normalized)
        }
        other => other.clone(),
    }
}

/// Mask every maximal hexadecimal run of digest length inside a string so
/// derived digests never defeat the normalized comparison.
fn mask_hex_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_hexdigit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
                index += 1;
            }
            if index - start >= 64 {
                out.push_str("<digest>");
            } else {
                out.push_str(&text[start..index]);
            }
        } else {
            out.push(bytes[index] as char);
            index += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Falsifier 4 + 5: missing fields stay partial; nothing is synthesized
// ---------------------------------------------------------------------------

#[test]
fn missing_field_stays_partial_and_never_projects() -> Result<()> {
    let observation = observe_with_mode("component_base", "missing_field", &["if.t", "cond.t"])?;
    let trace = observation.trace.as_ref().ok_or_else(|| {
        color_eyre::eyre::eyre!("the trace receipt still exists for a partial row")
    })?;
    let first = &trace.payload.rows[0];
    assert_eq!(first.state, InvocationObservationState::ObservedPartial);
    assert!(!first.fields.environment.is_observed());
    assert!(!matches!(
        first.projection,
        perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
    ));
    let second = &trace.payload.rows[1];
    assert_eq!(second.state, InvocationObservationState::ObservedComplete);
    assert!(matches!(
        second.projection,
        perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
    ));
    assert_eq!(observation.work.payload.state, InstrumentationState::TracePartial);
    assert_eq!(observation.work.payload.work.complete_rows, 1);
    assert_eq!(observation.work.payload.work.fields_synthesized, 0);
    // Command surface: typed failure naming the state.
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "missing_field", &["if.t", "cond.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations_command(&config) else {
        bail!("a partial observation must not be a clean pass");
    };
    assert!(
        error.to_string().contains("TracePartial"),
        "typed failure must name the state: {error}"
    );
    Ok(())
}

#[test]
fn instrument_failure_field_types_instrument_failed() -> Result<()> {
    let observation =
        observe_with_mode("component_base", "instrument_failure", &["if.t", "cond.t"])?;
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("trace receipt retained for instrument failure"))?;
    let first = &trace.payload.rows[0];
    assert_eq!(first.state, InvocationObservationState::InstrumentFailed);
    assert!(first.fields.scheduling.is_instrument_failure());
    assert!(!matches!(
        first.projection,
        perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
    ));
    assert_eq!(observation.work.payload.state, InstrumentationState::InstrumentFailed);
    assert_eq!(observation.work.payload.work.instrument_failed_rows, 1);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 7+9: duplicates and cross-run rows are typed, never repaired
// ---------------------------------------------------------------------------

#[test]
fn duplicate_row_retains_first_contributor_never_last_writer() -> Result<()> {
    let observation = observe_with_mode("component_base", "duplicate_row", &["if.t", "cond.t"])?;
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("trace receipt retained for duplicates"))?;
    let rows = &trace.payload.rows;
    assert_eq!(rows.len(), 3);
    assert!(rows[0].disposition.is_accepted());
    match &rows[2].disposition {
        TraceRowDisposition::DuplicateRowId { row_id } => {
            assert_eq!(row_id, &rows[0].row_id);
        }
        other => bail!("expected duplicate disposition, got {other:?}"),
    }
    assert_eq!(rows[2].state, InvocationObservationState::NotProven);
    // The first contributor keeps its observation; the duplicate projects
    // nothing.
    assert!(matches!(
        rows[0].projection,
        perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
    ));
    assert!(!matches!(
        rows[2].projection,
        perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
    ));
    assert_eq!(observation.work.payload.work.conflicting_rows, 1);
    assert_eq!(observation.work.payload.state, InstrumentationState::TracePartial);
    Ok(())
}

#[test]
fn foreign_session_and_out_of_order_rows_are_typed_not_repaired() -> Result<()> {
    let foreign = observe_with_mode("component_base", "foreign_session", &["if.t", "cond.t"])?;
    let trace =
        foreign.trace.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("trace receipt retained"))?;
    match &trace.payload.rows[1].disposition {
        TraceRowDisposition::CrossRunInterleaved { session_id } => {
            assert!(session_id.ends_with("-foreign"));
        }
        other => bail!("expected cross-run disposition, got {other:?}"),
    }
    assert_eq!(trace.payload.rows[1].state, InvocationObservationState::NotProven);

    let ordered = observe_with_mode("component_base", "out_of_order", &["if.t", "cond.t"])?;
    let trace =
        ordered.trace.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("trace receipt retained"))?;
    match &trace.payload.rows[1].disposition {
        TraceRowDisposition::OutOfOrderSequence { expected, actual } => {
            assert_eq!((*expected, *actual), (1, 5));
        }
        other => bail!("expected out-of-order disposition, got {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 10+11: truncation and lying terminals never complete
// ---------------------------------------------------------------------------

#[test]
fn truncated_trace_stream_is_never_a_clean_observation() -> Result<()> {
    let observation = observe_with_mode("component_base", "truncated", &["if.t", "cond.t"])?;
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("the malformed trace stays a typed receipt"))?;
    assert!(!trace.payload.trace_decode.is_complete());
    assert!(trace.payload.terminal.is_none());
    for row in &trace.payload.rows {
        assert_eq!(row.state, InvocationObservationState::NotProven);
        assert!(!matches!(
            row.projection,
            perl_core_harness::invocation_trace::ProjectionRecord::Projected { .. }
        ));
    }
    assert_eq!(observation.work.payload.state, InstrumentationState::TraceMalformed);
    Ok(())
}

#[test]
fn nonzero_exit_yields_runner_failed_rows_and_typed_failure() -> Result<()> {
    let observation = observe_with_mode("component_base", "nonzero", &["if.t"])?;
    let parent = observation
        .parent
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("runner-failed parent is retained evidence"))?;
    assert_eq!(parent.payload.state, DiscoveryObservationState::RunnerFailed);
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("runner-failed trace is retained evidence"))?;
    for row in &trace.payload.rows {
        assert_eq!(row.state, InvocationObservationState::RunnerFailed);
    }
    assert_eq!(observation.work.payload.state, InstrumentationState::RunnerFailed);
    assert_eq!(observation.work.payload.work.terminal_disagreements, 0);
    Ok(())
}

#[test]
fn lying_terminal_frame_types_the_disagreement() -> Result<()> {
    let observation = observe_with_mode("component_base", "lying_terminal", &["if.t"])?;
    // The trace rows look complete on the instrument's word alone...
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("the lying trace stays retained evidence"))?;
    assert_eq!(trace.payload.rows[0].state, InvocationObservationState::ObservedComplete);
    // ...but the work receipt types the supervisor disagreement.
    assert_eq!(observation.work.payload.work.terminal_disagreements, 1);
    assert_eq!(observation.work.payload.state, InstrumentationState::TerminalDisagreement);
    // Command surface: typed failure.
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "lying_terminal", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations_command(&config) else {
        bail!("a lying terminal must not be a clean pass");
    };
    assert!(
        error.to_string().contains("TerminalDisagreement"),
        "typed failure must name the disagreement: {error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 3: trace bytes on ordinary streams void the transport contract
// ---------------------------------------------------------------------------

#[test]
fn stdout_contamination_refuses_trace_construction() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "contaminate", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations_command(&config) else {
        bail!("a contaminated capture must not be a clean pass");
    };
    assert!(
        error.to_string().contains("ContaminatedParent"),
        "typed failure must name the contamination: {error}"
    );
    // The contaminated parent stays retained as evidence; no trace receipt
    // is fabricated against a voided transport contract.
    let parent = load_parent(&config.output)?;
    assert_ne!(parent.payload.state, DiscoveryObservationState::ObservedComplete);
    assert!(!config.trace_output.exists());
    let work = load_work(&config.work_output)?;
    assert_eq!(work.payload.state, InstrumentationState::ContaminatedParent);
    assert_eq!(work.payload.work.ordinary_output_contamination_count, 1);
    Ok(())
}

// ---------------------------------------------------------------------------
// Empty discovery: explicit typed refusal, never silence
// ---------------------------------------------------------------------------

#[test]
fn empty_discovery_is_a_typed_parent_construction_failure() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "empty", &[])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations_command(&config) else {
        bail!("an empty instrumented discovery must not be a clean pass");
    };
    assert!(
        error.to_string().contains("ParentConstructionFailed"),
        "typed failure must name the empty-stream law: {error}"
    );
    assert!(!config.output.exists(), "no parent receipt may be fabricated");
    assert!(!config.trace_output.exists(), "no trace receipt may be fabricated");
    let work = load_work(&config.work_output)?;
    assert_eq!(work.payload.state, InstrumentationState::ParentConstructionFailed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 10 (process plane): a hung runner types not-proven from evidence
// ---------------------------------------------------------------------------

#[test]
fn captured_mid_run_timeout_yields_not_proven_with_retained_rows() -> Result<()> {
    let limits = CaptureLimits { deadline: Duration::from_secs(5), cancel_file: None };
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "hang", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", limits);
    let started = std::time::Instant::now();
    let observation = observe_invocations(&config)?;
    assert!(started.elapsed() < Duration::from_secs(45), "supervision must bound the hung runner");
    let parent = observation
        .parent
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("the timed-out parent stays retained"))?;
    assert_eq!(parent.payload.state, DiscoveryObservationState::TimedOut);
    let trace = observation
        .trace
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("the un-terminated trace stays retained"))?;
    assert!(!trace.payload.trace_decode.is_complete());
    assert_eq!(trace.payload.rows.len(), 1);
    assert_eq!(trace.payload.rows[0].state, InvocationObservationState::NotProven);
    assert_eq!(observation.work.payload.state, InstrumentationState::NotProven);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 1+2: patch subject drift and anchors refuse before any process
// ---------------------------------------------------------------------------

#[test]
fn patch_drift_missing_and_ambiguous_anchors_refuse_before_any_process() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "clean", &["if.t"])?;

    // Subject drift: the spec pins another ordinary digest.
    let mut drifted_spec_raw = serde_json::to_value(&spec_from(&fs::read_to_string(
        &write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?,
    )?))?;
    drifted_spec_raw["expected_ordinary_sha256"] =
        serde_json::Value::String(sha_hex(b"#!./perl\n# drifted\n"));
    let drifted_path = temp.path().join("drifted-spec.json");
    fs::write(&drifted_path, serde_json::to_vec(&drifted_spec_raw)?)?;
    let config = config_for(&tree, &drifted_path, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations(&config) else {
        bail!("patch subject drift must refuse the capture");
    };
    assert!(
        error.to_string().contains("source drift is rejected"),
        "drift refusal must name the law: {error}"
    );

    // Missing anchor: exact bytes absent from the artifact.
    let mut missing_spec =
        spec_from(&fs::read_to_string(&write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?)?);
    missing_spec.operations[0].anchor = "# absent from the artifact".to_string();
    let missing_path = temp.path().join("missing-spec.json");
    fs::write(&missing_path, serde_json::to_vec(&missing_spec)?)?;
    let config = config_for(&tree, &missing_path, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations(&config) else {
        bail!("a missing anchor must refuse the capture");
    };
    assert!(
        error.to_string().contains("anchors on bytes absent"),
        "missing-anchor refusal: {error}"
    );

    // Ambiguous anchor: the anchor occurs twice in the artifact.
    let ambiguous_artifact = format!("{ORDINARY_ARTIFACT}{ORDINARY_ANCHOR}\n");
    let mut ambiguous_spec =
        spec_from(&fs::read_to_string(&write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?)?);
    ambiguous_spec.expected_ordinary_sha256 = sha_hex(ambiguous_artifact.as_bytes());
    let ambiguous_path = temp.path().join("ambiguous-spec.json");
    fs::write(&ambiguous_path, serde_json::to_vec(&ambiguous_spec)?)?;
    fs::write(tree.join("t").join("TEST"), &ambiguous_artifact)?;
    let config =
        config_for(&tree, &ambiguous_path, temp.path(), "component_base", default_limits());
    let Err(error) = observe_invocations(&config) else {
        bail!("an ambiguous anchor must refuse the capture");
    };
    assert!(error.to_string().contains("anchors 2 times"), "ambiguous-anchor refusal: {error}");
    // The pure patch tool carries the same typed refusals.
    let Err(error) = apply_exact_patch(ORDINARY_ARTIFACT.as_bytes(), &ambiguous_spec) else {
        bail!("the pure tool must refuse a drifted subject");
    };
    assert!(error.message().contains("measures"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Route admission fails closed
// ---------------------------------------------------------------------------

#[test]
fn unadmitted_routes_and_targets_refuse_before_any_process() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "clean", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;

    let mut harness_config =
        config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    harness_config.runner = RunnerKind::Harness;
    let Err(error) = observe_invocations(&harness_config) else {
        bail!("t/harness must refuse the instrumentation route");
    };
    assert!(
        error.to_string().contains("not an admitted instrumentation route"),
        "unexpected runner refusal: {error}"
    );

    for (target_id, fragment) in [
        ("component_op", "test selection authority"),
        ("manifest_root_lib", "t/TEST observation route vocabulary"),
        ("no_such_target", "no target no_such_target"),
    ] {
        let config = config_for(&tree, &patch, temp.path(), target_id, default_limits());
        let Err(error) = observe_invocations(&config) else {
            bail!("{target_id} must refuse the instrumentation route");
        };
        assert!(error.to_string().contains(fragment), "unexpected {target_id} refusal: {error}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 14 + the ordinary/instrumented identity law
// ---------------------------------------------------------------------------

#[test]
fn ordinary_tree_is_never_modified_and_stays_ordinary() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "clean", &["if.t"])?;
    let patch = write_patch_spec(temp.path(), ORDINARY_ARTIFACT)?;
    let config = config_for(&tree, &patch, temp.path(), "component_base", default_limits());
    observe_invocations_command(&config)?;

    // The pinned ordinary artifact bytes are untouched, and no trace channel
    // or patch residue entered the original tree.
    assert_eq!(fs::read_to_string(tree.join("t").join("TEST"))?, ORDINARY_ARTIFACT);
    assert!(!tree.join("t").join(".perl-core-harness-trace.jsonl").exists());
    let work = load_work(&config.work_output)?;
    assert!(work.payload.cleanup.is_proven());

    // The landed ordinary route on the same tree keeps its distinct identity:
    // instrumentation_id absent, ordinary artifact digest, own nonce.
    let ordinary = perl_core_harness::observed_discovery::observe_discovery(
        &perl_core_harness::observed_discovery::ObserveDiscoveryConfig {
            matrix: matrix_path(),
            target_id: "component_base".to_string(),
            runner: RunnerKind::Test,
            perl_tree: tree.clone(),
            host_perl: PathBuf::from(env!("CARGO_BIN_EXE_perl-core-harness-observe-fixture")),
            repository_commit: "a".repeat(40),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-generation-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            output: temp.path().join("ordinary-receipt.json"),
            limits: default_limits(),
        },
    )?;
    assert_eq!(ordinary.payload.subject.instrumentation_id, None);
    assert_eq!(
        ordinary.payload.invocation.runner_artifact.content_sha256,
        sha_hex(ORDINARY_ARTIFACT.as_bytes())
    );
    assert_ne!(
        ordinary.payload.invocation.runner_artifact.content_sha256,
        work.payload.instrumented_artifact.content_sha256,
        "the instrumented artifact must never stand in for the ordinary one"
    );
    assert_ne!(ordinary.payload.terminal.process_nonce, work.payload.process_nonce);
    Ok(())
}

// ---------------------------------------------------------------------------
// The instrumented process proves it consumed the patched artifact
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_refuses_an_unpatched_artifact_digest() -> Result<()> {
    use std::process::Command;
    let temp = tempfile::tempdir()?;
    let t_dir = temp.path().join("t");
    fs::create_dir_all(&t_dir)?;
    fs::write(t_dir.join("TEST"), ORDINARY_ARTIFACT)?;
    let output = Command::new(fixture_host_perl())
        .current_dir(&t_dir)
        .args(["TEST".to_string(), "--dumptests".to_string(), "base".to_string()])
        .env("PERL_CORE_HARNESS_TRACE_FILE", ".trace.jsonl")
        .env("PERL_CORE_HARNESS_TRACE_SESSION", "trace-session-x")
        .env("PERL_CORE_HARNESS_TRACE_ARTIFACT_SHA256", sha_hex(b"#!./perl\n# something else\n"))
        .env("PERL_CORE_HARNESS_TRACE_TARGET", "component_base")
        .env("PERL_CORE_HARNESS_TRACE_INSTRUMENTATION", INSTRUMENTATION_ID)
        .output()?;
    assert_eq!(output.status.code(), Some(66), "the fixture must refuse a foreign artifact");
    assert!(String::from_utf8_lossy(&output.stderr).contains("measures"));
    assert!(!t_dir.join(".trace.jsonl").exists(), "no trace may be emitted for a foreign subject");
    Ok(())
}
