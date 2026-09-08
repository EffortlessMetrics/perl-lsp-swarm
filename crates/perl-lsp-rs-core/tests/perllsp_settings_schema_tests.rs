use perl_lsp_rs_core::config::{FormatterMode, Perl5LibPrecedence, ServerConfig, WorkspaceConfig};
use perl_lsp_rs_core::runtime::LspLimits;
use perl_test_must::must_some_with;
use serde_json::{Value, json};
use std::{error::Error, time::Duration};

fn production_source(source: &str) -> String {
    let lines: Vec<_> = source.lines().collect();
    let mut production = Vec::with_capacity(lines.len());
    let mut index = 0;

    while index < lines.len() {
        if lines[index].trim() == "#[cfg(test)]" {
            let mut next = index + 1;
            while next < lines.len() && lines[next].trim().is_empty() {
                next += 1;
            }

            // Some runtime modules have cfg(test)-only imports before production code.
            // The first cfg(test) item that is a function or module starts the fixture tail.
            if next < lines.len()
                && (lines[next].trim_start().starts_with("fn ")
                    || lines[next].trim_start().starts_with("mod "))
            {
                break;
            }
        }

        production.push(lines[index]);
        index += 1;
    }

    production.join("\n")
}

#[test]
fn removed_client_test_runner_authority_stays_absent_from_current_surfaces() {
    let sources = [
        ("core config", production_source(include_str!("../src/config/mod.rs"))),
        (
            "configuration authority model",
            production_source(include_str!("../src/configuration_authority/mod.rs")),
        ),
        (
            "configuration authority catalog",
            production_source(include_str!("../src/configuration_authority/catalog.rs")),
        ),
        (
            "settings schema",
            include_str!("../../../schemas/perllsp-settings.schema.json").to_owned(),
        ),
        ("configuration reference", include_str!("../../../docs/reference/CONFIG.md").to_owned()),
        (
            "configuration guide",
            include_str!("../../../docs/reference/CONFIGURATION.md").to_owned(),
        ),
        (
            "configuration schema reference",
            include_str!("../../../docs/reference/CONFIGURATION_SCHEMA.md").to_owned(),
        ),
        (
            "runtime workspace authority",
            production_source(include_str!("../../perl-lsp-rs/src/runtime/workspace.rs")),
        ),
        (
            "runtime lifecycle workspace authority",
            production_source(include_str!("../../perl-lsp-rs/src/runtime/lifecycle/workspace.rs")),
        ),
        (
            "runtime lifecycle capability authority",
            production_source(include_str!(
                "../../perl-lsp-rs/src/runtime/lifecycle/capabilities.rs"
            )),
        ),
    ];
    let forbidden = [
        "test_runner_command",
        "test_runner_args",
        "test_runner_timeout",
        "test_runner_enabled",
        "testRunner",
        "testRunnerEnabled",
        "testRunner.enabled",
        "testCommand",
        "testArgs",
        "testTimeout",
        "TestRunner",
    ];

    for (source_name, source) in sources {
        for marker in forbidden {
            assert!(
                !source.contains(marker),
                "removed client test-runner authority marker {marker:?} reintroduced in {source_name}"
            );
        }
    }
}

fn load_schema() -> Result<Value, Box<dyn Error>> {
    match serde_json::from_str(include_str!("../../../schemas/perllsp-settings.schema.json")) {
        Ok(value) => Ok(value),
        Err(error) => Err(Box::new(error)),
    }
}

