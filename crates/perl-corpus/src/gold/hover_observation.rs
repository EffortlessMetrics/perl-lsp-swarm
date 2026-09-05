//! Typed fail-closed observation of an LSP hover JSON-RPC response.
//!
//! The gold matcher previously collapsed every non-`MarkupContent.value` shape
//! into the same `None` as a legitimate `"result": null`. This module is the
//! oracle that replaces that extraction: it normalizes every LSP-valid
//! `Hover.contents` shape and names product versus instrument failure so no
//! assertion can pass on an error, a missing envelope, or a malformed payload.
//!
//! Unit-testable without a running server.

use super::{HoverAssertion, HoverAssertionKind};
use serde_json::Value;
use std::fmt;

/// Maximum retained rendered hover text, in UTF-8 bytes.
///
/// Larger payloads are truncated at a char boundary so failure evidence stays
/// bounded. Observation classification uses the truncated text.
pub const HOVER_RENDERED_TEXT_BOUND: usize = 8192;

/// Rigor class for a hover gold assertion.
///
/// Compatibility kinds prove usefulness (nullness / substring). Exact kinds
/// prove range identity. A scorecard must not report a compatibility row as
/// range proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverAssertionRigor {
    /// Substring or nullness kind. Not range or subject-identity proof.
    Compatibility,
    /// Range-bearing kind. A wrong-token card with matching text fails.
    Exact,
}

/// Terminal classification of a hover JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverObservation {
    /// Valid hover with normalized contents and optional range.
    Content(HoverContent),
    /// Explicit `"result": null` — the only legitimate no-hover success.
    LegitimateNoHover,
    /// JSON-RPC `error` object. Never satisfies any hover assertion.
    ProductFailure {
        /// JSON-RPC error code, when present and integral.
        code: Option<i64>,
        /// JSON-RPC error message, or empty when absent.
        message: String,
    },
    /// Missing, unsupported, or malformed observation. Never a no-hover pass.
    InstrumentFailure(HoverInstrumentFailure),
}

/// Why a hover envelope could not be observed as content or legitimate null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverInstrumentFailure {
    /// Response has neither `result` nor `error`.
    MissingResult,
    /// `result` is present but is not a hover object or null.
    MalformedHover,
    /// `contents` is missing or not one of the closed LSP shapes.
    UnsupportedContentsShape,
    /// Valid contents shape rendered no user-visible text.
    EmptyContents,
    /// `range` is present but not a well-formed LSP Range.
    MalformedRange,
}

/// Normalized hover contents plus optional range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverContent {
    /// Ordered sections in protocol order (`MarkedString[]` preserves order).
    pub sections: Vec<HoverContentSection>,
    /// Sections joined with `\\n`, truncated to [`HOVER_RENDERED_TEXT_BOUND`].
    pub rendered_text: String,
    /// Observed `Hover.range`, when present and well-formed.
    pub range: Option<HoverRange>,
}

/// One normalized contents section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverContentSection {
    /// Section text (`value` or bare `MarkedString`).
    pub text: String,
    /// Protocol form that produced this section.
    pub form: HoverContentForm,
}

/// Closed LSP contents forms retained after normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverContentForm {
    /// `MarkupContent` with a declared [`HoverMarkupKind`].
    Markup {
        /// `plaintext` or `markdown`.
        kind: HoverMarkupKind,
    },
    /// Bare string, or `{ language, value }`.
    MarkedString {
        /// Language identifier when the object form was used.
        language: Option<String>,
    },
}

/// `MarkupKind` values admitted by the LSP hover contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverMarkupKind {
    /// `plaintext`
    Plaintext,
    /// `markdown`
    Markdown,
}

/// LSP Range (`start` inclusive, `end` exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverRange {
    /// Inclusive start.
    pub start: HoverPosition,
    /// Exclusive end.
    pub end: HoverPosition,
}

/// Zero-based LSP position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverPosition {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based character offset in the negotiated encoding.
    pub character: u32,
}

