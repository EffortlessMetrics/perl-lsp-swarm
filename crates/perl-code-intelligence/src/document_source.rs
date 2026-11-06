//! Document input authority for code-intelligence requests (#7218).

use perl_source_identity::{ContentDigest, ContentRevision, LogicalSourceId, SourceGeneration};
use serde::{Deserialize, Serialize};

use crate::error::DocumentAuthorityError;

/// Which source of document text is authoritative for a request.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DocumentSourceKind {
    /// Open or retained editor/session buffer owned by the workspace model.
    OpenRetained,
    /// Caller supplied exact bytes and an explicit generation or revision.
    CallerSupplied,
    /// Observed on-disk bytes with digest metadata; never impersonates an open buffer.
    DiskObservation,
}

/// Declared authority for one document-scoped request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSourceAuthority {
    /// Logical source identity (revision-independent).
    pub logical_source: LogicalSourceId,
    /// Which source kind answers this request.
    pub kind: DocumentSourceKind,
    /// Exact revision when the kind requires byte identity.
    pub content_revision: Option<ContentRevision>,
    /// Document generation observed for this authority.
    pub document_generation: SourceGeneration,
}

impl DocumentSourceAuthority {
    /// Construct a caller-supplied authority from explicit revision and generation.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentAuthorityError::MissingContentRevision`] when the kind
    /// requires a revision but none was supplied.
    pub fn caller_supplied(
        logical_source: LogicalSourceId,
        revision: ContentRevision,
        document_generation: SourceGeneration,
    ) -> Result<Self, DocumentAuthorityError> {
        let authority = Self {
            logical_source,
            kind: DocumentSourceKind::CallerSupplied,
            content_revision: Some(revision),
            document_generation,
        };
        authority.validate()?;
        Ok(authority)
    }

    /// Construct an open/retained buffer authority bound to a known generation.
    pub fn open_retained(
        logical_source: LogicalSourceId,
        revision: ContentRevision,
        document_generation: SourceGeneration,
    ) -> Result<Self, DocumentAuthorityError> {
        let authority = Self {
            logical_source,
            kind: DocumentSourceKind::OpenRetained,
            content_revision: Some(revision),
            document_generation,
        };
        authority.validate()?;
        Ok(authority)
    }

    /// Construct a disk observation authority from digest-only metadata.
    pub fn disk_observation(
        logical_source: LogicalSourceId,
        revision: ContentRevision,
        document_generation: SourceGeneration,
    ) -> Result<Self, DocumentAuthorityError> {
        let authority = Self {
            logical_source,
            kind: DocumentSourceKind::DiskObservation,
            content_revision: Some(revision),
            document_generation,
        };
        authority.validate()?;
        Ok(authority)
    }

    /// Fail closed when required fields for the declared kind are missing or malformed.
    pub fn validate(&self) -> Result<(), DocumentAuthorityError> {
        if !generation_is_well_formed(&self.document_generation) {
            return Err(DocumentAuthorityError::MalformedDocumentGeneration);
        }
        match self.kind {
            DocumentSourceKind::OpenRetained
            | DocumentSourceKind::CallerSupplied
            | DocumentSourceKind::DiskObservation => {
                let Some(revision) = &self.content_revision else {
                    return Err(DocumentAuthorityError::MissingContentRevision);
                };
                if revision.logical_source_id != self.logical_source {
                    return Err(DocumentAuthorityError::RevisionSourceMismatch);
                }
            }
        }
        Ok(())
    }

    /// Whether answering from a different revision would violate this authority.
    #[must_use]
    pub fn accepts_revision(&self, candidate: &ContentRevision) -> bool {
        self.content_revision.as_ref().is_some_and(|expected| expected == candidate)
    }

    /// Whether answering from a different digest would violate this authority.
    #[must_use]
    pub fn accepts_digest(&self, candidate: &ContentDigest) -> bool {
        self.content_revision.as_ref().is_some_and(|expected| &expected.content_digest == candidate)
    }
}

/// Resolved authority after comparing declared and observed facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDocumentAuthority {
    /// The admitted authority.
    pub authority: DocumentSourceAuthority,
    /// Freshness generation recorded on resolution.
    pub observed_generation: SourceGeneration,
}

