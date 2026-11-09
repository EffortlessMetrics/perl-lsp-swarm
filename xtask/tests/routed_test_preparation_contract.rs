//! Compilation staging cannot substitute for the routed runtime proof (#17459).
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
    assert!(route_command.contains("2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"));
    assert!(route_command.contains("preparation_preserves_runtime_command_and_budget"));
    assert!(route_command.contains("preparation_precedes_watchdog_and_has_separate_evidence"));
    assert_eq!(route["continue-on-error"], Value::Null);
    // The revival sibling (pr-fast-unblocked, #19045) copies the preparation
    // and watchdog shape so the tier runs without the fatal disk floor.
    // Assert both jobs stay in lockstep; a one-sided edit must fail here,
    // not drift silently.
    for (job_name, receipt_prefix, missing_prefix) in [
        ("pr-smoke", "pr-fast-receipt-", "pr-fast-missing-receipt-"),
        (
            "pr-fast-unblocked",
            "pr-fast-unblocked-receipt-",
            "pr-fast-unblocked-missing-receipt-",
        ),
    ] {
        let job = &workflow["jobs"][job_name];
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
        assert_eq!(
            steps[prepare]["timeout-minutes"].as_u64(),
            Some(25),
            "preparation must be bounded inside the 75m job envelope"
        );
        assert!(build.contains("routed-test-build/logs/unit_routed_full_build.log"));
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
                step["with"]["name"].as_str().is_some_and(|name| name.starts_with(receipt_prefix))
            })
            .ok_or("missing receipt artifact")?;
        let upload_if = upload["if"].as_str().ok_or("missing receipt upload guard")?;
        assert!(
            upload_if.contains("hashFiles('target/receipts/receipt.json') != ''"),
            "tier-named receipt upload must require receipt.json (#17915)"
        );
        let missing = steps
            .iter()
            .find(|step| {
                step["with"]["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(missing_prefix))
            })
            .ok_or("missing missing-receipt diagnostics artifact")?;
        let missing_if = missing["if"].as_str().ok_or("missing diagnostics upload guard")?;
        assert!(
            missing_if.contains("hashFiles('target/receipts/receipt.json') == ''"),
            "missing-receipt diagnostics must publish only without receipt.json"
        );
        let paths = upload["with"]["path"].as_str().ok_or("missing artifact paths")?;
        assert!(
            paths.lines().any(|path| path.trim() == "target/receipts/routed-test-build/receipt.json")
        );
        assert!(paths.lines().any(|path| path.trim() == "target/receipts/logs/*.log"));
        assert!(
            paths.lines().any(|path| path.trim() == "target/receipts/routed-test-build/logs/*.log")
        );
    }
    Ok(())
}
