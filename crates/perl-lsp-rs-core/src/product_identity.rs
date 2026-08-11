//! Canonical runtime identity packets for the shipped server and debug adapter.
//!
//! The packet deliberately separates product, executable, build, artifact, and
//! compatibility identity. A matching semantic version is not sufficient to
//! establish source, target, artifact, or candidate parity.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Version of the runtime identity packet schema.
pub const BINARY_IDENTITY_SCHEMA_V1: &str = "perl_lsp.binary_identity.v1";
/// Canonical product name.
pub const PRODUCT_NAME: &str = "perl-lsp";
/// Canonical public repository.
pub const PUBLIC_REPOSITORY: &str = "EffortlessMetrics/perl-lsp";
/// Canonical development repository.
pub const DEVELOPMENT_REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";
/// Version of the product-identity contract understood by this packet.
pub const PRODUCT_IDENTITY_VERSION: u32 = 1;

/// Shipped executable role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryRole {
    /// Language-server process.
    Server,
    /// Debug-adapter process.
    Dap,
}

/// Strength of the embedded build identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildIdentityState {
    /// Source revision and target are both embedded.
    Exact,
    /// Some build identity is present, but one or more load-bearing fields are absent.
    Partial,
    /// No authoritative build identity is available.
    NotProven,
}

/// How the executable entered the current installation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    /// Installed by the managed editor path.
    Managed,
    /// Selected explicitly by the user.
    UserSupplied,
    /// Installed from a package registry.
    PackageInstall,
    /// Extracted from a release archive.
    Archive,
    /// Installation role is unavailable.
    Unknown,
}

/// Product-level identity shared by every shipped executable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductIdentity {
    /// Canonical product name.
    pub name: String,
    /// Public release-lineage repository.
    pub public_repository: String,
    /// Active development repository.
    pub development_repository: String,
}

/// Identity of the process emitting the packet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutableIdentity {
    /// Executable name.
    pub executable: String,
    /// Cargo package that builds the executable.
    pub cargo_package: String,
    /// Product role of the executable.
    pub role: BinaryRole,
    /// Semantic package version.
    pub version: String,
}

/// Build-time identity embedded into the executable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BuildIdentity {
    /// Source revision, when injected by the reviewed build path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    /// Source-tree digest, when injected by the reviewed build path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_tree_digest: Option<String>,
    /// Compiled target triple.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Cargo build profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Strength of the available build evidence.
    pub identity_state: BuildIdentityState,
}

/// Artifact and installation identity supplied by a trusted packager or installer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactIdentity {
    /// Installation role.
    pub role: ArtifactRole,
    /// Digest of the exact executable artifact, when externally bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Opaque release-candidate identity, when externally bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_identity: Option<String>,
}

/// Compatibility facts needed by protocol and installed-product consumers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityIdentity {
    /// Product-identity contract version expected by this packet.
    pub expected_product_identity_version: u32,
    /// Current public DAP posture.
    pub dap_posture: String,
}

/// Complete canonical runtime identity packet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BinaryIdentityPacketV1 {
    /// Packet schema identifier.
    pub schema_version: String,
    /// Canonical product identity.
    pub product: ProductIdentity,
    /// Emitting executable identity.
    pub binary: ExecutableIdentity,
    /// Embedded build identity.
    pub build: BuildIdentity,
    /// Externally bound artifact identity.
    pub artifact: ArtifactIdentity,
    /// Compatibility contract.
    pub compatibility: CompatibilityIdentity,
    /// Explicit limitations on the packet's authority.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

/// Explicit construction inputs used by tests and trusted packaging adapters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BinaryIdentityInput {
    /// Source revision embedded by the build.
    pub source_revision: Option<String>,
    /// Source-tree digest embedded by the build.
    pub source_tree_digest: Option<String>,
    /// Target triple embedded by the build.
    pub target: Option<String>,
    /// Cargo profile embedded by the build.
    pub profile: Option<String>,
    /// Installation role supplied by a trusted caller.
    pub artifact_role: Option<ArtifactRole>,
    /// Artifact digest supplied by a trusted caller.
    pub artifact_digest: Option<String>,
    /// Candidate identity supplied by a trusted caller.
    pub candidate_identity: Option<String>,
}

/// Requested command-line identity projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityOutputFormat {
    /// Stable human-readable projection.
    Human,
    /// Deterministic JSON projection.
    Json,
}

impl BinaryIdentityPacketV1 {
    /// Build a server identity packet from explicit inputs.
    #[must_use]
    pub fn server(version: impl Into<String>, input: BinaryIdentityInput) -> Self {
        Self::new("perllsp", "perllsp", BinaryRole::Server, version.into(), input)
    }