impl ResolvedDocumentAuthority {
    /// Admit a caller-supplied authority only when observed revision and generation match.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentAuthorityError::SuppliedRevisionMismatch`] or
    /// [`DocumentAuthorityError::StaleDocumentGeneration`] when observation disagrees.
    pub fn admit_caller_supplied(
        authority: DocumentSourceAuthority,
        observed_revision: &ContentRevision,
        observed_generation: SourceGeneration,
    ) -> Result<Self, DocumentAuthorityError> {
        if authority.kind != DocumentSourceKind::CallerSupplied {
            return Err(DocumentAuthorityError::KindMismatch {
                expected: DocumentSourceKind::CallerSupplied,
                observed: authority.kind,
            });
        }
        authority.validate()?;
        if !authority.accepts_revision(observed_revision) {
            return Err(DocumentAuthorityError::SuppliedRevisionMismatch);
        }
        if !generation_matches(&authority.document_generation, &observed_generation) {
            return Err(DocumentAuthorityError::StaleDocumentGeneration);
        }
        Ok(Self { authority, observed_generation })
    }

    /// Refuse to treat disk bytes as answering a caller-supplied request.
    pub fn refuse_disk_for_supplied(
        authority: &DocumentSourceAuthority,
        disk_revision: &ContentRevision,
    ) -> Result<(), DocumentAuthorityError> {
        if authority.kind != DocumentSourceKind::CallerSupplied {
            return Ok(());
        }
        if authority.accepts_revision(disk_revision) {
            return Ok(());
        }
        Err(DocumentAuthorityError::SilentDiskFallbackRefused)
    }
}

fn generation_is_well_formed(generation: &SourceGeneration) -> bool {
    !matches!(generation, SourceGeneration::Known(label) if label.trim().is_empty())
}

fn generation_matches(expected: &SourceGeneration, observed: &SourceGeneration) -> bool {
    expected == observed
}

#[cfg(test)]
mod tests {
    use perl_source_identity::{
        ContentDigest, ContentRevision, LogicalPathError, LogicalSourceId, ProjectId,
        RootRelativeLogicalPath, SourceGeneration, WorkspaceRootId,
    };
    use perl_test_must::must_with;

    use super::*;

    fn fixture_source() -> Result<(LogicalSourceId, ContentRevision), LogicalPathError> {
        let project = ProjectId::from_canonical_name("https://example.com/demo");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "root-a");
        let path = RootRelativeLogicalPath::parse("lib/Demo.pm")?;
        let logical = LogicalSourceId::from_root_and_logical_path(&root, &path);
        let digest_a = ContentDigest::of_bytes(b"package Demo; 1;\n");
        Ok((logical.clone(), ContentRevision::new(logical, digest_a)))
    }

    #[test]
    fn supplied_snapshot_differs_from_disk_is_not_admitted() -> Result<(), LogicalPathError> {
        let (logical, open_revision) = fixture_source()?;
        let disk_digest = ContentDigest::of_bytes(b"package Demo; 2;\n");
        let disk_revision = ContentRevision::new(logical.clone(), disk_digest);
        let authority = must_with(
            DocumentSourceAuthority::caller_supplied(
                logical,
                open_revision,
                SourceGeneration::known("3"),
            ),
            "valid supplied authority",
        );

        assert_eq!(
            DocumentAuthorityError::SilentDiskFallbackRefused,
            ResolvedDocumentAuthority::refuse_disk_for_supplied(&authority, &disk_revision)
                .unwrap_err()
        );
        Ok(())
    }

    #[test]
    fn stale_supplied_generation_is_explicit_error() -> Result<(), LogicalPathError> {
        let (logical, revision) = fixture_source()?;
        let authority = must_with(
            DocumentSourceAuthority::caller_supplied(
                logical,
                revision.clone(),
                SourceGeneration::known("2"),
            ),
            "valid supplied authority",
        );

        let err = ResolvedDocumentAuthority::admit_caller_supplied(
            authority,
            &revision,
            SourceGeneration::known("1"),
        )
        .unwrap_err();
        assert_eq!(DocumentAuthorityError::StaleDocumentGeneration, err);
        Ok(())
    }
}