/// #8311 recurrence check, part 1: every section exposed by the public
/// generic settings schema must map to a registered runtime owner.
///
/// A public configuration key may not name an editor/provider surface that
/// has no registered runtime owner; `nextEdit` was hidden for exactly that
/// reason. Adding a schema section without a registered owner (or retiring a
/// section without dropping its mapping) fails here. Reintroducing something
/// like `nextEdit` requires a dedicated issue/programme plus an actually
/// registered provider, at which point it earns a mapping entry.
#[test]
fn generic_schema_sections_have_registered_runtime_owners() -> Result<(), Box<dyn Error>> {
    /// Sections of the generic settings schema and the registered runtime
    /// owner that consumes each one. Deny-by-default: no entry, no exposure.
    const REGISTERED_OWNERS: &[(&str, &str)] = &[
        ("workspace", "WorkspaceConfig::update_from_value"),
        ("inlayHints", "ServerConfig::update_from_value"),
        ("limits", "LspLimits::update_from_value"),
        ("telemetry", "ServerConfig::update_from_value"),
        ("perlcritic", "ServerConfig::update_from_value (deprecated alias)"),
        ("critic", "ServerConfig::update_from_value"),
        ("formatting", "ServerConfig::update_from_value"),
        ("aiCompletion", "ServerConfig::update_from_value + registered inline-completion runtime"),
    ];

    let schema = load_schema()?;
    let sections = schema["properties"]["perl"]["properties"]
        .as_object()
        .ok_or("generic settings schema has no perl.properties object")?;

    for (section, owner) in REGISTERED_OWNERS {
        assert!(
            sections.contains_key(*section),
            "registered owner mapping for `{section}` ({owner}) has no matching schema section; \
             remove the stale mapping",
        );
    }
    for section in sections.keys() {
        assert!(
            REGISTERED_OWNERS.iter().any(|(name, _)| name == section),
            "public settings section `{section}` has no registered runtime owner (#8311): a \
             public configuration key may not name an editor/provider surface with no \
             registered runtime owner",
        );
    }

    Ok(())
}

/// #8311 recurrence check, part 2: the hidden next-edit setting must stay
/// absent from every surface that advertises public configuration.
///
/// The internal scaffold types remain (default-off, receipt-only, exercised
/// only by dev harnesses) and legacy supplied keys are answered with one
/// bounded ignored/deprecation reason by the config layer, but no schema,
/// example, onboarding, or editor-contribution surface may advertise
/// `nextEdit`/`[next_edit]` again until a provider is registered.
#[test]
fn hidden_next_edit_setting_stays_absent_from_public_configuration_surfaces() {
    let surfaces = [
        ("settings schema", include_str!("../../../schemas/perllsp-settings.schema.json")),
        ("configuration reference", include_str!("../../../docs/reference/CONFIG.md")),
        ("configuration guide", include_str!("../../../docs/reference/CONFIGURATION.md")),
        (
            "configuration schema reference",
            include_str!("../../../docs/reference/CONFIGURATION_SCHEMA.md"),
        ),
        ("example project config", include_str!("../../../.perl-lsp.toml.example")),
        (
            "fuzz corpus example project config",
            include_str!("../../../fuzz/corpus/config_surfaces/.perl-lsp.toml.example"),
        ),
        ("vscode extension contributions", include_str!("../../../vscode-extension/package.json")),
    ];
    let forbidden = ["nextEdit", "next_edit"];

    for (surface_name, surface) in surfaces {
        for marker in forbidden {
            assert!(
                !surface.contains(marker),
                "hidden next-edit setting marker {marker:?} reintroduced in {surface_name} (#8311)"
            );
        }
    }
}

#[test]
fn generic_settings_schema_is_server_native_and_namespaced() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let properties = &schema["properties"]["perl"]["properties"];
    assert_eq!(properties.get("testRunner"), None);
    // #8311: `nextEdit` names an editor provider surface with no registered
    // runtime owner, so it must not appear as a public settings section.
    assert_eq!(properties.get("nextEdit"), None);

    for section in [
        "workspace",
        "inlayHints",
        "limits",
        "telemetry",
        "perlcritic",
        "critic",
        "formatting",
        "aiCompletion",
    ] {
        assert!(properties.get(section).is_some(), "missing generic settings section {section}");
    }

    assert!(schema["properties"].get("perl-lsp").is_none());
    assert!(schema["properties"].get("serverPath").is_none());
    assert!(schema["properties"].get("autoDownload").is_none());

    Ok(())
}

#[test]
fn generic_formatter_schema_excludes_external_process_modes() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let engine = &schema["properties"]["perl"]["properties"]["formatting"]["properties"]["engine"];
    assert_eq!(engine["enum"], json!(["native", "compat", "off"]));
    Ok(())
}

