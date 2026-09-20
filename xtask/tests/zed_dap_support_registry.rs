//! Registry-shape checks for the generated Zed perl-dap support surface
//! (#9489, train stage D06).
//!
//! The support registry `policy/zed-dap-support.toml` and the generated
//! documentation `docs/EDITORS/ZED_DAP_SUPPORT.md` are projections of the
//! committed #9487 official-registry journey receipt: they may carry only the
//! cells the receipt earned, keep the Zed LSP row and the debugger surface
//! separate, and stay byte-identical to a second generation run. This suite
//! binds those invariants offline; the journey itself stays blocked_external
//! and no cell here claims otherwise.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCRIPT: &str = "scripts/zed_dap_asset_receipts.py";
const SUPPORT_POLICY: &str = "policy/zed-dap-support.toml";
const SUPPORT_DOCS: &str = "docs/EDITORS/ZED_DAP_SUPPORT.md";
const LSP_POLICY: &str = "policy/lsp-client-support.toml";
const ZED_SETUP: &str = "docs/EDITORS/ZED_SETUP.md";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn read(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(relative))?)
}

fn parse_toml(root: &Path, relative: &str) -> Result<toml::Value, Box<dyn Error>> {
    Ok(toml::from_str(&read(root, relative)?)?)
}

fn run_check(root: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("project-dap-support")
        .arg("--check")
        .current_dir(root)
        .output()?)
}

fn assert_success(output: &std::process::Output, context: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

fn policy_value(root: &Path, key: &str) -> Result<toml::Value, Box<dyn Error>> {
    let policy = parse_toml(root, SUPPORT_POLICY)?;
    policy
        .get(key)
        .cloned()
        .ok_or_else(|| io::Error::other(format!("support policy lacks `{key}`")).into())
}

#[test]
fn committed_projection_is_current_and_drift_free() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    assert_success(&run_check(&root)?, "checking the committed Zed DAP support projection")?;
    Ok(())
}

#[test]
fn second_generation_is_byte_identical() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let output = Command::new(python())
        .arg(root.join(SCRIPT))
        .arg("project-dap-support")
        .arg("--policy-output")
        .arg(root.join("target/zed-dap-support-second.toml"))
        .arg("--docs-output")
        .arg(root.join("target/zed-dap-support-second.md"))
        .current_dir(&root)
        .output()?;
    assert_success(&output, "regenerating the Zed DAP support projection")?;

    let committed_policy = read(&root, SUPPORT_POLICY)?;
    let committed_docs = read(&root, SUPPORT_DOCS)?;
    let regenerated_policy = fs::read_to_string(root.join("target/zed-dap-support-second.toml"))?;
    let regenerated_docs = fs::read_to_string(root.join("target/zed-dap-support-second.md"))?;
    assert_eq!(
        committed_policy, regenerated_policy,
        "second policy generation must produce no diff"
    );
    assert_eq!(
        committed_docs, regenerated_docs,
        "second documentation generation must produce no diff"
    );
    Ok(())
}

#[test]
fn adapter_identity_is_exact_and_never_aliases_a_language_server() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let adapter = policy_value(&root, "adapter")?;
    assert_eq!(
        adapter.get("adapter_id").and_then(toml::Value::as_str),
        Some("perl-dap"),
        "the debugger surface must carry the exact perl-dap adapter identity"
    );
    assert_eq!(
        adapter.get("executable").and_then(toml::Value::as_str),
        Some("perl-dap"),
        "the adapter executable identity must stay perl-dap"
    );
    let separate: Vec<&str> = adapter
        .get("separate_language_server_ids")
        .and_then(toml::Value::as_array)
        .map(|entries| entries.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    assert_eq!(
        separate,
        vec!["perlnavigator-server", "perl-lsp", "perllsp"],
        "the three Zed LSP server identities must stay listed outside the adapter identity"
    );
    assert!(
        !separate.contains(&"perl-dap"),
        "the adapter identity must stay outside the LSP identity family"
    );

    // The LSP registry row may not absorb the debugger surface.
    let lsp = parse_toml(&root, LSP_POLICY)?;
    let zed_row = lsp
        .get("client")
        .and_then(toml::Value::as_array)
        .and_then(|clients| {
            clients
                .iter()
                .find(|client| client.get("id").and_then(toml::Value::as_str) == Some("zed"))
        })
        .cloned()
        .ok_or_else(|| io::Error::other("the lsp client-support registry lacks its zed row"))?;
    let zed_row_text = zed_row.to_string();
    assert_eq!(
        zed_row.get("integration_mode").and_then(toml::Value::as_str),
        Some("extension_registered_language_server"),
        "the Zed LSP row must stay a language-server row"
    );
    assert!(
        !zed_row_text.contains("perl-dap") && !zed_row_text.contains("debug"),
        "the Zed LSP row must not absorb DAP cells; found: {zed_row_text}"
    );
    Ok(())
}

