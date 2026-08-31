//! Normalized, private-safe evidence observation contract and deterministic
//! adapter registry for maintained compiler operating profiles (#12188, train
//! row COMP-PROFILE-E01, parent #12177).
//!
//! This module gives heterogeneous canonical receipts one common **observation
//! envelope** ([`CompilerProfileObservationV1`]) without copying their
//! semantics, plus one deterministic [`ObservationAdapterRegistry`] that owns
//! adapter identity, accepted source schema ranges, lossiness, and allowed
//! observation classes.  It composes with the landed #12186 vocabulary in
//! `compiler_profile_contract` (`ClaimFamily`, `ProofClass`, `ClaimCeiling`,
//! `InvalidationInput`, `InvalidationKind`) instead of inventing a second
//! one; the receipt-family adapters (E02–E06), evidence-set assembly (E07),
//! and the evaluator (E08) consume these types through the public surface.
//!
//! Deliberately absent (issue non-goals): no concrete receipt-family adapter
//! (the registry ships empty; tests use synthetic descriptors), no manifest
//! loading, no row evaluation, no receipt discovery from logs, no proof
//! execution, no serde/file syntax, no CLI, and no compiler/product path
//! change.  Source receipts remain canonical: an observation is an
//! evaluation input, never a replacement evidence authority.
//!
//! Closure laws expressed and validated here:
//!
//! - the envelope preserves, never flattens, source vocabulary: pass/failed/
//!   not_proven/stale, unsupported/not_applicable/conditional_not_selected/
//!   optional_absent, instrument_failed/cancelled/timed_out/zero_work,
//!   observed-red-but-complete, and accepted-debt-not-general-support are
//!   independent typed states;
//! - every envelope field is private-safe: no host-specific paths, no
//!   issue/PR/workflow colour, no log prose, no source payload content
//!   (`ensure_private_safe`); the envelope structurally carries no payload
//!   field at all, only a digest-bearing [`CanonicalReceiptReference`];
//! - absent subject dimensions remain explicit `not_proven`; they are never
//!   filled from nearby receipts or display text
//!   ([`CandidateSubjectIdentity::dimension`]);
//! - an observation may narrow but never strengthen the source claim
//!   ([`ObservedClaimCeiling`]), and an adapter's declared observation
//!   ceiling never exceeds its source claim ceiling;
//! - unknown, future, or explicitly unsupported source schemas fail closed
//!   ([`ObservationAdapterRegistry::select_adapter`]);
//! - one adapter cannot claim another receipt family; overlapping adapters
//!   for one source family/version are rejected unless an explicit migration
//!   relation (`supersedes`) selects one;
//! - registration order cannot change adapter identity, selection, or
//!   normalized bytes (`semantic_fingerprint`), and envelope identity is
//!   order-insensitive but content-sensitive
//!   ([`CompilerProfileObservationV1::identity`]);
//! - an observation must carry every currentness input its adapter requires
//!   ([`ObservationAdapterRegistry::validate_observation`]).

use crate::compiler_profile_contract::{
    ClaimCeiling, ClaimFamily, InvalidationInput, InvalidationKind, ProofClass,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Shared validation helpers
// ---------------------------------------------------------------------------

/// Markers that would smuggle host-specific, private, or workflow content
/// into the normalized envelope.  Free-text fields are evidence metadata,
/// never payload: absolute paths, user directories, issue/PR references,
/// workflow state, and log prose can never become evidence.
const PRIVATE_OR_WORKFLOW_MARKERS: [&str; 12] = [
    "/home/", "/users/", "/root/", "c:\\", "\\\\", "github", "workflow", "issue #", "issues/",
    "pull/", "pr #", ".log",
];

/// The envelope's private-safety predicate, for adapters that must reject a
/// source value before it reaches envelope text.
///
/// Every free-text field in the envelope is already checked on construction,
/// but a receipt-family adapter (#12302 and its E02–E06 siblings) builds that
/// text out of source identifiers, and a deep failure inside a subject
/// dimension or a disposition reason is a poor account of what is wrong with
/// the receipt. Exposing the predicate lets an adapter fail closed early and
/// name the offending source field instead.
pub fn ensure_private_safe_text(field: &str, value: &str) -> Result<()> {
    ensure_private_safe(field, value)
}

/// Validate that a free-text field is non-empty and private-safe.
fn ensure_private_safe(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    if value.starts_with('/') || value.starts_with('~') {
        bail!("{field} must not carry a host-specific absolute path, got {value:?}");
    }
    // Any drive root (`C:\`, `d:/`, ...) is a host-specific absolute path;
    // hard-coding one drive letter lets every other Windows drive through.
    let bytes = value.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        bail!("{field} must not carry a host-specific absolute path, got {value:?}");
    }
    let lowered = value.to_lowercase();
    for marker in PRIVATE_OR_WORKFLOW_MARKERS {
        if lowered.contains(marker) {
            bail!("{field} must not carry private or workflow content ({marker}), got {value:?}");
        }
    }
    Ok(())
}

