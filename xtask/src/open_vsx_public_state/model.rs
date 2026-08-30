//! Types for the Open VSX public-state observation and receipt.
//!
//! The observation is deliberately narrow: it can express transport facts and
//! parsed identity facts and nothing else. No field is *shaped* to carry a
//! response body, a request header, or a credential.
//!
//! Field shape alone is not the whole boundary, though. The free-text reference
//! fields here — `Instrument` and `Expected::publication_refs` — are copied
//! verbatim into a durable, shareable receipt, so their *values* are validated
//! too (`classify::unsafe_reference`): bounded, control-character-free, and
//! either plain text or an `https` URL without userinfo. Property-name closure
//! proves nothing about what a producer puts inside a string.

use serde::{Deserialize, Serialize};

pub(super) const OBSERVATION_SCHEMA_VERSION: &str = "open_vsx_public_state_observation.v1";
pub(super) const RECEIPT_SCHEMA_VERSION: &str = "open_vsx_public_state.v1";
pub(super) const REGISTRY: &str = "open_vsx";

/// Owner recorded on every blocker this classifier raises.
pub(super) const OWNER: &str = "#9923";

// ---------------------------------------------------------------------------
// Observation (input)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub(crate) schema_version: String,
    pub(crate) observed_at: String,
    pub(crate) registry: String,
    pub(crate) identity: ObservedIdentity,
    pub(crate) instrument: Instrument,
    pub(crate) expected: Expected,
    pub(crate) cells: ObservedCells,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedIdentity {
    pub(crate) namespace: String,
    pub(crate) extension: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Instrument {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source_ref: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expected {
    pub(crate) versions: Vec<ExpectedVersion>,
    pub(crate) publication_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedVersion {
    pub(crate) version: String,
    pub(crate) vsix_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedCells {
    pub(crate) listing: ListingCell,
    pub(crate) search: SearchCell,
    pub(crate) namespace_metadata: NamespaceMetadataCell,
    pub(crate) extension_metadata: ExtensionMetadataCell,
    pub(crate) version_rows: VersionRowsCell,
    pub(crate) versioned_file: VersionedFileCell,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transport {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) outcome: TransportOutcome,
    pub(crate) status: Option<u16>,
    pub(crate) redirects: u32,
    pub(crate) response_bytes: Option<u64>,
    pub(crate) truncated: bool,
    pub(crate) error_kind: Option<ErrorKind>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransportOutcome {
    HttpResponse,
    TransportError,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    Timeout,
    Dns,
    Tls,
    Connect,
    BodyLimitExceeded,
    RedirectLimitExceeded,
    RateLimited,
    SchemaDrift,
    ParseError,
}

impl ErrorKind {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Dns => "dns",
            Self::Tls => "tls",
            Self::Connect => "connect",
            Self::BodyLimitExceeded => "body_limit_exceeded",
            Self::RedirectLimitExceeded => "redirect_limit_exceeded",
            Self::RateLimited => "rate_limited",
            Self::SchemaDrift => "schema_drift",
            Self::ParseError => "parse_error",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListingCell {
    pub(crate) transport: Transport,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchCell {
    pub(crate) transport: Transport,
    pub(crate) matched_identity: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamespaceMetadataCell {
    pub(crate) transport: Transport,
    pub(crate) namespace_present: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtensionMetadataCell {
    pub(crate) transport: Transport,
    pub(crate) identity_matches: Option<bool>,
    pub(crate) versions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionRowsCell {
    pub(crate) transport: Transport,
    pub(crate) versions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionedFileCell {
    pub(crate) transport: Transport,
    pub(crate) version: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) byte_length: Option<u64>,
}

// ---------------------------------------------------------------------------
// Receipt (output)
// ---------------------------------------------------------------------------

/// A state this classifier can actually emit.
///
/// Deliberately one variant smaller than the published contract: the contract
/// also defines `available_exact` for consumers holding a resolved candidate
/// authority (#9138), and this classifier has none, so it cannot produce that
/// state. Modelling it here would let the type claim a capability the code does
/// not have — `schema_tests` pins the exact relationship between the two sets.
///
/// `ProviderNotProven` absorbs every transport, budget, rate-limit, schema-drift
/// and contradictory-evidence outcome. Nothing but three independent affirmative
/// `404` answers can reach `ExtensionMissing`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublicState {
    AvailableIdentityNotProven,
    ListingMissingVersionRetrievable,
    ExtensionMissing,
    NamespaceOrPublisherProblem,
    ProviderNotProven,
    Invalid,
}

impl PublicState {
    /// Every state this classifier can emit, so one test can prove the code and
    /// the published contract have not drifted apart.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 6] = [
        Self::AvailableIdentityNotProven,
        Self::ListingMissingVersionRetrievable,
        Self::ExtensionMissing,
        Self::NamespaceOrPublisherProblem,
        Self::ProviderNotProven,
        Self::Invalid,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::AvailableIdentityNotProven => "available_identity_not_proven",
            Self::ListingMissingVersionRetrievable => "listing_missing_version_retrievable",
            Self::ExtensionMissing => "extension_missing",
            Self::NamespaceOrPublisherProblem => "namespace_or_publisher_problem",
            Self::ProviderNotProven => "provider_not_proven",
            Self::Invalid => "invalid",
        }
    }
}

/// What one surface affirmatively established.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CellObservation {
    /// An affirmative 2xx.
    Present,
    /// An affirmative 404. The only answer that can contribute to absence.
    ProvenAbsent,
    /// Transport failure, budget overrun, rate limit, schema drift, or any
    /// other status. Never absence.
    ProviderFailed,
    /// The probe did not run this cell. An instrument gap, never absence.
    NotAttempted,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReceiptIdentity {
    pub(crate) namespace: String,
    pub(crate) extension: String,
    pub(crate) extension_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CellResult {
    pub(crate) cell: &'static str,
    pub(crate) url: String,
    pub(crate) method: &'static str,
    pub(crate) observation: CellObservation,
    pub(crate) status: Option<u16>,
    pub(crate) redirects: u32,
    pub(crate) response_bytes: Option<u64>,
    pub(crate) truncated: bool,
    pub(crate) error_kind: Option<&'static str>,
    pub(crate) identity_match: Option<bool>,
    /// Version rows this surface published, where it publishes any. Kept per
    /// surface so the metadata record and the versions endpoint stay separate
    /// observations rather than one merged list.
    pub(crate) versions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PublicBytes {
    pub(crate) version: String,
    pub(crate) sha256: String,
    pub(crate) byte_length: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Blocker {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) owner: String,
}

impl Blocker {
    pub(super) fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_owned(), message: message.into(), owner: OWNER.to_owned() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Receipt {
    pub(crate) schema_version: &'static str,
    pub(crate) observed_at: String,
    pub(crate) registry: &'static str,
    pub(crate) identity: ReceiptIdentity,
    pub(crate) instrument: Instrument,
    pub(crate) instrument_complete: bool,
    pub(crate) probe_plan_digest: Option<String>,
    pub(crate) subject_version: Option<String>,
    pub(crate) cells: Vec<CellResult>,
    pub(crate) public_bytes: Option<PublicBytes>,
    pub(crate) expected: Expected,
    pub(crate) limitations: Vec<String>,
    pub(crate) blockers: Vec<Blocker>,
    pub(crate) state: PublicState,
}
