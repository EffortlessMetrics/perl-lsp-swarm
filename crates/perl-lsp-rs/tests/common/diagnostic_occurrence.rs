//! Fail-closed diagnostic observation and exact-occurrence matching for the
//! editor-intelligence scorecard (#13452).
//!
//! Gold `byte_offset` remains a compatibility location field. This adapter
//! interprets it as point-containment until #7006/#7009 land a registered
//! typed extension. Conversion uses `perl_position_tracking`'s strict
//! byte↔wire mapping and the session position-encoding contract.

use perl_corpus::gold::GoldAssertion;
use perl_position_tracking::{
    PositionEncoding, PositionMappingDisposition, WirePosition, WireRange, byte_to_wire_position,
    wire_range_to_bytes,
};
use serde_json::Value;

/// How leftover unmatched actual diagnostics are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnexpectedExtraPolicy {
    /// Default: leftover actuals are a non-pass.
    Reject,
    /// Compatibility: leftover actuals are ignored.
    Permit,
}

/// Byte-based location contract for one assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationConstraint {
    /// Code/message membership only.
    Unconstrained,
    /// Compatibility `byte_offset`: the point must sit inside the range.
    PointContained {
        /// Source UTF-8 byte offset.
        byte_offset: usize,
    },
    /// Observed range start must equal this byte offset.
    ExactStart {
        /// Source UTF-8 byte offset.
        byte_offset: usize,
    },
    /// Observed range must equal this source span.
    ExactRange {
        /// Inclusive start byte.
        start_byte: usize,
        /// Exclusive end byte.
        end_byte: usize,
    },
}

/// One expected diagnostic assertion with a stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticExpectation {
    /// Durable identity in failure output (`project/member#sidecar/assertion`).
    pub identity: String,
    /// Present / absent / count / empty.
    pub kind: DiagnosticExpectationKind,
    /// Optional code selector.
    pub code: Option<String>,
    /// Optional message substring selector.
    pub message_contains: Option<String>,
    /// Location contract. A supplied location is load-bearing.
    pub location: LocationConstraint,
}

/// Closed set of diagnostic expectation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticExpectationKind {
    /// Consume exactly one matching actual diagnostic.
    Present,
    /// The selected code must be absent.
    Absent,
    /// Consume exactly `count` actuals with the selected code.
    Count {
        /// Required cardinality.
        count: usize,
    },
    /// The complete observation must contain zero diagnostics.
    Empty,
}

/// Document identity and source used to interpret an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticSubject<'a> {
    /// Expected document URI.
    pub uri: &'a str,
    /// Expected document version, when the caller knows it.
    pub version: Option<i64>,
    /// Exact fixture bytes.
    pub source: &'a str,
    /// Negotiated LSP position encoding. Defaults to UTF-16.
    pub encoding: PositionEncoding,
}

/// Terminal classification of a diagnostic payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticObservation {
    /// Well-formed complete diagnostic set for one document.
    Valid(ValidDiagnosticObservation),
    /// Payload was present but not a usable complete observation.
    InvalidObservation {
        /// Compact reason, including stale/wrong-method/malformed cases.
        reason: String,
    },
    /// Missing envelope, JSON-RPC error, or decoder failure.
    InstrumentFailure {
        /// Compact reason.
        reason: String,
    },
}

/// A valid complete diagnostic observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidDiagnosticObservation {
    /// Observed document URI.
    pub uri: String,
    /// Observed document version, when present.
    pub version: Option<i64>,
    /// Observed diagnostics in payload order.
    pub diagnostics: Vec<ObservedDiagnostic>,
}

/// One observed diagnostic with preserved range, code, and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedDiagnostic {
    /// Normalized diagnostic code, when present.
    pub code: Option<String>,
    /// Diagnostic message.
    pub message: String,
    /// LSP range.
    pub range: WireRange,
    /// Canonical start byte, when conversion was exact enough to use.
    pub start_byte: usize,
    /// Canonical exclusive end byte.
    pub end_byte: usize,
}

/// One-to-one match verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticMatchVerdict {
    /// Every assertion bound uniquely and extras policy passed.
    Pass,
    /// Zero matches, wrong occurrence, invalid authored location, or cardinality.
    Mismatch {
        /// Assertion identity.
        identity: String,
        /// Compact expected/actual occurrence diff.
        evidence: String,
    },
    /// Two or more actuals satisfy one expectation.
    Ambiguous {
        /// Assertion identity.
        identity: String,
        /// Compact expected/actual occurrence diff.
        evidence: String,
    },
    /// Leftover actual diagnostics under the default extras policy.
    UnexpectedExtra {
        /// Fixture-level identity.
        identity: String,
        /// Compact leftover occurrence list.
        evidence: String,
    },
    /// Transport produced no valid complete observation.
    InvalidObservation {
        /// Compact reason.
        evidence: String,
    },
    /// Decoder/instrument could not observe diagnostics.
    InstrumentFailure {
        /// Compact reason.
        evidence: String,
    },
}

