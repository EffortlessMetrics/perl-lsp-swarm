#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use anyhow::{Result, ensure};
use perl_test_must::must_some_with;
use serde_json::Value;
use std::process::Command;

const RESULT_SCHEMA_VERSION: &str = "claude_host_test.v1";
const USER_ACTION_IDENTITY: &str = "perllsp.doctor.client.claude.host_test";
const USAGE_DISCLOSURE: &str = "This diagnostic invokes actual authenticated Claude Code and may consume user entitlement/usage. No project source is selected. Host testing is never run by ordinary doctor, status, setup, or completion.";

fn run_perllsp(args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new(env!("CARGO_BIN_EXE_perllsp")).args(args).output()?;
    Ok(output)
}

mod claude_host_test_contract {
    use super::*;

    #[test]
    fn cli_host_test_json_is_opt_in_and_does_not_require_claude() -> Result<()> {
        let output = run_perllsp(&["doctor", "--client", "claude", "--host-test", "--json"])?;
        ensure!(!output.status.success(), "reserved host-test should not exit 0");
        let stdout = String::from_utf8(output.stdout)?;
        ensure!(stdout.contains(RESULT_SCHEMA_VERSION));
        ensure!(stdout.contains(USAGE_DISCLOSURE));
        ensure!(stdout.contains("instrument_not_proven"));
        ensure!(stdout.contains("contract_admission"));
        ensure!(!stdout.contains("\"evidence_class\": \"actual_host\""));
        ensure!(!stdout.contains("PROMPT_CANARY"));
        let value: Value = serde_json::from_str(&stdout)?;
        ensure!(value["claim_ceiling"]["updates_compatibility_authority"] == false);
        ensure!(value["claim_ceiling"]["promotes_support_registry"] == false);
        ensure!(value["claim_ceiling"]["actual_host_evidence"] == false);
        ensure!(value["later_observed"]["state"] == "unobserved");
        ensure!(value["host_capability"] == "unobserved");
        ensure!(value["operation_identity"] == USER_ACTION_IDENTITY);
        Ok(())
    }

    #[test]
    fn cli_human_host_test_discloses_usage_before_result() -> Result<()> {
        let output = run_perllsp(&["doctor", "--client", "claude", "--host-test"])?;
        let stdout = String::from_utf8(output.stdout)?;
        let disclosure_at = must_some_with(stdout.find(USAGE_DISCLOSURE), "usage disclosure");
        let terminal_at = must_some_with(stdout.find("Terminal:"), "terminal");
        ensure!(disclosure_at < terminal_at);
        Ok(())
    }

    #[test]
    fn ordinary_doctor_and_completion_do_not_select_host_test() -> Result<()> {
        let doctor = run_perllsp(&["doctor"])?;
        let doctor_out = format!(
            "{}{}",
            String::from_utf8_lossy(&doctor.stdout),
            String::from_utf8_lossy(&doctor.stderr)
        );
        ensure!(!doctor_out.contains(RESULT_SCHEMA_VERSION));
        ensure!(!doctor_out.contains(USER_ACTION_IDENTITY));

        let completion = run_perllsp(&["--completion", "bash"])?;
        let completion_out = String::from_utf8(completion.stdout)?;
        ensure!(!completion_out.contains("--host-test"));
        ensure!(!completion_out.contains(RESULT_SCHEMA_VERSION));
        Ok(())
    }

    #[test]
    fn cli_rejects_hostile_host_test_fields() -> Result<()> {
        let output = run_perllsp(&[
            "doctor",
            "--client",
            "claude",
            "--host-test",
            "--prompt",
            "PROMPT_CANARY",
        ])?;
        ensure!(!output.status.success());
        let stderr = String::from_utf8(output.stderr)?;
        ensure!(stderr.contains("rejects client-supplied"));
        ensure!(!stderr.contains("PROMPT_CANARY"));
        Ok(())
    }
}
