//! Versioned protocol transport for canonical server, DAP, and VSIX identity.
//!
//! The transport adapts [`crate::product_identity::BinaryIdentityPacketV1`]. It
//! never reconstructs identity from filenames, configured paths, or human CLI
//! output, and it never labels a packet copy-safe until every emitted field has
//! passed the bounded protocol projection.

use crate::product_identity::{
    ArtifactRole, BINARY_IDENTITY_SCHEMA_V1, BinaryIdentityPacketV1, BinaryRole,
    BuildIdentityState, DEVELOPMENT_REPOSITORY, PRODUCT_IDENTITY_VERSION, PRODUCT_NAME,
    PUBLIC_REPOSITORY,
};
use serde::{Deserialize, Serialize};

/// Protocol method for reading the current process-bound identity relation.
pub const BINARY_IDENTITY_METHOD: &str = "perl/binaryIdentity";
/// Protocol method for an explicit compatibility evaluation.
pub const BINARY_COMPATIBILITY_METHOD: &str = "perl/binaryCompatibility";
/// Current feature-family version.
pub const BINARY_IDENTITY_FEATURE_VERSION: u32 = 1;
/// Canonical extension publisher.
pub const CANONICAL_EXTENSION_PUBLISHER: &str = "EffortlessMetrics";
/// Canonical extension package name.
pub const CANONICAL_EXTENSION_PACKAGE: &str = "perl-lsp-rs";
/// Canonical extension identifier.
pub const CANONICAL_EXTENSION_ID: &str = "EffortlessMetrics.perl-lsp-rs";
/// Current bounded DAP posture.
pub const CANONICAL_DAP_POSTURE: &str = "preview";

const MAX_IDENTITY_LEN: usize = 128;
const MAX_VERSION_LEN: usize = 64;
const MAX_TARGET_LEN: usize = 128;
const MAX_LIMITATION_LEN: usize = 128;
const GIT_REVISION_HEX_LEN: usize = 40;
const SHA256_HEX_LEN: usize = 64;

/// Compatibility verdict returned to a negotiated client.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryCompatibilityState {
    /// Every required product, source, target, artifact, and candidate relation is proven equal.
    ExactMatch,
    /// The observed identities are usable, but an optional exactness dimension is unavailable.
    CompatiblePartial,
    /// A load-bearing identity relationship is proven inconsistent.
    Mismatch,
    /// The client or packet uses an unsupported feature/contract version.
    Unsupported,
    /// The request refers to another server process or environment snapshot.
    Stale,
    /// Mandatory evidence is unavailable, unsafe, malformed, or contradictory.
    NotProven,
}

/// Stable machine reasons contributing to a compatibility verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryCompatibilityReason {
    /// Server product, executable, package, or role is not canonical.
    ServerProductMismatch,
    /// Packet repository identity is not canonical.
    ProductRepositoryMismatch,
    /// Packet schema cannot be interpreted as the current identity contract.
    PacketSchemaUnsupported,
    /// Packet product-identity contract version is unsupported.
    ProductIdentityVersionUnsupported,
    /// Packet DAP posture disagrees with the product contract.
    DapPostureMismatch,
    /// Extension publisher is not canonical.
    ExtensionPublisherMismatch,
    /// Extension package name is not canonical.
    ExtensionPackageMismatch,
    /// Extension identifier is not canonical.
    ExtensionIdentityMismatch,
    /// Extension/package authority identity is unavailable.
    ExtensionAuthorityNotProven,
    /// VSIX/package digest is unavailable.
    ExtensionPackageDigestNotProven,
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
    /// Proven source-tree digests differ.
    SourceTreeDigestMismatch,
    /// One side cannot prove its source-tree digest.
    SourceTreeDigestNotProven,
    /// Build profiles differ.
    ProfileMismatch,
    /// One side cannot prove its build profile.
    ProfileNotProven,
    /// Proven candidate identities differ.
    CandidateMismatch,
    /// One side cannot prove its candidate identity.
    CandidateNotProven,
    /// Artifact roles differ.
    ArtifactRoleMismatch,
    /// Artifact role is unavailable.
    ArtifactRoleNotProven,
    /// Externally bound artifact digests differ.
    ArtifactDigestMismatch,
    /// A required externally bound artifact digest is unavailable.
    ArtifactDigestNotProven,
    /// DAP packet has the wrong product role or executable/package identity.
    DapRoleMismatch,
    /// No DAP packet is available for the preview surface.
    DapIdentityAbsent,
    /// Build evidence is partial but still interpretable.
    BuildIdentityPartial,
    /// Mandatory build evidence is not proven.
    BuildIdentityNotProven,
    /// One or more values could not be emitted through the copy-safe projection.
    PayloadNotRedacted,
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

