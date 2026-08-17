//! Opaque, bounded wire contract for lazy LSP resolve items.
//!
//! Resolve payloads are round-tripped through an untrusted client. This module
//! preserves a typed provider subject behind one canonical opaque token and an
//! injected authentication boundary. It deliberately does not own session key
//! material, provider semantics, or capability advertisement.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

/// Current resolve-envelope wire version.
pub const RESOLVE_ENVELOPE_VERSION: u16 = 1;

/// Prefix used for opaque resolve tokens.
pub const RESOLVE_ENVELOPE_TOKEN_PREFIX: &str = "perl-lsp.resolve.v1:";

/// Number of authentication-tag bytes carried by a resolve envelope.
pub const RESOLVE_AUTH_TAG_BYTES: usize = 32;

const DEFAULT_MAX_DECODED_BYTES: usize = 32 * 1024;
const DEFAULT_MAX_SUBJECT_BYTES: usize = 16 * 1024;
const DEFAULT_MAX_IDENTITY_BYTES: usize = 96;
const DEFAULT_MAX_CURRENTNESS_REFS: usize = 8;
const DEFAULT_MAX_JSON_DEPTH: usize = 32;

/// Resolve methods covered by the shared lazy-resolve envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResolveMethod {
    /// `workspace/symbol/resolve`.
    WorkspaceSymbol,
    /// `inlayHint/resolve`.
    InlayHint,
    /// `documentLink/resolve`.
    DocumentLink,
    /// `codeLens/resolve`.
    CodeLens,
    #[cfg(test)]
    /// Test-only extension seam.
    Synthetic,
}

impl ResolveMethod {
    /// Return the LSP wire method represented by this identifier.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::WorkspaceSymbol => "workspace/symbol/resolve",
            Self::InlayHint => "inlayHint/resolve",
            Self::DocumentLink => "documentLink/resolve",
            Self::CodeLens => "codeLens/resolve",
            #[cfg(test)]
            Self::Synthetic => "test/resolve",
        }
    }
}

/// Closed provider-family registry for lazy resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResolveFamily {
    /// Workspace-symbol resolve subjects.
    WorkspaceSymbol,
    /// Inlay-hint resolve subjects.
    InlayHint,
    /// Document-link resolve subjects.
    DocumentLink,
    /// Code-lens resolve subjects.
    CodeLens,
    #[cfg(test)]
    /// Test-only typed-family extension seam.
    Synthetic,
}

impl ResolveFamily {
    /// Return the only resolve method admitted for this family.
    #[must_use]
    pub const fn method(self) -> ResolveMethod {
        match self {
            Self::WorkspaceSymbol => ResolveMethod::WorkspaceSymbol,
            Self::InlayHint => ResolveMethod::InlayHint,
            Self::DocumentLink => ResolveMethod::DocumentLink,
            Self::CodeLens => ResolveMethod::CodeLens,
            #[cfg(test)]
            Self::Synthetic => ResolveMethod::Synthetic,
        }
    }
}

/// Stable currentness-reference classes retained by the common header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResolveCurrentnessKind {
    /// Open-document instance or generation.
    Document,
    /// Exact source revision or content generation.
    Source,
    /// Owning workspace-root generation.
    Root,
    /// Captured workspace or query-view generation.
    Workspace,
    /// Accepted configuration or provider-policy generation.
    Configuration,
}

/// Bounded replay disposition selected by the issuing provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResolveReplayDisposition {
    /// Valid only within the issuing server session.
    SessionBound,
    /// Valid only while every retained currentness reference remains current.
    CurrentSubjectBound,
}

/// A bounded, path-free reference to an identity owned elsewhere.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResolveIdentityRef(String);

