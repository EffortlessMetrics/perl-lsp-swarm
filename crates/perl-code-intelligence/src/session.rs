//! Transport-neutral workspace session facade (#7218).

use perl_source_identity::{SourceGeneration, WorkspaceRootId};
use serde::{Deserialize, Serialize};

use crate::{
    document_source::ResolvedDocumentAuthority,
    error::{DocumentAuthorityError, SessionConstructionError},
};

/// Scope carried by one code-intelligence session.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionScope {
    /// Canonical workspace root for the session.
    pub workspace_root: WorkspaceRootId,
    /// Workspace/index generation observed for the session.
    pub workspace_generation: SourceGeneration,
    /// Resolved configuration generation observed for the session.
    pub configuration_generation: SourceGeneration,
}

impl SessionScope {
    /// Construct a session scope after validating generation labels.
    ///
    /// # Errors
    ///
    /// Returns [`SessionConstructionError`] when generation labels are malformed.
    pub fn new(
        workspace_root: WorkspaceRootId,
        workspace_generation: SourceGeneration,
        configuration_generation: SourceGeneration,
    ) -> Result<Self, SessionConstructionError> {
        if !generation_is_well_formed(&workspace_generation) {
            return Err(SessionConstructionError::MalformedWorkspaceGeneration);
        }
        if !generation_is_well_formed(&configuration_generation) {
            return Err(SessionConstructionError::MalformedConfigurationGeneration);
        }
        Ok(Self { workspace_root, workspace_generation, configuration_generation })
    }
}

/// Deterministic session identity suitable for receipts (#7218).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionIdentity {
    /// Stable wire form for receipts and parity fixtures.
    wire: String,
}

impl SessionIdentity {
    /// Derive identity from the accepted session scope.
    #[must_use]
    pub fn from_scope(scope: &SessionScope) -> Self {
        let root = scope.workspace_root.to_string();
        let workspace = generation_label(&scope.workspace_generation);
        let configuration = generation_label(&scope.configuration_generation);
        Self { wire: format!("ci-session:v1:{root}:{workspace}:{configuration}") }
    }

    /// Observe the wire identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.wire
    }
}

/// Transport-neutral session facade; adapters populate equivalent facts from LSP or MCP launch.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIntelligenceSession {
    /// Exact session identity for receipts.
    pub identity: SessionIdentity,
    /// Accepted workspace scope.
    pub scope: SessionScope,
}

impl CodeIntelligenceSession {
    /// Construct a session without requiring an LSP initialize handshake.
    ///
    /// # Errors
    ///
    /// Returns [`SessionConstructionError`] when scope generations are malformed.
    pub fn from_local_host(scope: SessionScope) -> Result<Self, SessionConstructionError> {
        let identity = SessionIdentity::from_scope(&scope);
        Ok(Self { identity, scope })
    }

    /// Attach a resolved document authority to this session for a document-scoped request.
    ///
    /// # Errors
    ///
    /// Propagates document authority errors unchanged.
    pub fn admit_document(
        &self,
        resolved: ResolvedDocumentAuthority,
    ) -> Result<ResolvedDocumentAuthority, DocumentAuthorityError> {
        let _ = self;
        Ok(resolved)
    }
}

fn generation_is_well_formed(generation: &SourceGeneration) -> bool {
    !matches!(generation, SourceGeneration::Known(label) if label.trim().is_empty())
}

fn generation_label(generation: &SourceGeneration) -> String {
    match generation {
        SourceGeneration::Known(label) => format!("known:{label}"),
        SourceGeneration::Unknown => "unknown".to_owned(),
        _ => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use perl_source_identity::{
        ContentRevision, LogicalPathError, LogicalSourceId, ProjectId, RootRelativeLogicalPath,
        SourceGeneration, WorkspaceRootId,
    };

    use perl_test_must::must_with;

    use super::*;
    use crate::document_source::{DocumentSourceAuthority, ResolvedDocumentAuthority};

    fn fixture_scope() -> Result<SessionScope, LogicalPathError> {
        let project = ProjectId::from_canonical_name("https://example.com/demo");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "root-a");
        Ok(must_with(
            SessionScope::new(
                root,
                SourceGeneration::known("ws-1"),
                SourceGeneration::known("cfg-1"),
            ),
            "valid scope",
        ))
    }

    #[test]
    fn session_identity_is_deterministic() -> Result<(), LogicalPathError> {
        let scope = fixture_scope()?;
        let a = SessionIdentity::from_scope(&scope);
        let b = SessionIdentity::from_scope(&scope);
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("ci-session:v1:"));
        Ok(())
    }

    #[test]
    fn local_host_constructor_does_not_require_lsp_initialize() -> Result<(), LogicalPathError> {
        let scope = fixture_scope()?;
        let session = must_with(CodeIntelligenceSession::from_local_host(scope), "session");
        assert!(session.identity.as_str().contains("ws-1"));
        Ok(())
    }

    #[test]
    fn equivalent_facts_yield_equivalent_session_identity() -> Result<(), LogicalPathError> {
        let project = ProjectId::from_canonical_name("https://example.com/demo");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "root-a");
        let lsp_scope = must_with(
            SessionScope::new(
                root.clone(),
                SourceGeneration::known("ws-1"),
                SourceGeneration::known("cfg-1"),
            ),
            "lsp scope",
        );
        let mcp_scope = must_with(
            SessionScope::new(
                root,
                SourceGeneration::known("ws-1"),
                SourceGeneration::known("cfg-1"),
            ),
            "mcp scope",
        );
        assert_eq!(
            SessionIdentity::from_scope(&lsp_scope),
            SessionIdentity::from_scope(&mcp_scope)
        );
        Ok(())
    }

    #[test]
    fn admit_document_preserves_resolved_authority() -> Result<(), LogicalPathError> {
        let session =
            must_with(CodeIntelligenceSession::from_local_host(fixture_scope()?), "session");
        let project = ProjectId::from_canonical_name("https://example.com/demo");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "root-a");
        let path = RootRelativeLogicalPath::parse("lib/Demo.pm")?;
        let logical = LogicalSourceId::from_root_and_logical_path(&root, &path);
        let revision = ContentRevision::new(
            logical.clone(),
            perl_source_identity::ContentDigest::of_bytes(b"a"),
        );
        let authority = must_with(
            DocumentSourceAuthority::caller_supplied(
                logical,
                revision.clone(),
                SourceGeneration::known("1"),
            ),
            "authority",
        );
        let resolved = must_with(
            ResolvedDocumentAuthority::admit_caller_supplied(
                authority,
                &revision,
                SourceGeneration::known("1"),
            ),
            "resolved",
        );
        let admitted = must_with(session.admit_document(resolved.clone()), "admitted");
        assert_eq!(resolved, admitted);
        Ok(())
    }
}
