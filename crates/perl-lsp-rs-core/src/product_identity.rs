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

const GIT_REVISION_HEX_LEN: usize = 40;
const SHA256_HEX_LEN: usize = 64;
const MAX_VERSION_LEN: usize = 64;
const MAX_TARGET_LEN: usize = 128;
const MAX_PROFILE_LEN: usize = 64;
const MAX_CANDIDATE_LEN: usize = 128;

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
    /// Source revision, tree digest, target, and profile are all valid and embedded.
    Exact,
    /// Some build identity is present, but one or more load-bearing fields are absent.
    Partial,
    /// No authoritative build identity is available, or supplied build identity is malformed.
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Artifact and installation identity supplied by a trusted external observer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct CompatibilityIdentity {
    /// Product-identity contract version expected by this packet.
    pub expected_product_identity_version: u32,
    /// Current public DAP posture.
    pub dap_posture: String,
}

/// Complete canonical runtime identity packet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
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

/// Explicit construction inputs used by tests and trusted external adapters.
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
    /// Artifact digest supplied by an external observer.
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

/// One-shot identity flag spelling.
pub const IDENTITY_FLAG: &str = "--identity";
/// JSON spelling of the one-shot identity flag.
pub const IDENTITY_JSON_FLAG: &str = "--identity-json";

/// How a command line requests the one-shot binary identity surface.
///
/// The surface is resolved before the ordinary CLI parser so a one-shot query
/// never reaches a server or DAP session. Resolving it here — rather than
/// letting an unparsed flag fall through to the parser — is also what keeps a
/// rejected mix from being reported as an unknown option.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityRequest {
    /// No one-shot identity request is present; use the ordinary CLI parser.
    None,
    /// A supported one-shot identity output was requested.
    Output(IdentityOutputFormat),
    /// An identity surface was requested alongside other flags.
    MixedOperands {
        /// The identity spelling that cannot be honored in this position.
        flag: String,
    },
}

impl IdentityRequest {
    /// Operator-facing message for a rejected one-shot identity mix.
    ///
    /// Returns `None` for a satisfied or absent identity request, so a caller
    /// can reject a mix before falling through to the ordinary parser.
    #[must_use]
    pub fn rejection_message(&self) -> Option<String> {
        match self {
            Self::MixedOperands { flag } => Some(format!(
                "`{flag}` is a one-shot identity query and must be the only argument.\n\
                 Drop the other flags, or run `perllsp --help` for the supported identity forms."
            )),
            Self::None | Self::Output(_) => None,
        }
    }
}

#[derive(Debug)]
struct NormalizedIdentityInput {
    input: BinaryIdentityInput,
    limitations: Vec<String>,
    malformed_build_identity: bool,
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
        let mut normalized = normalize_input(input);
        let version = normalize_version(version, &mut normalized.limitations);
        let identity_state = build_identity_state(&normalized);
        normalized.limitations.sort();
        normalized.limitations.dedup();

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
                source_revision: normalized.input.source_revision,
                source_tree_digest: normalized.input.source_tree_digest,
                target: normalized.input.target,
                profile: normalized.input.profile,
                identity_state,
            },
            artifact: ArtifactIdentity {
                role: normalized.input.artifact_role.unwrap_or(ArtifactRole::Unknown),
                digest: normalized.input.artifact_digest,
                candidate_identity: normalized.input.candidate_identity,
            },
            compatibility: CompatibilityIdentity {
                expected_product_identity_version: PRODUCT_IDENTITY_VERSION,
                dap_posture: "preview".to_owned(),
            },
            limitations: normalized.limitations,
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
        let _ = writeln!(output, "Public repository: {}", self.product.public_repository);
        let _ = writeln!(output, "Development repository: {}", self.product.development_repository);
        let _ = writeln!(output, "Executable: {}", self.binary.executable);
        let _ = writeln!(output, "Cargo package: {}", self.binary.cargo_package);
        let _ = writeln!(output, "Role: {}", role_name(self.binary.role));
        let _ = writeln!(output, "Version: {}", self.binary.version);
        let _ = writeln!(
            output,
            "Source revision: {}",
            display_optional(self.build.source_revision.as_deref())
        );
        let _ = writeln!(
            output,
            "Source tree digest: {}",
            display_optional(self.build.source_tree_digest.as_deref())
        );
        let _ = writeln!(output, "Target: {}", display_optional(self.build.target.as_deref()));
        let _ =
            writeln!(output, "Build profile: {}", display_optional(self.build.profile.as_deref()));
        let _ = writeln!(output, "Build identity: {}", build_state_name(self.build.identity_state));
        let _ = writeln!(output, "Artifact role: {}", artifact_role_name(self.artifact.role));
        let _ = writeln!(
            output,
            "Artifact digest: {}",
            display_optional(self.artifact.digest.as_deref())
        );
        let _ = writeln!(
            output,
            "Candidate identity: {}",
            display_optional(self.artifact.candidate_identity.as_deref())
        );
        let _ = writeln!(output, "Artifact identity: {}", artifact_identity_name(self));
        let _ = writeln!(output, "DAP posture: {}", self.compatibility.dap_posture);
        if self.limitations.is_empty() {
            let _ = writeln!(output, "Limitations: none");
        } else {
            let _ = writeln!(output, "Limitations:");
            for limitation in &self.limitations {
                let _ = writeln!(output, "- {limitation}");
            }
        }
        output
    }
}

