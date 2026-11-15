//! Shared dbix class contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::dbix_class`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::framework::{AdapterId, ModuleActivationIdentity};
use crate::{AnchorId, Confidence, FileId, SourceGeneration};

/// Framework family handled by this adapter.
pub const DBIX_CLASS_FRAMEWORK_NAME: &str = "DBIx::Class";

/// Exact activation module admitted by the reviewed result-class profile.
pub const DBIX_CLASS_CORE_MODULE: &str = "DBIx::Class::Core";

/// Reviewed stable DBIx::Class version family.
///
/// The profile is intentionally bounded to the 0.08 release family. A future
/// version family requires a separate review before it can produce exact
/// result/source identities.
pub const DBIX_CLASS_VERSION_CONSTRAINT: &str = ">=0.080000,<0.090000";

/// Stable shadow adapter identity reserved for DBIx::Class.
pub const DBIX_CLASS_ADAPTER_ID: AdapterId = AdapterId(0x0044_4249);

/// Versioned identity of the reviewed result-class/result-source profile.
pub const DBIX_CLASS_PROFILE_VERSION: &str = "dbix-class.result-source.1.v1";

/// Descriptor schema revision for the DBIx::Class shadow adapter.
pub const DBIX_CLASS_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Static inheritance form that activates one reviewed result class.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DbixClassInheritanceForm {
    /// `use base 'DBIx::Class::Core';`.
    Base,
    /// `use parent 'DBIx::Class::Core';`.
    Parent,
}

/// Source evidence for result-class activation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbixClassInheritanceEvidence {
    /// Exact static activation of the reviewed module.
    Exact {
        /// Admitted inheritance spelling.
        form: DbixClassInheritanceForm,
        /// Exact inherited module.
        module: String,
    },
    /// No DBIx::Class activation was present in the package.
    Missing,
    /// The parent expression is computed at runtime.
    Dynamic {
        /// Bounded dynamic-boundary explanation.
        reason: String,
    },
    /// A static inheritance spelling was present but is outside the profile.
    Unsupported {
        /// Bounded unsupported explanation.
        reason: String,
    },
    /// Parser recovery made the activation source non-exact.
    Recovered {
        /// Bounded recovery explanation.
        reason: String,
    },
}

impl DbixClassInheritanceEvidence {
    /// Whether this evidence is an exact reviewed activation.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(
            self,
            Self::Exact {
                module,
                form: DbixClassInheritanceForm::Base | DbixClassInheritanceForm::Parent,
            } if module == DBIX_CLASS_CORE_MODULE
        )
    }
}

/// Source evidence for the result source/table declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbixTableEvidence {
    /// Static `__PACKAGE__->table(...)` source name.
    Static {
        /// Static table/source spelling.
        name: String,
        /// Anchor for the table declaration statement.
        anchor_id: AnchorId,
        /// Byte range covering the static table/source argument.
        source_range: (u32, u32),
    },
    /// No table/source declaration was present.
    Missing,
    /// The table/source expression is computed at runtime.
    Dynamic {
        /// Bounded dynamic-boundary explanation.
        reason: String,
    },
    /// Parser recovery made the table/source spelling non-exact.
    Recovered {
        /// Bounded recovery explanation.
        reason: String,
    },
}

/// Load-bearing source identity for one result-class candidate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbixResultSiteAnchor {
    /// Source file identity.
    pub file_id: FileId,
    /// Owning package/result-class spelling.
    pub package: Option<String>,
    /// Anchor for the inheritance activation statement.
    pub activation_anchor_id: Option<AnchorId>,
    /// Byte range of the activation statement.
    pub activation_range: Option<(u32, u32)>,
    /// Current source generation.
    pub source_generation: SourceGeneration,
    /// Explicit project/root identity, when source-proven by the caller.
    pub root_identity: Option<String>,
    /// Explicit schema owner identity, when source-proven by the caller.
    pub schema_identity: Option<String>,
}