/// Why a hover assertion failed against an observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverAssertionFailure {
    /// Discriminating reason for the mismatch.
    pub reason: String,
}

impl fmt::Display for HoverInstrumentFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingResult => write!(f, "instrument failure: missing result"),
            Self::MalformedHover => write!(f, "instrument failure: malformed hover result"),
            Self::UnsupportedContentsShape => {
                write!(f, "instrument failure: unsupported contents shape")
            }
            Self::EmptyContents => write!(f, "instrument failure: empty contents"),
            Self::MalformedRange => write!(f, "instrument failure: malformed range"),
        }
    }
}

impl fmt::Display for HoverAssertionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl HoverAssertionKind {
    /// Compatibility versus exact rigor for this kind.
    #[must_use]
    pub fn rigor(&self) -> HoverAssertionRigor {
        match self {
            Self::HoverNonNull
            | Self::HoverNull
            | Self::HoverContains { .. }
            | Self::HoverAbsent { .. } => HoverAssertionRigor::Compatibility,
            Self::HoverRangeCovers | Self::HoverRangeEquals { .. } => HoverAssertionRigor::Exact,
        }
    }
}

impl HoverRange {
    /// LSP half-open containment: `start <= position < end`.
    #[must_use]
    pub fn covers(self, line: u32, character: u32) -> bool {
        position_on_or_after(self.start, line, character)
            && position_before(line, character, self.end)
    }
}

/// Observe a JSON-RPC hover response. Product failure wins over `result`.
#[must_use]
pub fn observe_hover_response(response: &Value) -> HoverObservation {
    if let Some(error) = response.get("error") {
        return observe_product_failure(error);
    }
    match response.get("result") {
        None => HoverObservation::InstrumentFailure(HoverInstrumentFailure::MissingResult),
        Some(Value::Null) => HoverObservation::LegitimateNoHover,
        Some(result) => observe_hover_result(result),
    }
}

/// Match a gold assertion against a typed observation.
///
/// Product and instrument failures satisfy no assertion. `hover_null` passes
/// only on [`HoverObservation::LegitimateNoHover`]. `hover_absent` requires
/// a content observation.
pub fn match_hover_assertion(
    observation: &HoverObservation,
    assertion: &HoverAssertion,
) -> Result<(), HoverAssertionFailure> {
    match &assertion.kind {
        HoverAssertionKind::HoverNull => match observation {
            HoverObservation::LegitimateNoHover => Ok(()),
            other => Err(fail(format!(
                "hover_null requires explicit result null, got {}",
                describe_observation(other)
            ))),
        },
        HoverAssertionKind::HoverNonNull => content(observation).map(|_| ()).ok_or_else(|| {
            fail(format!(
                "hover_non_null requires hover content, got {}",
                describe_observation(observation)
            ))
        }),
        HoverAssertionKind::HoverContains { needle } => {
            let content = require_content(observation, "hover_contains")?;
            if content.rendered_text.contains(needle.as_str()) {
                Ok(())
            } else {
                Err(fail(format!(
                    "hover_contains missing needle {needle:?} in {:?}",
                    content.rendered_text
                )))
            }
        }
        HoverAssertionKind::HoverAbsent { needle } => {
            let content = require_content(observation, "hover_absent")?;
            if content.rendered_text.contains(needle.as_str()) {
                Err(fail(format!(
                    "hover_absent found forbidden needle {needle:?} in {:?}",
                    content.rendered_text
                )))
            } else {
                Ok(())
            }
        }
        HoverAssertionKind::HoverRangeCovers => {
            let content = require_content(observation, "hover_range_covers")?;
            let range = content.range.ok_or_else(|| {
                fail("hover_range_covers requires an observed Hover.range".to_string())
            })?;
            if range.covers(assertion.line, assertion.character) {
                Ok(())
            } else {
                Err(fail(format!(
                    "hover_range_covers: range {}:{}-{}:{} does not cover request {}:{}",
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                    assertion.line,
                    assertion.character
                )))
            }
        }
        HoverAssertionKind::HoverRangeEquals {
            start_line,
            start_character,
            end_line,
            end_character,
        } => {
            let content = require_content(observation, "hover_range_equals")?;
            let range = content.range.ok_or_else(|| {
                fail("hover_range_equals requires an observed Hover.range".to_string())
            })?;
            let expected = HoverRange {
                start: HoverPosition { line: *start_line, character: *start_character },
                end: HoverPosition { line: *end_line, character: *end_character },
            };
            if range == expected {
                Ok(())
            } else {
                Err(fail(format!(
                    "hover_range_equals: observed {}:{}-{}:{} != expected {}:{}-{}:{}",
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                    start_line,
                    start_character,
                    end_line,
                    end_character
                )))
            }
        }
    }
}