/// Detect a supported one-shot identity request without replacing the ordinary CLI parser.
///
/// Mixed identity and operational arguments are deliberately rejected so a
/// one-shot query cannot silently replace a requested server or DAP session.
/// The rejection is reported rather than inferred: a mix that fell through to
/// the ordinary parser would either deny a real flag as unknown, or answer a
/// requested JSON packet with human text.
#[must_use]
pub fn requested_identity(args: &[String]) -> IdentityRequest {
    let operands = args.get(1..).unwrap_or_default();

    // A one-shot form is honored only when it is the *entire* command line, so
    // the match reads the full operand list. Classifying a shortened prefix
    // instead would answer `perllsp --identity -- file.pm` as a clean query
    // while silently dropping the file the caller also asked about.
    match operands {
        [flag] if flag == IDENTITY_FLAG => IdentityRequest::Output(IdentityOutputFormat::Human),
        [flag] if flag == IDENTITY_JSON_FLAG => IdentityRequest::Output(IdentityOutputFormat::Json),
        [first, second]
            if (first == "--info" && second == "--json")
                || (first == "--json" && second == "--info") =>
        {
            IdentityRequest::Output(IdentityOutputFormat::Json)
        }
        _ => rejected_mixed_identity(operands),
    }
}

/// Name the identity surface that a mixed command line cannot honor.
fn rejected_mixed_identity(operands: &[String]) -> IdentityRequest {
    // Operands past a bare `--` are positional file paths, not flags, so
    // `perllsp --check -- --identity` names a file and must not be read as a
    // request for the identity surface. The terminator bounds only what may be
    // read as a flag; the operands still counted as extras above.
    let flag_region = match operands.iter().position(|token| token == "--") {
        Some(terminator) => &operands[..terminator],
        None => operands,
    };

    // The one-shot identity flags are this binary's own spellings, so finding
    // one anywhere in the flag region is a mix whatever else is present —
    // including the value half of a value-taking flag such as `--port 9257`.
    if let Some(flag) =
        flag_region.iter().find(|token| *token == IDENTITY_FLAG || *token == IDENTITY_JSON_FLAG)
    {
        return IdentityRequest::MixedOperands { flag: flag.clone() };
    }

    // `--json` on its own is the `--doctor` output switch, so the composed form
    // only conflicts when `--info` is present too. Every other operand then
    // makes it a mix, including the value of a value-taking option: answering
    // `perllsp --info --json --feature-profile prod` with the human `--info`
    // projection is the silent substitution this surface must not perform.
    if flag_region.contains(&"--info".to_owned()) && flag_region.contains(&"--json".to_owned()) {
        return IdentityRequest::MixedOperands { flag: "--info --json".to_owned() };
    }

    IdentityRequest::None
}

