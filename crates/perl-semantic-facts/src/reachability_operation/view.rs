//! Neutral bounded-view metadata for explanations and projections.

use super::{
    ReachabilityContractError, ReachabilityOperationOutcome, ReachabilitySubjectIdentity,
    ReachabilitySubjectIdentityKind,
};
use serde::{Deserialize, Serialize};

/// Stable identity of one bounded-view profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachableViewProfileId(String);

impl<'de> Deserialize<'de> for ReachableViewProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ReachableViewProfileId::new(value).map_err(serde::de::Error::custom)
    }
}

impl ReachableViewProfileId {
    /// Construct a view-profile identifier, rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    /// The opaque view-profile identifier value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Proof that one complete exact result underlies a bounded view.
///
/// The only producer of this token is
/// [`ReachabilityOperationOutcome::bounded_view_source`], which returns
/// `None` unless the outcome [`may claim
/// exact`](ReachabilityOperationOutcome::may_claim_exact). A truncated or
/// incomplete semantic computation therefore cannot mint the token, and a
/// bounded view cannot claim a complete underlying result it does not have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityCompleteResultRef {
    result_identity: ReachabilitySubjectIdentity,
    currentness_authority: ReachabilitySubjectIdentity,
}

impl ReachabilityCompleteResultRef {
    /// The exact output identity of the complete underlying result.
    #[must_use]
    pub const fn result_identity(&self) -> &ReachabilitySubjectIdentity {
        &self.result_identity
    }

    /// The accepted authority the underlying result is current against.
    #[must_use]
    pub const fn currentness_authority(&self) -> &ReachabilitySubjectIdentity {
        &self.currentness_authority
    }
}

/// Neutral bounded-view metadata reusable by explanation, diagnostic, and
/// transport projections (#10935/#10947/#10953).
///
/// Truncation of presentation does not change complete underlying
/// semantics; truncation during semantic computation does. The two are
/// impossible to conflate here: a view is constructible only over a
/// [`ReachabilityCompleteResultRef`] minted from an exact-completed
/// outcome, while semantic-stage truncation caps the semantic outcome at
/// `Partial` and can never mint that token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachabilityBoundedView {
    underlying: ReachabilityCompleteResultRef,
    view_profile: ReachableViewProfileId,
    items_returned: u64,
    bytes_returned: u64,
    known_total: Option<u64>,
    omitted_count: Option<u64>,
    truncated: bool,
    truncation_reason: Option<String>,
}

impl<'de> Deserialize<'de> for ReachabilityBoundedView {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            underlying: ReachabilityCompleteResultRef,
            view_profile: ReachableViewProfileId,
            items_returned: u64,
            bytes_returned: u64,
            known_total: Option<u64>,
            omitted_count: Option<u64>,
            truncated: bool,
            truncation_reason: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        ReachabilityBoundedView::new(
            raw.underlying,
            raw.view_profile,
            raw.items_returned,
            raw.bytes_returned,
            raw.known_total,
            raw.omitted_count,
            raw.truncated,
            raw.truncation_reason,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl ReachabilityBoundedView {
    /// Construct a bounded view over one proven-complete underlying result.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::IncoherentBoundedView`] when
    /// the view claims truncation without a reason, omits the omitted-count
    /// even though the total is known and exceeds the returned items, claims
    /// an omitted count of zero while truncated (truncation always omits at
    /// least one item; a bytes-only truncation reports an unknown omitted
    /// count, not zero), or reports a known total smaller than the items
    /// returned.
    pub fn new(
        underlying: ReachabilityCompleteResultRef,
        view_profile: ReachableViewProfileId,
        items_returned: u64,
        bytes_returned: u64,
        known_total: Option<u64>,
        omitted_count: Option<u64>,
        truncated: bool,
        truncation_reason: Option<String>,
    ) -> Result<Self, ReachabilityContractError> {
        if truncated && truncation_reason.as_deref().is_none_or(str::is_empty) {
            return Err(ReachabilityContractError::IncoherentBoundedView);
        }
        if let Some(total) = known_total
            && total < items_returned
        {
            return Err(ReachabilityContractError::IncoherentBoundedView);
        }
        if truncated && omitted_count == Some(0) {
            return Err(ReachabilityContractError::IncoherentBoundedView);
        }
        if let Some(total) = known_total
            && total > items_returned
        {
            let omitted = omitted_count.ok_or(ReachabilityContractError::IncoherentBoundedView)?;
            if omitted != total - items_returned {
                return Err(ReachabilityContractError::IncoherentBoundedView);
            }
            if !truncated {
                return Err(ReachabilityContractError::IncoherentBoundedView);
            }
        }
        Ok(Self {
            underlying,
            view_profile,
            items_returned,
            bytes_returned,
            known_total,
            omitted_count,
            truncated,
            truncation_reason,
        })
    }

    /// The proven-complete underlying result.
    #[must_use]
    pub const fn underlying(&self) -> &ReachabilityCompleteResultRef {
        &self.underlying
    }

    /// The view/projection profile.
    #[must_use]
    pub const fn view_profile(&self) -> &ReachableViewProfileId {
        &self.view_profile
    }

    /// Items/members/paths returned by this view.
    #[must_use]
    pub const fn items_returned(&self) -> u64 {
        self.items_returned
    }

    /// Serialized bytes returned by this view.
    #[must_use]
    pub const fn bytes_returned(&self) -> u64 {
        self.bytes_returned
    }

    /// The known total, or `None` when explicitly unknown.
    #[must_use]
    pub const fn known_total(&self) -> Option<u64> {
        self.known_total
    }

    /// The omitted count, where knowable.
    #[must_use]
    pub const fn omitted_count(&self) -> Option<u64> {
        self.omitted_count
    }

    /// Whether this view truncated the presentation.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// The reason retained when truncated.
    #[must_use]
    pub fn truncation_reason(&self) -> Option<&str> {
        self.truncation_reason.as_deref()
    }
}

impl<T> ReachabilityOperationOutcome<T> {
    /// Mint the complete-result token a bounded view requires.
    ///
    /// Returns `None` unless this outcome is `Completed` with exact truth —
    /// a complete value or a proven legitimate empty — over a clean receipt:
    /// a truncated semantic computation, a partial value, a terminal
    /// outcome, or missing instrument evidence cannot underlie a complete
    /// bounded view.
    #[must_use]
    pub fn bounded_view_source(
        &self,
        result_identity: ReachabilitySubjectIdentity,
        currentness_authority: ReachabilitySubjectIdentity,
    ) -> Option<ReachabilityCompleteResultRef> {
        if !self.may_claim_exact() {
            return None;
        }
        if result_identity.kind() != ReachabilitySubjectIdentityKind::StageOutput {
            return None;
        }
        Some(ReachabilityCompleteResultRef { result_identity, currentness_authority })
    }
}
