//! Deterministic identity for critic-bearing pull-diagnostic results.

use std::collections::BTreeSet;

use crate::config::CriticEngine;

use super::NativeCriticProfile;

/// Schema version for [`DiagnosticResultIdentity`] composition.
pub const DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION: u16 = 1;

const FNV1A_128_OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const FNV1A_128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// Source snapshot inputs that affect diagnostic output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSourceIdentity {
    content_digest: String,
    document_generation: Option<u64>,
}

impl DiagnosticSourceIdentity {
    /// Construct source identity from a precomputed content digest and optional
    /// live document generation.
    #[must_use]
    pub fn new(content_digest: impl Into<String>, document_generation: Option<u64>) -> Self {
        Self { content_digest: content_digest.into(), document_generation }
    }
}

/// Accepted critic policy inputs for one document/folder configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticPolicyIdentity {
    configuration_generation: u64,
    engine: CriticEngine,
    profile: NativeCriticProfile,
    severity: u8,
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
}

impl CriticPolicyIdentity {
    /// Construct a canonical policy identity.
    ///
    /// Include and exclude inputs are set-like: ordering and duplicate entries
    /// do not change the resulting identity.
    #[must_use]
    pub fn new(
        configuration_generation: u64,
        engine: CriticEngine,
        profile: NativeCriticProfile,
        severity: u8,
        include: impl IntoIterator<Item = String>,
        exclude: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            configuration_generation,
            engine,
            profile,
            severity,
            include: include.into_iter().collect(),
            exclude: exclude.into_iter().collect(),
        }
    }
}

/// Relevant project-fact identity for one diagnostic computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticFactIdentity {
    /// No project facts were available or eligible.
    Unavailable,
    /// Current project facts are identified by a generation.
    Generation(u64),
    /// Offline/batch facts are identified by a deterministic digest.
    Digest(String),
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
    /// The ID is a stable versioned digest. Raw paths, rule lists, and other
    /// configuration values are not exposed in the returned string.
    #[must_use]
    pub fn compose(&self) -> DiagnosticResultIdentity {
        let mut canonical = String::new();
        push_u64(
            &mut canonical,
            "identity_schema",
            u64::from(DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION),
        );
        push_str(&mut canonical, "source_digest", &self.source.content_digest);
        push_optional_u64(&mut canonical, "document_generation", self.source.document_generation);
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

        let digest = fnv1a_128(canonical.as_bytes());
        DiagnosticResultIdentity(format!(
            "diagnostic-result.v{}-{digest:032x}",
            DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION
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
        DiagnosticFactIdentity::Generation(generation) => {
            push_str(output, "facts", "generation");
            push_u64(output, "fact_generation", *generation);
        }
        DiagnosticFactIdentity::Digest(digest) => {
            push_str(output, "facts", "digest");
            push_str(output, "fact_digest", digest);
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

fn fnv1a_128(bytes: &[u8]) -> u128 {
    let mut hash = FNV1A_128_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(FNV1A_128_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        CriticPolicyIdentity, DiagnosticFactIdentity, DiagnosticResultIdentityInput,
        DiagnosticResultSchemaVersions, DiagnosticSourceIdentity,
    };
    use crate::config::CriticEngine;
    use crate::tooling::perl_critic::NativeCriticProfile;

    fn baseline() -> DiagnosticResultIdentityInput {
        DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["native.testing.require_use_strict".to_string()],
                vec!["native.security.string_eval".to_string()],
            ),
            DiagnosticFactIdentity::Generation(13),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
    }

    #[test]
    fn identical_inputs_produce_identical_ids() {
        assert_eq!(baseline().compose(), baseline().compose());
    }

    #[test]
    fn profile_and_configuration_generation_change_the_id() {
        let baseline_id = baseline().compose();
        let strict = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Strict,
                3,
                vec!["native.testing.require_use_strict".to_string()],
                vec!["native.security.string_eval".to_string()],
            ),
            DiagnosticFactIdentity::Generation(13),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let newer_config = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                12,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["native.testing.require_use_strict".to_string()],
                vec!["native.security.string_eval".to_string()],
            ),
            DiagnosticFactIdentity::Generation(13),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();

        assert_ne!(baseline_id, strict);
        assert_ne!(baseline_id, newer_config);
    }

    #[test]
    fn set_order_and_duplicates_do_not_change_the_id() {
        let first = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["b".to_string(), "a".to_string(), "a".to_string()],
                vec!["d".to_string(), "c".to_string()],
            ),
            DiagnosticFactIdentity::Unavailable,
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let second = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string(), "c".to_string()],
            ),
            DiagnosticFactIdentity::Unavailable,
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();

        assert_eq!(first, second);
    }

    #[test]
    fn fact_and_schema_changes_invalidate_the_id() {
        let baseline_id = baseline().compose();
        let no_facts = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["native.testing.require_use_strict".to_string()],
                vec!["native.security.string_eval".to_string()],
            ),
            DiagnosticFactIdentity::Unavailable,
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let new_alias_schema = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["native.testing.require_use_strict".to_string()],
                vec!["native.security.string_eval".to_string()],
            ),
            DiagnosticFactIdentity::Generation(13),
            DiagnosticResultSchemaVersions::new(28, 2, 1, 1, 1),
        )
        .compose();

        assert_ne!(baseline_id, no_facts);
        assert_ne!(baseline_id, new_alias_schema);
    }

    #[test]
    fn length_prefixing_distinguishes_concatenation_collisions() {
        let first = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", None),
            CriticPolicyIdentity::new(
                1,
                CriticEngine::Legacy,
                NativeCriticProfile::Recommended,
                3,
                vec!["a".to_string(), "bc".to_string()],
                Vec::new(),
            ),
            DiagnosticFactIdentity::Digest("facts".to_string()),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();
        let second = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("sha256:source", None),
            CriticPolicyIdentity::new(
                1,
                CriticEngine::Legacy,
                NativeCriticProfile::Recommended,
                3,
                vec!["ab".to_string(), "c".to_string()],
                Vec::new(),
            ),
            DiagnosticFactIdentity::Digest("facts".to_string()),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        )
        .compose();

        assert_ne!(first, second);
    }

    #[test]
    fn public_id_is_opaque_and_fixed_shape() {
        let input = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new("private/source/path", Some(7)),
            CriticPolicyIdentity::new(
                11,
                CriticEngine::Native,
                NativeCriticProfile::Recommended,
                3,
                vec!["native.private.rule".to_string()],
                Vec::new(),
            ),
            DiagnosticFactIdentity::Digest("private/fact/path".to_string()),
            DiagnosticResultSchemaVersions::new(28, 1, 1, 1, 1),
        );
        let id = input.compose();

        assert!(id.as_str().starts_with("diagnostic-result.v1-"));
        assert_eq!(id.as_str().len(), "diagnostic-result.v1-".len() + 32);
        assert!(!id.as_str().contains("private"));
        assert!(
            id.as_str().strip_prefix("diagnostic-result.v1-").is_some_and(|digest| digest
                .chars()
                .all(|character| character.is_ascii_hexdigit()))
        );
    }
}
