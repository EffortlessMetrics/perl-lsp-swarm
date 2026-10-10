//! Compilation staging cannot substitute for the routed runtime proof (#17459).
#[path = "../src/tasks/gates/routed_preparation.rs"]
mod routed_preparation;

use serde_yaml_ng::Value;
use std::{fs, path::PathBuf};

fn root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing workspace root")?
        .to_path_buf())
}

#[test]
fn preparation_preserves_runtime_command_and_budget() -> Result<(), Box<dyn std::error::Error>> {
    let policy: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root()?.join(".ci/gate-policy.yaml"))?)?;
    let gates = policy["gates"].as_sequence().ok_or("missing gates")?;
    let find =
        |name| gates.iter().find(|gate| gate["name"].as_str() == Some(name)).ok_or("missing gate");
    let runtime = find("unit_routed_full")?;
    let build = find("unit_routed_full_build")?;
    let command = runtime["command"].as_str().ok_or("missing runtime command")?;
    assert_eq!(
        command,
        "cargo build -p perllsp --locked && env PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" cargo test --locked --tests {package_args}"
    );
    assert_eq!(
        build["command"].as_str().and_then(|value| value.strip_suffix(" --no-run")),
        Some(command)
    );
    assert!(!command.contains("--no-run"));
    for key in ["tier", "required", "quarantine", "planning", "timeout_seconds"] {
        assert_eq!(build[key], runtime[key], "compile and runtime must share {key}");
    }
    assert_eq!(runtime["timeout_seconds"].as_u64(), Some(1500));
    assert_eq!(runtime["retry_count"].as_u64(), Some(1));
    assert_eq!(build["retry_count"].as_u64(), Some(0));
    let dap = find("dap_workspace_prepare")?;
    assert_eq!(
        dap["command"].as_str(),
        Some(
            "cargo build -p perl-lsp-rs --message-format=short --locked && cargo build -p perl-lsp-rs-core --message-format=short --locked && cargo build -p perl-dap --bin perl-dap --locked && cargo clippy -p perl-dap --lib --locked -- -D warnings -A clippy::wildcard_imports"
        )
    );
    assert_eq!(dap["required"].as_bool(), Some(true));
    assert_eq!(dap["quarantine"].as_bool(), Some(false));
    assert_eq!(dap["retry_count"].as_u64(), Some(0));
    assert_eq!(dap["timeout_seconds"].as_u64(), Some(1500));
    assert_eq!(dap["planning"]["role"].as_str(), Some("rust_package_scoped"));
    assert_eq!(dap["planning"]["packages"], serde_yaml_ng::from_str::<Value>("[perl-dap]")?);
    let index = |name| gates.iter().position(|row| row["name"].as_str() == Some(name));
    assert!(index("unit_routed_full_build") < index("dap_workspace_prepare"));
    assert!(index("dap_workspace_prepare") < index("unit_routed_full"));
    assert_eq!(find("fmt")?["command"].as_str(), Some("cargo xtask fmt --check"));
    assert_eq!(find("fmt")?["required"].as_bool(), Some(true));
    let parser = find("parser_workspace_prepare")?;
    assert_eq!(parser["command"].as_str(), Some("python scripts/ci/parser_workspace_prepare.py"));
    assert_eq!(parser["required"].as_bool(), Some(true));
    assert_eq!(parser["retry_count"].as_u64(), Some(0));
    assert_eq!(parser["quarantine"].as_bool(), Some(false));
    assert_eq!(parser["planning"]["packages"], serde_yaml_ng::from_str::<Value>("[perl-parser]")?);
    assert!(index("parser_workspace_prepare") < index("unit_routed_full"));

    assert!(!root()?.join("crates/perl-dap/tests/wave_h_workspace_verification.rs").exists());
    let manifest = fs::read_to_string(root()?.join("crates/perl-dap/Cargo.toml"))?;
    assert!(!manifest.contains("wave_h_workspace_verification"));
    let runner = fs::read_to_string(root()?.join("xtask/src/tasks/gates.rs"))?;
    assert!(runner.contains("routed_preparation::retain_for_filter("));
    assert!(runner.contains("routed_preparation::blocking_failure("));
    Ok(())
}

