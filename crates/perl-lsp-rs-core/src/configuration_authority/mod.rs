//! Typed authority for configuration scope, precedence, validation, and effects.
//!
//! This module is intentionally crate-private while the runtime still mutates
//! configuration through legacy handlers. It establishes the checked contract
//! consumed by the generation pipeline in #7057 without creating a second
//! effective-state store.
//!
//! Explicit non-registration dispositions (#7054): parsed settings that
//! deliberately carry no authority row are limited to
//! `ProjectPerlConfig.version` (parsed from `.perl-lsp.toml`, documented as
//! reserved and ignored — no effective field exists to own) and the internal
//! `LspLimits` fields listed in `INTERNAL_UNPARSED_LIMIT_FIELDS` (compiled
//! defaults with no external configuration channel).

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

mod catalog;

pub(crate) use catalog::CONFIGURATION_AUTHORITY;

/// Rust configuration structure that owns an effective field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConfigOwner {
    Server,
    AiCompletion,
    AiStreaming,
    Workspace,
    Limits,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ConfigValidation {
    Boolean,
    NonEmptyString,
    OptionalNonEmptyString,
    StringList,
    UnsignedRange {
        minimum: u64,
        maximum: u64,
    },
    PositiveFloat,
    KnownEnum,
    RelativeWorkspacePathList,
    AbsoluteExternalPathList,
    ExecutableAndArgs,
    HttpHeaderName,
    SafeHeaderPrefix,
    HttpsOrLoopbackEndpoint,
    /// Non-negative integer accepted as-is. Declared floor is 0 (matching
    /// `schemas/perllsp-settings.schema.json`); no range clamp is enforced by
    /// the runtime today — enforcement belongs to the #7057 runtime slice.
    Unsigned,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ConfigSensitivity {
    Ordinary,
    Path,
    Executable,
    NetworkEndpoint,
    CredentialLocator,
    HeaderMaterial,
}

/// How the value may appear in logs, receipts, and generated status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    InlineCompletion,
    AiTransport,
    AiScheduler,
    WorkspaceDiscovery,
    ModuleResolver,
    WorkspaceIndex,
    DependencyGraph,
    PerlToolchain,
    TrustPolicy,
    /// Request-result truncation: providers that cap list-shaped responses
    /// (symbols, references, completions, lenses, diagnostics, hints).
    ResultCaps,
    /// Bounded execution: caches, index budgets, deadlines, and the memory
    /// monitor that gates degradation behavior.
    BoundedExecution,
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

    use ConfigConsumer as Consumer;
    use ConfigSensitivity as Sensitivity;
    use ConfigSource as Source;
    use ConfigValidation as Validation;
    use ConfigValueKind as Kind;
    use EvidencePolicy as Evidence;
    use InvalidValueFallback as Fallback;
    use InvalidationClass as Invalidation;

    /// Owner structs that only aggregate nested config and own no leaf value.
    const CONTAINER_FIELDS: &[(ConfigOwner, &str)] = &[
        (ConfigOwner::Server, "next_edit"),
        (ConfigOwner::Server, "ai_completion"),
        (ConfigOwner::AiCompletion, "streaming"),
        // Memory thresholds are declared as their own authority rows even
        // though the runtime nests them under `LspLimits.memory_budget`.
        (ConfigOwner::Limits, "memory_budget"),
    ];

    /// Container fields whose leaves come from a named nested struct in the
    /// same source file instead of being skipped.
    const NESTED_CONTAINER_FIELDS: &[((ConfigOwner, &str), &str)] =
        &[((ConfigOwner::Limits, "memory_budget"), "MemoryBudget")];

    /// `LspLimits` fields with no external configuration channel: compiled
    /// tuning constants read only by server internals. #7054 requires
    /// registration for *parsed* settings, so each is exempted here by name;
    /// adding a parse site for one of these keys must come with a catalog row
    /// and removal from this list.
    const INTERNAL_UNPARSED_LIMIT_FIELDS: &[&str] = &[
        "max_symbols_per_file",
        "parse_storm_threshold",
        "file_index_deadline",
        "regex_scan_deadline",
        "fs_operation_deadline",
        "semantic_tokens_deadline",
        "code_lens_resolve_deadline",
        "completion_deadline",
        "return_partial_on_timeout",
        "include_open_docs_when_degraded",
    ];

    fn owner_source(owner: ConfigOwner) -> &'static str {
        match owner {
            ConfigOwner::Limits => include_str!("../runtime/limits/mod.rs"),
            ConfigOwner::Server
            | ConfigOwner::AiCompletion
            | ConfigOwner::AiStreaming
            | ConfigOwner::Workspace => include_str!("../config/mod.rs"),
        }
    }

    fn owner_struct(owner: ConfigOwner) -> &'static str {
        match owner {
            ConfigOwner::Server => "ServerConfig",
            ConfigOwner::AiCompletion => "AiCompletionConfig",
            ConfigOwner::AiStreaming => "AiStreamingConfig",
            ConfigOwner::Workspace => "WorkspaceConfig",
            ConfigOwner::Limits => "LspLimits",
        }
    }

    fn public_fields(source: &str, struct_name: &str) -> BTreeSet<String> {
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

    fn expected_leaf_set() -> BTreeSet<(ConfigOwner, String)> {
        let mut expected = BTreeSet::new();
        for owner in [
            ConfigOwner::Server,
            ConfigOwner::AiCompletion,
            ConfigOwner::AiStreaming,
            ConfigOwner::Workspace,
            ConfigOwner::Limits,
        ] {
            let source = owner_source(owner);
            for field in public_fields(source, owner_struct(owner)) {
                if CONTAINER_FIELDS.contains(&(owner, field.as_str())) {
                    if let Some((_, nested)) = NESTED_CONTAINER_FIELDS
                        .iter()
                        .find(|((row_owner, name), _)| *row_owner == owner && *name == field)
                    {
                        for leaf in public_fields(source, nested) {
                            expected.insert((owner, leaf));
                        }
                    }
                    continue;
                }
                if owner == ConfigOwner::Limits
                    && INTERNAL_UNPARSED_LIMIT_FIELDS.contains(&field.as_str())
                {
                    continue;
                }
                expected.insert((owner, field));
            }
        }
        expected
    }

    fn authority_leaf_set(rows: &[FieldAuthority]) -> BTreeSet<(ConfigOwner, String)> {
        rows.iter().map(|field| (field.owner, field.rust_field.to_string())).collect()
    }

    /// Bidirectional drift between declared authority rows and parsed config
    /// leaves, phrased so mutated row slices can prove both failure modes.
    fn leaf_drift(rows: &[FieldAuthority]) -> Vec<String> {
        let expected = expected_leaf_set();
        let actual = authority_leaf_set(rows);
        let mut drift = Vec::new();
        for missing in expected.difference(&actual) {
            drift.push(format!("parsed-but-unregistered: {missing:?}"));
        }
        for phantom in actual.difference(&expected) {
            drift.push(format!("registered-but-unparsed: {phantom:?}"));
        }
        drift
    }

    #[test]
    fn every_effective_public_config_leaf_has_exactly_one_authority() {
        let actual = authority_leaf_set(CONFIGURATION_AUTHORITY);
        assert_eq!(actual, expected_leaf_set(), "configuration authority drift");
        assert_eq!(actual.len(), CONFIGURATION_AUTHORITY.len(), "duplicate owner/field authority");
    }

    #[test]
    fn dropping_a_limit_row_fails_the_machine_check() {
        let mut rows = CONFIGURATION_AUTHORITY.to_vec();
        let position = rows
            .iter()
            .position(|field| field.id == "limits.workspace_symbol_cap")
            .expect("limits row present");
        let removed = rows.remove(position);

        let drift = leaf_drift(&rows);
        assert!(
            drift.iter().any(|entry| entry.contains("parsed-but-unregistered")
                && entry.contains(removed.rust_field)),
            "removing a parsed limit row must fail the parity check: {drift:?}"
        );
    }

    #[test]
    fn registering_an_unparsed_limit_field_fails_the_machine_check() {
        let mut rows = CONFIGURATION_AUTHORITY.to_vec();
        rows.push(FieldAuthority {
            id: "limits.phantom_cap",
            owner: ConfigOwner::Limits,
            rust_field: "phantom_cap",
            scope: ConfigScope::Global,
            value_kind: Kind::Unsigned,
            sources: &[Source::CompiledDefault],
            validation: Validation::Unsigned,
            invalid_fallback: Fallback::KeepLastValid,
            sensitivity: Sensitivity::Ordinary,
            evidence_policy: Evidence::SafeValue,
            invalidation: Invalidation::RuntimeScheduling,
            consumers: &[Consumer::ResultCaps],
            source_markers: &["phantomCap"],
        });

        let drift = leaf_drift(&rows);
        assert!(
            drift
                .iter()
                .any(|entry| entry.contains("registered-but-unparsed")
                    && entry.contains("phantom_cap")),
            "registering a field no parser writes must fail the parity check: {drift:?}"
        );
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
            "ai.activation_authority",
            "ai.api_key_env",
            "ai.api_key_header",
            "ai.api_key_prefix",
            "ai.endpoint",
            "ai.model",
            "ai.provider",
            "ai.streaming.effective_enabled",
            "ai.streaming.user_enabled",
            "ai.user_enabled",
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

    /// Recurrence gate for #4997: AI arm/select authority must come only from
    /// compiled defaults plus trusted user/operator observations — never from
    /// a project file or any client channel. Restoring `ProjectFile`,
    /// `InitializationOptions`, `GlobalClientSettings`, or
    /// `WorkspaceConfiguration` to one of these rows must fail here.
    #[test]
    fn ai_arm_and_select_rows_admit_only_trusted_operator_sources() {
        const ARM_SELECT_ROWS: &[&str] = &[
            "ai.activation_authority",
            "ai.model",
            "ai.provider",
            "ai.streaming.effective_enabled",
            "ai.streaming.user_enabled",
            "ai.user_enabled",
        ];

        for id in ARM_SELECT_ROWS {
            let field = authority_by_id(id).unwrap_or_else(|| panic!("missing {id}"));
            for source in field.sources {
                assert!(
                    matches!(
                        source,
                        ConfigSource::CompiledDefault | ConfigSource::TrustedUserSettings
                    ),
                    "{id} row admits unauthorized source {source:?} (#4997)",
                );
            }
        }

        // The derived effective flag may additionally be reduced by the
        // project file, but still cannot be armed by any client channel.
        let effective = authority_by_id("ai.effective_enabled")
            .unwrap_or_else(|| panic!("missing ai.effective_enabled"));
        for source in effective.sources {
            assert!(
                matches!(
                    source,
                    ConfigSource::CompiledDefault
                        | ConfigSource::ProjectFile
                        | ConfigSource::TrustedUserSettings
                ),
                "ai.effective_enabled admits unauthorized source {source:?} (#4997)",
            );
        }
    }

    #[test]
    fn source_markers_and_rust_fields_remain_present_in_config_implementation() {
        let config_source = include_str!("../config/mod.rs");
        let limits_source = include_str!("../runtime/limits/mod.rs");
        let mut missing = BTreeMap::<&str, Vec<&str>>::new();
        for field in CONFIGURATION_AUTHORITY {
            let source =
                if field.owner == ConfigOwner::Limits { limits_source } else { config_source };
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
    fn limits_authority_matches_the_generic_settings_schema_exactly() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../../schemas/perllsp-settings.schema.json"))
                .expect("valid perllsp settings schema");
        let schema_keys = schema["properties"]["perl"]["properties"]["limits"]["properties"]
            .as_object()
            .expect("schema declares a perl.limits section")
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();

        let catalog_keys = CONFIGURATION_AUTHORITY
            .iter()
            .filter(|field| field.owner == ConfigOwner::Limits)
            .flat_map(|field| field.source_markers.iter().copied())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            schema_keys, catalog_keys,
            "perl.limits schema properties and limits authority rows must match exactly"
        );
    }

    #[test]
    fn limit_rows_stay_global_with_only_the_declared_client_channels() {
        const LIMITS_CHANNELS: &[Source] = &[
            Source::CompiledDefault,
            Source::InitializationOptions,
            Source::GlobalClientSettings,
            Source::WorkspaceConfiguration,
        ];

        for field in
            CONFIGURATION_AUTHORITY.iter().filter(|field| field.owner == ConfigOwner::Limits)
        {
            assert_eq!(field.scope, ConfigScope::Global, "{}", field.id);
            assert!(
                field.sources.iter().all(|source| LIMITS_CHANNELS.contains(source)),
                "{} claims an undeclared channel: {:?}",
                field.id,
                field.sources
            );
            assert!(
                field.sources.contains(&Source::GlobalClientSettings)
                    && field.sources.contains(&Source::WorkspaceConfiguration),
                "{} must declare both live write channels",
                field.id
            );
        }
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
