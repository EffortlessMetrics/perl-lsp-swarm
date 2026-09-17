use anyhow::{Result, anyhow};
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Map, Value};
use tempfile::TempDir;

#[test]
fn supported_editor_inline_smoke_cli_writes_route_bundle() -> Result<()> {
    let temp = TempDir::new()?;
    let receipt = temp.path().join("supported-editor-inline-smoke.json");
    let receipt_arg =
        receipt.to_str().ok_or_else(|| anyhow!("invalid supported editor receipt path"))?;

    cargo_bin_cmd!("xtask")
        .args(["supported-editor-inline-smoke", "--receipt", receipt_arg])
        .assert()
        .success();

    let bundle: Value = serde_json::from_str(&std::fs::read_to_string(&receipt)?)?;
    assert_eq!(
        bundle.get("schema_version").and_then(Value::as_str),
        Some("supported-editor-inline-smoke.v2")
    );
    assert_eq!(bundle.get("provider").and_then(Value::as_str), Some("inline_completion"));
    assert_eq!(
        bundle.get("provider_action").and_then(Value::as_str),
        Some("supported_editor_inline_smoke_bundle")
    );
    assert_eq!(bundle.get("all_supported_routes_registered").and_then(Value::as_bool), Some(true));
    assert_eq!(bundle.get("route_count").and_then(Value::as_u64), Some(4));
    let boundary = bundle
        .get("claim_boundary")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("claim_boundary missing"))?;
    assert!(boundary.contains("not live editor UI automation"));
    assert!(boundary.contains("editor-visible next-edit suggestions"));
    assert!(boundary.contains("runtime multiline behavior"));

    let routes = bundle
        .get("supported_editor_routes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("supported_editor_routes map missing"))?;
    for route in [
        "stdio_cli_smoke",
        "lsp4ij_upstream_integration",
        "vscode_extension_path",
        "release_built_binary_smoke",
    ] {
        assert!(routes.contains_key(route), "missing route {route}");
    }
    assert_route_surface(
        routes,
        "stdio_cli_smoke",
        "xtask/src/tasks/inline_completion_smoke.rs",
        "fn run_static_client",
    )?;
    assert_route_surface(
        routes,
        "stdio_cli_smoke",
        "xtask/src/tasks/inline_completion_smoke.rs",
        "fn run_disabled_client",
    )?;
    assert_route_surface(
        routes,
        "lsp4ij_upstream_integration",
        "docs/EDITORS/INTELLIJ_IDEA_SETUP.md",
        "Recommended: LSP4IJ Upstream Integration",
    )?;
    assert_route_surface(
        routes,
        "lsp4ij_upstream_integration",
        "crates/perl-lsp-rs/tests/lsp_inline_completion_registration_tests.rs",
        "fn initialize_static_advertises_inline_completion_when_dynamic_registration_false",
    )?;
    assert_route_surface(
        routes,
        "lsp4ij_upstream_integration",
        "crates/perl-lsp-rs/tests/lsp_inline_completion_registration_tests.rs",
        "client/registerCapability",
    )?;
    assert_plane(routes, "lsp4ij_upstream_integration", "launch_identity", "registered")?;
    assert_plane(routes, "lsp4ij_upstream_integration", "static_protocol", "registered")?;
    assert_plane(routes, "lsp4ij_upstream_integration", "dynamic_protocol", "registered")?;
    assert_plane(routes, "lsp4ij_upstream_integration", "host_support_boundary", "registered")?;
    let actual_host =
        assert_plane(routes, "lsp4ij_upstream_integration", "actual_host_evidence", "not_proven")?;
    assert_eq!(
        actual_host.get("owner").and_then(Value::as_str),
        Some("#7719/#7977/#8179"),
        "actual-host evidence must name its owning evidence programme"
    );
    assert_route_surface(
        routes,
        "vscode_extension_path",
        "docs/EDITORS/VS_CODE_SETUP.md",
        "The extension auto-downloads the matching `perllsp` server by default.",
    )?;
    assert_route_surface(
        routes,
        "vscode_extension_path",
        "vscode-extension/package.json",
        "\"perl-lsp.serverPath\"",
    )?;
    assert_route_surface(
        routes,
        "release_built_binary_smoke",
        "docs/development/INLINE_COMPLETION_RELEASE_GATE.md",
        "./scripts/cargo-safe xtask inline-completion-smoke --binary target/agent/perllsp",
    )?;
    assert_route_surface(
        routes,
        "release_built_binary_smoke",
        "xtask/src/tasks/inline_completion_smoke.rs",
        "resolve_binary_path",
    )?;

    let future_gated = bundle
        .get("future_gated")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("future_gated map missing"))?;
    assert_eq!(
        future_gated.get("runtime_multiline_inline_completion").and_then(Value::as_str),
        Some("future_gated")
    );
    assert_eq!(
        future_gated.get("runtime_next_edit_provider").and_then(Value::as_str),
        Some("future_gated")
    );
    assert_eq!(
        future_gated.get("editor_visible_next_edit_suggestions").and_then(Value::as_str),
        Some("future_gated")
    );
    assert_eq!(
        future_gated.get("live_lsp4ij_ui_automation").and_then(Value::as_str),
        Some("future_gated")
    );
    let next_edit_boundary = bundle
        .get("next_edit_boundary")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("next_edit_boundary map missing"))?;
    assert_eq!(next_edit_boundary.get("enabled_by_default").and_then(Value::as_bool), Some(false));
    assert_eq!(
        next_edit_boundary.get("explicit_dev_gate_enabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        next_edit_boundary.get("runtime_provider_registered").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        next_edit_boundary.get("editor_visible_suggestions").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        next_edit_boundary.get("ai_candidate_source_enabled").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        next_edit_boundary
            .get("default_response")
            .and_then(|response| response.get("status"))
            .and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        next_edit_boundary
            .get("explicit_gate_response")
            .and_then(|response| response.get("status"))
            .and_then(Value::as_str),
        Some("runtime_provider_not_registered")
    );

    Ok(())
}

