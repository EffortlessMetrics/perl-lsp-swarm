use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use perl_parser_comparison::{
    AttachmentPrivacy, BoundedAttachment, BoundedText, ConformanceOutcome, DiagnosticSummary,
    DivergencePath, EvidenceKind, EvidencePayloadError, EvidenceRef, InstrumentState,
    MismatchClass, MismatchDetail, ObligationRef, ObservationDisposition, ObservationPlane,
    ObserverId, ObserverManifestRef, ReviewedExpectationId, ScoredComparison, SemanticDigest,
    SemanticFingerprint, SourceCaseRef, StableId, SubjectConformanceEvidence, SubjectDisposition,
    SubjectExecution, SubjectExecutionEvidence, SubjectManifestRef, SubjectObservationEvidence,
    SubjectRole, parser_comparison_evidence_schema_json,
};
use serde_json::Value;

#[cfg(feature = "historical")]
use perl_parser_comparison::{HarnessOutcome, execute_v3};

fn stable(value: &str) -> Result<StableId, Box<dyn Error>> {
    Ok(StableId::new(value)?)
}

fn digest(value: &str) -> SemanticDigest {
    SemanticDigest::from_bytes(value.as_bytes())
}

fn reference(
    kind: EvidenceKind,
    schema_version: &str,
    semantic_id: &str,
    semantic_bytes: &str,
) -> Result<EvidenceRef, Box<dyn Error>> {
    Ok(EvidenceRef::new(
        kind,
        stable(schema_version)?,
        stable(semantic_id)?,
        digest(semantic_bytes),
    ))
}

fn source_case() -> Result<SourceCaseRef, Box<dyn Error>> {
    Ok(SourceCaseRef::new(
        stable("case.assignment.basic.v1")?,
        reference(
            EvidenceKind::SourceCase,
            "parser_comparison_source_case.v1",
            "case.assignment.basic.v1",
            "canonical source case",
        )?,
        digest("my $x = 42;"),
    )?)
}

fn subject_manifest(role: SubjectRole) -> Result<SubjectManifestRef, Box<dyn Error>> {
    Ok(SubjectManifestRef::new(
        reference(
            EvidenceKind::SubjectManifest,
            "parser_comparison_subject_manifest.v1",
            "subject.native_recursive_descent.v1",
            "native subject manifest",
        )?,
        role,
    )?)
}

fn observer_manifest(
    observer_id: &str,
    plane: ObservationPlane,
) -> Result<ObserverManifestRef, Box<dyn Error>> {
    Ok(ObserverManifestRef::new(
        reference(
            EvidenceKind::ObserverManifest,
            "parser_comparison_observer_manifest.v1",
            observer_id,
            observer_id,
        )?,
        ObserverId::new(observer_id)?,
        plane,
    )?)
}

fn obligation(observer: ObserverManifestRef) -> Result<ObligationRef, Box<dyn Error>> {
    Ok(ObligationRef::new(
        reference(
            EvidenceKind::CaseObligation,
            "parser_comparison_case_obligation.v1",
            "obligation.assignment.shape.v1",
            "reviewed assignment obligation",
        )?,
        ReviewedExpectationId::new("obligation.assignment.shape.v1")?,
        observer,
    )?)
}

fn generic_execution(
    instrument_state: InstrumentState,
) -> Result<SubjectExecution, Box<dyn Error>> {
    let disposition = if instrument_state == InstrumentState::Complete {
        ObservationDisposition::Observed
    } else {
        ObservationDisposition::ObservedWithLimitations
    };
    Ok(SubjectExecution::completed(
        SubjectRole::NativeRecursiveDescent,
        SubjectDisposition::AcceptedClean,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, disposition)]),
        None,
        instrument_state,
    )?)
}

fn execution_evidence(
    execution: &SubjectExecution,
    attachments: Vec<BoundedAttachment>,
) -> Result<SubjectExecutionEvidence, Box<dyn Error>> {
    Ok(SubjectExecutionEvidence::new(
        source_case()?,
        subject_manifest(SubjectRole::NativeRecursiveDescent)?,
        execution,
        attachments,
    )?)
}

