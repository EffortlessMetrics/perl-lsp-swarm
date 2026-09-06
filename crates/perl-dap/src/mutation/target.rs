//! Mutation location, inspected-value, and target identity (#10736).
//!
//! The contract keeps three propositions apart, because collapsing any two of
//! them is what makes a debugger write to the wrong storage:
//!
//! ```text
//! MutationLocationProvenance  exact current writable storage cell (#11310)
//! InspectedValueIdentity      observed value / referent / graph subject (#9048, #9050)
//! MutationTarget              admitted writable operation subject
//! ```
//!
//! Equal values can live in different locations; one referent can be reached
//! through several locations; a location outlives the value currently in it.
//! So a value node, referent, display name, `evaluateName`, source range,
//! pointer text, or public DAP handle can never *identify* what to write.
//!
//! # Why binding is a two-step
//!
//! [`MutationTargetCandidate`] is deliberately constructible while incomplete,
//! mirroring [`SubjectCandidate`](crate::reload::subject::SubjectCandidate):
//! acquisition produces a partial claim, and
//! [`MutationTargetCandidate::bind`] is the single place that either yields a
//! sealed [`MutationTarget`] or names exactly why the claim failed. There is
//! no other constructor, so an unbindable claim cannot become a target.
//!
//! The candidate has **no field** for a DAP `frameId`, `variablesReference`,
//! display name, `evaluateName`, source range, or pointer text. That is the
//! structural half of the recurrence control: there is no slot those
//! identities fit into, so they cannot become a target's identity by being
//! passed in.
//!
//! # What that does not prevent
//!
//! `frame_identity` and `binding_identity` are opaque `String`s, so a caller
//! could still *put* a display spelling or a stringified `frameId` inside one.
//! Nothing in this contract can detect that, because it never sees the DAP
//! request. Keeping those identities exact is the acquisition path's
//! obligation (#11310), which produces them from observed runtime storage
//! facts rather than from request fields. When that path lands, the honest
//! tightening is to make them newtypes only acquisition can mint; until it
//! exists, there is nothing to constrain them against.

use serde::Serialize;

/// Version of the supported mutation-target profile.
pub const MUTATION_TARGET_PROFILE_VERSION: u32 = 1;

/// Kind of storage a location denotes.
///
/// The two deferred variants are representable so that acquisition can record
/// an honest "this is a real location, and it is out of scope for v1" rather
/// than silently dropping the row — but they never bind to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationLocationKind {
    /// A lexical scalar binding in the current executable frame.
    CurrentFrameLexicalScalar,
    /// An element of an array reached from the current frame.
    CurrentFrameArrayElement,
    /// An entry of a hash reached from the current frame.
    CurrentFrameHashEntry,
    /// Package/global scalar. Deferred to #11323; never admitted in v1.
    PackageScalar,
    /// Scalar in a non-current ordinary frame. Deferred to #11324/#11325.
    NonCurrentFrameScalar,
}

impl MutationLocationKind {
    /// Whether this kind is admitted by the v1 supported-location table.
    pub fn is_supported_in_v1(self) -> bool {
        matches!(
            self,
            Self::CurrentFrameLexicalScalar
                | Self::CurrentFrameArrayElement
                | Self::CurrentFrameHashEntry
        )
    }
}

/// Which cell inside the binding a location denotes.
///
/// Hash keys carry exact key *data*. Client display escaping is a rendering
/// concern and never reaches this type, so an empty key, a key containing a
/// quote, a backslash, a control character, or a digit-looking key stays
/// exactly the bytes the runtime observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationMember {
    /// The whole scalar binding.
    WholeScalar,
    /// An exact bounded array index.
    ArrayIndex(i64),
    /// Exact hash key data.
    HashKey(String),
}

impl MutationMember {
    /// Whether this member selector is coherent with a location kind.
    fn matches_kind(&self, kind: MutationLocationKind) -> bool {
        matches!(
            (self, kind),
            (
                Self::WholeScalar,
                MutationLocationKind::CurrentFrameLexicalScalar
                    | MutationLocationKind::PackageScalar
                    | MutationLocationKind::NonCurrentFrameScalar
            ) | (Self::ArrayIndex(_), MutationLocationKind::CurrentFrameArrayElement)
                | (Self::HashKey(_), MutationLocationKind::CurrentFrameHashEntry)
        )
    }

    /// Whether this member addresses a container cell rather than a whole
    /// binding.
    fn is_container_member(&self) -> bool {
        matches!(self, Self::ArrayIndex(_) | Self::HashKey(_))
    }
}

