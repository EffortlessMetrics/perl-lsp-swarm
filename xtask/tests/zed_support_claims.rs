use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(relative))?)
}

fn toml_value(root: &Path, relative: &str) -> Result<toml::Value, Box<dyn Error>> {
    Ok(toml::from_str(&read(root, relative)?)?)
}

fn json_value(root: &Path, relative: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&read(root, relative)?)?)
}

fn markdown_section<'a>(
    text: &'a str,
    heading: &str,
    next_heading_prefix: &str,
) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("{heading}\n");
    let start = text
        .find(&marker)
        .ok_or_else(|| io::Error::other(format!("missing Markdown section `{heading}`")))?;
    let body_start = start + marker.len();
    let next_marker = format!("\n{next_heading_prefix}");
    let end = text[body_start..]
        .find(&next_marker)
        .map_or(text.len(), |offset| body_start + offset);
    Ok(&text[body_start..end])
}

fn string_array(value: &toml::Value, key: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let values = value
        .get(key)
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other(format!("missing TOML array `{key}`")))?;
    values
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| io::Error::other(format!("non-string entry in `{key}`")).into())
        })
        .collect()
}

#[test]
fn active_zed_claims_are_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let readme = read(&root, "README.md")?;
    let faq = read(&root, "docs/reference/FAQ.md")?;
    let zed = read(&root, "docs/EDITORS/ZED_SETUP.md")?;
    let editor_setup = read(&root, "docs/how-to/EDITOR_SETUP.md")?;
    let book_setup = read(&root, "book/src/reference/editor-setup-canonical.md")?;
    let troubleshooting = read(&root, "docs/how-to/TROUBLESHOOTING.md")?;
    let steering = read(&root, ".kiro/steering/product.md")?;

    assert!(readme.contains("Zed integration: planned / not proven"));
    assert!(!readme.contains("Helix, Zed, Sublime"));
    assert!(faq.contains("Zed is **planned / not proven**"));
    assert!(zed.contains("**Status: planned / not proven.**"));
    assert!(zed.contains("does **not** register `perllsp`"));
    assert!(editor_setup.contains("Planned / not proven"));
    assert_eq!(
        book_setup, editor_setup,
        "mdBook projection drifted from canonical editor setup"
    );
    assert!(troubleshooting.contains("public Perl extension does not register `perllsp`"));
    assert!(steering.contains("Zed integration: planned / not proven"));

    let combined_zed = markdown_section(&editor_setup, "### Zed", "### ")?;
    let troubleshooting_zed = markdown_section(
        &troubleshooting,
        "## Zed Does Not Start `perllsp`",
        "## ",
    )?;
    for (path, text) in [
        ("docs/EDITORS/ZED_SETUP.md", zed.as_str()),
        ("docs/how-to/EDITOR_SETUP.md", combined_zed),
        (
            "docs/how-to/TROUBLESHOOTING.md",
            troubleshooting_zed,
        ),
    ] {
        assert!(
            !text.contains("\"perl-lsp\": {"),
            "{path} must not repoint Zed's independent perl-lsp ID to perllsp"
        );
    }

    Ok(())
}

#[test]
fn submission_package_preserves_product_identities() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let extension = toml_value(
        &root,
        ".ci/fixtures/zed-perl-upstream/zed-perl/extension.toml",
    )?;
    let servers = extension
        .get("language_servers")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("missing language_servers table"))?;
    let server_ids: BTreeSet<&str> = servers.keys().map(String::as_str).collect();
    let expected = BTreeSet::from(["perl-lsp", "perllsp", "perlnavigator-server"]);
    assert_eq!(server_ids, expected);

    let source = read(
        &root,
        ".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs",
    )?;
    assert!(source.contains("const PERLLSP_SERVER_ID: &str = \"perllsp\";"));
    assert!(source.contains(
        "const PERLLSP_REPO: &str = \"EffortlessMetrics/perl-lsp\";"
    ));
    assert!(source.contains(
        "const PERL_LSP_REPO: &str = \"tree-sitter-perl/perl-tree-sitter-lsp\";"
    ));
    assert!(source.contains("unknown Perl language server id"));
    assert!(source.contains("normalize_perllsp_args"));
    assert!(source.contains("worktree.shell_env()"));
    assert!(source.contains("LspSettings::for_worktree(PERLLSP_SERVER_ID, worktree)"));
    assert!(source.contains("lsp.perllsp.binary.path must not be empty"));
    assert!(source.contains("unsupported perllsp argument"));

    let language = toml_value(
        &root,
        ".ci/fixtures/zed-perl-upstream/zed-perl/languages/perl/config.toml",
    )?;
    let suffixes: BTreeSet<String> = string_array(&language, "path_suffixes")?
        .into_iter()
        .collect();
    for required in ["pl", "PL", "pm", "t", "psgi", "cgi", "fcgi"] {
        assert!(
            suffixes.contains(required),
            "missing Perl suffix `{required}`"
        );
    }
    assert!(
        !suffixes
            .iter()
            .any(|suffix| suffix.eq_ignore_ascii_case("pod"))
    );

    let semantic_rules = json_value(
        &root,
        ".ci/fixtures/zed-perl-upstream/zed-perl/languages/perl/semantic_token_rules.json",
    )?;
    let rule_types: BTreeSet<&str> = semantic_rules
        .as_array()
        .ok_or_else(|| io::Error::other("semantic token rules are not an array"))?
        .iter()
        .filter_map(|rule| {
            rule.get("token_type")
                .and_then(serde_json::Value::as_str)
        })
        .collect();
    assert_eq!(
        rule_types,
        BTreeSet::from(["json_heredoc_key", "sql_heredoc_keyword", "sql_string"])
    );

    Ok(())
}

#[test]
fn managed_targets_exist_in_release_contract() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let manifest = toml_value(&root, ".ci/fixtures/zed-perl-upstream/manifest.toml")?;
    let managed: BTreeSet<String> = string_array(&manifest, "managed_targets")?
        .into_iter()
        .collect();
    let unsupported: BTreeSet<String> = string_array(&manifest, "unsupported_managed_targets")?
        .into_iter()
        .collect();

    let contract = json_value(&root, "docs/reference/downstream-dap-integrations.json")?;
    let released: BTreeSet<String> = contract
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| io::Error::other("release contract targets are missing"))?
        .iter()
        .filter_map(|entry| {
            entry
                .get("triple")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
        .collect();

    let missing: Vec<_> = managed.difference(&released).cloned().collect();
    assert!(
        missing.is_empty(),
        "managed Zed targets lack release artifacts: {missing:?}"
    );
    assert!(unsupported.contains("aarch64-pc-windows-msvc"));

    Ok(())
}

#[test]
fn zed_defaults_keep_alternative_servers_dormant() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let defaults = json_value(&root, ".ci/fixtures/zed-perl-upstream/zed-defaults.json")?;
    let servers: Vec<&str> = defaults
        .pointer("/languages/Perl/language_servers")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| io::Error::other("Perl language server defaults are missing"))?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();

    assert_eq!(
        servers,
        vec!["perlnavigator-server", "!perl-lsp", "!perllsp", "..."]
    );
    Ok(())
}
