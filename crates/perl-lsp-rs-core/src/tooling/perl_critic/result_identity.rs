//! Deterministic identity for critic-bearing pull-diagnostic results.
//!
//! Every load-bearing input is a canonical typed identity from
//! `perl-source-identity` or this module's closed vocabulary: logical document
//! and folder/configuration authority IDs are host-path-free, content and
//! policy digests are collision-resistant SHA-256, and generation counters are
//! always scoped by the authority that issued them.

use std::collections::BTreeSet;

use perl_source_identity::{ContentDigest, LogicalSourceId, WorkspaceRootId};

use crate::config::CriticEngine;

use super::NativeCriticProfile;

/// Schema version for [`DiagnosticResultIdentity`] composition.
pub const DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION: u16 = 2;

/// Source snapshot inputs that affect diagnostic output.
///
/// `document` is the logical source identity within its workspace root, so two
/// same-content files under different folder authorities can never share a
/// result identity. `content_digest` is the exact content revision.
/// `document_generation` is a live-session cursor scoped by `document`; it is
/// never read as a globally unique value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSourceIdentity {
    document: LogicalSourceId,
    content_digest: ContentDigest,
    document_generation: Option<u64>,
}

impl DiagnosticSourceIdentity {
    /// Construct source identity from the logical document identity, its exact
    /// content digest, and an optional live document generation.
    #[must_use]
    pub fn new(
        document: LogicalSourceId,
        content_digest: ContentDigest,
        document_generation: Option<u64>,
    ) -> Self {
        Self { document, content_digest, document_generation }
    }
}

/// Accepted critic policy inputs for one document/folder configuration.
///
/// `configuration_authority` binds the owning folder/configuration authority;
/// `configuration_generation` is scoped by it. When `engine` is
/// [`CriticEngine::Legacy`], `legacy_policy_digest` must carry the digest of
/// the effective legacy profile/theme/tool policy, because the legacy engine
/// resolves behavior-bearing inputs that the canonical generations do not
/// cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticPolicyIdentity {
    configuration_authority: WorkspaceRootId,
    configuration_generation: u64,
    engine: CriticEngine,
    profile: NativeCriticProfile,
    severity: u8,
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
    legacy_policy_digest: Option<ContentDigest>,
}

/// Error returned when a policy identity contradicts its engine's requirements.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CriticPolicyIdentityError {
    /// A legacy engine identity without the effective legacy policy digest.
    #[error("legacy engine identity requires the effective legacy policy digest")]
    MissingLegacyPolicyDigest,
    /// A native engine identity carrying a legacy policy digest.
    #[error("native engine identity must not carry a legacy policy digest")]
    UnexpectedLegacyPolicyDigest,
}

impl CriticPolicyIdentity {
    /// Construct a canonical policy identity.
    ///
    /// Include and exclude inputs are set-like: ordering and duplicate entries
    /// do not change the resulting identity. Construction fails closed when the
    /// engine's required policy binding is missing or contradictory.
    pub fn new(
        configuration_authority: WorkspaceRootId,
        configuration_generation: u64,
        engine: CriticEngine,
        profile: NativeCriticProfile,
        severity: u8,
        include: impl IntoIterator<Item = String>,
        exclude: impl IntoIterator<Item = String>,
        legacy_policy_digest: Option<ContentDigest>,
    ) -> Result<Self, CriticPolicyIdentityError> {
        match (engine, &legacy_policy_digest) {
            (CriticEngine::Legacy, None) => {
                return Err(CriticPolicyIdentityError::MissingLegacyPolicyDigest);
            }
            (CriticEngine::Native, Some(_)) => {
                return Err(CriticPolicyIdentityError::UnexpectedLegacyPolicyDigest);
            }
            _ => {}
        }
        Ok(Self {
            configuration_authority,
            configuration_generation,
            engine,
            profile,
            severity,
            include: include.into_iter().collect(),
            exclude: exclude.into_iter().collect(),
            legacy_policy_digest,
        })
    }
}

/// Relevant project-fact identity for one diagnostic computation.
///
/// A bare generation is never globally meaningful: `Live` pairs the owning
/// workspace/fact-store authority with the generation it issued, and
/// `Snapshot` carries the complete fact snapshot's content digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticFactIdentity {
    /// No project facts were available or eligible.
    Unavailable,
    /// Current project facts identified by the issuing workspace authority and
    /// its generation.
    Live {
        /// Workspace/fact-store authority that issued the generation.
        workspace: WorkspaceRootId,
        /// Generation issued by that authority.
        generation: u64,
    },
    /// Offline/batch facts identified by a deterministic content digest.
    Snapshot(ContentDigest),
}