#[test]
fn preparation_precedes_watchdog_and_has_separate_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root()?.join(".github/workflows/ci.yml"))?)?;
    let required = &workflow["jobs"]["check-all-targets"];
    assert_eq!(required["name"].as_str(), Some("Compile All Targets (bit-rot guard)"));
    let required_steps = required["steps"].as_sequence().ok_or("missing required steps")?;
    let route = required_steps
        .iter()
        .find(|step| {
            step["name"].as_str()
                == Some("Routed test preparation contract (required merge surface)")
        })
        .ok_or("missing required preparation contract route")?;
    let route_command = route["run"].as_str().ok_or("missing required contract command")?;
    assert!(
        route_command
            .contains("cargo test -p xtask --test routed_test_preparation_contract --locked --")
    );
    assert!(route_command.contains("16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"));
    assert!(route_command.contains("preparation_preserves_runtime_command_and_budget"));
    assert!(route_command.contains("preparation_precedes_watchdog_and_has_separate_evidence"));
    assert_eq!(route["continue-on-error"], Value::Null);
    let job = &workflow["jobs"]["pr-smoke"];
    assert_eq!(job["timeout-minutes"].as_u64(), Some(75));
    let steps = job["steps"].as_sequence().ok_or("missing steps")?;
    let prepare = steps
        .iter()
        .position(|step| step["name"].as_str() == Some("Prepare routed integration-test targets"))
        .ok_or("missing preparation")?;
    let execute = steps
        .iter()
        .position(|step| step["name"].as_str() == Some("Run PR-fast via shared xtask gate runner"))
        .ok_or("missing runtime")?;
    assert!(prepare < execute);
    assert_eq!(steps[prepare]["id"].as_str(), Some("routed-test-preparation"));
    assert_eq!(
        steps[execute]["env"]["PR_SMOKE_ROUTED_PREPARATION_OUTCOME"].as_str(),
        Some("${{ steps.routed-test-preparation.outcome }}")
    );
    let build = steps[prepare]["run"].as_str().ok_or("missing preparation command")?;
    assert!(
        build.contains("--gate unit_routed_full_build --subject target/receipts/ci-subject.json")
    );
    assert!(build.contains("--receipt-path target/receipts/routed-test-build/receipt.json"));
    assert!(!build.contains("--receipt-path target/receipts/receipt.json"));
    assert!(!build.contains("|| true"));
    assert!(build.contains("preparation_status=$?"));
    assert!(build.contains("exit \"$preparation_status\""));
    assert!(build.contains("target/receipts/routed-test-build/logs/unit_routed_full_build.log"));
    assert_eq!(
        steps[prepare]["continue-on-error"].as_bool(),
        Some(true),
        "failed preparation must retain its receipt and allow all independent full-tier proofs"
    );
    let runtime = steps[execute]["run"].as_str().ok_or("missing runtime command")?;
    assert!(runtime.contains("timeout --signal=TERM --kill-after=60s 3600s"));
    assert!(
        runtime
            .contains("gates --tier pr-fast --subject target/receipts/ci-subject.json --receipt")
    );
    assert!(!runtime.contains("--gate unit_routed_full_build"));
    let upload = steps
        .iter()
        .find(|step| {
            step["with"]["name"].as_str().is_some_and(|name| name.starts_with("pr-fast-receipt-"))
        })
        .ok_or("missing receipt artifact")?;
    let paths = upload["with"]["path"].as_str().ok_or("missing artifact paths")?;
    assert!(
        paths.lines().any(|path| path.trim() == "target/receipts/routed-test-build/receipt.json")
    );
    assert!(paths.lines().any(|path| path.trim() == "target/receipts/logs/*.log"));
    assert!(
        paths.lines().any(|path| path.trim() == "target/receipts/routed-test-build/logs/*.log")
    );
    Ok(())
}
