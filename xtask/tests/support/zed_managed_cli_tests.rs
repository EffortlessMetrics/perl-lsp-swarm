//! Exercise the built executable, including both existing upstream authorities.

use super::{CONTRACT, TEMPLATE, read_json, repo_root, valid_pass};
use serde_json::{Value, json};
use std::{
    error::Error,
    fs, io,
    path::Path,
    process::{Command, Output},
};

#[path = "zed_managed_bindings.rs"]
mod bindings;
#[path = "zed_managed_fixtures.rs"]
mod fixtures;

struct Inputs {
    directory: tempfile::TempDir,
    managed: Value,
    asset: Value,
    host: Value,
    contract: Vec<u8>,
    asset_contract: Vec<u8>,
}

impl Inputs {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = repo_root()?;
        let contract = fs::read(root.join(CONTRACT))?;
        let asset_contract =
            fs::read(root.join(".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json"))?;
        let asset_value: Value = serde_json::from_slice(&asset_contract)?;
        let asset = fixtures::asset(&asset_value, &bindings::content_sha256(&asset_contract))?;
        let host = fixtures::host(&root, &asset_value)?;
        let mut managed = read_json(&root, TEMPLATE)?;
        valid_pass(&mut managed)?;
        managed["contract"]["sha256"] = json!(bindings::content_sha256(&contract));
        for (key, pointer) in [
            ("zed_version", "/zed/version"),
            ("zed_build", "/zed/build"),
            ("extension_version", "/extension/manifest_version"),
            ("extension_candidate_commit", "/extension/candidate_commit"),
            ("extension_wasm_sha256", "/extension/wasm_sha256"),
            ("fixture_id", "/workspace/fixture_id"),
            ("fixture_sha256", "/workspace/fixture_sha256"),
            ("version", "/perllsp/version"),
        ] {
            managed["subject"][key] = host
                .pointer(pointer)
                .cloned()
                .ok_or_else(|| io::Error::other(format!("fixture lacks {pointer}")))?;
        }
        managed["subject"]["target"] = asset_value["targets"][0]["target"].clone();
        managed["subject"]["installed_path"] = asset_value["targets"][0]["installed_path"].clone();
        managed["subject"]["asset_sha256"] = asset_value["targets"][0]["asset_digest"].clone();
        // valid_pass uses the same synthetic installed digest as the upstream fixtures.
        let mut inputs = Self {
            directory: tempfile::Builder::new().prefix("zed managed inputs ").tempdir()?,
            managed,
            asset,
            host,
            contract,
            asset_contract,
        };
        inputs.bind_upstream()?;
        bindings::validate_bound_pass(
            &inputs.managed,
            &serde_json::to_vec(&inputs.asset)?,
            &serde_json::to_vec(&inputs.host)?,
            &inputs.asset_contract,
        )
        .map_err(io::Error::other)?;
        Ok(inputs)
    }

    fn bind_upstream(&mut self) -> Result<(), Box<dyn Error>> {
        self.managed["upstream"]["asset_receipt_sha256"] =
            json!(bindings::content_sha256(&serde_json::to_vec(&self.asset)?));
        self.managed["upstream"]["host_receipt_sha256"] =
            json!(bindings::content_sha256(&serde_json::to_vec(&self.host)?));
        Ok(())
    }

    fn run(&self, upstream: bool, python: Option<&Path>) -> Result<Output, Box<dyn Error>> {
        let directory = self.directory.path();
        for (name, bytes) in [
            ("managed.json", serde_json::to_vec(&self.managed)?),
            ("assets.json", serde_json::to_vec(&self.asset)?),
            ("host.json", serde_json::to_vec(&self.host)?),
            ("contract.json", self.contract.clone()),
            ("asset-contract.json", self.asset_contract.clone()),
        ] {
            fs::write(directory.join(name), bytes)?;
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_validate-zed-managed-route"));
        command
            .arg("--contract")
            .arg(directory.join("contract.json"))
            .arg("--receipt")
            .arg(directory.join("managed.json"));
        if upstream {
            command
                .arg("--asset-receipt")
                .arg(directory.join("assets.json"))
                .arg("--host-receipt")
                .arg(directory.join("host.json"))
                .arg("--asset-contract")
                .arg(directory.join("asset-contract.json"));
        }
        if let Some(python) = python {
            command.arg("--python").arg(python);
        }
        Ok(command.output()?)
    }
}

fn require(output: Output, success: bool, diagnostic: &str) -> Result<(), Box<dyn Error>> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() != success
        || (!diagnostic.is_empty() && !stderr.contains(diagnostic))
    {
        return Err(
            io::Error::other(format!("unexpected CLI result {}: {stderr}", output.status)).into()
        );
    }
    Ok(())
}

