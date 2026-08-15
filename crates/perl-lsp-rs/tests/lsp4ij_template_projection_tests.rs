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

/// The part of one platform command that actually reaches `perllsp`.
///
/// LSP4IJ tokenizes the command line itself before spawning, so an unquoted
/// `sh -c ${BASE_DIR}/perllsp --stdio` hands the shell `${BASE_DIR}/perllsp` as
/// the whole command string and turns `--stdio` into the shell's `$0`, where it
/// never reaches the server. Only the quoted `sh -c "..."` payload survives.
fn server_invocation(command: &str) -> &str {
    let Some(rest) = command.strip_prefix("sh -c ") else {
        return command;
    };

    rest.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')).unwrap_or_else(|| {
        panic!(
            "`sh -c` command must quote the whole server invocation so every argument survives \
             shell tokenization: {command}"
        )
    })
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
        // Checking the first dotted segment rather than a `perl.` prefix keeps this a real
        // negative control: `perl-lsp.critic.enabled` fails here on its own terms rather than
        // as a side effect of prefix matching.
        let namespace = key.split('.').next().unwrap_or_default();
        assert_eq!(
            namespace, "perl",
            "generic LSP4IJ wire settings must use the server-native perl namespace, never the \
             VS Code extension namespace: {key}"
        );
        assert!(key.contains('.'), "LSP4IJ server setting must be a dotted perl.* key: {key}");

        let canonical_property = canonical_property(&canonical, key).unwrap_or_else(|| {
            panic!("projected field is absent from canonical generic schema: {key}")
        });

        for facet in ["type", "items", "default", "enum", "minimum", "maximum", "exclusiveMinimum"]
        {
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
    let properties =
        projection.get("properties").and_then(Value::as_object).expect("projection properties");

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

/// A name blocklist only rejects the controls someone already thought to name. The canonical
/// schema carries the server's own scope and transport authority, so a field the server refuses
/// on the resource-scoped workspace channel — `perl.workspace.externalIncludePaths` is the
/// current one — must not become a per-project LSP4IJ setting even though its facets would
/// otherwise project cleanly.
#[test]
fn lsp4ij_projection_never_gains_authority_the_server_denies() {
    let canonical = parse(CANONICAL_SCHEMA);
    let projection = parse(LSP4IJ_SCHEMA);
    let properties =
        projection.get("properties").and_then(Value::as_object).expect("projection properties");

    for key in properties.keys() {
        let canonical_property = canonical_property(&canonical, key)
            .unwrap_or_else(|| panic!("projected field is absent from canonical schema: {key}"));

        if let Some(scope) = canonical_property.get("x-perllsp-scope").and_then(Value::as_str) {
            assert_eq!(
                scope, "resource",
                "LSP4IJ exposes per-project settings, so a {scope}-scoped server field cannot \
                 become project-controlled configuration: {key}"
            );
        }

        if let Some(transports) =
            canonical_property.get("x-perllsp-transports").and_then(Value::as_array)
        {
            assert!(
                transports.iter().any(|transport| transport.as_str()
                    == Some("workspace/didChangeConfiguration")),
                "projected field is not accepted on the live workspace-configuration channel \
                 LSP4IJ settings use: {key}"
            );
        }
    }

    // The negative control must be reachable: if the canonical schema ever stops marking
    // authority, this test would silently pass on everything.
    assert!(
        canonical_property(&canonical, "perl.workspace.externalIncludePaths")
            .and_then(|property| property.get("x-perllsp-scope"))
            .and_then(Value::as_str)
            == Some("machine"),
        "canonical schema must still mark a denied authority for this control to discriminate"
    );
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
    let expanded = expand_one_dotted_key("perl.workspace.includePaths", json!(["vendor/lib"]));
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

    let program_args =
        template.get("programArgs").and_then(Value::as_object).expect("template programArgs");
    assert!(!program_args.is_empty(), "template must declare at least one platform command");

    for (platform, value) in program_args {
        let command = value
            .as_str()
            .unwrap_or_else(|| panic!("{platform} command must be a string: {value}"));
        let invocation = server_invocation(command);

        assert!(
            invocation.contains("perllsp"),
            "{platform} command must launch the canonical perllsp binary: {command}"
        );
        assert!(
            invocation.contains("--stdio"),
            "{platform} command must pass --stdio to perllsp rather than to the shell: {command}"
        );
    }

    let patterns: BTreeSet<_> = template
        .pointer("/fileTypeMappings/0/fileType/patterns")
        .and_then(Value::as_array)
        .expect("Perl file patterns")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(patterns, BTreeSet::from(["*.pl", "*.pm", "*.t"]));
}