/// Identity of an *observed value* — never of storage.
///
/// Produced by the inspection/value-graph path (#9048, #9050). It may
/// accompany a location, and it is what proves "these two locations currently
/// hold the same referent", but on its own it addresses nothing writable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedValueIdentity {
    /// Value-graph node identity of the observation.
    pub value_node: String,
    /// Runtime referent identity, when the observation proved one.
    pub referent: Option<String>,
    /// Value-authority generation the observation was made under.
    pub value_authority_generation: u64,
}

/// Exact current writable storage location.
///
/// Sealed: the only producer is [`MutationTargetCandidate::bind`], so every
/// provenance value is generation-bound and kind/member-coherent by
/// construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationLocationProvenance {
    session_generation: u64,
    suspension_generation: u64,
    value_authority_generation: u64,
    frame_identity: String,
    binding_identity: String,
    kind: MutationLocationKind,
    member: MutationMember,
    referent_identity: Option<String>,
    profile_version: u32,
}

impl MutationLocationProvenance {
    /// Debuggee process/session generation the location was acquired under.
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// Suspension generation the location was acquired under.
    pub fn suspension_generation(&self) -> u64 {
        self.suspension_generation
    }

    /// Value-authority generation the location was acquired under.
    ///
    /// Part of target identity, not a separate caller-supplied expectation:
    /// [`MutationOperation`](crate::mutation::MutationOperation) reads it from
    /// here, so an operation cannot claim a value authority the target was
    /// never bound under.
    pub fn value_authority_generation(&self) -> u64 {
        self.value_authority_generation
    }

    /// Exact observed frame identity. Never a DAP `frameId`.
    pub fn frame_identity(&self) -> &str {
        &self.frame_identity
    }

    /// Exact observed storage binding identity. Never a display spelling.
    pub fn binding_identity(&self) -> &str {
        &self.binding_identity
    }

    /// Kind of storage denoted.
    pub fn kind(&self) -> MutationLocationKind {
        self.kind
    }

    /// Which cell inside the binding is denoted.
    pub fn member(&self) -> &MutationMember {
        &self.member
    }

    /// Runtime referent identity, when proven. Never the sole identity.
    pub fn referent_identity(&self) -> Option<&str> {
        self.referent_identity.as_deref()
    }

    /// Supported-location profile version.
    pub fn profile_version(&self) -> u32 {
        self.profile_version
    }
}

/// Whether an acquired location may be written.
///
/// The [`Default`] is [`WritabilityDisposition::NotProven`], so a candidate
/// built field-by-field fails closed: writability must be positively
/// established by the acquisition path, never inherited from a zero value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum WritabilityDisposition {
    /// Proven writable for the admitted cohort.
    Writable,
    /// Proven present but not writable.
    ReadOnly,
    /// Present but with no addressable storage cell.
    Unaddressable,
    /// Not established. Uncertainty fails closed and never becomes writable.
    #[default]
    NotProven,
}

/// Why a location was *not* writable.
///
/// This is [`WritabilityDisposition`] minus [`WritabilityDisposition::Writable`],
/// and it exists so a refusal cannot carry the one disposition that would
/// contradict it. A `NotWritable(Writable)` refusal or a
/// `ReadOnlyOrUnaddressable(Writable)` outcome is not merely untested — it is
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RefusedWritability {
    /// Proven present but not writable.
    ReadOnly,
    /// Present but with no addressable storage cell.
    Unaddressable,
    /// Writability was never established. Uncertainty fails closed.
    NotProven,
}

impl RefusedWritability {
    /// The refusal a disposition names, or `None` when it is writable.
    ///
    /// Returning `None` for [`WritabilityDisposition::Writable`] is the point:
    /// there is no refusal to build from it.
    pub fn from_disposition(disposition: WritabilityDisposition) -> Option<Self> {
        match disposition {
            WritabilityDisposition::Writable => None,
            WritabilityDisposition::ReadOnly => Some(Self::ReadOnly),
            WritabilityDisposition::Unaddressable => Some(Self::Unaddressable),
            WritabilityDisposition::NotProven => Some(Self::NotProven),
        }
    }
}

/// Admitted writable operation subject cohort.
///
/// Exactly the three v1 cohorts. A deferred or incoherent location kind has no
/// cohort, which is what makes breadth expansion a contract change rather than
/// an accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationTargetCohort {
    /// Current-frame lexical scalar.
    CurrentFrameLexicalScalar,
    /// Current-frame array element.
    CurrentFrameArrayElement,
    /// Current-frame hash entry.
    CurrentFrameHashEntry,
}

