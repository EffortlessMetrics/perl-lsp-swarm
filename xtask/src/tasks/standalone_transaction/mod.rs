//! Pure standalone install transaction model (#10243 child 1, #11099).
//!
//! Transport-neutral grammar for one immutable standalone install transaction:
//!
//! ```text
//! trusted user/config input
//! → StandaloneInstallIntent            (immutable, may still be unresolved)
//! → ResolvedStandaloneInstallSubject   (one mode-specific exact subject)
//! → StageReceipt                       (subject-bound stage evidence)
//! → validated StageDag fan-in          (composition rules)
//! → TerminalStandaloneInstallOutcome   (one closed terminal result)
//! ```
//!
//! Every type is `#[serde(deny_unknown_fields)]`, collections are order-
//! stable, derived identities are domain-separated SHA-256 digests over
//! canonical (key-sorted) JSON bytes, and every validator fails closed on
//! missing fields, ambiguous selectors, reversed/cyclic stage graphs,
//! predecessor mismatches, mixed subjects/attempts, unauthorized skips,
//! producer-declared completeness lies, and private output leakage.
//!
//! Boundary (#11099): pure model plus independent validators only. Nothing
//! here resolves against live configuration, performs network/checksum/
//! provenance/archive/process/promotion/PATH work, or owns platform
//! mechanics. Until child 2 restricts construction, treating the resolver
//! seam functions ([`resolve_subject`] and [`fallback_branch`]) as the only
//! subject producers is a convention, not an enforced invariant; child 2
//! owns the real bounded resolver and later children own adapters.
//!
//! # Vocabulary authority (#12642 coordination)
//!
//! This module is the typed authority for the standalone transaction
//! grammar: schema-version literals, closed vocabularies, target identity,
//! release selector, product-unit/fallback/path policies, receipt shape,
//! digest domains, and terminal outcome. Sibling PR #12642 defines an
//! overlapping corpus vocabulary with divergent spellings; when both land,
//! #12642's corpus grammar rebases onto these types and constants instead of
//! maintaining parallel spellings, and until that rebase lands #12642's
//! fixtures remain data-only and must not mint competing canonical
//! identities under this module's digest domains.

mod fixtures;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fmt::{Display, Formatter};

// ---------------------------------------------------------------------------
// Schema versions and digest domains
// ---------------------------------------------------------------------------

pub const INTENT_SCHEMA_VERSION: &str = "standalone_install_intent.v1";
pub const SUBJECT_SCHEMA_VERSION: &str = "standalone_install_subject.v1";
pub const RECEIPT_SCHEMA_VERSION: &str = "standalone_stage_receipt.v1";
pub const DAG_SCHEMA_VERSION: &str = "standalone_stage_dag.v1";
pub const OUTCOME_SCHEMA_VERSION: &str = "standalone_terminal_outcome.v1";

const INTENT_DIGEST_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-install-intent.v1\0";
const SUBJECT_DIGEST_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-subject.v1\0";
const RECEIPT_DIGEST_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-stage-receipt.v1\0";
const STAGE_SET_DIGEST_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-stage-set.v1\0";

const MAX_ID_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 512;

// ---------------------------------------------------------------------------
// Closed vocabularies
//
// Each enum emits ALL/as_str/parse so the wire literals stay the single
// authority, every value stays constructed (adapter projections consume the
// full vocabulary), and later generated bindings (#11497) get symmetric
// encode/decode tables instead of inventing spellings.
// ---------------------------------------------------------------------------

macro_rules! closed_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        $(#[$meta])*
        pub enum $name {
            $(
                #[serde(rename = $text)]
                $(#[$vmeta])*
                $variant
            ),+
        }

        impl $name {
            /// Every value in declaration (wire-stable) order. The full
            /// vocabulary stays constructed even where the current route
            /// does not yet use each value: children #11104/#11111/#11497
            /// consume it as a closed adapter surface.
            #[allow(dead_code)]
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// Decode the wire literal; unknown values fail closed.
            #[allow(dead_code)]
            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.as_str() == text)
            }
        }
    };
}

closed_enum!(RouteMode {
    FirstPartyPosix => "first_party_posix",
    FirstPartyPowershell => "first_party_powershell"
});
closed_enum!(InstallOperation {
    Install => "install",
    Repair => "repair",
    Update => "update",
    Rollback => "rollback",
    Uninstall => "uninstall"
});
closed_enum!(InstallMode {
    ReleaseArchive => "release_archive",
    ExactRegistrySource => "exact_registry_source",
    ExplicitLocalDevelopment => "explicit_local_development"
});
closed_enum!(SelectorKind {
    Exact => "exact",
    LatestRequested => "latest_requested",
    NotApplicable => "not_applicable"
});
closed_enum!(Platform {
    Linux => "linux",
    Macos => "macos",
    Windows => "windows"
});
closed_enum!(LibcDisposition {
    Gnu => "gnu",
    Musl => "musl",
    Msvc => "msvc",
    NoneLibc => "none"
});
closed_enum!(ProductUnit {
    ServerOnly => "server_only",
    ServerDapPair => "server_dap_pair"
});
closed_enum!(MemberRole {
    PerllspServer => "perllsp_server",
    PerlDapAdapter => "perl_dap_adapter"
});
closed_enum!(DestinationRole {
    UserLocal => "user_local",
    SystemShared => "system_shared"
});
closed_enum!(PathPolicy {
    Persist => "persist",
    SessionOnly => "session_only"
});
closed_enum!(FallbackPolicy {
    Forbidden => "forbidden",
    ArchiveToSourceAllowed => "archive_to_source_allowed"
});
closed_enum!(ArchiveFormat {
    TarGz => "tar.gz",
    Zip => "zip"
});

// Required standalone stage vocabulary (#10243 body plus #11099's
// `uninstall`; documented delta).
closed_enum!(StageId {
    ResolveSubject => "resolve_subject",
    Transport => "transport",
    ChecksumIntegrity => "checksum_integrity",
    Provenance => "provenance",
    ArchiveManifestAndStaging => "archive_manifest_and_staging",
    ExecutableObservation => "executable_observation",
    SourceBuild => "source_build",
    Promotion => "promotion",
    PathPersistence => "path_persistence",
    FreshProcessObservation => "fresh_process_observation",
    InstalledTransition => "installed_transition",
    Uninstall => "uninstall"
});

// Closed per-receipt result vocabulary. Superset of the #10243 body list:
// `timed_out` is retained from #11099's receipt contract (documented delta).
closed_enum!(StageResult {
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    TimedOut => "timed_out",
    NotProven => "not_proven",
    NotApplicable => "not_applicable"
});

closed_enum!(InstrumentCompleteness {
    Complete => "complete",
    Partial => "partial",
    Unavailable => "unavailable"
});

closed_enum!(RedactionDisposition {
    /// Durable/public receipts carry roles, digests, and bounded display
    /// identities only.
    RedactedRolesOnly => "redacted_roles_only",
    Raw => "raw"
});

closed_enum!(ActionClass {
    None => "none",
    AbortInstall => "abort_install",
    VerifyEnvironmentThenRetry => "verify_environment_then_retry",
    CreateFallbackBranch => "create_fallback_branch",
    RetryNewAttempt => "retry_new_attempt"
});

// Bounded reason vocabulary carried by receipts and terminal outcomes.
closed_enum!(ReasonFamily {
    None => "none",
    TransportFailed => "transport_failed",
    IntegrityFailed => "integrity_failed",
    ProvenanceFailed => "provenance_failed",
    ArchiveInvalid => "archive_invalid",
    ObservationFailed => "observation_failed",
    PairIncomplete => "pair_incomplete",
    HealthCheckFailed => "health_check_failed",
    MissingEvidence => "missing_evidence",
    UnauthorizedNotApplicable => "unauthorized_not_applicable",
    SubjectMismatch => "subject_mismatch",
    PredecessorMismatch => "predecessor_mismatch",
    UnknownSchema => "unknown_schema",
    InstrumentFailure => "instrument_failure",
    NotProven => "not_proven",
    StaleAttempt => "stale_attempt",
    Timeout => "timeout",
    Cancelled => "cancelled",
    AmbiguousSelector => "ambiguous_selector",
    LocalDevelopmentNonAuthoritative => "local_development_non_authoritative",
    FallbackBranchRequired => "fallback_branch_required",
    PrivateOutputLeakage => "private_output_leakage"
});

// What a completed transaction authorizes claiming (#11099 terminal list).
closed_enum!(TerminalResult {
    Installed => "installed",
    Repaired => "repaired",
    Updated => "updated",
    RolledBack => "rolled_back",
    Uninstalled => "uninstalled",
    Failed => "failed",
    Cancelled => "cancelled",
    TimedOut => "timed_out",
    NotProven => "not_proven"
});

