//! Hermetic exact-process proof for the observed-discovery capture route
//! (#12283).
//!
//! Every observation in this suite runs a real supervised process: the
//! `perl-core-harness-observe-fixture` binary stands in for the prepared
//! tree's host Perl executing `t/TEST --dumptests <selector>` and performs the
//! upstream-side selection itself. The route under test derives argv from the
//! pinned target matrix, captures byte-exact stdout/stderr through the bounded
//! supervisor, and assembles a strict #12281 receipt that must reconstruct
//! through the landed decoder and validators.
//!
//! The pinned real-tree rows (prepared upstream `t/TEST` at perl-5.42.2)
//! remain explicitly unproven until a real prepared tree is captured; nothing
//! here fabricates them.

use color_eyre::eyre::{Result, bail};
use perl_core_harness::artifacts::CaptureLimits;
use perl_core_harness::model::UpstreamTargetMatrix;
use perl_core_harness::observed_discovery::capture::ObserveDiscoveryConfig;
use perl_core_harness::observed_discovery::model::{
    DiscoveryObservationState, EvidenceClass, MemberDisposition, UpstreamDiscoveryReceiptV1,
};
use perl_core_harness::observed_discovery::{
    DiscoveryFrame, RunnerKind, observe_discovery, observe_discovery_command,
    validate_observed_discovery_receipt,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fixture_host_perl() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-core-harness-observe-fixture"))
}

fn matrix_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".ci/perl-core-harness/upstream-targets-5.42.2.v1")
}

fn matrix() -> Result<UpstreamTargetMatrix> {
    perl_core_harness::io::read_matrix(&matrix_path())
}

/// One hermetic prepared tree: `t/TEST` artifact, base/comp/run members, and
/// the fixture drift-mode marker.
fn fixture_tree(root: &Path, mode: &str) -> Result<PathBuf> {
    fixture_tree_with_base_members(root, mode, &["if.t", "cond.t"])
}

fn fixture_tree_with_base_members(
    root: &Path,
    mode: &str,
    base_members: &[&str],
) -> Result<PathBuf> {
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
    fs::write(
        t_dir.join("TEST"),
        "#!./perl\n# hermetic stand-in for the pinned upstream t/TEST\n",
    )?;
    fs::write(t_dir.join(".observe-fixture-mode"), mode)?;
    Ok(tree)
}

struct Observation {
    _temp: tempfile::TempDir,
    receipt: UpstreamDiscoveryReceiptV1,
}

fn observe_with_mode(target_id: &str, mode: &str) -> Result<Observation> {
    observe_with_mode_and_limits(target_id, mode, default_limits())
}

fn default_limits() -> CaptureLimits {
    CaptureLimits { deadline: Duration::from_secs(30), cancel_file: None }
}

fn observe_with_mode_and_limits(
    target_id: &str,
    mode: &str,
    limits: CaptureLimits,
) -> Result<Observation> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), mode)?;
    let config = config_for(&tree, temp.path().join("receipt.json"), target_id, limits);
    let receipt = observe_discovery(&config)?;
    Ok(Observation { _temp: temp, receipt })
}

fn config_for(
    tree: &Path,
    output: PathBuf,
    target_id: &str,
    limits: CaptureLimits,
) -> ObserveDiscoveryConfig {
    ObserveDiscoveryConfig {
        matrix: matrix_path(),
        target_id: target_id.to_string(),
        runner: RunnerKind::Test,
        perl_tree: tree.to_path_buf(),
        host_perl: fixture_host_perl(),
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        output,
        limits,
    }
}

fn load_receipt(path: &Path) -> Result<UpstreamDiscoveryReceiptV1> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn accepted_paths(receipt: &UpstreamDiscoveryReceiptV1) -> Vec<String> {
    receipt
        .payload
        .rows
        .iter()
        .filter(|row| row.is_accepted())
        .filter_map(|row| row.canonical_path().map(str::to_string))
        .collect()
}

fn ensure_reconstructs(label: &str, receipt: &UpstreamDiscoveryReceiptV1) -> Result<()> {
    validate_observed_discovery_receipt(&matrix()?, receipt)
        .map_err(|error| color_eyre::eyre::eyre!("{label} receipt reconstruction: {error}"))
}