fn attachment(
    kind: &str,
    text: &str,
    privacy: AttachmentPrivacy,
) -> Result<BoundedAttachment, Box<dyn Error>> {
    Ok(BoundedAttachment::new(stable(kind)?, BoundedText::new(text, 32)?, privacy))
}

fn exact_observation(
    execution: &SubjectExecutionEvidence,
    observer: ObserverManifestRef,
    fingerprint: &str,
) -> Result<SubjectObservationEvidence, Box<dyn Error>> {
    Ok(SubjectObservationEvidence::new(
        execution,
        observer,
        stable("exact.fresh.v1")?,
        ObservationDisposition::Observed,
        Some(SemanticFingerprint::new(fingerprint)?),
        None,
        Vec::new(),
    )?)
}

#[test]
fn exact_reference_wrappers_reject_the_wrong_authority_kind() -> Result<(), Box<dyn Error>> {
    let wrong = reference(
        EvidenceKind::ObserverManifest,
        "parser_comparison_observer_manifest.v1",
        "observer.structure.v1",
        "observer manifest",
    )?;
    let result = SourceCaseRef::new(stable("case.v1")?, wrong, digest("source"));

    assert_eq!(
        result,
        Err(EvidencePayloadError::WrongEvidenceKind {
            expected: EvidenceKind::SourceCase,
            actual: EvidenceKind::ObserverManifest,
        })
    );
    Ok(())
}

#[test]
fn execution_payload_rejects_subject_substitution() -> Result<(), Box<dyn Error>> {
    let execution = generic_execution(InstrumentState::Complete)?;
    let result = SubjectExecutionEvidence::new(
        source_case()?,
        subject_manifest(SubjectRole::HistoricalTreeSitterC)?,
        &execution,
        Vec::new(),
    );

    assert_eq!(result, Err(EvidencePayloadError::SubjectRoleMismatch));
    Ok(())
}

#[test]
fn bounded_attachments_are_sorted_and_nonsemantic() -> Result<(), Box<dyn Error>> {
    let execution = generic_execution(InstrumentState::Complete)?;
    let first = execution_evidence(
        &execution,
        vec![
            attachment("tree", "second", AttachmentPrivacy::Private)?,
            attachment("diagnostic", "first", AttachmentPrivacy::Public)?,
        ],
    )?;
    let second = execution_evidence(
        &execution,
        vec![
            attachment("diagnostic", "changed", AttachmentPrivacy::Public)?,
            attachment("tree", "different", AttachmentPrivacy::Private)?,
        ],
    )?;

    assert_eq!(first.semantic_digest(), second.semantic_digest());
    assert_eq!(first.canonical_semantic_json()?, second.canonical_semantic_json()?);
    assert_ne!(first.canonical_payload_json()?, second.canonical_payload_json()?);
    assert_eq!(first.attachments()[0].kind().as_str(), "diagnostic");
    first.validate()?;
    second.validate()?;
    Ok(())
}

#[test]
fn observed_payload_requires_exact_complete_evidence() -> Result<(), Box<dyn Error>> {
    let complete = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&complete, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;

    let missing_fingerprint = SubjectObservationEvidence::new(
        &execution,
        observer.clone(),
        stable("exact.fresh.v1")?,
        ObservationDisposition::Observed,
        None,
        None,
        Vec::new(),
    );
    assert_eq!(missing_fingerprint, Err(EvidencePayloadError::InvalidObservationDisposition));

    let partial = generic_execution(InstrumentState::Partial)?;
    let partial_execution = execution_evidence(&partial, Vec::new())?;
    let exact_from_partial = SubjectObservationEvidence::new(
        &partial_execution,
        observer,
        stable("exact.fresh.v1")?,
        ObservationDisposition::Observed,
        Some(SemanticFingerprint::new("assignment(variable,integer)")?),
        None,
        Vec::new(),
    );
    assert_eq!(exact_from_partial, Err(EvidencePayloadError::InvalidObservationDisposition));
    Ok(())
}

