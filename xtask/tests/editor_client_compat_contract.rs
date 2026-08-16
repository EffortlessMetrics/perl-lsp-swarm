#[path = "support/editor_client_compat.rs"]
mod editor_client_compat;

use anyhow::{Context, Result, ensure};
use editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_IDS, CANONICAL_EXPECTATION_SET_ID, CapabilityBasis,
    CapabilityIdentity, CleanupResult, ClientSourceState, DiagnosticMode, DiagnosticsIdentity,
    EditorClientCompatReceipt, EvidenceArtifact, EvidenceStage, FailureClass, HostIdentity,
    IntegrationIdentity, IntegrationMode, JourneyCell, ObservationResult,
    PROTOCOL_EVIDENCE_SCHEMA_VERSION, PlatformIdentity, PositionEncodingBasis, ProtocolEvidence,
    RegistrationState, SCHEMA_VERSION, ServerIdentity, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn fixture_root() -> Result<PathBuf> {
    Ok(repository_root()?.join("crates/perl-lsp-ux-tests/fixtures/agent-client-compat"))
}

fn sha256(fill: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    value.extend(std::iter::repeat_n(fill, 64));
    value
}

fn required_artifacts() -> Vec<EvidenceArtifact> {
    vec![
        EvidenceArtifact {
            kind: ArtifactKind::ClientLog,
            id: "artifacts/emacs-events.log".to_string(),
            sha256: sha256('5'),
        },
        EvidenceArtifact {
            kind: ArtifactKind::ServerStderr,
            id: "artifacts/perllsp.stderr".to_string(),
            sha256: sha256('6'),
        },
        EvidenceArtifact {
            kind: ArtifactKind::CapabilitySnapshot,
            id: "artifacts/initialize.json".to_string(),
            sha256: sha256('7'),
        },
        EvidenceArtifact {
            kind: ArtifactKind::ProcessLedger,
            id: "artifacts/process-ledger.json".to_string(),
            sha256: sha256('8'),
        },
    ]
}

fn valid_receipt() -> Result<EditorClientCompatReceipt> {
    Ok(EditorClientCompatReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        observed_at: "2026-08-13T10:30:00Z".to_string(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        candidate_sha: "a".repeat(40),
        platform: PlatformIdentity {
            os: "linux".to_string(),
            os_version: "ubuntu-24.04".to_string(),
            arch: "x86_64".to_string(),
        },
        host: HostIdentity {
            client_id: "emacs-eglot-1.23".to_string(),
            product: "emacs".to_string(),
            version: "31.1".to_string(),
            source_state: ClientSourceState::Released,
            source_ref: "gnu-elpa/eglot-1.23".to_string(),
            executable_sha256: sha256('1'),
        },
        integration: IntegrationIdentity {
            mode: IntegrationMode::GenericLsp,
            registration_state: RegistrationState::ManualClientRegistration,
            configuration_sha256: sha256('2'),
            driver_sha256: sha256('3'),
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: "0.18.0-dev".to_string(),
            build_revision: "a".repeat(40),
            artifact_sha256: sha256('4'),
            protocol_version: "3.17".to_string(),
            launch_command: vec!["perllsp".to_string(), "--stdio".to_string()],
        },
        workspace_fixture: WorkspaceFixtureIdentity {
            id: "perl-agent-client-v1".to_string(),
            digest: fixture_digest(&fixture_root()?)?,
            expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
            expectation_set_digest: canonical_expectation_set_digest()?,
        },
        capabilities: CapabilityIdentity {
            initialize_snapshot_sha256: sha256('9'),
            position_encodings_offered: vec!["utf-16".to_string()],
            position_encoding_basis: PositionEncodingBasis::Offered,
            position_encoding_selected: Some("utf-16".to_string()),
        },
        diagnostics: DiagnosticsIdentity {
            advertised_mode: DiagnosticMode::Pull,
            observed_messages: vec![
                "text_document_diagnostic".to_string(),
                "flymake_rendered".to_string(),
            ],
        },
        journey: vec![
            JourneyCell {
                id: "definition.cross_file".to_string(),
                capability_basis: CapabilityBasis::Advertised,
                observed: true,
                result: ObservationResult::Pass,
                evidence: vec!["host-event.definition".to_string()],
                limitation: None,
            },
            JourneyCell {
                id: "lifecycle.shutdown".to_string(),
                capability_basis: CapabilityBasis::NotApplicable,
                observed: true,
                result: ObservationResult::Pass,
                evidence: vec!["process-cleanup".to_string()],
                limitation: None,
            },
        ],
        protocol_evidence: None,
        process_cleanup: CleanupResult::Pass,
        result: ObservationResult::Pass,
        failure_class: None,
        limitations: Vec::new(),
        artifacts: required_artifacts(),
        claim_boundary: "Exact-source Eglot generic-LSP fixture cells only.".to_string(),
    })
}

