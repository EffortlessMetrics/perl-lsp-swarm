//! Consumer lookup: accepted values stay distinct from snapshot/not-proven.

/// How a later family consumer may use one limit field.
///
/// `ParsedNoConsumer` and `NotProven` still carry the current snapshot so the
/// denominator can name the value without promoting it to behavior-backed
/// authority. A migrated consumer must match [`LimitLookup::Accepted`] or
/// [`LimitLookup::FixedProduct`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitLookup<T> {
    /// Generation-bound accepted value for a live first-effect consumer.
    Accepted(T),
    /// Explicit product policy, not a configurable field.
    FixedProduct(T),
    /// Parsed or stored on `LspLimits` with no current production reader.
    ParsedNoConsumer { snapshot: T },
    /// Owned by another domain; this view classifies rather than absorbs it.
    Transferred { snapshot: T, owner: &'static str },
    /// Instrumentation or prerequisite is missing. Absence is not zero.
    NotProven { snapshot: Option<T>, reason: &'static str },
}

impl<T> LimitLookup<T> {
    pub(crate) fn accepted(self) -> Option<T> {
        match self {
            Self::Accepted(value) | Self::FixedProduct(value) => Some(value),
            Self::ParsedNoConsumer { .. } | Self::Transferred { .. } | Self::NotProven { .. } => {
                None
            }
        }
    }

    pub(crate) fn is_behavior_backed(&self) -> bool {
        matches!(self, Self::Accepted(_) | Self::FixedProduct(_))
    }
}