impl DiagnosticMatchVerdict {
    /// Whether the verdict is a pass.
    #[must_use]
    pub const fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

impl std::fmt::Display for DiagnosticMatchVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "pass"),
            Self::Mismatch { identity, evidence } => {
                write!(f, "mismatch [{identity}]: {evidence}")
            }
            Self::Ambiguous { identity, evidence } => {
                write!(f, "ambiguous [{identity}]: {evidence}")
            }
            Self::UnexpectedExtra { identity, evidence } => {
                write!(f, "unexpected_extra [{identity}]: {evidence}")
            }
            Self::InvalidObservation { evidence } => {
                write!(f, "invalid_observation: {evidence}")
            }
            Self::InstrumentFailure { evidence } => {
                write!(f, "instrument_failure: {evidence}")
            }
        }
    }
}

/// Observe a `textDocument/publishDiagnostics` notification or a pull
/// `textDocument/diagnostic` JSON-RPC response. Missing, wrong-method, stale,
/// and malformed payloads never become an empty valid set.
#[must_use]
pub fn observe_diagnostics_json(
    payload: &Value,
    subject: DiagnosticSubject<'_>,
) -> DiagnosticObservation {
    if let Some(error) = payload.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        let message = error.get("message").and_then(Value::as_str).unwrap_or("");
        return DiagnosticObservation::InstrumentFailure {
            reason: format!("json-rpc error code={code:?} message={message:?}"),
        };
    }

    if let Some(method) = payload.get("method").and_then(Value::as_str) {
        if method != "textDocument/publishDiagnostics" {
            return DiagnosticObservation::InvalidObservation {
                reason: format!("unexpected method {method:?}"),
            };
        }
        let Some(params) = payload.get("params") else {
            return DiagnosticObservation::InvalidObservation {
                reason: "publishDiagnostics missing params".to_string(),
            };
        };
        return observe_publish_params(params, subject);
    }

    match payload.get("result") {
        None => DiagnosticObservation::InstrumentFailure {
            reason: "missing result and missing publishDiagnostics method".to_string(),
        },
        Some(Value::Null) => DiagnosticObservation::InvalidObservation {
            reason: "diagnostic result is null".to_string(),
        },
        Some(result) => observe_pull_result(result, subject),
    }
}

/// Match expectations against an observation. Negative and zero-count
/// assertions require [`DiagnosticObservation::Valid`].
#[must_use]
pub fn match_diagnostic_expectations(
    expectations: &[DiagnosticExpectation],
    observation: &DiagnosticObservation,
    extras: UnexpectedExtraPolicy,
    subject: DiagnosticSubject<'_>,
) -> DiagnosticMatchVerdict {
    let valid = match observation {
        DiagnosticObservation::Valid(valid) => valid,
        DiagnosticObservation::InvalidObservation { reason } => {
            return DiagnosticMatchVerdict::InvalidObservation { evidence: reason.clone() };
        }
        DiagnosticObservation::InstrumentFailure { reason } => {
            return DiagnosticMatchVerdict::InstrumentFailure { evidence: reason.clone() };
        }
    };

    let mut consumed = vec![false; valid.diagnostics.len()];
    for expectation in expectations {
        match expectation.kind {
            DiagnosticExpectationKind::Empty => {
                if !valid.diagnostics.is_empty() {
                    return DiagnosticMatchVerdict::Mismatch {
                        identity: expectation.identity.clone(),
                        evidence: format!(
                            "expected no diagnostics, actual: {}",
                            describe_actuals(&valid.diagnostics)
                        ),
                    };
                }
            }
            DiagnosticExpectationKind::Absent => {
                let forbidden = expectation.code.as_deref();
                let hits: Vec<&ObservedDiagnostic> = valid
                    .diagnostics
                    .iter()
                    .filter(|actual| {
                        code_matches(actual, forbidden)
                            && message_matches(actual, expectation.message_contains.as_deref())
                    })
                    .collect();
                if !hits.is_empty() {
                    return DiagnosticMatchVerdict::Mismatch {
                        identity: expectation.identity.clone(),
                        evidence: format!(
                            "expected code {:?} absent, actual: {}",
                            expectation.code,
                            describe_actuals(&hits.into_iter().cloned().collect::<Vec<_>>())
                        ),
                    };
                }
            }
            DiagnosticExpectationKind::Present => {
                let verdict = bind_present(expectation, valid, &mut consumed, subject);
                if !verdict.is_pass() {
                    return verdict;
                }
            }
            DiagnosticExpectationKind::Count { count } => {
                let verdict = bind_count(expectation, count, valid, &mut consumed, subject);
                if !verdict.is_pass() {
                    return verdict;
                }
            }
        }
    }

    if extras == UnexpectedExtraPolicy::Reject {
        let leftovers: Vec<&ObservedDiagnostic> = valid
            .diagnostics
            .iter()
            .zip(consumed.iter())
            .filter_map(|(actual, used)| if *used { None } else { Some(actual) })
            .collect();
        if !leftovers.is_empty() {
            return DiagnosticMatchVerdict::UnexpectedExtra {
                identity: "fixture".to_string(),
                evidence: format!(
                    "unexpected extra diagnostics: {}",
                    describe_actuals(&leftovers.into_iter().cloned().collect::<Vec<_>>())
                ),
            };
        }
    }

    DiagnosticMatchVerdict::Pass
}