/// Attach an `actual_host_receipt.v1` payload describing the same run as
/// [`valid_receipt`], after letting the caller mutate it.
///
/// The payload is deliberately built to agree with the wrapping receipt on every
/// shared fact, so each mutation below isolates exactly one disagreement.
fn with_protocol_evidence(
    mut receipt: EditorClientCompatReceipt,
    mutate: impl FnOnce(&mut Value),
) -> Result<EditorClientCompatReceipt> {
    let run_id = "emacs-eglot-exact-source-001";
    let mut payload = serde_json::json!({
        "schema_version": PROTOCOL_EVIDENCE_SCHEMA_VERSION,
        "receipt_version": 1,
        "run_id": run_id,
        "timestamp": receipt.observed_at.clone(),
        "editor": { "family": "emacs", "version": "31.1", "source": "gnu-release" },
        "client": { "family": "eglot", "version": "1.23", "source": "bundled" },
        "server": {
            "path": "artifacts/perllsp",
            "sha256": "4".repeat(64),
            "version": "0.18.0-dev"
        },
        "platform": { "os": "linux", "arch": "x86_64" },
        "workspace": { "root": "fixture/perl-agent-client-v1", "identity": "fixture:git-root" },
        "profile": { "identity": "clean-home-001", "source": "hermetic-runner" },
        "registration_state": "manual_client_registration",
        "artifacts": {
            "client_log": "artifacts/emacs-events.log",
            "server_stderr": "artifacts/perllsp.stderr"
        },
        "features": {
            "diagnostics": { "advertised": true, "observed": true, "outcome": "passed" }
        },
        "state_machine": {
            "initialize": { "outcome": "ok" },
            "initialized": { "outcome": "ok" },
            "position_encoding": "utf-16",
            "diagnostics_mode": "pull",
            "diagnostics_response_form": "full",
            "workspace_configuration": { "outcome": "ok" },
            "register_capability": { "outcome": "ok" },
            "watcher_behavior": { "outcome": "ok" },
            "refresh": { "outcome": "ok" },
            "shutdown": { "outcome": "ok" },
            "exit": { "outcome": "ok" },
            "orphan_result": "none"
        }
    });
    mutate(&mut payload);

    receipt.protocol_evidence = Some(ProtocolEvidence {
        run_id: run_id.to_string(),
        receipt_sha256: sha256('0'),
        receipt: payload,
    });
    Ok(receipt)
}

#[test]
fn editor_client_receipt_round_trips_and_validates() -> Result<()> {
    let receipt = valid_receipt()?;
    receipt.validate()?;

    let encoded = serde_json::to_string_pretty(&receipt)?;
    let decoded: EditorClientCompatReceipt = serde_json::from_str(&encoded)?;
    ensure!(decoded == receipt, "serialized receipt did not round-trip exactly");
    ensure!(
        serde_json::to_string_pretty(&decoded)? == encoded,
        "receipt serialization was not deterministic"
    );

    let value = serde_json::to_value(&receipt)?;
    ensure!(
        value.get("limitations").is_some_and(Value::is_array),
        "limitations must serialize when empty"
    );
    ensure!(value.get("artifacts").is_some_and(Value::is_array), "artifacts must always serialize");
    Ok(())
}