#[test]
fn stages_routes_and_platform_rows_stay_distinct() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let stages = policy_value(&root, "stage")?
        .as_array()
        .cloned()
        .ok_or_else(|| io::Error::other("stage must be an array of tables"))?;
    let stage_ids: Vec<&str> =
        stages.iter().filter_map(|stage| stage.get("id").and_then(toml::Value::as_str)).collect();
    assert_eq!(
        stage_ids,
        vec![
            "configuration_static_adapter_authority",
            "exact_source_dev_extension",
            "public_registry_install",
        ],
        "the three evidence stages must stay present and distinct"
    );
    let state_of = |stage_id: &str| {
        stages
            .iter()
            .find(|stage| stage.get("id").and_then(toml::Value::as_str) == Some(stage_id))
            .and_then(|stage| stage.get("state").and_then(toml::Value::as_str))
            .map(str::to_string)
    };
    assert_eq!(
        state_of("configuration_static_adapter_authority").as_deref(),
        Some("registered_static_authority")
    );
    assert_eq!(state_of("exact_source_dev_extension").as_deref(), Some("not_proven"));
    assert_eq!(state_of("public_registry_install").as_deref(), Some("blocked_external"));

    let routes = policy_value(&root, "binary_route")?
        .as_array()
        .cloned()
        .ok_or_else(|| io::Error::other("binary_route must be an array of tables"))?;
    let route_ids: Vec<&str> =
        routes.iter().filter_map(|route| route.get("id").and_then(toml::Value::as_str)).collect();
    assert_eq!(
        route_ids,
        vec!["managed_download", "path"],
        "managed and PATH routes must remain distinct rows"
    );
    for route in &routes {
        assert_eq!(
            route.get("state").and_then(toml::Value::as_str),
            Some("not_proven"),
            "no binary route may claim support from the blocked public journey"
        );
    }
    assert_ne!(
        routes[0].get("boundary").and_then(toml::Value::as_str),
        routes[1].get("boundary").and_then(toml::Value::as_str),
        "managed and PATH boundaries must stay distinct"
    );

    let platform = policy_value(&root, "platform")?;
    assert_eq!(platform.get("os").and_then(toml::Value::as_str), Some("windows"));
    assert_eq!(platform.get("architecture").and_then(toml::Value::as_str), Some("x86_64"));
    assert_eq!(
        platform.get("cross_platform_promotion").and_then(toml::Value::as_str),
        Some("denied"),
        "one platform/architecture must not promote another"
    );
    assert_eq!(
        platform
            .get("other_os_architecture_rows")
            .and_then(|rows| rows.get("state"))
            .and_then(toml::Value::as_str),
        Some("not_observed")
    );
    Ok(())
}

