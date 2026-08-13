use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const CANONICAL_SCHEMA: &str = include_str!("../../../schemas/perllsp-settings.schema.json");
const LSP4IJ_SCHEMA: &str =
    include_str!("../../../integrations/lsp4ij/perl-lsp/settings.schema.json");
const LSP4IJ_SETTINGS: &str = include_str!("../../../integrations/lsp4ij/perl-lsp/settings.json");
const LSP4IJ_INIT_OPTIONS: &str =
    include_str!("../../../integrations/lsp4ij/perl-lsp/initializationOptions.json");
const LSP4IJ_TEMPLATE: &str = include_str!("../../../integrations/lsp4ij/perl-lsp/template.json");

fn parse(input: &str) -> Value {
    serde_json::from_str(input).expect("checked JSON fixture must parse")
}

fn canonical_property<'a>(schema: &'a Value, dotted_key: &str) -> Option<&'a Value> {
    let mut segments = dotted_key.split('.');
    if segments.next()? != "perl" {
        return None;
    }

    let remaining: Vec<_> = segments.collect();
    let mut properties = schema.pointer("/properties/perl/properties")?;
    for (index, segment) in remaining.iter().enumerate() {
        let property = properties.get(*segment)?;
        if index + 1 == remaining.len() {
            return Some(property);
        }
        properties = property.get("properties")?;
    }
    None
}

fn expand_one_dotted_key(key: &str, value: Value) -> Value {
    let mut nested = value;
    for segment in key.split('.').collect::<Vec<_>>().into_iter().rev() {
        let mut object = Map::new();
        object.insert(segment.to_string(), nested);
        nested = Value::Object(object);
    }
    nested
}

#[test]
fn lsp4ij_projection_is_a_checked_subset_of_the_generic_perl_schema() {
    let canonical = parse(CANONICAL_SCHEMA);
    let projection = parse(LSP4IJ_SCHEMA);
    let properties = projection
        .get("properties")
        .and_then(Value::as_object)
        .expect("LSP4IJ schema must expose flat dotted properties");

    assert!(!properties.is_empty(), "projection must expose at least one useful setting");

    for (key, projected_property) in properties {
        assert!(key.starts_with("perl."), "LSP4IJ server setting must use perl.*: {key}");
        assert!(
            !key.starts_with("perl-lsp."),
            "VS Code extension namespace must never become the generic LSP4IJ wire schema: {key}"
        );

        let canonical_property = canonical_property(&canonical, key)
            .unwrap_or_else(|| panic!("projected field is absent from canonical generic schema: {key}"));

        for facet in ["type", "default", "enum", "minimum", "maximum", "exclusiveMinimum"] {
            assert_eq!(
                projected_property.get(facet),
                canonical_property.get(facet),
                "LSP4IJ projection drifted from canonical {facet} for {key}"
            );
        }
    }
}

#[test]
fn lsp4ij_projection_excludes_non_server_and_unproven_controls() {
    let projection = parse(LSP4IJ_SCHEMA);
    let properties = projection
        .get("properties")
        .and_then(Value::as_object)
        .expect("projection properties");

    let forbidden_suffixes = [
        "serverPath",
        "autoDownload",
        "channel",
        "versionTag",
        "downloadBaseUrl",
        "linuxLibc",
        "updateCheckInterval",
        "autoUpdate",
        "enableTestIntegration",
        "autoPopulateNewFiles",
        "mcp.servers",
        "trace.server",
        "disabledFeatures",
        "formatOnSave",
        "perlcritic.theme",
    ];

    for key in properties.keys() {
        assert!(
            forbidden_suffixes.iter().all(|forbidden| !key.contains(forbidden)),
            "non-server, initialization-only, deprecated, or unproven control leaked into LSP4IJ settings: {key}"
        );
    }
}

#[test]
fn lsp4ij_default_settings_do_not_manufacture_editor_overrides() {
    let settings = parse(LSP4IJ_SETTINGS);
    assert_eq!(settings, json!({}), "settings.json must remain semantically empty by default");

    let initialization_options = parse(LSP4IJ_INIT_OPTIONS);
    assert_eq!(
        initialization_options,
        json!({}),
        "initialization-only controls must be explicit rather than copied from live settings defaults"
    );
}

#[test]
fn lsp4ij_dotted_configuration_expands_to_the_server_native_perl_wire_shape() {
    let expanded = expand_one_dotted_key(
        "perl.workspace.includePaths",
        json!(["vendor/lib"]),
    );
    assert_eq!(
        expanded,
        json!({
            "perl": {
                "workspace": {
                    "includePaths": ["vendor/lib"]
                }
            }
        })
    );
}

#[test]
fn lsp4ij_template_is_bounded_to_proven_perl_files_and_canonical_stdio_identity() {
    let template = parse(LSP4IJ_TEMPLATE);
    assert_eq!(template.get("expandConfiguration"), Some(&json!(true)));

    let program_args = template
        .get("programArgs")
        .and_then(Value::as_object)
        .expect("template programArgs");
    assert!(
        program_args.values().all(|value| {
            value.as_str().is_some_and(|command| command.contains("perllsp") && command.contains("--stdio"))
        }),
        "every platform command must retain canonical perllsp --stdio identity"
    );

    let patterns: BTreeSet<_> = template
        .pointer("/fileTypeMappings/0/fileType/patterns")
        .and_then(Value::as_array)
        .expect("Perl file patterns")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(patterns, BTreeSet::from(["*.pl", "*.pm", "*.t"]));
}