// ---------------------------------------------------------------------------
// Positive proof: clean exact base/comp/run selector observations
// ---------------------------------------------------------------------------

#[test]
fn clean_base_comp_and_run_selector_observations_are_complete() -> Result<()> {
    for (target_id, selector_argument, expected_members) in [
        ("component_base", "base", vec!["t/base/cond.t", "t/base/if.t"]),
        ("component_comp", "comp", vec!["t/comp/require.t"]),
        ("component_run", "run", vec!["t/run/exit.t"]),
    ] {
        let observation = observe_with_mode(target_id, "select")?;
        let receipt = &observation.receipt;
        assert_eq!(
            receipt.payload.state,
            DiscoveryObservationState::ObservedComplete,
            "{target_id}"
        );
        assert_eq!(receipt.evidence_class, EvidenceClass::ObservedUpstream, "{target_id}");
        assert_eq!(accepted_paths(receipt), expected_members, "{target_id}");
        assert_eq!(
            receipt.payload.invocation.argv,
            vec!["TEST".to_string(), "--dumptests".to_string(), selector_argument.to_string()],
            "{target_id} argv must carry the selector root, never expanded members"
        );
        assert_eq!(receipt.payload.invocation.working_directory, "t", "{target_id}");
        assert_eq!(
            receipt.payload.invocation.runner_artifact.canonical_path, "t/TEST",
            "{target_id}"
        );
        assert_eq!(
            receipt.payload.work.accepted_rows,
            expected_members.len() as u64,
            "{target_id}"
        );
        assert_eq!(receipt.payload.work.decoded_rows, expected_members.len() as u64, "{target_id}");
        // The structural zeroes of the observation route.
        assert_eq!(receipt.payload.work.filesystem_discovery_operations, 0, "{target_id}");
        assert_eq!(receipt.payload.work.direct_probe_rows_consumed, 0, "{target_id}");
        // Raw evidence stays byte-exact and load-bearing.
        let stdout_bytes =
            receipt.payload.stdout.bytes().map_err(|error| {
                color_eyre::eyre::eyre!("decoding retained stdout bytes: {error}")
            })?;
        let expected_stream =
            expected_members.iter().map(|member| format!("{member}\n")).collect::<String>();
        assert_eq!(stdout_bytes, expected_stream.as_bytes(), "{target_id} raw stdout");
        assert_eq!(
            receipt.payload.work.raw_stdout_bytes,
            expected_stream.len() as u64,
            "{target_id}"
        );
        ensure_reconstructs(target_id, receipt)?;
    }
    Ok(())
}

