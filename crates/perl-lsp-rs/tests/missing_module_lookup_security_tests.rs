//! Security regression tests for `perl.explainMissingModuleLookup`.
//!
//! Guards against using crafted module names as a filesystem existence oracle,
//! and pins the #4998 / #10817 client include-path admission boundary: absolute
//! (and escaping) client `includePaths` never reach effective `@INC`.
//!
//! There is no production or test-only LSP constructor for
//! `ExternalIncludePathAuthority::TrustedUserOperator`. Config-layer tests in
//! `perl-lsp-rs-core` already prove that trusted-operator channel. This file
//! does not restore absolute `includePaths` admission from client settings.

use perl_lsp::{JsonRpcRequest, LspServer};
use perl_lsp_rs_core::config::{
    ExternalIncludePathAuthority, RejectedClientIncludePath, RejectedClientIncludePathReason,
    UnauthorizedExternalIncludePathSource, WorkspaceConfig, WorkspaceConfigUpdateContext,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn setup_server(root_path: Option<String>, initialization_options: Option<Value>) -> LspServer {
    let server = LspServer::new();

    let mut init_params = json!({
        "processId": null,
        "rootPath": root_path,
        "capabilities": {}
    });
    if let Some(options) = initialization_options {
        init_params["initializationOptions"] = options;
    }

    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "initialize".to_string(),
        params: Some(init_params),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
    };
    let _ = server.handle_request(init_request);

    let initialized_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "initialized".to_string(),
        params: Some(json!({})),
        id: None,
    };
    let _ = server.handle_request(initialized_request);
    server
}

fn explain_missing_module(
    server: &LspServer,
    module: &str,
) -> Option<perl_lsp::protocol::JsonRpcResponse> {
    let execute_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "workspace/executeCommand".to_string(),
        params: Some(json!({
            "command": "perl.explainMissingModuleLookup",
            "arguments": [{ "module": module }]
        })),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
    };
    server.handle_request(execute_request)
}

fn lookup_result(server: &LspServer, module: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let response = explain_missing_module(server, module)
        .ok_or_else(|| format!("expected JSON-RPC response for {module}"))?;
    let result = response
        .result
        .ok_or_else(|| format!("lookup for {module} should succeed without error"))?;
    if result
        .pointer("/module_resolution/effective_include_paths")
        .and_then(Value::as_array)
        .is_none()
    {
        return Err(format!("missing effective_include_paths for {module}: {result}").into());
    }
    Ok(result)
}

fn effective_include_paths(result: &Value) -> &[Value] {
    result
        .pointer("/module_resolution/effective_include_paths")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn collect_candidates(result: &Value) -> Vec<&Value> {
    effective_include_paths(result)
        .iter()
        .flat_map(|entry| {
            entry.get("candidate_paths").and_then(Value::as_array).into_iter().flatten()
        })
        .collect()
}

fn candidate_exists_values(result: &Value) -> TestResult {
    let mut values = Vec::new();
    for candidate in collect_candidates(result) {
        match candidate.get("exists") {
            Some(Value::Bool(value)) => values.push(Some(*value)),
            Some(Value::Null) | None => values.push(None),
            Some(other) => return Err(format!("unexpected exists payload: {other:?}").into()),
        }
    }
    Ok(values)
}

fn write_module(root: &Path, relative_pm: &str, package: &str) -> TestResult {
    let path = root.join(relative_pm);
    fs::create_dir_all(path.parent().ok_or("missing module parent dir")?)?;
    fs::write(&path, format!("package {package};\n1;\n"))?;
    Ok(())
}

fn isolated_client_workspace(include_paths: Vec<Value>, extra: Option<Value>) -> Value {
    let mut workspace = json!({
        "includePaths": include_paths,
        "usePerl5lib": false,
        "useSystemInc": false,
    });
    if let Some(Value::Object(extra_fields)) = extra
        && let Some(object) = workspace.as_object_mut()
    {
        object.extend(extra_fields);
    }
    json!({ "workspace": workspace })
}

fn reject_client_include_paths(
    workspace_root: &Path,
    settings: &Value,
) -> Vec<RejectedClientIncludePath> {
    let mut config = WorkspaceConfig::default();
    config.update_from_value_with_context(
        settings,
        WorkspaceConfigUpdateContext {
            workspace_root: Some(workspace_root),
            external_include_paths: ExternalIncludePathAuthority::Untrusted(
                UnauthorizedExternalIncludePathSource::InitializationOptions,
            ),
        },
    )
}

fn path_is_under(path: &str, root: &Path) -> bool {
    Path::new(path).starts_with(root)
}

fn candidates_under_root<'a>(result: &'a Value, root: &Path) -> Vec<&'a Value> {
    collect_candidates(result)
        .into_iter()
        .filter(|candidate| {
            candidate
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path_is_under(path, root))
        })
        .collect()
}