fn fail(reason: String) -> HoverAssertionFailure {
    HoverAssertionFailure { reason }
}

fn require_content<'a>(
    observation: &'a HoverObservation,
    kind: &str,
) -> Result<&'a HoverContent, HoverAssertionFailure> {
    content(observation).ok_or_else(|| {
        fail(format!("{kind} requires hover content, got {}", describe_observation(observation)))
    })
}

fn content(observation: &HoverObservation) -> Option<&HoverContent> {
    match observation {
        HoverObservation::Content(content) => Some(content),
        _ => None,
    }
}

fn describe_observation(observation: &HoverObservation) -> String {
    match observation {
        HoverObservation::Content(content) => {
            format!("content text {:?}", content.rendered_text)
        }
        HoverObservation::LegitimateNoHover => "legitimate no-hover (result null)".to_string(),
        HoverObservation::ProductFailure { code, message } => {
            format!("product failure code={code:?} message={message:?}")
        }
        HoverObservation::InstrumentFailure(failure) => failure.to_string(),
    }
}

fn observe_product_failure(error: &Value) -> HoverObservation {
    let code = error.get("code").and_then(Value::as_i64);
    let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    HoverObservation::ProductFailure { code, message }
}

fn observe_hover_result(result: &Value) -> HoverObservation {
    let Some(object) = result.as_object() else {
        return HoverObservation::InstrumentFailure(HoverInstrumentFailure::MalformedHover);
    };

    let range = match object.get("range") {
        None => None,
        Some(value) => match parse_range(value) {
            Some(range) => Some(range),
            None => {
                return HoverObservation::InstrumentFailure(HoverInstrumentFailure::MalformedRange);
            }
        },
    };

    let Some(contents) = object.get("contents") else {
        return HoverObservation::InstrumentFailure(
            HoverInstrumentFailure::UnsupportedContentsShape,
        );
    };
    let Some(sections) = normalize_contents(contents) else {
        return HoverObservation::InstrumentFailure(
            HoverInstrumentFailure::UnsupportedContentsShape,
        );
    };
    let rendered_text = bound_rendered_text(&sections);
    if rendered_text.is_empty() {
        return HoverObservation::InstrumentFailure(HoverInstrumentFailure::EmptyContents);
    }

    HoverObservation::Content(HoverContent { sections, rendered_text, range })
}

fn normalize_contents(contents: &Value) -> Option<Vec<HoverContentSection>> {
    match contents {
        Value::String(text) => Some(vec![marked_section(text.clone(), None)]),
        Value::Array(items) => {
            let mut sections = Vec::with_capacity(items.len());
            for item in items {
                sections.push(marked_string_section(item)?);
            }
            Some(sections)
        }
        Value::Object(_) => {
            markup_section(contents).or_else(|| marked_string_section(contents)).map(|s| vec![s])
        }
        _ => None,
    }
}

fn markup_section(value: &Value) -> Option<HoverContentSection> {
    let kind = value.get("kind")?.as_str()?;
    let markup = match kind {
        "plaintext" => HoverMarkupKind::Plaintext,
        "markdown" => HoverMarkupKind::Markdown,
        _ => return None,
    };
    let text = value.get("value")?.as_str()?.to_string();
    if value.as_object()?.len() != 2 {
        return None;
    }
    Some(HoverContentSection { text, form: HoverContentForm::Markup { kind: markup } })
}

