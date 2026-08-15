use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::Write,
    process::{Command, Stdio},
};

const MANIFEST: &str = include_str!("../../../integrations/lsp4ij/upstream/0.20.1/manifest.json");
const LSP_TEMPLATE: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/template.json");
const LSP_SETTINGS: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/settings.json");
const LSP_INIT_OPTIONS: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/initializationOptions.json");
const LSP_INSTALLER: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/installer.json");
const DAP_TEMPLATE: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/dap/template.json");
const DAP_INSTALLER: &str =
    include_str!("../../../integrations/lsp4ij/upstream/0.20.1/dap/installer.json");
const LSP_DOC: &str = include_str!("../../../integrations/lsp4ij/upstream/0.20.1/docs/perl-lsp.md");
const DAP_DOC: &str = include_str!("../../../integrations/lsp4ij/upstream/0.20.1/docs/perl-dap.md");

const MATERIALIZED_FIXTURES: &[(&str, &[u8])] = &[
    (
        "lsp/template.json",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/template.json"),
    ),
    (
        "lsp/settings.json",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/settings.json"),
    ),
    (
        "lsp/initializationOptions.json",
        include_bytes!(
            "../../../integrations/lsp4ij/upstream/0.20.1/lsp/initializationOptions.json"
        ),
    ),
    (
        "lsp/installer.json",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/lsp/installer.json"),
    ),
    (
        "dap/template.json",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/dap/template.json"),
    ),
    (
        "dap/installer.json",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/dap/installer.json"),
    ),
    (
        "docs/perl-lsp.md",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/docs/perl-lsp.md"),
    ),
    (
        "docs/perl-dap.md",
        include_bytes!("../../../integrations/lsp4ij/upstream/0.20.1/docs/perl-dap.md"),
    ),
];

fn parse(input: &str) -> Value {
    serde_json::from_str(input).expect("checked LSP4IJ fixture must parse")
}

