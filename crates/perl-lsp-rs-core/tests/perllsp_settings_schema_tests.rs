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
        (
            "configuration schema reference",
            include_str!("../../../docs/reference/CONFIGURATION_SCHEMA.md"),
        ),
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
    // `compat` is deliberately absent (#7129): it was a bare alias for the
    // native formatter, producing byte-identical output, so the public
    // settings contract must not offer it as an engine to choose. The server
    // rejects the token outright (#15624 closed the deprecation window), so
    // the schema and the parser now agree: only `native` and `off` are
    // engines on this channel.
    assert_eq!(engine["enum"], json!(["native", "off"]));
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
            "maxFileSizeBytes": 123456,
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
            "engine": "off",
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
    // A non-default, schema-valid engine, so the assertion proves the field is
    // actually read rather than matching the compiled default.
    assert!(matches!(server.formatting_engine, FormatterMode::Off));
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
    assert_eq!(limits.max_file_size_bytes, 123456);
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

/// The published schema must advertise the same `maxInflight` bounds the
/// runtime actually enforces (`#8300`).
///
/// The schema previously declared only `minimum: 1`, so a client could send
/// `maxInflight: 128`, pass schema validation, and have the value silently
/// discarded by `update_from_value` — validated configuration that does
/// nothing is worse than configuration rejected up front. This pins both
/// ends: the schema advertises `1..=64`, and the runtime agrees at each
/// boundary.
#[test]
fn ai_max_inflight_schema_bounds_match_the_runtime_contract() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let max_inflight =
        &schema["properties"]["perl"]["properties"]["aiCompletion"]["properties"]["maxInflight"];

    if max_inflight["minimum"] != json!(1) {
        return Err(std::io::Error::other("schema minimum must be 1").into());
    }
    if max_inflight["maximum"] != json!(64) {
        return Err(std::io::Error::other("schema maximum must be 64").into());
    }

    // The runtime honours exactly the range the schema publishes: both
    // boundaries are accepted, and the first value past each is not.
    for (value, expected) in [(1_u64, 1_u32), (64, 64)] {
        let mut config = ServerConfig::default();
        config.update_from_value(&json!({ "aiCompletion": { "maxInflight": value } }));
        if config.ai_completion.max_inflight != expected {
            return Err(std::io::Error::other(format!(
                "maxInflight={value} is inside the published range and must be accepted"
            ))
            .into());
        }
    }

    for rejected in [0_u64, 65] {
        let mut config = ServerConfig::default();
        config.update_from_value(&json!({ "aiCompletion": { "maxInflight": 8 } }));
        config.update_from_value(&json!({ "aiCompletion": { "maxInflight": rejected } }));
        if config.ai_completion.max_inflight != 8 {
            return Err(std::io::Error::other(format!(
                "maxInflight={rejected} is outside the published range and must keep the previous value"
            )).into());
        }
    }

    Ok(())
}