#[test]
fn selector_argv_follows_the_contract_not_local_tree_contents() -> Result<()> {
    // Two different prepared trees (different member populations) produce the
    // same contract-derived argv and both observations stay complete: the argv
    // is a function of the target contract, and the member population is a
    // fact the upstream process alone contributes.
    let first = observe_with_mode("component_base", "select")?;
    let second_temp = tempfile::tempdir()?;
    let second_tree = fixture_tree_with_base_members(second_temp.path(), "select", &["zebra.t"])?;
    let second_config = config_for(
        &second_tree,
        second_temp.path().join("receipt.json"),
        "component_base",
        default_limits(),
    );
    let second = observe_discovery(&second_config)?;

    assert_eq!(
        first.receipt.payload.invocation.argv, second.payload.invocation.argv,
        "argv must not depend on the local tree"
    );
    assert_eq!(
        accepted_paths(&second),
        vec!["t/base/zebra.t".to_string()],
        "the observed population must come from the upstream process"
    );
    assert_eq!(second.payload.state, DiscoveryObservationState::ObservedComplete);
    assert_ne!(first.receipt.payload_digest, second.payload_digest);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 5: a complete-looking prefix under a failed terminal never
// completes, and the command never reports a clean pass
// ---------------------------------------------------------------------------

#[test]
fn nonzero_exit_with_complete_members_is_runner_failed_not_complete() -> Result<()> {
    let observation = observe_with_mode("component_base", "select_fail")?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::RunnerFailed);
    assert!(!receipt.payload.state.is_complete());
    assert_eq!(receipt.payload.work.accepted_rows, 2, "members stay retained as evidence");
    ensure_reconstructs("runner-failed", receipt)?;

    // Command surface: the receipt is retained, but the exit is a typed
    // failure naming the state.
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "select_fail")?;
    let output = temp.path().join("failed-receipt.json");
    let config = config_for(&tree, output.clone(), "component_base", default_limits());
    let Err(error) = observe_discovery_command(&config) else {
        bail!("a runner-failed observation must not be a clean pass");
    };
    let message = error.to_string();
    assert!(
        message.contains("RunnerFailed") && message.contains("not observed_complete"),
        "typed failure must name the state: {message}"
    );
    let retained = load_receipt(&output)?;
    assert_eq!(retained.payload.state, DiscoveryObservationState::RunnerFailed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 6+7: missing members are never filled; extras/duplicates are
// retained with typed dispositions, never filtered
// ---------------------------------------------------------------------------

#[test]
fn duplicate_members_are_retained_as_duplicates_and_make_the_observation_partial() -> Result<()> {
    let observation = observe_with_mode("component_base", "duplicate_first")?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
    assert_eq!(receipt.payload.rows.len(), 3);
    assert_eq!(receipt.payload.work.accepted_rows, 2);
    assert_eq!(receipt.payload.work.duplicate_rows, 1);
    match &receipt.payload.rows[1].disposition {
        MemberDisposition::DuplicateOfCanonical { canonical_path } => {
            assert_eq!(canonical_path, "t/base/cond.t");
        }
        other => bail!("expected duplicate disposition, got {other:?}"),
    }
    ensure_reconstructs("duplicate", receipt)?;
    Ok(())
}

#[test]
fn out_of_target_members_are_retained_not_filtered() -> Result<()> {
    let observation = observe_with_mode("component_base", "foreign_extra")?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
    assert_eq!(receipt.payload.work.accepted_rows, 2);
    assert_eq!(receipt.payload.work.out_of_target_rows, 1);
    let foreign = receipt
        .payload
        .rows
        .iter()
        .find(|row| matches!(row.disposition, MemberDisposition::OutsideTargetSelection))
        .ok_or_else(|| color_eyre::eyre::eyre!("out-of-target row was dropped"))?;
    assert_eq!(foreign.raw_text, "t/comp/foreign_extra.t");
    ensure_reconstructs("foreign-extra", receipt)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Decode-side strictness binds the capture: drifted and malformed rows
// ---------------------------------------------------------------------------

#[test]
fn drifted_row_is_malformed_not_trimmed_into_membership() -> Result<()> {
    let observation = observe_with_mode("component_base", "drifted_row")?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    let drifted = &receipt.payload.rows[0];
    assert_eq!(drifted.raw_text, " t/base/cond.t");
    assert!(matches!(drifted.disposition, MemberDisposition::MalformedRow));
    assert!(drifted.normalized.is_none());
    ensure_reconstructs("drifted", receipt)?;
    Ok(())
}

#[test]
fn invalid_utf8_stream_is_malformed_with_zero_reconstructed_rows() -> Result<()> {
    let observation = observe_with_mode("component_base", "invalid_utf8")?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    assert!(receipt.payload.rows.is_empty());
    assert_eq!(receipt.payload.work.raw_stdout_bytes, b"t/base/\xff.t\n".len() as u64);
    ensure_reconstructs("invalid-utf8", receipt)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Empty discovery is an explicit typed refusal, never silence
// ---------------------------------------------------------------------------

#[test]
fn empty_discovery_is_a_typed_refusal_and_writes_no_receipt() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "empty")?;
    let output = temp.path().join("empty-receipt.json");
    let config = config_for(&tree, output.clone(), "component_base", default_limits());
    let Err(error) = observe_discovery_command(&config) else {
        bail!("an empty discovery must not be a clean pass");
    };
    let message = error.to_string();
    assert!(message.contains("no members"), "refusal must name the empty-stream law: {message}");
    assert!(!output.exists(), "no receipt may be fabricated for an empty discovery");
    Ok(())
}

// ---------------------------------------------------------------------------
// Process supervision: a captured-mid-run observation types its terminal
// state from real evidence, never from the members it liked
// ---------------------------------------------------------------------------

#[test]
fn captured_mid_run_timeout_yields_timed_out_not_complete() -> Result<()> {
    // The deadline leaves the runner enough room under parallel test load to
    // emit its member rows before supervision stops the capture, so the typed
    // terminal state is proven from real retained evidence.
    let limits = CaptureLimits { deadline: Duration::from_secs(5), cancel_file: None };
    let started = std::time::Instant::now();
    let observation = observe_with_mode_and_limits("component_base", "hang", limits)?;
    let receipt = &observation.receipt;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::TimedOut);
    assert!(!receipt.payload.state.is_complete());
    // The complete-looking emitted member set stays retained as evidence, but
    // the terminal state dominates the observation.
    assert_eq!(
        accepted_paths(receipt),
        vec!["t/base/cond.t".to_string(), "t/base/if.t".to_string()]
    );
    assert!(started.elapsed() < Duration::from_secs(45), "supervision must bound the hung runner");
    ensure_reconstructs("timed-out", receipt)?;
    Ok(())
}

