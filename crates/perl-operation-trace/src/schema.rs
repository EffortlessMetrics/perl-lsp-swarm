//! Schema version for the `operation_trace.v1` wire contract.

use serde::{Deserialize, Deserializer, Serialize};

/// Current `operation_trace.v1` schema version.
pub const SCHEMA_VERSION_V1: u32 = 1;

/// A versioned schema marker for `operation_trace.v1`.
///
/// Mirrors `perl-source-identity`'s `SourceIdentitySchemaVersion`: decoding
/// rejects any version this build does not support, rather than admitting it
/// and relying on a consumer to remember to call
/// [`OperationTraceSchemaVersion::is_supported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct OperationTraceSchemaVersion(pub u32);

impl<'de> Deserialize<'de> for OperationTraceSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u32::deserialize(deserializer)?;
        let version = Self(raw);
        if version.is_supported() {
            Ok(version)
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Unsigned(u64::from(raw)),
                &"a supported operation_trace schema version (currently 1)",
            ))
        }
    }
}

impl OperationTraceSchemaVersion {
    /// The current `operation_trace.v1` schema version.
    pub const V1: Self = Self(SCHEMA_VERSION_V1);

    /// Whether this version is one the current runtime recognizes.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.0 == SCHEMA_VERSION_V1
    }

    /// Unwrap the raw integer version.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for OperationTraceSchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation_trace.v{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn v1_is_supported() {
        assert!(OperationTraceSchemaVersion::V1.is_supported());
    }

    /// `SCHEMA_VERSION_V1` is a public contract constant; pin its exact
    /// numeric value directly, and that `V1` wraps exactly it.
    #[test]
    fn schema_version_v1_constant_is_exactly_one() {
        assert_eq!(SCHEMA_VERSION_V1, 1);
        assert_eq!(OperationTraceSchemaVersion::V1.as_u32(), SCHEMA_VERSION_V1);
    }

    #[test]
    fn unknown_version_is_unsupported() {
        assert!(!OperationTraceSchemaVersion(0).is_supported());
        assert!(!OperationTraceSchemaVersion(2).is_supported());
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", OperationTraceSchemaVersion::V1), "operation_trace.v1");
    }

    #[test]
    fn deserialization_fails_closed_on_unsupported_version() {
        for bad in ["0", "2", "99", "4294967295"] {
            assert!(
                serde_json::from_str::<OperationTraceSchemaVersion>(bad).is_err(),
                "schema version {bad} must be rejected"
            );
        }
    }

    /// Stronger than `deserialization_fails_closed_on_unsupported_version`'s
    /// `is_err()` checks: the rejected deserialization's own error message
    /// must name the currently supported version, not merely fail for some
    /// unspecified reason.
    #[test]
    fn deserialization_error_message_names_the_supported_version() {
        let err = serde_json::from_str::<OperationTraceSchemaVersion>("2").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("currently 1"),
            "expected the error to name the supported version, got: {message}"
        );
    }

    #[test]
    fn deserialization_round_trips_the_supported_version() {
        let json = serde_json::to_string(&OperationTraceSchemaVersion::V1).unwrap();
        assert_eq!(json, "1");
        let back: OperationTraceSchemaVersion = serde_json::from_str(&json).expect("v1 must parse");
        assert_eq!(back, OperationTraceSchemaVersion::V1);
    }

    #[test]
    fn unsupported_version_is_constructible_in_code() {
        let future = OperationTraceSchemaVersion(2);
        assert!(!future.is_supported());
        assert_eq!(future.as_u32(), 2);
    }
}
