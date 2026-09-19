//! Checked view of the declarative configuration authority.
//!
//! The declared catalog (`mod.rs` / `catalog.rs`) is the sole semantic
//! authority: every source/scope/owner relationship a generation or consumer
//! reads comes from the declared row itself. This module validates that
//! declaration and retains only non-semantic derived structure (a
//! deterministic index and lookup) over it.
//!
//! Validation answers `is the declared row valid?` with a typed,
//! row-specific diagnostic. It never rewrites sources, scope, or meaning: an
//! invalid declaration fails instead of being silently repaired (#10790).
//!
//! The suppression below is scoped to this module: the index and lookup exist
//! to prove the declaration valid under drift tests, and the generation
//! pipeline (#7057) consumes the declared authority through the same
//! `authority_by_id` spelling, so no consumer can select a different catalog
//! meaning. It stays an `allow` rather than an `expect` because the items are
//! genuinely live under `cfg(test)`.

#![allow(dead_code, unused_imports)]

#[path = "mod.rs"]
mod declared;

pub(crate) use declared::{
    ConfigConsumer, ConfigOwner, ConfigScope, ConfigSensitivity, ConfigSource, ConfigValidation,
    ConfigValueKind, EvidencePolicy, FieldAuthority, InvalidValueFallback, InvalidationClass,
};

use std::sync::LazyLock;

/// Typed rejection for a declared row that violates source/scope/owner law.
///
/// Each variant names the exact row and the violated invariant. Validation
/// returns these; it never repairs the row it rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorityViolation {
    /// A shared server-global field claims per-folder pull authority.
    /// `workspace/configuration` writes per-folder `WorkspaceConfig`, never
    /// the shared `ServerConfig`.
    ServerFieldClaimsFolderPull { id: &'static str },
    /// A non-derived policy field admits an environment or probe observation
    /// as a source writer. Observations are downstream derived facts about
    /// the world, never writers of policy.
    PolicyFieldAdmitsObservationWriter { id: &'static str, source: ConfigSource },
}

/// Canonical effective-field authority: validated references to the declared
/// rows themselves, in declaration order.
///
/// The backing vector is immutable after first use. A declaration that
/// violates source/scope/owner law fails here with the exact row named;
/// construction never narrows, widens, reorders, or copies what the catalog
/// declares.
pub(crate) static CONFIGURATION_AUTHORITY: LazyLock<Vec<&'static FieldAuthority>> =
    LazyLock::new(|| {
        let violations = validate_catalog();
        assert!(
            violations.is_empty(),
            "declared configuration authority violates source/scope/owner law: {violations:?}"
        );
        declared::CONFIGURATION_AUTHORITY.iter().collect()
    });

/// Find one authority row by stable field ID: the declared row itself, never
/// a rewritten projection.
pub(crate) fn authority_by_id(id: &str) -> Option<&'static FieldAuthority> {
    declared::authority_by_id(id)
}

/// Validate one declared row without rewriting it.
pub(crate) fn validate_row(field: &FieldAuthority) -> Result<(), AuthorityViolation> {
    if field.owner == ConfigOwner::Server
        && field.sources.contains(&ConfigSource::WorkspaceConfiguration)
    {
        return Err(AuthorityViolation::ServerFieldClaimsFolderPull { id: field.id });
    }
    if !matches!(field.scope, ConfigScope::DerivedGlobal | ConfigScope::DerivedWorkspaceFolder)
        && let Some(source) =
            field.sources.iter().copied().find(|source| {
                matches!(source, ConfigSource::Environment | ConfigSource::SystemProbe)
            })
    {
        return Err(AuthorityViolation::PolicyFieldAdmitsObservationWriter {
            id: field.id,
            source,
        });
    }
    Ok(())
}

/// Validate every declared row, collecting each violation.
pub(crate) fn validate_catalog() -> Vec<AuthorityViolation> {
    declared::CONFIGURATION_AUTHORITY.iter().filter_map(|field| validate_row(field).err()).collect()
}

#[cfg(test)]
mod tests {
    use perl_test_must::must_some_with;