#[test]
fn unavailable_observation_requires_a_typed_reason() -> Result<(), Box<dyn Error>> {
    let execution = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&execution, Vec::new())?;
    let observer = observer_manifest("observer.geometry.v1", ObservationPlane::SourceGeometry)?;

    let missing_reason = SubjectObservationEvidence::new(
        &execution,
        observer.clone(),
        stable("exact.fresh.v1")?,
        ObservationDisposition::NotObservable,
        None,
        None,
        Vec::new(),
    );
    assert_eq!(missing_reason, Err(EvidencePayloadError::InvalidObservationDisposition));

    let valid = SubjectObservationEvidence::new(
        &execution,
        observer,
        stable("exact.fresh.v1")?,
        ObservationDisposition::NotObservable,
        None,
        Some(stable("subject.api.unavailable")?),
        Vec::new(),
    )?;
    valid.validate()?;
    Ok(())
}

#[test]
fn decisive_conformance_binds_observer_obligation_and_actual_fingerprint()
-> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&generic, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;
    let observation =
        exact_observation(&execution, observer.clone(), "assignment(integer,variable)")?;
    let obligation = obligation(observer)?;
    let comparison = ScoredComparison::mismatch(
        &generic,
        ObserverId::new("observer.structure.v1")?,
        ReviewedExpectationId::new("obligation.assignment.shape.v1")?,
        ObservationPlane::Structure,
        SemanticFingerprint::new("assignment(variable,integer)")?,
        SemanticFingerprint::new("assignment(integer,variable)")?,
        MismatchDetail::new(
            MismatchClass::WrongOrderOrOwnership,
            DivergencePath::new("children[0]")?,
        ),
    )?;

    let evidence =
        SubjectConformanceEvidence::scored(&observation, obligation, &comparison, Vec::new())?;
    assert_eq!(evidence.outcome(), ConformanceOutcome::Mismatch);
    assert_eq!(
        evidence.mismatch().map(MismatchDetail::first_divergence).map(DivergencePath::as_str),
        Some("children[0]")
    );
    evidence.validate()?;
    Ok(())
}

#[test]
fn conformance_rejects_an_obligation_from_another_observer() -> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&generic, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;
    let observation = exact_observation(&execution, observer, "assignment(variable,integer)")?;
    let wrong_observer = observer_manifest("observer.other.v1", ObservationPlane::Structure)?;
    let wrong_obligation = obligation(wrong_observer)?;
    let result = SubjectConformanceEvidence::non_decisive(
        &observation,
        wrong_obligation,
        ConformanceOutcome::NotProven,
        stable("observer.mismatch")?,
        Vec::new(),
    );

    assert_eq!(result, Err(EvidencePayloadError::ObligationObserverMismatch));
    Ok(())
}

#[test]
fn unscored_observation_cannot_fabricate_conformance() -> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&generic, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;
    let observation =
        exact_observation(&execution, observer.clone(), "assignment(variable,integer)")?;
    let result = SubjectConformanceEvidence::non_decisive(
        &observation,
        obligation(observer)?,
        ConformanceOutcome::Unscored,
        stable("no.reviewed.obligation")?,
        Vec::new(),
    );

    assert_eq!(
        result,
        Err(EvidencePayloadError::InvalidNonDecisiveConformance(ConformanceOutcome::Unscored))
    );
    Ok(())
}

#[test]
fn payload_references_are_content_bound_and_deterministic() -> Result<(), Box<dyn Error>> {
    let execution = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&execution, Vec::new())?;
    let reference = execution.evidence_ref()?;

    assert_eq!(reference.kind(), EvidenceKind::SubjectExecution);
    assert_eq!(reference.schema_version().as_str(), "parser_comparison_subject_execution.v1");
    assert!(reference.semantic_id().as_str().starts_with("subject_execution."));
    assert_eq!(reference.semantic_digest(), execution.semantic_digest());
    assert_eq!(execution.canonical_semantic_json()?, execution.canonical_semantic_json()?);
    Ok(())
}

