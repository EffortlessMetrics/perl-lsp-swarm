//! Synthetic receipts for validator discrimination only; no editor or release was run.

use serde_json::{Value, json};
use std::{error::Error, fs, io, path::Path};

pub fn host(root: &Path, contract: &Value) -> Result<Value, Box<dyn Error>> {
    let mut receipt: Value = serde_json::from_slice(&fs::read(
        root.join(".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json"),
    )?)?;
    let target = contract
        .pointer("/targets/0")
        .ok_or_else(|| io::Error::other("fixture contract lacks first target"))?;
    receipt["result"] = json!("pass");
    receipt["limitations"] = json!(["Synthetic validator fixture; no actual Zed host was run."]);
    receipt["claim_boundary"] = json!("Synthetic acceptance shape only; no Zed support claim.");
    receipt["observed_at"] = json!("2026-08-15T00:00:00Z");
    receipt["zed"]["version"] = json!("0.0.0-test");
    receipt["zed"]["channel"] = json!("stable");
    receipt["zed"]["build"] = json!("synthetic-test-build");
    receipt["extension"]["candidate_commit"] = json!("f".repeat(40));
    receipt["extension"]["wasm_sha256"] = json!(format!("sha256:{}", "a".repeat(64)));
    receipt["perllsp"]["command"] = json!(format!(
        "/synthetic-cache/{}",
        target
            .get("installed_path")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("fixture lacks install path"))?
    ));
    receipt["perllsp"]["version"] = contract["source"]["version"].clone();
    receipt["perllsp"]["build_commit"] = json!("d".repeat(40));
    receipt["perllsp"]["binary_sha256"] = json!(format!("sha256:{}", "b".repeat(64)));
    receipt["perllsp"]["resolution_route"] = json!("managed_download");
    receipt["platform"] =
        json!({"os": target["os"], "architecture": target["architecture"], "version": "synthetic"});
    receipt["profile"] = json!({"clean_profile": true, "prior_extension_absent": true,
        "prior_managed_cache_absent": true, "other_perl_servers_disabled": true});
    receipt["workspace"] = json!({"fixture_id": "synthetic-zed-managed-v1",
        "fixture_sha256": format!("sha256:{}", "c".repeat(64)), "root_identity": "synthetic"});
    receipt["configuration"]["settings_sha256"] = json!(format!("sha256:{}", "d".repeat(64)));
    receipt["configuration"]["workspace_configuration_observed"] = json!(true);
    receipt["artifacts"] = json!({"zed_log": "synthetic/zed.log", "language_server_log": "synthetic/lsp.log",
        "process_inventory": "synthetic/process.json", "redacted": true});
    let journeys = receipt
        .get_mut("journey")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("host template lacks journeys"))?;
    for cell in journeys.values_mut() {
        *cell = json!({"result": "pass", "evidence": "synthetic validator fixture only"});
    }
    receipt["activation"]["pod"] =
        json!({"result": "pass", "evidence": "synthetic POD separation"});
    Ok(receipt)
}

pub fn asset(contract: &Value, contract_digest: &str) -> Result<Value, Box<dyn Error>> {
    let source = &contract["source"];
    let mut targets = Vec::new();
    for expected in contract
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("asset contract lacks targets"))?
    {
        let mut row = json!({"target": expected["target"], "disposition": expected["disposition"],
            "os": expected["os"], "architecture": expected["architecture"]});
        if expected["disposition"] == "managed" {
            let name = expected
                .get("archive_member")
                .and_then(Value::as_str)
                .and_then(|member| member.rsplit('/').next())
                .ok_or_else(|| io::Error::other("asset contract lacks member"))?;
            row["result"] = json!("managed_extracted_not_executed");
            row["asset"] = json!({"id": expected["asset_id"], "name": expected["asset_name"],
                "size": expected["asset_size"], "sha256": expected["asset_digest"], "archive_type": expected["archive_type"]});
            row["archive"] = json!({"members_sha256": format!("sha256:{}", "e".repeat(64)),
                "required_member": expected["archive_member"], "installed_name": name, "safe": true});
            row["binary"] = json!({"name": name, "sha256": format!("sha256:{}", "b".repeat(64)), "executable": true});
            row["stdio_smoke"] = json!({"result": "not_executed"});
        }
        targets.push(row);
    }
    Ok(json!({"schema_version": "zed_managed_asset_receipt.v1", "result": "pass",
        "contract": {"sha256": contract_digest},
        "release": {"repository": source["repository"], "id": source["release_id"],
            "tag": source["tag"], "version": source["version"], "prerelease": false, "draft": false},
        "targets": targets,
        "claim_boundary": {"actual_zed": "not_proven", "public_registry": "not_proven",
            "host_process": "not_executed_on_this_verifier"}}))
}
