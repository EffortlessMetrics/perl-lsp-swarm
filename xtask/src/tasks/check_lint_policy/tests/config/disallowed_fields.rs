use super::super::super::model::{ConfigurationState, LintLedger};
use super::super::super::validate::validate_clippy_config_value;
use super::super::{ledger_with, lint_entry, planned_lint};
use color_eyre::eyre::{Result, bail};
use serde_json::Value as JsonValue;
use std::ffi::OsString;
use std::fs;
use std::process::Command;
use tempfile::tempdir;
use toml::Value;

const DISALLOWED_FIELD_REASON: &str = "fixture proves configured fields are enforced";
const MAX_DIAGNOSTIC_LINES: usize = 20;
const MAX_DIAGNOSTIC_CHARS_PER_LINE: usize = 500;

#[test]
fn active_lint_requires_config_hook() -> Result<()> {
    let config = toml::from_str::<Value>("msrv = \"1.95\"")?;
    let ledger = disallowed_fields_ledger("active", Some(ConfigurationState::EmptyByDesign));

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("missing disallowed-fields config should fail");
    };
    assert!(error.to_string().contains("must define disallowed-fields"));
    Ok(())
}

#[test]
fn config_requires_a_ledger_entry() -> Result<()> {
    let config = disallowed_fields_config("[]")?;
    let ledger = ledger_with(Vec::new());

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("unledgered disallowed-fields config should fail");
    };
    assert!(
        error.to_string().contains("must contain an active or debt clippy::disallowed_fields row")
    );
    Ok(())
}

#[test]
fn jointly_missing_policy_inputs_fail_closed() -> Result<()> {
    let config = toml::from_str::<Value>("msrv = \"1.95\"")?;
    let ledger = ledger_with(Vec::new());

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("joint Cargo/ledger/config policy removal must fail closed");
    };
    assert!(
        error.to_string().contains(
            "removing or demoting the policy identity together with its Cargo/config hooks"
        )
    );
    Ok(())
}

#[test]
fn disallowed_fields_cannot_be_demoted_to_future_planned() -> Result<()> {
    let config = toml::from_str::<Value>("msrv = \"1.95\"")?;
    let mut ledger = ledger_with(Vec::new());
    ledger.planned.push(planned_lint("clippy::disallowed_fields", "1.99"));

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("demoting disallowed_fields out of the active catalog must fail closed");
    };
    assert!(
        error.to_string().contains("must contain an active or debt clippy::disallowed_fields row")
    );
    Ok(())
}

#[test]
fn config_must_be_an_array() -> Result<()> {
    let config = disallowed_fields_config("{ unexpected = true }")?;
    let ledger = disallowed_fields_ledger("active", Some(ConfigurationState::EmptyByDesign));

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("non-array disallowed-fields config should fail");
    };
    assert!(error.to_string().contains("disallowed-fields must be an array"));
    Ok(())
}

#[test]
fn empty_config_requires_explicit_configuration_state() -> Result<()> {
    let config = disallowed_fields_config("[]")?;
    let ledger = disallowed_fields_ledger("active", None);

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("unmarked empty disallowed-fields config should fail");
    };
    assert!(error.to_string().contains("configuration_state = \"empty-by-design\""));
    Ok(())
}

#[test]
fn empty_config_accepts_explicit_configuration_state() -> Result<()> {
    let config = disallowed_fields_config("[]")?;
    let ledger = disallowed_fields_ledger("active", Some(ConfigurationState::EmptyByDesign));

    validate_clippy_config_value(&config, &ledger)
}

#[test]
fn populated_config_rejects_stale_empty_marker() -> Result<()> {
    let config = disallowed_fields_config(&configured_disallowed_field())?;
    let ledger = disallowed_fields_ledger("active", Some(ConfigurationState::EmptyByDesign));

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("populated disallowed-fields config should reject the empty marker");
    };
    assert!(error.to_string().contains("remove stale configuration_state"));
    Ok(())
}

#[test]
fn populated_config_requires_governed_selector_contract() -> Result<()> {
    let config = disallowed_fields_config(&configured_disallowed_field())?;
    let ledger = disallowed_fields_ledger("active", None);

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("populated disallowed-fields config should fail before selector governance lands");
    };
    assert!(error.to_string().contains("Phase 1 permits only the validated empty set"));
    Ok(())
}

#[test]
fn configuration_state_is_rejected_for_unrelated_lints() -> Result<()> {
    let config = toml::from_str::<Value>("msrv = \"1.95\"")?;
    let mut lint = lint_entry("clippy::panic", "active");
    lint.configuration_state = Some(ConfigurationState::EmptyByDesign);
    let ledger = ledger_with(vec![lint]);

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("unrelated configuration_state should fail");
    };
    assert!(error.to_string().contains("only config-backed lints"));
    Ok(())
}

