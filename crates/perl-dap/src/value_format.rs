//! One shared typed presentation policy for the DAP `ValueFormat` option (#9588).
//!
//! The pinned upstream schema (`microsoft/debug-adapter-adapter` commit
//! `bf8a5d27e8040044b84b863f90916e08925ee811`, see
//! `.ci/dap/protocol-authority.json`) defines `ValueFormat` as an object with a
//! single optional boolean property `hex`, accepted by exactly four request
//! families: `variables`, `setVariable`, `evaluate`, and `setExpression`.
//! (`StackTraceArguments.format` is the distinct `StackFrameFormat` type and is
//! not a `ValueFormat` family.)
//!
//! This module is the single authority for how a requested format turns an
//! already-established current typed value ([`crate::value::PerlValue`]) into a
//! display string. Formatting is presentation-only and happens after
//! acquisition/current-value semantics (#9050 train) are established:
//!
//! - no re-query, re-evaluation, or user callback is executed for formatting;
//! - display text is never reparsed as numeric authority — hex rendering reads
//!   the typed integer authority captured at acquisition time, and values
//!   without typed numeric authority keep their default display under any
//!   format;
//! - `value`, `type`, named/indexed counts, child references, `evaluateName`,
//!   frame identity, and completeness are independent from the display string;
//! - for `setVariable`/`setExpression`, the format affects the response
//!   rendering only — the assigned data is the admitted client value, and
//!   mutation/read-back authority stays with #8364/#9070.
//!
//! # Per-class rendering rules under `hex: true`
//!
//! | Typed class | Rendering | Notes |
//! |---|---|---|
//! | `Integer(0)` | `0x0` | zero |
//! | `Integer(n > 0)` | `0x{magnitude, lowercase}` (e.g. `42` → `0x2a`) | typed `i64` authority |
//! | `Integer(n < 0)` | `-0x{magnitude}` (e.g. `-42` → `-0x2a`) | signed sign–magnitude; the model is `i64`, so two's-complement reinterpretation would fabricate an unsigned value the model does not have |
//! | `Integer(i64::MIN/MAX)` | full 16-digit magnitude (`0x7fffffffffffffff`, `-0x8000000000000000`) | exact `i64`, never routed through `f64` |
//! | `Number(f64)` (incl. integral-valued floats such as `42.0`) | unchanged decimal | a float is not an integer authority; DAP defines no float-hex |
//! | `Scalar(String)` (even numeric-looking text) | unchanged | no heuristic parse of display text |
//! | `Undef`, references, containers, objects, code, globs, regexes, tied, truncated, error | unchanged | non-numeric classes |
//! | rows without typed facts (frame-argument strings, fallback placeholders, unparseable evaluate output) | unchanged | no typed numeric authority exists; heuristic parsing is forbidden |
//! | integer leaves inside array/hash/reference previews | hex | one policy at every render point |
//!
//! Under the default (no format, `hex` absent, `hex: false`, or `format: {}`)
//! every class renders exactly as before this policy existed.
//!
//! # Unsupported options
//!
//! Unknown properties inside `format` fail request deserialization
//! (`deny_unknown_fields` on [`crate::protocol::ValueFormat`]) — the single
//! documented compatibility behavior for unsupported options. A format is never
//! silently ignored while `supportsValueFormattingOptions` is advertised true.
//!
//! # #9581 capability floor
//!
//! Until the value-format re-enable gate passes (#9050 + #8364 + #9070 +
//! #7342/#7345 + #9588 + #9590), `supportsValueFormattingOptions` is an
//! explicit `false` wire row and a non-default `format` request (`hex: true`)
//! is rejected by the dispatcher before any handler runs — this policy stays
//! unit-proven here, ready for the re-enable PR to re-advertise.

use crate::protocol::ValueFormat;
use crate::types::Variable;
use crate::value::PerlValue;
use crate::variables::PerlVariableRenderer;
#[cfg(test)]
use perl_tdd_support::must_err;