#[test]
fn machine_schema_matches_all_serialized_payload_field_sets() -> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&generic, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;
    let observation =
        exact_observation(&execution, observer.clone(), "assignment(variable,integer)")?;
    let comparison = ScoredComparison::matches_expected(
        &generic,
        ObserverId::new("observer.structure.v1")?,
        ReviewedExpectationId::new("obligation.assignment.shape.v1")?,
        ObservationPlane::Structure,
        SemanticFingerprint::new("assignment(variable,integer)")?,
        SemanticFingerprint::new("assignment(variable,integer)")?,
    )?;
    let conformance = SubjectConformanceEvidence::scored(
        &observation,
        obligation(observer)?,
        &comparison,
        Vec::new(),
    )?;

    let schema: Value = serde_json::from_str(&parser_comparison_evidence_schema_json()?)?;
    assert_field_contract(&schema, "subject_execution", &execution.canonical_payload_json()?)?;
    assert_field_contract(&schema, "subject_observation", &observation.canonical_payload_json()?)?;
    assert_field_contract(&schema, "subject_conformance", &conformance.canonical_payload_json()?)?;

    let schema_text = parser_comparison_evidence_schema_json()?;
    for forbidden in [
        "candidate",
        "profile_id",
        "complete_run",
        "pairwise",
        "retention",
        "generated_at",
        "duration_millis",
    ] {
        assert!(!schema_text.contains(forbidden), "domain cell schema must not own {forbidden}");
    }

    validate_schema(&execution.canonical_payload_json()?, &schema)?;
    validate_schema(&observation.canonical_payload_json()?, &schema)?;
    validate_schema(&conformance.canonical_payload_json()?, &schema)?;

    let mut invalid: Value = serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid["semantic_digest"] = Value::String("sha256:not-a-digest".to_owned());
    assert!(validate_value(&invalid, &schema, &schema).is_err());
    Ok(())
}

#[test]
fn schema_rejects_realistic_constraint_violations() -> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let with_attachment = execution_evidence(
        &generic,
        vec![attachment("trace", "terminal", AttachmentPrivacy::Public)?],
    )?;
    let schema: Value = serde_json::from_str(&parser_comparison_evidence_schema_json()?)?;
    let payload: Value = serde_json::from_str(&with_attachment.canonical_payload_json()?)?;

    let mut missing_required = payload.clone();
    missing_required
        .as_object_mut()
        .ok_or("execution payload must be an object")?
        .remove("source_case");
    assert!(validate_value(&missing_required, &schema, &schema).is_err(), "required");

    let mut extra_property = payload.clone();
    extra_property["unexpected"] = Value::Bool(true);
    assert!(validate_value(&extra_property, &schema, &schema).is_err(), "additionalProperties");

    let mut wrong_const = payload.clone();
    wrong_const["schema_version"] = Value::String("parser_comparison_observation.v1".to_owned());
    assert!(validate_value(&wrong_const, &schema, &schema).is_err(), "const");

    let mut wrong_enum = payload.clone();
    wrong_enum["attachments"][0]["privacy"] = Value::String("secret".to_owned());
    assert!(validate_value(&wrong_enum, &schema, &schema).is_err(), "enum");

    let mut wrong_type = payload.clone();
    wrong_type["semantic_digest"] = Value::Array(Vec::new());
    assert!(validate_value(&wrong_type, &schema, &schema).is_err(), "type");

    let mut broken_ref = payload.clone();
    broken_ref["source_case"]["authority"]["semantic_digest"] =
        Value::String("not-a-digest".to_owned());
    assert!(validate_value(&broken_ref, &schema, &schema).is_err(), "$ref");

    // Validate the unchanged generated root with a near-miss. The terminal
    // branches must reject an otherwise valid payload whose discriminator is
    // not one of their distinct schema-version consts.
    let mut invalid_root = payload.clone();
    invalid_root["schema_version"] =
        Value::String("parser_comparison_subject_execution.v2".to_owned());
    assert!(validate_value(&invalid_root, &schema, &schema).is_err(), "oneOf");
    Ok(())
}