impl ResolveIdentityRef {
    /// Construct a bounded opaque identity reference.
    ///
    /// References admit only ASCII letters, digits, `.`, `_`, `-`, and `:`.
    /// This keeps host paths, URIs, whitespace, and free-form prose out of the
    /// durable wire token.
    pub fn new(value: impl Into<String>) -> Result<Self, ResolveEnvelopeIssueError> {
        let reference = Self(value.into());
        reference.validate(DEFAULT_MAX_IDENTITY_BYTES).map_err(ResolveEnvelopeIssueError::from)?;
        Ok(reference)
    }

    /// Borrow the validated opaque spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self, max_bytes: usize) -> Result<(), HeaderValidationError> {
        if self.0.is_empty() || self.0.len() > max_bytes {
            return Err(HeaderValidationError::InvalidIdentityReference);
        }

        if !self
            .0
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
        {
            return Err(HeaderValidationError::InvalidIdentityReference);
        }

        Ok(())
    }
}

impl fmt::Debug for ResolveIdentityRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ResolveIdentityRef").field(&self.0).finish()
    }
}

/// One exact currentness reference carried by a resolve item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveCurrentnessRef {
    kind: ResolveCurrentnessKind,
    identity: ResolveIdentityRef,
}

impl ResolveCurrentnessRef {
    /// Construct a currentness reference.
    #[must_use]
    pub const fn new(kind: ResolveCurrentnessKind, identity: ResolveIdentityRef) -> Self {
        Self { kind, identity }
    }

    /// Currentness class.
    #[must_use]
    pub const fn kind(&self) -> ResolveCurrentnessKind {
        self.kind
    }

    /// Opaque identity owned by the corresponding lifecycle authority.
    #[must_use]
    pub const fn identity(&self) -> &ResolveIdentityRef {
        &self.identity
    }
}

/// Common versioned header authenticated with every provider subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveEnvelopeHeaderV1 {
    envelope_version: u16,
    method: ResolveMethod,
    family: ResolveFamily,
    server_session: ResolveIdentityRef,
    originating_operation: ResolveIdentityRef,
    originating_result: ResolveIdentityRef,
    effective_profile: ResolveIdentityRef,
    subject_version: u16,
    currentness: Vec<ResolveCurrentnessRef>,
    replay: ResolveReplayDisposition,
    issue_sequence: u64,
}

impl ResolveEnvelopeHeaderV1 {
    /// Build a header for one typed provider subject.
    #[allow(clippy::too_many_arguments)]
    pub fn for_subject<T: ResolveEnvelopeSubject>(
        server_session: ResolveIdentityRef,
        originating_operation: ResolveIdentityRef,
        originating_result: ResolveIdentityRef,
        effective_profile: ResolveIdentityRef,
        currentness: Vec<ResolveCurrentnessRef>,
        replay: ResolveReplayDisposition,
        issue_sequence: u64,
    ) -> Result<Self, ResolveEnvelopeIssueError> {
        let header = Self {
            envelope_version: RESOLVE_ENVELOPE_VERSION,
            method: T::FAMILY.method(),
            family: T::FAMILY,
            server_session,
            originating_operation,
            originating_result,
            effective_profile,
            subject_version: T::VERSION,
            currentness,
            replay,
            issue_sequence,
        };
        header
            .validate_for::<T>(&ResolveEnvelopeLimits::default())
            .map_err(ResolveEnvelopeIssueError::from)?;
        Ok(header)
    }

    /// Resolve method.
    #[must_use]
    pub const fn method(&self) -> ResolveMethod {
        self.method
    }

    /// Provider family.
    #[must_use]
    pub const fn family(&self) -> ResolveFamily {
        self.family
    }

    /// Issuing server-session identity.
    #[must_use]
    pub const fn server_session(&self) -> &ResolveIdentityRef {
        &self.server_session
    }

    /// Originating operation identity.
    #[must_use]
    pub const fn originating_operation(&self) -> &ResolveIdentityRef {
        &self.originating_operation
    }

    /// Originating parent-result identity.
    #[must_use]
    pub const fn originating_result(&self) -> &ResolveIdentityRef {
        &self.originating_result
    }