#[test]
fn evidence_registration_source_and_diagnostic_boundaries_serialize_distinctly() -> Result<()> {
    let stages = [
        EvidenceStage::ExactSourceLocal,
        EvidenceStage::ReleaseCandidate,
        EvidenceStage::PublicArtifact,
    ];
    let stage_json =
        stages.iter().map(serde_json::to_value).collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        stage_json
            == vec![
                Value::String("exact_source_local".to_string()),
                Value::String("release_candidate".to_string()),
                Value::String("public_artifact".to_string()),
            ],
        "evidence stages changed serialization identity"
    );

    let source_states = [
        ClientSourceState::Bundled,
        ClientSourceState::Released,
        ClientSourceState::UpstreamSource,
    ];
    let source_json = source_states
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        source_json
            == vec![
                Value::String("bundled".to_string()),
                Value::String("released".to_string()),
                Value::String("upstream_source".to_string()),
            ],
        "client source states changed serialization identity"
    );

    let registrations = [
        RegistrationState::ManualClientRegistration,
        RegistrationState::UpstreamSourceRegistration,
        RegistrationState::UpstreamAcceptedUnreleased,
        RegistrationState::UpstreamBuiltinReleased,
    ];
    let registration_json = registrations
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        registration_json
            == vec![
                Value::String("manual_client_registration".to_string()),
                Value::String("upstream_source_registration".to_string()),
                Value::String("upstream_accepted_unreleased".to_string()),
                Value::String("upstream_builtin_released".to_string()),
            ],
        "registration states changed serialization identity"
    );

    let diagnostics = [
        DiagnosticMode::Push,
        DiagnosticMode::Pull,
        DiagnosticMode::Both,
        DiagnosticMode::None,
        DiagnosticMode::Malformed,
        DiagnosticMode::NotProven,
    ];
    let diagnostic_json =
        diagnostics.iter().map(serde_json::to_value).collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        diagnostic_json
            == vec![
                Value::String("push".to_string()),
                Value::String("pull".to_string()),
                Value::String("both".to_string()),
                Value::String("none".to_string()),
                Value::String("malformed".to_string()),
                Value::String("not_proven".to_string()),
            ],
        "diagnostic modes changed serialization identity"
    );

    let encoding_bases = [
        PositionEncodingBasis::Offered,
        PositionEncodingBasis::ProtocolDefault,
        PositionEncodingBasis::NotProven,
    ];
    let encoding_json = encoding_bases
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        encoding_json
            == vec![
                Value::String("offered".to_string()),
                Value::String("protocol_default".to_string()),
                Value::String("not_proven".to_string()),
            ],
        "position encoding bases changed serialization identity"
    );
    Ok(())
}

#[test]
fn protocol_default_utf16_is_distinct_from_an_offered_encoding() -> Result<()> {
    let mut receipt = valid_receipt()?;
    receipt.capabilities.position_encodings_offered.clear();
    receipt.capabilities.position_encoding_basis = PositionEncodingBasis::ProtocolDefault;
    receipt.capabilities.position_encoding_selected = Some("utf-16".to_string());
    receipt.validate()?;

    receipt.capabilities.position_encoding_selected = Some("utf-8".to_string());
    ensure!(receipt.validate().is_err(), "protocol-default basis accepted a non-UTF-16 selection");
    Ok(())
}

#[test]
fn validation_rejects_false_actual_host_and_wrong_subject_shapes() -> Result<()> {
    let mut wrong_executable = valid_receipt()?;
    wrong_executable.server.executable = "perl-lsp".to_string();
    ensure!(wrong_executable.validate().is_err(), "non-canonical server executable was accepted");

    let mut wrong_launch = valid_receipt()?;
    wrong_launch.server.launch_command = vec!["perllsp".to_string()];
    ensure!(wrong_launch.validate().is_err(), "incomplete launch command was accepted");

    let mut selected_not_offered = valid_receipt()?;
    selected_not_offered.capabilities.position_encoding_selected = Some("utf-8".to_string());
    ensure!(
        selected_not_offered.validate().is_err(),
        "selected encoding absent from client offer was accepted"
    );

    let mut duplicate_encoding = valid_receipt()?;
    duplicate_encoding.capabilities.position_encodings_offered.push("utf-16".to_string());
    ensure!(duplicate_encoding.validate().is_err(), "duplicate offered encoding was accepted");

    let mut diagnostic_not_proven = valid_receipt()?;
    diagnostic_not_proven.diagnostics.advertised_mode = DiagnosticMode::NotProven;
    diagnostic_not_proven.diagnostics.observed_messages.clear();
    ensure!(
        diagnostic_not_proven.validate().is_err(),
        "passing receipt left diagnostic mode not proven"
    );

    let mut diagnostic_unobserved = valid_receipt()?;
    diagnostic_unobserved.diagnostics.observed_messages.clear();
    ensure!(
        diagnostic_unobserved.validate().is_err(),
        "passing pull receipt omitted diagnostic observations"
    );

    let mut cleanup_not_proven = valid_receipt()?;
    cleanup_not_proven.process_cleanup = CleanupResult::NotProven;
    ensure!(cleanup_not_proven.validate().is_err(), "passing receipt omitted proven cleanup");

    let mut missing_artifact = valid_receipt()?;
    missing_artifact.artifacts.retain(|artifact| artifact.kind != ArtifactKind::ServerStderr);
    ensure!(
        missing_artifact.validate().is_err(),
        "passing actual-host receipt omitted server stderr"
    );
    Ok(())
}