/// Expected VSIX and managed-binary identity supplied by extension/package authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpectedExtensionIdentityV1 {
    /// Extension publisher.
    pub publisher: String,
    /// Extension package name.
    pub package_name: String,
    /// Fully qualified extension identifier.
    pub id: String,
    /// Expected extension version.
    pub version: String,
    /// Expected candidate identity, when the managed installer can prove it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_identity: Option<String>,
    /// Expected managed target, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Digest of the exact VSIX/package bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    /// Digest of the exact selected server bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_sha256: Option<String>,
    /// Digest of the exact selected DAP bytes, when selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dap_sha256: Option<String>,
    /// Expected installation role of the selected binary pair.
    pub binary_artifact_role: ArtifactRole,
    /// Content-addressed managed/package authority subject.
    pub authority_identity: String,
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
    /// Current bounded server packet projection.
    pub server: BinaryIdentityPacketV1,
    /// Current bounded DAP packet projection, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dap: Option<BinaryIdentityPacketV1>,
    /// Bounded expected extension projection.
    pub expected_extension: ExpectedExtensionIdentityV1,
    /// Process-bound server instance identity.
    pub server_instance_id: String,
    /// Environment snapshot identity.
    pub environment_snapshot_id: String,
    /// Compatibility verdict.
    pub compatibility: BinaryCompatibilityState,
    /// Stable reasons contributing to the verdict.
    pub reasons: Vec<BinaryCompatibilityReason>,
    /// Whether no field had to be removed or replaced by the copy-safe projection.
    pub redacted: bool,
    /// Explicit authority limitations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

#[derive(Debug)]
struct SafePacket {
    packet: BinaryIdentityPacketV1,
    unchanged: bool,
}

#[derive(Debug)]
struct SafeExpectedExtension {
    identity: ExpectedExtensionIdentityV1,
    unchanged: bool,
}

impl BinaryIdentityTransportStateV1 {
    /// Evaluate one negotiated request against current server-owned identity.
    #[must_use]
    pub fn respond(&self, request: BinaryIdentityRequestV1) -> BinaryIdentityResponseV1 {
        let server = safe_packet_projection(&self.server);
        let dap = self.dap.as_ref().map(safe_packet_projection);
        let expected = safe_expected_extension_projection(&request.expected_extension);
        let (server_instance_id, server_instance_unchanged) =
            safe_required_token(&self.server_instance_id);
        let (environment_snapshot_id, environment_snapshot_unchanged) =
            safe_required_token(&self.environment_snapshot_id);

        let projected_state = BinaryIdentityTransportStateV1 {
            server: server.packet.clone(),
            dap: dap.as_ref().map(|projection| projection.packet.clone()),
            server_instance_id: server_instance_id.clone(),
            environment_snapshot_id: environment_snapshot_id.clone(),
        };
        let projected_request = BinaryIdentityRequestV1 {
            feature_version: request.feature_version,
            expected_extension: expected.identity.clone(),
            expected_server_instance_id: request.expected_server_instance_id.clone(),
            expected_environment_snapshot_id: request.expected_environment_snapshot_id.clone(),
        };
        let payload_unchanged = server.unchanged
            && dap.as_ref().is_none_or(|projection| projection.unchanged)
            && expected.unchanged
            && server_instance_unchanged
            && environment_snapshot_unchanged;
        let (compatibility, mut reasons, mut limitations) =
            evaluate_compatibility(&projected_state, &projected_request, payload_unchanged);
        limitations.sort();
        limitations.dedup();
        deduplicate(&mut reasons);

        BinaryIdentityResponseV1 {
            feature_version: BINARY_IDENTITY_FEATURE_VERSION,
            server: server.packet,
            dap: dap.map(|projection| projection.packet),
            expected_extension: expected.identity,
            server_instance_id,
            environment_snapshot_id,
            compatibility,
            reasons,
            redacted: payload_unchanged,
            limitations,
        }
    }
}