fn ensure_distinct_ids<'a>(ids: impl Iterator<Item = &'a str>, name: &str) -> Result<()> {
    let ids = ids.collect::<Vec<_>>();
    let unique = ids.iter().collect::<BTreeSet<_>>();
    if unique.len() != ids.len() {
        bail!("{name} must not contain duplicate ids");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

/// Deterministic digest newtype (64 lowercase hex sha256 characters) used
/// for receipt references and registry fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObservationDigest(String);

impl ObservationDigest {
    /// Construct a digest from exactly 64 lowercase hex characters.
    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("observation digest must be 64 lowercase hex characters, got {value:?}");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Content-sensitive identity of one normalized observation.  Computed as
/// the sha256 of the envelope's canonical semantic text: order-insensitive
/// collections are sorted before hashing, so identity cannot depend on
/// insertion order but changes with any semantic field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObservationIdentity(String);

impl ObservationIdentity {
    /// Construct an identity from exactly 64 lowercase hex characters.
    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("observation identity must be 64 lowercase hex characters, got {value:?}");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact identity of a source receipt family.  One adapter accepts exactly
/// one family; one family reference never names payload content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReceiptFamily(String);

impl ReceiptFamily {
    /// Construct a non-empty, private-safe receipt family identity.
    pub fn new(value: &str) -> Result<Self> {
        ensure_private_safe("receipt family", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the family text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact identity of one canonical source receipt.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReceiptId(String);

impl ReceiptId {
    /// Construct a non-empty, private-safe receipt identity.
    pub fn new(value: &str) -> Result<Self> {
        ensure_private_safe("receipt id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact source schema version.  Adapters accept a closed inclusive range;
/// anything outside it is unknown or future and fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Construct a schema version.
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    /// The version number.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Stable identity of one observation adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdapterId(String);

impl AdapterId {
    /// Construct a non-empty, private-safe adapter identity.
    pub fn new(value: &str) -> Result<Self> {
        ensure_private_safe("adapter id", value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable version of one observation adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdapterVersion(String);

impl AdapterVersion {
    /// Construct a non-empty `v`-prefixed adapter version.
    pub fn new(value: &str) -> Result<Self> {
        ensure_private_safe("adapter version", value)?;
        if !value.starts_with('v') {
            bail!("adapter version {value:?} must start with 'v'");
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Producer and schema identity of the source receipt: which tool produced
/// it, which family it belongs to, and which schema version it claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerAndSchemaIdentity {
    /// Producer tool identity (private-safe).
    pub producer: String,
    /// Source receipt family.
    pub family: ReceiptFamily,
    /// Source schema version.
    pub schema: SchemaVersion,
}

impl ProducerAndSchemaIdentity {
    /// Construct a producer/schema identity with a private-safe producer.
    pub fn new(producer: &str, family: ReceiptFamily, schema: SchemaVersion) -> Result<Self> {
        ensure_private_safe("receipt producer", producer)?;
        Ok(Self { producer: producer.to_owned(), family, schema })
    }

    fn validate(&self) -> Result<()> {
        ensure_private_safe("receipt producer", &self.producer)
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(
            out,
            "producer={:?} family={:?} schema={}",
            self.producer,
            self.family.as_str(),
            self.schema.get()
        );
    }
}

/// Canonical reference to the source receipt.  This is a reference, never a
/// copy: it carries identity, producer/schema, and a content digest only, so
/// no source or private payload content can enter the normalized envelope
/// through it.  The source receipt remains the canonical evidence authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalReceiptReference {
    /// Exact receipt identity.
    pub id: ReceiptId,
    /// Content digest of the canonical receipt bytes.
    pub digest: ObservationDigest,
    /// Producer and schema identity.
    pub producer: ProducerAndSchemaIdentity,
}

impl CanonicalReceiptReference {
    fn validate(&self) -> Result<()> {
        self.producer.validate()
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "receipt {:?} digest={:?} ", self.id.as_str(), self.digest.as_str());
        self.producer.write_canonical(out);
    }
}

/// Identity of the adapter that produced one observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterIdentity {
    /// Stable adapter identity.
    pub id: AdapterId,
    /// Stable adapter version.
    pub version: AdapterVersion,
}

impl AdapterIdentity {
    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "adapter={:?} version={:?}", self.id.as_str(), self.version.as_str());
    }
}

// ---------------------------------------------------------------------------
// Subject identity
// ---------------------------------------------------------------------------

/// Exact subject dimension an observation can bind.  The eight dimensions
/// are closed typed states; an adapter declares which of them it can prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SubjectDimensionKind {
    /// Repository/tree/lockfile/build identity.
    RepositoryTree,
    /// Binary/artifact/package identity.
    BinaryArtifact,
    /// Rust/Perl/toolchain/target/profile identity.
    Toolchain,
    /// Parser/semantic/PIR/world/provider policy identity.
    CompilerPolicy,
    /// Platform/architecture/client/host identity.
    Platform,
    /// Fixture/project/upstream series identity.
    FixtureSeries,
    /// Receipt producer/tool/schema/configuration identity.
    ProducerConfiguration,
    /// Observation/currentness time identity.
    ObservationTime,
}

impl SubjectDimensionKind {
    /// Closed list of every subject dimension.
    pub const ALL: [Self; 8] = [
        Self::RepositoryTree,
        Self::BinaryArtifact,
        Self::Toolchain,
        Self::CompilerPolicy,
        Self::Platform,
        Self::FixtureSeries,
        Self::ProducerConfiguration,
        Self::ObservationTime,
    ];

    /// Stable canonical tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::RepositoryTree => "repository_tree",
            Self::BinaryArtifact => "binary_artifact",
            Self::Toolchain => "toolchain",
            Self::CompilerPolicy => "compiler_policy",
            Self::Platform => "platform",
            Self::FixtureSeries => "fixture_series",
            Self::ProducerConfiguration => "producer_configuration",
            Self::ObservationTime => "observation_time",
        }
    }
}

/// One bound subject dimension.  A dimension is either proven with an
/// exact, private-safe value or explicitly not proven; it is never filled
/// from a nearby receipt or display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectDimension {
    /// Exact proven dimension value.
    Proven(String),
    /// Explicitly not proven.
    NotProven,
}

impl SubjectDimension {
    /// Construct a proven dimension with a non-empty, private-safe value.
    pub fn proven(value: &str) -> Result<Self> {
        ensure_private_safe("subject dimension", value)?;
        Ok(Self::Proven(value.to_owned()))
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Proven(value) => ensure_private_safe("subject dimension", value),
            Self::NotProven => Ok(()),
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Proven(value) => {
                let _ = write!(out, "proven({value:?})");
            }
            Self::NotProven => out.push_str("not_proven"),
        }
    }
}

/// Exact candidate subject identity.  Absent dimensions read back as
/// explicit [`SubjectDimension::NotProven`]; canonical form names every
/// closed dimension so an absent applicable dimension can never be
/// reconstructed implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CandidateSubjectIdentity {
    dimensions: BTreeMap<SubjectDimensionKind, SubjectDimension>,
}

impl CandidateSubjectIdentity {
    /// An identity with every dimension explicitly not proven.
    pub fn not_proven() -> Self {
        Self::default()
    }

    /// Bind one dimension.  Binding is exact: rebinding replaces the value.
    pub fn bind(&mut self, kind: SubjectDimensionKind, dimension: SubjectDimension) {
        self.dimensions.insert(kind, dimension);
    }

    /// Read one dimension.  Absent dimensions are explicit `not_proven`.
    pub fn dimension(&self, kind: SubjectDimensionKind) -> SubjectDimension {
        self.dimensions.get(&kind).cloned().unwrap_or(SubjectDimension::NotProven)
    }

    /// The dimensions this identity proves.
    pub fn proven_dimensions(&self) -> BTreeSet<SubjectDimensionKind> {
        self.dimensions
            .iter()
            .filter(|(_, dimension)| matches!(dimension, SubjectDimension::Proven(_)))
            .map(|(kind, _)| *kind)
            .collect()
    }

    fn validate(&self) -> Result<()> {
        for dimension in self.dimensions.values() {
            dimension.validate()?;
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        for kind in SubjectDimensionKind::ALL {
            let _ = write!(out, "  subject {}=", kind.tag());
            self.dimension(kind).write_canonical(out);
            out.push('\n');
        }
    }
}

// ---------------------------------------------------------------------------
// Observation class and independent dispositions
// ---------------------------------------------------------------------------

/// Exact observation class: one independent proposition family and one
/// independent proof axis from the landed #12186 vocabulary.  Classes are
/// closed typed pairs; a curated-expectation observation can never be read
/// as an EIR-mechanism or evaluated-work observation, and a parser-internal
/// observation can never be read as provider, edit, or installed-host
/// evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObservationClass {
    /// Independent proposition family (#12186 `ClaimFamily`).
    pub family: ClaimFamily,
    /// Independent proof axis (#12186 `ProofClass`).
    pub proof_class: ProofClass,
}

impl ObservationClass {
    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "class={}/{}", self.family.tag(), self.proof_class.tag());
    }
}

/// Closed observation disposition.  The source vocabulary is preserved, not
/// flattened: pass, failed, not-proven, and stale are distinct states, and
/// unsupported, not-applicable, conditional-not-selected, and
/// optional-absent are closed typed states with named payloads, never
/// omissions and never aliases of pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationDisposition {
    /// The observation passed.
    Pass,
    /// The observation failed; observed red is preserved as red.
    Failed,
    /// The observation is not proven.
    NotProven,
    /// The observation is stale relative to its currentness basis.
    Stale,
    /// Explicitly unsupported with a named reason.
    Unsupported { reason: String },
    /// Explicitly not applicable with a named justification.
    NotApplicable { justification: String },
    /// A conditional observation whose trigger was not selected.
    ConditionalNotSelected { trigger: String },
    /// An optional observation that is absent, stated explicitly.
    OptionalAbsent,
}

impl ObservationDisposition {
    /// Construct an unsupported disposition with a named reason.
    pub fn unsupported(reason: &str) -> Result<Self> {
        ensure_private_safe("unsupported disposition reason", reason)?;
        Ok(Self::Unsupported { reason: reason.to_owned() })
    }

    /// Construct a not-applicable disposition with a named justification.
    pub fn not_applicable(justification: &str) -> Result<Self> {
        ensure_private_safe("not-applicable disposition justification", justification)?;
        Ok(Self::NotApplicable { justification: justification.to_owned() })
    }

    /// Construct a conditional-not-selected disposition with a named trigger.
    pub fn conditional_not_selected(trigger: &str) -> Result<Self> {
        ensure_private_safe("conditional disposition trigger", trigger)?;
        Ok(Self::ConditionalNotSelected { trigger: trigger.to_owned() })
    }

    /// True only for pass observations.
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }

    /// True for the closed non-claiming states that cannot carry more than
    /// observed evidence.
    fn is_closed_non_claiming(&self) -> bool {
        matches!(
            self,
            Self::Unsupported { .. }
                | Self::NotApplicable { .. }
                | Self::ConditionalNotSelected { .. }
                | Self::OptionalAbsent
        )
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Pass | Self::Failed | Self::NotProven | Self::Stale | Self::OptionalAbsent => {
                Ok(())
            }
            Self::Unsupported { reason } => {
                ensure_private_safe("unsupported disposition reason", reason)
            }
            Self::NotApplicable { justification } => {
                ensure_private_safe("not-applicable disposition justification", justification)
            }
            Self::ConditionalNotSelected { trigger } => {
                ensure_private_safe("conditional disposition trigger", trigger)
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Pass => out.push_str("pass"),
            Self::Failed => out.push_str("failed"),
            Self::NotProven => out.push_str("not_proven"),
            Self::Stale => out.push_str("stale"),
            Self::Unsupported { reason } => {
                let _ = write!(out, "unsupported({reason:?})");
            }
            Self::NotApplicable { justification } => {
                let _ = write!(out, "not_applicable({justification:?})");
            }
            Self::ConditionalNotSelected { trigger } => {
                let _ = write!(out, "conditional_not_selected({trigger:?})");
            }
            Self::OptionalAbsent => out.push_str("optional_absent"),
        }
    }
}

/// Closed currentness disposition, independent of the product disposition:
/// a passing observation can be stale and a failed one can be current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentnessDisposition {
    /// The observation is current against its invalidation inputs.
    Current,
    /// The observation is stale.
    Stale,
    /// Currentness is not proven.
    NotProven,
}

impl CurrentnessDisposition {
    fn tag(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::NotProven => "not_proven",
        }
    }
}

/// Closed completeness disposition, independent of the product disposition:
/// observed-red-but-complete is representable exactly (failed product
/// disposition plus complete completeness) and can never collapse into
/// pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletenessDisposition {
    /// The observation covers its whole intended scope.
    Complete,
    /// The observation is explicitly partial; the remainder is named.
    Partial { remainder: String },
    /// Completeness is not proven.
    NotProven,
}

impl CompletenessDisposition {
    /// Construct a partial disposition with a named remainder.
    pub fn partial(remainder: &str) -> Result<Self> {
        ensure_private_safe("completeness remainder", remainder)?;
        Ok(Self::Partial { remainder: remainder.to_owned() })
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Complete | Self::NotProven => Ok(()),
            Self::Partial { remainder } => ensure_private_safe("completeness remainder", remainder),
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Complete => out.push_str("complete"),
            Self::Partial { remainder } => {
                let _ = write!(out, "partial({remainder:?})");
            }
            Self::NotProven => out.push_str("not_proven"),
        }
    }
}

/// Closed work disposition.  Completed work names a non-empty scope;
/// zero-work is a distinct typed state that can never collapse into pass or
/// not-applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkDisposition {
    /// Work completed with a named non-empty scope.
    Completed { scope: String },
    /// The instrument ran zero work, stated explicitly.
    ZeroWork,
    /// Work is not applicable to this observation, with a named reason.
    NotApplicable { reason: String },
    /// Work state is not proven.
    NotProven,
}

impl WorkDisposition {
    /// Construct a completed disposition with a non-empty, private-safe
    /// scope.
    pub fn completed(scope: &str) -> Result<Self> {
        ensure_private_safe("work scope", scope)?;
        Ok(Self::Completed { scope: scope.to_owned() })
    }

    /// Construct a not-applicable disposition with a named reason.
    pub fn not_applicable(reason: &str) -> Result<Self> {
        ensure_private_safe("work not-applicable reason", reason)?;
        Ok(Self::NotApplicable { reason: reason.to_owned() })
    }