fn marked_string_section(value: &Value) -> Option<HoverContentSection> {
    if let Some(text) = value.as_str() {
        return Some(marked_section(text.to_string(), None));
    }
    let language = value.get("language")?.as_str()?.to_string();
    let text = value.get("value")?.as_str()?.to_string();
    if value.as_object()?.len() != 2 {
        return None;
    }
    Some(marked_section(text, Some(language)))
}

fn marked_section(text: String, language: Option<String>) -> HoverContentSection {
    HoverContentSection { text, form: HoverContentForm::MarkedString { language } }
}

fn bound_rendered_text(sections: &[HoverContentSection]) -> String {
    let joined =
        sections.iter().map(|section| section.text.as_str()).collect::<Vec<_>>().join("\n");
    bound_text(joined)
}

fn bound_text(text: String) -> String {
    if text.len() <= HOVER_RENDERED_TEXT_BOUND {
        return text;
    }
    let mut end = HOVER_RENDERED_TEXT_BOUND;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = text[..end].to_string();
    truncated.push('…');
    truncated
}

fn parse_range(value: &Value) -> Option<HoverRange> {
    let object = value.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let start = parse_position(object.get("start")?)?;
    let end = parse_position(object.get("end")?)?;
    if position_before(end.line, end.character, start) {
        return None;
    }
    Some(HoverRange { start, end })
}

fn parse_position(value: &Value) -> Option<HoverPosition> {
    let object = value.as_object()?;
    if object.len() != 2 {
        return None;
    }
    Some(HoverPosition {
        line: parse_u32_field(object.get("line")?)?,
        character: parse_u32_field(object.get("character")?)?,
    })
}

fn parse_u32_field(value: &Value) -> Option<u32> {
    let number = value.as_u64()?;
    u32::try_from(number).ok()
}

fn position_on_or_after(start: HoverPosition, line: u32, character: u32) -> bool {
    start.line < line || (start.line == line && start.character <= character)
}