#[test]
fn configuration_state_is_rejected_for_non_active_lint() -> Result<()> {
    let config = disallowed_fields_config("[]")?;
    let ledger = disallowed_fields_ledger("tracked", Some(ConfigurationState::EmptyByDesign));

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("tracked disallowed_fields marker should fail");
    };
    assert!(error.to_string().contains("valid only for active or debt lints"));
    Ok(())
}

#[test]
fn tracked_lint_without_marker_or_hook_is_rejected() -> Result<()> {
    let config = toml::from_str::<Value>("msrv = \"1.95\"")?;
    let ledger = disallowed_fields_ledger("tracked", None);

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("tracked disallowed_fields without marker or hook should fail");
    };
    assert!(error.to_string().contains("must remain active or debt"));
    Ok(())
}

#[test]
fn configured_field_is_rejected_by_clippy() -> Result<()> {
    let fixture = tempdir()?;
    let source_dir = fixture.path().join("src");
    fs::create_dir(&source_dir)?;
    fs::write(
        fixture.path().join("Cargo.toml"),
        r#"[package]
name = "clippy-disallowed-fields-fixture"
version = "0.0.0"
edition = "2024"
rust-version = "1.95"
publish = false

[workspace]
"#,
    )?;
    fs::write(
        fixture.path().join("clippy.toml"),
        format!(
            "disallowed-fields = [{{ path = \"std::ops::Range::start\", reason = \"{DISALLOWED_FIELD_REASON}\" }}]\n"
        ),
    )?;
    fs::write(
        source_dir.join("lib.rs"),
        r#"#![deny(clippy::disallowed_fields)]

pub fn range_start(range: std::ops::Range<usize>) -> usize {
    range.start
}
"#,
    )?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .current_dir(fixture.path())
        .args([
            "clippy",
            "--offline",
            "--quiet",
            "--lib",
            "--no-deps",
            "--message-format=json",
            "--",
            "-D",
            "warnings",
        ])
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .output()?;

    if output.status.success() {
        bail!("configured disallowed field unexpectedly passed Clippy");
    }

    if !contains_disallowed_fields_lint(&output.stdout)? {
        let stdout = bounded_diagnostic(&output.stdout);
        let stderr = bounded_diagnostic(&output.stderr);
        bail!(
            "Clippy failed without the expected lint identity:\nstdout (bounded):\n{stdout}\nstderr (bounded):\n{stderr}"
        );
    }
    Ok(())
}

#[test]
fn malformed_json_after_expected_lint_is_rejected() -> Result<()> {
    let stdout = br#"{"message":{"code":{"code":"clippy::disallowed_fields"}}}
not-json"#;

    let result = contains_disallowed_fields_lint(stdout);
    if result.is_ok() {
        bail!("malformed JSON after the expected lint must fail closed");
    }
    Ok(())
}

#[test]
fn blank_json_record_inside_stream_is_rejected() -> Result<()> {
    let stdout = br#"{"message":{"code":{"code":"clippy::disallowed_fields"}}}

"#;

    let result = contains_disallowed_fields_lint(stdout);
    if result.is_ok() {
        bail!("an interior blank JSON record must fail closed");
    }
    Ok(())
}

fn contains_disallowed_fields_lint(stdout: &[u8]) -> Result<bool> {
    let mut found_lint = false;
    let lines = stdout.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            let is_single_terminal_newline = index + 1 == lines.len();
            if is_single_terminal_newline {
                continue;
            }
            bail!("blank JSON record in strict stdout stream");
        }
        // `--message-format=json` makes stdout a strict machine channel. A
        // non-JSON line is instrument failure and must not be skipped as if it
        // were unrelated compiler chatter; ordinary notices belong on stderr.
        let event: JsonValue = serde_json::from_slice(line)?;
        let lint_code = event.pointer("/message/code/code").and_then(JsonValue::as_str);
        if lint_code == Some("clippy::disallowed_fields") {
            found_lint = true;
        }
    }
    Ok(found_lint)
}

fn disallowed_fields_ledger(status: &str, state: Option<ConfigurationState>) -> LintLedger {
    let mut lint = lint_entry("clippy::disallowed_fields", status);
    lint.configuration_state = state;
    ledger_with(vec![lint])
}

fn disallowed_fields_config(entries: &str) -> Result<Value> {
    Ok(toml::from_str(&format!("msrv = \"1.95\"\ndisallowed-fields = {entries}\n"))?)
}

fn configured_disallowed_field() -> String {
    format!(r#"[{{ path = "std::ops::Range::start", reason = "{DISALLOWED_FIELD_REASON}" }}]"#)
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut output = String::new();

    for (index, line) in lines.by_ref().take(MAX_DIAGNOSTIC_LINES).enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let mut chars = line.chars();
        output.extend(chars.by_ref().take(MAX_DIAGNOSTIC_CHARS_PER_LINE));
        if chars.next().is_some() {
            output.push('…');
        }
    }
    if lines.next().is_some() {
        output.push_str("\n… additional lines omitted");
    }
    if output.is_empty() {
        output.push_str("<empty>");
    }

    output
}
