//! Shared native-initialize witnesses for DAP capability-floor tests.
//!
//! #9578's anti-flattening control and #9581's positive core-cell control
//! must name the same surviving `dap.core`-derived true rows. Those rows are
//! bound to `supports_core` in `debug_adapter/process.rs` (`has_feature("dap.core")`).
//!
//! When a later floor closes one of these, update this table. Do not promote a
//! floored cell to keep the list non-empty, and do not treat an empty table as
//! proof that flattening is impossible.
//!
//! Not every test binary consumes every symbol; that is expected.

#![allow(dead_code)]

use serde_json::Value;

/// Native initialize rows still derived from `dap.core` and advertised true.
///
/// A wholesale deletion of advertisement rows, or flattening every boolean to
/// `false`, drops at least one of these. An absent key is not a witness: the
/// value must be present as a JSON boolean.
pub const DAP_CORE_DERIVED_TRUE_SIBLINGS: &[&str] =
    &["supportsConfigurationDoneRequest", "supportTerminateDebuggee", "supportsTerminateRequest"];

/// Former anti-flattening true-siblings that later floors closed.
///
/// Pin these false so this control cannot be "fixed" by promoting them, and so
/// a later accidental widening is visible here as well as in the owning floor
/// suite. The issue tag is the floor that closed the row.
pub const FORMER_TRUE_SIBLINGS_NOW_FLOORED: &[(&str, &str)] = &[
    ("supportsValueFormattingOptions", "#9581"),
    ("supportsBreakpointLocationsRequest", "#9581"),
    ("supportsSetVariable", "#8354"),
];

/// Capability field this claim must keep in `initialize_sequence.json`.
///
/// Deleting the field is not reconciliation; the golden must continue to name
/// the #9581 floor as `false`.
pub const VALUE_FORMAT_FLOOR_FIELD: &str = "supportsValueFormattingOptions";

#[must_use]
pub fn capability_bool(body: &Value, name: &str) -> Option<bool> {
    body.get(name).and_then(Value::as_bool)
}

pub fn assert_capability_bool(body: &Value, name: &str, expected: bool, why: &str) {
    assert_eq!(
        capability_bool(body, name),
        Some(expected),
        "{name}: {why}; present value was {:?}",
        body.get(name)
    );
}

/// Absent, null, or non-boolean values are not witnesses. Flattening by
/// deleting rows would otherwise look like `None != Some(true)` only on the
/// true-sibling side and could be missed on a floored pin that used
/// `unwrap_or(false)`.
pub fn assert_capability_is_json_boolean(body: &Value, name: &str) {
    match body.get(name) {
        Some(Value::Bool(_)) => {}
        other => panic!(
            "{name} must be present as a JSON boolean (absent/null/string is not a floor or a sibling witness); present value was {other:?}"
        ),
    }
}