fn include_roots_under<'a>(result: &'a Value, root: &Path) -> Vec<&'a Value> {
    effective_include_paths(result)
        .iter()
        .filter(|entry| {
            entry.get("path").and_then(Value::as_str).is_some_and(|path| path_is_under(path, root))
        })
        .collect()
}

fn assert_no_existence_oracle(result: &Value) -> TestResult {
    let exists_values = candidate_exists_values(result)?;
    assert!(
        !exists_values.contains(&Some(true)),
        "outside-workspace include roots must not report exists=true: {result}"
    );
    for candidate in collect_candidates(result) {
        let inside = candidate.get("inside_workspace").and_then(Value::as_bool);
        if inside == Some(false) {
            assert_eq!(
                candidate.get("exists"),
                Some(&Value::Null),
                "out-of-workspace candidate must not leak exists: {candidate:?}"
            );
            assert_eq!(
                candidate.get("probed").and_then(Value::as_bool),
                Some(false),
                "out-of-workspace candidate must not be probed: {candidate:?}"
            );
        }
    }
    Ok(())
}

fn assert_rejected_root_absent(
    result: &Value,
    rejected_root: &Path,
    expected_reason: RejectedClientIncludePathReason,
    rejected: &[RejectedClientIncludePath],
) -> TestResult {
    assert!(
        rejected.iter().any(|entry| {
            Path::new(&entry.entry) == rejected_root && entry.reason == expected_reason
        }),
        "expected {expected_reason:?} for {}: {rejected:?}",
        rejected_root.display()
    );
    assert!(
        include_roots_under(result, rejected_root).is_empty(),
        "rejected include root must not appear in effective @INC: {result}"
    );
    assert!(
        candidates_under_root(result, rejected_root).is_empty(),
        "rejected include root must produce zero candidates: {result}"
    );
    assert_no_existence_oracle(result)
}

#[test]
fn explain_missing_module_rejects_path_traversal_module() -> TestResult {
    let temp_dir = TempDir::new()?;
    let root_path = temp_dir.path().to_string_lossy().to_string();
    let server = setup_server(Some(root_path), None);

    let response = explain_missing_module(&server, "../../etc/passwd")
        .ok_or("expected JSON-RPC response for traversal module")?;
    assert!(
        response.result.is_none(),
        "path traversal module must not return a success payload: {:?}",
        response.result
    );
    let error = response.error.ok_or("expected invalid_params error for traversal module")?;
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("Invalid module name"),
        "unexpected error message: {}",
        error.message
    );

    Ok(())
}

#[test]
fn explain_missing_module_rejects_slash_module() -> TestResult {
    let temp_dir = TempDir::new()?;
    let root_path = temp_dir.path().to_string_lossy().to_string();
    let server = setup_server(Some(root_path), None);

    let response = explain_missing_module(&server, "Foo/Bar")
        .ok_or("expected JSON-RPC response for slash module")?;
    assert!(
        response.result.is_none(),
        "slash-shaped module must not return a success payload: {:?}",
        response.result
    );
    let error = response.error.ok_or("expected invalid_params error for slash module")?;
    assert_eq!(error.code, -32602);

    Ok(())
}

#[test]
fn explain_missing_module_reports_exists_true_inside_workspace() -> TestResult {
    let workspace_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;

    let server = setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), None);
    let result = lookup_result(&server, "Foo::Bar")?;
    assert!(
        candidate_exists_values(&result)?.contains(&Some(true)),
        "in-workspace module should report exists=true for a workspace candidate: {result}"
    );

    Ok(())
}

#[test]
fn explain_missing_module_probes_missing_in_workspace_module() -> TestResult {
    let workspace_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;

    let server = setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), None);
    let result = lookup_result(&server, "Foo::Missing")?;
    let missing_candidates = collect_candidates(&result)
        .into_iter()
        .filter(|candidate| {
            candidate.get("inside_workspace").and_then(Value::as_bool) == Some(true)
                && candidate.get("exists") == Some(&Value::Bool(false))
                && candidate.get("probed").and_then(Value::as_bool) == Some(true)
        })
        .count();
    assert!(
        missing_candidates > 0,
        "missing in-workspace module should be classified inside and probed: {result}"
    );

    Ok(())
}