/// The typed presentation policy distilled from a `ValueFormat` request option.
///
/// One enum, one implementation, consumed by every `ValueFormat` request family.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValueFormatPolicy {
    /// Default decimal presentation (no format, `hex` absent or `false`).
    /// Byte-identical to the pre-#9588 rendering.
    #[default]
    Decimal,
    /// Render values that have typed integer authority as hexadecimal.
    Hex,
}

impl ValueFormatPolicy {
    /// Distills the request `format` option into the typed policy.
    ///
    /// Total: the pinned schema's only property is the boolean `hex`, and
    /// unsupported properties already failed deserialization on
    /// [`ValueFormat`], so this cannot fail.
    #[must_use]
    pub fn from_options(options: Option<&ValueFormat>) -> Self {
        match options {
            Some(ValueFormat { hex: Some(true) }) => Self::Hex,
            _ => Self::Decimal,
        }
    }

    /// Renders a typed integer under this policy.
    ///
    /// `Decimal` is the identity (`i64::to_string`); `Hex` is sign–magnitude
    /// lowercase hexadecimal from the exact `i64` (see the module table).
    #[must_use]
    pub fn render_integer(self, value: i64) -> String {
        match self {
            Self::Decimal => value.to_string(),
            Self::Hex => format_hex_i64(value),
        }
    }

    /// Projects a display string for an already-established current value.
    ///
    /// `default_display` is the policy-neutral (decimal) rendering produced at
    /// acquisition time; `typed` is the typed value retained alongside it, when
    /// one exists. Under [`ValueFormatPolicy::Decimal`] (and for any class the
    /// table above leaves unchanged) the default display is returned
    /// byte-identical; only [`ValueFormatPolicy::Hex`] re-renders, and only
    /// from typed facts — never from the display string.
    #[must_use]
    pub fn project_display(self, default_display: &str, typed: Option<&PerlValue>) -> String {
        match (self, typed) {
            (Self::Hex, Some(typed)) => {
                PerlVariableRenderer::new().with_policy(self).render_display(typed)
            }
            _ => default_display.to_string(),
        }
    }

    /// Projects one protocol row under this policy.
    ///
    /// The display `value` is recomputed from typed facts (see
    /// [`Self::project_display`]); every identity field — `name`, `type`,
    /// `variablesReference`, `namedVariables`/`indexedVariables`,
    /// `evaluateName` — is copied unchanged from the cached row. Formatting can
    /// never leak into row identity.
    #[must_use]
    pub fn project_variable(self, row: &Variable, typed: Option<&PerlValue>) -> Variable {
        Variable {
            name: row.name.clone(),
            value: self.project_display(&row.value, typed),
            type_: row.type_.clone(),
            variables_reference: row.variables_reference,
            named_variables: row.named_variables,
            indexed_variables: row.indexed_variables,
            evaluate_name: row.evaluate_name.clone(),
        }
    }
}