#[test]
fn validation_rejects_false_green_cells_and_unsafe_artifact_identity() -> Result<()> {
    let mut duplicate_cell = valid_receipt()?;
    let first =
        duplicate_cell.journey.first().context("valid receipt has no journey cell")?.clone();
    duplicate_cell.journey.push(first);
    ensure!(duplicate_cell.validate().is_err(), "duplicate journey id was accepted");

    let mut false_green = valid_receipt()?;
    let first = false_green.journey.first_mut().context("valid receipt has no journey cell")?;
    first.result = ObservationResult::NotProven;
    first.limitation = Some("host did not expose the requested action".to_string());
    ensure!(false_green.validate().is_err(), "passing receipt accepted a not-proven journey cell");

    let mut unsupported_without_limitation = valid_receipt()?;
    let first = unsupported_without_limitation
        .journey
        .first_mut()
        .context("valid receipt has no journey cell")?;
    first.result = ObservationResult::Unsupported;
    first.observed = false;
    ensure!(
        unsupported_without_limitation.validate().is_err(),
        "unsupported journey cell omitted its limitation"
    );

    let mut nothing_observed = valid_receipt()?;
    for cell in &mut nothing_observed.journey {
        cell.capability_basis = CapabilityBasis::NotAdvertised;
        cell.observed = false;
        cell.result = ObservationResult::Unsupported;
        cell.limitation = Some("host does not implement this action".to_string());
    }
    ensure!(
        nothing_observed.validate().is_err(),
        "passing receipt accepted a journey in which nothing was observed to work"
    );

    let mut absolute_artifact = valid_receipt()?;
    absolute_artifact.artifacts[0].id = "/home/user/emacs-events.log".to_string();
    ensure!(absolute_artifact.validate().is_err(), "Unix absolute artifact identity was accepted");

    let mut drive_artifact = valid_receipt()?;
    drive_artifact.artifacts[0].id = "C:/Users/alice/emacs-events.log".to_string();
    ensure!(
        drive_artifact.validate().is_err(),
        "Windows drive-qualified artifact identity was accepted"
    );

    let mut uri_artifact = valid_receipt()?;
    uri_artifact.artifacts[0].id = "file:///home/alice/emacs-events.log".to_string();
    ensure!(uri_artifact.validate().is_err(), "URI-qualified artifact identity was accepted");

    let mut private_source = valid_receipt()?;
    private_source.host.source_ref = "../../private/checkout".to_string();
    ensure!(private_source.validate().is_err(), "parent-traversing client source ref was accepted");
    Ok(())
}

