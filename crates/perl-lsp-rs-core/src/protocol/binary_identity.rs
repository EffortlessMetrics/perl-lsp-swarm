//! Versioned protocol transport for canonical server, DAP, and VSIX identity.
//!
//! The transport adapts [`crate::product_identity::BinaryIdentityPacketV1`]. It
//! never reconstructs identity from filenames, configured paths, or human CLI
//! output.

use crate::product_identity::{
    BINARY_IDENTITY_SCHEMA_V1, BinaryIdentityPacketV1, BinaryRole, BuildIdentityState,
    PRODUCT_NAME,
};
use serde::{Deserialize, Serialize};

/// Protocol method for reading the current process-bound identity relation.
pub const BINARY_IDENTITY_METHOD: &str = "perl/binaryIdentity";
/// Protocol method for an explicit compatibility evaluation.
pub const BINARY_COMPATIBILITY_METHOD: &str = "perl/binaryCompatibility";
/// Current feature-family version.
pub const BINARY_IDENTITY_FEATURE_VERSION: u32 = 1;
/// Canonical extension identifier.
pub const CANONICAL_EXTENSION_ID: &str = "EffortlessMetrics.perl-lsp-rs";

/// Compatibility verdict returned to a negotiated client.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryCompatibilityState {
    /// Every required source, target, version, and candidate relation is proven equal.
    ExactMatch,
    /// The observed identities are compatible, but one or more exactness fields are absent.
    CompatiblePartial,
    /// A load-bearing identity relationship is proven inconsistent.
    Mismatch,
    /// The client requested an unsupported feature-family version.
    Unsupported,
    /// The request refers to another server process or environment snapshot.
    Stale,
    /// Mandatory evidence is unavailable or contradictory.
    NotProven,
}

/// Stable machine reasons contributing to a compatibility verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryCompatibilityReason {
    /// Server product or executable role is not canonical.
    ServerProductMismatch,
    /// Extension identity is not the expected product extension.
    ExtensionIdentityMismatch,
    /// Semantic versions differ.
    VersionMismatch,
    /// Target triples differ.
    TargetMismatch,
    /// One side cannot prove its target triple.
    TargetNotProven,
    /// Proven source revisions differ.
    SourceRevisionMismatch,
    /// One side cannot prove its source revision.
    SourceRevisionNotProven,
    /// Proven candidate identities differ.
    CandidateMismatch,
    /// One side cannot prove its candidate identity.
    CandidateNotProven,
    /// DAP packet has the wrong product role.
    DapRoleMismatch,
    /// No DAP packet is available for the preview surface.
    DapIdentityAbsent,
    /// One or more other exact build fields are unavailable.
    BuildIdentityPartial,
    /// The request names another server process.
    ServerInstanceStale,
    /// The request names another environment snapshot.
    EnvironmentSnapshotStale,
    /// Requested protocol-family version is unsupported.
    FeatureVersionUnsupported,
    /// Every compared relation is exact.
    ExactIdentityMatch,
}

/// Negotiated capability declaration for this feature family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BinaryIdentityCapabilityV1 {
    /// Feature-family version.
    pub version: u32,
    /// Whether a DAP identity may be returned with the server identity.
    pub supports_dap_identity: bool,
    /// Whether explicit compatibility evaluation is supported.
    pub supports_compatibility: bool,
}

impl Default for BinaryIdentityCapabilityV1 {
    fn default() -> Self {
        Self {
            version: BINARY_IDENTITY_FEATURE_VERSION,
            supports_dap_identity: true,
            supports_compatibility: true,
        }
    }
}

/// Expected VSIX identity supplied by extension/package authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpectedExtensionIdentityV1 {
    /// Extension identifier.
    pub id: String,
    /// Expected extension version.
    pub version: String,
    /// Expected candidate identity, when the managed installer can prove it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_identity: Option<String>,
    /// Expected managed target, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Client request for one current process-bound identity report.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BinaryIdentityRequestV1 {
    /// Requested feature-family version.
    pub feature_version: u32,
    /// Expected extension identity from trusted client packaging state.
    pub expected_extension: ExpectedExtensionIdentityV1,
    /// Server process identity previously observed by the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_server_instance_id: Option<String>,
    /// Environment snapshot identity previously observed by the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_environment_snapshot_id: Option<String>,
}