#[test]
fn schema_rejects_arbitrary_modeled_values() -> Result<(), Box<dyn Error>> {
    let generic = generic_execution(InstrumentState::Complete)?;
    let execution = execution_evidence(&generic, Vec::new())?;
    let observer = observer_manifest("observer.structure.v1", ObservationPlane::Structure)?;
    let observation =
        exact_observation(&execution, observer.clone(), "assignment(variable,integer)")?;
    let comparison = ScoredComparison::matches_expected(
        &generic,
        ObserverId::new("observer.structure.v1")?,
        ReviewedExpectationId::new("obligation.assignment.shape.v1")?,
        ObservationPlane::Structure,
        SemanticFingerprint::new("assignment(variable,integer)")?,
        SemanticFingerprint::new("assignment(variable,integer)")?,
    )?;
    let conformance = SubjectConformanceEvidence::scored(
        &observation,
        obligation(observer)?,
        &comparison,
        Vec::new(),
    )?;
    let schema: Value = serde_json::from_str(&parser_comparison_evidence_schema_json()?)?;

    let mut invalid_kind: Value = serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_kind["source_case"]["authority"]["kind"] = Value::String("arbitrary_kind".to_owned());
    assert!(validate_value(&invalid_kind, &schema, &schema).is_err(), "kind");

    let mut invalid_role: Value = serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_role["subject_manifest"]["role"] = Value::String("arbitrary_role".to_owned());
    assert!(validate_value(&invalid_role, &schema, &schema).is_err(), "role");

    let mut invalid_harness: Value = serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_harness["harness"] = Value::String("arbitrary_harness".to_owned());
    assert!(validate_value(&invalid_harness, &schema, &schema).is_err(), "harness");

    let mut invalid_subject_disposition: Value =
        serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_subject_disposition["subject_disposition"] =
        Value::String("arbitrary_subject_disposition".to_owned());
    assert!(
        validate_value(&invalid_subject_disposition, &schema, &schema).is_err(),
        "subject_disposition"
    );

    let mut invalid_instrument_state: Value =
        serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_instrument_state["instrument_state"] =
        Value::String("arbitrary_instrument_state".to_owned());
    assert!(
        validate_value(&invalid_instrument_state, &schema, &schema).is_err(),
        "instrument_state"
    );

    let mut invalid_plane: Value = serde_json::from_str(&observation.canonical_payload_json()?)?;
    invalid_plane["observer_manifest"]["plane"] = Value::String("arbitrary_plane".to_owned());
    assert!(validate_value(&invalid_plane, &schema, &schema).is_err(), "plane");

    let mut invalid_disposition: Value =
        serde_json::from_str(&observation.canonical_payload_json()?)?;
    invalid_disposition["disposition"] = Value::String("arbitrary_disposition".to_owned());
    assert!(validate_value(&invalid_disposition, &schema, &schema).is_err(), "disposition");

    let mut invalid_outcome: Value = serde_json::from_str(&conformance.canonical_payload_json()?)?;
    invalid_outcome["outcome"] = Value::String("arbitrary_outcome".to_owned());
    assert!(validate_value(&invalid_outcome, &schema, &schema).is_err(), "outcome");

    let mut invalid_schema_version: Value =
        serde_json::from_str(&execution.canonical_payload_json()?)?;
    invalid_schema_version["schema_version"] = Value::String("arbitrary_schema_version".to_owned());
    assert!(validate_value(&invalid_schema_version, &schema, &schema).is_err(), "schema_version");
    Ok(())
}

#[cfg(feature = "historical")]
#[test]
fn harness_terminal_execution_reaches_durable_schema_evidence() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $answer = 42;")?;
    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert!(execution.subject_disposition().is_some());

    let evidence = execution_evidence(&execution, Vec::new())?;
    evidence.validate()?;
    let schema: Value = serde_json::from_str(&parser_comparison_evidence_schema_json()?)?;
    validate_schema(&evidence.canonical_payload_json()?, &schema)?;
    Ok(())
}