// Ordered side-effect ceilings a standalone transaction can reach; derived
// deterministically from the furthest executed stage.
closed_enum!(SideEffectCeiling {
    None => "none",
    ResolveOnly => "resolve_only",
    TransportArtifacts => "transport_artifacts",
    Staged => "staged",
    PromotionReached => "promotion_reached",
    PathPersisted => "path_persisted",
    InstalledClaim => "installed_claim",
    RemovalCompleted => "removal_completed"
});

// Which candidate identity survives the transaction (#11099 terminal
// contract: "current/previous candidate disposition"). Derived from the
// validated receipt chain: a green install/repair/update confirms the newly
// promoted current candidate; a green rollback restores the previous
// complete candidate; a green uninstall leaves no candidate installed;
// terminal evidence before the installed transition leaves the disposition
// unresolved rather than guessed.
closed_enum!(CandidateDisposition {
    Unresolved => "unresolved",
    CurrentConfirmed => "current_confirmed",
    PreviousRestored => "previous_restored",
    NoneRemaining => "none_remaining"
});

// Whether a DAG node must run or is positively authorized to skip.
//
// Missing evidence is never `NotApplicable`: a skip is valid only where the
// (mode, stage) authorization map below positively allows it.
closed_enum!(Applicability {
    Required => "required",
    NotApplicable => "not_applicable"
});

// ---------------------------------------------------------------------------
// Validator errors
// ---------------------------------------------------------------------------

/// Closed fail-closed rejection codes. Every rejection names exactly one code
/// so fixture expectations and adapter projections cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractViolation {
    MalformedDocument,
    UnknownSchemaVersion,
    AmbiguousSelector,
    SelectorSubjectMismatch,
    ModeMismatch,
    IncoherentTargetIdentity,
    SubjectIncomplete,
    DuplicateMemberRole,
    FallbackNotAllowed,
    DuplicateStageNode,
    UnknownPredecessor,
    CyclicStageGraph,
    UnauthorizedStageApplicability,
    UnknownReceiptSchema,
    SubjectDigestMismatch,
    TransactionMismatch,
    AttemptMismatch,
    DuplicateStageResult,
    PredecessorMismatch,
    MissingRequiredStage,
    SuccessAfterTerminalEvidence,
    InstrumentIncompleteSuccess,
    OutcomeConflict,
    PrivateOutputLeakage,
    PolicyIdentityMismatch,
}

impl ContractViolation {
    /// Every value in declaration order; consumed as a closed surface by the
    /// adapter and bindings children even where this crate does not yet
    /// construct each value.
    #[allow(dead_code)]
    pub const ALL: &'static [Self] = &[
        Self::MalformedDocument,
        Self::UnknownSchemaVersion,
        Self::AmbiguousSelector,
        Self::SelectorSubjectMismatch,
        Self::ModeMismatch,
        Self::IncoherentTargetIdentity,
        Self::SubjectIncomplete,
        Self::DuplicateMemberRole,
        Self::FallbackNotAllowed,
        Self::DuplicateStageNode,
        Self::UnknownPredecessor,
        Self::CyclicStageGraph,
        Self::UnauthorizedStageApplicability,
        Self::UnknownReceiptSchema,
        Self::SubjectDigestMismatch,
        Self::TransactionMismatch,
        Self::AttemptMismatch,
        Self::DuplicateStageResult,
        Self::PredecessorMismatch,
        Self::MissingRequiredStage,
        Self::SuccessAfterTerminalEvidence,
        Self::InstrumentIncompleteSuccess,
        Self::OutcomeConflict,
        Self::PrivateOutputLeakage,
        Self::PolicyIdentityMismatch,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedDocument => "malformed_document",
            Self::UnknownSchemaVersion => "unknown_schema_version",
            Self::AmbiguousSelector => "ambiguous_selector",
            Self::SelectorSubjectMismatch => "selector_subject_mismatch",
            Self::ModeMismatch => "mode_mismatch",
            Self::IncoherentTargetIdentity => "incoherent_target_identity",
            Self::SubjectIncomplete => "subject_incomplete",
            Self::DuplicateMemberRole => "duplicate_member_role",
            Self::FallbackNotAllowed => "fallback_not_allowed",
            Self::DuplicateStageNode => "duplicate_stage_node",
            Self::UnknownPredecessor => "unknown_predecessor",
            Self::CyclicStageGraph => "cyclic_stage_graph",
            Self::UnauthorizedStageApplicability => "unauthorized_stage_applicability",
            Self::UnknownReceiptSchema => "unknown_receipt_schema",
            Self::SubjectDigestMismatch => "subject_digest_mismatch",
            Self::TransactionMismatch => "transaction_mismatch",
            Self::AttemptMismatch => "attempt_mismatch",
            Self::DuplicateStageResult => "duplicate_stage_result",
            Self::PredecessorMismatch => "predecessor_mismatch",
            Self::MissingRequiredStage => "missing_required_stage",
            Self::SuccessAfterTerminalEvidence => "success_after_terminal_evidence",
            Self::InstrumentIncompleteSuccess => "instrument_incomplete_success",
            Self::OutcomeConflict => "outcome_conflict",
            Self::PrivateOutputLeakage => "private_output_leakage",
            Self::PolicyIdentityMismatch => "policy_identity_mismatch",
        }
    }

    /// Decode the wire literal; unknown values fail closed.
    #[allow(dead_code)]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|value| value.as_str() == text)
    }
}

#[derive(Debug)]
pub struct ContractError {
    code: ContractViolation,
    detail: String,
}

impl ContractError {
    fn new(code: ContractViolation, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }

    pub const fn code(&self) -> ContractViolation {
        self.code
    }
}

impl Display for ContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.detail)
    }
}

type ContractResult<T> = std::result::Result<T, ContractError>;

fn violation<T>(code: ContractViolation, detail: impl Into<String>) -> ContractResult<T> {
    Err(ContractError::new(code, detail))
}

// ---------------------------------------------------------------------------
// Bounded scalar validation, canonical bytes, domain-separated digests
// ---------------------------------------------------------------------------

fn head(value: &str) -> &str {
    let mut end = value.len().min(16);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn hex_sha256(value: &str, field: &str) -> ContractResult<()> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be exactly 64 hexadecimal characters"),
        )
    }
}

fn bounded_id(value: &str, field: &str) -> ContractResult<()> {
    let valid = !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'));
    if valid {
        Ok(())
    } else {
        violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be a bounded path-safe identity (1..={MAX_ID_CHARS})"),
        )
    }
}

/// Repository identities are exactly `owner/name`.
fn bounded_repo(value: &str, field: &str) -> ContractResult<()> {
    let mut segments = value.split('/');
    let (owner, name, rest) = (segments.next(), segments.next(), segments.next());
    let ok = value.len() <= MAX_ID_CHARS
        && rest.is_none()
        && owner.is_some_and(|segment| bounded_id(segment, field).is_ok())
        && name.is_some_and(|segment| bounded_id(segment, field).is_ok());
    if ok {
        Ok(())
    } else {
        violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be an owner/name repository identity"),
        )
    }
}

/// Flat package identities (crates.io / Perl distro style): alphanumerics
/// plus `-`/`_`/`.` and no hierarchy separators.
fn bounded_package(value: &str, field: &str) -> ContractResult<()> {
    let ok = !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if ok {
        Ok(())
    } else {
        violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be a flat package identity (no separators)"),
        )
    }
}

/// Artifact names must never be paths: no separators, printable ASCII only.
fn bounded_artifact_name(value: &str, field: &str) -> ContractResult<()> {
    let ok = !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && !value.contains('/')
        && !value.contains('\\')
        && value.bytes().all(|byte| byte.is_ascii_graphic());
    if ok {
        Ok(())
    } else {
        violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be a flat artifact name, never a path"),
        )
    }
}

fn bounded_text(value: &str, field: &str, max: usize) -> ContractResult<()> {
    if value.trim().is_empty() || value.len() > max {
        return violation(
            ContractViolation::MalformedDocument,
            format!("{field} must be non-empty and at most {max} characters"),
        );
    }
    if value.chars().any(char::is_control) {
        return violation(
            ContractViolation::MalformedDocument,
            format!("{field} must not contain control characters"),
        );
    }
    Ok(())
}

