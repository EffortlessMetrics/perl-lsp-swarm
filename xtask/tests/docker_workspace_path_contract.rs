//! Integration tests for `policy/docker-workspace-path.v1.toml` (#8987).

use std::fs;
use std::path::{Path, PathBuf};

use xtask::docker_workspace_path::{
    PlanError, WorkspaceFolder, build_host_editor_plan, load_contract, reject_lossy_host_remap,
    validate_documentation_pointers,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn contract_and_documentation_pointers_exist() -> anyhow::Result<()> {
    let root = repo_root();
    let contract = load_contract(&root)?;
    assert_eq!(contract.topologies.len(), 2);
    validate_documentation_pointers(&root)?;
    Ok(())
}

#[test]
fn fixture_cases_match_mount_contract() -> anyhow::Result<()> {
    let root = repo_root();
    let contract = load_contract(&root)?;
    let raw = fs::read_to_string(root.join("fixtures/docker_workspace_path/cases.toml"))?;
    let table: toml::Table = raw.parse()?;
    let cases = table
        .get("case")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing [[case]] rows"))?;

    for case in cases {
        let id = case.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        if id == "lossy_remap_negative" {
            let host = case.get("root_path").and_then(|v| v.as_str()).unwrap();
            let bad = case.get("bad_container_mount").and_then(|v| v.as_str()).unwrap();
            let err = reject_lossy_host_remap(Path::new(host), Path::new(bad))
                .expect_err("lossy fixture must fail");
            assert!(matches!(err, PlanError::LossyHostRemap { .. }));
            continue;
        }
        if id == "multi_root_refused" {
            let roots = case.get("roots").and_then(|v| v.as_array()).unwrap();
            let folders: Vec<WorkspaceFolder> = roots
                .iter()
                .filter_map(|v| v.as_str())
                .map(|path| WorkspaceFolder {
                    uri: format!("file://{path}"),
                    absolute_path: PathBuf::from(path),
                })
                .collect();
            let err = build_host_editor_plan(&contract, &folders, None).expect_err("multi-root");
            assert_eq!(err, PlanError::MultiRootHostWrapperUnsupported);
            continue;
        }
        if id == "host_editor_single_root" {
            let root_path = case.get("root_path").and_then(|v| v.as_str()).unwrap();
            let rel = case.get("unopened_relative").and_then(|v| v.as_str()).unwrap();
            let expect_path = case.get("expect_container_path").and_then(|v| v.as_str()).unwrap();
            let plan = build_host_editor_plan(
                &contract,
                &[WorkspaceFolder {
                    uri: case.get("root_uri").and_then(|v| v.as_str()).unwrap().into(),
                    absolute_path: PathBuf::from(root_path),
                }],
                None,
            )?;
            let projected = plan.dap_source_path(Path::new(root_path), Path::new(rel));
            assert_eq!(projected, PathBuf::from(expect_path));
        }
    }
    Ok(())
}

#[test]
fn host_launcher_script_uses_same_path_bind() -> anyhow::Result<()> {
    let script = fs::read_to_string(repo_root().join("scripts/docker/run-perllsp-host-editor.sh"))?;
    assert!(script.contains("--init"));
    assert!(script.contains("src=${WORKSPACE_ROOT},dst=${WORKSPACE_ROOT},readonly"));
    assert!(script.contains("--workdir"));
    assert!(!script.contains(":/workspace"));
    Ok(())
}

#[test]
fn release_process_points_at_host_launcher() -> anyhow::Result<()> {
    let doc = fs::read_to_string(repo_root().join("docs/RELEASE_PROCESS.md"))?;
    assert!(
        doc.contains("run-perllsp-host-editor.sh") || doc.contains("same-path bind"),
        "RELEASE_PROCESS must not present /workspace as the universal host-editor command"
    );
    Ok(())
}