fn assert_route_surface(
    routes: &Map<String, Value>,
    route: &str,
    path: &str,
    marker: &str,
) -> Result<()> {
    let route_entry = routes.get(route).ok_or_else(|| anyhow!("route {route} missing"))?;
    assert_eq!(
        route_entry.get("status").and_then(Value::as_str),
        Some("registered"),
        "route {route} should be registered"
    );
    let planes = route_entry
        .get("proof_planes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("route {route} proof_planes missing"))?;
    let mut searched_planes = Vec::new();
    for (plane_name, plane) in planes {
        let surfaces = plane
            .get("proof_surfaces")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("route {route} plane {plane_name} proof_surfaces missing"))?;
        if let Some(surface) = surfaces
            .iter()
            .find(|surface| surface.get("path").and_then(Value::as_str) == Some(path))
        {
            let markers =
                surface.get("required_markers").and_then(Value::as_array).ok_or_else(|| {
                    anyhow!(
                        "route {route} plane {plane_name} surface {path} required_markers missing"
                    )
                })?;
            if markers.iter().any(|value| value.as_str() == Some(marker)) {
                return Ok(());
            }
            searched_planes.push(plane_name.clone());
        }
    }
    Err(anyhow!(
        "route {route} has no proof surface {path} with marker {marker} (searched planes: {searched_planes:?})"
    ))
}

fn assert_plane<'a>(
    routes: &'a Map<String, Value>,
    route: &str,
    plane: &str,
    status: &str,
) -> Result<&'a Value> {
    let route_entry = routes.get(route).ok_or_else(|| anyhow!("route {route} missing"))?;
    let planes = route_entry
        .get("proof_planes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("route {route} proof_planes missing"))?;
    let plane_entry =
        planes.get(plane).ok_or_else(|| anyhow!("route {route} missing plane {plane}"))?;
    assert_eq!(
        plane_entry.get("status").and_then(Value::as_str),
        Some(status),
        "route {route} plane {plane} should be {status}"
    );
    Ok(plane_entry)
}