fn evaluate_compatibility(
    state: &BinaryIdentityTransportStateV1,
    request: &BinaryIdentityRequestV1,
    payload_unchanged: bool,
) -> (BinaryCompatibilityState, Vec<BinaryCompatibilityReason>, Vec<String>) {
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
    let mut not_proven = Vec::new();
    let mut partial = Vec::new();
    let mut limitations = Vec::new();

    if !payload_unchanged {
        not_proven.push(BinaryCompatibilityReason::PayloadNotRedacted);
        limitations.push("identity_payload_required_redaction".to_owned());
    }

    validate_packet_contract(
        &state.server,
        BinaryRole::Server,
        "perllsp",
        "perllsp",
        BinaryCompatibilityReason::ServerProductMismatch,
        &mut mismatch,
        &mut not_proven,
        &mut partial,
        &mut limitations,
    );
    validate_expected_extension(
        &request.expected_extension,
        &mut mismatch,
        &mut not_proven,
        &mut partial,
    );
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
    compare_artifact_role(
        state.server.artifact.role,
        request.expected_extension.binary_artifact_role,
        &mut mismatch,
        &mut not_proven,
    );
    compare_required_digest(
        state.server.artifact.digest.as_deref(),
        request.expected_extension.server_sha256.as_deref(),
        request.expected_extension.binary_artifact_role,
        &mut mismatch,
        &mut not_proven,
        &mut partial,
    );

    match state.dap.as_ref() {
        Some(dap) => {
            validate_packet_contract(
                dap,
                BinaryRole::Dap,
                "perl-dap",
                "perl-dap",
                BinaryCompatibilityReason::DapRoleMismatch,
                &mut mismatch,
                &mut not_proven,
                &mut partial,
                &mut limitations,
            );
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
                dap.build.source_tree_digest.as_deref(),
                state.server.build.source_tree_digest.as_deref(),
                BinaryCompatibilityReason::SourceTreeDigestMismatch,
                BinaryCompatibilityReason::SourceTreeDigestNotProven,
                &mut mismatch,
                &mut partial,
            );
            compare_optional(
                dap.build.profile.as_deref(),
                state.server.build.profile.as_deref(),
                BinaryCompatibilityReason::ProfileMismatch,
                BinaryCompatibilityReason::ProfileNotProven,
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
            compare_artifact_role(
                dap.artifact.role,
                request.expected_extension.binary_artifact_role,
                &mut mismatch,
                &mut not_proven,
            );
            compare_required_digest(
                dap.artifact.digest.as_deref(),
                request.expected_extension.dap_sha256.as_deref(),
                request.expected_extension.binary_artifact_role,
                &mut mismatch,
                &mut not_proven,
                &mut partial,
            );
        }
        None => {
            partial.push(BinaryCompatibilityReason::DapIdentityAbsent);
            limitations.push("dap_identity_not_available_without_probe".to_owned());
        }
    }

    deduplicate(&mut mismatch);
    deduplicate(&mut not_proven);
    deduplicate(&mut partial);
    if !mismatch.is_empty() {
        return (BinaryCompatibilityState::Mismatch, mismatch, limitations);
    }
    if !not_proven.is_empty() {
        return (BinaryCompatibilityState::NotProven, not_proven, limitations);
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

#[allow(clippy::too_many_arguments)]
fn validate_packet_contract(
    packet: &BinaryIdentityPacketV1,
    expected_role: BinaryRole,
    expected_executable: &str,
    expected_package: &str,
    role_mismatch_reason: BinaryCompatibilityReason,
    mismatch: &mut Vec<BinaryCompatibilityReason>,
    not_proven: &mut Vec<BinaryCompatibilityReason>,
    partial: &mut Vec<BinaryCompatibilityReason>,
    limitations: &mut Vec<String>,
) {
    if packet.schema_version != BINARY_IDENTITY_SCHEMA_V1 {
        not_proven.push(BinaryCompatibilityReason::PacketSchemaUnsupported);
    }
    if packet.product.name != PRODUCT_NAME
        || packet.binary.role != expected_role
        || packet.binary.executable != expected_executable
        || packet.binary.cargo_package != expected_package
    {
        mismatch.push(role_mismatch_reason);
    }
    if packet.product.public_repository != PUBLIC_REPOSITORY
        || packet.product.development_repository != DEVELOPMENT_REPOSITORY
    {
        mismatch.push(BinaryCompatibilityReason::ProductRepositoryMismatch);
    }
    if packet.compatibility.expected_product_identity_version != PRODUCT_IDENTITY_VERSION {
        not_proven.push(BinaryCompatibilityReason::ProductIdentityVersionUnsupported);
    }
    if packet.compatibility.dap_posture != CANONICAL_DAP_POSTURE {
        mismatch.push(BinaryCompatibilityReason::DapPostureMismatch);
    }

    match packet.build.identity_state {
        BuildIdentityState::Exact => {
            require_optional(
                packet.build.source_revision.as_deref(),
                BinaryCompatibilityReason::SourceRevisionNotProven,
                not_proven,
            );
            require_optional(
                packet.build.source_tree_digest.as_deref(),
                BinaryCompatibilityReason::SourceTreeDigestNotProven,
                not_proven,
            );
            require_optional(
                packet.build.target.as_deref(),
                BinaryCompatibilityReason::TargetNotProven,
                not_proven,
            );
            require_optional(
                packet.build.profile.as_deref(),
                BinaryCompatibilityReason::ProfileNotProven,
                not_proven,
            );
        }
        BuildIdentityState::Partial => {
            partial.push(BinaryCompatibilityReason::BuildIdentityPartial);
            limitations.push(format!("{}_build_identity_partial", expected_executable));
        }
        BuildIdentityState::NotProven => {
            not_proven.push(BinaryCompatibilityReason::BuildIdentityNotProven);
            limitations.push(format!("{}_build_identity_not_proven", expected_executable));
        }
    }

    if packet.artifact.role == ArtifactRole::Unknown {
        not_proven.push(BinaryCompatibilityReason::ArtifactRoleNotProven);
    }
}

fn validate_expected_extension(
    expected: &ExpectedExtensionIdentityV1,
    mismatch: &mut Vec<BinaryCompatibilityReason>,
    not_proven: &mut Vec<BinaryCompatibilityReason>,
    partial: &mut Vec<BinaryCompatibilityReason>,
) {
    if expected.publisher != CANONICAL_EXTENSION_PUBLISHER {
        mismatch.push(BinaryCompatibilityReason::ExtensionPublisherMismatch);
    }
    if expected.package_name != CANONICAL_EXTENSION_PACKAGE {
        mismatch.push(BinaryCompatibilityReason::ExtensionPackageMismatch);
    }
    if expected.id != CANONICAL_EXTENSION_ID {
        mismatch.push(BinaryCompatibilityReason::ExtensionIdentityMismatch);
    }
    if expected.authority_identity == "not_proven" {
        not_proven.push(BinaryCompatibilityReason::ExtensionAuthorityNotProven);
    }
    if expected.package_sha256.is_none() {
        match expected.binary_artifact_role {
            ArtifactRole::UserSupplied => {
                partial.push(BinaryCompatibilityReason::ExtensionPackageDigestNotProven)
            }
            _ => not_proven.push(BinaryCompatibilityReason::ExtensionPackageDigestNotProven),
        }
    }
    if expected.binary_artifact_role == ArtifactRole::Unknown {
        not_proven.push(BinaryCompatibilityReason::ArtifactRoleNotProven);
    }
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

fn compare_artifact_role(
    actual: ArtifactRole,
    expected: ArtifactRole,
    mismatch: &mut Vec<BinaryCompatibilityReason>,
    not_proven: &mut Vec<BinaryCompatibilityReason>,
) {
    match (actual, expected) {
        (ArtifactRole::Unknown, _) | (_, ArtifactRole::Unknown) => {
            not_proven.push(BinaryCompatibilityReason::ArtifactRoleNotProven)
        }
        (actual, expected) if actual != expected => {
            mismatch.push(BinaryCompatibilityReason::ArtifactRoleMismatch)
        }
        _ => {}
    }
}

fn compare_required_digest(
    actual: Option<&str>,
    expected: Option<&str>,
    role: ArtifactRole,
    mismatch: &mut Vec<BinaryCompatibilityReason>,
    not_proven: &mut Vec<BinaryCompatibilityReason>,
    partial: &mut Vec<BinaryCompatibilityReason>,
) {
    match (actual, expected) {
        (Some(actual), Some(expected)) if actual != expected => {
            mismatch.push(BinaryCompatibilityReason::ArtifactDigestMismatch)
        }
        (Some(_), Some(_)) => {}
        _ if role == ArtifactRole::UserSupplied => {
            partial.push(BinaryCompatibilityReason::ArtifactDigestNotProven)
        }
        _ => not_proven.push(BinaryCompatibilityReason::ArtifactDigestNotProven),
    }
}

fn require_optional(
    value: Option<&str>,
    reason: BinaryCompatibilityReason,
    not_proven: &mut Vec<BinaryCompatibilityReason>,
) {
    if value.is_none() {
        not_proven.push(reason);
    }
}

fn deduplicate(values: &mut Vec<BinaryCompatibilityReason>) {
    values.sort_by_key(|value| *value as u8);
    values.dedup();
}

fn safe_packet_projection(packet: &BinaryIdentityPacketV1) -> SafePacket {
    let mut projected = packet.clone();
    let mut unchanged = true;

    sanitize_required(&mut projected.schema_version, is_schema_version, &mut unchanged);
    sanitize_required(&mut projected.product.name, is_general_token, &mut unchanged);
    sanitize_required(
        &mut projected.product.public_repository,
        is_repository_value,
        &mut unchanged,
    );
    sanitize_required(
        &mut projected.product.development_repository,
        is_repository_value,
        &mut unchanged,
    );
    sanitize_required(&mut projected.binary.executable, is_general_token, &mut unchanged);
    sanitize_required(&mut projected.binary.cargo_package, is_general_token, &mut unchanged);
    sanitize_required(&mut projected.binary.version, is_version, &mut unchanged);
    sanitize_optional(&mut projected.build.source_revision, is_git_revision, &mut unchanged);
    sanitize_optional(&mut projected.build.source_tree_digest, is_sha256, &mut unchanged);
    sanitize_optional(&mut projected.build.target, is_target, &mut unchanged);
    sanitize_optional(&mut projected.build.profile, is_general_token, &mut unchanged);
    sanitize_optional(&mut projected.artifact.digest, is_sha256, &mut unchanged);
    sanitize_optional(&mut projected.artifact.candidate_identity, is_general_token, &mut unchanged);
    sanitize_required(&mut projected.compatibility.dap_posture, is_general_token, &mut unchanged);
    projected.limitations.retain(|value| {
        let safe = is_reason_token(value);
        unchanged &= safe;
        safe
    });
    projected.limitations.sort();
    projected.limitations.dedup();
    if !unchanged {
        projected.limitations.push("identity_payload_redacted".to_owned());
        projected.limitations.sort();
        projected.limitations.dedup();
    }

    SafePacket { packet: projected, unchanged }
}

fn safe_expected_extension_projection(
    expected: &ExpectedExtensionIdentityV1,
) -> SafeExpectedExtension {
    let mut identity = expected.clone();
    let mut unchanged = true;
    sanitize_required(&mut identity.publisher, is_general_token, &mut unchanged);
    sanitize_required(&mut identity.package_name, is_general_token, &mut unchanged);
    sanitize_required(&mut identity.id, is_extension_id, &mut unchanged);
    sanitize_required(&mut identity.version, is_version, &mut unchanged);
    sanitize_optional(&mut identity.candidate_identity, is_general_token, &mut unchanged);
    sanitize_optional(&mut identity.target, is_target, &mut unchanged);
    sanitize_optional(&mut identity.package_sha256, is_sha256, &mut unchanged);
    sanitize_optional(&mut identity.server_sha256, is_sha256, &mut unchanged);
    sanitize_optional(&mut identity.dap_sha256, is_sha256, &mut unchanged);
    sanitize_required(&mut identity.authority_identity, is_general_token, &mut unchanged);
    SafeExpectedExtension { identity, unchanged }
}

fn sanitize_required(value: &mut String, validator: fn(&str) -> bool, unchanged: &mut bool) {
    if !validator(value) {
        *value = "not_proven".to_owned();
        *unchanged = false;
    }
}

fn sanitize_optional(
    value: &mut Option<String>,
    validator: fn(&str) -> bool,
    unchanged: &mut bool,
) {
    if value.as_deref().is_some_and(|item| !validator(item)) {
        *value = None;
        *unchanged = false;
    }
}

fn safe_required_token(value: &str) -> (String, bool) {
    if is_general_token(value) {
        (value.to_owned(), true)
    } else {
        ("not_proven".to_owned(), false)
    }
}

fn is_schema_version(value: &str) -> bool {
    value == BINARY_IDENTITY_SCHEMA_V1
}

fn is_repository_value(value: &str) -> bool {
    value == PUBLIC_REPOSITORY || value == DEVELOPMENT_REPOSITORY
}

fn is_git_revision(value: &str) -> bool {
    value.len() == GIT_REVISION_HEX_LEN && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256(value: &str) -> bool {
    value.len() == SHA256_HEX_LEN && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VERSION_LEN
        && value.is_ascii()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
        })
        && value.chars().any(|character| character.is_ascii_digit())
}

fn is_target(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TARGET_LEN
        && value.is_ascii()
        && value.contains('-')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn is_extension_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_LEN
        && value.is_ascii()
        && value.contains('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn is_general_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_LEN
        && value.is_ascii()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.split('/').any(|segment| segment == "." || segment == "..")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '.' | ':' | '@' | '+')
        })
}

fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_LIMITATION_LEN
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

#[cfg(test)]
mod tests {
    use super::{
        BINARY_IDENTITY_FEATURE_VERSION, BinaryCompatibilityReason, BinaryCompatibilityState,
        BinaryIdentityRequestV1, BinaryIdentityTransportStateV1, CANONICAL_EXTENSION_ID,
        CANONICAL_EXTENSION_PACKAGE, CANONICAL_EXTENSION_PUBLISHER, ExpectedExtensionIdentityV1,
    };
    use crate::product_identity::{ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1};

    fn revision(character: char) -> String {
        character.to_string().repeat(40)
    }

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn exact_input(candidate: &str, artifact_digest: &str) -> BinaryIdentityInput {
        BinaryIdentityInput {
            source_revision: Some(revision('a')),
            source_tree_digest: Some(digest('b')),
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            profile: Some("release".to_owned()),
            artifact_role: Some(ArtifactRole::Managed),
            artifact_digest: Some(artifact_digest.to_owned()),
            candidate_identity: Some(candidate.to_owned()),
        }
    }

    fn request() -> BinaryIdentityRequestV1 {
        BinaryIdentityRequestV1 {
            feature_version: BINARY_IDENTITY_FEATURE_VERSION,
            expected_extension: ExpectedExtensionIdentityV1 {
                publisher: CANONICAL_EXTENSION_PUBLISHER.to_owned(),
                package_name: CANONICAL_EXTENSION_PACKAGE.to_owned(),
                id: CANONICAL_EXTENSION_ID.to_owned(),
                version: "0.18.0".to_owned(),
                candidate_identity: Some("rc1".to_owned()),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
                package_sha256: Some(digest('e')),
                server_sha256: Some(digest('c')),
                dap_sha256: Some(digest('d')),
                binary_artifact_role: ArtifactRole::Managed,
                authority_identity: "vsix:rc1".to_owned(),
            },
            expected_server_instance_id: Some("server-1".to_owned()),
            expected_environment_snapshot_id: Some("env-1".to_owned()),
        }
    }