/// Versioned authorities whose semantic changes invalidate prior pull results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticResultSchemaVersions {
    rule_catalog: u32,
    alias_catalog: u16,
    suppression_contract: u16,
    projection: u16,
    remediation: u16,
}

impl DiagnosticResultSchemaVersions {
    /// Construct the schema/version inputs for result identity.
    #[must_use]
    pub const fn new(
        rule_catalog: u32,
        alias_catalog: u16,
        suppression_contract: u16,
        projection: u16,
        remediation: u16,
    ) -> Self {
        Self { rule_catalog, alias_catalog, suppression_contract, projection, remediation }
    }
}

/// Complete load-bearing input to a critic-bearing pull-diagnostic result ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticResultIdentityInput {
    source: DiagnosticSourceIdentity,
    policy: CriticPolicyIdentity,
    facts: DiagnosticFactIdentity,
    schemas: DiagnosticResultSchemaVersions,
}

impl DiagnosticResultIdentityInput {
    /// Construct a complete diagnostic result identity input.
    #[must_use]
    pub const fn new(
        source: DiagnosticSourceIdentity,
        policy: CriticPolicyIdentity,
        facts: DiagnosticFactIdentity,
        schemas: DiagnosticResultSchemaVersions,
    ) -> Self {
        Self { source, policy, facts, schemas }
    }

    /// Compose a deterministic, opaque public result ID.
    ///
    /// The ID is a versioned SHA-256 digest of the canonical length-prefixed
    /// subject encoding, computed by the repository's domain-separated
    /// content-digest authority. Raw paths, rule lists, and other
    /// configuration values are not exposed in the returned string.
    #[must_use]
    pub fn compose(&self) -> DiagnosticResultIdentity {
        let mut canonical = String::new();
        push_u64(
            &mut canonical,
            "identity_schema",
            u64::from(DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION),
        );
        push_str(&mut canonical, "document", self.source.document.as_wire());
        push_str(&mut canonical, "source_digest", self.source.content_digest.as_wire());
        push_optional_u64(&mut canonical, "document_generation", self.source.document_generation);
        push_str(
            &mut canonical,
            "configuration_authority",
            self.policy.configuration_authority.as_wire(),
        );
        push_u64(&mut canonical, "configuration_generation", self.policy.configuration_generation);
        push_str(
            &mut canonical,
            "critic_engine",
            match self.policy.engine {
                CriticEngine::Legacy => "legacy",
                CriticEngine::Native => "native",
            },
        );
        push_str(&mut canonical, "native_profile", self.policy.profile.as_str());
        push_u64(&mut canonical, "severity", u64::from(self.policy.severity));
        push_set(&mut canonical, "include", &self.policy.include);
        push_set(&mut canonical, "exclude", &self.policy.exclude);
        match &self.policy.legacy_policy_digest {
            Some(digest) => push_str(&mut canonical, "legacy_policy_digest", digest.as_wire()),
            None => push_str(&mut canonical, "legacy_policy_digest", "none"),
        }
        push_fact_identity(&mut canonical, &self.facts);
        push_u64(&mut canonical, "rule_catalog", u64::from(self.schemas.rule_catalog));
        push_u64(&mut canonical, "alias_catalog", u64::from(self.schemas.alias_catalog));
        push_u64(
            &mut canonical,
            "suppression_contract",
            u64::from(self.schemas.suppression_contract),
        );
        push_u64(&mut canonical, "projection_schema", u64::from(self.schemas.projection));
        push_u64(&mut canonical, "remediation_schema", u64::from(self.schemas.remediation));

        let digest = ContentDigest::of_bytes(canonical.as_bytes());
        DiagnosticResultIdentity(format!(
            "diagnostic-result.v{}-{}",
            DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION,
            digest.as_wire()
        ))
    }
}

/// Opaque deterministic LSP pull-diagnostic result identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticResultIdentity(String);