    /// Effective client/property profile identity.
    #[must_use]
    pub const fn effective_profile(&self) -> &ResolveIdentityRef {
        &self.effective_profile
    }

    /// Provider-subject schema version.
    #[must_use]
    pub const fn subject_version(&self) -> u16 {
        self.subject_version
    }

    /// Currentness references.
    #[must_use]
    pub fn currentness(&self) -> &[ResolveCurrentnessRef] {
        &self.currentness
    }

    /// Replay disposition.
    #[must_use]
    pub const fn replay(&self) -> ResolveReplayDisposition {
        self.replay
    }

    /// Session-local issue sequence.
    #[must_use]
    pub const fn issue_sequence(&self) -> u64 {
        self.issue_sequence
    }

    fn validate_for<T: ResolveEnvelopeSubject>(
        &self,
        limits: &ResolveEnvelopeLimits,
    ) -> Result<(), HeaderValidationError> {
        if self.envelope_version != RESOLVE_ENVELOPE_VERSION {
            return Err(HeaderValidationError::UnknownEnvelopeVersion(self.envelope_version));
        }
        if self.family != T::FAMILY
            || self.method != T::FAMILY.method()
            || self.method != self.family.method()
        {
            return Err(HeaderValidationError::WrongMethodOrFamily);
        }
        if self.subject_version != T::VERSION {
            return Err(HeaderValidationError::UnknownSubjectVersion(self.subject_version));
        }
        if self.issue_sequence == 0 {
            return Err(HeaderValidationError::InvalidIssueSequence);
        }

        self.server_session.validate(limits.max_identity_bytes)?;
        self.originating_operation.validate(limits.max_identity_bytes)?;
        self.originating_result.validate(limits.max_identity_bytes)?;
        self.effective_profile.validate(limits.max_identity_bytes)?;

        if self.currentness.len() > limits.max_currentness_refs {
            return Err(HeaderValidationError::TooManyCurrentnessReferences);
        }

        let mut seen = BTreeSet::new();
        for reference in &self.currentness {
            reference.identity.validate(limits.max_identity_bytes)?;
            if !seen.insert(reference.kind) {
                return Err(HeaderValidationError::DuplicateCurrentnessKind);
            }
        }

        Ok(())
    }
}

/// Provider-owned typed payload carried by a resolve envelope.
pub trait ResolveEnvelopeSubject: Serialize + DeserializeOwned {
    /// Closed resolve family.
    const FAMILY: ResolveFamily;
    /// Provider-subject schema version.
    const VERSION: u16;
}

/// Fixed-size authentication tag produced by the connection-owned authenticator.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResolveAuthTag(String);

impl ResolveAuthTag {
    /// Construct a tag from exactly 32 authenticator bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; RESOLVE_AUTH_TAG_BYTES]) -> Self {
        Self(encode_hex(&bytes))
    }

    fn decoded(&self) -> Option<[u8; RESOLVE_AUTH_TAG_BYTES]> {
        let decoded = decode_hex_exact(&self.0, RESOLVE_AUTH_TAG_BYTES)?;
        decoded.try_into().ok()
    }

    fn is_well_formed(&self) -> bool {
        self.decoded().is_some()
    }

    fn constant_time_eq(&self, other: &Self) -> bool {
        let Some(left) = self.decoded() else {
            return false;
        };
        let Some(right) = other.decoded() else {
            return false;
        };

        let mut difference = 0_u8;
        for (left_byte, right_byte) in left.iter().zip(right.iter()) {
            difference |= left_byte ^ right_byte;
        }
        difference == 0
    }
}

impl fmt::Debug for ResolveAuthTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResolveAuthTag([redacted])")
    }
}

/// Failure returned by the concrete connection-owned authenticator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ResolveAuthenticatorFailure {
    /// Required key or authenticator state is unavailable.
    #[error("resolve authenticator is unavailable")]
    Unavailable,
    /// The authenticator failed internally.
    #[error("resolve authenticator failed")]
    Internal,
}

