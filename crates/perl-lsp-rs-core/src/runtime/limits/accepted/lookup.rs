//! Consumer lookup: accepted values stay distinct from snapshot/not-proven.

/// How a later family consumer may use one limit field.
///
/// Snapshot-bearing variants still carry the current magnitude so the
/// denominator can name the value without promoting it to behavior-backed
/// authority. A migrated consumer must match [`LimitLookup::Accepted`] or
/// [`LimitLookup::FixedProduct`]. [`LimitLookup::FixedInternal`] is a stored
/// internal with no current first-effect reader; it is not product policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitLookup<T> {
    /// Generation-bound accepted value for a live first-effect consumer.
    Accepted(T),
    /// Explicit product policy with a current first-effect consumer.
    FixedProduct(T),
    /// Stored internal with no current production reader.
    FixedInternal { snapshot: T },
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
            Self::FixedInternal { .. }
            | Self::ParsedNoConsumer { .. }
            | Self::Transferred { .. }
            | Self::NotProven { .. } => None,
        }
    }

    pub(crate) fn is_behavior_backed(&self) -> bool {
        matches!(self, Self::Accepted(_) | Self::FixedProduct(_))
    }

    pub(crate) fn behavior_ne(self, other: Self) -> bool
    where
        T: PartialEq,
    {
        self.accepted() != other.accepted()
    }

    pub(crate) fn snapshot(self) -> Option<T> {
        match self {
            Self::Accepted(value)
            | Self::FixedProduct(value)
            | Self::FixedInternal { snapshot: value }
            | Self::ParsedNoConsumer { snapshot: value }
            | Self::Transferred { snapshot: value, .. } => Some(value),
            Self::NotProven { snapshot, .. } => snapshot,
        }
    }
}