/// Compatibility extras policy while #7009 remains open.
///
/// Location-bearing `diagnostic_present` rows and `no_diagnostics` stay
/// closed-world. Unconstrained membership rows keep the historical extras
/// permit so this leaf does not rewrite gold fixtures owned by #18780/#17737.
#[must_use]
pub fn gold_extras_policy(assertions: &[GoldAssertion]) -> UnexpectedExtraPolicy {
    let location_bound = assertions.iter().any(|assertion| {
        matches!(assertion, GoldAssertion::DiagnosticPresent { byte_offset: Some(_), .. })
    });
    let closed_empty =
        assertions.iter().any(|assertion| matches!(assertion, GoldAssertion::NoDiagnostics));
    if location_bound || closed_empty {
        UnexpectedExtraPolicy::Reject
    } else {
        UnexpectedExtraPolicy::Permit
    }
}

/// Compatibility adapter: map current gold assertions without editing gold schema.
#[must_use]
pub fn gold_expectations(
    fixture_name: &str,
    assertions: &[GoldAssertion],
) -> Vec<DiagnosticExpectation> {
    assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| gold_expectation(fixture_name, index, assertion))
        .collect()
}

fn gold_expectation(
    fixture_name: &str,
    index: usize,
    assertion: &GoldAssertion,
) -> DiagnosticExpectation {
    match assertion {
        GoldAssertion::NoDiagnostics => DiagnosticExpectation {
            identity: gold_identity(fixture_name, index, "no_diagnostics"),
            kind: DiagnosticExpectationKind::Empty,
            code: None,
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        },
        GoldAssertion::NoDiagnostic { code } => DiagnosticExpectation {
            identity: gold_identity(fixture_name, index, "no_diagnostic"),
            kind: DiagnosticExpectationKind::Absent,
            code: Some(code.clone()),
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        },
        GoldAssertion::DiagnosticPresent { code, byte_offset, message_contains } => {
            DiagnosticExpectation {
                identity: gold_identity(fixture_name, index, "diagnostic_present"),
                kind: DiagnosticExpectationKind::Present,
                code: Some(code.clone()),
                message_contains: message_contains.clone(),
                location: match byte_offset {
                    Some(byte_offset) => {
                        LocationConstraint::PointContained { byte_offset: *byte_offset }
                    }
                    None => LocationConstraint::Unconstrained,
                },
            }
        }
        GoldAssertion::DiagnosticCount { code, count } => DiagnosticExpectation {
            identity: gold_identity(fixture_name, index, "diagnostic_count"),
            kind: DiagnosticExpectationKind::Count { count: *count },
            code: Some(code.clone()),
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        },
    }
}

fn gold_identity(fixture_name: &str, index: usize, kind: &str) -> String {
    format!("gold/{fixture_name}#expected.json/{index}:{kind}")
}

fn observe_publish_params(params: &Value, subject: DiagnosticSubject<'_>) -> DiagnosticObservation {
    let Some(params) = params.as_object() else {
        return DiagnosticObservation::InvalidObservation {
            reason: "publishDiagnostics params is not an object".to_string(),
        };
    };
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return DiagnosticObservation::InvalidObservation {
            reason: "publishDiagnostics missing uri".to_string(),
        };
    };
    if uri != subject.uri {
        return DiagnosticObservation::InvalidObservation {
            reason: format!("stale uri {uri:?} != expected {:?}", subject.uri),
        };
    }
    let version = match optional_version(params.get("version")) {
        Ok(version) => version,
        Err(reason) => {
            return DiagnosticObservation::InvalidObservation { reason };
        }
    };
    if let (Some(expected), Some(observed)) = (subject.version, version)
        && expected != observed
    {
        return DiagnosticObservation::InvalidObservation {
            reason: format!("stale version {observed} != expected {expected}"),
        };
    }
    let Some(diagnostics) = params.get("diagnostics") else {
        return DiagnosticObservation::InvalidObservation {
            reason: "publishDiagnostics missing diagnostics array".to_string(),
        };
    };
    decode_diagnostic_array(uri, version, diagnostics, subject)
}

fn observe_pull_result(result: &Value, subject: DiagnosticSubject<'_>) -> DiagnosticObservation {
    let Some(result) = result.as_object() else {
        return DiagnosticObservation::InvalidObservation {
            reason: "diagnostic result is not an object".to_string(),
        };
    };
    if let Some(kind) = result.get("kind").and_then(Value::as_str) {
        match kind {
            "full" => {}
            "unchanged" => {
                return DiagnosticObservation::InvalidObservation {
                    reason: "unchanged diagnostic report is not a complete observation".to_string(),
                };
            }
            other => {
                return DiagnosticObservation::InvalidObservation {
                    reason: format!("unsupported diagnostic report kind {other:?}"),
                };
            }
        }
    }
    let Some(items) = result.get("items") else {
        return DiagnosticObservation::InvalidObservation {
            reason: "diagnostic result missing items array".to_string(),
        };
    };
    decode_diagnostic_array(subject.uri, subject.version, items, subject)
}

