//! Apply a published JSON Schema to an actual payload.
//!
//! Several receipt and manifest validators walk a schema *document* with
//! hand-written helpers -- asserting that `$id` is right, that an enum still
//! carries the pinned vocabulary, that a `required` list holds the expected
//! names -- but never compile the schema and run it over an instance. A
//! schema that is only walked binds nothing: `additionalProperties`,
//! `uniqueItems`, `minItems`, `minLength`, `minimum` and every nested
//! `$defs` constraint gate no producer and no schema-only consumer, so the
//! published `*.schema.json` is a weaker contract than it claims (#14268).
//!
//! `format` is deliberately not in that list. JSON Schema 2020-12 treats it
//! as an annotation unless a validator opts into assertion, and this helper
//! does not, so a `"format": "date-time"` keyword still binds only the
//! underlying `type`. Callers that need an asserted format must add their
//! own check rather than assume this layer supplies one.
//!
//! This module owns the apply step. The walk-only helpers stay where they
//! are and keep doing what they are good at -- proving the *schema* has not
//! drifted from the vocabulary the Rust code depends on -- and the apply
//! step adds the layer that actually binds the document.
//!
//! The precedent this generalizes is `compiler_lexical_cutline`, which
//! compiles its schema and iterates errors over the canonical manifest
//! bytes; `oracle_fixture_manifest` and `ux_scorecard` do the same for their
//! own subjects. Violations are returned rather than raised so callers keep
//! their own error type and can join schema violations with the rest of
//! their findings in one report.

use color_eyre::eyre::{Result, eyre};
use serde_json::Value;

/// Compile `schema` and collect one violation string per schema error found
/// in `payload`.
///
/// `schema_label` and `payload_label` are the caller's names for the two
/// documents -- normally their repository-relative paths. `payload_label`
/// prefixes every violation so a failing report names the offending
/// instance, and each violation carries the instance path of the error so a
/// deeply nested failure is locatable without re-reading the document.
///
/// # Errors
///
/// Returns `Err` only when the schema itself cannot compile. An invalid
/// schema is a defect in the contract rather than in the payload, and must
/// not be reported as though the payload were at fault.
pub fn validate_payload_against_schema(
    schema: &Value,
    schema_label: &str,
    payload: &Value,
    payload_label: &str,
) -> Result<Vec<String>> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| eyre!("{schema_label}: invalid schema: {error}"))?;
    Ok(validator
        .iter_errors(payload)
        .map(|error| {
            format!("{payload_label}: schema violation at {}: {error}", error.instance_path())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::validate_payload_against_schema;
    use color_eyre::eyre::Result;
    use serde_json::json;

    fn closed_schema() -> serde_json::Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "tags"],
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "tags": {"type": "array", "items": {"type": "string"}, "uniqueItems": true}
            }
        })
    }

    #[test]
    fn conforming_payload_has_no_violations() -> Result<()> {
        let violations = validate_payload_against_schema(
            &closed_schema(),
            "schema.json",
            &json!({"name": "ok", "tags": ["a", "b"]}),
            "payload.json",
        )?;

        assert!(violations.is_empty(), "unexpected violations: {violations:?}");
        Ok(())
    }

    /// The four constraint classes a walk-only check cannot reach: an
    /// unknown key under `additionalProperties: false`, a duplicate under
    /// `uniqueItems`, an empty string under `minLength`, and a missing
    /// `required` member. Each must surface as its own violation.
    #[test]
    fn reports_constraints_a_schema_walk_cannot_reach() -> Result<()> {
        let violations = validate_payload_against_schema(
            &closed_schema(),
            "schema.json",
            &json!({"name": "", "tags": ["a", "a"], "surprise": 1}),
            "payload.json",
        )?;

        assert_eq!(violations.len(), 3, "unexpected violations: {violations:?}");
        assert!(
            violations.iter().all(|violation| violation.starts_with("payload.json: ")),
            "violations must name the payload: {violations:?}"
        );

        let missing_required = validate_payload_against_schema(
            &closed_schema(),
            "schema.json",
            &json!({"name": "ok"}),
            "payload.json",
        )?;
        assert_eq!(missing_required.len(), 1, "unexpected violations: {missing_required:?}");
        assert!(
            missing_required[0].contains("tags"),
            "missing-required violation must name the field: {missing_required:?}"
        );
        Ok(())
    }

    /// A violation names where it happened, so a nested failure does not
    /// require re-reading the document to locate.
    #[test]
    fn violation_carries_the_instance_path() -> Result<()> {
        let violations = validate_payload_against_schema(
            &closed_schema(),
            "schema.json",
            &json!({"name": "ok", "tags": ["a", 7]}),
            "payload.json",
        )?;

        assert_eq!(violations.len(), 1, "unexpected violations: {violations:?}");
        assert!(
            violations[0].contains("/tags/1"),
            "violation must carry the instance path: {violations:?}"
        );
        Ok(())
    }

    /// Pins the documented limitation: `format` is an annotation here, not
    /// an assertion. Callers must not read a `"format"` keyword as proof
    /// that this layer checks it. If the validator default ever changes,
    /// this test fails and the module documentation gets corrected with it.
    #[test]
    fn format_is_an_annotation_not_an_assertion() -> Result<()> {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"when": {"type": "string", "format": "date-time"}}
        });

        let violations = validate_payload_against_schema(
            &schema,
            "schema.json",
            &json!({"when": "last Tuesday"}),
            "payload.json",
        )?;
        assert!(violations.is_empty(), "format is not asserted by default: {violations:?}");

        // The underlying `type` is still bound.
        let wrong_type = validate_payload_against_schema(
            &schema,
            "schema.json",
            &json!({"when": 7}),
            "payload.json",
        )?;
        assert_eq!(wrong_type.len(), 1, "unexpected violations: {wrong_type:?}");
        Ok(())
    }

    /// An uncompilable schema is a contract defect, never a payload
    /// violation: it must fail loudly instead of returning "no violations"
    /// and silently passing every document through.
    #[test]
    fn invalid_schema_is_an_error_not_an_empty_violation_list() {
        let error = validate_payload_against_schema(
            &json!({"type": "not-a-json-schema-type"}),
            "schema.json",
            &json!({}),
            "payload.json",
        )
        .expect_err("an uncompilable schema must not report a clean payload");

        assert!(
            error.to_string().contains("schema.json: invalid schema"),
            "unexpected error: {error:?}"
        );
    }
}
