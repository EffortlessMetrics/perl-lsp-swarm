use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MARKETPLACE_PATH: &str = ".claude-plugin/marketplace.json";
const PLUGIN_ROOT: &str = "integrations/claude-code/plugins/perl-lsp";
const EXPECTED_EXTENSIONS: &[&str] = &[".PL", ".cgi", ".fcgi", ".pl", ".pm", ".psgi", ".t"];

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live below the repository root".into())
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, Box<dyn Error>> {
    value.as_object().ok_or_else(|| format!("{name} must be a JSON object").into())
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, Box<dyn Error>> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field `{field}`").into())
}

#[test]
fn claude_marketplace_points_at_the_single_plugin_package() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let marketplace = read_json(&root.join(MARKETPLACE_PATH))?;
    let marketplace = object(&marketplace, "marketplace")?;

    assert_eq!(string_field(marketplace, "name")?, "effortlessmetrics");
    let plugins = marketplace
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or("marketplace plugins must be an array")?;
    assert_eq!(plugins.len(), 1, "marketplace must publish one Perl plugin");

    let entry = object(&plugins[0], "marketplace plugin entry")?;
    assert_eq!(string_field(entry, "name")?, "perl-lsp");
    assert_eq!(string_field(entry, "source")?, format!("./{PLUGIN_ROOT}"));
    assert!(entry.get("version").is_none(), "plugin version must be single-sourced in plugin.json");
    assert!(
        !string_field(entry, "source")?.contains(".."),
        "plugin source must not escape the marketplace root"
    );
    Ok(())
}

#[test]
fn claude_plugin_manifest_owns_identity_and_lsp_component() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?.join(PLUGIN_ROOT);
    let manifest = read_json(&root.join(".claude-plugin/plugin.json"))?;
    let manifest = object(&manifest, "plugin manifest")?;

    assert_eq!(string_field(manifest, "name")?, "perl-lsp");
    assert_eq!(string_field(manifest, "version")?, "0.1.0");
    assert_eq!(string_field(manifest, "lspServers")?, "./.lsp.json");
    assert_eq!(
        string_field(manifest, "repository")?,
        "https://github.com/EffortlessMetrics/perl-lsp"
    );
    assert!(manifest.get("mcpServers").is_none(), "Claude package must not add MCP");

    let version = string_field(manifest, "version")?;
    let parts = version.split('.').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3, "plugin version must be semantic x.y.z");
    assert!(parts.iter().all(|part| part.parse::<u64>().is_ok()));
    Ok(())
}

#[test]
fn claude_lsp_launch_contract_is_exact_and_bounded() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?.join(PLUGIN_ROOT);
    let config = read_json(&root.join(".lsp.json"))?;
    let config = object(&config, "LSP config")?;
    assert_eq!(config.len(), 1, "Claude package must configure one LSP server");

    let perl = config.get("perl").ok_or("missing `perl` LSP entry")?;
    let perl = object(perl, "perl LSP entry")?;
    assert_eq!(string_field(perl, "command")?, "perllsp");
    assert_eq!(string_field(perl, "transport")?, "stdio");
    assert_eq!(string_field(perl, "workspaceFolder")?, "${CLAUDE_PROJECT_DIR}");

    let args = perl.get("args").and_then(Value::as_array).ok_or("args must be an array")?;
    assert_eq!(args, &[Value::String("--stdio".to_string())]);

    let extensions = perl
        .get("extensionToLanguage")
        .and_then(Value::as_object)
        .ok_or("extensionToLanguage must be an object")?;
    let actual = extensions.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = EXPECTED_EXTENSIONS.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert!(extensions.values().all(|language| language == "perl"));

    for forbidden in [".pod", ".xs", ".ep", ".tt", ".tt2", ".mason"] {
        assert!(!extensions.contains_key(forbidden), "unproven activation leaked: {forbidden}");
    }
    Ok(())
}

#[test]
fn claude_plugin_package_inventory_contains_no_binary_or_bridge() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?.join(PLUGIN_ROOT);
    let mut files = BTreeSet::new();

    for entry in WalkDir::new(&root) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(format!(
                "plugin package must not contain symlink: {}",
                entry.path().display()
            )
            .into());
        }
        if !entry.file_type().is_file() {
            continue;
        }
        files.insert(entry.path().strip_prefix(&root)?.to_string_lossy().replace('\\', "/"));
    }

    let expected = [
        ".claude-plugin/plugin.json",
        ".lsp.json",
        "CHANGELOG.md",
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "README.md",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    assert_eq!(files, expected, "plugin package inventory changed without review");
    assert!(!root.join(".mcp.json").exists());
    assert!(!root.join("bin").exists());
    Ok(())
}
