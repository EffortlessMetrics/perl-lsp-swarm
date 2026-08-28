use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The one supported manifest schema version for this boundary.
pub const SOURCE_AUTHORITY_SCHEMA_VERSION: &str = "zed-source-authority.v1";

/// The only accepted external-write policy for Zed stage packets.
///
/// Packet content may never raise a stage's capability: branch, PR, merge,
/// release, registry, and upstream mutations stay maintainer-only manual
/// checkpoints regardless of what any embedded text requests.
pub const EXTERNAL_WRITE_POLICY: &str = "maintainer_manual_checkpoint_only";

/// Authority classes for every piece of text or data entering a stage packet.
///
/// Declaration order is precedence order (strongest first). A class states
/// whether its inputs may direct work and which verification it demands;
/// renderers derive behavior from the class, never from prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAuthorityClass {
    RepositoryPolicy,
    MaintainerRuling,
    CanonicalStageSpec,
    CurrentIssueScope,
    CurrentPrScope,
    VerifiedReviewFinding,
    UnverifiedReviewFinding,
    ExternalReadOnlySubject,
    ToolObservation,
    ReceiptEvidence,
    HistoricalContext,
    UntrustedContent,
    RenderedExternalBody,
}

impl SourceAuthorityClass {
    /// Parse the manifest spelling of this class.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Some(match raw {
            "repository_policy" => Self::RepositoryPolicy,
            "maintainer_ruling" => Self::MaintainerRuling,
            "canonical_stage_spec" => Self::CanonicalStageSpec,
            "current_issue_scope" => Self::CurrentIssueScope,
            "current_pr_scope" => Self::CurrentPrScope,
            "verified_review_finding" => Self::VerifiedReviewFinding,
            "unverified_review_finding" => Self::UnverifiedReviewFinding,
            "external_read_only_subject" => Self::ExternalReadOnlySubject,
            "tool_observation" => Self::ToolObservation,
            "receipt_evidence" => Self::ReceiptEvidence,
            "historical_context" => Self::HistoricalContext,
            "untrusted_content" => Self::UntrustedContent,
            "rendered_external_body" => Self::RenderedExternalBody,
            _ => return None,
        })
    }

    /// Manifest spelling of this class.
    pub fn as_schema_name(self) -> &'static str {
        match self {
            Self::RepositoryPolicy => "repository_policy",
            Self::MaintainerRuling => "maintainer_ruling",
            Self::CanonicalStageSpec => "canonical_stage_spec",
            Self::CurrentIssueScope => "current_issue_scope",
            Self::CurrentPrScope => "current_pr_scope",
            Self::VerifiedReviewFinding => "verified_review_finding",
            Self::UnverifiedReviewFinding => "unverified_review_finding",
            Self::ExternalReadOnlySubject => "external_read_only_subject",
            Self::ToolObservation => "tool_observation",
            Self::ReceiptEvidence => "receipt_evidence",
            Self::HistoricalContext => "historical_context",
            Self::UntrustedContent => "untrusted_content",
            Self::RenderedExternalBody => "rendered_external_body",
        }
    }

    /// Whether this class may change a work package's objective, scope,
    /// external-write policy, claim boundary, or acceptance.
    ///
    /// Only current repository policy and current maintainer rulings direct
    /// work. Every other class is evidence to inspect, whatever its prose
    /// claims.
    pub fn may_direct_work(self) -> bool {
        matches!(self, Self::RepositoryPolicy | Self::MaintainerRuling)
    }

    /// Whether inputs of this class are findings that must be confirmed
    /// against current code before any implementation action is converted.
    pub fn is_review_finding(self) -> bool {
        matches!(self, Self::VerifiedReviewFinding | Self::UnverifiedReviewFinding)
    }

    /// Whether this class names outbound data rendered into an external
    /// submission surface. Such bodies travel as opaque payload: consumers may
    /// render them but never execute them.
    pub fn is_rendered_external_body(self) -> bool {
        matches!(self, Self::RenderedExternalBody)
    }
}

/// Sensitivity handling required for a packet input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    #[default]
    Public,
    RedactRequired,
    MachineLocalForbidden,
}

/// A directive-classified input bound to durable repository authority.
///
/// Directive classification without checkable provenance is rejected: the
/// verifier validates the `ruling_id` shape (`issue#<n>`, `pr#<n>`, or
/// `<existing repo-relative path>#<anchor>`) and that the governed
/// `subject_path` names an existing repository-relative subject, so authority
/// is checkable against the repository rather than asserted by the text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RulingBinding {
    /// Durable ruling identity: `issue#123`, `pr#123`, or a repo-relative
    /// policy document path with an anchor (`docs/policy/x.md#stage-authority`).
    pub ruling_id: String,
    /// Repository-relative subject the ruling governs; must exist.
    pub subject_path: String,
}

/// One classified input to a Zed agent stage packet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketInput {
    /// Stable identifier unique within the manifest.
    pub id: String,
    /// Packet-root-relative subject file this input binds.
    pub subject: String,
    /// Authority class of this input.
    pub authority: SourceAuthorityClass,
    /// SHA-256 over the normalized subject bytes (currentness binding).
    pub digest: String,
    /// Declared instruction capability; must equal the class capability.
    #[serde(default)]
    pub instruction_allowed: bool,
    /// Required sensitivity handling.
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Content is referenced by digest only and never rendered inline.
    #[serde(default)]
    pub digest_only: bool,
    /// Whether this input currently governs the packet.
    #[serde(default = "default_true")]
    pub active: bool,
    /// Input that supersedes this one; a superseded input cannot stay active.
    #[serde(default)]
    pub superseded_by: Option<String>,
    /// Optional key grouping inputs that must agree; same-key active inputs
    /// with different digests are an explicit authority conflict.
    #[serde(default)]
    pub conflict_key: Option<String>,
    /// Review finding confirmed against current code.
    #[serde(default)]
    pub verified_against_current_code: bool,
    /// Finding converted into an implementation action.
    #[serde(default)]
    pub converted_to_action: bool,
    /// Directive provenance; required for directive-class inputs.
    #[serde(default)]
    pub ruling_binding: Option<RulingBinding>,
}