impl DiagnosticResultIdentity {
    /// Result ID string suitable for LSP `resultId` fields.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the identity and return its result-ID string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

fn push_fact_identity(output: &mut String, facts: &DiagnosticFactIdentity) {
    match facts {
        DiagnosticFactIdentity::Unavailable => push_str(output, "facts", "unavailable"),
        DiagnosticFactIdentity::Live { workspace, generation } => {
            push_str(output, "facts", "live");
            push_str(output, "fact_workspace", workspace.as_wire());
            push_u64(output, "fact_generation", *generation);
        }
        DiagnosticFactIdentity::Snapshot(digest) => {
            push_str(output, "facts", "snapshot");
            push_str(output, "fact_digest", digest.as_wire());
        }
    }
}

fn push_set(output: &mut String, name: &str, values: &BTreeSet<String>) {
    push_u64(output, name, values.len() as u64);
    for value in values {
        push_str(output, "item", value);
    }
}

fn push_optional_u64(output: &mut String, name: &str, value: Option<u64>) {
    match value {
        Some(value) => {
            push_str(output, name, "some");
            push_u64(output, "value", value);
        }
        None => push_str(output, name, "none"),
    }
}

fn push_u64(output: &mut String, name: &str, value: u64) {
    push_str(output, name, &value.to_string());
}

fn push_str(output: &mut String, name: &str, value: &str) {
    push_token(output, name);
    push_token(output, value);
}

fn push_token(output: &mut String, value: &str) {
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}

#[cfg(test)]
mod tests {
    use super::{
        CriticPolicyIdentity, CriticPolicyIdentityError, DiagnosticFactIdentity,
        DiagnosticResultIdentityInput, DiagnosticResultSchemaVersions, DiagnosticSourceIdentity,
    };
    use crate::config::CriticEngine;
    use crate::tooling::perl_critic::NativeCriticProfile;
    use perl_source_identity::{ContentDigest, LogicalSourceId, ProjectId, WorkspaceRootId};
    use std::collections::BTreeSet;

    fn folder(name: &str) -> WorkspaceRootId {
        WorkspaceRootId::from_project_and_root_key(
            &ProjectId::from_canonical_name("perl-lsp-swarm"),
            name,
        )
    }

    fn document(folder_id: &WorkspaceRootId, path: &str) -> LogicalSourceId {
        LogicalSourceId::from_root_and_path(folder_id, path)
    }

    fn digest_of(content: &str) -> ContentDigest {
        ContentDigest::of_bytes(content.as_bytes())
    }

    fn policy(
        authority: WorkspaceRootId,
        engine: CriticEngine,
    ) -> Result<CriticPolicyIdentity, CriticPolicyIdentityError> {
        let legacy_digest =
            (engine == CriticEngine::Legacy).then(|| digest_of("legacy profile v1"));
        CriticPolicyIdentity::new(
            authority,
            11,
            engine,
            NativeCriticProfile::Recommended,
            3,
            vec!["native.testing.require_use_strict".to_string()],
            vec!["native.security.string_eval".to_string()],
            legacy_digest,
        )
    }

    fn baseline() -> DiagnosticResultIdentityInput {
        let root = folder("workspace-a");
        DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new(
                document(&root, "lib/Module.pm"),
                digest_of("source bytes v1"),
                Some(7),
            ),
            policy(root.clone(), CriticEngine::Native)
                .unwrap_or_else(|error| unreachable!("valid native policy: {error}")),
            DiagnosticFactIdentity::Live { workspace: folder("workspace-a"), generation: 13 },
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
    }

    fn baseline_with(
        source: DiagnosticSourceIdentity,
        policy: CriticPolicyIdentity,
        facts: DiagnosticFactIdentity,
        schemas: DiagnosticResultSchemaVersions,
    ) -> DiagnosticResultIdentityInput {
        DiagnosticResultIdentityInput::new(source, policy, facts, schemas)
    }

    fn baseline_parts() -> (
        DiagnosticSourceIdentity,
        CriticPolicyIdentity,
        DiagnosticFactIdentity,
        DiagnosticResultSchemaVersions,
    ) {
        let input = baseline();
        (input.source, input.policy, input.facts, input.schemas)
    }

    #[test]
    fn identical_inputs_produce_identical_ids() {
        assert_eq!(baseline().compose(), baseline().compose());
    }