impl MutationTargetCohort {
    /// The cohort a location kind admits, if any.
    fn from_kind(kind: MutationLocationKind) -> Option<Self> {
        match kind {
            MutationLocationKind::CurrentFrameLexicalScalar => {
                Some(Self::CurrentFrameLexicalScalar)
            }
            MutationLocationKind::CurrentFrameArrayElement => Some(Self::CurrentFrameArrayElement),
            MutationLocationKind::CurrentFrameHashEntry => Some(Self::CurrentFrameHashEntry),
            MutationLocationKind::PackageScalar | MutationLocationKind::NonCurrentFrameScalar => {
                None
            }
        }
    }
}

/// Why a candidate cannot be bound to an exact mutation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
pub enum MutationTargetBindingError {
    /// No session generation was supplied.
    #[error("mutation target candidate has no session generation")]
    MissingSessionGeneration,
    /// No suspension generation was supplied.
    #[error("mutation target candidate has no suspension generation")]
    MissingSuspensionGeneration,
    /// No value-authority generation was supplied.
    #[error("mutation target candidate has no value authority generation")]
    MissingValueAuthorityGeneration,
    /// No exact frame identity was supplied.
    #[error("mutation target candidate has no exact frame identity")]
    MissingFrameIdentity,
    /// No exact storage binding identity was supplied.
    ///
    /// This is the refusal that a value-node-only or display-name-only claim
    /// lands on: an observed value does not address storage.
    #[error("mutation target candidate has no exact storage binding identity")]
    MissingBindingIdentity,
    /// No location kind was claimed.
    #[error("mutation target candidate has no location kind")]
    MissingLocationKind,
    /// No member selector was claimed.
    #[error("mutation target candidate has no member selector")]
    MissingMember,
    /// The location kind is real but outside the v1 supported table.
    #[error("mutation location kind is not supported in v1: {0:?}")]
    UnsupportedLocationKind(MutationLocationKind),
    /// The member selector does not match the location kind.
    #[error("mutation member selector does not match the location kind")]
    MemberKindMismatch,
    /// A container member was claimed without a proven container referent.
    #[error("container member requires a proven referent identity")]
    MissingReferentForContainerMember,
    /// The location is not proven writable.
    ///
    /// Carries [`RefusedWritability`], so this refusal cannot claim the
    /// location was writable.
    #[error("mutation location is not writable: {0:?}")]
    NotWritable(RefusedWritability),
    /// The accompanying value observation belongs to another suspension.
    #[error("inspected value identity was observed under a different value authority")]
    StaleValueObservation,
}

/// Partial, possibly wrong, target claim as produced by acquisition.
///
/// Deliberately constructible while incomplete so binding failures are
/// observable and testable rather than unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutationTargetCandidate {
    /// Session generation the claim was observed under.
    pub session_generation: Option<u64>,
    /// Suspension generation the claim was observed under.
    pub suspension_generation: Option<u64>,
    /// Value-authority generation the operation expects.
    pub value_authority_generation: Option<u64>,
    /// Exact observed frame identity. Empty when unknown.
    pub frame_identity: String,
    /// Exact observed storage binding identity. Empty when unknown.
    pub binding_identity: String,
    /// Kind of storage claimed.
    pub kind: Option<MutationLocationKind>,
    /// Which cell inside the binding is claimed.
    pub member: Option<MutationMember>,
    /// Observed value identity accompanying the claim, when any.
    pub inspected_value: Option<InspectedValueIdentity>,
    /// Writability classification supplied by the acquisition path (#10765).
    pub writability: WritabilityDisposition,
    /// Backend/mode cell the target will be operated under.
    pub backend_mode: String,
}