fn default_true() -> bool {
    true
}

/// A generator or consumer path that assembles or checks stage packets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorPath {
    /// Repository-relative script path.
    pub path: String,
}

/// The classified source-authority manifest for one stage-packet tree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAuthorityManifest {
    pub schema_version: String,
    /// Packet-tree root, repository-relative.
    pub packet_root: String,
    /// Must equal [`EXTERNAL_WRITE_POLICY`].
    pub external_write_policy: String,
    /// File name of this manifest inside the packet tree; excluded from the
    /// unclassified-content walk.
    pub manifest_file: String,
    /// Declared generator/consumer scripts addressing the packet tree.
    pub generators: Vec<GeneratorPath>,
    /// Classified inputs covering every file in the packet tree.
    pub inputs: Vec<PacketInput>,
}

/// Normalize raw bytes into the canonical form that digests bind.
///
/// Deterministic across platforms: require UTF-8 and unify CRLF/CR into LF.
/// Everything else binds exactly. Trailing spaces and tabs and trailing blank
/// lines are semantic — two trailing spaces are a Markdown hard break in
/// rendered bodies, and trailing whitespace is part of patch hunk content —
/// so editor whitespace repair must not silently keep an authority digest
/// "current" while the bound bytes changed.
pub fn normalize_content(raw: &[u8]) -> Result<Vec<u8>, std::str::Utf8Error> {
    let text = std::str::from_utf8(raw)?;
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(unified.into_bytes())
}

/// SHA-256 hex digest over the normalized content of raw bytes.
pub fn normalized_digest(raw: &[u8]) -> Result<String, std::str::Utf8Error> {
    use std::fmt::Write as _;
    let normalized = normalize_content(raw)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(&normalized) {
        let _ = write!(digest, "{byte:02x}");
    }
    Ok(digest)
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn only_policy_and_rulings_direct_work() {
        assert!(SourceAuthorityClass::RepositoryPolicy.may_direct_work());
        assert!(SourceAuthorityClass::MaintainerRuling.may_direct_work());
        assert!(!SourceAuthorityClass::CanonicalStageSpec.may_direct_work());
        assert!(!SourceAuthorityClass::UntrustedContent.may_direct_work());
        assert!(!SourceAuthorityClass::RenderedExternalBody.may_direct_work());
    }

    #[test]
    fn declaration_order_is_precedence_order() {
        assert!(SourceAuthorityClass::RepositoryPolicy < SourceAuthorityClass::MaintainerRuling);
        assert!(SourceAuthorityClass::MaintainerRuling < SourceAuthorityClass::CanonicalStageSpec);
        assert!(
            SourceAuthorityClass::VerifiedReviewFinding
                < SourceAuthorityClass::UnverifiedReviewFinding
        );
        assert!(SourceAuthorityClass::ReceiptEvidence < SourceAuthorityClass::HistoricalContext);
        assert!(SourceAuthorityClass::HistoricalContext < SourceAuthorityClass::UntrustedContent);
    }

    #[test]
    fn schema_names_round_trip() {
        let classes = [
            SourceAuthorityClass::RepositoryPolicy,
            SourceAuthorityClass::MaintainerRuling,
            SourceAuthorityClass::CanonicalStageSpec,
            SourceAuthorityClass::CurrentIssueScope,
            SourceAuthorityClass::CurrentPrScope,
            SourceAuthorityClass::VerifiedReviewFinding,
            SourceAuthorityClass::UnverifiedReviewFinding,
            SourceAuthorityClass::ExternalReadOnlySubject,
            SourceAuthorityClass::ToolObservation,
            SourceAuthorityClass::ReceiptEvidence,
            SourceAuthorityClass::HistoricalContext,
            SourceAuthorityClass::UntrustedContent,
            SourceAuthorityClass::RenderedExternalBody,
        ];
        for class in classes {
            assert_eq!(SourceAuthorityClass::parse(class.as_schema_name()), Some(class));
        }
        assert_eq!(SourceAuthorityClass::parse("directive_from_bot"), None);
    }

    #[test]
    fn digests_unify_line_endings_but_bind_trailing_bytes() {
        // Line-ending spellings of one document share a digest.
        let lf = b"# heading\nbody line\n";
        assert_eq!(normalized_digest(b"# heading\r\nbody line\r\n"), normalized_digest(lf));
        assert_eq!(normalized_digest(b"# heading\rbody line\r"), normalized_digest(lf));
        // Trailing whitespace is semantic and must change the digest: two
        // trailing spaces are a Markdown hard break, a trailing tab is patch
        // hunk content.
        assert_ne!(normalized_digest(b"# heading  \nbody line\n"), normalized_digest(lf));
        assert_ne!(normalized_digest(b"# heading\t\n"), normalized_digest(b"# heading\n"));
        // Trailing blank lines are bound as well.
        assert_ne!(normalized_digest(b"# heading\nbody line\n\n"), normalized_digest(lf));
    }

    #[test]
    fn non_utf8_content_is_rejected_not_lossily_digested() {
        assert!(normalized_digest(&[0xff, 0xfe]).is_err());
    }
}