#[test]
fn unearned_cells_stay_not_proven_and_visible() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let cells = policy_value(&root, "cell")?
        .as_array()
        .cloned()
        .ok_or_else(|| io::Error::other("cell must be an array of tables"))?;
    assert_eq!(cells.len(), 9, "every D05 journey cell must be consumed");
    for cell in &cells {
        assert_eq!(
            cell.get("exact_source_dev_extension").and_then(toml::Value::as_str),
            Some("not_proven"),
            "cell {:?} has no exact-source receipt and must stay not_proven",
            cell.get("id")
        );
        assert_eq!(
            cell.get("public_registry_install").and_then(toml::Value::as_str),
            Some("not_proven"),
            "cell {:?} cannot outrun the blocked public journey",
            cell.get("id")
        );
        assert_eq!(
            cell.get("static_authority").and_then(toml::Value::as_str),
            Some("configuration_only"),
            "static authority may not read as a behavior claim"
        );
    }
    let launch = cells
        .iter()
        .find(|cell| {
            cell.get("id").and_then(toml::Value::as_str) == Some("session_initialize_launch")
        })
        .ok_or_else(|| io::Error::other("the session cell is missing"))?;
    assert_eq!(
        launch.get("session_kind").and_then(toml::Value::as_str),
        Some("launch"),
        "the session cell must record the exact launch session kind"
    );

    let boundary = policy_value(&root, "claim_boundary")?;
    assert_eq!(boundary.get("support_tier").and_then(toml::Value::as_str), Some("not_proven"));
    assert_eq!(
        boundary.get("public_support_requires_issue").and_then(toml::Value::as_integer),
        Some(9487),
        "public debugger support must require the #9487 journey"
    );
    assert_eq!(
        boundary.get("exact_source_alone_cannot_promote").and_then(toml::Value::as_bool),
        Some(true),
        "#9486 exact-source evidence alone cannot create public support"
    );
    assert_eq!(
        boundary.get("lsp_support_rows").and_then(toml::Value::as_str),
        Some("unchanged"),
        "the DAP projection must not disturb the LSP rows"
    );

    let currentness = policy_value(&root, "currentness")?;
    let blockers = currentness
        .get("blockers")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("currentness.blockers must stay visible"))?;
    assert!(
        !blockers.is_empty(),
        "unsupported/not-proven cells must remain visible with their blockers"
    );
    let gates = policy_value(&root, "gates")?;
    assert_eq!(
        gates.get("matching_host_asset_receipt").and_then(toml::Value::as_str),
        Some("current")
    );
    for absent_gate in [
        "released_zed_build",
        "official_registry_entry",
        "extension_upstream_release",
        "exact_source_zed_dap_receipt",
        "routing_final_check_authority",
    ] {
        assert_eq!(
            gates.get(absent_gate).and_then(toml::Value::as_str),
            Some("absent"),
            "gate {absent_gate} must stay honestly absent"
        );
    }
    Ok(())
}

#[test]
fn generated_docs_and_lsp_guide_stay_independent() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let docs = read(&root, SUPPORT_DOCS)?;
    assert!(
        docs.contains("**Status: planned / not proven.**"),
        "the generated debugger docs must keep the planned/not-proven status"
    );
    assert!(
        docs.contains("presence of `perl-dap` in a release archive is not session proof"),
        "the generated docs must reject archive presence as session proof"
    );
    assert!(
        docs.contains("\"request\": \"launch\""),
        "the generated docs must show the exact supported launch configuration shape"
    );
    assert!(
        docs.contains("#9486 exact-source evidence alone cannot"),
        "the generated docs must keep the exact-source/public boundary"
    );
    assert!(
        docs.contains("never alters the LSP verdict"),
        "the generated docs must keep the LSP/DAP independence rule"
    );
    assert!(
        docs.contains("One platform cannot promote"),
        "the generated docs must deny cross-platform promotion"
    );

    let setup = read(&root, ZED_SETUP)?;
    assert!(
        setup.contains("## Debugger support (perl-dap)"),
        "the Zed setup guide must point at the separate debugger surface"
    );
    assert!(
        setup.contains("[ZED_DAP_SUPPORT.md](ZED_DAP_SUPPORT.md)"),
        "the Zed setup guide must link the generated debugger surface"
    );
    assert!(
        setup.contains("**Status: planned / not proven.**"),
        "the LSP guide status must remain independent of the debugger section"
    );
    assert!(
        setup.contains("never alters the debugger cells"),
        "the pointer section must keep the two surfaces independent in both directions"
    );
    Ok(())
}
