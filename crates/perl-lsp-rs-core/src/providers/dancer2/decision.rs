//! Provider decision contract for the Dancer2 slice (#8928).
//!
//! Every promoted Dancer2 decision preserves the issue's decision fields:
//! canonical entity/fact identity, framework/adapter/version, source and
//! workspace generation, confidence/provenance, readiness/currentness,
//! fallback/refusal reason, and the legacy/canonical selection. The three
//! outcomes select exactly one authority per request.

/// One selected authority for a promoted Dancer2 provider decision.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2Decision<T> {
    /// Canonical facts answered this request. The payload carries the
    /// canonical result and its provenance.
    PromoteCanonical {
        /// Canonical result payload.
        payload: T,
        /// Framework that produced the facts (`Dancer2`).
        framework: &'static str,
        /// Observed framework version behind the activation.
        framework_version: String,
        /// Source generation of the facts (content-derived).
        source_generation: String,
        /// True when every contributing fact was exact; degraded facts carry
        /// their own boundary links.
        exact: bool,
    },
    /// The request class is not admitted by #8928; the existing generic
    /// provider path owns it unchanged (recorded boundary).
    FallbackExisting {
        /// Bounded reason the request class stays on the existing path.
        reason: String,
    },
    /// A dynamic, unsupported, ambiguous, or stale boundary was met. The
    /// cell returns no framework answer and this typed refusal instead of
    /// merging canonical and legacy output.
    RefuseTyped {
        /// Bounded machine-readable refusal reason.
        reason: &'static str,
        /// Human explanation of the boundary.
        detail: String,
    },
}

impl<T> Dancer2Decision<T> {
    /// Whether the decision promoted canonical output.
    #[must_use]
    pub fn is_promoted(&self) -> bool {
        matches!(self, Self::PromoteCanonical { .. })
    }

    /// Whether the decision is a typed refusal.
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::RefuseTyped { .. })
    }

    /// The promoted payload, when this decision promoted canonical output.
    #[must_use]
    pub fn promoted(self) -> Option<T> {
        match self {
            Self::PromoteCanonical { payload, .. } => Some(payload),
            _ => None,
        }
    }
}