#[test]
fn cancellation_is_typed_cancelled_and_distinct_from_the_deadline() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "hang")?;
    let cancel = temp.path().join("cancel-requested");
    let output = temp.path().join("cancelled-receipt.json");
    #[expect(
        clippy::duration_suboptimal_units,
        reason = "stable Duration constructors stop at seconds; the 600-second backstop must only ever be beaten by cancellation"
    )]
    let far_deadline = Duration::from_secs(600);
    let config = config_for(
        &tree,
        output.clone(),
        "component_base",
        CaptureLimits { deadline: far_deadline, cancel_file: Some(cancel.clone()) },
    );
    // Cancel only after the runner has provably flushed its rows, so the
    // observation retains real evidence and the typed state is deterministic.
    let ready = tree.join("t").join(".observe-fixture-ready");
    std::thread::spawn(move || {
        for _ in 0..3000 {
            if ready.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = fs::write(&cancel, "requested\n");
    });
    let started = std::time::Instant::now();
    let Err(error) = observe_discovery_command(&config) else {
        bail!("a cancelled observation must not be a clean pass");
    };
    assert!(
        error.to_string().contains("Cancelled"),
        "typed failure must name the cancelled state: {error}"
    );
    #[expect(
        clippy::duration_suboptimal_units,
        reason = "stable Duration constructors stop at seconds; generous bound for a supervised cancellation under load"
    )]
    let cancellation_bound = Duration::from_secs(60);
    assert!(
        started.elapsed() < cancellation_bound,
        "cancellation must not wait for the capture deadline"
    );
    let receipt = load_receipt(&output)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::Cancelled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 8: the source frame is bound by the runner route, not the rows
// ---------------------------------------------------------------------------