/// Exact hexadecimal rendering of a signed `i64` (sign–magnitude, lowercase
/// digits, `0x` prefix): `0` → `0x0`, `42` → `0x2a`, `-42` → `-0x2a`,
/// `i64::MAX` → `0x7fffffffffffffff`, `i64::MIN` → `-0x8000000000000000`.
///
/// The magnitude is computed on the unsigned domain (`unsigned_abs`), so
/// `i64::MIN` cannot overflow.
fn format_hex_i64(value: i64) -> String {
    if value < 0 { format!("-0x{:x}", value.unsigned_abs()) } else { format!("0x{:x}", value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(hex: Option<bool>) -> ValueFormatPolicy {
        ValueFormatPolicy::from_options(Some(&ValueFormat { hex }))
    }

    // --- option distillation -------------------------------------------------

    #[test]
    fn no_format_defaults_to_decimal() {
        assert_eq!(ValueFormatPolicy::from_options(None), ValueFormatPolicy::Decimal);
    }

    #[test]
    fn empty_format_object_is_decimal() {
        assert_eq!(policy(None), ValueFormatPolicy::Decimal);
    }

    #[test]
    fn hex_false_is_decimal() {
        assert_eq!(policy(Some(false)), ValueFormatPolicy::Decimal);
    }

    #[test]
    fn hex_true_is_hex() {
        assert_eq!(policy(Some(true)), ValueFormatPolicy::Hex);
    }

    // --- unsupported options fail deserialization (the ONE compatibility
    //     behavior; shared by all four families through the same struct) -----

    #[test]
    fn unknown_format_property_fails_deserialization() {
        let raw = json!({ "hex": true, "decimal": true });
        let err = must_err(serde_json::from_value::<ValueFormat>(raw));
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    #[test]
    fn wrong_typed_hex_property_fails_deserialization() {
        let raw = json!({ "hex": "true" });
        assert!(serde_json::from_value::<ValueFormat>(raw).is_err());
    }

    #[test]
    fn hex_only_property_is_accepted() {
        let parsed: ValueFormat =
            serde_json::from_value(json!({ "hex": true })).unwrap_or_default();
        assert_eq!(parsed.hex, Some(true));
    }

    // --- hex semantics table (typed authority = PerlValue variant) ----------

    #[test]
    fn hex_table_zero_positive_negative_extremes() {
        let cases = [
            (0_i64, "0x0"),
            (42, "0x2a"),
            (255, "0xff"),
            (-42, "-0x2a"),
            (i64::MAX, "0x7fffffffffffffff"),
            (i64::MIN, "-0x8000000000000000"),
            (2_i64.pow(53), "0x20000000000000"), // beyond f64 exact range, exact via i64
        ];
        for (value, expected) in cases {
            assert_eq!(ValueFormatPolicy::Hex.render_integer(value), expected, "i64 {value}");
        }
    }

    #[test]
    fn decimal_policy_is_identity() {
        assert_eq!(ValueFormatPolicy::Decimal.render_integer(-42), "-42");
        assert_eq!(ValueFormatPolicy::Decimal.render_integer(0), "0");
    }

    // --- per-class projection -----------------------------------------------

    #[test]
    fn hex_projects_integer_from_typed_authority() {
        let p = ValueFormatPolicy::Hex;
        assert_eq!(p.project_display("42", Some(&PerlValue::Integer(42))), "0x2a");
        assert_eq!(p.project_display("-7", Some(&PerlValue::Integer(-7))), "-0x7");
    }

    #[test]
    fn hex_leaves_floats_unchanged_including_integral_valued_floats() {
        let p = ValueFormatPolicy::Hex;
        // 42.0 is a float, not an integer authority: no hex.
        assert_eq!(p.project_display("42", Some(&PerlValue::Number(42.0))), "42");
        assert_eq!(p.project_display("3.5", Some(&PerlValue::Number(3.5))), "3.5");
    }

    #[test]
    fn hex_never_parses_display_text_of_strings() {
        let p = ValueFormatPolicy::Hex;
        assert_eq!(p.project_display("\"42\"", Some(&PerlValue::Scalar("42".into()))), "\"42\"");
        assert_eq!(
            p.project_display("\"0x1E\"", Some(&PerlValue::Scalar("0x1E".into()))),
            "\"0x1E\""
        );
    }

    #[test]
    fn hex_applies_to_integer_leaves_inside_refs_and_containers() {
        let p = ValueFormatPolicy::Hex;
        // A reference chain renders its typed leaf under the policy.
        assert_eq!(
            p.project_display(
                "\\42",
                Some(&PerlValue::Reference(Box::new(PerlValue::Integer(42))))
            ),
            "\\0x2a"
        );
        // Container previews keep their shape; integer leaves render hex.
        let hash = PerlValue::Hash(vec![
            ("a".into(), PerlValue::Integer(1)),
            ("b".into(), PerlValue::Scalar("x".into())),
        ]);
        assert_eq!(p.project_display("HASH(2)", Some(&hash)), "{a => 0x1, b => \"x\"}");
    }

    #[test]
    fn hex_leaves_undef_truncated_error_and_unicode_strings_unchanged() {
        let p = ValueFormatPolicy::Hex;
        assert_eq!(p.project_display("undef", Some(&PerlValue::Undef)), "undef");
        let truncated = PerlValue::Truncated { summary: "...".into(), total_count: Some(500) };
        assert_eq!(p.project_display("... (500 total)", Some(&truncated)), "... (500 total)");
        assert_eq!(
            p.project_display("<error: boom>", Some(&PerlValue::Error("boom".into()))),
            "<error: boom>"
        );
        // Unicode content is a string class: unchanged, byte-safe.
        assert_eq!(
            p.project_display("\"café\"", Some(&PerlValue::Scalar("café".into()))),
            "\"café\""
        );
    }

    #[test]
    fn untyped_rows_keep_default_display_under_any_policy() {
        for p in [ValueFormatPolicy::Decimal, ValueFormatPolicy::Hex] {
            assert_eq!(p.project_display("42", None), "42");
        }
    }

    #[test]
    fn decimal_policy_is_byte_identical_default_display() {
        for default in ["42", "\"s\"", "undef", "ARRAY(3)", "... (500 total)"] {
            let typed = PerlValue::Integer(42);
            assert_eq!(ValueFormatPolicy::Decimal.project_display(default, Some(&typed)), default);
        }
    }

    // --- identity independence ----------------------------------------------

    #[test]
    fn projected_row_keeps_identity_fields_but_changes_only_value() {
        let row = Variable {
            name: "$n".into(),
            value: "42".into(),
            type_: Some("SCALAR".into()),
            variables_reference: 0,
            named_variables: Some(2),
            indexed_variables: Some(3),
            evaluate_name: Some("$n".into()),
        };
        let projected =
            ValueFormatPolicy::Hex.project_variable(&row, Some(&PerlValue::Integer(42)));
        assert_eq!(projected.value, "0x2a");
        assert_eq!(projected.name, "$n");
        assert_eq!(projected.type_, Some("SCALAR".into()));
        assert_eq!(projected.variables_reference, 0);
        assert_eq!(projected.named_variables, Some(2));
        assert_eq!(projected.indexed_variables, Some(3));
        assert_eq!(projected.evaluate_name, Some("$n".into()));
    }

    #[test]
    fn same_typed_value_renders_differently_without_identity_change() {
        let typed = PerlValue::Integer(255);
        let row = Variable {
            name: "$mask".into(),
            value: "255".into(),
            type_: Some("SCALAR".into()),
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            evaluate_name: Some("$mask".into()),
        };
        let decimal = ValueFormatPolicy::Decimal.project_variable(&row, Some(&typed));
        let hex = ValueFormatPolicy::Hex.project_variable(&row, Some(&typed));
        assert_eq!(decimal.value, "255");
        assert_eq!(hex.value, "0xff");
        // Identity fields identical across renderings.
        assert_eq!(decimal.name, hex.name);
        assert_eq!(decimal.type_, hex.type_);
        assert_eq!(decimal.variables_reference, hex.variables_reference);
        assert_eq!(decimal.evaluate_name, hex.evaluate_name);
    }

    // --- bounds --------------------------------------------------------------

    #[test]
    fn hex_rendering_is_bounded() {
        // Longest possible: sign + 0x + 16 digits = 19 chars.
        for v in [i64::MIN, i64::MAX, -1, 0, 1] {
            let rendered = ValueFormatPolicy::Hex.render_integer(v);
            assert!(rendered.len() <= 19, "unbounded hex render: {rendered}");
        }
    }
}
