//! Checked projection of the declarative configuration authority.
//!
//! `mod.rs` and `catalog.rs` retain the complete leaf inventory and its drift
//! tests. This projection applies the channel semantics proved by the live
//! runtime before later generations consume the catalog:
//!
//! - `workspace/configuration` writes per-folder `WorkspaceConfig`, not the
//!   shared `ServerConfig`;
//! - PERL5LIB and startup `@INC` probes are downstream inputs controlled by
//!   policy fields, not higher-precedence writers of those policy fields.

#[path = "mod.rs"]
mod declared;

pub(crate) use declared::{
    ConfigConsumer, ConfigOwner, ConfigScope, ConfigSensitivity, ConfigSource, ConfigValidation,
    ConfigValueKind, EvidencePolicy, FieldAuthority, InvalidValueFallback, InvalidationClass,
};

use std::sync::LazyLock;

const GLOBAL_SERVER_CHANNELS: &[ConfigSource] = &[
    ConfigSource::CompiledDefault,
    ConfigSource::InitializationOptions,
    ConfigSource::ProjectFile,
    ConfigSource::GlobalClientSettings,
];

const WORKSPACE_POLICY_CHANNELS: &[ConfigSource] = &[
    ConfigSource::CompiledDefault,
    ConfigSource::InitializationOptions,
    ConfigSource::ProjectFile,
    ConfigSource::GlobalClientSettings,
    ConfigSource::WorkspaceConfiguration,
];

/// Canonical effective-field authority consumed by configuration generations.
///
/// The backing vector is immutable after first use. Entries remain sorted in
/// the declaration order established by the source catalog.
pub(crate) static CONFIGURATION_AUTHORITY: LazyLock<Vec<FieldAuthority>> =
    LazyLock::new(|| declared::CONFIGURATION_AUTHORITY.iter().copied().map(check_field).collect());

/// Find one checked authority row by stable field ID.
pub(crate) fn authority_by_id(id: &str) -> Option<&'static FieldAuthority> {
    CONFIGURATION_AUTHORITY.iter().find(|field| field.id == id)
}

fn check_field(mut field: FieldAuthority) -> FieldAuthority {
    if field.owner == ConfigOwner::Server
        && field.sources.contains(&ConfigSource::WorkspaceConfiguration)
    {
        field.sources = GLOBAL_SERVER_CHANNELS;
    }

    if matches!(
        field.id,
        "workspace.perl5lib_precedence" | "workspace.use_perl5lib" | "workspace.use_system_inc"
    ) {
        field.sources = WORKSPACE_POLICY_CHANNELS;
    }

    field
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn workspace_policy_fields_are_not_written_by_observed_environment() {
        for id in [
            "workspace.perl5lib_precedence",
            "workspace.use_perl5lib",
            "workspace.use_system_inc",
        ] {
            let field = authority_by_id(id).unwrap_or_else(|| panic!("missing authority row {id}"));
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