#[test]
fn a_cell_cannot_pass_a_capability_the_host_never_advertised() -> Result<()> {
    let mut unadvertised_observation = valid_receipt()?;
    let first = unadvertised_observation
        .journey
        .first_mut()
        .context("valid receipt has no journey cell")?;
    first.capability_basis = CapabilityBasis::NotAdvertised;
    ensure!(
        unadvertised_observation.validate().is_err(),
        "cell observed a capability the host never advertised"
    );

    let mut unobserved_pass = valid_receipt()?;
    let first = unobserved_pass.journey.first_mut().context("valid receipt has no journey cell")?;
    first.observed = false;
    ensure!(unobserved_pass.validate().is_err(), "cell passed without observing anything");

    let mut observed_unsupported = valid_receipt()?;
    let first =
        observed_unsupported.journey.first_mut().context("valid receipt has no journey cell")?;
    first.result = ObservationResult::Unsupported;
    first.limitation = Some("host does not expose this action".to_string());
    ensure!(
        observed_unsupported.validate().is_err(),
        "cell reported unsupported while also reporting an observation"
    );

    // `not_applicable` exempts host-native cells from the advertisement rule, but
    // must not become a way to manufacture a pass out of nothing.
    let mut host_native_without_observation = valid_receipt()?;
    let first = host_native_without_observation
        .journey
        .first_mut()
        .context("valid receipt has no journey cell")?;
    first.capability_basis = CapabilityBasis::NotApplicable;
    first.observed = false;
    ensure!(
        host_native_without_observation.validate().is_err(),
        "host-native cell passed without an observation"
    );
    Ok(())
}

#[test]
fn embedded_protocol_evidence_must_describe_the_same_run() -> Result<()> {
    let receipt = valid_receipt()?;
    let composed = with_protocol_evidence(receipt.clone(), |_| ())?;
    composed.validate()?;

    let wrong_registration = with_protocol_evidence(receipt.clone(), |payload| {
        payload["registration_state"] = Value::String("upstream_builtin_released".to_string());
    })?;
    ensure!(
        wrong_registration.validate().is_err(),
        "embedded receipt claimed a different registration state"
    );

    let wrong_server = with_protocol_evidence(receipt.clone(), |payload| {
        payload["server"]["sha256"] = Value::String("f".repeat(64));
    })?;
    ensure!(wrong_server.validate().is_err(), "embedded receipt bound a different server artifact");

    let wrong_platform = with_protocol_evidence(receipt.clone(), |payload| {
        payload["platform"]["arch"] = Value::String("aarch64".to_string());
    })?;
    ensure!(wrong_platform.validate().is_err(), "embedded receipt claimed a different platform");

    let wrong_editor = with_protocol_evidence(receipt.clone(), |payload| {
        payload["editor"]["version"] = Value::String("29.4".to_string());
    })?;
    ensure!(
        wrong_editor.validate().is_err(),
        "embedded receipt claimed a different editor version"
    );

    let wrong_encoding = with_protocol_evidence(receipt.clone(), |payload| {
        payload["state_machine"]["position_encoding"] = Value::String("utf-8".to_string());
    })?;
    ensure!(
        wrong_encoding.validate().is_err(),
        "embedded receipt negotiated a different position encoding"
    );

    let wrong_diagnostics = with_protocol_evidence(receipt.clone(), |payload| {
        payload["state_machine"]["diagnostics_mode"] = Value::String("push".to_string());
    })?;
    ensure!(
        wrong_diagnostics.validate().is_err(),
        "embedded receipt reported a different diagnostic mode"
    );

    // The embedded payload is checked by the production validator, not a second
    // copy of its rules: a receipt this contract would otherwise accept must still
    // fail when the payload violates `actual_host_receipt.v1` itself.
    let invalid_payload = with_protocol_evidence(receipt.clone(), |payload| {
        payload["features"]["diagnostics"]["advertised"] = Value::Bool(false);
    })?;
    ensure!(
        invalid_payload.validate().is_err(),
        "an embedded payload invalid under its own contract was accepted"
    );

    let wrong_schema = with_protocol_evidence(receipt.clone(), |payload| {
        payload["schema_version"] = Value::String(SCHEMA_VERSION.to_string());
    })?;
    ensure!(wrong_schema.validate().is_err(), "embedded payload in the wrong dialect was accepted");

    // An orphan is a cleanup failure in whichever dialect observed it.
    let orphaned = with_protocol_evidence(receipt, |payload| {
        payload["state_machine"]["orphan_result"] = Value::String("orphan_detected".to_string());
    })?;
    ensure!(
        orphaned.validate().is_err(),
        "embedded receipt detected an orphan while cleanup was reported as passing"
    );
    Ok(())
}