/// Port implemented by #8342's concrete session-owned authenticator.
pub trait ResolveEnvelopeAuthenticator {
    /// Authenticate canonical unsigned envelope bytes.
    fn authenticate(
        &self,
        canonical_unsigned: &[u8],
    ) -> Result<ResolveAuthTag, ResolveAuthenticatorFailure>;
}

/// Resource ceilings for one resolve envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveEnvelopeLimits {
    max_decoded_bytes: usize,
    max_subject_bytes: usize,
    max_identity_bytes: usize,
    max_currentness_refs: usize,
    max_json_depth: usize,
}

impl Default for ResolveEnvelopeLimits {
    fn default() -> Self {
        Self {
            max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES,
            max_subject_bytes: DEFAULT_MAX_SUBJECT_BYTES,
            max_identity_bytes: DEFAULT_MAX_IDENTITY_BYTES,
            max_currentness_refs: DEFAULT_MAX_CURRENTNESS_REFS,
            max_json_depth: DEFAULT_MAX_JSON_DEPTH,
        }
    }
}

/// Opaque string placed in an LSP item's `data` field.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResolveEnvelopeToken(String);

impl ResolveEnvelopeToken {
    /// Parse the outer opaque token shape without authenticating it.
    pub fn parse(value: impl Into<String>) -> Result<Self, ResolveEnvelopeRejection> {
        let token = Self(value.into());
        token.validate_outer_shape(&ResolveEnvelopeLimits::default())?;
        Ok(token)
    }

    /// Borrow the wire spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the token into its wire spelling.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    fn validate_outer_shape(
        &self,
        limits: &ResolveEnvelopeLimits,
    ) -> Result<(), ResolveEnvelopeRejection> {
        let Some(encoded) = self.0.strip_prefix(RESOLVE_ENVELOPE_TOKEN_PREFIX) else {
            return Err(ResolveEnvelopeRejection::Malformed);
        };
        if encoded.is_empty()
            || !encoded.len().is_multiple_of(2)
            || encoded.len() / 2 > limits.max_decoded_bytes
        {
            return Err(ResolveEnvelopeRejection::OversizedOrResourceBound);
        }
        if !encoded.bytes().all(is_lower_hex) {
            return Err(ResolveEnvelopeRejection::Malformed);
        }
        Ok(())
    }
}

impl fmt::Debug for ResolveEnvelopeToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolveEnvelopeToken")
            .field("wire_bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Successfully authenticated typed envelope.
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedResolveEnvelope<T> {
    header: ResolveEnvelopeHeaderV1,
    subject: T,
}

impl<T> ValidatedResolveEnvelope<T> {
    /// Authenticated common header.
    #[must_use]
    pub const fn header(&self) -> &ResolveEnvelopeHeaderV1 {
        &self.header
    }

    /// Authenticated provider subject.
    #[must_use]
    pub const fn subject(&self) -> &T {
        &self.subject
    }

    /// Consume the envelope into its parts.
    #[must_use]
    pub fn into_parts(self) -> (ResolveEnvelopeHeaderV1, T) {
        (self.header, self.subject)
    }
}

/// Errors while issuing a server-owned token.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ResolveEnvelopeIssueError {
    /// One identity reference was empty, too long, or contained forbidden bytes.
    #[error("resolve envelope identity reference is invalid")]
    InvalidIdentityReference,
    /// Currentness references exceeded the fixed limit.
    #[error("resolve envelope contains too many currentness references")]
    TooManyCurrentnessReferences,
    /// The same currentness class appeared more than once.
    #[error("resolve envelope repeats a currentness class")]
    DuplicateCurrentnessKind,
    /// Issue sequence zero is reserved and invalid.
    #[error("resolve envelope issue sequence is invalid")]
    InvalidIssueSequence,
    /// Header method, family, or version did not match the typed subject.
    #[error("resolve envelope header does not match its typed subject")]
    HeaderSubjectMismatch,
    /// Canonical serialization failed.
    #[error("resolve envelope serialization failed")]
    Serialization,
    /// Subject or complete envelope exceeded a resource ceiling.
    #[error("resolve envelope exceeds a resource ceiling")]
    OversizedOrResourceBound,
    /// The concrete authenticator failed.
    #[error("resolve envelope authenticator failed")]
    AuthenticatorFailure,
    /// The authenticator returned a malformed fixed-size tag.
    #[error("resolve envelope authenticator returned an invalid tag")]
    InvalidAuthenticatorTag,
}

