//! Falsifier-first proof for `compiler_upstream_conformance_status.v1` (#12532).
//!
//! Tests exercise the exact rejections the issue declares first: omitted
//! non-green rows, witness-replaces-original, scalar/count overrides,
//! historical masking, missing-snapshot preference, hidden limitations,
//! private leaks, prior-Markdown retention, and ordering/host drift.

use super::*;
use tempfile::TempDir;

type TestResult = anyhow::Result<()>;

fn base_manifest(series: Vec<SeriesSelectorInput>) -> StatusInputsManifest {
    StatusInputsManifest {
        schema_version: INPUTS_SCHEMA_VERSION.to_string(),
        status_id: "u18-conformance-status".to_string(),
        compiler_candidate_identity: "candidate:local/main-worktree".to_string(),
        toolchain_build_identity: "toolchain:debug-build".to_string(),
        semantic_obligation_graph_identity: Some("obligation-graph:u08@absent".to_string()),
        slice_registry_identity: Some("slice-registry:absent".to_string()),
        maintained_sync_identity: Some("maintained-sync:u16@absent".to_string()),
        performance_packet_identity: None,
        compiler_profile_generation_identity: Some("compiler-profile:generation-0".to_string()),
        maintained_series: series,
    }
}

fn selector(series_id: &str, snapshot: Option<&str>) -> SeriesSelectorInput {
    SeriesSelectorInput {
        series_id: series_id.to_string(),
        role: "maintained-release-series".to_string(),
        snapshot_identity: snapshot.map(|value| value.to_string()),
        upstream_index_identity: format!("index:{series_id}").into(),
        snapshot_relation: Some("exact-accepted-snapshot-relation".to_string()),
    }
}

fn installed_witness(original_path: &str) -> Option<WitnessRecord> {
    Some(WitnessRecord {
        kind: WitnessKind::Minimized,
        identity: "witness:minimized-001".to_string(),
        minimizes_case_path: format!("{original_path}.min.t"),
        installation: WitnessInstallation::Installed,
    })
}

fn agreement_row(row_id: &str, series_id: &str) -> CaseInputRow {
    let original = format!("t/op/{row_id}.t");
    CaseInputRow {
        schema_version: INPUTS_SCHEMA_VERSION.to_string(),
        row_id: row_id.to_string(),
        series_id: series_id.to_string(),
        concept_family: "scalar-context".to_string(),
        concept_id: "array-return-shape".to_string(),
        obligation_id: format!("obl:{row_id}"),
        boundary: ObservationBoundary::Parser,
        oracle_subject: "oracle:real-perl-v5.38".to_string(),
        compiler_subject: "compiler:product-tree".to_string(),
        instrument_identity: "instrument:differential-compare".to_string(),
        upstream_case: UpstreamCaseRef {
            snapshot_ref: match series_id {
                "perl-5.40" => "upstream:v5.40.0",
                _ => "upstream:v5.38.0",
            }
            .to_string(),
            case_path: original.clone(),
            case_name: row_id.to_string(),
        },
        terminal_state: TerminalState::AgreementCurrent,
        witness: installed_witness(&original),
        support_boundary: SupportBoundary::Supported,
        limitation: None,
        owner: OwnerRecord {
            canonical_owner: "#12532".to_string(),
            first_blocker: None,
            wake_event: None,
        },
        performance: PerformanceEvidence { correctness_eligible: false, evidence_identity: None },
        history: RowHistory {
            upstream_change: UpstreamChange::None,
            retained_obligation_after_removal: false,
            predecessor_row_id: None,
            successor_row_id: None,
            recurrence_of_row_id: None,
        },
    }
}

fn failing_row(row_id: &str, series_id: &str) -> CaseInputRow {
    let mut row = agreement_row(row_id, series_id);
    row.terminal_state = TerminalState::CompilerFailed;
    row.witness = None;
    row.support_boundary = SupportBoundary::Unsupported;
    row.owner.first_blocker = Some("#5215".to_string());
    row.owner.wake_event = Some("wake:#5215-close".to_string());
    row
}

