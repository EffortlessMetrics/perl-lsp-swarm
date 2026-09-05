//! Execution-source authority contract for the managed Zed perllsp route
//! (#11041).
//!
//! Every executable-selection route must carry a typed authority and
//! provenance disposition, and the classification must be consumed before any
//! binary lookup, command construction, download, or process start. Unknown
//! or merged-away provenance must fail closed instead of silently authorizing
//! execution.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/execution-source.v1.json";
const EXTENSION_SOURCE_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs";

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

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(root.join(relative))?)?)
}

fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("missing string at `{pointer}`")).into())
}

fn route<'a>(value: &'a Value, route_name: &str) -> Result<&'a Value, Box<dyn Error>> {
    value
        .pointer("/routes")
        .and_then(Value::as_array)
        .and_then(|routes| {
            routes
                .iter()
                .find(|route| route.get("route").and_then(Value::as_str) == Some(route_name))
        })
        .ok_or_else(|| io::Error::other(format!("missing route `{route_name}`")).into())
}

#[test]
fn execution_source_contract_binds_all_three_routes() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;

    assert_eq!(string(&contract, "/schema_version")?, "perllsp_execution_source.v1");
    assert_eq!(string(&contract, "/server_id")?, "perllsp");
    assert_eq!(
        string(&contract, "/policy/authority_identity_separation")?,
        "route authority is typed independently of canonical binary identity (#10340): identity proves what was selected, never that the selecting source was authorized"
    );

    let explicit = route(&contract, "binary_override")?;
    assert_eq!(
        explicit.get("extension_authority").and_then(Value::as_str),
        Some("refused"),
        "unproven merged overrides must be refused outright"
    );
    assert_eq!(
        explicit.get("extension_execution").and_then(Value::as_bool),
        Some(false),
        "the extension must never execute a merged settings override"
    );

    let worktree_path = route(&contract, "worktree_path")?;
    assert_eq!(
        worktree_path.get("extension_authority").and_then(Value::as_str),
        Some("authorized_worktree_environment")
    );
    let limitations = worktree_path
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or("worktree_path cell must record its limitations")?;
    assert!(
        !limitations.is_empty(),
        "the worktree-environment cell must stay visibly distinct with recorded limitations"
    );

    let managed = route(&contract, "managed_download")?;
    assert_eq!(
        managed.get("extension_authority").and_then(Value::as_str),
        Some("authorized_release_identity")
    );

    assert_eq!(string(&contract, "/claim_boundary/real_host_trust_receipt")?, "not_run");
    assert_eq!(string(&contract, "/claim_boundary/public_zed_support")?, "not_proven");

    Ok(())
}

/// Drop `//` comment lines so structural pins can only be satisfied by real
/// code, not by commented-out invocations.
fn code_lines(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn extension_classifies_the_execution_source_before_any_lookup() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(&root, EXTENSION_SOURCE_RELATIVE_PATH)?;

    assert!(
        source.contains("fn authorize_execution_source"),
        "the fail-closed gate function must exist"
    );
    assert!(
        source.contains("enum ExecutionRoute"),
        "execution routes must be a typed enum carrying no path or digest fields"
    );

    let classified = code_lines(&source);
    let command_start =
        classified.find("fn perllsp_command(").ok_or("missing perllsp_command entry point")?;
    let command_end = command_start
        + classified[command_start..]
            .find("fn perllsp_binary(")
            .ok_or("perllsp_command body has no terminator anchor")?;
    let body = &classified[command_start..command_end];

    let gate = body
        .find("authorize_execution_source(command_settings.path.as_deref())")
        .ok_or("perllsp_command must consume the execution-source gate")?;
    let resolution =
        body.find("perllsp_binary(").ok_or("perllsp_command must resolve the binary")?;
    assert!(gate < resolution, "classification must run before any binary lookup or download");

    let lookup_after_gate = body.find("worktree.which(");
    if let Some(lookup_at) = lookup_after_gate {
        assert!(gate < lookup_at, "no PATH lookup may precede the execution-source gate");
    }

    Ok(())
}