/// Current server-owned identity state adapted into the protocol response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BinaryIdentityTransportStateV1 {
    /// Canonical server runtime packet.
    pub server: BinaryIdentityPacketV1,
    /// Selected DAP packet, when known without probing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dap: Option<BinaryIdentityPacketV1>,
    /// Current server process identity.
    pub server_instance_id: String,
    /// Current canonical environment snapshot identity.
    pub environment_snapshot_id: String,
}

/// Response returned by the binary identity/compatibility feature family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BinaryIdentityResponseV1 {
    /// Feature-family version used for this response.
    pub feature_version: u32,
    /// Current server packet.
    pub server: BinaryIdentityPacketV1,
    /// Current DAP packet, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dap: Option<BinaryIdentityPacketV1>,
    /// Expected extension supplied by trusted client state.
    pub expected_extension: ExpectedExtensionIdentityV1,
    /// Process-bound server instance identity.
    pub server_instance_id: String,
    /// Environment snapshot identity.
    pub environment_snapshot_id: String,
    /// Compatibility verdict.
    pub compatibility: BinaryCompatibilityState,
    /// Stable reasons contributing to the verdict.
    pub reasons: Vec<BinaryCompatibilityReason>,
    /// Whether the normal packet is safe for copied support output.
    pub redacted: bool,
    /// Explicit authority limitations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

impl BinaryIdentityTransportStateV1 {
    /// Evaluate one negotiated request against current server-owned identity.
    #[must_use]
    pub fn respond(&self, request: BinaryIdentityRequestV1) -> BinaryIdentityResponseV1 {
        let (compatibility, reasons, limitations) = evaluate_compatibility(self, &request);
        BinaryIdentityResponseV1 {
            feature_version: BINARY_IDENTITY_FEATURE_VERSION,
            server: self.server.clone(),
            dap: self.dap.clone(),
            expected_extension: request.expected_extension,
            server_instance_id: self.server_instance_id.clone(),
            environment_snapshot_id: self.environment_snapshot_id.clone(),
            compatibility,
            reasons,
            redacted: true,
            limitations,
        }
    }
}