    use super::*;

    static NO_CONSUMERS: &[ConfigConsumer] = &[];
    static NO_MARKERS: &[&str] = &[];
    static ENVIRONMENT_ONLY: &[ConfigSource] = &[ConfigSource::Environment];
    static SYSTEM_PROBE_ONLY: &[ConfigSource] = &[ConfigSource::SystemProbe];
    static FOLDER_PULL: &[ConfigSource] =
        &[ConfigSource::CompiledDefault, ConfigSource::WorkspaceConfiguration];

    fn synthetic_row(
        id: &'static str,
        owner: ConfigOwner,
        scope: ConfigScope,
        sources: &'static [ConfigSource],
    ) -> FieldAuthority {
        FieldAuthority {
            id,
            owner,
            rust_field: "synthetic",
            scope,
            value_kind: ConfigValueKind::Boolean,
            sources,
            validation: ConfigValidation::Boolean,
            invalid_fallback: InvalidValueFallback::KeepLastValid,
            sensitivity: ConfigSensitivity::Ordinary,
            evidence_policy: EvidencePolicy::SafeValue,
            invalidation: InvalidationClass::None,
            consumers: NO_CONSUMERS,
            source_markers: NO_MARKERS,
        }
    }

    /// The regression fixture for #10790: the declared catalog must already
    /// satisfy source/scope/owner law, so the checked layer has nothing to
    /// repair, and every consumed row must be the declared row itself.
    /// Restoring any silent source-channel rewrite fails here.
    #[test]
    fn declared_catalog_needs_no_silent_repair() {
        let violations = validate_catalog();
        assert!(violations.is_empty(), "checked layer must validate, not repair: {violations:?}");

        for declared in declared::CONFIGURATION_AUTHORITY {
            let consumed = authority_by_id(declared.id).expect("declared row resolves");
            assert!(
                std::ptr::eq(consumed, declared),
                "consumed row for {:?} is not the declared row",
                declared.id
            );
        }
    }

    /// Lookup returns the declared row itself, never a copied projection.
    /// Returning a copied projection with altered sources fails here.
    #[test]
    fn lookup_returns_the_declared_row_itself() {
        for declared in declared::CONFIGURATION_AUTHORITY {
            let via_checked = authority_by_id(declared.id).expect("checked lookup resolves");
            let via_declared =
                declared::authority_by_id(declared.id).expect("declared lookup resolves");
            assert!(std::ptr::eq(via_checked, declared));
            assert!(std::ptr::eq(via_checked, via_declared));
        }
    }

    /// The checked index is byte-stable: identical declared input produces
    /// identical output order, matching declaration order. Reordering or
    /// re-indexing the projection fails here.
    #[test]
    fn checked_index_is_byte_stable_in_declaration_order() {
        let first: Vec<&str> = CONFIGURATION_AUTHORITY.iter().map(|field| field.id).collect();
        let second: Vec<&str> = CONFIGURATION_AUTHORITY.iter().map(|field| field.id).collect();
        let declared: Vec<&str> =
            declared::CONFIGURATION_AUTHORITY.iter().map(|field| field.id).collect();
        assert_eq!(first, second, "repeated index builds must be identical");
        assert_eq!(first, declared, "index order must match declaration order");
    }

    /// A declared server-global field claiming per-folder pull authority is
    /// rejected with a row-specific diagnostic instead of being silently
    /// narrowed.
    #[test]
    fn rejects_a_server_field_claiming_folder_pull_authority() {
        let row = synthetic_row(
            "synthetic.server_global",
            ConfigOwner::Server,
            ConfigScope::Global,
            FOLDER_PULL,
        );
        assert_eq!(
            validate_row(&row),
            Err(AuthorityViolation::ServerFieldClaimsFolderPull { id: "synthetic.server_global" })
        );
    }

