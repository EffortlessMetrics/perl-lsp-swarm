//! Fixture-driven proof for the reload contract.
//!
//! Consumes the machine-checkable corpus under
//! `.spec/10097-loaded-module-reload-contract/`: the classification corpus,
//! the transaction corpus, the negative controls with their expected
//! reason codes, and the schema/enum sync. These tests are the
//! machine-check that binds the `.spec` documents to this module's closed
//! vocabularies; the fixtures live outside the crate and are read relative
//! to `CARGO_MANIFEST_DIR` (in-repo proof only, following the
//! `.spec/10690` crate-test precedent).

use super::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Operation identity used when applying the clock in this corpus.
///
/// These fixtures assert only what the clock *did* — its effect code and
/// whether it advanced — and never project a witness to the wire, so the
/// identity that binds a witness to its transaction is immaterial here.
/// The projector's ownership guard is proven in `reload_family`.
const FIXTURE_OPERATION: u64 = 1;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Harness-local reason code: the clock advanced but the candidate claims
/// the generation stayed current (negative control 6).
const POSSIBLY_APPLIED_WITHOUT_ADVANCE: &str = "possibly_applied_without_generation_advance";
/// Harness-local reason code: the candidate claims an advance the outcome
/// does not earn.
const ADVANCE_WITHOUT_POSSIBLY_APPLIED: &str = "generation_advanced_without_possibly_applied";

fn spec_dir() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.spec/10097-loaded-module-reload-contract")
        .canonicalize()
        .map_err(|error| format!("spec bundle must exist in-repo: {error}"))
}

fn read_json_dir(dir: &str) -> Result<Vec<(String, Value)>, String> {
    let path = spec_dir()?.join(dir);
    let entries =
        fs::read_dir(&path).map_err(|error| format!("{dir} must be readable: {error}"))?;
    let mut documents: Vec<(String, Value)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{dir} entry must be readable: {error}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") {
            let bytes = fs::read(entry.path()).map_err(|error| format!("{dir}/{name}: {error}"))?;
            let value = serde_json::from_slice(&bytes)
                .map_err(|error| format!("{dir}/{name} must be valid JSON: {error}"))?;
            documents.push((name, value));
        }
    }
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    if documents.is_empty() {
        return Err(format!("{dir} must contain fixtures"));
    }
    Ok(documents)
}

fn string_field(value: &Value, field: &str) -> Result<String, String> {
    value[field]
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("field {field} must be a string"))
}

// ---------------------------------------------------------------------------
// Classification corpus
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ClassificationFixture {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    fixture_class: String,
    observation: ObservationDoc,
    expected_disposition: String,
}

#[derive(Deserialize)]
struct ObservationDoc {
    stopped_and_command_ready: bool,
    runtime_supported: bool,
    loaded_in_runtime: bool,
    within_launch_authority: bool,
    runtime_mapping_unambiguous: bool,
    identity_binding_complete: bool,
    identity_current: bool,
    client_source_matches_saved: bool,
    module_classification: String,
    active_frame_in_target: bool,
}

impl ObservationDoc {
    fn to_observation(&self) -> Result<ReloadAdmissionObservation, String> {
        let module_classification = ModuleClassification::parse(&self.module_classification)
            .ok_or_else(|| format!("unknown classification {}", self.module_classification))?;
        Ok(ReloadAdmissionObservation {
            stopped_and_command_ready: self.stopped_and_command_ready,
            runtime_supported: self.runtime_supported,
            loaded_in_runtime: self.loaded_in_runtime,
            within_launch_authority: self.within_launch_authority,
            runtime_mapping_unambiguous: self.runtime_mapping_unambiguous,
            identity_binding_complete: self.identity_binding_complete,
            identity_current: self.identity_current,
            client_source_matches_saved: self.client_source_matches_saved,
            module_classification,
            active_frame_in_target: self.active_frame_in_target,
        })
    }
}

#[test]
fn classification_corpus_is_deterministic_and_covers_every_disposition() -> TestResult {
    let documents = read_json_dir("fixtures/classification")?;
    let mut covered = BTreeSet::new();
    for (file, value) in documents {
        let fixture: ClassificationFixture = serde_json::from_value(value)
            .map_err(|error| format!("classification/{file} must deserialize: {error}"))?;
        let expected = LoadedModuleReloadEligibility::parse(&fixture.expected_disposition)
            .ok_or_else(|| format!("unknown expected disposition in {file}"))?;
        let observation = fixture.observation.to_observation()?;
        let actual = classify_reload_eligibility(&observation);
        assert_eq!(
            actual, expected,
            "classification/{file} must classify to {}",
            fixture.expected_disposition
        );
        covered.insert(expected.as_str());
    }
    let expected: BTreeSet<&str> =
        LoadedModuleReloadEligibility::ALL.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(covered, expected, "every disposition must be reached by the corpus");
    Ok(())
}