/// Key-sorted canonical JSON serialization: identical documents serialize to
/// identical bytes regardless of input key order.
pub fn canonical_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Object(map) => {
            let members: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(item)
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        JsonValue::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn canonical_bytes<T: Serialize>(value: &T) -> ContractResult<Vec<u8>> {
    let json = serde_json::to_value(value).map_err(|error| {
        ContractError::new(
            ContractViolation::MalformedDocument,
            format!("serialization failed: {error}"),
        )
    })?;
    Ok(canonical_json(&json).into_bytes())
}

/// Domain-separated SHA-256 over canonical bytes. Two domains never produce
/// equal digests for equal payloads, so an intent digest can never stand in
/// for a subject or receipt digest of the same document.
fn domain_digest(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

// ---------------------------------------------------------------------------
// Privacy boundary
// ---------------------------------------------------------------------------

/// Bounded scan for private process-local state in durable output: absolute
/// user/system paths, home/env interpolations, environment dumps, and
/// credential shapes are forbidden in any durable projection.
pub fn redaction_finding(text: &str) -> Option<&'static str> {
    const PATTERNS: [(&str, &str); 12] = [
        ("/usr/", "unix system path"),
        ("/home/", "unix home path"),
        ("/root/", "unix root path"),
        ("/tmp/", "unix temp path"),
        ("\\users\\", "windows profile path"),
        ("$home", "home interpolation"),
        ("${home}", "home interpolation"),
        ("%userprofile%", "profile interpolation"),
        ("path=", "environment dump"),
        ("bearer ", "bearer credential"),
        ("begin private key", "private key material"),
        ("token=", "inline token"),
    ];
    let lowered = text.to_ascii_lowercase();
    PATTERNS.iter().find(|(needle, _)| lowered.contains(needle)).map(|(_, kind)| *kind)
}

fn reject_private_output<T: Serialize>(value: &T, what: &str) -> ContractResult<()> {
    let bytes = canonical_bytes(value)?;
    let rendered = String::from_utf8_lossy(&bytes);
    match redaction_finding(&rendered) {
        Some(kind) => violation(
            ContractViolation::PrivateOutputLeakage,
            format!("{what} leaked {kind} into durable output"),
        ),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Shared scalar shapes
// ---------------------------------------------------------------------------

fn schema_check(actual: &str, expected: &str) -> ContractResult<()> {
    if actual == expected {
        Ok(())
    } else {
        violation(
            ContractViolation::UnknownSchemaVersion,
            format!("schema_version must be {expected}, got {actual}"),
        )
    }
}

/// Platform / execution environment / architecture / libc disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetIdentity {
    pub platform: Platform,
    pub triple: String,
    pub libc: LibcDisposition,
}

impl TargetIdentity {
    fn validate(&self) -> ContractResult<()> {
        bounded_id(&self.triple, "target.triple")?;
        // External platform truth: gnu/musl are Linux libcs, msvc is
        // Windows-only; macOS carries no libc disposition.
        let coherent = match self.libc {
            LibcDisposition::Gnu | LibcDisposition::Musl => self.platform == Platform::Linux,
            LibcDisposition::Msvc => self.platform == Platform::Windows,
            LibcDisposition::NoneLibc => true,
        };
        if coherent {
            Ok(())
        } else {
            violation(
                ContractViolation::IncoherentTargetIdentity,
                format!(
                    "libc {} is impossible on platform {}",
                    self.libc.as_str(),
                    self.platform.as_str()
                ),
            )
        }
    }
}

/// One required product-unit member identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberIdentity {
    pub role: MemberRole,
    /// Flat archive member name (never a path).
    pub artifact_name: String,
}

impl MemberIdentity {
    fn validate(&self) -> ContractResult<()> {
        bounded_artifact_name(&self.artifact_name, "member.artifact_name")
    }
}

/// A server-only unit is exactly the server binary; the reviewed pair is
/// exactly server + DAP adapter. Anything else is drift.
fn validate_member_set(unit: ProductUnit, members: &[MemberIdentity]) -> ContractResult<()> {
    let mut roles = BTreeSet::new();
    for member in members {
        member.validate()?;
        if !roles.insert(member.role) {
            return violation(
                ContractViolation::DuplicateMemberRole,
                format!("role {} declared twice", member.role.as_str()),
            );
        }
    }
    let complete = match unit {
        ProductUnit::ServerOnly => roles == BTreeSet::from([MemberRole::PerllspServer]),
        ProductUnit::ServerDapPair => {
            roles == BTreeSet::from([MemberRole::PerllspServer, MemberRole::PerlDapAdapter])
        }
    };
    if complete {
        Ok(())
    } else {
        violation(
            ContractViolation::SubjectIncomplete,
            format!(
                "{} requires exactly its product-unit members, got {}",
                unit.as_str(),
                members.len()
            ),
        )
    }
}

// ---------------------------------------------------------------------------
// Install intent
// ---------------------------------------------------------------------------

/// Release selector: pinned exact tag, unresolved latest request, or positive
/// not-applicable. A closed struct rather than an open tagged enum so unknown
/// keys fail at the serde boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSelector {
    pub kind: SelectorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl ReleaseSelector {
    pub fn exact(tag: impl Into<String>) -> Self {
        Self { kind: SelectorKind::Exact, tag: Some(tag.into()) }
    }

    pub const fn latest_requested() -> Self {
        Self { kind: SelectorKind::LatestRequested, tag: None }
    }

    pub const fn not_applicable() -> Self {
        Self { kind: SelectorKind::NotApplicable, tag: None }
    }

    fn validate(&self) -> ContractResult<()> {
        match self.kind {
            SelectorKind::Exact => {
                let tag = self.tag.as_deref().ok_or_else(|| {
                    ContractError::new(
                        ContractViolation::MalformedDocument,
                        "exact selectors must carry a tag".to_string(),
                    )
                })?;
                bounded_id(tag, "selector.tag")
            }
            SelectorKind::LatestRequested | SelectorKind::NotApplicable => {
                if self.tag.is_some() {
                    return violation(
                        ContractViolation::MalformedDocument,
                        "latest_requested/not_applicable selectors must not carry a tag",
                    );
                }
                Ok(())
            }
        }
    }
}

/// Explicit target override with its provenance authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetOverride {
    pub triple: String,
    /// Who/what authorized the override (bounded identity, not free prose).
    pub authority: String,
}

/// The immutable first-phase record: trusted user/config input bound to one
/// operation. An intent is NOT yet an exact release subject; a
/// `latest_requested` selector remains unresolved and can never authorize
/// artifact work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneInstallIntent {
    pub schema_version: String,
    pub transaction_id: String,
    pub attempt_id: String,
    pub operation: InstallOperation,
    pub route: RouteMode,
    pub mode: InstallMode,
    pub selector: ReleaseSelector,
    pub target: TargetIdentity,
    pub target_override: Option<TargetOverride>,
    pub requested_product_unit: ProductUnit,
    pub destination_role: DestinationRole,
    pub path_policy: PathPolicy,
    pub fallback_policy: FallbackPolicy,
    /// Trusted configuration/input generation digest (64 hex).
    pub trusted_config_digest: String,
    pub policy_version: String,
    pub contract_generation: u32,
}

impl StandaloneInstallIntent {
    /// Validate fail-closed and recompute the immutable intent identity over
    /// canonical bytes.
    pub fn validate(&self) -> ContractResult<String> {
        schema_check(&self.schema_version, INTENT_SCHEMA_VERSION)?;
        bounded_id(&self.transaction_id, "transaction_id")?;
        bounded_id(&self.attempt_id, "attempt_id")?;
        hex_sha256(&self.trusted_config_digest, "trusted_config_digest")?;
        bounded_text(&self.policy_version, "policy_version", 64)?;
        if self.contract_generation == 0 {
            return violation(
                ContractViolation::MalformedDocument,
                "contract_generation starts at 1",
            );
        }
        self.target.validate()?;
        if let Some(override_target) = &self.target_override {
            bounded_id(&override_target.triple, "target_override.triple")?;
            bounded_text(&override_target.authority, "target_override.authority", MAX_TEXT_CHARS)?;
        }
        self.selector.validate()?;
        // Only release-archive intents select releases. `latest_requested`
        // stays unresolved by definition: it names an intent but authorizes
        // no transport, staging, or mutation.
        match self.mode {
            InstallMode::ReleaseArchive => {
                if self.selector.kind == SelectorKind::NotApplicable {
                    return violation(
                        ContractViolation::AmbiguousSelector,
                        "release-archive intents must carry exact or latest_requested selectors",
                    );
                }
                if self.selector.kind == SelectorKind::LatestRequested
                    && self.operation == InstallOperation::Uninstall
                {
                    return violation(
                        ContractViolation::AmbiguousSelector,
                        "an uninstall operation cannot be driven by an unresolved latest request",
                    );
                }
            }
            InstallMode::ExactRegistrySource | InstallMode::ExplicitLocalDevelopment => {
                if self.selector.kind != SelectorKind::NotApplicable {
                    return violation(
                        ContractViolation::AmbiguousSelector,
                        "only release-archive modes carry release selectors",
                    );
                }
            }
        }
        let bytes = canonical_bytes(self)?;
        Ok(domain_digest(INTENT_DIGEST_DOMAIN, &bytes))
    }

    /// True when this intent's selection is fully resolved and may authorize
    /// artifact work. `latest_requested` and local development never satisfy
    /// this. Resolver/adapters children consume this predicate.
    #[allow(dead_code)]
    pub fn authorizes_artifact_work(&self) -> bool {
        self.mode != InstallMode::ExplicitLocalDevelopment
            && self.selector.kind == SelectorKind::Exact
    }
}

// ---------------------------------------------------------------------------
// Resolved subject
// ---------------------------------------------------------------------------