fn decode_diagnostic_array(
    uri: &str,
    version: Option<i64>,
    diagnostics: &Value,
    subject: DiagnosticSubject<'_>,
) -> DiagnosticObservation {
    let Some(items) = diagnostics.as_array() else {
        return DiagnosticObservation::InvalidObservation {
            reason: "diagnostics is not an array".to_string(),
        };
    };
    let mut observed = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        match decode_one_diagnostic(item, subject) {
            Ok(diagnostic) => observed.push(diagnostic),
            Err(reason) => {
                return DiagnosticObservation::InvalidObservation {
                    reason: format!("diagnostics[{index}]: {reason}"),
                };
            }
        }
    }
    DiagnosticObservation::Valid(ValidDiagnosticObservation {
        uri: uri.to_string(),
        version,
        diagnostics: observed,
    })
}

fn decode_one_diagnostic(
    item: &Value,
    subject: DiagnosticSubject<'_>,
) -> Result<ObservedDiagnostic, String> {
    let Some(object) = item.as_object() else {
        return Err("diagnostic is not an object".to_string());
    };
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| "diagnostic missing string message".to_string())?
        .to_string();
    let range_value = object.get("range").ok_or_else(|| "diagnostic missing range".to_string())?;
    let range = parse_range(range_value)?;
    let mapping = wire_range_to_bytes(subject.source, range, subject.encoding);
    if !mapping.disposition().admits_incoming_mutation() {
        return Err(format!(
            "range {}:{}-{}:{} is not a canonical source span ({:?})",
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
            mapping.disposition()
        ));
    }
    let byte_range = mapping.byte_range().ok_or_else(|| {
        format!(
            "range {}:{}-{}:{} mapping admitted but produced no bytes",
            range.start.line, range.start.character, range.end.line, range.end.character
        )
    })?;
    Ok(ObservedDiagnostic {
        code: object.get("code").and_then(normalize_code),
        message,
        range,
        start_byte: byte_range.start,
        end_byte: byte_range.end,
    })
}

fn parse_range(value: &Value) -> Result<WireRange, String> {
    let Some(object) = value.as_object() else {
        return Err("range is not an object".to_string());
    };
    Ok(WireRange {
        start: parse_position(object.get("start"), "start")?,
        end: parse_position(object.get("end"), "end")?,
    })
}

fn parse_position(value: Option<&Value>, label: &str) -> Result<WirePosition, String> {
    let Some(object) = value.and_then(Value::as_object) else {
        return Err(format!("range.{label} is not an object"));
    };
    Ok(WirePosition {
        line: json_u32(object.get("line"))
            .ok_or_else(|| format!("range.{label}.line is not a u32"))?,
        character: json_u32(object.get("character"))
            .ok_or_else(|| format!("range.{label}.character is not a u32"))?,
    })
}

fn json_u32(value: Option<&Value>) -> Option<u32> {
    value.and_then(Value::as_u64).and_then(|number| u32::try_from(number).ok())
}

fn optional_version(value: Option<&Value>) -> Result<Option<i64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .ok_or_else(|| "publishDiagnostics version is not an integer".to_string())
            .map(Some),
    }
}