#[test]
fn built_cli_composes_real_authorities_and_needs_no_python_for_template()
-> Result<(), Box<dyn Error>> {
    let mut inputs = Inputs::new()?;
    require(inputs.run(true, None)?, true, "")?;
    require(inputs.run(false, None)?, false, "--asset-receipt")?;
    let missing = inputs.directory.path().join("missing-python");
    require(inputs.run(true, Some(&missing))?, false, "cannot run existing Python")?;
    inputs.managed = read_json(&repo_root()?, TEMPLATE)?;
    require(inputs.run(false, Some(&missing))?, true, "")?;
    require(
        Command::new(env!("CARGO_BIN_EXE_validate-zed-managed-route"))
            .current_dir(repo_root()?)
            .arg("--python")
            .arg(&missing)
            .output()?,
        true,
        "",
    )?;
    Ok(())
}

#[test]
fn built_cli_rejects_missing_and_changed_input_bytes() -> Result<(), Box<dyn Error>> {
    let mut inputs = Inputs::new()?;
    inputs.contract.push(b' ');
    require(inputs.run(true, None)?, false, "contract digest mismatch")?;
    inputs.contract.pop();
    inputs.asset["test_changed_bytes"] = json!(true);
    require(inputs.run(true, None)?, false, "upstream receipt digest mismatch")?;
    inputs
        .asset
        .as_object_mut()
        .ok_or_else(|| io::Error::other("asset object"))?
        .remove("test_changed_bytes");
    inputs.host["limitations"] = json!(["different captured bytes"]);
    require(inputs.run(true, None)?, false, "upstream receipt digest mismatch")?;
    let mut inputs = Inputs::new()?;
    inputs.asset_contract.push(b' ');
    require(inputs.run(true, None)?, false, "asset receipt authority rejected")?;
    Ok(())
}

#[test]
fn built_cli_rejects_invalid_upstream_receipts_after_rebinding() -> Result<(), Box<dyn Error>> {
    let mut inputs = Inputs::new()?;
    inputs.asset["targets"][0]["archive"]["safe"] = json!(false);
    inputs.bind_upstream()?;
    require(inputs.run(true, None)?, false, "asset receipt authority rejected")?;
    let mut inputs = Inputs::new()?;
    inputs.host["journey"]["hover"]["result"] = json!("not_proven");
    inputs.bind_upstream()?;
    require(inputs.run(true, None)?, false, "hover")?;
    let mut inputs = Inputs::new()?;
    inputs.asset["result"] = json!("not_run");
    inputs.bind_upstream()?;
    require(inputs.run(true, None)?, false, "passing asset receipt")?;
    Ok(())
}

#[test]
fn built_cli_rejects_rebound_subject_mismatches() -> Result<(), Box<dyn Error>> {
    for (pointer, replacement) in [
        ("/subject/target", json!("aarch64-pc-windows-msvc")),
        ("/subject/zed_build", json!("other-build")),
        ("/subject/extension_candidate_commit", json!("e".repeat(40))),
        ("/subject/extension_wasm_sha256", json!(format!("sha256:{}", "f".repeat(64)))),
        ("/subject/fixture_sha256", json!(format!("sha256:{}", "f".repeat(64)))),
        ("/subject/asset_sha256", json!(format!("sha256:{}", "f".repeat(64)))),
        ("/subject/installed_path", json!("other/perllsp")),
    ] {
        let mut inputs = Inputs::new()?;
        *inputs
            .managed
            .pointer_mut(pointer)
            .ok_or_else(|| io::Error::other(pointer.to_string()))? = replacement;
        require(inputs.run(true, None)?, false, "")?;
    }
    for (pointer, replacement) in [
        ("/perllsp/resolution_route", json!("worktree_path")),
        ("/perllsp/binary_sha256", json!(format!("sha256:{}", "f".repeat(64)))),
        ("/perllsp/command", json!("/unmanaged/perllsp")),
        ("/platform/os", json!("macos")),
    ] {
        let mut inputs = Inputs::new()?;
        *inputs.host.pointer_mut(pointer).ok_or_else(|| io::Error::other(pointer.to_string()))? =
            replacement;
        inputs.bind_upstream()?;
        require(inputs.run(true, None)?, false, "")?;
    }
    for (pointer, replacement) in [
        ("/release/id", json!(123)),
        ("/targets/0/os", json!("macos")),
        ("/targets/0/asset/id", json!(123)),
        ("/targets/0/archive/required_member", json!("other/perllsp")),
    ] {
        let mut inputs = Inputs::new()?;
        *inputs
            .asset
            .pointer_mut(pointer)
            .ok_or_else(|| io::Error::other(pointer.to_string()))? = replacement;
        inputs.bind_upstream()?;
        require(inputs.run(true, None)?, false, "subject mismatch")?;
    }
    Ok(())
}