impl From<HeaderValidationError> for ResolveEnvelopeIssueError {
    fn from(error: HeaderValidationError) -> Self {
        match error {
            HeaderValidationError::InvalidIdentityReference => Self::InvalidIdentityReference,
            HeaderValidationError::TooManyCurrentnessReferences => {
                Self::TooManyCurrentnessReferences
            }
            HeaderValidationError::DuplicateCurrentnessKind => Self::DuplicateCurrentnessKind,
            HeaderValidationError::InvalidIssueSequence => Self::InvalidIssueSequence,
            HeaderValidationError::UnknownEnvelopeVersion(_)
            | HeaderValidationError::UnknownSubjectVersion(_)
            | HeaderValidationError::WrongMethodOrFamily => Self::HeaderSubjectMismatch,
        }
    }
}

/// Common validation rejection before provider-specific work.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ResolveEnvelopeRejection {
    /// Token belongs to another server session.
    #[error("resolve envelope belongs to another server session")]
    ForeignSession,
    /// Method or family does not match the typed resolver.
    #[error("resolve envelope method or family is wrong")]
    WrongMethodOrFamily,
    /// Envelope version is unknown.
    #[error("resolve envelope version {0} is unknown")]
    UnknownEnvelopeVersion(u16),
    /// Provider-subject version is unknown.
    #[error("resolve subject version {0} is unknown")]
    UnknownSubjectVersion(u16),
    /// Token could not be parsed or had an invalid outer encoding.
    #[error("resolve envelope is malformed")]
    Malformed,
    /// Token bytes were valid JSON but not the one canonical representation.
    #[error("resolve envelope is not canonically encoded")]
    NonCanonical,
    /// A fixed resource ceiling was exceeded.
    #[error("resolve envelope exceeds a resource ceiling")]
    OversizedOrResourceBound,
    /// Authentication tag was invalid.
    #[error("resolve envelope integrity validation failed")]
    IntegrityFailure,
    /// Replay is no longer admitted by the issuing lifecycle owner.
    #[error("resolve envelope replay is no longer admitted")]
    ExpiredOrDisallowedReplay,
    /// Authenticator could not complete validation.
    #[error("resolve envelope authenticator failed")]
    InstrumentFailure,
}

#[derive(Debug, Error)]
enum HeaderValidationError {
    #[error("invalid identity reference")]
    InvalidIdentityReference,
    #[error("too many currentness references")]
    TooManyCurrentnessReferences,
    #[error("duplicate currentness class")]
    DuplicateCurrentnessKind,
    #[error("invalid issue sequence")]
    InvalidIssueSequence,
    #[error("unknown envelope version")]
    UnknownEnvelopeVersion(u16),
    #[error("unknown subject version")]
    UnknownSubjectVersion(u16),
    #[error("wrong method or family")]
    WrongMethodOrFamily,
}

impl From<HeaderValidationError> for ResolveEnvelopeRejection {
    fn from(error: HeaderValidationError) -> Self {
        match error {
            HeaderValidationError::UnknownEnvelopeVersion(version) => {
                Self::UnknownEnvelopeVersion(version)
            }
            HeaderValidationError::UnknownSubjectVersion(version) => {
                Self::UnknownSubjectVersion(version)
            }
            HeaderValidationError::WrongMethodOrFamily => Self::WrongMethodOrFamily,
            HeaderValidationError::InvalidIdentityReference
            | HeaderValidationError::DuplicateCurrentnessKind
            | HeaderValidationError::InvalidIssueSequence => Self::Malformed,
            HeaderValidationError::TooManyCurrentnessReferences => Self::OversizedOrResourceBound,
        }
    }
}