    /// Build a DAP identity packet from explicit inputs.
    #[must_use]
    pub fn dap(version: impl Into<String>, input: BinaryIdentityInput) -> Self {
        Self::new("perl-dap", "perl-dap", BinaryRole::Dap, version.into(), input)
    }

    /// Build a packet from compile-time inputs available to ordinary workspace builds.
    #[must_use]
    pub fn embedded_server(version: impl Into<String>) -> Self {
        Self::server(version, embedded_build_input())
    }

    /// Build a DAP packet from compile-time inputs available to ordinary workspace builds.
    #[must_use]
    pub fn embedded_dap(version: impl Into<String>) -> Self {
        Self::dap(version, embedded_build_input())
    }

    fn new(
        executable: &str,
        cargo_package: &str,
        role: BinaryRole,
        version: String,
        input: BinaryIdentityInput,
    ) -> Self {
        let identity_state = build_identity_state(&input);
        let mut limitations = Vec::new();
        if input.source_revision.is_none() {
            limitations.push("source_revision_not_embedded".to_owned());
        }
        if input.target.is_none() {
            limitations.push("target_triple_not_embedded".to_owned());
        }
        if input.artifact_digest.is_none() {
            limitations.push("artifact_digest_not_externally_bound".to_owned());
        }

        Self {
            schema_version: BINARY_IDENTITY_SCHEMA_V1.to_owned(),
            product: ProductIdentity {
                name: PRODUCT_NAME.to_owned(),
                public_repository: PUBLIC_REPOSITORY.to_owned(),
                development_repository: DEVELOPMENT_REPOSITORY.to_owned(),
            },
            binary: ExecutableIdentity {
                executable: executable.to_owned(),
                cargo_package: cargo_package.to_owned(),
                role,
                version,
            },
            build: BuildIdentity {
                source_revision: input.source_revision,
                source_tree_digest: input.source_tree_digest,
                target: input.target,
                profile: input.profile,
                identity_state,
            },
            artifact: ArtifactIdentity {
                role: input.artifact_role.unwrap_or(ArtifactRole::Unknown),
                digest: input.artifact_digest,
                candidate_identity: input.candidate_identity,
            },
            compatibility: CompatibilityIdentity {
                expected_product_identity_version: PRODUCT_IDENTITY_VERSION,
                dap_posture: "preview".to_owned(),
            },
            limitations,
        }
    }

    /// Serialize the packet as deterministic, pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the packet cannot be encoded.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Render the stable human-readable projection from the same packet.
    #[must_use]
    pub fn to_human(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "Product: {}", self.product.name);
        let _ = writeln!(output, "Executable: {}", self.binary.executable);
        let _ = writeln!(output, "Cargo package: {}", self.binary.cargo_package);
        let _ = writeln!(output, "Role: {}", role_name(self.binary.role));
        let _ = writeln!(output, "Version: {}", self.binary.version);
        let _ = writeln!(
            output,
            "Source revision: {}",
            self.build.source_revision.as_deref().unwrap_or("not proven")
        );
        let _ = writeln!(
            output,
            "Target: {}",
            self.build.target.as_deref().unwrap_or("not proven")
        );
        let _ = writeln!(output, "Build identity: {}", build_state_name(self.build.identity_state));
        let _ = writeln!(output, "Artifact role: {}", artifact_role_name(self.artifact.role));
        let _ = writeln!(output, "DAP posture: {}", self.compatibility.dap_posture);
        output
    }
}

/// Detect a supported one-shot identity request without replacing the ordinary CLI parser.
///
/// Mixed identity and operational arguments are deliberately rejected so a
/// one-shot query cannot silently replace a requested server or DAP session.
#[must_use]
pub fn requested_identity_output(args: &[String]) -> Option<IdentityOutputFormat> {
    let operands = args.get(1..).unwrap_or_default();
    match operands {
        [flag] if flag == "--identity" => Some(IdentityOutputFormat::Human),
        [flag] if flag == "--identity-json" => Some(IdentityOutputFormat::Json),
        [first, second]
            if (first == "--info" && second == "--json")
                || (first == "--json" && second == "--info") =>
        {
            Some(IdentityOutputFormat::Json)
        }
        _ => None,
    }
}

fn embedded_build_input() -> BinaryIdentityInput {
    BinaryIdentityInput {
        source_revision: option_env!("PERL_LSP_BUILD_REVISION").map(str::to_owned),
        source_tree_digest: option_env!("PERL_LSP_SOURCE_TREE_DIGEST").map(str::to_owned),
        target: option_env!("PERL_LSP_TARGET_TRIPLE").map(str::to_owned),
        profile: option_env!("PERL_LSP_BUILD_PROFILE").map(str::to_owned),
        artifact_role: embedded_artifact_role(),
        artifact_digest: option_env!("PERL_LSP_ARTIFACT_SHA256").map(str::to_owned),
        candidate_identity: option_env!("PERL_LSP_CANDIDATE_ID").map(str::to_owned),
    }
}