fn embedded_build_input() -> BinaryIdentityInput {
    BinaryIdentityInput {
        source_revision: embedded_string(option_env!("PERL_LSP_BUILD_REVISION")),
        source_tree_digest: embedded_string(option_env!("PERL_LSP_SOURCE_TREE_DIGEST")),
        target: embedded_string(option_env!("PERL_LSP_TARGET_TRIPLE")),
        profile: embedded_string(option_env!("PERL_LSP_BUILD_PROFILE")),
        artifact_role: embedded_artifact_role(),
        // The final executable digest cannot truthfully be embedded into the
        // executable whose bytes it measures. Installed/release observers bind
        // this field after hashing the staged artifact.
        artifact_digest: None,
        candidate_identity: embedded_string(option_env!("PERL_LSP_CANDIDATE_ID")),
    }
}

fn embedded_string(value: Option<&str>) -> Option<String> {
    value.map(str::to_owned)
}

fn embedded_artifact_role() -> Option<ArtifactRole> {
    match option_env!("PERL_LSP_ARTIFACT_ROLE").map(str::trim) {
        Some("managed") => Some(ArtifactRole::Managed),
        Some("user_supplied") => Some(ArtifactRole::UserSupplied),
        Some("package_install") => Some(ArtifactRole::PackageInstall),
        Some("archive") => Some(ArtifactRole::Archive),
        _ => None,
    }
}

fn normalize_input(input: BinaryIdentityInput) -> NormalizedIdentityInput {
    let mut limitations = Vec::new();
    let (source_revision, source_revision_invalid) = normalize_git_revision(
        input.source_revision,
        "source_revision_not_embedded",
        "source_revision_invalid",
        &mut limitations,
    );
    let (source_tree_digest, source_tree_digest_invalid) = normalize_sha256(
        input.source_tree_digest,
        "source_tree_digest_not_embedded",
        "source_tree_digest_invalid",
        &mut limitations,
    );
    let (target, target_invalid) = normalize_token(
        input.target,
        MAX_TARGET_LEN,
        valid_target_character,
        true,
        "target_triple_not_embedded",
        "target_triple_invalid",
        &mut limitations,
    );
    let (profile, profile_invalid) = normalize_token(
        input.profile,
        MAX_PROFILE_LEN,
        valid_profile_character,
        false,
        "build_profile_not_embedded",
        "build_profile_invalid",
        &mut limitations,
    );
    let (artifact_digest, _) = normalize_sha256(
        input.artifact_digest,
        "artifact_digest_not_externally_bound",
        "artifact_digest_invalid",
        &mut limitations,
    );
    let (candidate_identity, _) = normalize_token(
        input.candidate_identity,
        MAX_CANDIDATE_LEN,
        valid_candidate_character,
        false,
        "candidate_identity_not_externally_bound",
        "candidate_identity_invalid",
        &mut limitations,
    );
    let artifact_role = match input.artifact_role {
        Some(ArtifactRole::Unknown) | None => {
            limitations.push("artifact_role_not_proven".to_owned());
            None
        }
        Some(role) => Some(role),
    };

    NormalizedIdentityInput {
        input: BinaryIdentityInput {
            source_revision,
            source_tree_digest,
            target,
            profile,
            artifact_role,
            artifact_digest,
            candidate_identity,
        },
        limitations,
        malformed_build_identity: source_revision_invalid
            || source_tree_digest_invalid
            || target_invalid
            || profile_invalid,
    }
}

fn normalize_version(value: String, limitations: &mut Vec<String>) -> String {
    let normalized = value.trim();
    if valid_bounded_value(normalized, MAX_VERSION_LEN, valid_version_character)
        && normalized.chars().any(|character| character.is_ascii_digit())
    {
        normalized.to_owned()
    } else {
        limitations.push("binary_version_invalid".to_owned());
        "not_proven".to_owned()
    }
}