#[derive(Serialize)]
struct UnsignedResolveEnvelopeRef<'a, T> {
    header: &'a ResolveEnvelopeHeaderV1,
    subject: &'a T,
}

#[derive(Serialize, Deserialize)]
struct SignedResolveEnvelope<T> {
    header: ResolveEnvelopeHeaderV1,
    subject: T,
    tag: ResolveAuthTag,
}

/// Canonical issue/validation entry point for opaque resolve tokens.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveEnvelopeCodec {
    limits: ResolveEnvelopeLimits,
}

impl ResolveEnvelopeCodec {
    /// Issue a canonical opaque token for one typed provider subject.
    pub fn issue<T, A>(
        &self,
        header: ResolveEnvelopeHeaderV1,
        subject: T,
        authenticator: &A,
    ) -> Result<ResolveEnvelopeToken, ResolveEnvelopeIssueError>
    where
        T: ResolveEnvelopeSubject,
        A: ResolveEnvelopeAuthenticator + ?Sized,
    {
        header.validate_for::<T>(&self.limits).map_err(ResolveEnvelopeIssueError::from)?;

        let subject_bytes =
            canonical_json_bytes(&subject).map_err(|_| ResolveEnvelopeIssueError::Serialization)?;
        if subject_bytes.len() > self.limits.max_subject_bytes {
            return Err(ResolveEnvelopeIssueError::OversizedOrResourceBound);
        }

        let unsigned = UnsignedResolveEnvelopeRef { header: &header, subject: &subject };
        let unsigned_bytes = canonical_json_bytes(&unsigned)
            .map_err(|_| ResolveEnvelopeIssueError::Serialization)?;
        if unsigned_bytes.len() > self.limits.max_decoded_bytes {
            return Err(ResolveEnvelopeIssueError::OversizedOrResourceBound);
        }

        let tag = authenticator
            .authenticate(&unsigned_bytes)
            .map_err(|_| ResolveEnvelopeIssueError::AuthenticatorFailure)?;
        if !tag.is_well_formed() {
            return Err(ResolveEnvelopeIssueError::InvalidAuthenticatorTag);
        }

        let signed = SignedResolveEnvelope { header, subject, tag };
        let signed_bytes =
            canonical_json_bytes(&signed).map_err(|_| ResolveEnvelopeIssueError::Serialization)?;
        if signed_bytes.len() > self.limits.max_decoded_bytes {
            return Err(ResolveEnvelopeIssueError::OversizedOrResourceBound);
        }

        Ok(ResolveEnvelopeToken(format!(
            "{RESOLVE_ENVELOPE_TOKEN_PREFIX}{}",
            encode_hex(&signed_bytes)
        )))
    }