#[test]
fn explain_missing_module_exists_only_inside_workspace() -> TestResult {
    let workspace_dir = TempDir::new()?;
    let outside_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;
    write_module(outside_dir.path(), "Oracle/Probe.pm", "Oracle::Probe")?;

    let outside_include_path = outside_dir.path().to_string_lossy().to_string();
    let init_options = isolated_client_workspace(vec![json!(outside_include_path)], None);
    let rejected = reject_client_include_paths(workspace_dir.path(), &init_options);

    let server =
        setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), Some(init_options));
    let result = lookup_result(&server, "Oracle::Probe")?;
    assert_rejected_root_absent(
        &result,
        outside_dir.path(),
        RejectedClientIncludePathReason::Absolute,
        &rejected,
    )?;

    Ok(())
}

#[test]
fn explain_missing_module_keeps_relative_include_when_absolute_sibling_rejected() -> TestResult {
    let workspace_dir = TempDir::new()?;
    let outside_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;
    write_module(outside_dir.path(), "Oracle/Probe.pm", "Oracle::Probe")?;

    let outside_include_path = outside_dir.path().to_string_lossy().to_string();
    let init_options =
        isolated_client_workspace(vec![json!("lib"), json!(outside_include_path)], None);
    let rejected = reject_client_include_paths(workspace_dir.path(), &init_options);
    assert!(
        rejected.iter().any(|entry| {
            entry.entry == outside_include_path
                && entry.reason == RejectedClientIncludePathReason::Absolute
        }),
        "absolute sibling must be RejectedClientIncludePath::Absolute: {rejected:?}"
    );
    assert!(
        !rejected.iter().any(|entry| entry.entry == "lib"),
        "contained relative includePaths entry must still be admitted: {rejected:?}"
    );

    let server =
        setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), Some(init_options));
    let inside = lookup_result(&server, "Foo::Bar")?;
    assert!(
        candidate_exists_values(&inside)?.contains(&Some(true)),
        "admitted relative include path must still probe in-workspace modules: {inside}"
    );

    let outside = lookup_result(&server, "Oracle::Probe")?;
    assert_rejected_root_absent(
        &outside,
        outside_dir.path(),
        RejectedClientIncludePathReason::Absolute,
        &rejected,
    )?;

    Ok(())
}

#[test]
fn explain_missing_module_rejects_traversal_include_paths_without_outside_candidates() -> TestResult
{
    let workspace_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;

    let traversal = "../../../../etc";
    let init_options = isolated_client_workspace(vec![json!(traversal)], None);
    let rejected = reject_client_include_paths(workspace_dir.path(), &init_options);
    assert!(
        rejected.iter().any(|entry| {
            entry.entry == traversal
                && matches!(entry.reason, RejectedClientIncludePathReason::EscapesWorkspace(_))
        }),
        "traversal includePaths must be RejectedClientIncludePath::EscapesWorkspace: {rejected:?}"
    );

    let server =
        setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), Some(init_options));
    let result = lookup_result(&server, "Oracle::Probe")?;
    assert!(
        !collect_candidates(&result).iter().any(|candidate| {
            candidate
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.contains("/etc/") || path.ends_with("/etc"))
        }),
        "escaping include path must not appear as a lookup candidate: {result}"
    );
    assert_no_existence_oracle(&result)?;

    Ok(())
}

#[test]
fn explain_missing_module_rejects_client_external_include_paths() -> TestResult {
    let workspace_dir = TempDir::new()?;
    let outside_dir = TempDir::new()?;
    write_module(workspace_dir.path(), "lib/Foo/Bar.pm", "Foo::Bar")?;
    write_module(outside_dir.path(), "Oracle/Probe.pm", "Oracle::Probe")?;

    let outside_include_path = outside_dir.path().to_string_lossy().to_string();
    let init_options = isolated_client_workspace(
        vec![json!("lib")],
        Some(json!({
            "externalIncludePaths": [outside_include_path]
        })),
    );
    let rejected = reject_client_include_paths(workspace_dir.path(), &init_options);
    assert!(
        rejected.iter().any(|entry| {
            entry.entry == outside_include_path
                && matches!(
                    entry.reason,
                    RejectedClientIncludePathReason::ExternalUnauthorized(
                        UnauthorizedExternalIncludePathSource::InitializationOptions
                    )
                )
        }),
        "client externalIncludePaths must be ExternalUnauthorized: {rejected:?}"
    );

    let server =
        setup_server(Some(workspace_dir.path().to_string_lossy().to_string()), Some(init_options));
    let inside = lookup_result(&server, "Foo::Bar")?;
    assert!(
        candidate_exists_values(&inside)?.contains(&Some(true)),
        "resource-scoped relative includePaths must still work: {inside}"
    );

    let outside = lookup_result(&server, "Oracle::Probe")?;
    assert_rejected_root_absent(
        &outside,
        outside_dir.path(),
        RejectedClientIncludePathReason::ExternalUnauthorized(
            UnauthorizedExternalIncludePathSource::InitializationOptions,
        ),
        &rejected,
    )?;

    Ok(())
}