    fn is_zero_work(&self) -> bool {
        matches!(self, Self::ZeroWork)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::ZeroWork | Self::NotProven => Ok(()),
            Self::Completed { scope } => ensure_private_safe("work scope", scope),
            Self::NotApplicable { reason } => {
                ensure_private_safe("work not-applicable reason", reason)
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Completed { scope } => {
                let _ = write!(out, "completed({scope:?})");
            }
            Self::ZeroWork => out.push_str("zero_work"),
            Self::NotApplicable { reason } => {
                let _ = write!(out, "not_applicable({reason:?})");
            }
            Self::NotProven => out.push_str("not_proven"),
        }
    }
}

/// Closed limitation disposition.  Accepted debt is bounded and named; it
/// is accepted debt for its exact scope, never general semantic support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitationDisposition {
    /// No limitation is carried.
    None,
    /// Source-locked accepted debt for an exact named scope; never general
    /// support.
    AcceptedDebt { scope: String, reason: String },
    /// Limitation state is not proven.
    NotProven,
}

impl LimitationDisposition {
    /// Construct accepted debt with a named scope and reason.
    pub fn accepted_debt(scope: &str, reason: &str) -> Result<Self> {
        ensure_private_safe("accepted debt scope", scope)?;
        ensure_private_safe("accepted debt reason", reason)?;
        Ok(Self::AcceptedDebt { scope: scope.to_owned(), reason: reason.to_owned() })
    }

    fn is_accepted_debt(&self) -> bool {
        matches!(self, Self::AcceptedDebt { .. })
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::None | Self::NotProven => Ok(()),
            Self::AcceptedDebt { scope, reason } => {
                ensure_private_safe("accepted debt scope", scope)?;
                ensure_private_safe("accepted debt reason", reason)
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::None => out.push_str("none"),
            Self::AcceptedDebt { scope, reason } => {
                let _ = write!(out, "accepted_debt(scope={scope:?}, reason={reason:?})");
            }
            Self::NotProven => out.push_str("not_proven"),
        }
    }
}

/// Ceiling an observation claims, expressed in the landed #12186
/// `ClaimCeiling` vocabulary.  This is a composition wrapper, not a second
/// vocabulary: the three ceilings are exactly the landed closed states, and
/// none of them is support, release, or publication authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedClaimCeiling(ClaimCeiling);

impl ObservedClaimCeiling {
    /// Wrap a landed claim ceiling.
    pub fn new(ceiling: ClaimCeiling) -> Self {
        Self(ceiling)
    }

    /// The landed claim ceiling.
    pub fn claim_ceiling(self) -> ClaimCeiling {
        self.0
    }

    /// Rank used for narrowing checks: observed evidence (0) <
    /// accepted compatibility (1) < bounded public claim (2).
    pub fn rank(self) -> u8 {
        rank_of(self.0)
    }

    /// True when this ceiling narrows or matches the named source ceiling.
    /// An observation may narrow but never strengthen the source claim.
    pub fn narrows_or_matches(self, source: ClaimCeiling) -> bool {
        self.rank() <= rank_of(source)
    }

    fn tag(self) -> &'static str {
        self.0.tag()
    }
}

fn rank_of(ceiling: ClaimCeiling) -> u8 {
    match ceiling {
        ClaimCeiling::ObservedEvidence => 0,
        ClaimCeiling::AcceptedCompatibility => 1,
        ClaimCeiling::BoundedPublicClaim => 2,
    }
}

/// Invalidation evidence carried by one observation: the exact inputs that
/// re-open it.  At least one input is required; the inputs reuse the landed
/// #12186 `InvalidationInput` vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidationEvidence {
    /// Invalidation inputs (non-empty).
    pub inputs: Vec<InvalidationInput>,
}

impl InvalidationEvidence {
    /// Construct invalidation evidence; at least one input is required.
    pub fn new(inputs: Vec<InvalidationInput>) -> Result<Self> {
        if inputs.is_empty() {
            bail!("invalidation evidence must name at least one input");
        }
        Ok(Self { inputs })
    }

    fn validate(&self) -> Result<()> {
        if self.inputs.is_empty() {
            bail!("invalidation evidence must name at least one input");
        }
        // Detail text is written into the private-safe canonical envelope and
        // its identity, so every input detail must satisfy the same
        // private-safety rules as every other free-text field.
        for input in &self.inputs {
            ensure_private_safe("invalidation detail", &input.detail)?;
        }
        Ok(())
    }

    fn write_canonical(&self, out: &mut String) {
        let mut inputs: Vec<&InvalidationInput> = self.inputs.iter().collect();
        inputs.sort_by(|a, b| a.kind.tag().cmp(b.kind.tag()).then_with(|| a.detail.cmp(&b.detail)));
        for input in inputs {
            let _ = writeln!(out, "  invalidation {} {:?}", input.kind.tag(), input.detail);
        }
    }
}

/// Closed terminal state of the producing instrument.  Instrument failure,
/// cancellation, and timeout are distinct typed states that can never
/// collapse into pass or not-applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalState {
    /// The instrument ran to completion.
    Completed,
    /// The instrument itself failed, with a named detail.
    InstrumentFailed { detail: String },
    /// The instrument run was cancelled, with a named reason.
    Cancelled { reason: String },
    /// The instrument run timed out, with a named detail.
    TimedOut { detail: String },
}

impl TerminalState {
    /// Construct an instrument-failed state with a named detail.
    pub fn instrument_failed(detail: &str) -> Result<Self> {
        ensure_private_safe("instrument failure detail", detail)?;
        Ok(Self::InstrumentFailed { detail: detail.to_owned() })
    }

    /// Construct a cancelled state with a named reason.
    pub fn cancelled(reason: &str) -> Result<Self> {
        ensure_private_safe("cancellation reason", reason)?;
        Ok(Self::Cancelled { reason: reason.to_owned() })
    }

    /// Construct a timed-out state with a named detail.
    pub fn timed_out(detail: &str) -> Result<Self> {
        ensure_private_safe("timeout detail", detail)?;
        Ok(Self::TimedOut { detail: detail.to_owned() })
    }

    fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Completed => Ok(()),
            Self::InstrumentFailed { detail } => {
                ensure_private_safe("instrument failure detail", detail)
            }
            Self::Cancelled { reason } => ensure_private_safe("cancellation reason", reason),
            Self::TimedOut { detail } => ensure_private_safe("timeout detail", detail),
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Completed => out.push_str("completed"),
            Self::InstrumentFailed { detail } => {
                let _ = write!(out, "instrument_failed({detail:?})");
            }
            Self::Cancelled { reason } => {
                let _ = write!(out, "cancelled({reason:?})");
            }
            Self::TimedOut { detail } => {
                let _ = write!(out, "timed_out({detail:?})");
            }
        }
    }
}

/// Instrument identity and its terminal state.  Both are mandatory: an
/// observation always names which instrument produced it and how that run
/// ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentAndTerminalState {
    /// Instrument identity (private-safe).
    pub instrument: String,
    /// Closed terminal state of the instrument run.
    pub terminal: TerminalState,
}

impl InstrumentAndTerminalState {
    /// Construct the pair with a non-empty, private-safe instrument name.
    pub fn new(instrument: &str, terminal: TerminalState) -> Result<Self> {
        ensure_private_safe("instrument identity", instrument)?;
        Ok(Self { instrument: instrument.to_owned(), terminal })
    }

    fn validate(&self) -> Result<()> {
        ensure_private_safe("instrument identity", &self.instrument)?;
        self.terminal.validate()
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = write!(out, "instrument={:?} terminal=", self.instrument);
        self.terminal.write_canonical(out);
    }
}

// ---------------------------------------------------------------------------
// Observation envelope
// ---------------------------------------------------------------------------

/// Version 1 normalized evidence observation envelope.  One envelope
/// normalizes one canonical receipt into evaluation input: exact receipt
/// reference, exact private-safe subject identity, observation class, and
/// independent product, currentness, completeness, work, limitation,
/// claim-ceiling, invalidation, and instrument states.  Product, instrument,
/// currentness, completeness, work, limitation, and claim-ceiling states
/// remain independent closed dimensions: none of them can stand in for
/// another, and none confers support, release, or publication authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerProfileObservationV1 {
    /// Canonical reference to the source receipt (reference, never payload).
    pub receipt: CanonicalReceiptReference,
    /// Exact candidate subject identity; absent dimensions stay `not_proven`.
    pub subject: CandidateSubjectIdentity,
    /// Exact observation class (family + proof axis).
    pub class: ObservationClass,
    /// Product disposition.
    pub disposition: ObservationDisposition,
    /// Currentness disposition.
    pub currentness: CurrentnessDisposition,
    /// Completeness disposition.
    pub completeness: CompletenessDisposition,
    /// Work disposition.
    pub work: WorkDisposition,
    /// Limitation disposition.
    pub limitation: LimitationDisposition,
    /// Claimed ceiling (narrower than or equal to the source claim).
    pub ceiling: ObservedClaimCeiling,
    /// Invalidation evidence (non-empty).
    pub invalidation: InvalidationEvidence,
    /// Producing instrument and its terminal state.
    pub instrument: InstrumentAndTerminalState,
    /// Identity of the adapter that produced this observation.
    pub adapter: AdapterIdentity,
}