// ---------------------------------------------------------------------------
// Transaction corpus
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TransactionFixture {
    #[allow(dead_code)]
    name: String,
    phase: String,
    outcome: OutcomeDoc,
    expected_generation_effect: String,
    expected_phase_valid: bool,
    expected_clean_projection: bool,
}

#[derive(Deserialize)]
struct OutcomeDoc {
    kind: String,
    disposition: Option<String>,
    failure_phase: Option<String>,
    cause: Option<String>,
}

fn pre_mutation_cause(code: &str) -> Result<PreMutationFailureCause, String> {
    PreMutationFailureCause::ALL
        .into_iter()
        .find(|cause| cause.as_str() == code)
        .ok_or_else(|| format!("unknown pre-mutation cause {code}"))
}

fn indeterminate_cause(code: &str) -> Result<IndeterminateCause, String> {
    IndeterminateCause::ALL
        .into_iter()
        .find(|cause| cause.as_str() == code)
        .ok_or_else(|| format!("unknown indeterminate cause {code}"))
}

fn outcome_from_doc(doc: &OutcomeDoc) -> Result<LoadedModuleReloadOutcome, String> {
    match doc.kind.as_str() {
        "reloaded" => Ok(LoadedModuleReloadOutcome::Reloaded),
        "refused" => {
            let disposition = doc
                .disposition
                .as_deref()
                .and_then(LoadedModuleReloadEligibility::parse)
                .ok_or("refused outcome needs a valid disposition")?;
            Ok(LoadedModuleReloadOutcome::Refused { disposition })
        }
        "failed_before_mutation" => {
            let phase = doc
                .failure_phase
                .as_deref()
                .and_then(ReloadTransactionPhase::parse)
                .ok_or("failed outcome needs a valid phase")?;
            let cause = pre_mutation_cause(doc.cause.as_deref().unwrap_or_default())?;
            Ok(LoadedModuleReloadOutcome::FailedBeforeMutation { phase, cause })
        }
        "indeterminate_possibly_applied" => {
            let phase = doc
                .failure_phase
                .as_deref()
                .and_then(ReloadTransactionPhase::parse)
                .ok_or("indeterminate outcome needs a valid phase")?;
            let cause = indeterminate_cause(doc.cause.as_deref().unwrap_or_default())?;
            Ok(LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase, cause })
        }
        other => Err(format!("unknown outcome kind {other}")),
    }
}

