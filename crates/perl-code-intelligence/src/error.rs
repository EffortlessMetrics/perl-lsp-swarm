//! Fail-closed session and document authority errors.

use serde::{Deserialize, Serialize};

use crate::document_source::DocumentSourceKind;

/// Document authority construction or admission failed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentAuthorityError {
    /// Supplied kind requires a content revision.
    MissingContentRevision,
    /// Revision logical source does not match authority source.
    RevisionSourceMismatch,
    /// Document generation label was empty.
    MalformedDocumentGeneration,
    /// Observed revision disagrees with caller-supplied authority.
    SuppliedRevisionMismatch,
    /// Observed generation disagrees with declared generation.
    StaleDocumentGeneration,
    /// Disk observation cannot answer a supplied-version request.
    SilentDiskFallbackRefused,
    /// Authority kind did not match the admission path.
    KindMismatch {
        /// Expected kind for the admission path.
        expected: DocumentSourceKind,
        /// Kind carried by the authority value.
        observed: DocumentSourceKind,
    },
}

/// Session construction failed before a request may run.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionConstructionError {
    /// Workspace generation label was empty.
    MalformedWorkspaceGeneration,
    /// Configuration generation label was empty.
    MalformedConfigurationGeneration,
    /// Document authority could not be admitted.
    DocumentAuthority(DocumentAuthorityError),
}

impl From<DocumentAuthorityError> for SessionConstructionError {
    fn from(error: DocumentAuthorityError) -> Self {
        Self::DocumentAuthority(error)
    }
}