    /// Workspace policy cannot be written by environment observations.
    #[test]
    fn rejects_a_policy_field_admitting_an_environment_writer() {
        let row = synthetic_row(
            "synthetic.workspace_policy",
            ConfigOwner::Workspace,
            ConfigScope::WorkspaceFolder,
            ENVIRONMENT_ONLY,
        );
        assert_eq!(
            validate_row(&row),
            Err(AuthorityViolation::PolicyFieldAdmitsObservationWriter {
                id: "synthetic.workspace_policy",
                source: ConfigSource::Environment,
            })
        );
    }

    /// Workspace policy cannot be written by system-probe observations.
    #[test]
    fn rejects_a_policy_field_admitting_a_system_probe_writer() {
        let row = synthetic_row(
            "synthetic.workspace_policy",
            ConfigOwner::Workspace,
            ConfigScope::WorkspaceFolder,
            SYSTEM_PROBE_ONLY,
        );
        assert_eq!(
            validate_row(&row),
            Err(AuthorityViolation::PolicyFieldAdmitsObservationWriter {
                id: "synthetic.workspace_policy",
                source: ConfigSource::SystemProbe,
            })
        );
    }

    /// Derived facts may still observe the environment: scoping, not the
    /// source alone, decides whether a writer relationship is claimed.
    #[test]
    fn accepts_derived_rows_observing_the_environment() {
        let row = synthetic_row(
            "synthetic.derived_fact",
            ConfigOwner::Workspace,
            ConfigScope::DerivedWorkspaceFolder,
            ENVIRONMENT_ONLY,
        );
        assert_eq!(validate_row(&row), Ok(()));
    }

    #[test]
    fn shared_server_fields_do_not_claim_folder_pull_authority() {
        let offenders = CONFIGURATION_AUTHORITY
            .iter()
            .filter(|field| {
                field.owner == ConfigOwner::Server
                    && field.sources.contains(&ConfigSource::WorkspaceConfiguration)
            })
            .map(|field| field.id)
            .collect::<Vec<_>>();

        assert!(
            offenders.is_empty(),
            "workspace/configuration does not write shared ServerConfig fields: {offenders:?}"
        );
    }

    #[test]
    fn formatter_engine_uses_project_and_generic_client_channels_only() {
        let field =
            must_some_with(authority_by_id("formatting.engine"), "missing formatter authority");

        assert_eq!(field.owner, ConfigOwner::Server);
        assert!(field.sources.contains(&ConfigSource::ProjectFile));
        assert!(field.sources.contains(&ConfigSource::InitializationOptions));
        assert!(field.sources.contains(&ConfigSource::GlobalClientSettings));
        assert!(!field.sources.contains(&ConfigSource::WorkspaceConfiguration));
    }

    #[test]
    fn workspace_policy_fields_are_not_written_by_observed_environment() {
        for id in
            ["workspace.perl5lib_precedence", "workspace.use_perl5lib", "workspace.use_system_inc"]
        {
            let field = must_some_with(authority_by_id(id), format!("missing authority row {id}"));
            assert!(
                !field.sources.contains(&ConfigSource::Environment)
                    && !field.sources.contains(&ConfigSource::SystemProbe),
                "{id} is a policy input; PERL5LIB/@INC observations are downstream facts: {:?}",
                field.sources
            );
        }
    }

    #[test]
    fn derived_environment_and_probe_rows_remain_explicit() {
        let environment_rows = CONFIGURATION_AUTHORITY
            .iter()
            .filter(|field| field.sources.contains(&ConfigSource::Environment))
            .collect::<Vec<_>>();
        let probe_rows = CONFIGURATION_AUTHORITY
            .iter()
            .filter(|field| field.sources.contains(&ConfigSource::SystemProbe))
            .collect::<Vec<_>>();

        assert!(
            environment_rows.iter().all(|field| matches!(
                field.scope,
                ConfigScope::DerivedGlobal | ConfigScope::DerivedWorkspaceFolder
            )),
            "environment sources must describe derived facts, not direct policy: {environment_rows:?}"
        );
        assert!(
            probe_rows.iter().all(|field| matches!(
                field.scope,
                ConfigScope::DerivedGlobal | ConfigScope::DerivedWorkspaceFolder
            )),
            "system probes must describe derived facts, not direct policy: {probe_rows:?}"
        );
    }
}