/// Exact release-archive subject produced by the bounded resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArchiveSubject {
    pub schema_version: String,
    pub subject_id: String,
    /// Exact repository identity (`owner/name`).
    pub repository: String,
    /// Exact resolved tag; latest drift after resolution is impossible here.
    pub tag: String,
    /// Frozen/prepared/public topology identity and digest.
    pub topology_id: String,
    pub topology_digest: String,
    /// Exact topology target row.
    pub topology_row: String,
    pub target: TargetIdentity,
    pub archive_format: ArchiveFormat,
    /// Flat archive asset name (never a path).
    pub archive_name: String,
    /// Required product-unit members (role + flat artifact identity).
    pub expected_members: Vec<MemberIdentity>,
    pub product_unit: ProductUnit,
    pub integrity_policy_id: String,
    /// Present means independent provenance is required for this subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_policy_id: Option<String>,
    pub destination_role: DestinationRole,
}

/// Exact registry/package/version source subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactRegistrySourceSubject {
    pub schema_version: String,
    pub subject_id: String,
    pub registry_id: String,
    pub package: String,
    pub version: String,
    /// Published lockfile/source identity where available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile_digest: Option<String>,
    pub toolchain_policy_id: String,
    pub target: TargetIdentity,
    pub product_unit: ProductUnit,
    /// Expected executable role/identity for this subject.
    pub executable_role: MemberRole,
    pub destination_role: DestinationRole,
}

/// Explicit local-development subject. Permanently non-authoritative: green
/// evidence can never satisfy a release/install claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDevelopmentSubject {
    pub schema_version: String,
    pub subject_id: String,
    /// Bounded non-authoritative description (redaction-scanned).
    pub description: String,
    pub destination_role: DestinationRole,
}

/// The single second-phase subject. Mode is the closed union tag: a resolved
/// subject is always exactly one mode-specific shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ResolvedStandaloneInstallSubject {
    ReleaseArchive(ReleaseArchiveSubject),
    ExactRegistrySource(ExactRegistrySourceSubject),
    ExplicitLocalDevelopment(LocalDevelopmentSubject),
}

impl ResolvedStandaloneInstallSubject {
    pub const fn mode(&self) -> InstallMode {
        match self {
            Self::ReleaseArchive(_) => InstallMode::ReleaseArchive,
            Self::ExactRegistrySource(_) => InstallMode::ExactRegistrySource,
            Self::ExplicitLocalDevelopment(_) => InstallMode::ExplicitLocalDevelopment,
        }
    }

    /// Requested product unit of the resolved subject (coordinator child
    /// seam).
    #[allow(dead_code)]
    pub const fn product_unit(&self) -> ProductUnit {
        match self {
            Self::ReleaseArchive(subject) => subject.product_unit,
            Self::ExactRegistrySource(subject) => subject.product_unit,
            Self::ExplicitLocalDevelopment(_) => ProductUnit::ServerOnly,
        }
    }

    /// Destination role of the resolved subject (coordinator child seam).
    #[allow(dead_code)]
    pub const fn destination_role(&self) -> DestinationRole {
        match self {
            Self::ReleaseArchive(subject) => subject.destination_role,
            Self::ExactRegistrySource(subject) => subject.destination_role,
            Self::ExplicitLocalDevelopment(subject) => subject.destination_role,
        }
    }

    fn target(&self) -> Option<&TargetIdentity> {
        match self {
            Self::ReleaseArchive(subject) => Some(&subject.target),
            Self::ExactRegistrySource(subject) => Some(&subject.target),
            Self::ExplicitLocalDevelopment(_) => None,
        }
    }

    pub fn subject_id(&self) -> &str {
        match self {
            Self::ReleaseArchive(subject) => &subject.subject_id,
            Self::ExactRegistrySource(subject) => &subject.subject_id,
            Self::ExplicitLocalDevelopment(subject) => &subject.subject_id,
        }
    }

    /// Validate fail-closed and recompute the immutable subject digest over
    /// canonical bytes.
    pub fn validate(&self) -> ContractResult<String> {
        match self {
            Self::ReleaseArchive(subject) => {
                schema_check(&subject.schema_version, SUBJECT_SCHEMA_VERSION)?;
                bounded_repo(&subject.repository, "repository")?;
                bounded_id(&subject.tag, "tag")?;
                bounded_id(&subject.topology_id, "topology_id")?;
                hex_sha256(&subject.topology_digest, "topology_digest")?;
                bounded_id(&subject.topology_row, "topology_row")?;
                subject.target.validate()?;
                bounded_artifact_name(&subject.archive_name, "archive_name")?;
                bounded_id(&subject.integrity_policy_id, "integrity_policy_id")?;
                if let Some(policy) = &subject.provenance_policy_id {
                    bounded_id(policy, "provenance_policy_id")?;
                }
                validate_member_set(subject.product_unit, &subject.expected_members)?;
            }
            Self::ExactRegistrySource(subject) => {
                schema_check(&subject.schema_version, SUBJECT_SCHEMA_VERSION)?;
                bounded_id(&subject.registry_id, "registry_id")?;
                bounded_package(&subject.package, "package")?;
                bounded_id(&subject.version, "version")?;
                if let Some(digest) = &subject.lockfile_digest {
                    hex_sha256(digest, "lockfile_digest")?;
                }
                bounded_id(&subject.toolchain_policy_id, "toolchain_policy_id")?;
                subject.target.validate()?;
            }
            Self::ExplicitLocalDevelopment(subject) => {
                schema_check(&subject.schema_version, SUBJECT_SCHEMA_VERSION)?;
                bounded_text(&subject.description, "description", MAX_TEXT_CHARS)?;
            }
        }
        bounded_id(self.subject_id(), "subject_id")?;
        let bytes = canonical_bytes(self)?;
        Ok(domain_digest(SUBJECT_DIGEST_DOMAIN, &bytes))
    }
}

// ---------------------------------------------------------------------------
// Resolver seam (the ONLY subject producers)
// ---------------------------------------------------------------------------

/// Resolver-boundary constructor: the sole sanctioned way to obtain a
/// validated resolved subject from an intent. Fail-closed on ambiguous
/// selectors, mode changes, selector/subject disagreement, and destination/
/// product-unit drift.
///
/// Real topology/release resolution belongs to child 2; this seam enforces
/// only the two-phase grammar so no caller can mint a subject from an
/// unresolved intent.
pub fn resolve_subject(
    intent: &StandaloneInstallIntent,
    candidate: ResolvedStandaloneInstallSubject,
) -> ContractResult<ResolvedStandaloneInstallSubject> {
    intent.validate()?;
    if candidate.mode() != intent.mode {
        return violation(
            ContractViolation::ModeMismatch,
            "a resolver cannot change the intent's mode",
        );
    }
    if intent.selector.kind == SelectorKind::LatestRequested {
        return violation(
            ContractViolation::AmbiguousSelector,
            "a latest_requested intent is unresolved and cannot produce a subject; \
             re-intent with an exact selector first",
        );
    }
    // Tag coherence stays archive-specific (registry/local subjects carry no
    // release tag); product-unit and destination-role coherence binds every
    // resolvable mode, so a registry-source subject cannot drift from the
    // requested unit or install root either.
    if let ResolvedStandaloneInstallSubject::ReleaseArchive(subject) = &candidate {
        if subject.tag != intent.selector.tag.as_deref().unwrap_or_default() {
            return violation(
                ContractViolation::SelectorSubjectMismatch,
                format!(
                    "resolved tag {} does not match the exact intent selector",
                    head(&subject.tag)
                ),
            );
        }
    }
    if candidate.mode() != InstallMode::ExplicitLocalDevelopment {
        // Local development declares no product-unit identity; the accessor's
        // server-only default is not an authorization surface there.
        if candidate.destination_role() != intent.destination_role {
            return violation(
                ContractViolation::OutcomeConflict,
                "resolved destination role disagrees with the intent",
            );
        }
        if candidate.product_unit() != intent.requested_product_unit {
            return violation(
                ContractViolation::OutcomeConflict,
                "resolved product unit disagrees with the intent",
            );
        }
    }
    candidate.validate()?;
    if let Some(subject_target) = candidate.target() {
        if subject_target.platform != intent.target.platform
            || subject_target.libc != intent.target.libc
        {
            return violation(
                ContractViolation::OutcomeConflict,
                "resolved subject target platform/libc disagrees with the intent",
            );
        }
        let expected_triple = intent
            .target_override
            .as_ref()
            .map(|override_target| override_target.triple.as_str())
            .unwrap_or(intent.target.triple.as_str());
        if subject_target.triple != expected_triple {
            return violation(
                ContractViolation::OutcomeConflict,
                "resolved subject target triple disagrees with the intent or its explicit override",
            );
        }
    } else if intent.target_override.is_some() {
        return violation(
            ContractViolation::OutcomeConflict,
            "a local-development subject cannot consume a target override",
        );
    }
    Ok(candidate)
}