fn evaluate_compatibility(
    state: &BinaryIdentityTransportStateV1,
    request: &BinaryIdentityRequestV1,
) -> (
    BinaryCompatibilityState,
    Vec<BinaryCompatibilityReason>,
    Vec<String>,
) {
    if request.feature_version != BINARY_IDENTITY_FEATURE_VERSION {
        return (
            BinaryCompatibilityState::Unsupported,
            vec![BinaryCompatibilityReason::FeatureVersionUnsupported],
            vec![format!("requested_feature_version_{}", request.feature_version)],
        );
    }

    if request
        .expected_server_instance_id
        .as_deref()
        .is_some_and(|value| value != state.server_instance_id)
    {
        return (
            BinaryCompatibilityState::Stale,
            vec![BinaryCompatibilityReason::ServerInstanceStale],
            Vec::new(),
        );
    }
    if request
        .expected_environment_snapshot_id
        .as_deref()
        .is_some_and(|value| value != state.environment_snapshot_id)
    {
        return (
            BinaryCompatibilityState::Stale,
            vec![BinaryCompatibilityReason::EnvironmentSnapshotStale],
            Vec::new(),
        );
    }

    let mut mismatch = Vec::new();
    let mut partial = Vec::new();
    let mut limitations = Vec::new();

    if state.server.schema_version != BINARY_IDENTITY_SCHEMA_V1
        || state.server.product.name != PRODUCT_NAME
        || state.server.binary.role != BinaryRole::Server
        || state.server.binary.executable != "perllsp"
        || state.server.binary.cargo_package != "perllsp"
    {
        mismatch.push(BinaryCompatibilityReason::ServerProductMismatch);
    }
    if request.expected_extension.id != CANONICAL_EXTENSION_ID {
        mismatch.push(BinaryCompatibilityReason::ExtensionIdentityMismatch);
    }
    compare_version(
        &state.server.binary.version,
        &request.expected_extension.version,
        &mut mismatch,
    );
    compare_optional(
        state.server.build.target.as_deref(),
        request.expected_extension.target.as_deref(),
        BinaryCompatibilityReason::TargetMismatch,
        BinaryCompatibilityReason::TargetNotProven,
        &mut mismatch,
        &mut partial,
    );
    compare_optional(
        state.server.artifact.candidate_identity.as_deref(),
        request.expected_extension.candidate_identity.as_deref(),
        BinaryCompatibilityReason::CandidateMismatch,
        BinaryCompatibilityReason::CandidateNotProven,
        &mut mismatch,
        &mut partial,
    );

    if state.server.build.identity_state != BuildIdentityState::Exact {
        partial.push(BinaryCompatibilityReason::BuildIdentityPartial);
        limitations.push("server_build_identity_not_exact".to_owned());
    }

    match state.dap.as_ref() {
        Some(dap) => {
            if dap.product.name != PRODUCT_NAME
                || dap.binary.role != BinaryRole::Dap
                || dap.binary.executable != "perl-dap"
                || dap.binary.cargo_package != "perl-dap"
            {
                mismatch.push(BinaryCompatibilityReason::DapRoleMismatch);
            }
            compare_version(&dap.binary.version, &state.server.binary.version, &mut mismatch);
            compare_optional(
                dap.build.target.as_deref(),
                state.server.build.target.as_deref(),
                BinaryCompatibilityReason::TargetMismatch,
                BinaryCompatibilityReason::TargetNotProven,
                &mut mismatch,
                &mut partial,
            );
            compare_optional(
                dap.build.source_revision.as_deref(),
                state.server.build.source_revision.as_deref(),
                BinaryCompatibilityReason::SourceRevisionMismatch,
                BinaryCompatibilityReason::SourceRevisionNotProven,
                &mut mismatch,
                &mut partial,
            );
            compare_optional(
                dap.artifact.candidate_identity.as_deref(),
                state.server.artifact.candidate_identity.as_deref(),
                BinaryCompatibilityReason::CandidateMismatch,
                BinaryCompatibilityReason::CandidateNotProven,
                &mut mismatch,
                &mut partial,
            );
            if dap.build.identity_state != BuildIdentityState::Exact {
                partial.push(BinaryCompatibilityReason::BuildIdentityPartial);
                limitations.push("dap_build_identity_not_exact".to_owned());
            }
        }
        None => {
            partial.push(BinaryCompatibilityReason::DapIdentityAbsent);
            limitations.push("dap_identity_not_available_without_probe".to_owned());
        }
    }

    deduplicate(&mut mismatch);
    deduplicate(&mut partial);
    if !mismatch.is_empty() {
        return (BinaryCompatibilityState::Mismatch, mismatch, limitations);
    }
    if !partial.is_empty() {
        return (BinaryCompatibilityState::CompatiblePartial, partial, limitations);
    }
    (
        BinaryCompatibilityState::ExactMatch,
        vec![BinaryCompatibilityReason::ExactIdentityMatch],
        limitations,
    )
}

fn compare_version(actual: &str, expected: &str, mismatch: &mut Vec<BinaryCompatibilityReason>) {
    if actual != expected {
        mismatch.push(BinaryCompatibilityReason::VersionMismatch);
    }
}

fn compare_optional(
    actual: Option<&str>,
    expected: Option<&str>,
    mismatch_reason: BinaryCompatibilityReason,
    partial_reason: BinaryCompatibilityReason,
    mismatch: &mut Vec<BinaryCompatibilityReason>,
    partial: &mut Vec<BinaryCompatibilityReason>,
) {
    match (actual, expected) {
        (Some(actual), Some(expected)) if actual != expected => mismatch.push(mismatch_reason),
        (Some(_), Some(_)) => {}
        _ => partial.push(partial_reason),
    }
}

fn deduplicate(values: &mut Vec<BinaryCompatibilityReason>) {
    values.sort_by_key(|value| *value as u8);
    values.dedup();
}