#[test]
fn the_two_contracts_share_one_registration_vocabulary() -> Result<()> {
    let peer_path = repository_root()?.join("contracts/actual_host_receipt.v1.schema.json");
    let peer: Value = serde_json::from_str(&fs::read_to_string(peer_path)?)?;
    let schema_path = repository_root()?.join(".ci/schemas/editor-client-compat.v1.schema.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(schema_path)?)?;

    ensure!(
        peer["properties"]["schema_version"]["const"] == PROTOCOL_EVIDENCE_SCHEMA_VERSION,
        "protocol evidence contract is not the one this contract composes with"
    );
    ensure!(
        schema["$defs"]["protocolEvidence"]["properties"]["receipt"]["properties"]["schema_version"]
            ["const"]
            == PROTOCOL_EVIDENCE_SCHEMA_VERSION,
        "schema does not bind embedded evidence to the peer contract"
    );

    // Both contracts name the registration state of the same run. Binding the
    // enums to each other means the vocabularies cannot drift apart silently:
    // editing either schema alone fails here.
    ensure!(
        schema["$defs"]["integration"]["properties"]["registration_state"]["enum"]
            == peer["properties"]["registration_state"]["enum"],
        "registration vocabularies diverged between the two contracts"
    );

    // The state-machine facts stay owned by the peer contract. Re-declaring them
    // here is what duplicate authority would look like.
    let owned_by_peer = peer["$defs"]["stateMachine"]["required"]
        .as_array()
        .context("peer state machine required list missing")?;
    for fact in owned_by_peer {
        let name = fact.as_str().context("peer required property name is not a string")?;
        ensure!(
            schema["properties"].get(name).is_none(),
            "this contract re-declared {name}, a fact {PROTOCOL_EVIDENCE_SCHEMA_VERSION} owns"
        );
    }
    Ok(())
}

#[test]
fn non_passing_receipts_require_failure_class_and_limitations() -> Result<()> {
    let mut failed_without_class = valid_receipt()?;
    failed_without_class.result = ObservationResult::Fail;
    failed_without_class.limitations = vec!["actual host returned a terminal failure".to_string()];
    ensure!(failed_without_class.validate().is_err(), "failed receipt omitted failure_class");

    let mut not_proven_without_limitation = valid_receipt()?;
    not_proven_without_limitation.result = ObservationResult::NotProven;
    not_proven_without_limitation.failure_class = Some(FailureClass::Instrument);
    ensure!(
        not_proven_without_limitation.validate().is_err(),
        "not-proven receipt omitted its limitation"
    );

    let mut cleanup_failure = valid_receipt()?;
    cleanup_failure.result = ObservationResult::Fail;
    cleanup_failure.failure_class = Some(FailureClass::Cleanup);
    cleanup_failure.process_cleanup = CleanupResult::Fail;
    cleanup_failure.limitations = vec!["candidate process remained alive".to_string()];
    cleanup_failure.validate()?;
    Ok(())
}

#[test]
fn subject_invalidation_is_identity_based_not_age_based() -> Result<()> {
    let previous = valid_receipt()?;
    let mut current = previous.clone();
    current.observed_at = "2026-09-01T00:00:00Z".to_string();
    ensure!(
        previous.subject_invalidations_against(&current).is_empty(),
        "observation age invalidated an unchanged subject"
    );

    current.stage = EvidenceStage::PublicArtifact;
    current.candidate_sha = "b".repeat(40);
    current.platform.arch = "aarch64".to_string();
    current.host.version = "31.2".to_string();
    current.integration.registration_state = RegistrationState::UpstreamBuiltinReleased;
    current.integration.configuration_sha256 = sha256('a');
    current.integration.driver_sha256 = sha256('b');
    current.server.artifact_sha256 = sha256('c');
    current.server.protocol_version = "3.18".to_string();
    current.server.launch_command =
        vec!["perllsp".to_string(), "--stdio".to_string(), "--bad".to_string()];
    current.workspace_fixture.digest = sha256('d');
    current.capabilities.initialize_snapshot_sha256 = sha256('e');
    current.diagnostics.advertised_mode = DiagnosticMode::Both;
    current.artifacts[0].sha256 = sha256('f');
    current.claim_boundary = "Public-artifact cells only.".to_string();

    ensure!(
        previous.subject_invalidations_against(&current)
            == BTreeSet::from([
                "artifacts",
                "candidate",
                "capabilities",
                "claim_boundary",
                "configuration",
                "diagnostics",
                "driver",
                "evidence_stage",
                "fixture",
                "host",
                "launch",
                "platform",
                "protocol",
                "registration",
                "server",
            ]),
        "subject invalidation did not report every changed identity dimension"
    );
    Ok(())
}

#[test]
fn shared_fixture_and_expectation_set_are_deterministic_and_source_bound() -> Result<()> {
    let root = fixture_root()?;
    let fixture_first = fixture_digest(&root)?;
    let fixture_second = fixture_digest(&root)?;
    ensure!(fixture_first == fixture_second, "canonical fixture digest was not deterministic");

    let expectation_first = canonical_expectation_set_digest()?;
    let expectation_second = canonical_expectation_set_digest()?;
    ensure!(
        expectation_first == expectation_second,
        "expectation-set digest was not deterministic"
    );
    ensure!(
        expectation_first.starts_with("sha256:") && expectation_first.len() == "sha256:".len() + 64,
        "expectation-set digest had the wrong identity shape"
    );

    let expectations = CANONICAL_EXPECTATION_IDS.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        expectations.len() == CANONICAL_EXPECTATION_IDS.len(),
        "canonical expectation ids contain duplicates"
    );
    ensure!(expectations.contains("definition.widget_new"), "definition expectation is missing");
    ensure!(
        expectations.contains("workspace.partial_not_ready"),
        "workspace readiness expectation is missing"
    );

    let copy = TempDir::new()?;
    copy_fixture(&root, copy.path())?;
    let before = fixture_digest(copy.path())?;
    fs::write(copy.path().join("app.pl"), b"# mutated\n")?;
    ensure!(fixture_digest(copy.path())? != before, "fixture digest ignored changed file bytes");

    let files = WalkFixture::new(&root)?.files;
    ensure!(
        files
            == BTreeSet::from([
                "app.pl".to_string(),
                "broken.pl".to_string(),
                "lib/Widget.pm".to_string(),
                "unicode.pl".to_string(),
            ]),
        "canonical fixture membership drifted"
    );
    Ok(())
}