/// An explicit fallback branch: a NEW resolved subject under a NEW attempt,
/// carrying the failed branch's subject digest only as isolated evidence.
/// Never a mutation of the failed archive subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackBranch {
    pub prior_subject_digest: String,
    pub new_attempt_id: String,
    pub subject: ResolvedStandaloneInstallSubject,
}

/// Fallback seam: archive failure may create a registry-source branch only
/// when the original intent explicitly admitted the transition. The new
/// attempt id MUST differ from the failed attempt and the resolved subject
/// MUST be genuinely new.
pub fn fallback_branch(
    intent: &StandaloneInstallIntent,
    failed_subject_digest: &str,
    new_attempt_id: &str,
    new_subject: ResolvedStandaloneInstallSubject,
) -> ContractResult<FallbackBranch> {
    intent.validate()?;
    hex_sha256(failed_subject_digest, "failed_subject_digest")?;
    bounded_id(new_attempt_id, "new_attempt_id")?;
    if intent.fallback_policy != FallbackPolicy::ArchiveToSourceAllowed {
        return violation(
            ContractViolation::FallbackNotAllowed,
            "the intent did not admit an archive-to-source fallback transition",
        );
    }
    if intent.mode != InstallMode::ReleaseArchive
        || new_subject.mode() != InstallMode::ExactRegistrySource
    {
        return violation(
            ContractViolation::FallbackNotAllowed,
            "fallback branches model archive-failure to registry-source transitions only",
        );
    }
    if new_attempt_id == intent.attempt_id {
        return violation(
            ContractViolation::AttemptMismatch,
            "a fallback branch is a new attempt, never a continuation of the failed one",
        );
    }
    let new_digest = new_subject.validate()?;
    if new_digest == failed_subject_digest {
        return violation(
            ContractViolation::SubjectDigestMismatch,
            "a fallback branch must resolve a NEW subject, never reuse the failed one",
        );
    }
    Ok(FallbackBranch {
        prior_subject_digest: failed_subject_digest.to_string(),
        new_attempt_id: new_attempt_id.to_string(),
        subject: new_subject,
    })
}

// ---------------------------------------------------------------------------
// Stage receipts
// ---------------------------------------------------------------------------

/// The bounded evidence envelope every stage emits. Receipts bind the exact
/// transaction, attempt, resolved-subject digest, predecessors, policies,
/// and artifacts; they may add evidence but never rebuild or widen the
/// subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageReceipt {
    pub schema_version: String,
    pub transaction_id: String,
    pub attempt_id: String,
    /// Digest of the resolved subject this receipt binds to (64 hex).
    pub subject_digest: String,
    pub stage_id: StageId,
    /// Stage implementation/policy identity (bounded, redaction-scanned).
    pub implementation_identity: String,
    /// Receipt↔subject policy binding (#11099): the integrity/provenance/
    /// toolchain policy identities this stage's evidence is bound to. The
    /// fan-in recomputes the expectation from the settled subject
    /// ([`expected_receipt_policies`]) and rejects drift; stages outside the
    /// policy-bearing set must leave these empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity_policy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_policy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain_policy_id: Option<String>,
    /// Ordered predecessor receipt digests (recomputed against the DAG).
    pub predecessor_receipt_digests: Vec<String>,
    /// Input artifact/content identities consumed by this stage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_artifact_ids: Vec<String>,
    pub result: StageResult,
    pub reason: ReasonFamily,
    pub next_action: ActionClass,
    /// Output evidence/artifact identities produced by this stage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_evidence_ids: Vec<String>,
    pub instrument_completeness: InstrumentCompleteness,
    pub redaction_disposition: RedactionDisposition,
}

impl StageReceipt {
    /// Validate fail-closed and recompute the receipt digest over canonical
    /// bytes.
    pub fn validate(&self) -> ContractResult<String> {
        if self.schema_version != RECEIPT_SCHEMA_VERSION {
            return violation(
                ContractViolation::UnknownReceiptSchema,
                format!(
                    "schema_version must be {RECEIPT_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }
        bounded_id(&self.transaction_id, "transaction_id")?;
        bounded_id(&self.attempt_id, "attempt_id")?;
        hex_sha256(&self.subject_digest, "subject_digest")?;
        for predecessor in &self.predecessor_receipt_digests {
            hex_sha256(predecessor, "predecessor_receipt_digests[]")?;
        }
        let mut unique = BTreeSet::new();
        for identity in self.input_artifact_ids.iter().chain(&self.output_evidence_ids) {
            bounded_id(identity, "artifact/evidence identity")?;
            if !unique.insert(identity.as_str()) {
                return violation(
                    ContractViolation::MalformedDocument,
                    format!("artifact/evidence identity {identity:?} declared twice"),
                );
            }
        }
        bounded_text(&self.implementation_identity, "implementation_identity", MAX_TEXT_CHARS)?;
        for policy in
            [&self.integrity_policy_id, &self.provenance_policy_id, &self.toolchain_policy_id]
                .into_iter()
                .flatten()
        {
            bounded_id(policy, "receipt policy identity")?;
        }
        // Result/reason/action coherence: success is silent; every failure
        // names its reason and next action; cancellation and timeouts stay
        // distinct; skips are silent and separately authorized.
        let coherent = match self.result {
            StageResult::Succeeded => {
                self.reason == ReasonFamily::None && self.next_action == ActionClass::None
            }
            StageResult::Failed => {
                self.reason != ReasonFamily::None && self.next_action != ActionClass::None
            }
            StageResult::Cancelled => {
                self.reason == ReasonFamily::Cancelled && self.next_action != ActionClass::None
            }
            StageResult::TimedOut => {
                self.reason == ReasonFamily::Timeout && self.next_action != ActionClass::None
            }
            StageResult::NotProven => {
                matches!(
                    self.reason,
                    ReasonFamily::NotProven
                        | ReasonFamily::InstrumentFailure
                        | ReasonFamily::MissingEvidence
                ) && self.next_action != ActionClass::None
            }
            StageResult::NotApplicable => {
                self.reason == ReasonFamily::None && self.next_action == ActionClass::None
            }
        };
        if !coherent {
            return violation(
                ContractViolation::OutcomeConflict,
                format!(
                    "result {} disagrees with reason {}/action {}",
                    self.result.as_str(),
                    self.reason.as_str(),
                    self.next_action.as_str()
                ),
            );
        }
        if self.redaction_disposition != RedactionDisposition::RedactedRolesOnly {
            return violation(
                ContractViolation::PrivateOutputLeakage,
                "durable receipts must declare redacted_roles_only disposition",
            );
        }
        reject_private_output(self, "stage receipt")?;
        let bytes = canonical_bytes(self)?;
        Ok(domain_digest(RECEIPT_DIGEST_DOMAIN, &bytes))
    }
}

/// Receipt↔subject policy binding (#11099): the exact
/// (integrity, provenance, toolchain) policy identities a receipt for
/// `stage` must carry against this settled subject. Integrity-bearing stages
/// bind the archive subject's integrity policy, the provenance stage binds
/// the subject's optional provenance policy (absent stays absent), and
/// source builds bind the registry subject's toolchain policy. The fan-in
/// recomputes this expectation from the resolved subject and rejects drift;
/// fixture builders consume the same table so producers cannot diverge.
fn expected_receipt_policies(
    subject: &ResolvedStandaloneInstallSubject,
    stage: StageId,
) -> (Option<String>, Option<String>, Option<String>) {
    let unbound = (None, None, None);
    match subject {
        ResolvedStandaloneInstallSubject::ReleaseArchive(archive) => match stage {
            StageId::ChecksumIntegrity | StageId::ArchiveManifestAndStaging => {
                (Some(archive.integrity_policy_id.clone()), None, None)
            }
            StageId::Provenance => (None, archive.provenance_policy_id.clone(), None),
            _ => unbound,
        },
        ResolvedStandaloneInstallSubject::ExactRegistrySource(registry) => match stage {
            StageId::SourceBuild | StageId::ExecutableObservation => {
                (None, None, Some(registry.toolchain_policy_id.clone()))
            }
            _ => unbound,
        },
        ResolvedStandaloneInstallSubject::ExplicitLocalDevelopment(_) => unbound,
    }
}

// ---------------------------------------------------------------------------
// Stage DAG
// ---------------------------------------------------------------------------

/// One node: a stage, whether it must run, and its exact predecessors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageNode {
    pub stage_id: StageId,
    pub applicability: Applicability,
    /// Exact predecessor stages; each must appear earlier in declaration
    /// order (topological declaration is itself load-bearing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predecessors: Vec<StageId>,
}

/// The composition plan for one mode/product-unit pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDag {
    pub schema_version: String,
    pub mode: InstallMode,
    pub product_unit: ProductUnit,
    /// Nodes in topological declaration order (predecessors first).
    pub nodes: Vec<StageNode>,
}

impl StageDag {
    /// Validate structure fail-closed: unique nodes, known predecessors,
    /// acyclic graph, topological declaration order, and applicability that
    /// the mode/stage authorization map positively allows.
    pub fn validate(&self) -> ContractResult<()> {
        schema_check(&self.schema_version, DAG_SCHEMA_VERSION)?;
        let required_floor: &[StageId] = match self.mode {
            InstallMode::ReleaseArchive => &[
                StageId::ResolveSubject,
                StageId::Transport,
                StageId::ChecksumIntegrity,
                StageId::ArchiveManifestAndStaging,
                StageId::ExecutableObservation,
                StageId::Promotion,
                StageId::PathPersistence,
                StageId::FreshProcessObservation,
                StageId::InstalledTransition,
            ],
            InstallMode::ExactRegistrySource => &[
                StageId::ResolveSubject,
                StageId::SourceBuild,
                StageId::Promotion,
                StageId::PathPersistence,
                StageId::FreshProcessObservation,
                StageId::InstalledTransition,
            ],
            InstallMode::ExplicitLocalDevelopment => {
                &[StageId::ResolveSubject, StageId::SourceBuild]
            }
        };
        for required in required_floor {
            match self.nodes.iter().find(|node| node.stage_id == *required) {
                Some(node) if node.applicability == Applicability::Required => {}
                Some(_) | None => {
                    return violation(
                        ContractViolation::MissingRequiredStage,
                        format!(
                            "{} mode requires a required {} stage",
                            self.mode.as_str(),
                            required.as_str()
                        ),
                    );
                }
            }
        }
        let mut positions = HashMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if positions.contains_key(&node.stage_id) {
                return violation(
                    ContractViolation::DuplicateStageNode,
                    format!("stage {} declared twice", node.stage_id.as_str()),
                );
            }
            positions.insert(node.stage_id, index);
            let allowed = authorized_applicabilities(self.mode, node.stage_id);
            let permitted: &[Applicability] =
                allowed.as_ref().map(|pair| pair.as_slice()).unwrap_or(&[]);
            if !permitted.contains(&node.applicability) {
                return violation(
                    ContractViolation::UnauthorizedStageApplicability,
                    format!(
                        "stage {} cannot be {} in {} mode",
                        node.stage_id.as_str(),
                        node.applicability.as_str(),
                        self.mode.as_str()
                    ),
                );
            }
        }
        for node in &self.nodes {
            let own_position = positions[&node.stage_id];
            for predecessor in &node.predecessors {
                let predecessor_position = positions.get(predecessor).ok_or_else(|| {
                    ContractError::new(
                        ContractViolation::UnknownPredecessor,
                        format!(
                            "stage {} cites undeclared predecessor {}",
                            node.stage_id.as_str(),
                            predecessor.as_str()
                        ),
                    )
                })?;
                if *predecessor_position >= own_position {
                    // A predecessor cited at or after the dependent position
                    // is self-reference or a cycle rendered as order; both
                    // are rejected before any fan-in runs.
                    return violation(
                        ContractViolation::CyclicStageGraph,
                        format!(
                            "stage {} cites predecessor {} at or after its own position \
                             (reversed/cyclic graph)",
                            node.stage_id.as_str(),
                            predecessor.as_str()
                        ),
                    );
                }
            }
        }
        self.validate_canonical_floor()
    }