fn position_before(line: u32, character: u32, end: HoverPosition) -> bool {
    line < end.line || (line == end.line && character < end.character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assertion(kind: HoverAssertionKind, line: u32, character: u32) -> HoverAssertion {
        HoverAssertion { kind, line, character, rationale: String::new() }
    }

    fn markup(value: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "contents": { "kind": "markdown", "value": value }
            }
        })
    }

    #[test]
    fn markup_content_is_content_not_null() -> Result<(), String> {
        let observation = observe_hover_response(&markup("` $x `"));
        match observation {
            HoverObservation::Content(content) => {
                if content.rendered_text != "` $x `" {
                    return Err(format!("unexpected text {:?}", content.rendered_text));
                }
                let section = content.sections.first().ok_or("missing section")?;
                if !matches!(
                    section.form,
                    HoverContentForm::Markup { kind: HoverMarkupKind::Markdown }
                ) {
                    return Err(format!("unexpected form {:?}", section.form));
                }
                Ok(())
            }
            other => Err(format!("expected content, got {other:?}")),
        }
    }

    #[test]
    fn marked_string_bare_and_language_object_normalize() -> Result<(), String> {
        let bare = json!({"result": {"contents": "bare hover"}});
        match observe_hover_response(&bare) {
            HoverObservation::Content(content) => {
                if content.rendered_text != "bare hover" {
                    return Err(format!("bare text {:?}", content.rendered_text));
                }
                let section = content.sections.first().ok_or("missing bare section")?;
                if !matches!(section.form, HoverContentForm::MarkedString { language: None }) {
                    return Err(format!("bare form {:?}", section.form));
                }
            }
            other => return Err(format!("bare string should be content, got {other:?}")),
        }

        let language = json!({
            "result": { "contents": { "language": "perl", "value": "my $x" } }
        });
        match observe_hover_response(&language) {
            HoverObservation::Content(content) => {
                if content.rendered_text != "my $x" {
                    return Err(format!("language text {:?}", content.rendered_text));
                }
                let section = content.sections.first().ok_or("missing language section")?;
                if section.form
                    != (HoverContentForm::MarkedString { language: Some("perl".to_string()) })
                {
                    return Err(format!("language form {:?}", section.form));
                }
                Ok(())
            }
            other => Err(format!("language object should be content, got {other:?}")),
        }
    }

    #[test]
    fn marked_string_array_preserves_order_and_rejects_markup_member() -> Result<(), String> {
        let array = json!({
            "result": { "contents": ["first", { "language": "perl", "value": "second" }] }
        });
        match observe_hover_response(&array) {
            HoverObservation::Content(content) => {
                if content.rendered_text != "first\nsecond" {
                    return Err(format!("array text {:?}", content.rendered_text));
                }
                if content.sections.len() != 2 {
                    return Err(format!("array sections {}", content.sections.len()));
                }
            }
            other => return Err(format!("array should be content, got {other:?}")),
        }

        let mixed = json!({
            "result": {
                "contents": [
                    "ok",
                    { "kind": "markdown", "value": "not a MarkedString" }
                ]
            }
        });
        assert!(matches!(
            observe_hover_response(&mixed),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::UnsupportedContentsShape)
        ));
        Ok(())
    }

    #[test]
    fn unsupported_and_empty_shapes_are_instrument_failures_not_no_hover() {
        let missing_value = json!({"result": {"contents": {"kind": "markdown"}}});
        assert!(matches!(
            observe_hover_response(&missing_value),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::UnsupportedContentsShape)
        ));

        let bad_kind = json!({"result": {"contents": {"kind": "html", "value": "x"}}});
        assert!(matches!(
            observe_hover_response(&bad_kind),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::UnsupportedContentsShape)
        ));

        let extra = json!({"result": {"contents": {"kind": "markdown", "value": "x", "k": 1}}});
        assert!(matches!(
            observe_hover_response(&extra),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::UnsupportedContentsShape)
        ));

        let empty = json!({"result": {"contents": {"kind": "plaintext", "value": ""}}});
        assert!(matches!(
            observe_hover_response(&empty),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::EmptyContents)
        ));
    }

    #[test]
    fn json_rpc_error_and_missing_result_never_satisfy_null_or_absent() {
        let error = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32603, "message": "internal" }
        });
        let observation = observe_hover_response(&error);
        assert!(matches!(observation, HoverObservation::ProductFailure { code: Some(-32603), .. }));
        assert!(
            match_hover_assertion(&observation, &assertion(HoverAssertionKind::HoverNull, 0, 0))
                .is_err()
        );
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverAbsent { needle: "x".into() }, 0, 0)
            )
            .is_err()
        );
        assert!(
            match_hover_assertion(&observation, &assertion(HoverAssertionKind::HoverNonNull, 0, 0))
                .is_err()
        );
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverContains { needle: "x".into() }, 0, 0)
            )
            .is_err()
        );

        let missing = json!({"jsonrpc": "2.0", "id": 1});
        let missing_obs = observe_hover_response(&missing);
        assert!(matches!(
            missing_obs,
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::MissingResult)
        ));
        assert!(
            match_hover_assertion(&missing_obs, &assertion(HoverAssertionKind::HoverNull, 0, 0))
                .is_err()
        );
    }

    #[test]
    fn hover_null_passes_only_on_explicit_result_null() {
        let null = json!({"result": null});
        let observation = observe_hover_response(&null);
        assert!(matches!(observation, HoverObservation::LegitimateNoHover));
        assert!(
            match_hover_assertion(&observation, &assertion(HoverAssertionKind::HoverNull, 1, 2))
                .is_ok()
        );
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverAbsent { needle: "x".into() }, 1, 2)
            )
            .is_err()
        );
    }

    #[test]
    fn hover_absent_requires_content_and_rejects_needle() {
        let content = observe_hover_response(&markup("scalar $greeting"));
        assert!(
            match_hover_assertion(
                &content,
                &assertion(HoverAssertionKind::HoverAbsent { needle: "array".into() }, 4, 3)
            )
            .is_ok()
        );
        assert!(
            match_hover_assertion(
                &content,
                &assertion(HoverAssertionKind::HoverAbsent { needle: "$greeting".into() }, 4, 3)
            )
            .is_err()
        );
    }

    #[test]
    fn range_covers_request_and_wrong_token_fails() {
        let response = json!({
            "result": {
                "contents": { "kind": "markdown", "value": "$greeting" },
                "range": {
                    "start": { "line": 4, "character": 3 },
                    "end": { "line": 4, "character": 12 }
                }
            }
        });
        let observation = observe_hover_response(&response);
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverRangeCovers, 4, 3)
            )
            .is_ok()
        );
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverRangeCovers, 4, 11)
            )
            .is_ok()
        );
        // Exclusive end: character 12 is not inside.
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverRangeCovers, 4, 12)
            )
            .is_err()
        );
        assert!(
            match_hover_assertion(
                &observation,
                &assertion(HoverAssertionKind::HoverRangeCovers, 5, 0)
            )
            .is_err()
        );

        let equals = HoverAssertionKind::HoverRangeEquals {
            start_line: 4,
            start_character: 3,
            end_line: 4,
            end_character: 12,
        };
        assert!(match_hover_assertion(&observation, &assertion(equals.clone(), 4, 3)).is_ok());
        let wrong = HoverAssertionKind::HoverRangeEquals {
            start_line: 4,
            start_character: 0,
            end_line: 4,
            end_character: 12,
        };
        assert!(match_hover_assertion(&observation, &assertion(wrong, 4, 3)).is_err());
    }

    #[test]
    fn malformed_range_is_instrument_failure() {
        let inverted = json!({
            "result": {
                "contents": { "kind": "markdown", "value": "x" },
                "range": {
                    "start": { "line": 2, "character": 5 },
                    "end": { "line": 2, "character": 1 }
                }
            }
        });
        assert!(matches!(
            observe_hover_response(&inverted),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::MalformedRange)
        ));

        let extra = json!({
            "result": {
                "contents": { "kind": "markdown", "value": "x" },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 },
                    "extra": true
                }
            }
        });
        assert!(matches!(
            observe_hover_response(&extra),
            HoverObservation::InstrumentFailure(HoverInstrumentFailure::MalformedRange)
        ));
    }

    #[test]
    fn error_wins_over_result_and_rendered_text_is_bounded() -> Result<(), String> {
        let both = json!({
            "result": { "contents": { "kind": "markdown", "value": "card" } },
            "error": { "code": -32000, "message": "boom" }
        });
        assert!(matches!(
            observe_hover_response(&both),
            HoverObservation::ProductFailure { code: Some(-32000), .. }
        ));

        let huge = "a".repeat(HOVER_RENDERED_TEXT_BOUND + 8);
        let observation = observe_hover_response(&markup(&huge));
        match observation {
            HoverObservation::Content(content) => {
                assert!(content.rendered_text.ends_with('…'));
                assert!(content.rendered_text.len() <= HOVER_RENDERED_TEXT_BOUND + '…'.len_utf8());
            }
            other => return Err(format!("expected bounded content, got {other:?}")),
        }
        Ok(())
    }

    #[test]
    fn compatibility_and_exact_rigor_are_distinct() {
        assert_eq!(HoverAssertionKind::HoverNonNull.rigor(), HoverAssertionRigor::Compatibility);
        assert_eq!(HoverAssertionKind::HoverNull.rigor(), HoverAssertionRigor::Compatibility);
        assert_eq!(
            HoverAssertionKind::HoverContains { needle: "x".into() }.rigor(),
            HoverAssertionRigor::Compatibility
        );
        assert_eq!(
            HoverAssertionKind::HoverAbsent { needle: "x".into() }.rigor(),
            HoverAssertionRigor::Compatibility
        );
        assert_eq!(HoverAssertionKind::HoverRangeCovers.rigor(), HoverAssertionRigor::Exact);
        assert_eq!(
            HoverAssertionKind::HoverRangeEquals {
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 1,
            }
            .rigor(),
            HoverAssertionRigor::Exact
        );
    }
}