#[test]
fn generic_schema_fields_are_behavior_backed_by_runtime_config() {
    let settings = json!({
        "workspace": {
            "includePaths": ["lib", "vendor/lib"],
            "discoveryExtensions": [".cgi"],
            "discoverySkippedDirs": ["generated"],
            "useSystemInc": true,
            "resolutionTimeout": 75,
            "usePerl5lib": false,
            "perl5libPrecedence": "append"
        },
        "inlayHints": {
            "enabled": false,
            "parameterHints": false,
            "typeHints": false,
            "chainedHints": true,
            "maxLength": 48
        },
        "limits": {
            "workspaceSymbolCap": 321,
            "referencesCap": 654,
            "completionCap": 87,
            "documentSymbolCap": 222,
            "codeLensCap": 111,
            "diagnosticsPerFileCap": 33,
            "inlayHintsCap": 44,
            "astCacheMaxEntries": 55,
            "astCacheTtlSecs": 66,
            "symbolCacheMaxEntries": 77,
            "maxIndexedFiles": 888,
            "maxTotalSymbols": 9999,
            "maxFileSizeBytes": 123456,
            "workspaceScanDeadlineMs": 4200,
            "referenceSearchDeadlineMs": 1300,
            "memoryWarningThresholdBytes": 1000,
            "memoryCriticalThresholdBytes": 2000,
            "astCacheMaxMemoryBytes": 3000
        },
        "telemetry": { "enabled": true },
        "critic": {
            "enabled": true,
            "severity": 4,
            "engine": "native",
            "profile": "strict",
            "include": [],
            "exclude": []
        },
        "formatting": {
            "enabled": true,
            "formatOnSave": false,
            "engine": "compat",
            "maximumLineLength": 100,
            "indentColumns": 2,
            "tabs": false,
            "openingBraceOnNewLine": true,
            "cuddledElse": false,
            "spaceAfterKeyword": false,
            "addTrailingCommas": true,
            "verticalAlignment": false,
            "blockCommentIndentation": 2,
            "timeoutSecs": 12
        },
        "aiCompletion": {
            "enabled": true,
            "provider": "openai_compat",
            "model": "fixture-model",
            "timeoutMs": 2200,
            "maxOutputTokens": 96,
            "rateLimitRps": 2.0,
            "maxInflight": 2,
            "fallback": false,
            "localModelMode": true,
            "streaming": {
                "enabled": false,
                "updateDebounceMs": 80
            }
        }
    });

    let mut server = ServerConfig::default();
    server.update_from_value(&settings);

    assert!(!server.inlay_hints_enabled);
    assert!(!server.inlay_hints_parameter_hints);
    assert!(!server.inlay_hints_type_hints);
    assert!(server.inlay_hints_chained_hints);
    assert_eq!(server.inlay_hints_max_length, 48);
    assert!(server.telemetry_enabled);
    assert_eq!(server.perlcritic_severity, 4);
    assert_eq!(server.native_critic_profile, "strict");
    assert!(!server.format_on_save);
    assert!(matches!(server.formatting_engine, FormatterMode::Compat));
    assert_eq!(server.perltidy_maximum_line_length, Some(100));
    assert_eq!(server.perltidy_indent_columns, Some(2));
    assert_eq!(server.perltidy_tabs, Some(false));
    assert_eq!(server.perltidy_timeout_secs, 12);
    // #4997: activation/selection fields from the generic schema are rejected;
    // compiled defaults survive. Envelope fields remain behavior-backed.
    assert!(!server.ai_completion.user_enabled);
    assert_eq!(
        server.ai_completion.activation_authority,
        perl_lsp_rs_core::config::AiActivationAuthority::Unavailable
    );
    assert_eq!(server.ai_completion.provider, "openai_compat");
    assert_eq!(server.ai_completion.model, "gpt-4o-mini");
    assert_eq!(server.ai_completion.timeout_ms, 2200);
    assert_eq!(server.ai_completion.max_output_tokens, 96);
    assert_eq!(server.ai_completion.max_inflight, 2);
    assert!(!server.ai_completion.fallback);
    assert!(server.ai_completion.local_model_mode);
    assert!(server.ai_completion.streaming.user_enabled);
    assert_eq!(server.ai_completion.streaming.update_debounce_ms, 80);

    let mut workspace = WorkspaceConfig::default();
    let rejected = workspace.update_from_value(&settings);
    assert!(rejected.is_empty());
    assert_eq!(workspace.include_paths, ["lib", "vendor/lib"]);
    assert_eq!(workspace.discovery_extra_extensions, [".cgi"]);
    assert_eq!(workspace.discovery_extra_skipped_dirs, ["generated"]);
    assert!(workspace.use_system_inc);
    assert_eq!(workspace.resolution_timeout_ms, 75);
    assert!(!workspace.use_perl5lib);
    assert!(matches!(workspace.perl5lib_precedence, Perl5LibPrecedence::Append));

    let mut limits = LspLimits::default();
    limits.update_from_value(&settings);
    assert_eq!(limits.workspace_symbol_cap, 321);
    assert_eq!(limits.references_cap, 654);
    assert_eq!(limits.completion_cap, 87);
    assert_eq!(limits.document_symbol_cap, 222);
    assert_eq!(limits.code_lens_cap, 111);
    assert_eq!(limits.diagnostics_per_file_cap, 33);
    assert_eq!(limits.inlay_hints_cap, 44);
    assert_eq!(limits.ast_cache_max_entries, 55);
    assert_eq!(limits.ast_cache_ttl_secs, 66);
    assert_eq!(limits.symbol_cache_max_entries, 77);
    assert_eq!(limits.max_indexed_files, 888);
    assert_eq!(limits.max_total_symbols, 9999);
    assert_eq!(limits.max_file_size_bytes, 123456);
    assert_eq!(limits.workspace_scan_deadline, Duration::from_millis(4200));
    assert_eq!(limits.reference_search_deadline, Duration::from_millis(1300));
    assert_eq!(limits.memory_budget.warning_threshold_bytes, 1000);
    assert_eq!(limits.memory_budget.critical_threshold_bytes, 2000);
    assert_eq!(limits.memory_budget.ast_cache_max_bytes, 3000);
}