impl DbixResultSiteAnchor {
    /// Construct one result-class site anchor.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file_id: FileId,
        package: Option<String>,
        activation_anchor_id: Option<AnchorId>,
        activation_range: Option<(u32, u32)>,
        source_generation: SourceGeneration,
        root_identity: Option<String>,
        schema_identity: Option<String>,
    ) -> Self {
        Self {
            file_id,
            package,
            activation_anchor_id,
            activation_range,
            source_generation,
            root_identity,
            schema_identity,
        }
    }
}

/// Typed result-class/result-source profile outcome.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbixResultProfileOutcome {
    /// Exact result class and static result source were established.
    Exact,
    /// Complete evidence establishes that the profile is absent.
    Absent {
        /// Bounded absence explanation.
        reason: String,
    },
    /// Module version or static profile is unsupported.
    Unsupported {
        /// Bounded unsupported explanation.
        reason: String,
    },
    /// Module/source evidence is missing or unavailable.
    MissingOrUnavailable {
        /// Bounded unavailability explanation.
        reason: String,
    },
    /// Module evidence is ambiguous or conflicting.
    AmbiguousOrConflicting {
        /// Bounded conflict explanation.
        reason: String,
    },
    /// Source, module, version, or input identity is stale or incomplete.
    StaleOrIncomplete {
        /// Bounded staleness explanation.
        reason: String,
    },
    /// Runtime-computed inheritance or table/source spelling was observed.
    DynamicBoundary {
        /// Bounded dynamic-boundary explanation.
        reason: String,
    },
    /// Parser-recovered source cannot establish exact identity.
    RecoveredSource {
        /// Bounded recovery explanation.
        reason: String,
    },
    /// Detection was cancelled or exhausted its budget.
    InstrumentFailure {
        /// Bounded failure explanation.
        reason: String,
    },
}

impl DbixResultProfileOutcome {
    /// Whether the profile established exact identities.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// Completeness of the emitted DBIx::Class identity profile.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbixFactCompleteness {
    /// Both distinct identities and all required proof were established.
    Complete,
    /// Evidence is bounded and no exact identities may be consumed.
    Bounded,
}

/// Canonical identity of one current result class.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbixResultClassIdentity {
    /// Deterministic root-scoped key.
    pub key: String,
    /// Exact package/result-class spelling.
    pub package: String,
    /// Resolver/project scope identity.
    pub scope_identity: String,
    /// Explicit project/root identity, when source-proven.
    pub root_identity: Option<String>,
    /// Explicit owning schema identity, when source-proven.
    pub schema_identity: Option<String>,
}

impl DbixResultClassIdentity {
    /// Construct the stable key for a caller-established result-class identity.
    #[must_use]
    pub fn new(
        package: String,
        scope_identity: String,
        root_identity: Option<String>,
        schema_identity: Option<String>,
    ) -> Self {
        let key = format!(
            "dbix-class/result-class/v1/{}/{}",
            identity_component(&scope_identity),
            identity_component(&package)
        );
        Self { key, package, scope_identity, root_identity, schema_identity }
    }
}

/// Canonical identity of one current result source.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbixResultSourceIdentity {
    /// Deterministic root- and class-scoped key.
    pub key: String,
    /// Owning result-class key. Result class and result source remain distinct.
    pub result_class_key: String,
    /// Static table/source spelling.
    pub table_name: String,
    /// Source anchor for the table/source declaration.
    pub declaration_anchor_id: AnchorId,
    /// Byte range of the table/source argument.
    pub declaration_range: (u32, u32),
}

impl DbixResultSourceIdentity {
    /// Construct the stable source key, including the escaped owning class key.
    #[must_use]
    pub fn new(
        result_class_key: String,
        table_name: String,
        declaration_anchor_id: AnchorId,
        declaration_range: (u32, u32),
    ) -> Self {
        let key = format!(
            "dbix-class/result-source/v1/{}/{}",
            identity_component(&result_class_key),
            identity_component(&table_name)
        );
        Self { key, result_class_key, table_name, declaration_anchor_id, declaration_range }
    }
}