    /// Canonical composition floor (#11099): outside explicit local
    /// development a transaction can only terminate installed through a
    /// required promotion of route-specific completed work, and PATH
    /// persistence, fresh-process observation, and the installed transition
    /// can only follow that promoted candidate. Structural self-consistency
    /// alone cannot express these mandatory nodes and edges — a DAG could
    /// otherwise omit promotion entirely or declare post-promotion stages
    /// with empty predecessor sets and still fold green to installed — so
    /// the validator enforces the floor itself.
    fn validate_canonical_floor(&self) -> ContractResult<()> {
        use StageId::{
            ExecutableObservation, FreshProcessObservation, InstalledTransition, PathPersistence,
            Promotion, SourceBuild,
        };
        if self.mode == InstallMode::ExplicitLocalDevelopment {
            // Local development never promotes: its non-authoritative graph
            // is exempt from the install floor by construction.
            return Ok(());
        }
        let promotion = match self.nodes.iter().find(|node| node.stage_id == Promotion) {
            Some(node) => node,
            None => {
                return violation(
                    ContractViolation::MissingRequiredStage,
                    format!(
                        "{} mode requires a promotion stage: no transaction may claim an \
                         installed outcome without promoting a selected candidate",
                        self.mode.as_str()
                    ),
                );
            }
        };
        if promotion.applicability != Applicability::Required {
            return violation(
                ContractViolation::MissingRequiredStage,
                "promotion is mandatory outside explicit local development and cannot be \
                 declared not_applicable",
            );
        }
        if promotion.predecessors.is_empty() {
            return violation(
                ContractViolation::PredecessorMismatch,
                "promotion must cite the route-specific completed work; it cannot open a \
                 transaction",
            );
        }
        // Route-specific complete-predecessor rule: archive promotions cite
        // observed archive artifacts; source promotions cite the built tree.
        let route_work = match self.mode {
            InstallMode::ReleaseArchive => ExecutableObservation,
            _ => SourceBuild,
        };
        if !self.nodes.iter().any(|node| node.stage_id == route_work)
            || !self.reaches(Promotion, route_work)
        {
            return violation(
                ContractViolation::PredecessorMismatch,
                format!(
                    "promotion must have {} in its transitive predecessor set: no promoted \
                     candidate exists before the route-specific work completes",
                    route_work.as_str()
                ),
            );
        }
        let mut unordered: Vec<StageId> = Vec::new();
        for stage in [PathPersistence, FreshProcessObservation, InstalledTransition] {
            if !self.nodes.iter().any(|node| node.stage_id == stage) {
                return violation(
                    ContractViolation::MissingRequiredStage,
                    format!(
                        "{} mode requires a {} stage downstream of promotion",
                        self.mode.as_str(),
                        stage.as_str()
                    ),
                );
            }
            if !self.reaches(stage, Promotion) {
                unordered.push(stage);
            }
        }
        if !unordered.is_empty() {
            let names = unordered.iter().map(|stage| stage.as_str()).collect::<Vec<_>>().join(", ");
            return violation(
                ContractViolation::PredecessorMismatch,
                format!(
                    "stages [{names}] must have promotion in their transitive predecessor set: \
                     PATH persistence, fresh-process observation, and the installed transition \
                     can only follow a promoted candidate"
                ),
            );
        }
        Ok(())
    }

    /// True when `target` is reachable from `from` through declared
    /// predecessor edges. Cycles are already rejected structurally before
    /// this closure runs.
    fn reaches(&self, from: StageId, target: StageId) -> bool {
        let mut pending = vec![from];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if current == target {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(node) = self.nodes.iter().find(|node| node.stage_id == current) {
                pending.extend(node.predecessors.iter().copied());
            }
        }
        false
    }

    fn position_of(&self, stage: StageId) -> Option<usize> {
        self.nodes.iter().position(|node| node.stage_id == stage)
    }

    fn required_stages(&self) -> impl Iterator<Item = StageId> + '_ {
        self.nodes
            .iter()
            .filter(|node| node.applicability == Applicability::Required)
            .map(|node| node.stage_id)
    }
}

/// Positive (mode, stage) authorization map. `None` means the stage cannot
/// appear in this mode at all; `Some` lists the allowed applicabilities.
/// This table IS the model's stage-applicability authority: adapters may
/// differ mechanically, never semantically.
fn authorized_applicabilities(mode: InstallMode, stage: StageId) -> Option<[Applicability; 2]> {
    use Applicability::{NotApplicable, Required};
    let policy_dependent = Some([Required, NotApplicable]);
    match (mode, stage) {
        // Every mode must positively resolve its subject.
        (_, StageId::ResolveSubject) => policy_dependent,
        // Source builds never happen in archive mode; archive staging never
        // happens outside it.
        (InstallMode::ReleaseArchive, StageId::SourceBuild) => None,
        (InstallMode::ExactRegistrySource, StageId::ArchiveManifestAndStaging) => None,
        // Archive mode: transport/integrity/provenance/staging/observation
        // chain plus promotion floor.
        (InstallMode::ReleaseArchive, StageId::Transport)
        | (InstallMode::ReleaseArchive, StageId::ChecksumIntegrity)
        | (InstallMode::ReleaseArchive, StageId::Provenance)
        | (InstallMode::ReleaseArchive, StageId::ArchiveManifestAndStaging)
        | (InstallMode::ReleaseArchive, StageId::ExecutableObservation)
        | (InstallMode::ReleaseArchive, StageId::Promotion)
        | (InstallMode::ReleaseArchive, StageId::PathPersistence)
        | (InstallMode::ReleaseArchive, StageId::FreshProcessObservation)
        | (InstallMode::ReleaseArchive, StageId::InstalledTransition) => policy_dependent,
        // Source builds never run archive staging; the rest is policy
        // dependent activation.
        (InstallMode::ExactRegistrySource, StageId::Transport)
        | (InstallMode::ExactRegistrySource, StageId::ChecksumIntegrity)
        | (InstallMode::ExactRegistrySource, StageId::Provenance)
        | (InstallMode::ExactRegistrySource, StageId::ExecutableObservation)
        | (InstallMode::ExactRegistrySource, StageId::SourceBuild)
        | (InstallMode::ExactRegistrySource, StageId::Promotion)
        | (InstallMode::ExactRegistrySource, StageId::PathPersistence)
        | (InstallMode::ExactRegistrySource, StageId::FreshProcessObservation)
        | (InstallMode::ExactRegistrySource, StageId::InstalledTransition) => policy_dependent,
        // Local development is non-authoritative end to end: only subject
        // resolution and a local build may even be described.
        (InstallMode::ExplicitLocalDevelopment, StageId::SourceBuild) => policy_dependent,
        (InstallMode::ExplicitLocalDevelopment, _) => None,
        // Lifecycle stage available to archive/source modes.
        (_, StageId::Uninstall) => policy_dependent,
    }
}