/// Returns the index of the `}` that closes the `{` at `open`, ignoring
/// braces that appear inside JSON string literals.
fn matching_json_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in text.as_bytes().iter().enumerate().skip(open) {
        let character = char::from(*byte);
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Collects every `perl.workspace.*` key a guide names, in both shapes that
/// operator guidance actually uses: dotted prose (`perl.workspace.foo`) and
/// a nested JSON example (`{ "workspace": { "foo": [] } }`).
///
/// The retired `excludePatterns` advice only ever appeared in the nested JSON
/// shape, so a dotted-only scan reports a clean tree while the defect is still
/// on the page.
fn workspace_keys_named_by(guide: &str, guide_name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut keys = Vec::new();
    let dotted = regex::Regex::new(r"perl\.workspace\.([A-Za-z][A-Za-z0-9]*)")
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    for capture in dotted.captures_iter(guide) {
        keys.push(capture.get(1).map_or("", |group| group.as_str()).to_owned());
    }

    let mut cursor = 0_usize;
    while let Some(offset) = guide[cursor..].find("\"workspace\"") {
        let after_key = cursor + offset + "\"workspace\"".len();
        let Some(open) = guide[after_key..].find('{').map(|index| after_key + index) else {
            // A prose mention with no example body: skip it and keep scanning
            // rather than abandoning the rest of the document.
            cursor = after_key;
            continue;
        };
        // An unbalanced brace must fail loudly. Stopping the scan here would
        // silently under-report the remaining examples, which is the same
        // vacuous-guard failure this test exists to prevent.
        let Some(close) = matching_json_brace(guide, open) else {
            return Err(std::io::Error::other(format!(
                "{guide_name} contains a `\"workspace\"` example whose braces are unbalanced"
            ))
            .into());
        };
        let example = &guide[open..=close];
        let value: Value = serde_json::from_str(example).map_err(|error| {
            std::io::Error::other(format!(
                "{guide_name} contains a `\"workspace\"` example that is not valid JSON: {error}"
            ))
        })?;
        if let Value::Object(fields) = value {
            keys.extend(fields.keys().cloned());
        }
        cursor = close;
    }

    keys.sort();
    keys.dedup();
    Ok(keys)
}

/// #16946: operator guidance may only name `perl.workspace.*` settings that
/// are actually configurable.
///
/// The large-workspace troubleshooting guide told users to narrow a slow
/// workspace with `perl.workspace.excludePatterns`. No reader ever existed
/// for that key, so the remedy was impossible to apply, and the directory
/// names it listed are already skipped unconditionally. The public settings
/// schema is the authority for which `perl.workspace.*` keys a client can
/// set, so every such key named in the large-workspace guides has to resolve
/// there.
///
/// This is a documentation seam, not a runtime one: the guides are compiled
/// into this test through `include_str!`, so the check fails on the merged
/// tree whenever guidance drifts ahead of the configuration surface.
#[test]
fn large_workspace_guidance_names_only_real_workspace_settings() -> Result<(), Box<dyn Error>> {
    const GUIDES: &[(&str, &str)] = &[
        (
            "large-workspaces troubleshooting guide",
            include_str!("../../../docs/large-workspaces/TROUBLESHOOTING.md"),
        ),
        ("large-workspaces readme", include_str!("../../../docs/large-workspaces/README.md")),
        (
            "large-workspaces profiling guide",
            include_str!("../../../docs/large-workspaces/PROFILING_GUIDE.md"),
        ),
        (
            "large-workspaces testing guide",
            include_str!("../../../docs/large-workspaces/TESTING_GUIDE.md"),
        ),
        (
            "large-workspaces memory patterns",
            include_str!("../../../docs/large-workspaces/MEMORY_PATTERNS.md"),
        ),
        (
            "large-workspaces memory control closeout",
            include_str!("../../../docs/large-workspaces/MEMORY_CONTROL_CLOSEOUT.md"),
        ),
        (
            "large-workspaces retained state inventory",
            include_str!("../../../docs/large-workspaces/RETAINED_STATE_INVENTORY.md"),
        ),
        (
            "large-workspaces churn repro",
            include_str!("../../../docs/large-workspaces/LSP_CHURN_REPRO.md"),
        ),
    ];

    let schema = load_schema()?;
    let workspace_keys = schema["properties"]["perl"]["properties"]["workspace"]["properties"]
        .as_object()
        .ok_or("settings schema has no perl.properties.workspace.properties object")?;

    for (guide_name, guide) in GUIDES {
        for key in workspace_keys_named_by(guide, guide_name)? {
            assert!(
                workspace_keys.contains_key(&key),
                "{guide_name} names workspace setting `{key}`, which is not configurable in \
                 schemas/perllsp-settings.schema.json (#16946): guidance must not recommend a \
                 setting no client can set"
            );
        }
    }

    Ok(())
}

/// #16946: the replacement advice must name the setting the server actually
/// reads, and the configuration reference must document that same key.
///
/// The guide now points slow-startup triage at `discoverySkippedDirs`. That
/// only helps if the key is published in the schema *and* described in the
/// canonical reference, so this pins all three surfaces together: schema,
/// reference prose, and the troubleshooting guide.
#[test]
fn discovery_skipped_dirs_guidance_is_published_on_every_configuration_surface()
-> Result<(), Box<dyn Error>> {
    const SKIPPED_DIRS: &str = "discoverySkippedDirs";
    const SURFACES: &[(&str, &str)] = &[
        ("settings schema", include_str!("../../../schemas/perllsp-settings.schema.json")),
        ("configuration reference", include_str!("../../../docs/reference/CONFIG.md")),
        (
            "large-workspaces troubleshooting guide",
            include_str!("../../../docs/large-workspaces/TROUBLESHOOTING.md"),
        ),
    ];

    for (surface_name, surface) in SURFACES {
        assert!(
            surface.contains(SKIPPED_DIRS),
            "the documented large-workspace skip control is absent from {surface_name}; the \
             troubleshooting guide, the schema, and the reference must agree (#16946)"
        );
    }

    // The reference must describe directory-name semantics rather than the
    // pattern language the retired `excludePatterns` example implied.
    let reference = SURFACES
        .iter()
        .find(|(name, _)| *name == "configuration reference")
        .map_or("", |(_, body)| *body);
    assert!(
        reference.contains("not globs"),
        "the configuration reference must state that discoverySkippedDirs takes exact \
         directory names, not globs (#16946)"
    );
    assert!(
        reference.contains("#### `perl.workspace.discoverySkippedDirs`"),
        "the configuration reference must have a discoverySkippedDirs setting section (#16946)"
    );
    assert!(
        !reference.contains("excludePatterns"),
        "the retired perl.workspace.excludePatterns advice must not return to the \
         configuration reference (#16946)"
    );

    let schema_reference = include_str!("../../../docs/reference/CONFIGURATION_SCHEMA.md");
    let schema_section = schema_reference
        .split_once("## JSON Schema")
        .ok_or("configuration schema reference has no JSON Schema section")?
        .1;
    let embedded_json = schema_section
        .split_once("```json")
        .ok_or("configuration schema reference has no embedded JSON schema")?
        .1
        .split_once("```")
        .ok_or("configuration schema reference has an unterminated JSON schema")?
        .0;
    let embedded: Value = serde_json::from_str(embedded_json)?;
    let published = load_schema()?;
    let embedded_key = &embedded["definitions"]["workspace"]["properties"][SKIPPED_DIRS];
    let published_key =
        &published["properties"]["perl"]["properties"]["workspace"]["properties"][SKIPPED_DIRS];
    assert_eq!(embedded_key["type"], published_key["type"]);
    assert_eq!(embedded_key["default"], published_key["default"]);

    Ok(())
}
