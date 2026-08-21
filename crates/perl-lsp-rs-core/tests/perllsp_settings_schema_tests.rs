use perl_lsp_rs_core::config::{FormatterMode, Perl5LibPrecedence, ServerConfig, WorkspaceConfig};
use perl_lsp_rs_core::runtime::LspLimits;
use serde_json::{Value, json};
use std::{error::Error, time::Duration};

fn load_schema() -> Result<Value, Box<dyn Error>> {
    match serde_json::from_str(include_str!("../../../schemas/perllsp-settings.schema.json")) {
        Ok(value) => Ok(value),
        Err(error) => Err(Box::new(error)),
    }
}

#[test]
fn generic_settings_schema_is_server_native_and_namespaced() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let properties = &schema["properties"]["perl"]["properties"];
    assert_eq!(properties.get("testRunner"), None);

    for section in [
        "workspace",
        "inlayHints",
        "limits",
        "telemetry",
        "nextEdit",
        "perlcritic",
        "critic",
        "formatting",
        "aiCompletion",
    ] {
        assert_eq!(
            properties.get(section).is_some(),
            true,
            "missing generic settings section {section}"
        );
    }

    assert_eq!(schema["properties"].get("perl-lsp").is_none(), true);
    assert_eq!(schema["properties"].get("serverPath").is_none(), true);
    assert_eq!(schema["properties"].get("autoDownload").is_none(), true);

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
        "nextEdit": { "enabled": true },
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

    assert_eq!(server.inlay_hints_enabled, false);
    assert_eq!(server.inlay_hints_parameter_hints, false);
    assert_eq!(server.inlay_hints_type_hints, false);
    assert_eq!(server.inlay_hints_chained_hints, true);
    assert_eq!(server.inlay_hints_max_length, 48);
    assert_eq!(server.telemetry_enabled, true);
    assert_eq!(server.next_edit.enabled, true);
    assert_eq!(server.perlcritic_severity, 4);
    assert_eq!(server.native_critic_profile, "strict");
    assert_eq!(server.format_on_save, false);
    assert_eq!(matches!(server.formatting_engine, FormatterMode::Compat), true);
    assert_eq!(server.perltidy_maximum_line_length, Some(100));
    assert_eq!(server.perltidy_indent_columns, Some(2));
    assert_eq!(server.perltidy_tabs, Some(false));
    assert_eq!(server.perltidy_timeout_secs, 12);
    assert_eq!(server.ai_completion.user_enabled, true);
    assert_eq!(server.ai_completion.model, "fixture-model");
    assert_eq!(server.ai_completion.timeout_ms, 2200);
    assert_eq!(server.ai_completion.max_output_tokens, 96);
    assert_eq!(server.ai_completion.max_inflight, 2);
    assert_eq!(server.ai_completion.fallback, false);
    assert_eq!(server.ai_completion.local_model_mode, true);
    assert_eq!(server.ai_completion.streaming.user_enabled, false);
    assert_eq!(server.ai_completion.streaming.update_debounce_ms, 80);

    let mut workspace = WorkspaceConfig::default();
    let rejected = workspace.update_from_value(&settings);
    assert_eq!(rejected.is_empty(), true);
    assert_eq!(workspace.include_paths, ["lib", "vendor/lib"]);
    assert_eq!(workspace.discovery_extra_extensions, [".cgi"]);
    assert_eq!(workspace.discovery_extra_skipped_dirs, ["generated"]);
    assert_eq!(workspace.use_system_inc, true);
    assert_eq!(workspace.resolution_timeout_ms, 75);
    assert_eq!(workspace.use_perl5lib, false);
    assert_eq!(matches!(workspace.perl5lib_precedence, Perl5LibPrecedence::Append), true);

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
    assert_eq!(workspace.get("perlPath").is_none(), true);
    assert_eq!(workspace.get("perlArgs").is_none(), true);

    let formatting = &perl["formatting"]["properties"];
    assert_eq!(formatting.get("profile").is_none(), true);
    assert_eq!(formatting.get("extraArgs").is_none(), true);

    let perlcritic = &perl["perlcritic"]["properties"];
    assert_eq!(perlcritic.get("profile").is_none(), true);
    assert_eq!(perlcritic.get("theme").is_none(), true);

    let ai = &perl["aiCompletion"]["properties"];
    assert_eq!(ai.get("endpoint").is_none(), true);
    assert_eq!(ai.get("apiKeyEnv").is_none(), true);
    assert_eq!(ai.get("apiKeyHeader").is_none(), true);
    assert_eq!(ai.get("apiKeyPrefix").is_none(), true);

    Ok(())
}