// ---------------------------------------------------------------------------
// Terminal outcome fold (fan-in validation)
// ---------------------------------------------------------------------------

/// Inputs to one deterministic fan-in fold over an ordered receipt set.
pub struct FanInInput<'a> {
    pub dag: &'a StageDag,
    pub operation: InstallOperation,
    pub mode: InstallMode,
    pub transaction_id: &'a str,
    pub attempt_id: &'a str,
    /// The settled subject every receipt binds to; its recomputed digest must
    /// equal `subject_digest` and its policies anchor receipt bindings.
    pub subject: &'a ResolvedStandaloneInstallSubject,
    pub subject_digest: &'a str,
    /// Receipts in execution order; duplicates and disorder fail closed.
    pub receipts: &'a [StageReceipt],
}

/// The one terminal outcome of a validated transaction (#11099 terminal
/// vocabulary). Owns no user copy, no candidate/current authority, and no
/// side effects; downstream consumers (#11179 selection records, #5903
/// installed proof) interpret it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalStandaloneInstallOutcome {
    pub schema_version: String,
    pub transaction_id: String,
    pub attempt_id: String,
    pub subject_digest: String,
    pub result: TerminalResult,
    pub terminal_stage: StageId,
    /// Which candidate identity survives the transaction (#11099 terminal
    /// contract): the promoted current candidate, the restored previous
    /// candidate, no remaining candidate, or unresolved when terminal
    /// evidence stopped before the installed transition.
    pub candidate_disposition: CandidateDisposition,
    pub reason: ReasonFamily,
    pub next_action: ActionClass,
    pub side_effect_ceiling: SideEffectCeiling,
    /// Domain-separated digest over the ordered validated receipt set.
    pub stage_set_digest: String,
}

impl TerminalStandaloneInstallOutcome {
    /// Validate and recompute the stage-set binding over canonical bytes.
    pub fn validate(&self) -> ContractResult<String> {
        schema_check(&self.schema_version, OUTCOME_SCHEMA_VERSION)?;
        bounded_id(&self.transaction_id, "transaction_id")?;
        bounded_id(&self.attempt_id, "attempt_id")?;
        hex_sha256(&self.subject_digest, "subject_digest")?;
        hex_sha256(&self.stage_set_digest, "stage_set_digest")?;
        let bytes = canonical_bytes(self)?;
        Ok(domain_digest(STAGE_SET_DIGEST_DOMAIN, &bytes))
    }
}