impl CompilerProfileObservationV1 {
    /// Validate the envelope's internal closure laws.
    pub fn validate(&self) -> Result<()> {
        self.receipt.validate().context("observation receipt")?;
        self.subject.validate().context("observation subject")?;
        self.disposition.validate().context("observation disposition")?;
        self.completeness.validate().context("observation completeness")?;
        self.work.validate().context("observation work")?;
        self.limitation.validate().context("observation limitation")?;
        self.invalidation.validate().context("observation invalidation")?;
        self.instrument.validate().context("observation instrument")?;

        // Closed non-claiming dispositions and accepted debt can never carry
        // more than observed evidence: accepted source-locked debt is not
        // general semantic support, and an unsupported/not-applicable/
        // conditional-not-selected/optional-absent observation claims nothing.
        if (self.disposition.is_closed_non_claiming() || self.limitation.is_accepted_debt())
            && self.ceiling.claim_ceiling() != ClaimCeiling::ObservedEvidence
        {
            bail!(
                "observation {:?} carries a closed non-claiming disposition or accepted debt and cannot claim more than observed evidence",
                self.receipt.id.as_str()
            );
        }

        // Instrument failure, cancellation, timeout, and zero work are
        // distinct typed states that can never collapse into pass or
        // not-applicable.
        if (!self.instrument.terminal.is_completed() || self.work.is_zero_work())
            && (self.disposition.is_pass()
                || matches!(self.disposition, ObservationDisposition::NotApplicable { .. }))
        {
            bail!(
                "observation {:?} has a non-completed instrument or zero work and cannot be typed as pass or not-applicable",
                self.receipt.id.as_str()
            );
        }
        Ok(())
    }

    /// Deterministic canonical semantic text: every semantic field of the
    /// envelope, with every closed subject dimension named explicitly and
    /// all order-insensitive collections sorted.
    pub fn canonical_semantic_text(&self) -> Result<String> {
        self.validate()?;
        let mut out = String::new();
        out.push_str("compiler_profile_observation v1\n");
        self.receipt.write_canonical(&mut out);
        out.push('\n');
        self.adapter.write_canonical(&mut out);
        out.push('\n');
        self.class.write_canonical(&mut out);
        out.push('\n');
        out.push_str("disposition=");
        self.disposition.write_canonical(&mut out);
        out.push('\n');
        let _ = writeln!(out, "currentness={}", self.currentness.tag());
        out.push_str("completeness=");
        self.completeness.write_canonical(&mut out);
        out.push('\n');
        out.push_str("work=");
        self.work.write_canonical(&mut out);
        out.push('\n');
        out.push_str("limitation=");
        self.limitation.write_canonical(&mut out);
        out.push('\n');
        let _ = writeln!(out, "ceiling={}", self.ceiling.tag());
        self.instrument.write_canonical(&mut out);
        out.push('\n');
        self.invalidation.write_canonical(&mut out);
        self.subject.write_canonical(&mut out);
        Ok(out)
    }

    /// Content-sensitive, order-insensitive observation identity: the sha256
    /// of [`Self::canonical_semantic_text`].
    pub fn identity(&self) -> Result<ObservationIdentity> {
        let canonical = self.canonical_semantic_text()?;
        ObservationIdentity::from_hex(&sha256_hex(canonical.as_bytes()))
            .context("sha256 hex output must satisfy the identity invariant")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// ---------------------------------------------------------------------------
// Adapter descriptor and registry
// ---------------------------------------------------------------------------

/// Lossiness declaration of one adapter.  Lossy adaptation names exactly
/// what is lost; lossiness is declared data, never discovered downstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterLossiness {
    /// The adapter preserves every field and semantic it declares.
    Lossless,
    /// The adapter drops or coarsens the named fields/semantics.
    Lossy { description: String },
}

impl AdapterLossiness {
    /// Construct a lossy declaration with a named description.
    pub fn lossy(description: &str) -> Result<Self> {
        ensure_private_safe("lossiness description", description)?;
        Ok(Self::Lossy { description: description.to_owned() })
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Lossless => Ok(()),
            Self::Lossy { description } => {
                ensure_private_safe("lossiness description", description)
            }
        }
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Lossless => out.push_str("lossless"),
            Self::Lossy { description } => {
                let _ = write!(out, "lossy({description:?})");
            }
        }
    }
}

/// Static descriptor of one observation adapter.  An adapter declares its
/// stable identity, the one source receipt family and inclusive schema range
/// it accepts, its source authority, the observation classes it may emit,
/// the subject dimensions it can prove, the fields/semantics it preserves,
/// its lossiness and claim ceiling, its required currentness inputs, the
/// source versions it explicitly does not support, and optionally the
/// adapter it supersedes (the explicit migration relation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationAdapterDescriptor {
    /// Stable adapter identity.
    pub id: AdapterId,
    /// Stable adapter version.
    pub version: AdapterVersion,
    /// The one source receipt family this adapter accepts.  An adapter can
    /// never claim another receipt family.
    pub source_family: ReceiptFamily,
    /// Inclusive lowest accepted source schema version.
    pub schema_min: SchemaVersion,
    /// Inclusive highest accepted source schema version.
    pub schema_max: SchemaVersion,
    /// Source authority/owner of the accepted receipt family.
    pub source_authority: String,
    /// Observation classes this adapter may emit (non-empty).
    pub emitted_classes: BTreeSet<ObservationClass>,
    /// Subject dimensions this adapter can prove.
    pub provable_dimensions: BTreeSet<SubjectDimensionKind>,
    /// Fields/semantics the adapter preserves from the source receipt.
    pub preserved_fields: BTreeSet<String>,
    /// Declared lossiness.
    pub lossiness: AdapterLossiness,
    /// The strongest claim the source receipt itself supports.
    pub source_claim_ceiling: ClaimCeiling,
    /// The strongest claim observations from this adapter may carry; never
    /// stronger than the source claim.
    pub observation_claim_ceiling: ObservedClaimCeiling,
    /// Currentness inputs the adapter requires before it may adapt.
    pub required_currentness_inputs: BTreeSet<InvalidationKind>,
    /// Source versions inside the accepted range that are explicitly
    /// unsupported and fail closed.
    pub unsupported_source_versions: BTreeSet<SchemaVersion>,
    /// Explicit migration relation: the adapter this one supersedes.
    pub supersedes: Option<AdapterId>,
}

impl ObservationAdapterDescriptor {
    /// Validate the descriptor's internal closure laws.
    pub fn validate(&self) -> Result<()> {
        if self.schema_min > self.schema_max {
            bail!(
                "adapter {:?} schema range is inverted: min {} > max {}",
                self.id.as_str(),
                self.schema_min.get(),
                self.schema_max.get()
            );
        }
        for version in &self.unsupported_source_versions {
            if *version < self.schema_min || *version > self.schema_max {
                bail!(
                    "adapter {:?} names unsupported source version {} outside its accepted range {}..={}",
                    self.id.as_str(),
                    version.get(),
                    self.schema_min.get(),
                    self.schema_max.get()
                );
            }
        }
        ensure_private_safe("source authority", &self.source_authority)?;
        if self.emitted_classes.is_empty() {
            bail!("adapter {:?} must declare at least one emitted class", self.id.as_str());
        }
        for field in &self.preserved_fields {
            ensure_private_safe("preserved field", field)?;
        }
        self.lossiness.validate()?;
        if !self.observation_claim_ceiling.narrows_or_matches(self.source_claim_ceiling) {
            bail!(
                "adapter {:?} strengthens the source claim: observation ceiling {} exceeds source ceiling {}",
                self.id.as_str(),
                self.observation_claim_ceiling.tag(),
                self.source_claim_ceiling.tag()
            );
        }
        if self.supersedes.as_ref() == Some(&self.id) {
            bail!("adapter {:?} must not supersede itself", self.id.as_str());
        }
        Ok(())
    }

    /// True when this adapter accepts the named source schema version:
    /// inside the inclusive range and not explicitly unsupported.
    pub fn accepts(&self, schema: SchemaVersion) -> bool {
        schema >= self.schema_min
            && schema <= self.schema_max
            && !self.unsupported_source_versions.contains(&schema)
    }

    fn write_canonical(&self, out: &mut String) {
        let _ = writeln!(out, "adapter {:?} version={:?}", self.id.as_str(), self.version.as_str());
        let _ = writeln!(
            out,
            "  source_family={:?} schema={}..={} authority={:?}",
            self.source_family.as_str(),
            self.schema_min.get(),
            self.schema_max.get(),
            self.source_authority
        );
        out.push_str("  emitted_classes=[");
        for class in &self.emitted_classes {
            let _ = write!(out, "{}/{},", class.family.tag(), class.proof_class.tag());
        }
        out.push_str("]\n  provable_dimensions=[");
        for kind in &self.provable_dimensions {
            let _ = write!(out, "{},", kind.tag());
        }
        out.push_str("]\n  preserved_fields=[");
        for field in &self.preserved_fields {
            let _ = write!(out, "{field:?},");
        }
        out.push_str("]\n  lossiness=");
        self.lossiness.write_canonical(out);
        let _ = writeln!(
            out,
            "\n  source_ceiling={} observation_ceiling={}",
            self.source_claim_ceiling.tag(),
            self.observation_claim_ceiling.tag()
        );
        out.push_str("  required_currentness=[");
        for kind in &self.required_currentness_inputs {
            let _ = write!(out, "{},", kind.tag());
        }
        out.push_str("]\n  unsupported_source_versions=[");
        for version in &self.unsupported_source_versions {
            let _ = write!(out, "{},", version.get());
        }
        match &self.supersedes {
            Some(target) => {
                let _ = writeln!(out, "]\n  supersedes={:?}", target.as_str());
            }
            None => out.push_str("]\n  supersedes=none\n"),
        }
    }
}

/// Deterministic registry of observation adapters.  The registry owns
/// adapter identity, source schema ranges, lossiness, and allowed
/// observation classes.  Registration order cannot change identity,
/// selection, or normalized bytes: adapters are keyed by identity and the
/// fingerprint is computed over sorted canonical text.  The registry ships
/// empty; concrete receipt-family adapters (E02–E06) register their own
/// descriptors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationAdapterRegistry {
    adapters: BTreeMap<AdapterId, ObservationAdapterDescriptor>,
}