    /// Decode, canonicalize, authenticate, and type-check one opaque token.
    pub fn validate<T, A>(
        &self,
        token: &ResolveEnvelopeToken,
        expected_session: &ResolveIdentityRef,
        authenticator: &A,
    ) -> Result<ValidatedResolveEnvelope<T>, ResolveEnvelopeRejection>
    where
        T: ResolveEnvelopeSubject,
        A: ResolveEnvelopeAuthenticator + ?Sized,
    {
        token.validate_outer_shape(&self.limits)?;
        expected_session
            .validate(self.limits.max_identity_bytes)
            .map_err(ResolveEnvelopeRejection::from)?;

        let encoded = token
            .0
            .strip_prefix(RESOLVE_ENVELOPE_TOKEN_PREFIX)
            .ok_or(ResolveEnvelopeRejection::Malformed)?;
        let decoded = decode_hex_bounded(encoded, self.limits.max_decoded_bytes)?;

        let mut value: Value =
            serde_json::from_slice(&decoded).map_err(|_| ResolveEnvelopeRejection::Malformed)?;
        if json_depth(&value) > self.limits.max_json_depth {
            return Err(ResolveEnvelopeRejection::OversizedOrResourceBound);
        }
        canonicalize_json(&mut value);
        let canonical_signed =
            serde_json::to_vec(&value).map_err(|_| ResolveEnvelopeRejection::Malformed)?;
        if canonical_signed != decoded {
            return Err(ResolveEnvelopeRejection::NonCanonical);
        }

        let signed: SignedResolveEnvelope<T> =
            serde_json::from_value(value).map_err(|_| ResolveEnvelopeRejection::Malformed)?;
        signed.header.validate_for::<T>(&self.limits).map_err(ResolveEnvelopeRejection::from)?;

        if signed.header.server_session != *expected_session {
            return Err(ResolveEnvelopeRejection::ForeignSession);
        }
        if !signed.tag.is_well_formed() {
            return Err(ResolveEnvelopeRejection::Malformed);
        }

        let subject_bytes = canonical_json_bytes(&signed.subject)
            .map_err(|_| ResolveEnvelopeRejection::Malformed)?;
        if subject_bytes.len() > self.limits.max_subject_bytes {
            return Err(ResolveEnvelopeRejection::OversizedOrResourceBound);
        }

        let unsigned =
            UnsignedResolveEnvelopeRef { header: &signed.header, subject: &signed.subject };
        let unsigned_bytes =
            canonical_json_bytes(&unsigned).map_err(|_| ResolveEnvelopeRejection::Malformed)?;
        let expected_tag = authenticator
            .authenticate(&unsigned_bytes)
            .map_err(|_| ResolveEnvelopeRejection::InstrumentFailure)?;
        if !expected_tag.is_well_formed() {
            return Err(ResolveEnvelopeRejection::InstrumentFailure);
        }
        if !signed.tag.constant_time_eq(&expected_tag) {
            return Err(ResolveEnvelopeRejection::IntegrityFailure);
        }

        Ok(ValidatedResolveEnvelope { header: signed.header, subject: signed.subject })
    }
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut json = serde_json::to_value(value)?;
    canonicalize_json(&mut json);
    serde_json::to_vec(&json)
}

fn canonicalize_json(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                canonicalize_json(value);
            }
        }
        Value::Object(object) => {
            let old = std::mem::take(object);
            let mut entries: Vec<_> = old.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));

            let mut canonical = Map::new();
            for (key, mut value) in entries {
                canonicalize_json(&mut value);
                canonical.insert(key, value);
            }
            *object = canonical;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(json_depth).max().unwrap_or_default(),
        Value::Object(object) => 1 + object.values().map(json_depth).max().unwrap_or_default(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_hex_bounded(
    encoded: &str,
    max_decoded_bytes: usize,
) -> Result<Vec<u8>, ResolveEnvelopeRejection> {
    if !encoded.len().is_multiple_of(2) || encoded.len() / 2 > max_decoded_bytes {
        return Err(ResolveEnvelopeRejection::OversizedOrResourceBound);
    }
    if !encoded.bytes().all(is_lower_hex) {
        return Err(ResolveEnvelopeRejection::Malformed);
    }

    let mut decoded = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let high = lower_hex_value(pair[0]).ok_or(ResolveEnvelopeRejection::Malformed)?;
        let low = lower_hex_value(pair[1]).ok_or(ResolveEnvelopeRejection::Malformed)?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn decode_hex_exact(encoded: &str, expected_bytes: usize) -> Option<Vec<u8>> {
    if encoded.len() != expected_bytes * 2 || !encoded.bytes().all(is_lower_hex) {
        return None;
    }

    let mut decoded = Vec::with_capacity(expected_bytes);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let high = lower_hex_value(pair[0])?;
        let low = lower_hex_value(pair[1])?;
        decoded.push((high << 4) | low);
    }
    Some(decoded)
}

const fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}

const fn lower_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