/// Fold one ordered receipt set into its single terminal outcome, enforcing
/// every composition rule from #10243/#11099:
///
/// - exact transaction/attempt/subject/schema binding on every receipt;
/// - no duplicate, unknown, out-of-DAG-order, or unauthorized results;
/// - predecessor digests recomputed from THIS validated chain, never trusted;
/// - cancelled/timed-out/instrument-failed/failing mandatory evidence blocks
///   all downstream success;
/// - producer-declared completeness is never authority (instrument state is
///   checked against the claimed result);
/// - explicit local development can never terminate as an install claim.
pub fn fold_terminal_outcome(
    input: FanInInput<'_>,
) -> ContractResult<TerminalStandaloneInstallOutcome> {
    input.dag.validate()?;
    if input.mode != InstallMode::ExplicitLocalDevelopment {
        let terminal_stage = match input.operation {
            InstallOperation::Uninstall => StageId::Uninstall,
            _ => StageId::InstalledTransition,
        };
        if !input.dag.nodes.iter().any(|node| node.stage_id == terminal_stage) {
            return violation(
                ContractViolation::MissingRequiredStage,
                format!(
                    "{} operation requires a {} terminal stage",
                    input.operation.as_str(),
                    terminal_stage.as_str()
                ),
            );
        }
    }
    if input.dag.mode != input.mode {
        return violation(ContractViolation::ModeMismatch, "fold mode disagrees with the DAG mode");
    }
    bounded_id(input.transaction_id, "transaction_id")?;
    bounded_id(input.attempt_id, "attempt_id")?;
    hex_sha256(input.subject_digest, "subject_digest")?;
    // The supplied subject is re-validated here, never trusted: its
    // recomputed digest must equal the digest every receipt cites, or the
    // whole fan-in composes against an unsettled identity.
    let settled_subject_digest = input.subject.validate()?;
    if settled_subject_digest != input.subject_digest {
        return violation(
            ContractViolation::SubjectDigestMismatch,
            format!(
                "the settled subject digests to {} but receipts bind {}",
                head(&settled_subject_digest),
                head(input.subject_digest)
            ),
        );
    }

    let mut validated_chain: Vec<(StageId, String)> = Vec::new();
    let mut executed: BTreeSet<StageId> = BTreeSet::new();
    let mut last_position: Option<usize> = None;
    let mut blocked: Option<(StageId, StageReceipt)> = None;

    for receipt in input.receipts {
        let digest = receipt.validate()?;
        if receipt.transaction_id != input.transaction_id {
            return violation(
                ContractViolation::TransactionMismatch,
                format!(
                    "receipt for stage {} binds another transaction",
                    receipt.stage_id.as_str()
                ),
            );
        }
        if receipt.attempt_id != input.attempt_id {
            return violation(
                ContractViolation::AttemptMismatch,
                format!(
                    "receipt for stage {} binds stale attempt {}; this fold composes {} only",
                    receipt.stage_id.as_str(),
                    head(&receipt.attempt_id),
                    head(input.attempt_id)
                ),
            );
        }
        if receipt.subject_digest != input.subject_digest {
            return violation(
                ContractViolation::SubjectDigestMismatch,
                format!(
                    "receipt for stage {} binds subject {} instead of {}",
                    receipt.stage_id.as_str(),
                    head(&receipt.subject_digest),
                    head(input.subject_digest)
                ),
            );
        }
        // Receipt↔subject policy binding (#11099): policy identities are
        // recomputed from the settled subject and compared exactly; a
        // receipt bound to another policy (or carrying one where the stage
        // bears none) cannot compose.
        let expected_policies = expected_receipt_policies(input.subject, receipt.stage_id);
        let actual_policies = (
            &receipt.integrity_policy_id,
            &receipt.provenance_policy_id,
            &receipt.toolchain_policy_id,
        );
        if actual_policies.0.as_ref() != expected_policies.0.as_ref()
            || actual_policies.1.as_ref() != expected_policies.1.as_ref()
            || actual_policies.2.as_ref() != expected_policies.2.as_ref()
        {
            return violation(
                ContractViolation::PolicyIdentityMismatch,
                format!(
                    "receipt for stage {} is bound to policies {:?}/{:?}/{:?} but the settled \
                     subject authorizes {:?}/{:?}/{:?}",
                    receipt.stage_id.as_str(),
                    receipt.integrity_policy_id,
                    receipt.provenance_policy_id,
                    receipt.toolchain_policy_id,
                    expected_policies.0,
                    expected_policies.1,
                    expected_policies.2,
                ),
            );
        }
        let position = input.dag.position_of(receipt.stage_id).ok_or_else(|| {
            ContractError::new(
                ContractViolation::UnauthorizedStageApplicability,
                format!("receipt stage {} is absent from the DAG", receipt.stage_id.as_str()),
            )
        })?;
        if last_position.is_some_and(|previous| position < previous) {
            return violation(
                ContractViolation::PredecessorMismatch,
                format!(
                    "receipt stage {} arrived before an already validated later stage",
                    receipt.stage_id.as_str()
                ),
            );
        }
        last_position = Some(position);
        if !executed.insert(receipt.stage_id) {
            return violation(
                ContractViolation::DuplicateStageResult,
                format!("stage {} produced more than one receipt", receipt.stage_id.as_str()),
            );
        }
        // Authorization and evidence shape for skips: a not_applicable
        // result is valid only on positively authorized DAG rows, and a
        // skipped stage consumes no predecessor evidence — it cites none.
        if receipt.result == StageResult::NotApplicable {
            let authorized = input
                .dag
                .nodes
                .get(position)
                .is_some_and(|node| node.applicability == Applicability::NotApplicable);
            if !authorized {
                return violation(
                    ContractViolation::UnauthorizedStageApplicability,
                    format!(
                        "stage {} is required by the DAG and cannot be skipped as not_applicable",
                        receipt.stage_id.as_str()
                    ),
                );
            }
            if !receipt.predecessor_receipt_digests.is_empty() {
                return violation(
                    ContractViolation::PredecessorMismatch,
                    format!(
                        "skipped stage {} must cite no predecessors; it consumed nothing",
                        receipt.stage_id.as_str()
                    ),
                );
            }
            validated_chain.push((receipt.stage_id, digest));
            continue;
        }
        // Predecessor digests are recomputed from THIS validated chain and
        // compared exactly; producer-cited chains are never authority.
        let expected_predecessors: Vec<String> = input
            .dag
            .nodes
            .get(position)
            .ok_or_else(|| {
                ContractError::new(
                    ContractViolation::UnauthorizedStageApplicability,
                    "receipt position disappeared while validating the DAG",
                )
            })?
            .predecessors
            .iter()
            .map(|predecessor| {
                validated_chain
                    .iter()
                    .find(|(stage, _)| stage == predecessor)
                    .map(|(_, digest)| digest.clone())
                    .ok_or_else(|| {
                        ContractError::new(
                            ContractViolation::PredecessorMismatch,
                            format!(
                                "receipt stage {} arrived before predecessor {}",
                                receipt.stage_id.as_str(),
                                predecessor.as_str()
                            ),
                        )
                    })
            })
            .collect::<ContractResult<Vec<_>>>()?;
        if receipt.predecessor_receipt_digests != expected_predecessors {
            return violation(
                ContractViolation::PredecessorMismatch,
                format!(
                    "receipt for stage {} cites predecessors outside this validated chain",
                    receipt.stage_id.as_str()
                ),
            );
        }
        // Producer-declared completeness is never authority.
        if receipt.result == StageResult::Succeeded
            && receipt.instrument_completeness != InstrumentCompleteness::Complete
        {
            return violation(
                ContractViolation::InstrumentIncompleteSuccess,
                format!(
                    "stage {} claimed success with {} instrument evidence",
                    receipt.stage_id.as_str(),
                    receipt.instrument_completeness.as_str()
                ),
            );
        }
        if blocked.is_some() && receipt.result == StageResult::Succeeded {
            return violation(
                ContractViolation::SuccessAfterTerminalEvidence,
                format!(
                    "stage {} succeeded after terminal evidence at {}; cancellation, \
                     timeout, instrument failure, and failure block downstream \
                     authorization",
                    receipt.stage_id.as_str(),
                    blocked.as_ref().map(|(stage, _)| stage.as_str()).unwrap_or("?")
                ),
            );
        }
        if blocked.is_none() && receipt.result != StageResult::Succeeded {
            blocked = Some((receipt.stage_id, receipt.clone()));
        }
        validated_chain.push((receipt.stage_id, digest));
    }

    // Every required stage must have contributed evidence — unless terminal
    // evidence already stopped authorization downstream, in which case later
    // required stages legitimately produced nothing.
    if blocked.is_none() {
        for required in input.dag.required_stages() {
            if !executed.contains(&required) {
                return violation(
                    ContractViolation::MissingRequiredStage,
                    format!("required stage {} produced no evidence", required.as_str()),
                );
            }
        }
    }

    let ceiling_for = |stage: StageId| match stage {
        StageId::ResolveSubject => SideEffectCeiling::ResolveOnly,
        StageId::Uninstall => SideEffectCeiling::RemovalCompleted,
        StageId::Transport | StageId::ChecksumIntegrity | StageId::Provenance => {
            SideEffectCeiling::TransportArtifacts
        }
        StageId::ArchiveManifestAndStaging
        | StageId::ExecutableObservation
        | StageId::SourceBuild => SideEffectCeiling::Staged,
        StageId::Promotion => SideEffectCeiling::PromotionReached,
        StageId::PathPersistence | StageId::FreshProcessObservation => {
            SideEffectCeiling::PathPersisted
        }
        StageId::InstalledTransition => SideEffectCeiling::InstalledClaim,
    };

    let mut ordered_executed: Vec<StageId> = executed.into_iter().collect();
    ordered_executed.sort_by_key(|stage| input.dag.position_of(*stage).unwrap_or_default());
    let installed_transition_green = ordered_executed.contains(&StageId::InstalledTransition);
    if blocked.is_none() && input.mode != InstallMode::ExplicitLocalDevelopment {
        let terminal_stage = match input.operation {
            InstallOperation::Uninstall => StageId::Uninstall,
            _ => StageId::InstalledTransition,
        };
        if !ordered_executed.contains(&terminal_stage) {
            return violation(
                ContractViolation::MissingRequiredStage,
                format!(
                    "{} operation produced no {} terminal evidence",
                    input.operation.as_str(),
                    terminal_stage.as_str()
                ),
            );
        }
    }
    let blocked_terminal = blocked.is_some();

    let (result, terminal_stage, reason, next_action) = match blocked {
        Some((stage, receipt)) => {
            let result = match receipt.result {
                StageResult::Failed => TerminalResult::Failed,
                StageResult::Cancelled => TerminalResult::Cancelled,
                StageResult::TimedOut => TerminalResult::TimedOut,
                _ => TerminalResult::NotProven,
            };
            let action = if receipt.next_action == ActionClass::None {
                ActionClass::AbortInstall
            } else {
                receipt.next_action
            };
            (result, stage, receipt.reason, action)
        }
        None => {
            let terminal_stage =
                ordered_executed.last().copied().unwrap_or(StageId::ResolveSubject);
            if input.mode == InstallMode::ExplicitLocalDevelopment {
                // Green local-development evidence still cannot satisfy a
                // release/install claim.
                (
                    TerminalResult::NotProven,
                    terminal_stage,
                    ReasonFamily::LocalDevelopmentNonAuthoritative,
                    ActionClass::VerifyEnvironmentThenRetry,
                )
            } else {
                let result = match input.operation {
                    InstallOperation::Install => TerminalResult::Installed,
                    InstallOperation::Repair => TerminalResult::Repaired,
                    InstallOperation::Update => TerminalResult::Updated,
                    InstallOperation::Rollback => TerminalResult::RolledBack,
                    InstallOperation::Uninstall => TerminalResult::Uninstalled,
                };
                (result, terminal_stage, ReasonFamily::None, ActionClass::None)
            }
        }
    };

    // Candidate disposition (#11099), derived from the validated receipts
    // only: an authoritative disposition requires green evidence through the
    // installed transition; blocked folds and non-authoritative local
    // development stay unresolved rather than guessed.
    let candidate_disposition = if blocked_terminal
        || input.mode == InstallMode::ExplicitLocalDevelopment
        || !installed_transition_green
    {
        CandidateDisposition::Unresolved
    } else {
        match input.operation {
            InstallOperation::Install | InstallOperation::Repair | InstallOperation::Update => {
                CandidateDisposition::CurrentConfirmed
            }
            InstallOperation::Rollback => CandidateDisposition::Unresolved,
            InstallOperation::Uninstall => CandidateDisposition::NoneRemaining,
        }
    };

    let outcome = TerminalStandaloneInstallOutcome {
        schema_version: OUTCOME_SCHEMA_VERSION.to_string(),
        transaction_id: input.transaction_id.to_string(),
        attempt_id: input.attempt_id.to_string(),
        subject_digest: input.subject_digest.to_string(),
        result,
        terminal_stage,
        candidate_disposition,
        reason,
        next_action,
        side_effect_ceiling: ceiling_for(terminal_stage),
        stage_set_digest: {
            let mut joined = Vec::new();
            for (stage, digest) in &validated_chain {
                joined.extend_from_slice(stage.as_str().as_bytes());
                joined.push(b'\0');
                joined.extend_from_slice(digest.as_bytes());
            }
            domain_digest(STAGE_SET_DIGEST_DOMAIN, &joined)
        },
    };
    reject_private_output(&outcome, "terminal outcome")?;
    outcome.validate()?;
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// Built-in invariant check (CLI surface)
// ---------------------------------------------------------------------------

/// Run the built-in fail-closed invariant checks over internally constructed
/// canonical pipelines. Exits nonzero through `bail!` on any violation. This
/// is the smoke surface for later children; the full falsifier matrix lives
/// in this module's tests.
pub fn run_check() -> color_eyre::eyre::Result<()> {
    let checked = fixtures::run_canonical_pipelines()
        .map_err(|error| color_eyre::eyre::eyre!("canonical pipeline failed: {error}"))?;
    println!("standalone-transaction: {checked} canonical pipelines verified");
    Ok(())
}

#[cfg(test)]
mod tests;
