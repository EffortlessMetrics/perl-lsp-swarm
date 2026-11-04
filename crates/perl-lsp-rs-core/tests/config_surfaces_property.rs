//! Property tests for configuration parsing/application (#5012 item 5).
//!
//! These mirror the totality and clamp invariants exercised by
//! `fuzz/fuzz_targets/config_surfaces.rs` so startup config paths stay panic-free
//! under adversarial TOML/JSON without relying on nightly fuzz alone.

use perl_lsp_rs_core::config::{ProjectConfig, ServerConfig, WorkspaceConfig};
use proptest::prelude::*;
use serde_json::json;

const MAX_FRAGMENT_CHARS: usize = 160;

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn toml_basic_string(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars().take(MAX_FRAGMENT_CHARS) {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push('_'),
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn json_string(input: &str) -> serde_json::Value {
    serde_json::Value::String(truncate_chars(input, MAX_FRAGMENT_CHARS))
}

fn exercise_project_config_toml(toml_source: &str) -> Result<(), TestCaseError> {
    if let Ok(project) = toml::from_str::<ProjectConfig>(toml_source) {
        let mut config = ServerConfig::default();
        project.apply_to_server_config(&mut config);
        project.apply_to_server_config(&mut config);
        prop_assert!(
            (1..=5).contains(&config.perlcritic_severity),
            "severity must stay clamped to 1..=5 after apply; got {}",
            config.perlcritic_severity
        );
    }
    Ok(())
}

fn exercise_lsp_settings(settings: serde_json::Value) -> Result<(), TestCaseError> {
    let mut config = ServerConfig::default();
    config.update_from_value(&settings);
    config.update_from_value(&settings);
    prop_assert!(
        (1..=5).contains(&config.perlcritic_severity),
        "severity must stay clamped to 1..=5 after JSON apply; got {}",
        config.perlcritic_severity
    );
    Ok(())
}

fn fuzz_fragment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), "\\PC*".prop_map(|s| truncate_chars(&s, MAX_FRAGMENT_CHARS)),]
}

fn severity_input_strategy() -> impl Strategy<Value = u64> {
    prop_oneof![Just(0), Just(1), Just(5), Just(6), Just(99), Just(u64::MAX), 0u64..=256,]
}

proptest! {
    #[test]
    fn prop_project_config_toml_apply_is_total_and_clamps_severity(fragment in fuzz_fragment_strategy()) {
        let escaped = toml_basic_string(&fragment);
        exercise_project_config_toml(&fragment)?;

        let variants = [
            format!(
                r#"
[perl]
include_paths = ["{escaped}", "lib", "."]
version = "{escaped}"
use_perl5lib = true
perl5lib_precedence = "prepend"

[diagnostics]
perlcritic = true
perlcritic_severity = 99

[features]
inlay_hints = false
"#
            ),
            format!(
                r#"
[formatting]
enabled = true
engine = "{escaped}"
perltidy_profile = "{escaped}"
perltidy_maximum_line_length = 4294967295
perltidy_indent_columns = 4294967295
perltidy_tabs = false
perltidy_extra_args = ["{escaped}", "--standard-output"]

[critic]
engine = "{escaped}"
profile = "{escaped}"
include = ["{escaped}", "Variables::ProhibitUnusedVariables"]
exclude = ["", "{escaped}"]
"#
            ),
            format!(
                r#"
[ai_completion]
enabled = true
provider = "{escaped}"
endpoint = "{escaped}"
model = "{escaped}"
api_key_env = "{escaped}"
"#
            ),
        ];

        for source in &variants {
            exercise_project_config_toml(source)?;
        }
    }

    #[test]
    fn prop_server_config_json_apply_is_total_and_clamps_severity(
        fragment in fuzz_fragment_strategy(),
        severity in severity_input_strategy(),
    ) {
        let settings = json!({
            "inlayHints": {
                "enabled": true,
                "parameterHints": false,
                "typeHints": true,
                "chainedHints": false,
                "maxLength": u64::MAX,
            },
            "testRunner": {
                "enabled": true,
                "command": fragment,
                "args": [fragment, 7, null, true],
                "timeout": u64::MAX,
            },
            "telemetry": { "enabled": true },
            "perlcritic": {
                "enabled": false,
                "severity": severity,
                "profile": fragment,
                "theme": fragment,
            },
            "critic": {
                "engine": "native",
                "profile": "recommended",
                "include": ["Native::Rule"],
                "exclude": ["native.common.assignment_in_condition"],
            },
            "formatting": {
                "enabled": true,
                "engine": fragment,
                "profile": fragment,
                "maximumLineLength": u64::MAX,
                "indentColumns": u64::MAX,
                "tabs": true,
                "openingBraceOnNewLine": false,
                "cuddledElse": true,
                "spaceAfterKeyword": false,
                "addTrailingCommas": true,
                "verticalAlignment": false,
                "blockCommentIndentation": u64::MAX,
                "extraArgs": [fragment, 1, false],
                "timeoutSecs": u64::MAX,
            },
            "aiCompletion": {
                "enabled": true,
                "provider": fragment,
                "endpoint": fragment,
                "model": fragment,
                "apiKeyEnv": fragment,
                "timeoutMs": u64::MAX,
                "maxOutputTokens": u64::MAX,
                "rateLimitRps": f64::MAX,
                "maxInflight": u64::MAX,
                "fallback": false,
                "streaming": {
                    "enabled": true,
                    "updateDebounceMs": u64::MAX,
                },
            },
        });
        exercise_lsp_settings(settings)?;

        exercise_lsp_settings(json!({
            "inlayHints": json_string(&fragment),
            "testRunner": [json_string(&fragment)],
            "telemetry": null,
            "perlcritic": { "severity": severity },
            "formatting": { "extraArgs": [json_string(&fragment)] },
        }))?;
    }

    #[test]
    fn prop_project_config_toml_clamps_generated_severity(severity in severity_input_strategy()) {
        let source = format!(
            r#"
[diagnostics]
perlcritic = true
perlcritic_severity = {severity}
"#
        );
        exercise_project_config_toml(&source)?;
    }

    #[test]
    fn prop_workspace_config_parse_perl5lib_is_total(fragment in fuzz_fragment_strategy()) {
        let _ = WorkspaceConfig::parse_perl5lib(&fragment);
        let _ = WorkspaceConfig::parse_perl5lib(&format!("{fragment}:{fragment};{fragment}"));
    }
}