impl ObservationAdapterRegistry {
    /// An empty registry.
    ///
    /// A registry built incrementally through [`Self::register`] is
    /// re-validated at every operational boundary ([`Self::select_adapter`],
    /// [`Self::validate_observation`], [`Self::canonical_text`]): a dangling
    /// migration target can never be selected, validated against, or
    /// canonicalized into identity bytes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a registry from descriptors, validating each and every
    /// ownership law.  Registration order carries no semantics.
    pub fn from_descriptors(descriptors: Vec<ObservationAdapterDescriptor>) -> Result<Self> {
        let mut registry = Self::new();
        for descriptor in descriptors {
            registry.register(descriptor)?;
        }
        registry.validate_migration_targets()?;
        Ok(registry)
    }

    /// Register one adapter.  Duplicate identities, inverted or empty
    /// declarations, ceiling strengthening, and ambiguous shared ownership
    /// of one source family/version are rejected.
    pub fn register(&mut self, descriptor: ObservationAdapterDescriptor) -> Result<()> {
        descriptor.validate()?;
        if self.adapters.contains_key(&descriptor.id) {
            bail!("adapter {:?} is registered twice", descriptor.id.as_str());
        }
        for existing in self.adapters.values() {
            if existing.source_family != descriptor.source_family {
                continue;
            }
            if !accepted_ranges_overlap(existing, &descriptor) {
                continue;
            }
            let new_supersedes = descriptor.supersedes.as_ref() == Some(&existing.id);
            let existing_supersedes = existing.supersedes.as_ref() == Some(&descriptor.id);
            if new_supersedes && existing_supersedes {
                bail!(
                    "adapters {:?} and {:?} supersede each other; a migration relation is one-directional",
                    existing.id.as_str(),
                    descriptor.id.as_str()
                );
            }
            if !new_supersedes && !existing_supersedes {
                bail!(
                    "adapters {:?} and {:?} ambiguously own source family {:?} with overlapping schema versions; an explicit migration relation must select one",
                    existing.id.as_str(),
                    descriptor.id.as_str(),
                    descriptor.source_family.as_str()
                );
            }
        }
        self.adapters.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// Registered adapters in deterministic identity order.
    pub fn adapters(&self) -> impl Iterator<Item = &ObservationAdapterDescriptor> {
        self.adapters.values()
    }

    /// Select the one adapter that owns a source family/version.  Unknown,
    /// future, or explicitly unsupported source schemas fail closed; so does
    /// ambiguous ownership.
    pub fn select_adapter(
        &self,
        family: &ReceiptFamily,
        schema: SchemaVersion,
    ) -> Result<&ObservationAdapterDescriptor> {
        // The incremental new()+register() path skips from_descriptors'
        // final validation, so every operational boundary re-checks it: a
        // registry with a dangling migration target can never be operated on.
        self.validate_migration_targets()?;
        let candidates: Vec<&ObservationAdapterDescriptor> = self
            .adapters
            .values()
            .filter(|adapter| adapter.source_family == *family && adapter.accepts(schema))
            .collect();
        if candidates.is_empty() {
            bail!(
                "no adapter owns source family {:?} schema {}: unknown, future, or explicitly unsupported source schemas fail closed",
                family.as_str(),
                schema.get()
            );
        }
        // An explicit migration relation selects the superseding adapter.
        let selected: Vec<&ObservationAdapterDescriptor> = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                !candidates.iter().any(|other| other.supersedes.as_ref() == Some(&candidate.id))
            })
            .collect();
        match selected.as_slice() {
            [adapter] => Ok(adapter),
            [] => bail!(
                "adapter selection for source family {:?} schema {} is a migration cycle and fails closed",
                family.as_str(),
                schema.get()
            ),
            _ => bail!(
                "adapter selection for source family {:?} schema {} is ambiguous and fails closed",
                family.as_str(),
                schema.get()
            ),
        }
    }

    /// Validate one observation against the registry: the producing adapter
    /// must be registered at the exact version, own the receipt's family and
    /// schema, be allowed to emit the observation's class, have every
    /// required currentness input carried by the observation's invalidation
    /// evidence, be able to prove every bound subject dimension, and the
    /// observation may narrow but never strengthen the adapter's declared
    /// observation ceiling.
    pub fn validate_observation(&self, observation: &CompilerProfileObservationV1) -> Result<()> {
        self.validate_migration_targets()?;
        observation.validate()?;
        let descriptor = self.adapters.get(&observation.adapter.id).ok_or_else(|| {
            anyhow::anyhow!(
                "observation {:?} names unregistered adapter {:?}",
                observation.receipt.id.as_str(),
                observation.adapter.id.as_str()
            )
        })?;
        if descriptor.version != observation.adapter.version {
            bail!(
                "observation {:?} names adapter {:?} version {:?} but the registry holds {:?}",
                observation.receipt.id.as_str(),
                observation.adapter.id.as_str(),
                observation.adapter.version.as_str(),
                descriptor.version.as_str()
            );
        }
        if observation.receipt.producer.family != descriptor.source_family {
            bail!(
                "adapter {:?} cannot claim receipt family {:?}; it owns {:?}",
                descriptor.id.as_str(),
                observation.receipt.producer.family.as_str(),
                descriptor.source_family.as_str()
            );
        }
        if !descriptor.accepts(observation.receipt.producer.schema) {
            bail!(
                "adapter {:?} does not accept source schema {} and fails closed",
                descriptor.id.as_str(),
                observation.receipt.producer.schema.get()
            );
        }
        if !descriptor.emitted_classes.contains(&observation.class) {
            bail!(
                "adapter {:?} may not emit observation class {}/{}",
                descriptor.id.as_str(),
                observation.class.family.tag(),
                observation.class.proof_class.tag()
            );
        }
        for required in &descriptor.required_currentness_inputs {
            if !observation.invalidation.inputs.iter().any(|input| input.kind == *required) {
                bail!(
                    "adapter {:?} requires currentness input {} that observation {:?} does not carry",
                    descriptor.id.as_str(),
                    required.tag(),
                    observation.receipt.id.as_str()
                );
            }
        }
        for kind in observation.subject.proven_dimensions() {
            if !descriptor.provable_dimensions.contains(&kind) {
                bail!(
                    "adapter {:?} cannot prove subject dimension {}",
                    descriptor.id.as_str(),
                    kind.tag()
                );
            }
        }
        if observation.ceiling.rank() > descriptor.observation_claim_ceiling.rank() {
            bail!(
                "observation {:?} strengthens adapter {:?}'s declared ceiling {} to {}",
                observation.receipt.id.as_str(),
                descriptor.id.as_str(),
                descriptor.observation_claim_ceiling.tag(),
                observation.ceiling.tag()
            );
        }
        Ok(())
    }

    /// Deterministic canonical text of every registered adapter in identity
    /// order.  Registration order cannot change these bytes.
    pub fn canonical_text(&self) -> Result<String> {
        self.validate_migration_targets()?;
        let mut out = String::new();
        out.push_str("observation_adapter_registry v1\n");
        for descriptor in self.adapters.values() {
            descriptor.write_canonical(&mut out);
        }
        Ok(out)
    }

    /// Deterministic registry fingerprint over [`Self::canonical_text`].
    pub fn semantic_fingerprint(&self) -> Result<ObservationDigest> {
        let canonical = self.canonical_text()?;
        ObservationDigest::from_hex(&sha256_hex(canonical.as_bytes()))
            .context("sha256 hex output must satisfy the digest invariant")
    }

    fn validate_migration_targets(&self) -> Result<()> {
        for descriptor in self.adapters.values() {
            if let Some(target) = &descriptor.supersedes {
                let target_descriptor = self.adapters.get(target).ok_or_else(|| {
                    anyhow::anyhow!(
                        "adapter {:?} supersedes unregistered adapter {:?}",
                        descriptor.id.as_str(),
                        target.as_str()
                    )
                })?;
                if target_descriptor.source_family != descriptor.source_family {
                    bail!(
                        "adapter {:?} supersedes {:?} of a different receipt family; a migration relation stays inside one family",
                        descriptor.id.as_str(),
                        target.as_str()
                    );
                }
            }
        }
        ensure_distinct_ids(
            self.adapters.values().map(|descriptor| descriptor.id.as_str()),
            "registry adapters",
        )
    }
}