fn embedded_artifact_role() -> Option<ArtifactRole> {
    match option_env!("PERL_LSP_ARTIFACT_ROLE") {
        Some("managed") => Some(ArtifactRole::Managed),
        Some("user_supplied") => Some(ArtifactRole::UserSupplied),
        Some("package_install") => Some(ArtifactRole::PackageInstall),
        Some("archive") => Some(ArtifactRole::Archive),
        _ => None,
    }
}

fn build_identity_state(input: &BinaryIdentityInput) -> BuildIdentityState {
    match (&input.source_revision, &input.target) {
        (Some(_), Some(_)) => BuildIdentityState::Exact,
        (None, None) => BuildIdentityState::NotProven,
        _ => BuildIdentityState::Partial,
    }
}

fn role_name(role: BinaryRole) -> &'static str {
    match role {
        BinaryRole::Server => "server",
        BinaryRole::Dap => "dap",
    }
}

fn build_state_name(state: BuildIdentityState) -> &'static str {
    match state {
        BuildIdentityState::Exact => "exact",
        BuildIdentityState::Partial => "partial",
        BuildIdentityState::NotProven => "not proven",
    }
}

fn artifact_role_name(role: ArtifactRole) -> &'static str {
    match role {
        ArtifactRole::Managed => "managed",
        ArtifactRole::UserSupplied => "user supplied",
        ArtifactRole::PackageInstall => "package install",
        ArtifactRole::Archive => "archive",
        ArtifactRole::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1, BinaryRole, BuildIdentityState,
        IdentityOutputFormat, requested_identity_output,
    };

    #[test]
    fn server_and_dap_roles_are_not_interchangeable() {
        let server = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
        let dap = BinaryIdentityPacketV1::dap("0.18.0", BinaryIdentityInput::default());

        assert_eq!(server.binary.role, BinaryRole::Server);
        assert_eq!(server.binary.executable, "perllsp");
        assert_eq!(dap.binary.role, BinaryRole::Dap);
        assert_eq!(dap.binary.executable, "perl-dap");
        assert_ne!(server.binary, dap.binary);
    }

    #[test]
    fn exact_identity_requires_source_revision_and_target() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some("abc123".to_owned()),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
                artifact_role: Some(ArtifactRole::Archive),
                artifact_digest: Some("sha256:deadbeef".to_owned()),
                ..BinaryIdentityInput::default()
            },
        );

        assert_eq!(packet.build.identity_state, BuildIdentityState::Exact);
        assert_eq!(packet.artifact.role, ArtifactRole::Archive);
        assert!(!packet.limitations.iter().any(|value| value == "source_revision_not_embedded"));
    }

    #[test]
    fn workspace_identity_is_honestly_not_proven() {
        let packet = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
        assert_eq!(packet.build.identity_state, BuildIdentityState::NotProven);
        assert!(packet.limitations.iter().any(|value| value == "source_revision_not_embedded"));
        assert!(packet.limitations.iter().any(|value| value == "target_triple_not_embedded"));
    }

    #[test]
    fn json_is_deterministic_for_identical_inputs() -> Result<(), serde_json::Error> {
        let packet = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
        let first = packet.to_json()?;
        let second = packet.to_json()?;
        assert_eq!(first, second);
        assert!(first.contains("perl_lsp.binary_identity.v1"));
        Ok(())
    }

    #[test]
    fn identity_flags_do_not_capture_ordinary_or_mixed_operations() {
        let ordinary = vec!["perllsp".to_owned(), "--info".to_owned()];
        let json = vec!["perllsp".to_owned(), "--info".to_owned(), "--json".to_owned()];
        let reversed_json = vec!["perllsp".to_owned(), "--json".to_owned(), "--info".to_owned()];
        let human = vec!["perllsp".to_owned(), "--identity".to_owned()];
        let mixed_server = vec!["perllsp".to_owned(), "--stdio".to_owned(), "--identity".to_owned()];
        let mixed_dap = vec![
            "perl-dap".to_owned(),
            "--external-peer".to_owned(),
            "127.0.0.1:5000".to_owned(),
            "--info".to_owned(),
            "--json".to_owned(),
        ];

        assert_eq!(requested_identity_output(&ordinary), None);
        assert_eq!(requested_identity_output(&json), Some(IdentityOutputFormat::Json));
        assert_eq!(requested_identity_output(&reversed_json), Some(IdentityOutputFormat::Json));
        assert_eq!(requested_identity_output(&human), Some(IdentityOutputFormat::Human));
        assert_eq!(requested_identity_output(&mixed_server), None);
        assert_eq!(requested_identity_output(&mixed_dap), None);
    }
}