fn write_inputs(root: &Path, manifest: &StatusInputsManifest, rows: &[CaseInputRow]) -> TestResult {
    fs::create_dir_all(root.join("rows"))?;
    fs::write(root.join("manifest.json"), serde_json::to_string_pretty(manifest)?)?;
    // Deliberately counter-alphabetical filenames prove that directory order
    // never influences canonical bytes.
    let count = rows.len();
    for (index, row) in rows.iter().enumerate() {
        let name = format!("rows/zeta_{:03}_{}.json", count - index, index);
        fs::write(root.join(name), serde_json::to_string_pretty(row)?)?;
    }
    Ok(())
}

fn project_from(root: &Path) -> anyhow::Result<ConformanceStatusPacket> {
    let (manifest, rows) = load_inputs(root)?;
    let packet = project_packet(manifest, rows)?;
    validate_packet(&packet)?;
    Ok(packet)
}

fn error_message<T>(result: anyhow::Result<T>) -> String {
    match result {
        Err(error) => error.to_string(),
        Ok(_) => "expected rejection but projection succeeded".to_string(),
    }
}

#[test]
fn compiler_upstream_conformance_status_closed_vocabulary_is_exact() -> TestResult {
    let expected = [
        "agreement_current",
        "agreement_with_declared_limitation",
        "compiler_failed",
        "not_proven",
        "stale",
        "invalid_or_conflicting",
        "unsupported_or_external_boundary",
        "platform_or_configuration_bound",
        "classification_pending",
        "witness_pending",
        "regression_not_installed",
        "no_current_snapshot",
        "no_current_compiler_observation",
    ];
    for (index, state) in TerminalState::ALL.iter().enumerate() {
        assert_eq!(state.as_str(), expected[index]);
    }
    let serialized = serde_json::to_string(&TerminalState::UnsupportedOrExternalBoundary)?;
    assert_eq!(serialized, "\"unsupported_or_external_boundary\"");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_constants_never_grant_authority() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-basic", "perl-5.38")])?;
    let packet = project_from(dir.path())?;
    assert!(!packet.structural_constants.support_authorized);
    assert!(!packet.structural_constants.release_authorized);
    assert!(packet.structural_constants.published_channels.is_empty());
    let text = String::from_utf8(canonical_bytes(&packet)?)?;
    assert!(text.contains("\"support_authorized\": false"));
    assert!(text.contains("\"release_authorized\": false"));
    assert!(text.contains("\"published_channels\": []"));
    assert!(text.contains(NO_SCORE_STATEMENT));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_bytes_are_order_and_host_stable() -> TestResult {
    let first = TempDir::new()?;
    let second = TempDir::new()?;
    let manifest = || {
        base_manifest(vec![
            selector("perl-5.38", Some("upstream:v5.38.0")),
            selector("perl-5.40", Some("upstream:v5.40.0")),
        ])
    };
    let mut rows_a = vec![
        agreement_row("op-a", "perl-5.38"),
        failing_row("op-b", "perl-5.38"),
        agreement_row("op-c", "perl-5.40"),
    ];
    write_inputs(first.path(), &manifest(), &rows_a)?;
    rows_a.reverse();
    write_inputs(second.path(), &manifest(), &rows_a)?;

    let left = project_from(first.path())?;
    let right = project_from(second.path())?;
    assert_eq!(canonical_bytes(&left)?, canonical_bytes(&right)?);
    assert_eq!(packet_identity(&left)?, packet_identity(&right)?);
    assert_eq!(render_markdown(&left)?, render_markdown(&right)?);
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_score_fields_are_structurally_impossible() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[failing_row("op-fail", "perl-5.38")])?;
    let packet = project_from(dir.path())?;
    let text = String::from_utf8(canonical_bytes(&packet)?)?;
    assert!(!text.contains("\"score"));
    assert!(!text.contains("readiness_score"));
    assert!(!text.contains(
        "
  \"maturity"
    ));
    // Numeric output is restricted to exact descriptive counts.
    assert!(text.contains("\"total_rows\": 1,"));

    let injected =
        text.replace("\"schema_version\"", "\"readiness_score\": 92,\n  \"schema_version\"");
    let parse_result: anyhow::Result<ConformanceStatusPacket> =
        serde_json::from_str(&injected).map_err(|error| anyhow::anyhow!("{error}"));
    let message = match parse_result {
        Err(error) => error.to_string(),
        Ok(_) => "score payload parsed".to_string(),
    };
    assert!(
        message.contains("readiness_score") || message.contains("unknown field"),
        "rejection should name the injected field"
    );
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_tampered_authority_rejected() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-basic", "perl-5.38")])?;
    let packet = project_from(dir.path())?;
    let text = String::from_utf8(canonical_bytes(&packet)?)?;
    let forged = text.replace("\"support_authorized\": false", "\"support_authorized\": true");
    let parsed: ConformanceStatusPacket = serde_json::from_str(&forged)?;
    let rejection = validate_packet(&parsed);
    let message = error_message(rejection.map(|_| ()));
    assert!(
        message.contains("structural authorization constants"),
        "authority flip must reject: {message}"
    );
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_omitted_rows_falsify_denominators() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let rows = vec![agreement_row("op-good", "perl-5.38"), failing_row("op-bad", "perl-5.38")];
    write_inputs(dir.path(), &manifest, &rows)?;
    let packet = project_from(dir.path())?;

    let mut trimmed = packet.clone();
    trimmed.rows.remove(0);
    let message = error_message(validate_packet(&trimmed).map(|_| ()));
    assert!(message.contains("descriptive_counts"), "row omission must break counts: {message}");

    let optimistic = String::from_utf8(canonical_bytes(&packet)?)?.replace(
        &format!("\"total_rows\": {}", packet.descriptive_counts.total_rows),
        "\"total_rows\": 0",
    );
    let parsed: ConformanceStatusPacket = serde_json::from_str(&optimistic)?;
    assert!(validate_packet(&parsed).is_err(), "denominator falsification must reject");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_witness_may_not_replace_original() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = agreement_row("op-min", "perl-5.38");
    if let Some(witness) = row.witness.as_mut() {
        witness.minimizes_case_path = row.upstream_case.case_path.clone();
    }
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("minimized witness replaces"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_performance_requires_correctness() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-perf", "perl-5.38");
    row.performance.correctness_eligible = true;
    row.performance.evidence_identity = Some("perf-packet:u17".to_string());
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("performance eligibility claimed"));

    let ok_dir = TempDir::new()?;
    let mut agreed = agreement_row("op-agreed-perf", "perl-5.38");
    agreed.performance.correctness_eligible = true;
    agreed.performance.evidence_identity = Some("perf-packet:u17".to_string());
    write_inputs(ok_dir.path(), &manifest, &[agreed])?;
    let rendered = render_markdown(&project_from(ok_dir.path())?)?;
    assert!(rendered.contains("- performance: correctness eligible"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_removed_cases_keep_obligations() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-removed", "perl-5.38");
    row.history.upstream_change = UpstreamChange::Removed;
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("removed upstream case loses"));

    let retained_dir = TempDir::new()?;
    let mut retained = failing_row("op-removed", "perl-5.38");
    retained.history.upstream_change = UpstreamChange::Removed;
    retained.history.retained_obligation_after_removal = true;
    write_inputs(retained_dir.path(), &manifest, &[retained])?;
    let rendered = render_markdown(&project_from(retained_dir.path())?)?;
    assert!(rendered.contains("semantic obligation retained locally"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_history_cannot_mask_currentness() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-stale", "perl-5.38");
    row.terminal_state = TerminalState::Stale;
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("historical predecessor"));

    let linked_dir = TempDir::new()?;
    let mut linked = failing_row("op-stale", "perl-5.38");
    linked.terminal_state = TerminalState::Stale;
    linked.history.predecessor_row_id = Some("hist:predecessor".to_string());
    write_inputs(linked_dir.path(), &manifest, &[linked])?;
    project_from(linked_dir.path())?;
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_absent_snapshot_reports_absence() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.42", None)]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-absent", "perl-5.42")])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("must be no_current_snapshot"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_private_surfaces_bounded_out() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    for leak in ["C:\\Users\\builder\\secret.json", "/home/builder/id_rsa", "${CI_TOKEN}"] {
        let mut row = agreement_row("op-leak", "perl-5.38");
        row.limitation = Some(LimitationRecord {
            statement: leak.to_string(),
            nonclaims: Vec::new(),
            claim_ceiling: "exact row claim only".to_string(),
        });
        row.terminal_state = TerminalState::AgreementWithDeclaredLimitation;
        write_inputs(dir.path(), &manifest, &[row])?;
        let message = error_message(project_from(dir.path()));
        assert!(
            message.contains("leaks host/private/path") || message.contains("absolute host path"),
            "leak `{leak}` must be bounded out"
        );
    }
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_markdown_drift_is_detected() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-doc", "perl-5.38")])?;
    let packet = project_from(dir.path())?;

    let packet_path = dir.path().join("packet.json");
    fs::write(&packet_path, canonical_bytes(&packet)?)?;
    let rendered = render_markdown(&packet)?;
    let committed = dir.path().join("view.md");
    fs::write(&committed, rendered.as_bytes())?;
    let matched = run_docs_check(&packet_path, &committed)?;
    assert!(matched.contains("matches its validated packet"));

    let drifted =
        rendered.replace("current result: `agreement_current`", "current result: `(hidden)`");
    assert_ne!(drifted, rendered);
    fs::write(&committed, drifted.as_bytes())?;
    let rejection = run_docs_check(&packet_path, &committed);
    let message = error_message(rejection);
    assert!(
        message.contains("drifts from its validated packet"),
        "hand-edited prose must never pass: {message}"
    );
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_diff_exposes_transitions_only() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-diff", "perl-5.38")])?;
    let before = project_from(dir.path())?;
    let before_path = dir.path().join("before.json");
    let after_path = dir.path().join("after.json");
    fs::write(&before_path, canonical_bytes(&before)?)?;
    let same = run_diff(&before_path, &before_path)?;
    assert!(same.contains("identical"));

    let mut after = before.clone();
    after.rows[0].terminal_state = TerminalState::CompilerFailed;
    after.rows[0].witness = None;
    after.descriptive_counts = compute_counts(&after.rows);
    fs::write(&after_path, canonical_bytes(&after)?)?;
    let message = error_message(run_diff(&before_path, &after_path));
    assert!(message.contains("`agreement_current` -> `compiler_failed`"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_multi_series_projection_is_exact() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![
        selector("perl-5.38", Some("upstream:v5.38.0")),
        selector("perl-5.40", Some("upstream:v5.40.0")),
        selector("perl-dev", None),
    ]);
    let mut limited = agreement_row("op-limited", "perl-5.40");
    limited.terminal_state = TerminalState::AgreementWithDeclaredLimitation;
    limited.limitation = Some(LimitationRecord {
        statement: "agreement holds for the declared scalar slice only".to_string(),
        nonclaims: vec!["no provider/editor behavior claim".to_string()],
        claim_ceiling: "declared scalar slice of perl-5.40 parser stage".to_string(),
    });
    limited.witness = installed_witness(&limited.upstream_case.case_path);
    let absent_snapshots =
        vec![agreement_row("op-z", "perl-5.38"), failing_row("op-red", "perl-5.38"), limited, {
            let mut pending = agreement_row("op-dev", "perl-dev");
            pending.terminal_state = TerminalState::NoCurrentSnapshot;
            pending.witness = None;
            pending
        }];
    // Counter-order filename writes; projection must canonicalize anyway.
    let mut shuffled = absent_snapshots.clone();
    shuffled.rotate_left(1);
    write_inputs(dir.path(), &manifest, &shuffled)?;
    let packet = project_from(dir.path())?;

    assert_eq!(packet.rows.len(), 4);
    let series_order: Vec<&str> = packet.rows.iter().map(|row| row.series_id.as_str()).collect();
    assert_eq!(series_order, vec!["perl-5.38", "perl-5.38", "perl-5.40", "perl-dev"]);

    let counts = &packet.descriptive_counts;
    assert_eq!(counts.total_rows, 4);
    assert_eq!(counts.by_terminal_state.get(&TerminalState::AgreementCurrent), Some(&1));
    assert_eq!(counts.by_terminal_state.get(&TerminalState::CompilerFailed), Some(&1));
    assert_eq!(
        counts.by_terminal_state.get(&TerminalState::AgreementWithDeclaredLimitation),
        Some(&1)
    );
    assert_eq!(counts.by_terminal_state.get(&TerminalState::NoCurrentSnapshot), Some(&1));

    let rendered = render_markdown(&packet)?;
    assert!(rendered.contains("Exact row denominator: 4."));
    assert!(rendered.contains("| compiler_failed | 1 |"));
    assert!(rendered.contains("no provider/editor behavior claim"));
    assert!(rendered.contains("(no accepted current upstream snapshot)"));
    assert!(rendered.contains("compiler_profile_generation_identity_informational_only"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_row_from_different_valid_snapshot() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = agreement_row("op-wrong-snapshot", "perl-5.38");
    row.upstream_case.snapshot_ref = "upstream:v5.40.0".to_string();
    write_inputs(dir.path(), &manifest, &[row])?;

    let message = error_message(project_from(dir.path()));
    assert!(message.contains("does not match selected snapshot"));
    assert!(message.contains("upstream:v5.40.0"));
    assert!(message.contains("upstream:v5.38.0"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_wrong_snapshot_at_every_packet_ingress()
-> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-ingress", "perl-5.38")])?;
    let valid = project_from(dir.path())?;
    let mut invalid = valid.clone();
    invalid.rows[0].upstream_case.snapshot_ref = "upstream:v5.40.0".to_string();
    let packet_path = dir.path().join("invalid.json");
    let valid_path = dir.path().join("valid.json");
    let view_path = dir.path().join("view.md");
    fs::write(&packet_path, canonical_bytes(&invalid)?)?;
    fs::write(&valid_path, canonical_bytes(&valid)?)?;

    for result in [
        validate_packet(&invalid).map(|_| ()),
        run_check(&packet_path).map(|_| ()),
        run_show(&packet_path, None, None).map(|_| ()),
        run_diff(&valid_path, &packet_path).map(|_| ()),
        run_docs(&packet_path, &view_path).map(|_| ()),
        run_docs_check(&packet_path, &view_path).map(|_| ()),
    ] {
        let message = error_message(result);
        assert!(
            message.contains("does not match selected snapshot"),
            "wrong snapshot must be rejected at packet ingress: {message}"
        );
    }
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_limitation_requires_installed_witness() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = agreement_row("op-limited-witness", "perl-5.38");
    row.terminal_state = TerminalState::AgreementWithDeclaredLimitation;
    row.limitation = Some(LimitationRecord {
        statement: "agreement holds for the declared slice".to_string(),
        nonclaims: vec!["no broader claim".to_string()],
        claim_ceiling: "declared slice only".to_string(),
    });
    row.witness = None;
    write_inputs(dir.path(), &manifest, std::slice::from_ref(&row))?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("requires a recorded installed witness"), "{message}");

    let witness = installed_witness("t/op/op-limited-witness.t")
        .ok_or_else(|| anyhow::anyhow!("fixture witness must be present"))?;
    row.witness = Some(witness.clone());
    write_inputs(dir.path(), &manifest, std::slice::from_ref(&row))?;
    project_from(dir.path())?;

    row.witness =
        Some(WitnessRecord { installation: WitnessInstallation::NotInstalled, ..witness });
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("requires an installed witness"), "{message}");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_dangling_successor() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-removed", "perl-5.38");
    row.history.upstream_change = UpstreamChange::Removed;
    row.history.successor_row_id = Some("op-missing".to_string());
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("successor_row_id") && message.contains("missing row"), "{message}");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_ineligible_evidence_and_private_bindings()
-> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-evidence", "perl-5.38")])?;
    let mut packet = project_from(dir.path())?;
    packet.rows[0].performance.evidence_identity = Some("perf:unreported".to_string());
    let message = error_message(validate_packet(&packet));
    assert!(message.contains("without correctness eligibility"), "{message}");

    packet = project_from(dir.path())?;
    packet.subject_binding.toolchain_build_identity = "C:\\Users\\builder\\secret".to_string();
    let message = error_message(validate_packet(&packet));
    assert!(
        message.contains("host/private/path") || message.contains("absolute host path"),
        "{message}"
    );

    packet = project_from(dir.path())?;
    packet.subject_binding.compiler_candidate_identity = "/opt/run/secrets/x".to_string();
    let message = error_message(validate_packet(&packet));
    assert!(message.contains("absolute POSIX path"), "{message}");

    packet = project_from(dir.path())?;
    packet.subject_binding.semantic_obligation_graph_identity =
        Some("/opt/run/secrets/x".to_string());
    let message = error_message(validate_packet(&packet));
    assert!(message.contains("absolute POSIX path"), "{message}");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rendering_rejects_markdown_injection() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-render-safe", "perl-5.38")])?;

    let mut packet = project_from(dir.path())?;
    packet.rows[0].owner.canonical_owner = "[forged](https://evil.example)".to_string();
    let rendered = render_markdown(&packet)?;
    assert!(rendered.contains(r"\[forged\]\(https://evil\.example\)"));

    let mut packet = project_from(dir.path())?;
    packet.rows[0].upstream_case.case_name = "safe\n- forged status: pass".to_string();
    assert!(render_markdown(&packet).is_err(), "Markdown line injection was rendered");

    let mut packet = project_from(dir.path())?;
    packet.rows[0].upstream_case.case_path = "t/with`backtick.t".to_string();
    packet.rows[0].upstream_case.case_name = "name`with`backticks".to_string();
    packet.rows[0]
        .witness
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("fixture witness must be present"))?
        .minimizes_case_path = "t/witness`with`backtick.t".to_string();
    packet.subject_binding.semantic_obligation_graph_identity =
        Some("obligation`with`backticks".to_string());
    let rendered = render_markdown(&packet)?;
    assert!(rendered.contains("case_path=``t/with`backtick.t``"));
    assert!(rendered.contains("case_name=``name`with`backticks``"));
    assert!(rendered.contains("minimizes_case_path=``t/witness`with`backtick.t``"));
    assert!(
        rendered.contains("- semantic_obligation_graph_identity: ``obligation`with`backticks``")
    );

    let packet = project_from(dir.path())?;
    let rendered = render_markdown(&packet)?;
    assert!(rendered.contains(r"- owner: \#12532"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_multiline_limitation_in_any_state() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-unhandled-limitation", "perl-5.38");
    row.limitation = Some(LimitationRecord {
        statement: "first line\n- forged Markdown line".to_string(),
        nonclaims: Vec::new(),
        claim_ceiling: "bounded claim".to_string(),
    });
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(
        message.contains("limitation.statement") && message.contains("control characters"),
        "{message}"
    );
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_rejects_absolute_witness_and_absence_witnesses()
-> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = agreement_row("op-absolute-witness", "perl-5.38");
    row.witness
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("fixture witness must be present"))?
        .minimizes_case_path = "/tmp/private/minimized.t".to_string();
    write_inputs(dir.path(), &manifest, &[row])?;
    let message = error_message(project_from(dir.path()));
    assert!(message.contains("normalized relative path"), "{message}");

    for invalid_path in ["../private/minimized.t", "~/private/minimized.t"] {
        let dir = TempDir::new()?;
        let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
        let mut row = agreement_row("op-invalid-witness-path", "perl-5.38");
        row.witness
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture witness must be present"))?
            .minimizes_case_path = invalid_path.to_string();
        write_inputs(dir.path(), &manifest, &[row])?;
        let message = error_message(project_from(dir.path()));
        assert!(message.contains("normalized relative path"), "{message}");
    }

    for (index, (state, snapshot)) in [
        (TerminalState::NoCurrentSnapshot, None),
        (TerminalState::NoCurrentCompilerObservation, Some("upstream:v5.38.0")),
        (TerminalState::ClassificationPending, Some("upstream:v5.38.0")),
        (TerminalState::WitnessPending, Some("upstream:v5.38.0")),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = TempDir::new()?;
        let manifest = base_manifest(vec![selector("perl-5.38", snapshot)]);
        let mut row = agreement_row(&format!("op-absence-witness-{index}"), "perl-5.38");
        row.terminal_state = state;
        write_inputs(dir.path(), &manifest, &[row])?;
        let message = error_message(project_from(dir.path()));
        assert!(
            message.contains(&format!("{} conflicts with a present witness", state.as_str())),
            "{message}"
        );
    }
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_accepts_unsupported_boundary_and_validates_rendered_ids()
-> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    let mut row = failing_row("op-unsupported", "perl-5.38");
    row.terminal_state = TerminalState::UnsupportedOrExternalBoundary;
    row.support_boundary = SupportBoundary::Unsupported;
    write_inputs(dir.path(), &manifest, &[row])?;
    project_from(dir.path())?;

    let mut packet = project_from(dir.path())?;
    packet.status_id = "/private/status".to_string();
    let message = error_message(validate_packet(&packet));
    assert!(message.contains("status_id") && message.contains("absolute POSIX path"), "{message}");

    let mut packet = project_from(dir.path())?;
    packet.subject_binding.maintained_series[0].series_id = "series[forged]".to_string();
    let message = error_message(validate_packet(&packet));
    assert!(
        message.contains("series.series_id") && message.contains("invalid identity charset"),
        "{message}"
    );
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_diff_reports_row_and_binding_changes() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-diff-fields", "perl-5.38")])?;
    let before = project_from(dir.path())?;
    let mut after = before.clone();
    after.rows[0].compiler_subject = "compiler:other-tree".to_string();
    after.subject_binding.semantic_obligation_graph_identity =
        Some("obligation-graph:u09".to_string());
    let before_path = dir.path().join("before.json");
    let after_path = dir.path().join("after.json");
    fs::write(&before_path, canonical_bytes(&before)?)?;
    fs::write(&after_path, canonical_bytes(&after)?)?;
    let message = error_message(run_diff(&before_path, &after_path));
    assert!(message.contains("compiler_subject changed"), "{message}");
    assert!(message.contains("semantic_obligation_graph_identity changed"), "{message}");
    assert!(!message.contains("0 summarized differences"), "{message}");
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_markdown_metadata_is_line_terminated() -> TestResult {
    let dir = TempDir::new()?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(dir.path(), &manifest, &[agreement_row("op-metadata", "perl-5.38")])?;
    let rendered = render_markdown(&project_from(dir.path())?)?;
    assert!(rendered.contains("- compiler_candidate_identity: `candidate:local/main-worktree`\n- toolchain_build_identity:"));
    assert!(!rendered.contains("worktree`- toolchain"));
    Ok(())
}

#[test]
fn compiler_upstream_conformance_status_build_writes_canonical_file() -> TestResult {
    let dir = TempDir::new()?;
    let inputs = dir.path().join("inputs");
    fs::create_dir_all(inputs.join("rows"))?;
    let manifest = base_manifest(vec![selector("perl-5.38", Some("upstream:v5.38.0"))]);
    write_inputs(&inputs, &manifest, &[agreement_row("op-file", "perl-5.38")])?;
    let output = dir.path().join("nested/status.v1.json");
    let build_summary = run_build(&inputs, &output)?;
    assert!(build_summary.contains("rows=1"));
    let written = fs::read_to_string(&output)?;
    let reloaded: ConformanceStatusPacket = serde_json::from_str(&written)?;
    validate_packet(&reloaded)?;
    assert!(written.ends_with('\n'));
    assert!(!written.contains("\r\n"));
    let shown = run_show(&output, Some("perl-5.38"), Some("array-return-shape"))?;
    assert!(shown.iter().any(|line| line.starts_with("row op-file ")));
    assert_eq!(shown.len(), 9);
    Ok(())
}
