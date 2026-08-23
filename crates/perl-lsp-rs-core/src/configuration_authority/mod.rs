//! Typed authority for configuration scope, precedence, validation, and effects.
//!
//! This module is intentionally crate-private while the runtime still mutates
//! configuration through legacy handlers. It establishes the checked contract
//! consumed by the generation pipeline in #7057 without creating a second
//! effective-state store.

#![allow(dead_code)]

mod catalog;

pub(crate) use catalog::CONFIGURATION_AUTHORITY;

/// Rust configuration structure that owns an effective field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConfigOwner {
    Server,
    NextEdit,
    AiCompletion,
    AiStreaming,
    Workspace,
}

/// Scope at which an effective value is authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigScope {
    Global,
    WorkspaceFolder,
    DerivedGlobal,
    DerivedWorkspaceFolder,
}

/// Input authority, ordered from lowest to highest precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConfigSource {
    CompiledDefault,
    InitializationOptions,
    ProjectFile,
    TrustedUserSettings,
    GlobalClientSettings,
    WorkspaceConfiguration,
    Environment,
    SystemProbe,
    ProjectMetadata,
}

impl ConfigSource {
    pub(crate) const fn precedence(self) -> u8 {
        match self {
            Self::CompiledDefault => 0,
            Self::InitializationOptions => 1,
            Self::ProjectFile => 2,
            Self::TrustedUserSettings => 3,
            Self::GlobalClientSettings => 4,
            Self::WorkspaceConfiguration => 5,
            Self::Environment => 6,
            Self::SystemProbe => 7,
            Self::ProjectMetadata => 8,
        }
    }
}

/// Effective value shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigValueKind {
    Boolean,
    Unsigned,
    Float,
    String,
    OptionalString,
    StringList,
    Enum,
    DerivedList,
}

/// Validation rule applied before an input can become authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigValidation {
    Boolean,
    NonEmptyString,
    OptionalNonEmptyString,
    StringList,
    UnsignedRange { minimum: u64, maximum: u64 },
    PositiveFloat,
    KnownEnum,
    RelativeWorkspacePathList,
    AbsoluteExternalPathList,
    ExecutableAndArgs,
    HttpHeaderName,
    SafeHeaderPrefix,
    HttpsOrLoopbackEndpoint,
    Derived,
}

/// Disposition when a new source value is absent or invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvalidValueFallback {
    KeepLastValid,
    UseDefault,
    ClampToRange,
    RejectSource,
    RecomputeDerived,
}

/// Sensitivity and trust class for configuration evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigSensitivity {
    Ordinary,
    Path,
    Executable,
    NetworkEndpoint,
    CredentialLocator,
    HeaderMaterial,
}

/// How the value may appear in logs, receipts, and generated status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidencePolicy {
    SafeValue,
    BoundedValue,
    PathIdentityOnly,
    Redacted,
    DerivedDigestOnly,
}

/// Downstream work invalidated by a changed effective value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvalidationClass {
    None,
    InlayHints,
    Telemetry,
    Diagnostics,
    Formatting,
    InlineCompletion,
    WorkspaceDiscovery,
    ModuleResolution,
    RuntimeScheduling,
}

/// Runtime subsystem that consumes an effective field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigConsumer {
    InlayHintProvider,
    Telemetry,
    NativeCritic,
    LegacyCritic,
    DiagnosticCache,
    NativeFormatter,
    ExternalFormatter,
    SaveFormatting,
    NextEditGate,
    InlineCompletion,
    AiTransport,
    AiScheduler,
    WorkspaceDiscovery,
    ModuleResolver,
    WorkspaceIndex,
    DependencyGraph,
    PerlToolchain,
    TrustPolicy,
}

/// One leaf effective configuration field.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FieldAuthority {
    pub(crate) id: &'static str,
    pub(crate) owner: ConfigOwner,
    pub(crate) rust_field: &'static str,
    pub(crate) scope: ConfigScope,
    pub(crate) value_kind: ConfigValueKind,
    /// Low-to-high source precedence.
    pub(crate) sources: &'static [ConfigSource],
    pub(crate) validation: ConfigValidation,
    pub(crate) invalid_fallback: InvalidValueFallback,
    pub(crate) sensitivity: ConfigSensitivity,
    pub(crate) evidence_policy: EvidencePolicy,
    pub(crate) invalidation: InvalidationClass,
    pub(crate) consumers: &'static [ConfigConsumer],
    /// External source paths or environment names represented by this field.
    pub(crate) source_markers: &'static [&'static str],
}