impl MutationTargetCandidate {
    /// Bind this claim into a sealed [`MutationTarget`], or say exactly why not.
    ///
    /// Every refusal is a distinct variant, because "not writable", "not
    /// supported yet", and "you gave me a value instead of a location" are
    /// different facts that later leaves route differently.
    pub fn bind(&self) -> Result<MutationTarget, MutationTargetBindingError> {
        let session_generation =
            self.session_generation.ok_or(MutationTargetBindingError::MissingSessionGeneration)?;
        let suspension_generation = self
            .suspension_generation
            .ok_or(MutationTargetBindingError::MissingSuspensionGeneration)?;
        let value_authority_generation = self
            .value_authority_generation
            .ok_or(MutationTargetBindingError::MissingValueAuthorityGeneration)?;
        if self.frame_identity.is_empty() {
            return Err(MutationTargetBindingError::MissingFrameIdentity);
        }
        if self.binding_identity.is_empty() {
            return Err(MutationTargetBindingError::MissingBindingIdentity);
        }
        let kind = self.kind.ok_or(MutationTargetBindingError::MissingLocationKind)?;
        let member = self.member.clone().ok_or(MutationTargetBindingError::MissingMember)?;
        if !member.matches_kind(kind) {
            return Err(MutationTargetBindingError::MemberKindMismatch);
        }
        let cohort = MutationTargetCohort::from_kind(kind)
            .ok_or(MutationTargetBindingError::UnsupportedLocationKind(kind))?;

        let referent_identity = self.inspected_value.as_ref().and_then(|v| v.referent.clone());
        if member.is_container_member() && referent_identity.is_none() {
            return Err(MutationTargetBindingError::MissingReferentForContainerMember);
        }
        let observation_is_stale = self.inspected_value.as_ref().is_some_and(|observation| {
            observation.value_authority_generation != value_authority_generation
        });
        if observation_is_stale {
            return Err(MutationTargetBindingError::StaleValueObservation);
        }
        if let Some(refusal) = RefusedWritability::from_disposition(self.writability) {
            return Err(MutationTargetBindingError::NotWritable(refusal));
        }

        Ok(MutationTarget {
            location: MutationLocationProvenance {
                session_generation,
                suspension_generation,
                value_authority_generation,
                frame_identity: self.frame_identity.clone(),
                binding_identity: self.binding_identity.clone(),
                kind,
                member,
                referent_identity,
                profile_version: MUTATION_TARGET_PROFILE_VERSION,
            },
            cohort,
            backend_mode: self.backend_mode.clone(),
            profile_version: MUTATION_TARGET_PROFILE_VERSION,
        })
    }
}

/// An admitted writable mutation subject.
///
/// Sealed; produced only by [`MutationTargetCandidate::bind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationTarget {
    location: MutationLocationProvenance,
    cohort: MutationTargetCohort,
    backend_mode: String,
    profile_version: u32,
}

impl MutationTarget {
    /// Exact storage location this target writes.
    pub fn location(&self) -> &MutationLocationProvenance {
        &self.location
    }

    /// Admitted cohort.
    pub fn cohort(&self) -> MutationTargetCohort {
        self.cohort
    }

    /// Backend/mode cell this target is operated under.
    pub fn backend_mode(&self) -> &str {
        &self.backend_mode
    }

    /// Supported-target profile version.
    pub fn profile_version(&self) -> u32 {
        self.profile_version
    }

    /// Receipt-safe projection: identity and cohort, never key or value data.
    ///
    /// A hash key is debuggee data, so it is reduced to its byte length; the
    /// array index is structural and is retained.
    pub fn receipt_projection(&self) -> MutationTargetReceipt {
        let (member_kind, array_index, key_bytes) = match self.location.member() {
            MutationMember::WholeScalar => ("whole_scalar", None, None),
            MutationMember::ArrayIndex(index) => ("array_index", Some(*index), None),
            MutationMember::HashKey(key) => ("hash_key", None, Some(key.len())),
        };
        MutationTargetReceipt {
            cohort: self.cohort,
            member_kind,
            array_index,
            key_bytes,
            session_generation: self.location.session_generation(),
            suspension_generation: self.location.suspension_generation(),
            value_authority_generation: self.location.value_authority_generation(),
            profile_version: self.profile_version,
        }
    }
}

/// Redacted projection of a mutation target for receipts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationTargetReceipt {
    /// Admitted cohort.
    pub cohort: MutationTargetCohort,
    /// Structural member discriminant.
    pub member_kind: &'static str,
    /// Array index, when the member is an array element.
    pub array_index: Option<i64>,
    /// Byte length of the redacted hash key, when the member is a hash entry.
    pub key_bytes: Option<usize>,
    /// Session generation the target was bound under.
    pub session_generation: u64,
    /// Suspension generation the target was bound under.
    pub suspension_generation: u64,
    /// Value-authority generation the target was bound under.
    ///
    /// Without this, two targets bound under different observed values during
    /// one suspension would produce identical receipts, and a consumer could
    /// not tell which observation authorized an edit.
    pub value_authority_generation: u64,
    /// Supported-target profile version.
    pub profile_version: u32,
}