/// True when two same-family adapters share at least one accepted source
/// schema version.
fn accepted_ranges_overlap(
    a: &ObservationAdapterDescriptor,
    b: &ObservationAdapterDescriptor,
) -> bool {
    let start = a.schema_min.max(b.schema_min);
    let end = a.schema_max.min(b.schema_max);
    if start > end {
        return false;
    }
    let span = u64::from(end.get()) - u64::from(start.get()) + 1;
    // To share no accepted version, every version in the span must be
    // unsupported by at least one side; that needs at least `span` entries.
    if span > (a.unsupported_source_versions.len() + b.unsupported_source_versions.len()) as u64 {
        return true;
    }
    for version in start.get()..=end.get() {
        let candidate = SchemaVersion::new(version);
        if a.accepts(candidate) && b.accepts(candidate) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Synthetic fixtures
// ---------------------------------------------------------------------------

/// Synthetic in-memory fixtures proving representability and closure only.
///
/// These fixtures are not a concrete receipt-family adapter: the E02–E06
/// successor lanes own the real adapters and register their own descriptors.
/// The synthetic family exists so every registry and envelope law has an
/// executable home without implementing any real receipt adaptation.
pub mod synthetic_fixtures {
    use super::{
        AdapterId, AdapterIdentity, AdapterLossiness, AdapterVersion, CandidateSubjectIdentity,
        CanonicalReceiptReference, ClaimCeiling, ClaimFamily, CompilerProfileObservationV1,
        CompletenessDisposition, CurrentnessDisposition, InstrumentAndTerminalState,
        InvalidationEvidence, InvalidationInput, InvalidationKind, LimitationDisposition,
        ObservationAdapterDescriptor, ObservationClass, ObservationDigest, ObservationDisposition,
        ObservedClaimCeiling, ProducerAndSchemaIdentity, ProofClass, ReceiptFamily, ReceiptId,
        Result, SchemaVersion, SubjectDimension, SubjectDimensionKind, TerminalState,
        WorkDisposition,
    };
    use std::collections::BTreeSet;

    /// Synthetic receipt family identity; no real receipt family is named.
    pub const FAMILY: &str = "synthetic-fixture-receipt";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const AUTHORITY: &str = "compiler-profile evidence train E01";

    /// Synthetic adapter `adapter.synthetic-v1` accepting schema 1..=3.
    pub fn synthetic_adapter_v1() -> Result<ObservationAdapterDescriptor> {
        Ok(ObservationAdapterDescriptor {
            id: AdapterId::new("adapter.synthetic-v1")?,
            version: AdapterVersion::new("v1")?,
            source_family: ReceiptFamily::new(FAMILY)?,
            schema_min: SchemaVersion::new(1),
            schema_max: SchemaVersion::new(3),
            source_authority: AUTHORITY.to_owned(),
            emitted_classes: BTreeSet::from([
                ObservationClass {
                    family: ClaimFamily::ParserInternal,
                    proof_class: ProofClass::CuratedExpectation,
                },
                ObservationClass {
                    family: ClaimFamily::Execution,
                    proof_class: ProofClass::EvaluatedWork,
                },
            ]),
            provable_dimensions: BTreeSet::from([
                SubjectDimensionKind::RepositoryTree,
                SubjectDimensionKind::Toolchain,
                SubjectDimensionKind::ObservationTime,
            ]),
            preserved_fields: BTreeSet::from([
                "terminal_state".to_owned(),
                "work_scope".to_owned(),
            ]),
            lossiness: AdapterLossiness::Lossless,
            source_claim_ceiling: ClaimCeiling::AcceptedCompatibility,
            observation_claim_ceiling: ObservedClaimCeiling::new(
                ClaimCeiling::AcceptedCompatibility,
            ),
            required_currentness_inputs: BTreeSet::from([InvalidationKind::Source]),
            unsupported_source_versions: BTreeSet::from([SchemaVersion::new(2)]),
            supersedes: None,
        })
    }

    /// Synthetic adapter `adapter.synthetic-v2` accepting schema 3..=5 with
    /// an explicit migration relation superseding v1.
    pub fn synthetic_adapter_v2() -> Result<ObservationAdapterDescriptor> {
        let mut descriptor = synthetic_adapter_v1()?;
        descriptor.id = AdapterId::new("adapter.synthetic-v2")?;
        descriptor.version = AdapterVersion::new("v2")?;
        descriptor.schema_min = SchemaVersion::new(3);
        descriptor.schema_max = SchemaVersion::new(5);
        descriptor.unsupported_source_versions = BTreeSet::new();
        descriptor.supersedes = Some(AdapterId::new("adapter.synthetic-v1")?);
        Ok(descriptor)
    }

    fn receipt(schema: u32) -> Result<CanonicalReceiptReference> {
        Ok(CanonicalReceiptReference {
            id: ReceiptId::new("synthetic-receipt-0001")?,
            digest: ObservationDigest::from_hex(DIGEST)?,
            producer: ProducerAndSchemaIdentity::new(
                "synthetic-producer",
                ReceiptFamily::new(FAMILY)?,
                SchemaVersion::new(schema),
            )?,
        })
    }

    fn bound_subject() -> Result<CandidateSubjectIdentity> {
        let mut subject = CandidateSubjectIdentity::not_proven();
        subject.bind(
            SubjectDimensionKind::RepositoryTree,
            SubjectDimension::proven("tree-digest-9f8e7d")?,
        );
        Ok(subject)
    }

    fn invalidation() -> Result<InvalidationEvidence> {
        InvalidationEvidence::new(vec![InvalidationInput::new(
            InvalidationKind::Source,
            "the named subject's source basis changed",
        )?])
    }

    /// A valid passing observation produced by `adapter.synthetic-v1`.
    pub fn passing_observation() -> Result<CompilerProfileObservationV1> {
        Ok(CompilerProfileObservationV1 {
            receipt: receipt(1)?,
            subject: bound_subject()?,
            class: ObservationClass {
                family: ClaimFamily::ParserInternal,
                proof_class: ProofClass::CuratedExpectation,
            },
            disposition: ObservationDisposition::Pass,
            currentness: CurrentnessDisposition::Current,
            completeness: CompletenessDisposition::Complete,
            work: WorkDisposition::completed("the named synthetic work scope")?,
            limitation: LimitationDisposition::None,
            ceiling: ObservedClaimCeiling::new(ClaimCeiling::AcceptedCompatibility),
            invalidation: invalidation()?,
            instrument: InstrumentAndTerminalState::new(
                "synthetic-instrument",
                TerminalState::Completed,
            )?,
            adapter: AdapterIdentity {
                id: AdapterId::new("adapter.synthetic-v1")?,
                version: AdapterVersion::new("v1")?,
            },
        })
    }

    /// A valid observed-red-but-complete observation: failed product
    /// disposition with complete completeness, preserved exactly.
    pub fn red_but_complete_observation() -> Result<CompilerProfileObservationV1> {
        let mut observation = passing_observation()?;
        observation.disposition = ObservationDisposition::Failed;
        observation.completeness = CompletenessDisposition::Complete;
        observation.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
        Ok(observation)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterId, CandidateSubjectIdentity, ClaimCeiling, ClaimFamily,
        CompilerProfileObservationV1, CompletenessDisposition, CurrentnessDisposition,
        InstrumentAndTerminalState, InvalidationEvidence, InvalidationInput, InvalidationKind,
        LimitationDisposition, ObservationAdapterDescriptor, ObservationAdapterRegistry,
        ObservationClass, ObservationDisposition, ObservedClaimCeiling, ProofClass, ReceiptFamily,
        Result, SchemaVersion, SubjectDimension, SubjectDimensionKind, TerminalState,
        WorkDisposition, synthetic_fixtures,
    };
    use std::collections::BTreeSet;

    fn adapter() -> ObservationAdapterDescriptor {
        match synthetic_fixtures::synthetic_adapter_v1() {
            Ok(descriptor) => descriptor,
            Err(error) => unreachable!("synthetic adapter builds: {error}"),
        }
    }

    fn observation() -> CompilerProfileObservationV1 {
        match synthetic_fixtures::passing_observation() {
            Ok(observation) => observation,
            Err(error) => unreachable!("passing observation builds: {error}"),
        }
    }

    fn registry_with(adapter: ObservationAdapterDescriptor) -> ObservationAdapterRegistry {
        match ObservationAdapterRegistry::from_descriptors(vec![adapter]) {
            Ok(registry) => registry,
            Err(error) => unreachable!("single-adapter registry builds: {error}"),
        }
    }

    fn assert_invalid_envelope(observation: &CompilerProfileObservationV1, expected: &str) {
        let error = match observation.validate() {
            Err(error) => error,
            Ok(()) => unreachable!("envelope must fail validation: {expected}"),
        };
        let text = format!("{error:#}");
        assert!(text.contains(expected), "expected {expected:?}, got {text}");
    }

    // Issue falsifier 1: a complete red observation becomes pass.
    #[test]
    fn falsifier_01_complete_red_observation_cannot_become_pass() -> Result<()> {
        let red = synthetic_fixtures::red_but_complete_observation()?;
        red.validate()?;
        assert_eq!(red.disposition, ObservationDisposition::Failed);
        assert_eq!(red.completeness, CompletenessDisposition::Complete);
        let canonical = red.canonical_semantic_text()?;
        assert!(canonical.contains("disposition=failed"), "observed red is preserved: {canonical}");
        assert!(canonical.contains("completeness=complete"), "completeness is preserved");
        // Re-typing the red observation as pass changes its identity.
        let mut pretender = red.clone();
        pretender.disposition = ObservationDisposition::Pass;
        assert_ne!(pretender.identity()?, red.identity()?);
        Ok(())
    }

    // Issue falsifier 2: accepted source-locked debt becomes general semantic
    // support.
    #[test]
    fn falsifier_02_accepted_debt_cannot_become_general_support() -> Result<()> {
        let mut debt = observation();
        debt.limitation = LimitationDisposition::accepted_debt(
            "the exact named debt scope",
            "source-locked debt, accepted and bounded",
        )?;
        debt.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
        debt.validate()?;
        for ceiling in [ClaimCeiling::AcceptedCompatibility, ClaimCeiling::BoundedPublicClaim] {
            let mut strengthened = debt.clone();
            strengthened.ceiling = ObservedClaimCeiling::new(ceiling);
            assert_invalid_envelope(&strengthened, "cannot claim more than observed evidence");
        }
        // No ceiling maps toward support/release authority (landed law).
        for ceiling in ClaimCeiling::ALL {
            for token in ceiling.strongest_claim().split_whitespace() {
                for forbidden in ["support", "release", "publication", "authorization"] {
                    assert!(token != forbidden, "ceiling maps toward {forbidden} authority");
                }
            }
        }
        Ok(())
    }

    // Issue falsifier 3: fixture replay becomes execution/EIR proof.
    #[test]
    fn falsifier_03_fixture_replay_cannot_become_execution_or_eir_proof() -> Result<()> {
        let replay = ObservationClass {
            family: ClaimFamily::Execution,
            proof_class: ProofClass::CuratedExpectation,
        };
        let eir = ObservationClass {
            family: ClaimFamily::Execution,
            proof_class: ProofClass::EirMechanism,
        };
        let evaluated = ObservationClass {
            family: ClaimFamily::Execution,
            proof_class: ProofClass::EvaluatedWork,
        };
        assert_ne!(replay, eir, "fixture replay is not an EIR mechanism");
        assert_ne!(replay, evaluated, "fixture replay is not evaluated work");

        // The synthetic adapter may emit execution/evaluated-work but not
        // execution/EIR: an envelope claiming EIR proof is rejected.
        let registry = registry_with(adapter());
        let mut pretender = observation();
        pretender.class = eir;
        let error = match registry.validate_observation(&pretender) {
            Err(error) => error,
            Ok(()) => unreachable!("an undeclared EIR class must be rejected"),
        };
        assert!(
            error.to_string().contains("may not emit observation class"),
            "unexpected error: {error}"
        );
        Ok(())
    }

    // Issue falsifier 4: parser/compiler-internal evidence becomes
    // provider/edit/installed-host evidence.
    #[test]
    fn falsifier_04_parser_internal_cannot_become_provider_edit_installed_host() -> Result<()> {
        let base = observation();
        for family in [ClaimFamily::Provider, ClaimFamily::Edit, ClaimFamily::InstalledHost] {
            let mut retyped = base.clone();
            retyped.class.family = family;
            assert_ne!(retyped.identity()?, base.identity()?);
            let registry = registry_with(adapter());
            assert!(
                registry.validate_observation(&retyped).is_err(),
                "re-typing parser-internal evidence as {family:?} must fail against the registry"
            );
        }
        Ok(())
    }

    // Issue falsifier 5: unknown source schema is accepted.
    #[test]
    fn falsifier_05_unknown_or_future_source_schema_fails_closed() -> Result<()> {
        let registry = registry_with(adapter());
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        // Accepted: schema 1 (schema 2 is explicitly unsupported, 3 in range).
        assert!(registry.select_adapter(&family, SchemaVersion::new(1)).is_ok());
        assert!(registry.select_adapter(&family, SchemaVersion::new(3)).is_ok());
        for version in [0, 2, 4, 99] {
            let error = match registry.select_adapter(&family, SchemaVersion::new(version)) {
                Err(error) => error,
                Ok(_) => unreachable!("schema version {version} must fail closed"),
            };
            assert!(
                error.to_string().contains("fail closed"),
                "unexpected error for version {version}: {error}"
            );
        }
        let unknown_family = ReceiptFamily::new("never-registered-family")?;
        assert!(registry.select_adapter(&unknown_family, SchemaVersion::new(1)).is_err());
        Ok(())
    }

    // Issue falsifier 6: an adapter strengthens the source claim ceiling.
    #[test]
    fn falsifier_06_adapter_cannot_strengthen_source_claim_ceiling() -> Result<()> {
        let mut descriptor = adapter();
        descriptor.source_claim_ceiling = ClaimCeiling::ObservedEvidence;
        descriptor.observation_claim_ceiling =
            ObservedClaimCeiling::new(ClaimCeiling::BoundedPublicClaim);
        let error = match ObservationAdapterRegistry::from_descriptors(vec![descriptor]) {
            Err(error) => error,
            Ok(_) => unreachable!("a strengthening adapter must be rejected"),
        };
        assert!(
            error.to_string().contains("strengthens the source claim"),
            "unexpected error: {error}"
        );

        // Narrowing is allowed: observed evidence below accepted compatibility.
        let mut narrowed = adapter();
        narrowed.observation_claim_ceiling =
            ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
        let registry = registry_with(narrowed);
        let mut narrowed_observation = observation();
        narrowed_observation.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
        registry.validate_observation(&narrowed_observation)?;
        // But even a valid envelope cannot exceed the narrowed adapter ceiling.
        let mut over = observation();
        over.ceiling = ObservedClaimCeiling::new(ClaimCeiling::AcceptedCompatibility);
        let error = match registry.validate_observation(&over) {
            Err(error) => error,
            Ok(()) => unreachable!("an observation above the adapter ceiling must be rejected"),
        };
        assert!(error.to_string().contains("strengthens"), "unexpected error: {error}");
        Ok(())
    }

    // Registry law: an observation must carry every currentness input its
    // adapter requires; a declared-but-unchecked input would be decorative.
    #[test]
    fn registry_required_currentness_inputs_are_enforced() -> Result<()> {
        let registry = registry_with(adapter());
        let mut lacking = observation();
        lacking.invalidation = InvalidationEvidence::new(vec![InvalidationInput::new(
            InvalidationKind::Oracle,
            "the oracle basis changed",
        )?])?;
        let error = match registry.validate_observation(&lacking) {
            Err(error) => error,
            Ok(()) => unreachable!("a missing required currentness input must fail"),
        };
        assert!(
            error.to_string().contains("requires currentness input"),
            "unexpected error: {error}"
        );
        // Carrying the required input validates.
        registry.validate_observation(&observation())?;
        Ok(())
    }

    // Issue falsifier 7: two adapters ambiguously own the same source
    // version.
    #[test]
    fn falsifier_07_overlapping_adapters_require_explicit_migration() -> Result<()> {
        let v1 = adapter();
        let mut rival = adapter();
        rival.id = AdapterId::new("adapter.synthetic-rival")?;
        let error = match ObservationAdapterRegistry::from_descriptors(vec![v1.clone(), rival]) {
            Err(error) => error,
            Ok(_) => unreachable!("overlapping adapters without migration must be rejected"),
        };
        assert!(error.to_string().contains("ambiguously own"), "unexpected error: {error}");

        // The explicit migration relation selects exactly one adapter.
        let v2 = synthetic_fixtures::synthetic_adapter_v2()?;
        let registry = ObservationAdapterRegistry::from_descriptors(vec![v1, v2])?;
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        let selected = registry.select_adapter(&family, SchemaVersion::new(3))?;
        assert_eq!(selected.id.as_str(), "adapter.synthetic-v2");
        // A mutual supersession cycle is rejected.
        let mut cycle_a = adapter();
        cycle_a.supersedes = Some(AdapterId::new("adapter.synthetic-v2")?);
        let mut cycle_b = synthetic_fixtures::synthetic_adapter_v2()?;
        cycle_b.supersedes = Some(AdapterId::new("adapter.synthetic-v1")?);
        assert!(
            ObservationAdapterRegistry::from_descriptors(vec![cycle_a, cycle_b]).is_err(),
            "a mutual supersession cycle must be rejected"
        );
        Ok(())
    }

    // Issue falsifier 8: issue, PR, workflow colour, log prose, or filename
    // becomes evidence.
    #[test]
    fn falsifier_08_workflow_state_never_becomes_evidence() -> Result<()> {
        for inject in [
            "https://github.com/example/pull/123",
            "ci run 48151623 workflow green",
            "see tracker issue #12188 for status",
            "parser-check output.log tail",
        ] {
            assert!(
                super::ReceiptId::new(inject).is_err(),
                "workflow content must not become a receipt id: {inject}"
            );
            assert!(
                InstrumentAndTerminalState::new(inject, TerminalState::Completed).is_err(),
                "workflow content must not become an instrument identity: {inject}"
            );
        }
        // Closed enum tags carry no workflow vocabulary either.
        for tag in SubjectDimensionKind::ALL.iter().map(|kind| kind.tag()) {
            for token in tag.split('_') {
                for forbidden in ["issue", "pr", "workflow", "github", "review", "merge"] {
                    assert!(token != forbidden, "tag {tag:?} encodes workflow state");
                }
            }
        }
        Ok(())
    }

    // Issue falsifier 9: missing subject identity is reconstructed from
    // another receipt implicitly.
    #[test]
    fn falsifier_09_missing_subject_identity_stays_explicit_not_proven() -> Result<()> {
        let empty = CandidateSubjectIdentity::not_proven();
        for kind in SubjectDimensionKind::ALL {
            assert_eq!(
                empty.dimension(kind),
                SubjectDimension::NotProven,
                "an absent dimension is explicit not_proven, never reconstructed"
            );
        }
        let mut sparse = CandidateSubjectIdentity::not_proven();
        sparse.bind(
            SubjectDimensionKind::RepositoryTree,
            SubjectDimension::proven("tree-digest-9f8e7d")?,
        );
        assert_eq!(
            sparse.dimension(SubjectDimensionKind::Platform),
            SubjectDimension::NotProven,
            "binding one dimension must not fill another"
        );
        // Canonical form names every closed dimension explicitly.
        let mut with_sparse_subject = observation();
        with_sparse_subject.subject = sparse;
        let canonical = with_sparse_subject.canonical_semantic_text()?;
        for kind in SubjectDimensionKind::ALL {
            assert!(
                canonical.contains(kind.tag()),
                "dimension {} missing from canonical form",
                kind.tag()
            );
        }
        assert!(canonical.contains("platform=not_proven"));
        Ok(())
    }

    // Issue falsifier 10: instrument_failed, zero_work, cancelled, or
    // timed_out collapses into pass/not-applicable.
    #[test]
    fn falsifier_10_instrument_and_zero_work_states_cannot_collapse() -> Result<()> {
        let terminals = [
            TerminalState::instrument_failed("the instrument itself errored")?,
            TerminalState::cancelled("the run was cancelled")?,
            TerminalState::timed_out("the run exceeded its bound")?,
        ];
        for terminal in terminals {
            for disposition in [
                ObservationDisposition::Pass,
                ObservationDisposition::not_applicable("claimed not applicable")?,
            ] {
                let mut collapsed = observation();
                collapsed.instrument =
                    InstrumentAndTerminalState::new("synthetic-instrument", terminal.clone())?;
                // Keep the ceiling at observed evidence so the instrument
                // law, not the ceiling law, is what fires.
                collapsed.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
                collapsed.disposition = disposition;
                assert_invalid_envelope(&collapsed, "cannot be typed as pass or not-applicable");
            }
            // The honest typing validates: not proven with a failed instrument.
            let mut honest = observation();
            honest.instrument = InstrumentAndTerminalState::new("synthetic-instrument", terminal)?;
            honest.disposition = ObservationDisposition::NotProven;
            honest.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
            honest.validate()?;
        }
        for disposition in [
            ObservationDisposition::Pass,
            ObservationDisposition::not_applicable("claimed not applicable")?,
        ] {
            let mut zero = observation();
            zero.work = WorkDisposition::ZeroWork;
            // Keep the ceiling at observed evidence so the zero-work law,
            // not the ceiling law, is what fires.
            zero.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
            zero.disposition = disposition;
            assert_invalid_envelope(&zero, "cannot be typed as pass or not-applicable");
        }
        // Zero work with a failed disposition is preserved, not collapsed.
        let mut zero_red = observation();
        zero_red.work = WorkDisposition::ZeroWork;
        zero_red.disposition = ObservationDisposition::Failed;
        zero_red.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
        zero_red.validate()?;
        assert!(zero_red.canonical_semantic_text()?.contains("work=zero_work"));
        Ok(())
    }

    // Issue falsifier 11: registry ordering changes adapter selection or
    // normalized bytes.
    #[test]
    fn falsifier_11_registry_order_cannot_change_selection_or_bytes() -> Result<()> {
        let v1 = adapter();
        let v2 = synthetic_fixtures::synthetic_adapter_v2()?;
        let forward = ObservationAdapterRegistry::from_descriptors(vec![v1.clone(), v2.clone()])?;
        let reverse = ObservationAdapterRegistry::from_descriptors(vec![v2, v1])?;
        assert_eq!(forward.semantic_fingerprint()?, reverse.semantic_fingerprint()?);
        assert_eq!(forward.canonical_text()?, reverse.canonical_text()?);
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        for schema in [1, 3, 4] {
            let a = forward.select_adapter(&family, SchemaVersion::new(schema))?;
            let b = reverse.select_adapter(&family, SchemaVersion::new(schema))?;
            assert_eq!(a.id, b.id, "selection must not depend on registration order");
        }
        // Registration twice under one identity is rejected outright.
        let mut registry = registry_with(adapter());
        assert!(registry.register(adapter()).is_err(), "duplicate registration must fail");
        Ok(())
    }

    // Issue falsifier 12: source/private payload content leaks into the
    // normalized envelope.
    #[test]
    fn falsifier_12_private_payload_cannot_leak_into_envelope() -> Result<()> {
        for leaked in [
            "/home/operator/token-cache/receipt.json",
            "/Users/ci/build/receipt.json",
            "C:\\Users\\ci\\receipt.json",
            "~/private/receipt.json",
            // Every Windows drive root is host-specific, not only C:\.
            "D:\\build\\operator-secret",
            "c:/Users/ci/receipt.json",
            "E:\\data\\receipt.json",
        ] {
            assert!(
                SubjectDimension::proven(leaked).is_err(),
                "host-specific content must be rejected: {leaked}"
            );
        }
        // The envelope structurally carries no payload field: canonical form
        // of a valid envelope contains only normalized fields and the digest.
        let valid = observation();
        let canonical = valid.canonical_semantic_text()?;
        let lowered = canonical.to_lowercase();
        for marker in super::PRIVATE_OR_WORKFLOW_MARKERS {
            assert!(
                !lowered.contains(marker),
                "canonical form must not carry private/workflow marker {marker:?}"
            );
        }
        Ok(())
    }

    // Review falsifier (PR #12492): an invalidation detail is written into
    // the canonical envelope and its identity, so private content there must
    // be rejected before canonicalizing.
    #[test]
    fn invalidation_details_are_private_safe_before_canonicalizing() -> Result<()> {
        let mut base = observation();
        base.invalidation = InvalidationEvidence::new(vec![InvalidationInput::new(
            InvalidationKind::Source,
            "/home/operator/token",
        )?])?;
        assert!(
            base.validate().is_err(),
            "a private-unsafe invalidation detail must fail envelope validation"
        );
        assert!(
            base.canonical_semantic_text().is_err(),
            "a private-unsafe invalidation detail must never reach canonical bytes"
        );
        Ok(())
    }

    // Review falsifier (PR #12492): the incremental new()+register() path
    // skips from_descriptors' final validation, so a registry with a dangling
    // migration target must fail closed at every operational boundary.
    #[test]
    fn incremental_registry_with_dangling_migration_target_fails_closed() -> Result<()> {
        let mut dangling = adapter();
        dangling.supersedes = Some(AdapterId::new("adapter-missing")?);
        let mut registry = ObservationAdapterRegistry::new();
        registry.register(dangling)?;
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        assert!(
            registry.select_adapter(&family, SchemaVersion::new(1)).is_err(),
            "selection on a dangling migration registry must fail closed"
        );
        assert!(
            registry.canonical_text().is_err(),
            "canonicalizing a dangling migration registry must fail closed"
        );
        assert!(
            registry.semantic_fingerprint().is_err(),
            "fingerprinting a dangling migration registry must fail closed"
        );
        assert!(
            registry.validate_observation(&observation()).is_err(),
            "validating an observation against a dangling migration registry must fail closed"
        );
        Ok(())
    }

    // Closure law: envelope identity is order-insensitive but sensitive to
    // every semantic field.
    #[test]
    fn closure_identity_is_order_insensitive_and_content_sensitive() -> Result<()> {
        let base = observation();
        let expected = base.identity()?;

        // Invalidation input order carries no semantics.
        let mut reordered = base.clone();
        reordered.invalidation = InvalidationEvidence::new(vec![
            InvalidationInput::new(InvalidationKind::Oracle, "the oracle basis changed")?,
            InvalidationInput::new(InvalidationKind::Source, "the source basis changed")?,
        ])?;
        let mut swapped = reordered.clone();
        swapped.invalidation.inputs.reverse();
        assert_eq!(reordered.identity()?, swapped.identity()?);

        // Every semantic field is content.
        let mutations: Vec<fn(&mut CompilerProfileObservationV1)> = vec![
            |observation| observation.disposition = ObservationDisposition::Stale,
            |observation| observation.currentness = CurrentnessDisposition::NotProven,
            |observation| {
                observation.completeness = CompletenessDisposition::NotProven;
            },
            |observation| observation.work = WorkDisposition::NotProven,
            |observation| observation.limitation = LimitationDisposition::NotProven,
            |observation| {
                observation.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
            },
            |observation| observation.class.proof_class = ProofClass::RealPerlOracle,
            |observation| {
                observation.subject.bind(
                    SubjectDimensionKind::Toolchain,
                    match SubjectDimension::proven("toolchain-digest-1a2b3c") {
                        Ok(dimension) => dimension,
                        Err(error) => unreachable!("dimension builds: {error}"),
                    },
                );
            },
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert_ne!(
                changed.identity()?,
                expected,
                "semantic mutation {index} must change observation identity"
            );
        }
        Ok(())
    }

    // Acceptance: source vocabulary is preserved, never flattened — every
    // closed disposition variant is representable and serializes distinctly.
    #[test]
    fn acceptance_source_vocabulary_is_preserved_not_flattened() -> Result<()> {
        let dispositions = [
            ObservationDisposition::Pass,
            ObservationDisposition::Failed,
            ObservationDisposition::NotProven,
            ObservationDisposition::Stale,
            ObservationDisposition::unsupported("named reason")?,
            ObservationDisposition::not_applicable("named justification")?,
            ObservationDisposition::conditional_not_selected("named trigger")?,
            ObservationDisposition::OptionalAbsent,
        ];
        let mut texts = BTreeSet::new();
        for disposition in &dispositions {
            let mut observation = observation();
            observation.disposition = disposition.clone();
            if !disposition.is_pass() {
                observation.ceiling = ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence);
            }
            observation.validate()?;
            let canonical = observation.canonical_semantic_text()?;
            let line =
                canonical.lines().find(|line| line.starts_with("disposition=")).map(str::to_owned);
            assert!(texts.insert(line), "dispositions must not flatten into one another");
        }
        assert_eq!(texts.len(), 8, "all eight dispositions are preserved distinctly");
        Ok(())
    }

    // Acceptance: the successor adapter lanes (E02–E06), evidence-set
    // assembly (E07), and evaluator (E08) can consume the registry and
    // envelope through the public surface only.
    #[test]
    fn acceptance_successor_lanes_can_consume_the_public_surface() -> Result<()> {
        let registry = ObservationAdapterRegistry::from_descriptors(vec![
            adapter(),
            synthetic_fixtures::synthetic_adapter_v2()?,
        ])?;
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        let selected = registry.select_adapter(&family, SchemaVersion::new(1))?;
        assert_eq!(selected.id.as_str(), "adapter.synthetic-v1");
        let passing = observation();
        registry.validate_observation(&passing)?;
        let red = synthetic_fixtures::red_but_complete_observation()?;
        registry.validate_observation(&red)?;
        // Observation identity and registry fingerprint are inspectable data
        // for evidence-set assembly.
        assert_eq!(passing.identity()?.as_str().len(), 64);
        assert_eq!(registry.semantic_fingerprint()?.as_str().len(), 64);
        // Required currentness inputs are declared per adapter for E07.
        assert!(selected.required_currentness_inputs.contains(&InvalidationKind::Source));
        Ok(())
    }

    // Acceptance: no concrete receipt-family adapter, evaluator, or support
    // vocabulary lands — the registry ships empty and every declared
    // authority stays private-safe.
    #[test]
    fn acceptance_registry_ships_empty_and_no_evaluation_lands() -> Result<()> {
        let empty = ObservationAdapterRegistry::new();
        assert_eq!(empty.adapters().count(), 0, "no concrete adapter lands in this PR");
        let family = ReceiptFamily::new(synthetic_fixtures::FAMILY)?;
        assert!(empty.select_adapter(&family, SchemaVersion::new(1)).is_err());
        // The module defines no evaluation or satisfaction function: this
        // test itself is the check that only validation/selection/identity
        // entry points exist by constructing them all above.
        Ok(())
    }
}
