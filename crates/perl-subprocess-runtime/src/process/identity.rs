//! Identity, privacy, and authorization-reference primitives.
//!
//! Everything here is a value type. Nothing in this module reads the ambient
//! environment, touches the filesystem, or resolves an executable: a plan
//! carries the identities its owner already established, and the validator
//! decides whether they are sufficient.

use std::fmt;
use std::path::{Path, PathBuf};

use super::encoding::{CanonicalEncoder, Fingerprint, PathFingerprint};

/// A monotonically versioned schema identity.
///
/// Any change to the *meaning* of a domain field, discriminant, or canonical
/// encoding requires moving [`super::PROCESS_DOMAIN_SCHEMA_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Construct a schema version.
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// The raw version number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// A filesystem path that must not appear in any public identity or receipt.
///
/// `Debug` is redacted deliberately: a leaked log line is the most common way
/// a private absolute path escapes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrivatePath(PathBuf);

impl PrivatePath {
    /// Wrap a path as private.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Borrow the underlying path.
    ///
    /// Callers that expose the result publicly are responsible for the leak;
    /// the name is deliberately awkward.
    pub fn expose(&self) -> &Path {
        &self.0
    }

    /// The public stand-in for this path.
    pub fn fingerprint(&self) -> PathFingerprint {
        PathFingerprint::of_str(&self.0.to_string_lossy())
    }

    /// Whether the path is absolute.
    pub fn is_absolute(&self) -> bool {
        self.0.is_absolute()
    }
}

impl fmt::Debug for PrivatePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivatePath(<redacted:{}>)", self.fingerprint())
    }
}

/// A value that may carry a secret and must never be encoded, fingerprinted,
/// or rendered.
///
/// Unlike [`PrivatePath`], a secret contributes **nothing** to any canonical
/// encoding — not even a fingerprint — because a fingerprint of a low-entropy
/// secret is a guessable secret.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    /// Wrap a value as secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the underlying value.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

/// Bytes fed to a child's stdin, treated as private input.
#[derive(Clone, PartialEq, Eq)]
pub struct PrivateBytes(Vec<u8>);

impl PrivateBytes {
    /// Wrap bytes as private.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Borrow the underlying bytes.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The number of private bytes, which is not itself private.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for PrivateBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateBytes(<redacted:{} bytes>)", self.0.len())
    }
}

macro_rules! correlation_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Construct the identifier.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Whether the identifier is blank.
            pub fn is_blank(&self) -> bool {
                self.0.trim().is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

correlation_id! {
    /// Stable identity of a plan, correlating a plan with its result.
    ///
    /// A correlation label only. It carries no authority: see
    /// [`OwnerDomain`] and [`AuthorizationEvidence`] for the fields that do.
    PlanId
}

correlation_id! {
    /// Stable identity of one start attempt.
    RunId
}

correlation_id! {
    /// Stable identity of the domain operation the plan serves.
    OperationId
}

/// The domain that owns the *meaning* of a plan's operation.
///
/// A closed enum on purpose: a caller-supplied owner string must never become
/// policy authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerDomain {
    /// Running a user-selected Perl file.
    RunFile,
    /// External formatter integration (for example `perltidy`).
    ExternalFormatter,
    /// External critic integration.
    ExternalCritic,
    /// Debug adapter launch and helper processes.
    DebugAdapter,
    /// Real-Perl differential oracle probes.
    RealPerlOracle,
    /// Upstream Perl test harness children.
    UpstreamHarness,
    /// Explicit compile-only service (`perl -c`).
    CompileService,
    /// Repository test execution.
    TestExecution,
    /// Packaged-artifact release smoke.
    ReleaseSmoke,
    /// The contained legacy adapter; see [`crate::process::legacy`].
    LegacyAdapter,
}

impl OwnerDomain {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::RunFile => 0,
            Self::ExternalFormatter => 1,
            Self::ExternalCritic => 2,
            Self::DebugAdapter => 3,
            Self::RealPerlOracle => 4,
            Self::UpstreamHarness => 5,
            Self::CompileService => 6,
            Self::TestExecution => 7,
            Self::ReleaseSmoke => 8,
            Self::LegacyAdapter => 9,
        }
    }
}

/// The execution *shape* a plan requires, independent of its owning domain.
///
/// Profiles exist so that the validator can demand exactness without knowing
/// anything about formatters, debuggers, or test runners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionProfile {
    /// One-shot execution with an exact deadline and no interactive stdin.
    ///
    /// This is the profile the Linux supervisor lane is being built for; it
    /// implies nothing about interactive, Windows, or macOS behavior.
    LinuxOneShot,
    /// A long-lived session with caller-driven stdin and cancellation.
    InteractiveSession,
    /// A probe that must not inherit ambient environment at all.
    HermeticProbe,
    /// A bounded smoke of a packaged artifact during release preparation.
    ReleaseArtifactSmoke,
}