#[cfg(test)]
mod tests {
    use super::{
        BINARY_IDENTITY_FEATURE_VERSION, BinaryCompatibilityReason, BinaryCompatibilityState,
        BinaryIdentityRequestV1, BinaryIdentityTransportStateV1, CANONICAL_EXTENSION_ID,
        ExpectedExtensionIdentityV1,
    };
    use crate::product_identity::{ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1};

    fn exact_input(candidate: &str) -> BinaryIdentityInput {
        BinaryIdentityInput {
            source_revision: Some("abc123".to_owned()),
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            artifact_role: Some(ArtifactRole::Managed),
            candidate_identity: Some(candidate.to_owned()),
            ..BinaryIdentityInput::default()
        }
    }

    fn request() -> BinaryIdentityRequestV1 {
        BinaryIdentityRequestV1 {
            feature_version: BINARY_IDENTITY_FEATURE_VERSION,
            expected_extension: ExpectedExtensionIdentityV1 {
                id: CANONICAL_EXTENSION_ID.to_owned(),
                version: "0.18.0".to_owned(),
                candidate_identity: Some("rc1".to_owned()),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
            },
            expected_server_instance_id: Some("server-1".to_owned()),
            expected_environment_snapshot_id: Some("env-1".to_owned()),
        }
    }

    fn state() -> BinaryIdentityTransportStateV1 {
        BinaryIdentityTransportStateV1 {
            server: BinaryIdentityPacketV1::server("0.18.0", exact_input("rc1")),
            dap: Some(BinaryIdentityPacketV1::dap("0.18.0", exact_input("rc1"))),
            server_instance_id: "server-1".to_owned(),
            environment_snapshot_id: "env-1".to_owned(),
        }
    }

    #[test]
    fn exact_server_dap_and_extension_match() {
        let response = state().respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::ExactMatch);
        assert_eq!(response.reasons, vec![BinaryCompatibilityReason::ExactIdentityMatch]);
        assert!(response.redacted);
    }

    #[test]
    fn same_version_different_source_is_a_mismatch() {
        let mut state = state();
        state.dap = Some(BinaryIdentityPacketV1::dap(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some("different".to_owned()),
                ..exact_input("rc1")
            },
        ));
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::SourceRevisionMismatch));
    }

    #[test]
    fn mixed_candidate_pair_is_a_mismatch() {
        let mut state = state();
        state.dap = Some(BinaryIdentityPacketV1::dap("0.18.0", exact_input("rc2")));
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::CandidateMismatch));
    }

    #[test]
    fn missing_extension_target_has_a_discriminating_partial_reason() {
        let mut request = request();
        request.expected_extension.target = None;
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::TargetNotProven));
        assert!(!response.reasons.contains(&BinaryCompatibilityReason::CandidateNotProven));
    }

    #[test]
    fn missing_candidate_has_a_discriminating_partial_reason() {
        let mut request = request();
        request.expected_extension.candidate_identity = None;
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::CandidateNotProven));
        assert!(!response.reasons.contains(&BinaryCompatibilityReason::TargetNotProven));
    }

    #[test]
    fn missing_dap_source_has_a_discriminating_partial_reason() {
        let mut state = state();
        state.dap = Some(BinaryIdentityPacketV1::dap(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: None,
                ..exact_input("rc1")
            },
        ));
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(
            response
                .reasons
                .contains(&BinaryCompatibilityReason::SourceRevisionNotProven)
        );
    }

    #[test]
    fn missing_dap_is_compatible_partial_not_exact() {
        let mut state = state();
        state.dap = None;
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::DapIdentityAbsent));
    }

    #[test]
    fn restarted_server_makes_the_request_stale() {
        let mut request = request();
        request.expected_server_instance_id = Some("old-server".to_owned());
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::Stale);
        assert_eq!(response.reasons, vec![BinaryCompatibilityReason::ServerInstanceStale]);
    }

    #[test]
    fn unsupported_feature_version_is_explicit() {
        let mut request = request();
        request.feature_version = 99;
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::Unsupported);
        assert_eq!(response.reasons, vec![BinaryCompatibilityReason::FeatureVersionUnsupported]);
    }
}