fn normalize_code(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(ToString::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn bind_present(
    expectation: &DiagnosticExpectation,
    valid: &ValidDiagnosticObservation,
    consumed: &mut [bool],
    subject: DiagnosticSubject<'_>,
) -> DiagnosticMatchVerdict {
    let candidates = match candidate_indices(expectation, valid, consumed, subject) {
        Ok(candidates) => candidates,
        Err(evidence) => {
            return DiagnosticMatchVerdict::Mismatch {
                identity: expectation.identity.clone(),
                evidence,
            };
        }
    };
    match candidates.len() {
        0 => DiagnosticMatchVerdict::Mismatch {
            identity: expectation.identity.clone(),
            evidence: format!(
                "expected {} ; actual: {}",
                describe_expectation(expectation, subject),
                describe_actuals(&valid.diagnostics)
            ),
        },
        1 => {
            if let Some(index) = candidates.first().copied()
                && let Some(flag) = consumed.get_mut(index)
            {
                *flag = true;
                DiagnosticMatchVerdict::Pass
            } else {
                DiagnosticMatchVerdict::InstrumentFailure {
                    evidence: format!("{}: candidate index out of range", expectation.identity),
                }
            }
        }
        _ => DiagnosticMatchVerdict::Ambiguous {
            identity: expectation.identity.clone(),
            evidence: format!(
                "expected {} matched {} actuals: {}",
                describe_expectation(expectation, subject),
                candidates.len(),
                describe_indexed(&valid.diagnostics, &candidates)
            ),
        },
    }
}

fn bind_count(
    expectation: &DiagnosticExpectation,
    count: usize,
    valid: &ValidDiagnosticObservation,
    consumed: &mut [bool],
    subject: DiagnosticSubject<'_>,
) -> DiagnosticMatchVerdict {
    let candidates = match candidate_indices(expectation, valid, consumed, subject) {
        Ok(candidates) => candidates,
        Err(evidence) => {
            return DiagnosticMatchVerdict::Mismatch {
                identity: expectation.identity.clone(),
                evidence,
            };
        }
    };
    if candidates.len() != count {
        return DiagnosticMatchVerdict::Mismatch {
            identity: expectation.identity.clone(),
            evidence: format!(
                "expected count {count} for {} ; matched {}: {}",
                describe_expectation(expectation, subject),
                candidates.len(),
                describe_indexed(&valid.diagnostics, &candidates)
            ),
        };
    }
    for index in candidates {
        if let Some(flag) = consumed.get_mut(index) {
            *flag = true;
        } else {
            return DiagnosticMatchVerdict::InstrumentFailure {
                evidence: format!("{}: candidate index out of range", expectation.identity),
            };
        }
    }
    DiagnosticMatchVerdict::Pass
}

fn candidate_indices(
    expectation: &DiagnosticExpectation,
    valid: &ValidDiagnosticObservation,
    consumed: &[bool],
    subject: DiagnosticSubject<'_>,
) -> Result<Vec<usize>, String> {
    let mut matches = Vec::new();
    for (index, actual) in valid.diagnostics.iter().enumerate() {
        if consumed.get(index).copied().unwrap_or(true) {
            continue;
        }
        if !code_matches(actual, expectation.code.as_deref()) {
            continue;
        }
        if !message_matches(actual, expectation.message_contains.as_deref()) {
            continue;
        }
        if location_matches(expectation.location, actual, subject)? {
            matches.push(index);
        }
    }
    Ok(matches)
}

fn location_matches(
    location: LocationConstraint,
    actual: &ObservedDiagnostic,
    subject: DiagnosticSubject<'_>,
) -> Result<bool, String> {
    match location {
        LocationConstraint::Unconstrained => Ok(true),
        LocationConstraint::PointContained { byte_offset } => {
            let point = exact_byte_position(subject, byte_offset)?;
            Ok(actual.start_byte <= point && point < actual.end_byte)
        }
        LocationConstraint::ExactStart { byte_offset } => {
            let start = exact_byte_position(subject, byte_offset)?;
            Ok(actual.start_byte == start)
        }
        LocationConstraint::ExactRange { start_byte, end_byte } => {
            let start = exact_byte_position(subject, start_byte)?;
            let end = exact_byte_position(subject, end_byte)?;
            Ok(actual.start_byte == start && actual.end_byte == end)
        }
    }
}

fn exact_byte_position(
    subject: DiagnosticSubject<'_>,
    byte_offset: usize,
) -> Result<usize, String> {
    let mapping = byte_to_wire_position(subject.source, byte_offset, subject.encoding);
    if mapping.disposition() != PositionMappingDisposition::Exact {
        return Err(format!(
            "invalid location byte_offset {byte_offset} ({:?})",
            mapping.disposition()
        ));
    }
    mapping.wire_position().map(|_| byte_offset).ok_or_else(|| {
        format!(
            "invalid location byte_offset {byte_offset}: exact mapping produced no wire position"
        )
    })
}

fn code_matches(actual: &ObservedDiagnostic, expected: Option<&str>) -> bool {
    match expected {
        None => true,
        Some(code) => actual.code.as_deref() == Some(code),
    }
}

fn message_matches(actual: &ObservedDiagnostic, needle: Option<&str>) -> bool {
    match needle {
        None => true,
        Some(needle) => actual.message.contains(needle),
    }
}

fn describe_expectation(
    expectation: &DiagnosticExpectation,
    subject: DiagnosticSubject<'_>,
) -> String {
    let code = expectation.code.as_deref().unwrap_or("<any>");
    let location = match expectation.location {
        LocationConstraint::Unconstrained => "unconstrained".to_string(),
        LocationConstraint::PointContained { byte_offset } => {
            format!("point-contained {}", describe_expected_byte(subject, byte_offset))
        }
        LocationConstraint::ExactStart { byte_offset } => {
            format!("exact-start {}", describe_expected_byte(subject, byte_offset))
        }
        LocationConstraint::ExactRange { start_byte, end_byte } => {
            format!(
                "exact-range {} .. {}",
                describe_expected_byte(subject, start_byte),
                describe_expected_byte(subject, end_byte)
            )
        }
    };
    format!("{code} {location}")
}

fn describe_expected_byte(subject: DiagnosticSubject<'_>, byte_offset: usize) -> String {
    match byte_to_wire_position(subject.source, byte_offset, subject.encoding).wire_position() {
        Some(position) => format!("byte {byte_offset} -> {}:{}", position.line, position.character),
        None => format!("byte {byte_offset} (unrepresentable)"),
    }
}

fn describe_actuals(actuals: &[ObservedDiagnostic]) -> String {
    if actuals.is_empty() {
        return "<none>".to_string();
    }
    actuals.iter().map(describe_actual).collect::<Vec<_>>().join("; ")
}

fn describe_indexed(actuals: &[ObservedDiagnostic], indices: &[usize]) -> String {
    indices
        .iter()
        .filter_map(|index| actuals.get(*index))
        .map(describe_actual)
        .collect::<Vec<_>>()
        .join("; ")
}

fn describe_actual(actual: &ObservedDiagnostic) -> String {
    let code = actual.code.as_deref().unwrap_or("<no-code>");
    format!(
        "{code} {}:{}-{}:{} bytes {}..{}",
        actual.range.start.line,
        actual.range.start.character,
        actual.range.end.line,
        actual.range.end.character,
        actual.start_byte,
        actual.end_byte
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};
    use serde_json::json;

    const URI: &str = "file:///gold/two_sites.pl";

    fn subject(source: &str) -> DiagnosticSubject<'_> {
        DiagnosticSubject { uri: URI, version: Some(1), source, encoding: PositionEncoding::Utf16 }
    }

    fn range_for(source: &str, start: usize, end: usize) -> Value {
        let start_pos = must_some(
            byte_to_wire_position(source, start, PositionEncoding::Utf16).wire_position(),
        );
        let end_pos =
            must_some(byte_to_wire_position(source, end, PositionEncoding::Utf16).wire_position());
        json!({
            "start": { "line": start_pos.line, "character": start_pos.character },
            "end": { "line": end_pos.line, "character": end_pos.character }
        })
    }

    fn publish(_source: &str, diagnostics: Vec<Value>) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": URI,
                "version": 1,
                "diagnostics": diagnostics
            }
        })
    }

    fn diag(source: &str, code: &str, message: &str, start: usize, end: usize) -> Value {
        json!({
            "code": code,
            "message": message,
            "range": range_for(source, start, end)
        })
    }

    fn present_at(identity: &str, code: &str, byte_offset: usize) -> DiagnosticExpectation {
        DiagnosticExpectation {
            identity: identity.to_string(),
            kind: DiagnosticExpectationKind::Present,
            code: Some(code.to_string()),
            message_contains: None,
            location: LocationConstraint::PointContained { byte_offset },
        }
    }

    fn empty_count(identity: &str, code: &str) -> DiagnosticExpectation {
        DiagnosticExpectation {
            identity: identity.to_string(),
            kind: DiagnosticExpectationKind::Count { count: 0 },
            code: Some(code.to_string()),
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        }
    }

    fn match_with(
        source: &str,
        payload: &Value,
        expectations: &[DiagnosticExpectation],
        extras: UnexpectedExtraPolicy,
    ) -> DiagnosticMatchVerdict {
        let observation = observe_diagnostics_json(payload, subject(source));
        match_diagnostic_expectations(expectations, &observation, extras, subject(source))
    }

    fn find_nth(source: &str, needle: &str, n: usize) -> Result<usize, String> {
        let mut from = 0;
        for index in 0..=n {
            let found = source[from..].find(needle).ok_or_else(|| {
                format!("needle {needle:?} occurrence {index} missing in {source:?}")
            })?;
            if index == n {
                return Ok(from + found);
            }
            from += found + needle.len();
        }
        Err(format!("needle {needle:?} occurrence {n} missing"))
    }

    #[test]
    fn same_code_wrong_occurrence_fails() -> Result<(), String> {
        // Discriminator: the server reports the right code on the wrong token.
        let source = "my $x = 1; my $x = 2;\n";
        let first = find_nth(source, "$x", 0)?;
        let second = find_nth(source, "$x", 1)?;
        let reported_at_second =
            publish(source, vec![diag(source, "PL001", "unused", second, second + 2)]);
        let reported_at_first =
            publish(source, vec![diag(source, "PL001", "unused", first, first + 2)]);
        let intended =
            present_at("gold/two_sites#expected.json/0:diagnostic_present", "PL001", second);

        let pass = match_with(
            source,
            &reported_at_second,
            std::slice::from_ref(&intended),
            UnexpectedExtraPolicy::Reject,
        );
        if !pass.is_pass() {
            return Err(format!("intended occurrence must pass, got {pass}"));
        }

        let wrong =
            match_with(source, &reported_at_first, &[intended], UnexpectedExtraPolicy::Reject);
        match wrong {
            DiagnosticMatchVerdict::Mismatch { .. } => Ok(()),
            other => Err(format!("same code at a different occurrence must mismatch, got {other}")),
        }
    }

    #[test]
    fn missing_or_malformed_publish_diagnostics_cannot_pass_zero_count() {
        let source = "my $x = 1;\n";
        let zero = [empty_count("gold/none#expected.json/0:diagnostic_count", "PL001")];
        let empty_ok =
            match_with(source, &publish(source, Vec::new()), &zero, UnexpectedExtraPolicy::Reject);
        assert!(
            empty_ok.is_pass(),
            "valid empty publishDiagnostics may pass zero-count, got {empty_ok}"
        );

        let missing = match_with(source, &json!({}), &zero, UnexpectedExtraPolicy::Reject);
        assert!(
            matches!(missing, DiagnosticMatchVerdict::InstrumentFailure { .. }),
            "missing observation must not pass zero-count, got {missing}"
        );

        let malformed = match_with(
            source,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": URI, "version": 1, "diagnostics": "nope" }
            }),
            &zero,
            UnexpectedExtraPolicy::Reject,
        );
        assert!(
            matches!(malformed, DiagnosticMatchVerdict::InvalidObservation { .. }),
            "malformed diagnostics array must not pass zero-count, got {malformed}"
        );

        let wrong_method = match_with(
            source,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishProgress",
                "params": { "uri": URI, "version": 1, "diagnostics": [] }
            }),
            &zero,
            UnexpectedExtraPolicy::Reject,
        );
        assert!(
            matches!(wrong_method, DiagnosticMatchVerdict::InvalidObservation { .. }),
            "wrong method must not pass zero-count, got {wrong_method}"
        );
    }

    #[test]
    fn duplicate_same_code_without_location_is_ambiguous() -> Result<(), String> {
        let source = "my $x = 1; my $x = 2;\n";
        let first = find_nth(source, "$x", 0)?;
        let second = find_nth(source, "$x", 1)?;
        let payload = publish(
            source,
            vec![
                diag(source, "PL001", "unused", first, first + 2),
                diag(source, "PL001", "unused", second, second + 2),
            ],
        );
        let expectation = DiagnosticExpectation {
            identity: "gold/dup#expected.json/0:diagnostic_present".to_string(),
            kind: DiagnosticExpectationKind::Present,
            code: Some("PL001".to_string()),
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        };
        let verdict = match_with(source, &payload, &[expectation], UnexpectedExtraPolicy::Reject);
        match verdict {
            DiagnosticMatchVerdict::Ambiguous { .. } => Ok(()),
            other => Err(format!(
                "two same-code actuals for one unconstrained expectation must be ambiguous, got {other}"
            )),
        }
    }

    #[test]
    fn one_actual_cannot_satisfy_two_expectations() -> Result<(), String> {
        let source = "my $x = 1;\n";
        let start = find_nth(source, "$x", 0)?;
        let payload = publish(source, vec![diag(source, "PL001", "unused", start, start + 2)]);
        let first = present_at("gold/one#expected.json/0:diagnostic_present", "PL001", start);
        let second = present_at("gold/one#expected.json/1:diagnostic_present", "PL001", start);
        let verdict = match_with(source, &payload, &[first, second], UnexpectedExtraPolicy::Reject);
        match verdict {
            DiagnosticMatchVerdict::Mismatch { .. } => Ok(()),
            other => Err(format!("second expectation must not reuse the only actual, got {other}")),
        }
    }

    #[test]
    fn unexpected_extra_is_rejected_by_default() -> Result<(), String> {
        let source = "my $x = 1; my $y = 2;\n";
        let x = find_nth(source, "$x", 0)?;
        let y = find_nth(source, "$y", 0)?;
        let payload = publish(
            source,
            vec![
                diag(source, "PL001", "unused x", x, x + 2),
                diag(source, "PL002", "unused y", y, y + 2),
            ],
        );
        let only_x = present_at("gold/extra#expected.json/0:diagnostic_present", "PL001", x);
        let rejected = match_with(
            source,
            &payload,
            std::slice::from_ref(&only_x),
            UnexpectedExtraPolicy::Reject,
        );
        assert!(
            matches!(rejected, DiagnosticMatchVerdict::UnexpectedExtra { .. }),
            "default extras policy must reject leftovers, got {rejected}"
        );
        let permitted = match_with(source, &payload, &[only_x], UnexpectedExtraPolicy::Permit);
        if !permitted.is_pass() {
            return Err(format!("explicit permit policy may ignore leftovers, got {permitted}"));
        }
        Ok(())
    }

    #[test]
    fn stale_and_utf16_crlf_and_exact_range_distinctions() -> Result<(), String> {
        let source = "café\r\n😀$x\n";
        let x = find_nth(source, "$x", 0)?;
        let payload = publish(source, vec![diag(source, "PL001", "unused", x, x + 2)]);

        let stale = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///other.pl",
                "version": 1,
                "diagnostics": []
            }
        });
        let stale_verdict = match_with(
            source,
            &stale,
            &[empty_count("gold/stale#expected.json/0:diagnostic_count", "PL001")],
            UnexpectedExtraPolicy::Reject,
        );
        assert!(
            matches!(stale_verdict, DiagnosticMatchVerdict::InvalidObservation { .. }),
            "stale uri must not pass zero-count, got {stale_verdict}"
        );

        let point = match_with(
            source,
            &payload,
            &[present_at("gold/crlf#expected.json/0:diagnostic_present", "PL001", x)],
            UnexpectedExtraPolicy::Reject,
        );
        assert!(point.is_pass(), "CRLF/astral prefix must convert, got {point}");

        let exact_ok = DiagnosticExpectation {
            identity: "gold/crlf#expected.json/1:exact_range".to_string(),
            kind: DiagnosticExpectationKind::Present,
            code: Some("PL001".to_string()),
            message_contains: None,
            location: LocationConstraint::ExactRange { start_byte: x, end_byte: x + 2 },
        };
        let exact_pass = match_with(source, &payload, &[exact_ok], UnexpectedExtraPolicy::Reject);
        assert!(exact_pass.is_pass(), "exact range must pass, got {exact_pass}");

        let exact_start_only = DiagnosticExpectation {
            identity: "gold/crlf#expected.json/2:exact_start".to_string(),
            kind: DiagnosticExpectationKind::Present,
            code: Some("PL001".to_string()),
            message_contains: None,
            location: LocationConstraint::ExactStart { byte_offset: x + 1 },
        };
        let start_mismatch =
            match_with(source, &payload, &[exact_start_only], UnexpectedExtraPolicy::Permit);
        assert!(
            matches!(start_mismatch, DiagnosticMatchVerdict::Mismatch { .. }),
            "point-inside-range must not satisfy exact-start, got {start_mismatch}"
        );

        let oob =
            present_at("gold/crlf#expected.json/3:diagnostic_present", "PL001", source.len() + 4);
        let oob_verdict = match_with(source, &payload, &[oob], UnexpectedExtraPolicy::Permit);
        assert!(
            matches!(oob_verdict, DiagnosticMatchVerdict::Mismatch { .. }),
            "out-of-bounds location must fail, got {oob_verdict}"
        );
        if !oob_verdict.to_string().contains("gold/crlf#expected.json/3:diagnostic_present") {
            return Err(format!("failure evidence must retain assertion identity: {oob_verdict}"));
        }
        Ok(())
    }

    #[test]
    fn utf8_encoding_distinguishes_astral_columns() -> Result<(), String> {
        let source = "😀x\n";
        let x = source.find('x').ok_or_else(|| "missing x".to_string())?;
        let utf8_subject = DiagnosticSubject {
            uri: URI,
            version: Some(1),
            source,
            encoding: PositionEncoding::Utf8,
        };
        let start =
            must_some(byte_to_wire_position(source, x, PositionEncoding::Utf8).wire_position());
        let end =
            must_some(byte_to_wire_position(source, x + 1, PositionEncoding::Utf8).wire_position());
        assert_eq!(start.character, 4, "UTF-8 column counts bytes before x");
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": URI,
                "version": 1,
                "diagnostics": [{
                    "code": "PL001",
                    "message": "x",
                    "range": {
                        "start": { "line": start.line, "character": start.character },
                        "end": { "line": end.line, "character": end.character }
                    }
                }]
            }
        });
        let observation = observe_diagnostics_json(&payload, utf8_subject);
        let verdict = match_diagnostic_expectations(
            &[present_at("gold/utf8#expected.json/0:diagnostic_present", "PL001", x)],
            &observation,
            UnexpectedExtraPolicy::Reject,
            utf8_subject,
        );
        if !verdict.is_pass() {
            return Err(format!("utf-8 encoding occurrence must pass, got {verdict}"));
        }
        Ok(())
    }

    #[test]
    fn gold_byte_offset_adapter_is_point_containment() -> Result<(), String> {
        let assertion: GoldAssertion = must(serde_json::from_str(
            r#"{"assertion":"diagnostic_present","code":"PL001","byte_offset":4}"#,
        ));
        let mapped = gold_expectations("two_sites", &[assertion]);
        assert_eq!(mapped.len(), 1);
        let first = must_some(mapped.first());
        assert_eq!(first.identity, "gold/two_sites#expected.json/0:diagnostic_present");
        assert_eq!(first.location, LocationConstraint::PointContained { byte_offset: 4 });
        Ok(())
    }

    #[test]
    fn pull_null_or_unchanged_cannot_pass_no_diagnostics() {
        let source = "1;\n";
        let empty = [DiagnosticExpectation {
            identity: "gold/hello#expected.json/0:no_diagnostics".to_string(),
            kind: DiagnosticExpectationKind::Empty,
            code: None,
            message_contains: None,
            location: LocationConstraint::Unconstrained,
        }];
        let null_result =
            match_with(source, &json!({"result": null}), &empty, UnexpectedExtraPolicy::Reject);
        assert!(
            matches!(null_result, DiagnosticMatchVerdict::InvalidObservation { .. }),
            "null pull result must not pass no_diagnostics, got {null_result}"
        );
        let unchanged = match_with(
            source,
            &json!({"result": {"kind": "unchanged", "resultId": "abc"}}),
            &empty,
            UnexpectedExtraPolicy::Reject,
        );
        assert!(
            matches!(unchanged, DiagnosticMatchVerdict::InvalidObservation { .. }),
            "unchanged report must not pass no_diagnostics, got {unchanged}"
        );
    }
}