fn normalize_git_revision(
    value: Option<String>,
    missing_reason: &str,
    invalid_reason: &str,
    limitations: &mut Vec<String>,
) -> (Option<String>, bool) {
    normalize_field(value, missing_reason, invalid_reason, limitations, |item| {
        item.len() == GIT_REVISION_HEX_LEN && item.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn normalize_sha256(
    value: Option<String>,
    missing_reason: &str,
    invalid_reason: &str,
    limitations: &mut Vec<String>,
) -> (Option<String>, bool) {
    let (normalized, invalid) =
        normalize_field(value, missing_reason, invalid_reason, limitations, |item| {
            let digest = item.strip_prefix("sha256:").unwrap_or(item);
            digest.len() == SHA256_HEX_LEN && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    let normalized =
        normalized.map(|item| item.strip_prefix("sha256:").unwrap_or(&item).to_ascii_lowercase());
    (normalized, invalid)
}

fn normalize_token(
    value: Option<String>,
    max_len: usize,
    valid_character: fn(char) -> bool,
    require_dash: bool,
    missing_reason: &str,
    invalid_reason: &str,
    limitations: &mut Vec<String>,
) -> (Option<String>, bool) {
    normalize_field(value, missing_reason, invalid_reason, limitations, |item| {
        valid_bounded_value(item, max_len, valid_character) && (!require_dash || item.contains('-'))
    })
}

fn normalize_field(
    value: Option<String>,
    missing_reason: &str,
    invalid_reason: &str,
    limitations: &mut Vec<String>,
    validator: impl FnOnce(&str) -> bool,
) -> (Option<String>, bool) {
    match value {
        None => {
            limitations.push(missing_reason.to_owned());
            (None, false)
        }
        Some(value) => {
            let normalized = value.trim();
            if validator(normalized) {
                (Some(normalized.to_owned()), false)
            } else {
                limitations.push(invalid_reason.to_owned());
                (None, true)
            }
        }
    }
}

fn valid_bounded_value(value: &str, max_len: usize, valid_character: fn(char) -> bool) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.is_ascii()
        && value.chars().all(valid_character)
}

fn valid_version_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
}

fn valid_target_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn valid_profile_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn valid_candidate_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '@' | '+')
}

fn build_identity_state(input: &NormalizedIdentityInput) -> BuildIdentityState {
    if input.malformed_build_identity {
        return BuildIdentityState::NotProven;
    }
    match (
        &input.input.source_revision,
        &input.input.source_tree_digest,
        &input.input.target,
        &input.input.profile,
    ) {
        (Some(_), Some(_), Some(_), Some(_)) => BuildIdentityState::Exact,
        (None, None, None, None) => BuildIdentityState::NotProven,
        _ => BuildIdentityState::Partial,
    }
}

fn display_optional(value: Option<&str>) -> &str {
    value.unwrap_or("not proven")
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

fn artifact_identity_name(packet: &BinaryIdentityPacketV1) -> &'static str {
    if packet.artifact.role != ArtifactRole::Unknown
        && packet.artifact.digest.is_some()
        && packet.artifact.candidate_identity.is_some()
    {
        "externally bound"
    } else {
        "partial or not proven"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1, BinaryRole, BuildIdentityState,
        IDENTITY_FLAG, IDENTITY_JSON_FLAG, IdentityOutputFormat, IdentityRequest,
        requested_identity,
    };
    use perl_tdd_support::must_some_with;

    fn revision(character: char) -> String {
        character.to_string().repeat(40)
    }

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

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
    fn exact_identity_requires_valid_build_and_artifact_inputs() {
        let expected_revision = revision('a');
        let expected_tree_digest = digest('b');
        let expected_artifact_digest = digest('c');
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some(expected_revision.clone()),
                source_tree_digest: Some(expected_tree_digest.clone()),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
                profile: Some("release".to_owned()),
                artifact_role: Some(ArtifactRole::Archive),
                artifact_digest: Some(format!("sha256:{expected_artifact_digest}")),
                candidate_identity: Some("rc1".to_owned()),
            },
        );

        assert_eq!(packet.build.identity_state, BuildIdentityState::Exact);
        assert_eq!(packet.build.source_revision.as_deref(), Some(expected_revision.as_str()));
        assert_eq!(packet.build.source_tree_digest.as_deref(), Some(expected_tree_digest.as_str()));
        assert_eq!(packet.artifact.digest.as_deref(), Some(expected_artifact_digest.as_str()));
        assert_eq!(packet.artifact.role, ArtifactRole::Archive);
        assert!(packet.limitations.is_empty(), "limitations={:?}", packet.limitations);
    }

    #[test]
    fn blank_build_inputs_cannot_claim_exact_identity() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some("  ".to_owned()),
                target: Some(String::new()),
                ..BinaryIdentityInput::default()
            },
        );

        assert_eq!(packet.build.identity_state, BuildIdentityState::NotProven);
        assert_eq!(packet.build.source_revision, None);
        assert_eq!(packet.build.target, None);
        assert!(packet.limitations.iter().any(|value| value == "source_revision_invalid"));
        assert!(packet.limitations.iter().any(|value| value == "target_triple_invalid"));
    }

    #[test]
    fn malformed_identity_inputs_are_omitted_and_not_authoritative() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0\nforged",
            BinaryIdentityInput {
                source_revision: Some("abc\nTarget: forged".to_owned()),
                source_tree_digest: Some("f".repeat(63)),
                target: Some("/home/user/custom-target.json".to_owned()),
                profile: Some("release\nforged".to_owned()),
                artifact_role: Some(ArtifactRole::Archive),
                artifact_digest: Some("sha256:deadbeef".to_owned()),
                candidate_identity: Some("../../private".to_owned()),
            },
        );

        assert_eq!(packet.binary.version, "not_proven");
        assert_eq!(packet.build.identity_state, BuildIdentityState::NotProven);
        assert_eq!(packet.build.source_revision, None);
        assert_eq!(packet.build.source_tree_digest, None);
        assert_eq!(packet.build.target, None);
        assert_eq!(packet.build.profile, None);
        assert_eq!(packet.artifact.digest, None);
        assert_eq!(packet.artifact.candidate_identity, None);
        for limitation in [
            "artifact_digest_invalid",
            "binary_version_invalid",
            "build_profile_invalid",
            "candidate_identity_invalid",
            "source_revision_invalid",
            "source_tree_digest_invalid",
            "target_triple_invalid",
        ] {
            assert!(
                packet.limitations.iter().any(|value| value == limitation),
                "missing {limitation}; limitations={:?}",
                packet.limitations
            );
        }
        let human = packet.to_human();
        assert!(!human.contains("forged"), "human={human}");
        assert!(!human.contains("/home/user"), "human={human}");
        assert!(!human.contains("../../private"), "human={human}");
    }

    #[test]
    fn wrong_length_revision_is_not_exact() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some("a".repeat(39)),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
                ..BinaryIdentityInput::default()
            },
        );
        assert_eq!(packet.build.identity_state, BuildIdentityState::NotProven);
        assert_eq!(packet.build.source_revision, None);
        assert!(packet.limitations.iter().any(|value| value == "source_revision_invalid"));
    }

    #[test]
    fn explicit_unknown_artifact_role_is_honestly_not_proven() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                artifact_role: Some(ArtifactRole::Unknown),
                artifact_digest: Some(digest('a')),
                candidate_identity: Some("rc1".to_owned()),
                ..BinaryIdentityInput::default()
            },
        );
        assert_eq!(packet.artifact.role, ArtifactRole::Unknown);
        assert!(packet.limitations.iter().any(|v| v == "artifact_role_not_proven"));
        assert!(packet.to_human().contains("Artifact identity: partial or not proven"));
    }

    #[test]
    fn workspace_identity_is_honestly_not_proven() {
        let packet = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
        assert_eq!(packet.build.identity_state, BuildIdentityState::NotProven);
        assert!(packet.limitations.iter().any(|value| value == "source_revision_not_embedded"));
        assert!(packet.limitations.iter().any(|value| value == "target_triple_not_embedded"));
        assert_eq!(packet.artifact.digest, None);
    }

    #[test]
    fn human_projection_exposes_every_parity_dimension_and_limitation() {
        let packet = BinaryIdentityPacketV1::server(
            "0.18.0",
            BinaryIdentityInput {
                source_revision: Some(revision('a')),
                target: Some("x86_64-unknown-linux-gnu".to_owned()),
                artifact_role: Some(ArtifactRole::Archive),
                ..BinaryIdentityInput::default()
            },
        );
        let human = packet.to_human();
        for expected in [
            "Source tree digest: not proven",
            "Build profile: not proven",
            "Artifact digest: not proven",
            "Candidate identity: not proven",
            "Artifact identity: partial or not proven",
            "Limitations:",
            "- artifact_digest_not_externally_bound",
            "- candidate_identity_not_externally_bound",
            "- source_tree_digest_not_embedded",
        ] {
            assert!(human.contains(expected), "missing {expected:?}; human={human}");
        }
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
        let human = vec!["perllsp".to_owned(), IDENTITY_FLAG.to_owned()];
        let mixed_server =
            vec!["perllsp".to_owned(), "--stdio".to_owned(), IDENTITY_FLAG.to_owned()];
        let terminated = vec![
            "perllsp".to_owned(),
            "--check".to_owned(),
            "--".to_owned(),
            IDENTITY_FLAG.to_owned(),
        ];
        let mixed_dap = vec![
            "perl-dap".to_owned(),
            "--external-peer".to_owned(),
            "127.0.0.1:5000".to_owned(),
            "--info".to_owned(),
            "--json".to_owned(),
        ];

        assert_eq!(requested_identity(&ordinary), IdentityRequest::None);
        assert_eq!(requested_identity(&json), IdentityRequest::Output(IdentityOutputFormat::Json));
        assert_eq!(
            requested_identity(&reversed_json),
            IdentityRequest::Output(IdentityOutputFormat::Json)
        );
        assert_eq!(
            requested_identity(&human),
            IdentityRequest::Output(IdentityOutputFormat::Human)
        );
        // A mix is reported to the caller rather than captured: the one-shot
        // surface must not answer a command line that also asked for a server.
        assert_eq!(
            requested_identity(&mixed_server),
            IdentityRequest::MixedOperands { flag: IDENTITY_FLAG.to_owned() }
        );
        assert_eq!(requested_identity(&terminated), IdentityRequest::None);
        // `perl-dap` has no `--info` option at all, so this shape is already
        // invalid for that binary; the resolver now rejects it with the mix
        // message instead of letting clap report the unknown flag. Either way
        // no session starts, and the rejection is the more specific message.
        assert_eq!(
            requested_identity(&mixed_dap),
            IdentityRequest::MixedOperands { flag: "--info --json".to_owned() }
        );
    }

    /// A near-miss must not deny that a real, supported flag exists.
    #[test]
    fn a_mixed_identity_flag_is_rejected_instead_of_denying_the_flag_exists() {
        for extra in [vec!["--log"], vec!["--stdio"], vec!["--port", "9257"], vec!["out.json"]] {
            let mut args = vec!["perllsp".to_owned()];
            args.extend(extra.iter().map(|token| (*token).to_owned()));
            args.push(IDENTITY_FLAG.to_owned());
            let request = requested_identity(&args);

            assert_eq!(
                request,
                IdentityRequest::MixedOperands { flag: IDENTITY_FLAG.to_owned() },
                "{extra:?} must be reported as a rejection, not fall through to the parser"
            );

            let message =
                must_some_with(request.rejection_message(), "a rejected mix must explain");
            assert!(message.contains(IDENTITY_FLAG), "message drops the flag: {message}");
            assert!(
                !message.to_lowercase().contains("unknown option"),
                "message denies a supported flag exists: {message}"
            );
        }
    }

    /// `--info --json` only names the JSON packet when it is the whole command
    /// line. A third operand must not downgrade a requested JSON answer to the
    /// human `--info` projection.
    #[test]
    fn a_json_identity_request_with_a_third_operand_is_rejected_not_downgraded() {
        for extra in ["--log", "--quiet", "--feature-profile=prod"] {
            for prefix in [vec!["--info", "--json"], vec!["--json", "--info"]] {
                let mut args = vec!["perllsp".to_owned()];
                args.extend(prefix.iter().map(|flag| (*flag).to_owned()));
                args.push(extra.to_owned());

                let request = requested_identity(&args);
                assert_eq!(
                    request,
                    IdentityRequest::MixedOperands { flag: "--info --json".to_owned() },
                    "{:?} must be rejected",
                    &args[1..]
                );
                assert!(request.rejection_message().is_some());
            }
        }
    }

    /// Controls for the two rejection arms: neither may fire on a command line
    /// that never asked for the one-shot identity surface.
    #[test]
    fn a_non_identity_command_line_is_never_rejected_as_a_mix() {
        let controls = [
            vec!["perllsp", "--doctor", "--json"],
            vec!["perllsp", "--json", "--doctor"],
            vec!["perllsp", "--json"],
            vec!["perllsp", "--info"],
            vec!["perllsp", "--log"],
            vec!["perllsp", "--doctor", "--external-tools"],
            vec!["perllsp", "--check", "--identity.pm"],
            vec!["perllsp", "--check", "--", "--identity-json"],
            vec!["perllsp", "--stdio", "--port", "9257"],
        ];

        for control in controls {
            let args: Vec<String> = control.iter().map(|token| (*token).to_owned()).collect();
            assert_eq!(
                requested_identity(&args),
                IdentityRequest::None,
                "{control:?} must reach the ordinary parser"
            );
        }
    }

    /// A one-shot form is honored only when it is the whole command line, so
    /// extra operands after a terminator are still extras.
    #[test]
    fn extra_operands_after_a_terminator_still_break_a_one_shot_form() {
        for prefix in [
            vec![IDENTITY_FLAG.to_owned()],
            vec![IDENTITY_JSON_FLAG.to_owned()],
            vec!["--info".to_owned(), "--json".to_owned()],
        ] {
            let mut args = vec!["perllsp".to_owned()];
            args.extend(prefix.iter().cloned());
            args.push("--".to_owned());
            args.push("lib/MyModule.pm".to_owned());

            assert_eq!(
                requested_identity(&args),
                IdentityRequest::MixedOperands {
                    flag: if prefix.len() == 1 {
                        prefix[0].clone()
                    } else {
                        "--info --json".to_owned()
                    }
                },
                "{prefix:?} plus a trailing file must be rejected, not answered as a clean query"
            );
        }
    }

    /// The composed form must not be answered with the human `--info`
    /// projection when a value-taking option is also present.
    #[test]
    fn a_valued_option_beside_the_composed_form_is_rejected_not_downgraded() {
        for valued in [
            vec!["--feature-profile", "prod"],
            vec!["--port", "9257"],
            vec!["--completion", "bash"],
            vec!["--ripr-root", "."],
            vec!["--runtime-mode", "e2e"],
            vec!["prod"],
        ] {
            for prefix in [vec!["--info", "--json"], vec!["--json", "--info"]] {
                let mut args = vec!["perllsp".to_owned()];
                args.extend(prefix.iter().map(|flag| (*flag).to_owned()));
                args.extend(valued.iter().map(|token| (*token).to_owned()));

                assert_eq!(
                    requested_identity(&args),
                    IdentityRequest::MixedOperands { flag: "--info --json".to_owned() },
                    "{valued:?} must be rejected, not answered with human --info"
                );
            }
        }
    }

    /// A satisfied or absent request has nothing to reject.
    #[test]
    fn only_a_rejected_mix_carries_a_message() {
        assert!(
            IdentityRequest::None.rejection_message().is_none(),
            "an absent request has nothing to reject"
        );
        assert!(
            IdentityRequest::Output(IdentityOutputFormat::Json).rejection_message().is_none(),
            "a satisfied request has nothing to reject"
        );
    }
}