impl ExecutionProfile {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::LinuxOneShot => 0,
            Self::InteractiveSession => 1,
            Self::HermeticProbe => 2,
            Self::ReleaseArtifactSmoke => 3,
        }
    }

    /// Whether the profile requires an exact working directory.
    pub fn requires_exact_cwd(self) -> bool {
        matches!(self, Self::LinuxOneShot | Self::HermeticProbe)
    }

    /// Whether the profile requires a wall-clock deadline.
    pub fn requires_deadline(self) -> bool {
        matches!(self, Self::LinuxOneShot | Self::HermeticProbe | Self::ReleaseArtifactSmoke)
    }

    /// Whether the profile permits a caller-streamed stdin channel.
    pub fn permits_streamed_stdin(self) -> bool {
        matches!(self, Self::InteractiveSession)
    }

    /// Whether the profile requires the run to be cancellable.
    pub fn requires_cancellation(self) -> bool {
        matches!(self, Self::InteractiveSession)
    }

    /// Whether the profile requires a root subject identity.
    pub fn requires_root_identity(self) -> bool {
        !matches!(self, Self::ReleaseArtifactSmoke)
    }
}

/// How a plan's executable was resolved to something spawnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableResolution {
    /// Not yet resolved. Never startable through the production port.
    Unresolved,
    /// Resolved by the owner to an exact absolute path.
    Resolved {
        /// The absolute path, private.
        path: PrivatePath,
        /// How the owner arrived at that path.
        provenance: ResolutionProvenance,
    },
}

/// How an executable path was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionProvenance {
    /// An absolute path supplied by explicit configuration.
    ConfiguredAbsolutePath,
    /// An absolute-directory search that excluded the working directory.
    AbsoluteDirectorySearch,
    /// A path derived from a declared, validated workspace root.
    DeclaredWorkspaceRoot,
    /// Whatever the operating system would find at spawn time.
    ///
    /// Rejected by the validator: post-validation ambient lookup is exactly
    /// the binary-planting hole the crate's Windows resolver already closes.
    AmbientLookup,
}

impl ResolutionProvenance {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::ConfiguredAbsolutePath => 0,
            Self::AbsoluteDirectorySearch => 1,
            Self::DeclaredWorkspaceRoot => 2,
            Self::AmbientLookup => 3,
        }
    }
}

/// The identity of the program a plan will execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableIdentity {
    logical_name: String,
    resolution: ExecutableResolution,
}

impl ExecutableIdentity {
    /// An executable whose resolution has not been performed.
    pub fn unresolved(logical_name: impl Into<String>) -> Self {
        Self { logical_name: logical_name.into(), resolution: ExecutableResolution::Unresolved }
    }

    /// An executable the owner already resolved to an absolute path.
    pub fn resolved(
        logical_name: impl Into<String>,
        path: PrivatePath,
        provenance: ResolutionProvenance,
    ) -> Self {
        Self {
            logical_name: logical_name.into(),
            resolution: ExecutableResolution::Resolved { path, provenance },
        }
    }

    /// The public logical name (for example `perl`).
    pub fn logical_name(&self) -> &str {
        &self.logical_name
    }

    /// How the executable was resolved.
    pub fn resolution(&self) -> &ExecutableResolution {
        &self.resolution
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("executable");
        encoder.text(&self.logical_name);
        match &self.resolution {
            ExecutableResolution::Unresolved => {
                encoder.variant(0);
                encoder.absent();
            }
            ExecutableResolution::Resolved { path, provenance } => {
                encoder.variant(1);
                // The path itself is private; only its fingerprint is public.
                encoder.nested_fingerprint(path.fingerprint().fingerprint());
                encoder.variant(provenance.discriminant());
            }
        }
    }
}

/// The working directory a plan runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdPolicy {
    /// Inherit whatever directory the supervisor happens to run in.
    ///
    /// Rejected for profiles that require exactness.
    InheritAmbient,
    /// An exact directory chosen by the plan's owner.
    ExactDirectory(PrivatePath),
}

impl CwdPolicy {
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("cwd");
        match self {
            Self::InheritAmbient => {
                encoder.variant(0);
                encoder.absent();
            }
            Self::ExactDirectory(path) => {
                encoder.variant(1);
                encoder.nested_fingerprint(path.fingerprint().fingerprint());
            }
        }
    }
}

