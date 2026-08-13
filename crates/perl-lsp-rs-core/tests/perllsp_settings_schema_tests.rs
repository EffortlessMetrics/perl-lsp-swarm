use perl_lsp_rs_core::config::{
    FormatterMode, Perl5LibPrecedence, ServerConfig, WorkspaceConfig,
};
use serde_json::{Value, json};
use std::error::Error;

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

    for section in [
        "workspace",
        "inlayHints",
        "testRunner",
        "telemetry",
        "nextEdit",
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
        "testRunner": {
            "enabled": false,
            "command": "prove",
            "args": ["-lr", "t"],
            "timeout": 90000
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

    assert!(!server.inlay_hints_enabled);
    assert!(!server.inlay_hints_parameter_hints);
    assert!(!server.inlay_hints_type_hints);
    assert!(server.inlay_hints_chained_hints);
    assert_eq!(server.inlay_hints_max_length, 48);
    assert!(!server.test_runner_enabled);
    assert_eq!(server.test_runner_command, "prove");
    assert_eq!(server.test_runner_args, ["-lr", "t"]);
    assert_eq!(server.test_runner_timeout, 90000);
    assert!(server.telemetry_enabled);
    assert!(server.next_edit.enabled);
    assert_eq!(server.perlcritic_severity, 4);
    assert_eq!(server.native_critic_profile, "strict");
    assert!(!server.format_on_save);
    assert!(matches!(server.formatting_engine, FormatterMode::Compat));
    assert_eq!(server.perltidy_maximum_line_length, Some(100));
    assert_eq!(server.perltidy_indent_columns, Some(2));
    assert_eq!(server.perltidy_tabs, Some(false));
    assert_eq!(server.perltidy_timeout_secs, 12);
    assert!(server.ai_completion.user_enabled);
    assert_eq!(server.ai_completion.model, "fixture-model");
    assert_eq!(server.ai_completion.timeout_ms, 2200);
    assert_eq!(server.ai_completion.max_output_tokens, 96);
    assert_eq!(server.ai_completion.max_inflight, 2);
    assert!(!server.ai_completion.fallback);
    assert!(server.ai_completion.local_model_mode);
    assert!(!server.ai_completion.streaming.user_enabled);
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

    Ok(())
}