    fn state() -> BinaryIdentityTransportStateV1 {
        BinaryIdentityTransportStateV1 {
            server: BinaryIdentityPacketV1::server("0.18.0", exact_input("rc1", &digest('c'))),
            dap: Some(BinaryIdentityPacketV1::dap("0.18.0", exact_input("rc1", &digest('d')))),
            server_instance_id: "server-1".to_owned(),
            environment_snapshot_id: "env-1".to_owned(),
        }
    }

    #[test]
    fn exact_server_dap_extension_and_artifacts_match() {
        let response = state().respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::ExactMatch);
        assert_eq!(response.reasons, vec![BinaryCompatibilityReason::ExactIdentityMatch]);
        assert!(response.redacted, "exact bounded payload must be copy-safe");
    }

    #[test]
    fn same_version_different_source_tree_is_a_mismatch() {
        let mut state = state();
        state.dap = Some(BinaryIdentityPacketV1::dap(
            "0.18.0",
            BinaryIdentityInput {
                source_tree_digest: Some(digest('9')),
                ..exact_input("rc1", &digest('d'))
            },
        ));
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::SourceTreeDigestMismatch));
    }

    #[test]
    fn wrong_repository_and_product_contract_do_not_exact_match() {
        let mut state = state();
        state.server.product.public_repository = "Other/project".to_owned();
        state.server.compatibility.expected_product_identity_version = 99;
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::ProductRepositoryMismatch));
    }

    #[test]
    fn not_proven_build_identity_is_not_compatible_partial() {
        let mut state = state();
        state.server = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::NotProven);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::BuildIdentityNotProven));
    }

    #[test]
    fn expected_artifact_digest_is_load_bearing() {
        let mut request = request();
        request.expected_extension.server_sha256 = Some(digest('f'));
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::ArtifactDigestMismatch));
    }

    #[test]
    fn missing_dap_is_bounded_preview_partial() {
        let mut state = state();
        state.dap = None;
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::DapIdentityAbsent));
    }

    #[test]
    fn user_supplied_missing_digest_is_partial_not_mismatch() {
        let mut state = state();
        state.server.artifact.role = ArtifactRole::UserSupplied;
        state.server.artifact.digest = None;
        state.dap = None;
        let mut request = request();
        request.expected_extension.binary_artifact_role = ArtifactRole::UserSupplied;
        request.expected_extension.server_sha256 = None;
        request.expected_extension.dap_sha256 = None;
        let response = state.respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::CompatiblePartial);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::ArtifactDigestNotProven));
    }

    #[test]
    fn unsafe_payload_is_sanitized_and_not_copy_safe() {
        let mut state = state();
        state.server.build.target = Some("/home/user/private-target".to_owned());
        state.server.limitations.push("private=/home/user".to_owned());
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::NotProven);
        assert!(!response.redacted, "source payload required a redaction");
        assert_eq!(response.server.build.target, None);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::PayloadNotRedacted));
        let raw = serde_json::to_string(&response).expect("response serialization must succeed");
        assert!(!raw.contains("/home/user"), "response leaked a private path: {raw}");
    }

    #[test]
    fn extension_id_mismatch_is_reportable_without_schema_fabrication() {
        let mut request = request();
        request.expected_extension.id = "Other.extension".to_owned();
        let response = state().respond(request);
        assert_eq!(response.compatibility, BinaryCompatibilityState::Mismatch);
        assert_eq!(response.expected_extension.id, "Other.extension");
        assert!(response.reasons.contains(&BinaryCompatibilityReason::ExtensionIdentityMismatch));
    }

    #[test]
    fn unsupported_dap_packet_schema_is_not_exact() {
        let mut state = state();
        let dap = state.dap.as_mut().expect("fixture carries a DAP packet");
        dap.schema_version = "perl_lsp.binary_identity.v2".to_owned();
        let response = state.respond(request());
        assert_eq!(response.compatibility, BinaryCompatibilityState::NotProven);
        assert!(response.reasons.contains(&BinaryCompatibilityReason::PacketSchemaUnsupported));
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