#[test]
fn frame_stays_route_bound_when_rows_arrive_in_a_foreign_spelling() -> Result<()> {
    let observation = observe_with_mode("component_base", "t_relative")?;
    let receipt = &observation.receipt;
    assert_eq!(
        receipt.payload.discovery_frame,
        DiscoveryFrame::CanonicalRepositoryPath,
        "the frame must come from the runner/cwd contract, not row content"
    );
    // `t/`-relative spellings under the canonical frame are not silently
    // re-framed into the target selection: every row stays foreign.
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
    assert_eq!(receipt.payload.work.accepted_rows, 0);
    assert_eq!(receipt.payload.work.unsupported_source_form_rows, 2);
    ensure_reconstructs("t-relative", receipt)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 9: another artifact cannot silently supply the observation
// ---------------------------------------------------------------------------

#[test]
fn runner_artifact_bytes_bind_the_receipt_identity() -> Result<()> {
    let first = observe_with_mode("component_base", "select")?;
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "select")?;
    // Same tree shape, drifted artifact bytes.
    fs::write(tree.join("t").join("TEST"), "#!./perl\n# drifted upstream artifact bytes\n")?;
    let config =
        config_for(&tree, temp.path().join("receipt.json"), "component_base", default_limits());
    let second = observe_discovery(&config)?;

    assert_ne!(
        first.receipt.payload.invocation.runner_artifact.content_sha256,
        second.payload.invocation.runner_artifact.content_sha256,
        "artifact identity must follow the measured bytes"
    );
    assert_ne!(first.receipt.payload_digest, second.payload_digest);
    ensure_reconstructs("drifted-artifact", &second)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 10: capture identity is minted per observation
// ---------------------------------------------------------------------------

#[test]
fn capture_identity_is_unique_per_observation() -> Result<()> {
    let first = observe_with_mode("component_base", "select")?;
    let second = observe_with_mode("component_base", "select")?;
    let first_nonce = &first.receipt.payload.terminal.process_nonce;
    let second_nonce = &second.receipt.payload.terminal.process_nonce;
    assert_ne!(first_nonce, second_nonce, "each capture must mint its own identity");
    assert_eq!(first_nonce, &first.receipt.payload.stdout.process_nonce);
    assert_eq!(first_nonce, &first.receipt.payload.stderr.process_nonce);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 1+12: route admission fails closed; declared/historical routes
// cannot produce an observed receipt through this command
// ---------------------------------------------------------------------------

#[test]
fn unadmitted_routes_and_targets_refuse_before_any_process_runs() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "select")?;

    // The t/harness runner is a separate lane.
    let mut harness_config =
        config_for(&tree, temp.path().join("r.json"), "component_base", default_limits());
    harness_config.runner = RunnerKind::Harness;
    let Err(error) = observe_discovery(&harness_config) else {
        bail!("t/harness observations must refuse the exact t/TEST route");
    };
    assert!(
        error.to_string().contains("not an admitted observation route"),
        "unexpected runner refusal: {error}"
    );

    // A harness-authority target refuses through its selection authority.
    let harness_target =
        config_for(&tree, temp.path().join("r2.json"), "component_op", default_limits());
    let Err(error) = observe_discovery(&harness_target) else {
        bail!("harness-authority targets must refuse the t/TEST route");
    };
    assert!(
        error.to_string().contains("test selection authority"),
        "unexpected authority refusal: {error}"
    );

    // A manifest-population target has no t/TEST selector spelling.
    let manifest_target =
        config_for(&tree, temp.path().join("r3.json"), "manifest_root_lib", default_limits());
    let Err(error) = observe_discovery(&manifest_target) else {
        bail!("manifest-population targets must refuse the t/TEST route");
    };
    assert!(
        error.to_string().contains("t/TEST observation route vocabulary"),
        "unexpected selector refusal: {error}"
    );

    // An absent target is a typed refusal.
    let absent = config_for(&tree, temp.path().join("r4.json"), "no_such_target", default_limits());
    let Err(error) = observe_discovery(&absent) else {
        bail!("absent targets must refuse");
    };
    assert!(
        error.to_string().contains("no target no_such_target"),
        "unexpected absent-target refusal: {error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 11: observed order is the process's own order
// ---------------------------------------------------------------------------

#[test]
fn rows_keep_the_upstream_emitted_order() -> Result<()> {
    let observation = observe_with_mode("component_base", "select")?;
    let rows = &observation.receipt.payload.rows;
    let emitted: Vec<&str> = rows.iter().map(|row| row.raw_text.as_str()).collect();
    assert_eq!(
        emitted,
        vec!["t/base/cond.t", "t/base/if.t"],
        "the fixture emits sorted rows; the receipt must preserve that exact order"
    );
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.ordinal, index as u32);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI surface: a complete observation writes a receipt that reconstructs
// ---------------------------------------------------------------------------

#[test]
fn command_writes_a_reconstructing_receipt_for_a_clean_observation() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let tree = fixture_tree(temp.path(), "select")?;
    let output = temp.path().join("clean-receipt.json");
    let config = config_for(&tree, output.clone(), "component_base", default_limits());
    observe_discovery_command(&config)?;
    assert!(output.is_file());
    let receipt = load_receipt(&output)?;
    assert_eq!(
        receipt.schema_version,
        perl_core_harness::observed_discovery::UPSTREAM_DISCOVERY_SCHEMA_VERSION
    );
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedComplete);
    ensure_reconstructs("clean-command", &receipt)?;
    Ok(())
}
