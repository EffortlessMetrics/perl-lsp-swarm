use std::io;

/// Typed storage outcomes for [`crate::SnapshotStore`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotStoreError {
    /// No object or generation exists for the requested key.
    Miss,
    /// A compare-and-swap or publication guard rejected the operation.
    Incompatible {
        /// Human-readable detail safe for receipts (no host paths).
        detail: String,
    },
    /// Payload or framing failed integrity checks.
    Corrupt {
        /// Human-readable detail safe for receipts (no host paths).
        detail: String,
    },
    /// The store root or an object is not writable.
    PermissionDenied {
        /// Human-readable detail safe for receipts (no host paths).
        detail: String,
    },
    /// A bounded resource limit was exceeded.
    LimitExceeded {
        /// Human-readable detail safe for receipts (no host paths).
        detail: String,
    },
    /// Another writer holds the publication lock.
    Busy,
    /// Underlying I/O failure.
    Io {
        /// Human-readable detail safe for receipts (no host paths).
        detail: String,
    },
}

impl SnapshotStoreError {
    pub(crate) fn io(error: io::Error) -> Self {
        Self::Io { detail: error.to_string() }
    }

    pub(crate) fn corrupt(detail: impl Into<String>) -> Self {
        Self::Corrupt { detail: detail.into() }
    }

    pub(crate) fn incompatible(detail: impl Into<String>) -> Self {
        Self::Incompatible { detail: detail.into() }
    }

    pub(crate) fn permission(detail: impl Into<String>) -> Self {
        Self::PermissionDenied { detail: detail.into() }
    }

    pub(crate) fn limit(detail: impl Into<String>) -> Self {
        Self::LimitExceeded { detail: detail.into() }
    }
}

impl std::fmt::Display for SnapshotStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Miss => f.write_str("snapshot store miss"),
            Self::Incompatible { detail } => write!(f, "snapshot store incompatible: {detail}"),
            Self::Corrupt { detail } => write!(f, "snapshot store corrupt: {detail}"),
            Self::PermissionDenied { detail } => {
                write!(f, "snapshot store permission denied: {detail}")
            }
            Self::LimitExceeded { detail } => write!(f, "snapshot store limit exceeded: {detail}"),
            Self::Busy => f.write_str("snapshot store busy"),
            Self::Io { detail } => write!(f, "snapshot store I/O error: {detail}"),
        }
    }
}

impl std::error::Error for SnapshotStoreError {}