#[test]
fn checked_in_schema_names_the_same_contract_and_subject_boundaries() -> Result<()> {
    let schema_path = repository_root()?.join(".ci/schemas/editor-client-compat.v1.schema.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(schema_path)?)?;

    ensure!(schema["title"] == SCHEMA_VERSION, "schema title drifted from Rust contract");
    ensure!(
        schema["properties"]["schema_version"]["const"] == SCHEMA_VERSION,
        "schema version const drifted from Rust contract"
    );

    for expected in ["exact_source_local", "release_candidate", "public_artifact"] {
        ensure!(
            schema["properties"]["stage"]["enum"]
                .as_array()
                .context("stage enum missing")?
                .iter()
                .any(|value| value.as_str() == Some(expected)),
            "missing evidence stage {expected}"
        );
    }
    for expected in [
        "manual_client_registration",
        "upstream_source_registration",
        "upstream_accepted_unreleased",
        "upstream_builtin_released",
    ] {
        ensure!(
            schema["$defs"]["integration"]["properties"]["registration_state"]["enum"]
                .as_array()
                .context("registration enum missing")?
                .iter()
                .any(|value| value.as_str() == Some(expected)),
            "missing registration state {expected}"
        );
    }

    let serialized = serde_json::to_value(valid_receipt()?)?;
    for required in schema["required"].as_array().context("top-level required list missing")? {
        let name = required.as_str().context("required property name is not a string")?;
        ensure!(
            serialized.get(name).is_some(),
            "valid receipt omitted schema-required property {name}"
        );
    }
    Ok(())
}

struct WalkFixture {
    files: BTreeSet<String>,
}

impl WalkFixture {
    fn new(root: &Path) -> Result<Self> {
        let mut files = BTreeSet::new();
        for entry in WalkDir::new(root) {
            let entry = entry?;
            if entry.file_type().is_file() {
                files.insert(entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(Self { files })
    }
}

fn copy_fixture(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
