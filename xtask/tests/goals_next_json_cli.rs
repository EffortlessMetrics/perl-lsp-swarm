use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

fn assert_retired_json(subcommand: &str) -> Result<()> {
    let assert = cargo_bin_cmd!("xtask").args(["goals", subcommand, "--json"]).assert().success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone())?;
    let stderr = String::from_utf8(assert.get_output().stderr.clone())?;
    let value: serde_json::Value = serde_json::from_str(stdout.trim())?;

    assert_eq!(value.get("status").and_then(serde_json::Value::as_str), Some("retired"));
    assert_eq!(value.get("authority").and_then(serde_json::Value::as_str), Some("github"));
    assert_eq!(
        value.get("command").and_then(serde_json::Value::as_str),
        Some(match subcommand {
            "next" => "goals next",
            "reconcile" => "goals reconcile",
            _ => "unknown",
        })
    );
    assert!(value.get("selected_work").is_some_and(serde_json::Value::is_null));
    assert_eq!(value.get("finding_count").and_then(serde_json::Value::as_u64), Some(0));
    assert_eq!(value.get("mutation_performed").and_then(serde_json::Value::as_bool), Some(false));
    assert!(stderr.trim().is_empty(), "compatibility command wrote stderr: {stderr:?}");
    Ok(())
}

#[test]
fn goals_next_json_reports_retired_selector() -> Result<()> {
    assert_retired_json("next")
}

#[test]
fn goals_reconcile_json_reports_retired_selector() -> Result<()> {
    assert_retired_json("reconcile")
}