/// Checked shadow facts for one DBIx::Class result-class candidate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbixResultProfileFacts {
    /// Typed profile outcome.
    pub outcome: DbixResultProfileOutcome,
    /// Versioned reviewed profile identity.
    pub profile_version: &'static str,
    /// Detection confidence.
    pub confidence: Confidence,
    /// Profile completeness.
    pub completeness: DbixFactCompleteness,
    /// Current source generation.
    pub source_generation: SourceGeneration,
    /// Project/module observation generation.
    pub project_generation: SourceGeneration,
    /// Resolver/project scope identity, when present.
    pub scope_identity: Option<String>,
    /// Exact environment identity, when present.
    pub environment_identity: Option<String>,
    /// Resolved activation module, when present.
    pub resolved_module: Option<ModuleActivationIdentity>,
    /// Observed supported framework version, when present.
    pub framework_version: Option<String>,
    /// Distinct result-class identity on exact outcomes.
    pub result_class: Option<DbixResultClassIdentity>,
    /// Distinct result-source identity on exact outcomes.
    pub result_source: Option<DbixResultSourceIdentity>,
    /// Bounded limitations carried by the shadow profile.
    pub limitations: Vec<String>,
}

impl DbixResultProfileFacts {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outcome: DbixResultProfileOutcome,
        profile_version: &'static str,
        confidence: Confidence,
        completeness: DbixFactCompleteness,
        source_generation: SourceGeneration,
        project_generation: SourceGeneration,
        scope_identity: Option<String>,
        environment_identity: Option<String>,
        resolved_module: Option<ModuleActivationIdentity>,
        framework_version: Option<String>,
        result_class: Option<DbixResultClassIdentity>,
        result_source: Option<DbixResultSourceIdentity>,
        limitations: Vec<String>,
    ) -> Self {
        Self {
            outcome,
            profile_version,
            confidence,
            completeness,
            source_generation,
            project_generation,
            scope_identity,
            environment_identity,
            resolved_module,
            framework_version,
            result_class,
            result_source,
            limitations,
        }
    }
}

fn identity_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F").replace(':', "%3A")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_keys_preserve_component_and_nested_class_escaping() {
        let class = DbixResultClassIdentity::new(
            "My::Result".to_string(),
            "root/%A".to_string(),
            None,
            None,
        );
        assert_eq!(class.key, "dbix-class/result-class/v1/root%2F%25A/My%3A%3AResult");
        let source = DbixResultSourceIdentity::new(
            class.key.clone(),
            "table/name%".to_string(),
            AnchorId(7),
            (10, 23),
        );
        assert_eq!(
            source.key,
            "dbix-class/result-source/v1/dbix-class%2Fresult-class%2Fv1%2Froot%252F%2525A%2FMy%253A%253AResult/table%2Fname%25"
        );
        assert_eq!(source.result_class_key, class.key);
        assert_eq!(source.declaration_anchor_id, AnchorId(7));
        assert_eq!(source.declaration_range, (10, 23));
    }

    #[test]
    fn identity_construction_preserves_caller_spelling_without_activation_policy() {
        let class = DbixResultClassIdentity::new(
            " App::Result ".to_string(),
            " root ".to_string(),
            Some("project".to_string()),
            Some("schema".to_string()),
        );
        assert_eq!(class.key, "dbix-class/result-class/v1/ root / App%3A%3AResult ");
        assert_eq!(class.package, " App::Result ");
        assert_eq!(class.scope_identity, " root ");
        assert_eq!(class.root_identity.as_deref(), Some("project"));
        assert_eq!(class.schema_identity.as_deref(), Some("schema"));
    }
}