#[test]
fn generic_schema_excludes_security_sensitive_lsp_settings() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let perl = &schema["properties"]["perl"]["properties"];

    let workspace = &perl["workspace"]["properties"];
    assert!(workspace.get("perlPath").is_none());
    assert!(workspace.get("perlArgs").is_none());

    let formatting = &perl["formatting"]["properties"];
    assert!(formatting.get("profile").is_none());
    assert!(formatting.get("extraArgs").is_none());

    let perlcritic = &perl["perlcritic"]["properties"];
    assert!(perlcritic.get("profile").is_none());
    assert!(perlcritic.get("theme").is_none());

    let ai = &perl["aiCompletion"]["properties"];
    assert!(ai.get("endpoint").is_none());
    assert!(ai.get("apiKeyEnv").is_none());
    assert!(ai.get("apiKeyHeader").is_none());
    assert!(ai.get("apiKeyPrefix").is_none());

    // #4997: activation and selection fields remain documented for the future
    // trusted adapter but advertise no generic client transport.
    for activation_field in ["enabled", "provider", "model"] {
        let field = must_some_with(
            ai.get(activation_field),
            format_args!("aiCompletion.{activation_field} must stay documented"),
        );
        assert_eq!(
            field["x-perllsp-transports"],
            json!([]),
            "aiCompletion.{activation_field} must not advertise client transports (#4997)",
        );
        assert_eq!(field["x-perllsp-scope"], json!("machine"));
    }
    let streaming_enabled =
        &perl["aiCompletion"]["properties"]["streaming"]["properties"]["enabled"];
    assert_eq!(
        streaming_enabled["x-perllsp-transports"],
        json!([]),
        "streaming.enabled must not advertise client transports (#4997)",
    );

    Ok(())
}