#[test]
fn transaction_corpus_pins_generation_effect_phase_validity_and_projection() -> TestResult {
    let documents = read_json_dir("fixtures/transactions")?;
    let mut kinds_covered = BTreeSet::new();
    for (file, value) in documents {
        let fixture: TransactionFixture = serde_json::from_value(value)
            .map_err(|error| format!("transactions/{file} must deserialize: {error}"))?;
        let phase = ReloadTransactionPhase::parse(&fixture.phase)
            .ok_or_else(|| format!("unknown phase {} in {file}", fixture.phase))?;
        let outcome = outcome_from_doc(&fixture.outcome)?;
        assert_eq!(
            outcome.generation_effect().as_str(),
            fixture.expected_generation_effect,
            "transactions/{file}: generation effect mismatch"
        );
        assert_eq!(
            phase_permits_outcome(phase, &outcome),
            fixture.expected_phase_valid,
            "transactions/{file}: phase validity mismatch"
        );
        assert_eq!(
            outcome.projects_as_clean(),
            fixture.expected_clean_projection,
            "transactions/{file}: clean projection mismatch"
        );
        // The generation clock must agree with the declared effect.
        let mut clock = RuntimeModuleGenerationClock::new();
        let advance = clock.apply(&outcome, FIXTURE_OPERATION);
        assert_eq!(
            advance.code(),
            fixture.expected_generation_effect,
            "transactions/{file}: clock must match the declared effect"
        );
        kinds_covered.insert(outcome.kind_code());
    }
    assert_eq!(
        kinds_covered,
        ["failed_before_mutation", "indeterminate_possibly_applied", "reloaded", "refused"]
            .into_iter()
            .collect(),
        "the corpus must cover all four outcome kinds"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NegativeControlDoc {
    #[allow(dead_code)]
    name: String,
    control: u8,
    op: String,
    input: Value,
    expected_error: String,
}

fn candidate_from_doc(value: &Value) -> SubjectCandidate {
    let field = |name: &str| value.get(name).cloned().unwrap_or(Value::Null);
    SubjectCandidate {
        session_generation: field("session_generation").as_u64(),
        suspension_generation: field("suspension_generation").as_u64(),
        observation_generation: field("observation_generation").as_u64(),
        inc_key: field("inc_key").as_str().unwrap_or_default().to_string(),
        resolved_runtime_path: field("resolved_runtime_path")
            .as_str()
            .unwrap_or_default()
            .to_string(),
        saved_content_digest: field("saved_content_digest")
            .as_str()
            .unwrap_or_default()
            .to_string(),
        logical_source_uri: field("logical_source_uri").as_str().unwrap_or_default().to_string(),
        perl_identity: field("perl_identity").as_str().unwrap_or_default().to_string(),
        launch_root: field("launch_root").as_str().unwrap_or_default().to_string(),
        module_classification: field("module_classification")
            .as_str()
            .and_then(ModuleClassification::parse),
        operation_identity: field("operation_identity").as_u64().unwrap_or(0),
    }
}

/// Run one control op. The outer `Result` is a harness error (malformed
/// control document); the inner `Result` is the op's own outcome, whose
/// `Err` code the control expects.
fn run_control(doc: &NegativeControlDoc) -> Result<Result<(), String>, String> {
    match doc.op.as_str() {
        "bind_subject" => {
            let candidate = candidate_from_doc(doc.input.get("candidate").unwrap_or(&Value::Null));
            Ok(candidate.bind().map(|_| ()).map_err(|error| error.code().to_string()))
        }
        "admit" => {
            let candidate = candidate_from_doc(doc.input.get("candidate").unwrap_or(&Value::Null));
            let observation: ObservationDoc = serde_json::from_value(
                doc.input.get("observation").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| {
                format!("control {} observation must deserialize: {error}", doc.control)
            })?;
            let subject = match candidate.bind() {
                Ok(subject) => subject,
                Err(error) => return Ok(Err(error.code().to_string())),
            };
            Ok(plan_reload(&subject, &observation.to_observation()?)
                .map(|_| ())
                .map_err(|refusal| refusal.as_str().to_string()))
        }
        "verify_mechanism_claims" => {
            let mechanism = ReloadMechanism::parse(
                doc.input.get("mechanism").and_then(Value::as_str).unwrap_or_default(),
            )
            .ok_or_else(|| format!("control {} names an unknown mechanism", doc.control))?;
            let claims: Vec<MechanismClaim> =
                match doc.input.get("claims").and_then(Value::as_array) {
                    Some(entries) => entries
                        .iter()
                        .map(|entry| {
                            MechanismClaim::ALL
                                .into_iter()
                                .find(|claim| claim.as_str() == entry.as_str().unwrap_or_default())
                                .ok_or_else(|| {
                                    format!("control {} names an unknown claim", doc.control)
                                })
                        })
                        .collect::<Result<Vec<MechanismClaim>, String>>()?,
                    None => Vec::new(),
                };
            Ok(verify_mechanism_claims(&MechanismClaims { mechanism, claims })
                .map_err(|error| error.code().to_string()))
        }
        "apply_generation_effect" => {
            let outcome_doc: OutcomeDoc =
                serde_json::from_value(doc.input.get("outcome").cloned().unwrap_or(Value::Null))
                    .map_err(|error| {
                        format!("control {} outcome must deserialize: {error}", doc.control)
                    })?;
            let outcome = outcome_from_doc(&outcome_doc)?;
            let claimed = doc
                .input
                .get("claimed_effect")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut clock = RuntimeModuleGenerationClock::new();
            let advance = clock.apply(&outcome, FIXTURE_OPERATION);
            Ok(match (advance.advanced(), claimed.as_str()) {
                (true, "advance") | (false, "none") => Ok(()),
                (true, _) => Err(POSSIBLY_APPLIED_WITHOUT_ADVANCE.to_string()),
                (false, _) => Err(ADVANCE_WITHOUT_POSSIBLY_APPLIED.to_string()),
            })
        }
        "verify_invalidation_plan" => {
            let outcome_doc: OutcomeDoc =
                serde_json::from_value(doc.input.get("outcome").cloned().unwrap_or(Value::Null))
                    .map_err(|error| {
                        format!("control {} outcome must deserialize: {error}", doc.control)
                    })?;
            let outcome = outcome_from_doc(&outcome_doc)?;
            let dispositions: Vec<(DapObjectKind, InvalidationDisposition)> =
                match doc.input.get("dispositions").and_then(Value::as_object) {
                    Some(entries) => entries
                        .iter()
                        .map(|(kind_code, disposition_code)| {
                            let kind = DapObjectKind::parse(kind_code)
                                .ok_or_else(|| format!("unknown object kind {kind_code}"))?;
                            let disposition = [
                                InvalidationDisposition::AlwaysStale,
                                InvalidationDisposition::StaleWhenGenerationAdvanced,
                                InvalidationDisposition::ProjectionReprojected,
                                InvalidationDisposition::PreservedForLaterReconciliation,
                            ]
                            .into_iter()
                            .find(|disposition| {
                                disposition.as_str()
                                    == disposition_code.as_str().unwrap_or_default()
                            })
                            .ok_or_else(|| format!("unknown disposition {disposition_code}"))?;
                            Ok((kind, disposition))
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                    None => Vec::new(),
                };
            Ok(verify_invalidation_plan(
                &ReloadInvalidationPlan::from_dispositions(&dispositions),
                &outcome,
            )
            .map_err(|error| error.code().to_string()))
        }
        "validate_request_surface" => {
            let family = string_field(&doc.input, "family")?;
            let family_version =
                doc.input.get("family_version").and_then(Value::as_u64).unwrap_or(0) as u32;
            let raw_payload_text = || -> Result<String, String> {
                doc.input
                    .get("payload_value")
                    .and_then(Value::as_str)
                    .map(|text| text.to_string())
                    .ok_or_else(|| {
                        "payload_value must be a string when a raw payload kind is named"
                            .to_string()
                    })
            };
            let payload =
                match doc.input.get("payload_kind").and_then(Value::as_str).unwrap_or_default() {
                    "raw_path" => ReloadRequestPayload::RawPath(raw_payload_text()?),
                    "debugger_command" => {
                        ReloadRequestPayload::DebuggerCommand(raw_payload_text()?)
                    }
                    "perl_expression" => ReloadRequestPayload::PerlExpression(raw_payload_text()?),
                    _ => ReloadRequestPayload::TypedModuleSubject,
                };
            let capability_name = doc
                .input
                .get("capability_name")
                .and_then(Value::as_str)
                .map(|text| text.to_string());
            let capability = match doc
                .input
                .get("capability_kind")
                .and_then(Value::as_str)
                .unwrap_or("unadvertised")
            {
                "namespaced_custom" => ReloadCapabilityProjection::NamespacedCustom(
                    capability_name.ok_or("capability_name must be a string")?,
                ),
                "invented_standard" => ReloadCapabilityProjection::InventedStandard(
                    capability_name.ok_or("capability_name must be a string")?,
                ),
                _ => ReloadCapabilityProjection::Unadvertised,
            };
            let descriptor = ReloadRequestSurfaceDescriptor {
                family,
                family_version,
                correlation_identity: doc.input.get("correlation_identity").and_then(Value::as_u64),
                payload,
                capability,
            };
            Ok(validate_request_surface(&descriptor).map_err(|error| error.code().to_string()))
        }
        other => Err(format!("unknown control op {other}")),
    }
}

#[test]
fn negative_controls_fail_with_their_exact_reason_codes() -> TestResult {
    let documents: Vec<(String, NegativeControlDoc)> = read_json_dir("fixtures/negative_controls")?
        .into_iter()
        .filter(|(name, _)| name != "expected_errors.json")
        .map(|(name, value)| {
            let doc: NegativeControlDoc = serde_json::from_value(value)
                .map_err(|error| format!("negative_controls/{name} must deserialize: {error}"))?;
            Ok((name, doc))
        })
        .collect::<Result<_, String>>()?;
    let mut controls_covered = BTreeSet::new();
    for (file, doc) in &documents {
        let op_result = run_control(doc)
            .map_err(|error| format!("negative_controls/{file}: harness error: {error}"))?;
        let produced = match op_result {
            Ok(()) => {
                return Err(format!("negative_controls/{file} must fail").into());
            }
            Err(code) => code,
        };
        assert_eq!(
            produced, doc.expected_error,
            "negative_controls/{file} must fail with {}",
            doc.expected_error
        );
        controls_covered.insert(doc.control);
    }
    assert_eq!(
        controls_covered,
        (1..=10).collect::<BTreeSet<u8>>(),
        "all ten negative controls must be encoded"
    );

    // expected_errors.json must name exactly the control documents.
    let expected_errors_value = read_json_dir("fixtures/negative_controls")?
        .into_iter()
        .find(|(name, _)| name == "expected_errors.json")
        .map(|(_, value)| value)
        .ok_or("expected_errors.json must exist")?;
    let expected_errors: std::collections::BTreeMap<String, String> =
        serde_json::from_value(expected_errors_value)
            .map_err(|error| format!("expected_errors.json must be a string map: {error}"))?;
    let control_files: BTreeSet<String> = documents.iter().map(|(name, _)| name.clone()).collect();
    let recorded: BTreeSet<String> = expected_errors.keys().cloned().collect();
    assert_eq!(control_files, recorded, "expected_errors.json must cover exactly the controls");
    for (file, doc) in &documents {
        assert_eq!(
            expected_errors.get(file),
            Some(&doc.expected_error),
            "expected_errors.json entry for {file} must match the fixture"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Schema sync
// ---------------------------------------------------------------------------

fn schema_enum(name: &str) -> Result<Vec<String>, String> {
    let schema_path = spec_dir()?.join("schemas/loaded_module_reload.v1.schema.json");
    let bytes =
        fs::read(&schema_path).map_err(|error| format!("schema must be readable: {error}"))?;
    let schema: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("schema must parse: {error}"))?;
    schema["$defs"][name]["enum"]
        .as_array()
        .ok_or_else(|| format!("schema $defs.{name}.enum must exist"))?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(|text| text.to_string())
                .ok_or_else(|| "enum entries are strings".to_string())
        })
        .collect()
}

#[test]
fn schema_enums_match_the_rust_vocabularies_exactly() -> TestResult {
    assert_eq!(
        schema_enum("disposition")?,
        LoadedModuleReloadEligibility::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("moduleClassification")?,
        ModuleClassification::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("phase")?,
        ReloadTransactionPhase::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("preMutationCause")?,
        PreMutationFailureCause::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("indeterminateCause")?,
        IndeterminateCause::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("dapObjectKind")?,
        DapObjectKind::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("invalidationDisposition")?,
        [
            InvalidationDisposition::AlwaysStale,
            InvalidationDisposition::StaleWhenGenerationAdvanced,
            InvalidationDisposition::ProjectionReprojected,
            InvalidationDisposition::PreservedForLaterReconciliation,
        ]
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("mechanism")?,
        ReloadMechanism::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("mechanismClaim")?,
        MechanismClaim::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("surfaceViolation")?,
        SurfaceViolation::ALL.iter().map(|kind| kind.code()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("bindingError")?,
        SubjectBindingError::ALL.iter().map(|error| error.code()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("invalidationPlanError")?,
        InvalidationPlanError::ALL.iter().map(|kind| kind.code()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("mechanismError")?,
        MechanismRecordError::ALL.iter().map(|kind| kind.code()).collect::<Vec<_>>()
    );
    assert_eq!(
        schema_enum("generationEffect")?,
        [GenerationEffect::Advance, GenerationEffect::None]
            .iter()
            .map(|effect| effect.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(schema_enum("controlOp")?, {
        let mut ops = vec![
            "admit".to_string(),
            "apply_generation_effect".to_string(),
            "bind_subject".to_string(),
            "validate_request_surface".to_string(),
            "verify_invalidation_plan".to_string(),
            "verify_mechanism_claims".to_string(),
        ];
        ops.sort();
        ops
    });
    assert_eq!(schema_enum("controlErrorCode")?, {
        let mut codes = vec![
            ADVANCE_WITHOUT_POSSIBLY_APPLIED.to_string(),
            POSSIBLY_APPLIED_WITHOUT_ADVANCE.to_string(),
        ];
        codes.extend(SubjectBindingError::ALL.iter().map(|error| error.code().to_string()));
        codes.extend(
            LoadedModuleReloadEligibility::ALL.iter().map(|kind| kind.as_str().to_string()),
        );
        codes.extend(InvalidationPlanError::ALL.iter().map(|kind| kind.code().to_string()));
        codes.extend(MechanismRecordError::ALL.iter().map(|kind| kind.code().to_string()));
        codes.extend(SurfaceViolation::ALL.iter().map(|kind| kind.code().to_string()));
        codes.sort();
        codes.dedup();
        codes
    });
    Ok(())
}