pub(crate) fn authority_by_id(id: &str) -> Option<&'static FieldAuthority> {
    CONFIGURATION_AUTHORITY.iter().find(|field| field.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const CONTAINER_FIELDS: &[(ConfigOwner, &str)] = &[
        (ConfigOwner::Server, "next_edit"),
        (ConfigOwner::Server, "ai_completion"),
        (ConfigOwner::AiCompletion, "streaming"),
    ];

    fn owner_struct(owner: ConfigOwner) -> &'static str {
        match owner {
            ConfigOwner::Server => "ServerConfig",
            ConfigOwner::NextEdit => "NextEditConfig",
            ConfigOwner::AiCompletion => "AiCompletionConfig",
            ConfigOwner::AiStreaming => "AiStreamingConfig",
            ConfigOwner::Workspace => "WorkspaceConfig",
        }
    }

    fn public_fields(struct_name: &str) -> BTreeSet<String> {
        let source = include_str!("../config/mod.rs");
        let marker = format!("pub struct {struct_name} {{");
        let body = source.split_once(&marker).unwrap_or_else(|| panic!("missing {struct_name}")).1;
        let body = body.split_once("\n}").unwrap_or_else(|| panic!("unterminated {struct_name}")).0;

        body.lines()
            .filter_map(|line| {
                let line = line.trim();
                let field = line.strip_prefix("pub ")?.split_once(':')?.0.trim();
                (!field.is_empty()).then(|| field.to_string())
            })
            .collect()
    }

    #[test]
    fn every_effective_public_config_leaf_has_exactly_one_authority() {
        let mut expected = BTreeSet::new();
        for owner in [
            ConfigOwner::Server,
            ConfigOwner::NextEdit,
            ConfigOwner::AiCompletion,
            ConfigOwner::AiStreaming,
            ConfigOwner::Workspace,
        ] {
            for field in public_fields(owner_struct(owner)) {
                if !CONTAINER_FIELDS.contains(&(owner, field.as_str())) {
                    expected.insert((owner, field));
                }
            }
        }

        let actual = CONFIGURATION_AUTHORITY
            .iter()
            .map(|field| (field.owner, field.rust_field.to_string()))
            .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected, "configuration authority drift");
        assert_eq!(actual.len(), CONFIGURATION_AUTHORITY.len(), "duplicate owner/field authority");
    }

    #[test]
    fn ids_are_unique_sorted_and_source_precedence_is_monotonic() {
        let ids = CONFIGURATION_AUTHORITY.iter().map(|field| field.id).collect::<Vec<_>>();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids, sorted, "authority IDs must be canonical and sorted");

        for field in CONFIGURATION_AUTHORITY {
            assert!(!field.sources.is_empty(), "{} has no source authority", field.id);
            assert!(
                field.sources.windows(2).all(|pair| pair[0].precedence() < pair[1].precedence()),
                "{} has duplicate or reversed source precedence: {:?}",
                field.id,
                field.sources
            );
            assert!(!field.consumers.is_empty(), "{} has no consumer", field.id);
            assert!(!field.source_markers.is_empty(), "{} has no external marker", field.id);
        }
    }

    #[test]
    fn sensitive_fields_fail_closed_in_evidence() {
        for field in CONFIGURATION_AUTHORITY {
            match field.sensitivity {
                ConfigSensitivity::NetworkEndpoint
                | ConfigSensitivity::CredentialLocator
                | ConfigSensitivity::HeaderMaterial => assert!(
                    matches!(field.evidence_policy, EvidencePolicy::Redacted),
                    "{} must be redacted",
                    field.id
                ),
                ConfigSensitivity::Path | ConfigSensitivity::Executable => assert!(
                    matches!(
                        field.evidence_policy,
                        EvidencePolicy::PathIdentityOnly | EvidencePolicy::Redacted
                    ),
                    "{} must not emit raw path/command material",
                    field.id
                ),
                ConfigSensitivity::Ordinary => {}
            }
        }
    }

    #[test]
    fn client_channels_cannot_override_trusted_command_or_ai_transport_fields() {
        let restricted = [
            "ai.api_key_env",
            "ai.api_key_header",
            "ai.api_key_prefix",
            "ai.endpoint",
            "critic.legacy_profile",
            "critic.legacy_theme",
            "formatting.extra_args",
            "formatting.profile",
            "workspace.external_include_paths",
            "workspace.perl_args",
            "workspace.perl_path",
        ];

        for id in restricted {
            let field = authority_by_id(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(
                !field.sources.iter().any(|source| matches!(
                    source,
                    ConfigSource::InitializationOptions
                        | ConfigSource::GlobalClientSettings
                        | ConfigSource::WorkspaceConfiguration
                )),
                "{id} accepts an untrusted client channel: {:?}",
                field.sources
            );
        }
    }

    #[test]
    fn source_markers_and_rust_fields_remain_present_in_config_implementation() {
        let source = include_str!("../config/mod.rs");
        let mut missing = BTreeMap::<&str, Vec<&str>>::new();
        for field in CONFIGURATION_AUTHORITY {
            if !source.contains(field.rust_field) {
                missing.entry(field.id).or_default().push(field.rust_field);
            }
            for marker in field.source_markers {
                let normalized =
                    marker.rsplit_once('.').map_or(*marker, |(_, leaf)| leaf).trim_matches('`');
                if !source.contains(normalized) {
                    missing.entry(field.id).or_default().push(marker);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "authority references missing implementation markers: {missing:?}"
        );
    }

    #[test]
    fn derived_fields_are_digest_only_and_recomputed() {
        for field in CONFIGURATION_AUTHORITY.iter().filter(|field| {
            matches!(field.scope, ConfigScope::DerivedGlobal | ConfigScope::DerivedWorkspaceFolder)
        }) {
            assert_eq!(field.validation, ConfigValidation::Derived, "{}", field.id);
            assert_eq!(
                field.invalid_fallback,
                InvalidValueFallback::RecomputeDerived,
                "{}",
                field.id
            );
            assert_eq!(field.evidence_policy, EvidencePolicy::DerivedDigestOnly, "{}", field.id);
        }
    }
}