/// Validate the generated schema with the repository-supported JSON-Schema
/// subset. The generated contract intentionally uses only these Draft 2020-12
/// keywords, keeping this proof independent of outer run authority.
fn validate_schema(payload_json: &str, schema: &Value) -> Result<(), Box<dyn Error>> {
    let payload: Value = serde_json::from_str(payload_json)?;
    validate_value(&payload, schema, schema).map_err(io::Error::other)?;
    Ok(())
}

fn validate_value(value: &Value, schema: &Value, root: &Value) -> Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference.strip_prefix('#').ok_or("external schema reference")?;
        let target = root.pointer(pointer).ok_or("unresolved schema reference")?;
        validate_value(value, target, root)?;
    }
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        let matches =
            branches.iter().filter(|branch| validate_value(value, branch, root).is_ok()).count();
        if matches != 1 {
            return Err(format!("oneOf matched {matches} branches"));
        }
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err("const mismatch".to_owned());
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err("enum mismatch".to_owned());
        }
    }
    if let Some(kind) = schema.get("type") {
        let valid = match kind {
            Value::String(kind) => matches_type(value, kind),
            Value::Array(kinds) => {
                kinds.iter().filter_map(Value::as_str).any(|kind| matches_type(value, kind))
            }
            _ => false,
        };
        if !valid {
            return Err("type mismatch".to_owned());
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            if required.iter().filter_map(Value::as_str).any(|key| !object.contains_key(key)) {
                return Err("required property missing".to_owned());
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            if schema.get("additionalProperties") == Some(&Value::Bool(false))
                && object.keys().any(|key| !properties.contains_key(key))
            {
                return Err("additional property".to_owned());
            }
            for (key, property_schema) in properties {
                if let Some(property) = object.get(key) {
                    validate_value(property, property_schema, root)?;
                }
            }
        }
    }
    if let Some(items) = schema.get("items") {
        if let Some(array) = value.as_array() {
            for item in array {
                validate_value(item, items, root)?;
            }
        }
    }
    if let Some(string) = value.as_str() {
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            if !matches_pattern(string, pattern) {
                return Err("pattern mismatch".to_owned());
            }
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|number| number < minimum) {
            return Err("minimum mismatch".to_owned());
        }
    }
    Ok(())
}

fn matches_type(value: &Value, kind: &str) -> bool {
    match kind {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        _ => false,
    }
}

fn matches_pattern(value: &str, pattern: &str) -> bool {
    match pattern {
        "^sha256:[0-9a-f]{64}$" => value.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        }),
        "^[a-z0-9][a-z0-9._-]{0,127}$" => {
            (1..=128).contains(&value.len())
                && value.chars().enumerate().all(|(index, character)| {
                    (index == 0 && character.is_ascii_lowercase())
                        || (index > 0
                            && (character.is_ascii_lowercase()
                                || character.is_ascii_digit()
                                || matches!(character, '.' | '_' | '-')))
                })
        }
        "^registered:[a-z0-9][a-z0-9._-]{0,127}$" => {
            value.strip_prefix("registered:").is_some_and(|id| {
                (1..=128).contains(&id.len())
                    && id.chars().enumerate().all(|(index, character)| {
                        (index == 0 && character.is_ascii_lowercase())
                            || (index > 0
                                && (character.is_ascii_lowercase()
                                    || character.is_ascii_digit()
                                    || matches!(character, '.' | '_' | '-')))
                    })
            })
        }
        _ => false,
    }
}

fn assert_field_contract(
    schema: &Value,
    definition: &str,
    payload_json: &str,
) -> Result<(), Box<dyn Error>> {
    let required = schema
        .pointer(&format!("/$defs/{definition}/required"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("schema required field list missing"))?;
    let required = required
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("schema required field is not a string"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let payload: Value = serde_json::from_str(payload_json)?;
    let object = payload.as_object().ok_or_else(|| io::Error::other("payload is not an object"))?;
    let keys = object.keys().cloned().collect::<BTreeSet<_>>();

    assert_eq!(required, keys, "schema and payload fields drifted for {definition}");
    Ok(())
}