#[test]
fn settings_overrides_cannot_inject_loader_environment() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(&root, EXTENSION_SOURCE_RELATIVE_PATH)?;

    assert!(
        source.contains("fn is_loader_injection_key"),
        "the dynamic-loader injection filter must exist"
    );
    for key in [
        "\"ld_preload\"",
        "\"ld_audit\"",
        "\"ld_library_path\"",
        "\"dyld_insert_libraries\"",
        "\"dyld_force_flat_namespace\"",
        "\"dyld_library_path\"",
        "\"dyld_framework_path\"",
        "\"dyld_fallback_framework_path\"",
    ] {
        assert!(
            source.contains(key),
            "the filter must cover {key} so project settings cannot load code into the server process"
        );
    }

    // The filter is applied to the settings-supplied layer before it joins
    // the Zed-defined worktree environment.
    let classified = code_lines(&source);
    let settings_start =
        classified.find("fn perllsp_command_settings").ok_or("missing perllsp_command_settings")?;
    let settings_end = settings_start
        + classified[settings_start..]
            .find("fn normalize_perllsp_args")
            .ok_or("perllsp_command_settings body has no terminator anchor")?;
    let body = &classified[settings_start..settings_end];
    let retain = body
        .find("overrides.retain(|key, _| !is_loader_injection_key(key))")
        .ok_or("the override layer must drop loader-injection keys")?;
    let extend = body
        .find("shell_env.extend(overrides)")
        .ok_or("the worktree environment merge must stay intact")?;
    assert!(
        retain < extend,
        "loader-injection keys must be dropped before overrides join the environment"
    );

    Ok(())
}

#[test]
fn worktree_path_hits_never_flow_through_release_identity_binding() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(&root, EXTENSION_SOURCE_RELATIVE_PATH)?;

    // Negative control for project-controlled PATH resolution (#11041): a
    // `worktree.which` hit resolves under the worktree-environment cell only
    // and must not touch the selection manifest or any digest binding, so a
    // hostile PATH entry can never be relabeled as release-proven.
    let classified = code_lines(&source);
    let binary_start = classified.find("fn perllsp_binary(").ok_or("missing perllsp_binary")?;
    let binary_end = binary_start
        + classified[binary_start..]
            .find("fn download_perllsp(")
            .ok_or("perllsp_binary body has no terminator anchor")?;
    let body = &classified[binary_start..binary_end];
    assert!(
        body.contains("worktree.which(PERLLSP_SERVER_ID)"),
        "perllsp_binary must keep its exact PATH discovery surface"
    );
    for identity_surface in ["SELECTION_MANIFEST", "content_sha256", "load_accepted_current_in"] {
        assert!(
            !body.contains(identity_surface),
            "a worktree PATH hit must never flow through `{identity_surface}` release-identity binding"
        );
    }

    Ok(())
}

#[test]
fn unproven_overrides_are_refused_without_echoing_the_value() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(&root, EXTENSION_SOURCE_RELATIVE_PATH)?;

    let refusal = source
        .find("const EXECUTION_SOURCE_REFUSAL")
        .ok_or("the typed refusal constant must exist")?;
    let gate_end = source[refusal..]
        .find("fn authorize_execution_source")
        .map(|offset| refusal + offset)
        .ok_or("the gate must follow its refusal message")?;
    let message = &source[refusal..gate_end];

    for required in [
        "#11041",
        "merged Zed settings carry no provenance",
        "worktree trust",
        "worktree PATH",
        "managed download",
    ] {
        // Source line breaks must not hide meaning: match the logical
        // message after collapsing concatenation whitespace.
        let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
        let required = required.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains(&required),
            "refusal must name `{required}` so every alternative stays reachable"
        );
    }

    // The gate refuses any present value; only the exact empty-path rejection
    // and the bounded None case return other outcomes.
    let gate_start = source.find("fn authorize_execution_source").ok_or("missing gate")?;
    let gate_body_end = source[gate_start..]
        .find("\n}")
        .map(|offset| gate_start + offset)
        .ok_or("gate has no terminator")?;
    let gate_body = &source[gate_start..gate_body_end];
    assert!(
        gate_body.contains("Some(_) => Err(EXECUTION_SOURCE_REFUSAL.to_string())"),
        "every non-empty override must fail closed"
    );
    assert!(
        gate_body.contains("\"lsp.perllsp.binary.path must not be empty\""),
        "the empty-override typed rejection must keep its exact surface"
    );
    assert!(
        !gate_body.contains("{}"),
        "the gate must not interpolate refused values into messages"
    );

    Ok(())
}

#[test]
fn upstream_candidate_pins_survive_the_authority_gate() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(&root, EXTENSION_SOURCE_RELATIVE_PATH)?;

    for pinned in [
        "LspSettings::for_worktree(PERLLSP_SERVER_ID, worktree)",
        "worktree.shell_env()",
        "shell_env.extend(overrides)",
        "\"lsp.perllsp.binary.path must not be empty\"",
    ] {
        assert!(
            source.contains(pinned),
            "upstream candidate pin `{pinned}` must survive the authority gate"
        );
    }

    Ok(())
}