fn is_lower_hex_sha1(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn git_blob_sha1(bytes: &[u8]) -> String {
    let mut child = Command::new("git")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git must be available to verify released fixture identities");
    child
        .stdin
        .take()
        .expect("git hash-object stdin")
        .write_all(bytes)
        .expect("write fixture bytes to git hash-object");
    let output = child.wait_with_output().expect("git hash-object must complete");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git hash-object output must be UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn released_fixture_pins_one_exact_nonmoving_upstream_subject() {
    let manifest = parse(MANIFEST);
    assert_eq!(manifest.get("schema_version"), Some(&json!("lsp4ij_released_fixture.v1")));
    assert_eq!(manifest.pointer("/upstream/repository"), Some(&json!("redhat-developer/lsp4ij")));
    assert_eq!(manifest.pointer("/upstream/release"), Some(&json!("0.20.1")));
    assert_eq!(manifest.pointer("/upstream/tag"), Some(&json!("0.20.1")));
    assert_eq!(manifest.pointer("/upstream/normalization"), Some(&json!("identity-v1")));
    assert_eq!(manifest.pointer("/import_tool/name"), Some(&json!("git-hash-object")));
    assert_eq!(manifest.pointer("/import_tool/version"), Some(&json!("1")));

    for pointer in ["/upstream/commit", "/upstream/lsp_tree_sha1", "/upstream/dap_tree_sha1"] {
        let value = manifest
            .pointer(pointer)
            .and_then(Value::as_str)
            .expect("upstream identity must be a string");
        assert!(is_lower_hex_sha1(value), "{pointer} must pin a lowercase 40-byte Git object id");
        assert!(!matches!(value, "main" | "master" | "HEAD"));
    }
}

#[test]
fn released_fixture_inventory_is_bounded_and_digest_addressed() {
    let manifest = parse(MANIFEST);
    let sources =
        manifest.get("sources").and_then(Value::as_array).expect("fixture manifest sources");
    assert_eq!(sources.len(), 17, "bounded Perl fixture source set drifted");

    let mut namespaces = BTreeSet::new();
    let mut materialized = 0usize;
    for source in sources {
        let namespace = source.get("namespace").and_then(Value::as_str).expect("source namespace");
        namespaces.insert(namespace);
        let path = source.get("path").and_then(Value::as_str).expect("source path");
        assert!(
            !path.starts_with('/') && !path.contains(".."),
            "upstream fixture path must be bounded: {path}"
        );
        assert!(
            path.starts_with("src/main/resources/templates/lsp/perl-lsp/")
                || path.starts_with("src/main/resources/templates/dap/perl-dap/")
                || path == "docs/user-defined-ls/perl-lsp.md"
                || path == "docs/dap/user-defined-dap/perl-dap.md",
            "unrelated upstream path entered the Perl fixture: {path}"
        );
        let blob = source
            .get("git_blob_sha1")
            .and_then(Value::as_str)
            .expect("raw upstream blob identity");
        assert!(is_lower_hex_sha1(blob), "invalid raw upstream blob identity for {path}");
        assert!(
            source.get("size").and_then(Value::as_u64).is_some(),
            "source byte size missing: {path}"
        );

        let is_materialized =
            source.get("materialized").and_then(Value::as_bool).expect("materialized disposition");
        if is_materialized {
            materialized += 1;
            assert!(
                source.get("fixture_path").and_then(Value::as_str).is_some(),
                "materialized source lacks fixture path: {path}"
            );
        } else {
            assert!(
                source.get("fixture_path").is_none(),
                "unmaterialized source must not claim a local fixture path: {path}"
            );
        }
    }
    assert_eq!(materialized, 8);
    assert!(namespaces.contains("lsp") && namespaces.contains("dap"));
    assert!(namespaces.contains("lsp_docs") && namespaces.contains("dap_docs"));
}

#[test]
fn materialized_fixture_bytes_match_manifest_git_blobs_and_sizes() {
    let manifest = parse(MANIFEST);
    let sources =
        manifest.get("sources").and_then(Value::as_array).expect("fixture manifest sources");
    let mut matched = BTreeSet::new();

    for source in sources {
        if !source.get("materialized").and_then(Value::as_bool).expect("materialized disposition") {
            continue;
        }
        let fixture_path = source
            .get("fixture_path")
            .and_then(Value::as_str)
            .expect("materialized source fixture path");
        assert!(matched.insert(fixture_path), "duplicate materialized fixture: {fixture_path}");
        let Some((_, bytes)) = MATERIALIZED_FIXTURES.iter().find(|(path, _)| *path == fixture_path)
        else {
            assert!(false, "manifest fixture path is not included: {fixture_path}");
            return;
        };
        let expected_blob = source
            .get("git_blob_sha1")
            .and_then(Value::as_str)
            .expect("materialized source blob identity");
        assert_eq!(git_blob_sha1(bytes), expected_blob, "fixture bytes drifted: {fixture_path}");
        assert_eq!(
            source.get("size").and_then(Value::as_u64),
            Some(bytes.len() as u64),
            "fixture byte size drifted: {fixture_path}"
        );
    }

    assert_eq!(matched.len(), MATERIALIZED_FIXTURES.len());
    for (fixture_path, _) in MATERIALIZED_FIXTURES {
        assert!(matched.contains(fixture_path), "fixture is absent from manifest: {fixture_path}");
    }
}

#[test]
fn changed_materialized_bytes_do_not_reuse_released_identity() {
    let manifest = parse(MANIFEST);
    let expected_blob = manifest
        .get("sources")
        .and_then(Value::as_array)
        .and_then(|sources| {
            sources.iter().find(|source| {
                source.get("path").and_then(Value::as_str)
                    == Some("src/main/resources/templates/lsp/perl-lsp/template.json")
            })
        })
        .and_then(|source| source.get("git_blob_sha1"))
        .and_then(Value::as_str)
        .expect("LSP template blob identity");
    let original = MATERIALIZED_FIXTURES
        .iter()
        .find(|(path, _)| *path == "lsp/template.json")
        .map(|(_, bytes)| *bytes)
        .expect("LSP template fixture");
    let mut changed = original.to_vec();
    let Some(first) = changed.first_mut() else {
        assert!(false, "LSP template fixture must not be empty");
        return;
    };
    *first ^= 1;

    assert_eq!(git_blob_sha1(original), expected_blob);
    assert_ne!(git_blob_sha1(&changed), expected_blob);
}

#[test]
fn released_lsp_bytes_preserve_the_upstream_behavior_and_known_drift() {
    let template = parse(LSP_TEMPLATE);
    assert_eq!(template.get("id"), Some(&json!("perl-lsp")));
    assert_eq!(template.get("expandConfiguration"), Some(&json!(true)));
    let program_args =
        template.get("programArgs").and_then(Value::as_object).expect("released programArgs");
    assert!(program_args.values().all(|value| {
        value
            .as_str()
            .is_some_and(|command| command.contains("perllsp") && command.contains("--stdio"))
    }));

    let patterns: BTreeSet<_> = template
        .pointer("/fileTypeMappings/0/fileType/patterns")
        .and_then(Value::as_array)
        .expect("released Perl mappings")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for released_extra in ["*.PL", "*.cgi", "*.fcgi", "*.xs", "*.pod", "*.psgi", "*.tt2"] {
        assert!(
            patterns.contains(released_extra),
            "released mapping evidence was silently narrowed: {released_extra}"
        );
    }

    let settings = parse(LSP_SETTINGS);
    for copied_extension_key in [
        "perl-lsp.autoDownload",
        "perl-lsp.formatOnSave",
        "perl-lsp.enableTestIntegration",
        "perl-lsp.trace.server",
        "perl-lsp.mcp.servers",
    ] {
        assert!(
            settings.get(copied_extension_key).is_some(),
            "released VS Code-style setting evidence disappeared: {copied_extension_key}"
        );
    }
    assert_eq!(parse(LSP_INIT_OPTIONS), json!({}));
    assert!(
        LSP_INSTALLER.contains("releases/download/v0.15.0/"),
        "released fixed installer fallback must remain visible as evidence"
    );
    assert!(
        LSP_DOC.contains("same settings as VSCode"),
        "released configuration guidance drifted from the pinned evidence"
    );
}

#[test]
fn released_dap_bytes_remain_independent_from_lsp_desired_state() {
    let template = parse(DAP_TEMPLATE);
    assert_eq!(template.get("id"), Some(&json!("perl-dap")));
    assert_eq!(
        template.pointer("/launch/default"),
        Some(&json!("<<insert base directory>>/perl-dap"))
    );
    let patterns: BTreeSet<_> = template
        .pointer("/fileTypeMappings/0/fileType/patterns")
        .and_then(Value::as_array)
        .expect("released DAP Perl mappings")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(patterns.contains("*.pod") && patterns.contains("*.xs") && patterns.contains("*.psgi"));
    assert!(DAP_INSTALLER.contains("releases/download/v0.15.0/"));
    assert!(DAP_INSTALLER.contains("perl-dap"));
    assert!(
        DAP_DOC.starts_with("TODO doc"),
        "placeholder DAP documentation is part of released truth until an upstream release changes it"
    );
}