    #[test]
    fn every_load_bearing_field_changes_the_id() {
        let baseline_id = baseline().compose();
        let (source, baseline_policy, facts, schemas) = baseline_parts();
        let root = folder("workspace-a");

        // Source content change — the fundamental content-identity property.
        let changed_content = baseline_with(
            DiagnosticSourceIdentity::new(
                document(&root, "lib/Module.pm"),
                digest_of("source bytes v2"),
                Some(7),
            ),
            policy(root.clone(), CriticEngine::Native)
                .unwrap_or_else(|error| unreachable!("{error}")),
            facts.clone(),
            schemas,
        );
        assert_ne!(baseline_id, changed_content.compose());

        // Document identity change with identical content and counters.
        let other_document = baseline_with(
            DiagnosticSourceIdentity::new(
                document(&root, "lib/Other.pm"),
                digest_of("source bytes v1"),
                Some(7),
            ),
            policy(root.clone(), CriticEngine::Native)
                .unwrap_or_else(|error| unreachable!("{error}")),
            facts.clone(),
            schemas,
        );
        assert_ne!(baseline_id, other_document.compose());

        // Same content, same counters, different folder authority.
        let other_root = folder("workspace-b");
        let other_folder = baseline_with(
            DiagnosticSourceIdentity::new(
                document(&other_root, "lib/Module.pm"),
                digest_of("source bytes v1"),
                Some(7),
            ),
            policy(other_root.clone(), CriticEngine::Native)
                .unwrap_or_else(|error| unreachable!("{error}")),
            DiagnosticFactIdentity::Live { workspace: other_root, generation: 13 },
            schemas,
        );
        assert_ne!(baseline_id, other_folder.compose());

        // Document generation advance.
        let newer_document = baseline_with(
            DiagnosticSourceIdentity::new(
                document(&root, "lib/Module.pm"),
                digest_of("source bytes v1"),
                Some(8),
            ),
            policy(root.clone(), CriticEngine::Native)
                .unwrap_or_else(|error| unreachable!("{error}")),
            facts.clone(),
            schemas,
        );
        assert_ne!(baseline_id, newer_document.compose());

        // Policy fields, one at a time.
        let mut varied = baseline_policy.clone();
        varied.severity = 4;
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        let mut varied = baseline_policy.clone();
        varied.configuration_generation = 12;
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        let varied = policy(root.clone(), CriticEngine::Legacy)
            .unwrap_or_else(|error| unreachable!("legacy policy with digest: {error}"));
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        let mut varied = baseline_policy.clone();
        varied.profile = NativeCriticProfile::Strict;
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        let mut varied = baseline_policy.clone();
        varied.include = BTreeSet::from(["native.testing.require_prototypes".to_string()]);
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        let mut varied = baseline_policy.clone();
        varied.exclude = BTreeSet::from(["native.security.eval_string".to_string()]);
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), varied, facts.clone(), schemas).compose()
        );

        // Fact identity: store authority, generation, and availability.
        let other_store =
            DiagnosticFactIdentity::Live { workspace: folder("workspace-b"), generation: 13 };
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), baseline_policy.clone(), other_store, schemas).compose()
        );
        let newer_facts =
            DiagnosticFactIdentity::Live { workspace: folder("workspace-a"), generation: 14 };
        assert_ne!(
            baseline_id,
            baseline_with(source.clone(), baseline_policy.clone(), newer_facts, schemas).compose()
        );
        assert_ne!(
            baseline_id,
            baseline_with(
                source.clone(),
                baseline_policy.clone(),
                DiagnosticFactIdentity::Unavailable,
                schemas,
            )
            .compose()
        );
        assert_ne!(
            baseline_id,
            baseline_with(
                source.clone(),
                baseline_policy.clone(),
                DiagnosticFactIdentity::Snapshot(digest_of("fact snapshot")),
                schemas,
            )
            .compose()
        );

        // Every schema version, one at a time.
        for varied in [
            DiagnosticResultSchemaVersions::new(29, 1, 1, 1, 1),
            DiagnosticResultSchemaVersions::new(28, 2, 1, 1, 1),
            DiagnosticResultSchemaVersions::new(28, 1, 2, 1, 1),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 2, 1),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 2),
        ] {
            assert_ne!(
                baseline_id,
                baseline_with(source.clone(), baseline_policy.clone(), facts.clone(), varied)
                    .compose()
            );
        }
    }

    #[test]
    fn legacy_engine_requires_the_effective_legacy_policy_digest() {
        let root = folder("workspace-a");
        assert_eq!(
            CriticPolicyIdentity::new(
                root.clone(),
                11,
                CriticEngine::Legacy,
                NativeCriticProfile::Recommended,
                3,
                Vec::new(),
                Vec::new(),
                None,
            ),
            Err(CriticPolicyIdentityError::MissingLegacyPolicyDigest)
        );
        assert_eq!(
            CriticPolicyIdentity::new(
                root.clone(),
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                Vec::new(),
                Vec::new(),
                Some(digest_of("stray legacy digest")),
            ),
            Err(CriticPolicyIdentityError::UnexpectedLegacyPolicyDigest)
        );

        // Two legacy identities with different effective policies differ even
        // when every shared generation counter is equal.
        let legacy_a = CriticPolicyIdentity::new(
            root.clone(),
            11,
            CriticEngine::Legacy,
            NativeCriticProfile::Recommended,
            3,
            Vec::new(),
            Vec::new(),
            Some(digest_of("legacy profile A")),
        )
        .unwrap_or_else(|error| unreachable!("{error}"));
        let legacy_b = CriticPolicyIdentity::new(
            root.clone(),
            11,
            CriticEngine::Legacy,
            NativeCriticProfile::Recommended,
            3,
            Vec::new(),
            Vec::new(),
            Some(digest_of("legacy profile B")),
        )
        .unwrap_or_else(|error| unreachable!("{error}"));
        let facts = DiagnosticFactIdentity::Unavailable;
        let schemas = DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1);
        let source = DiagnosticSourceIdentity::new(
            document(&root, "lib/Module.pm"),
            digest_of("source bytes v1"),
            Some(7),
        );
        assert_ne!(
            baseline_with(source.clone(), legacy_a, facts.clone(), schemas).compose(),
            baseline_with(source, legacy_b, facts, schemas).compose()
        );
    }

    #[test]
    fn set_order_and_duplicates_do_not_change_the_id() {
        let root = folder("workspace-a");
        let source = DiagnosticSourceIdentity::new(
            document(&root, "lib/Module.pm"),
            digest_of("source bytes v1"),
            Some(7),
        );
        let first = baseline_with(
            source.clone(),
            CriticPolicyIdentity::new(
                root.clone(),
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["b".to_string(), "a".to_string(), "a".to_string()],
                vec!["d".to_string(), "c".to_string()],
                None,
            )
            .unwrap_or_else(|error| unreachable!("{error}")),
            DiagnosticFactIdentity::Unavailable,
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let second = baseline_with(
            source,
            CriticPolicyIdentity::new(
                root,
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string(), "c".to_string()],
                None,
            )
            .unwrap_or_else(|error| unreachable!("{error}")),
            DiagnosticFactIdentity::Unavailable,
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();

        assert_eq!(first, second);
    }

    #[test]
    fn length_prefixing_distinguishes_concatenation_collisions() {
        let root = folder("workspace-a");
        let source = DiagnosticSourceIdentity::new(
            document(&root, "lib/Module.pm"),
            digest_of("source bytes v1"),
            None,
        );
        let first = baseline_with(
            source.clone(),
            CriticPolicyIdentity::new(
                root.clone(),
                1,
                CriticEngine::Legacy,
                NativeCriticProfile::Recommended,
                3,
                vec!["a".to_string(), "bc".to_string()],
                Vec::new(),
                Some(digest_of("legacy profile")),
            )
            .unwrap_or_else(|error| unreachable!("{error}")),
            DiagnosticFactIdentity::Snapshot(digest_of("facts")),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let second = baseline_with(
            source,
            CriticPolicyIdentity::new(
                root,
                1,
                CriticEngine::Legacy,
                NativeCriticProfile::Recommended,
                3,
                vec!["ab".to_string(), "c".to_string()],
                Vec::new(),
                Some(digest_of("legacy profile")),
            )
            .unwrap_or_else(|error| unreachable!("{error}")),
            DiagnosticFactIdentity::Snapshot(digest_of("facts")),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();

        assert_ne!(first, second);
    }

    #[test]
    fn public_id_is_opaque_collision_resistant_and_fixed_shape() {
        let id = baseline().compose();

        assert!(id.as_str().starts_with("diagnostic-result.v2-sha256:"));
        assert_eq!(id.as_str().len(), "diagnostic-result.v2-sha256:".len() + 64);
        assert!(!id.as_str().contains("workspace-a"));
        assert!(
            id.as_str().strip_prefix("diagnostic-result.v2-sha256:").is_some_and(|digest| digest
                .chars()
                .all(|character| character.is_ascii_hexdigit()))
        );
    }
}
