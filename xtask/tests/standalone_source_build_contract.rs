use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const POLICY: &str = "policy/standalone-source-build.v1.toml";
const SCHEMA: &str = "schemas/standalone_source_build.v1.schema.json";
const FIXTURE_DIR: &str = "fixtures/experience/standalone_source_build";

fn root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask has no repository root").into())
}

fn read(root: &Path, path: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

fn json(root: &Path, path: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&read(root, path)?)?)
}

#[test]
fn policy_and_schema_pin_the_source_build_boundary() -> Result<(), Box<dyn Error>> {
    let root = root()?;
    let policy = read(&root, POLICY)?;
    let schema = json(&root, SCHEMA)?;
    for required in [
        "archive_failure_fallback = \"forbidden\"",
        "consent = \"required_before_code_execution\"",
        "build_scripts_and_proc_macros = \"may_execute_with_installer_authority\"",
        "security_sandbox_claim = \"not_proven_by_directory_isolation\"",
        "product_unit = \"advanced_source_server_only\"",
        "registry_credentials = \"never_durable_or_unrestricted\"",
        "private_values_in_receipts = \"forbidden\"",
    ] {
        assert!(policy.contains(required), "missing policy pin: {required}");
    }
    assert_eq!(
        schema["$id"],
        "https://github.com/EffortlessMetrics/perl-lsp-swarm/schemas/standalone_source_build.v1.schema.json"
    );
    assert_eq!(schema["properties"]["product_unit"]["enum"][0], "advanced_source_server_only");
    assert!(
        schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "profile"))
    );
    assert_eq!(
        schema["properties"]["execution"]["properties"]["archive_fallback"]["const"],
        "forbidden"
    );
    assert_eq!(
        schema["properties"]["execution"]["properties"]["isolation_claim"]["const"],
        "not_a_security_sandbox"
    );
    Ok(())
}

#[test]
fn deterministic_vectors_cover_exact_subject_and_fail_closed_edges() -> Result<(), Box<dyn Error>> {
    let root = root()?;
    let expected = [
        ("01_exact_plan.json", "accept", ""),
        ("02_latest_selector_rejected.json", "reject", "exact_version_required"),
        ("03_workspace_source_rejected.json", "reject", "workspace_source_forbidden"),
        ("04_checksum_mismatch.json", "reject", "source_checksum_mismatch"),
        ("05_lockfile_missing.json", "reject", "lockfile_missing_or_mismatch"),
        ("06_source_as_pair_rejected.json", "reject", "product_unit_mismatch"),
        ("07_implicit_fallback_rejected.json", "reject", "archive_failure_fallback_forbidden"),
        ("08_no_consent_rejected.json", "reject", "consent_required"),
        ("09_ambient_configuration_rejected.json", "reject", "ambient_configuration_rejected"),
        ("10_directory_is_not_sandbox.json", "reject", "security_sandbox_not_proven"),
        ("11_private_receipt_rejected.json", "reject", "private_output_leakage"),
    ];
    for (file, verdict, reason) in expected {
        let packet = json(&root, &format!("{FIXTURE_DIR}/{file}"))?;
        assert_eq!(packet["expectation"]["verdict"], verdict, "{file}");
        let actual_reason = semantic_reason(&packet["plan"]);
        if reason.is_empty() {
            let schema_document = json(&root, SCHEMA)?;
            let schema_validator = jsonschema::validator_for(&schema_document)?;
            assert!(
                schema_validator.is_valid(&packet["plan"]),
                "{file} must validate against the closed plan schema"
            );
            assert!(
                actual_reason.is_none(),
                "{file} unexpectedly has a rejection reason: {actual_reason:?}"
            );
        } else {
            assert_eq!(
                packet["expectation"]["reason_code"], reason,
                "{file} fixture reason must match the executable oracle"
            );
            assert_eq!(
                actual_reason,
                Some(reason),
                "{file} must be rejected from plan content, not its label"
            );
        }
        assert_eq!(packet["schema_version"], "standalone_source_build.vector.v1", "{file}");
    }
    Ok(())
}

fn semantic_reason(plan: &Value) -> Option<&'static str> {
    if plan["package"]["version"] == "latest" {
        return Some("exact_version_required");
    }
    if plan["source"]["kind"] != "registry_package" {
        return Some("workspace_source_forbidden");
    }
    if plan["package"]["checksum"] != plan["package"]["materialized_checksum"] {
        return Some("source_checksum_mismatch");
    }
    if plan["source"]["lockfile_sha256"].is_null() {
        return Some("lockfile_missing_or_mismatch");
    }
    if plan["product_unit"] != "advanced_source_server_only" {
        return Some("product_unit_mismatch");
    }
    if plan["execution"]["archive_fallback"] != "forbidden" {
        return Some("archive_failure_fallback_forbidden");
    }
    if plan["execution"]["performed"] == true && plan["execution"]["consent"] != "recorded" {
        return Some("consent_required");
    }
    if plan["execution"]["network_accessed"] == true
        && plan["network"]["materialization"] == "forbidden"
        && plan["network"]["build"] == "forbidden"
    {
        return Some("network_policy_contradiction");
    }
    if plan["configuration"]["inputs"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == "credential_helpers"))
    {
        return Some("ambient_configuration_rejected");
    }
    if plan["execution"]["isolation_claim"] != "not_a_security_sandbox" {
        return Some("security_sandbox_not_proven");
    }
    if plan["receipt_path"].as_str().is_some_and(|path| {
        let path = path.to_ascii_lowercase();
        path.contains("credential") || path.contains(".cargo") || path.contains("private")
    }) {
        return Some("private_output_leakage");
    }
    None
}

#[test]
fn vectors_keep_planning_distinct_from_execution() -> Result<(), Box<dyn Error>> {
    let root = root()?;
    let exact = json(&root, &format!("{FIXTURE_DIR}/01_exact_plan.json"))?;
    assert_eq!(exact["plan"]["execution"]["performed"], false);
    assert_eq!(exact["plan"]["execution"]["network_accessed"], false);
    assert_eq!(exact["plan"]["product_unit"], "advanced_source_server_only");
    assert_eq!(exact["plan"]["execution"]["archive_fallback"], "forbidden");
    assert_eq!(exact["plan"]["profile"], "release");
    let mut benign_receipt =
        json(&root, &format!("{FIXTURE_DIR}/11_private_receipt_rejected.json"))?["plan"].clone();
    benign_receipt["receipt_path"] = "receipt.json".into();
    assert_eq!(semantic_reason(&benign_receipt), None);
    let mut contradictory_network = exact["plan"].clone();
    contradictory_network["network"]["materialization"] = "forbidden".into();
    contradictory_network["network"]["build"] = "forbidden".into();
    contradictory_network["execution"]["network_accessed"] = true.into();
    assert_eq!(semantic_reason(&contradictory_network), Some("network_policy_contradiction"));
    Ok(())
}