/// How current a referenced piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceFreshness {
    /// Established against the state the plan will execute against.
    Current,
    /// Known to predate the current state.
    Stale,
    /// Freshness was never established.
    Unknown,
}

impl EvidenceFreshness {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::Current => 0,
            Self::Stale => 1,
            Self::Unknown => 2,
        }
    }
}

/// An opaque reference to evidence owned by another authority.
///
/// The process domain deliberately does not interpret these strings: source,
/// root, configuration, and toolchain semantics belong to their own owners.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubjectReference {
    reference: String,
    freshness: EvidenceFreshness,
}

impl SubjectReference {
    /// Construct a subject reference.
    pub fn new(reference: impl Into<String>, freshness: EvidenceFreshness) -> Self {
        Self { reference: reference.into(), freshness }
    }

    /// The opaque reference text.
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// How current the reference is.
    pub fn freshness(&self) -> EvidenceFreshness {
        self.freshness
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.text(&self.reference);
        encoder.variant(self.freshness.discriminant());
    }
}

/// The exact subject a plan executes against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubjectIdentity {
    /// The workspace or project root the operation belongs to.
    pub root: Option<SubjectReference>,
    /// The source document, when the operation is source-backed.
    pub source: Option<SubjectReference>,
    /// The configuration generation in force.
    pub configuration: Option<SubjectReference>,
    /// The toolchain or interpreter identity in force.
    pub toolchain: Option<SubjectReference>,
}

impl SubjectIdentity {
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("subject");
        for field in [&self.root, &self.source, &self.configuration, &self.toolchain] {
            match field {
                None => {
                    encoder.absent();
                }
                Some(reference) => reference.encode(encoder),
            }
        }
    }

    pub(crate) fn references(&self) -> impl Iterator<Item = &SubjectReference> {
        [&self.root, &self.source, &self.configuration, &self.toolchain].into_iter().flatten()
    }
}

/// How strongly an execution was authorized.
///
/// The process domain records the strength it was handed; it never derives
/// authority from workspace opening, client assertions, or ambient values.
/// The execution-authorization programme owns what these mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthorizationStrength {
    /// A person took an explicit action that requested this execution.
    ExplicitUserAction,
    /// A reviewed workspace-trust policy admitted this execution class.
    TrustedWorkspacePolicy,
    /// The plan takes no ambient input, so no trust decision is required.
    HermeticNoAmbientInput,
    /// No authorization was established.
    NotProven,
}

impl AuthorizationStrength {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::ExplicitUserAction => 0,
            Self::TrustedWorkspacePolicy => 1,
            Self::HermeticNoAmbientInput => 2,
            Self::NotProven => 3,
        }
    }
}

/// An opaque, versioned reference to an authorization decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationEvidence {
    scheme_version: SchemaVersion,
    reference: String,
    freshness: EvidenceFreshness,
    strength: AuthorizationStrength,
}

impl AuthorizationEvidence {
    /// Construct an authorization evidence reference.
    pub fn new(
        scheme_version: SchemaVersion,
        reference: impl Into<String>,
        freshness: EvidenceFreshness,
        strength: AuthorizationStrength,
    ) -> Self {
        Self { scheme_version, reference: reference.into(), freshness, strength }
    }

    /// The authorization scheme's own version.
    pub fn scheme_version(&self) -> SchemaVersion {
        self.scheme_version
    }

    /// The opaque reference text.
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// How current the decision is.
    pub fn freshness(&self) -> EvidenceFreshness {
        self.freshness
    }

    /// How strongly the execution was authorized.
    pub fn strength(&self) -> AuthorizationStrength {
        self.strength
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("authorization");
        encoder.unsigned(u64::from(self.scheme_version.get()));
        encoder.text(&self.reference);
        encoder.variant(self.freshness.discriminant());
        encoder.variant(self.strength.discriminant());
    }
}

/// The platform a plan requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlatformRequirement {
    /// Requires Linux process semantics.
    LinuxOnly,
    /// Requires Unix process semantics.
    AnyUnix,
    /// Runs anywhere the backend supports.
    AnyPlatform,
}

impl PlatformRequirement {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::LinuxOnly => 0,
            Self::AnyUnix => 1,
            Self::AnyPlatform => 2,
        }
    }
}

/// Fingerprint helper for values that are identified but never disclosed.
pub(crate) fn fingerprint_of_bytes(bytes: &[u8]) -> Fingerprint {
    Fingerprint::of(bytes)
}
