//! Regression guard for the editor UX quarantine registry.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::Value;

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[test]
fn scenario_14_quarantine_rows_have_terminal_executable_dispositions(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ledger_raw = fs::read_to_string(root.join(".ci/ux-flakes.json"))?;
    let ledger: Value = serde_json::from_str(&ledger_raw)?;
    let entries = ledger["entries"]
        .as_array()
        .ok_or_else(|| invalid_data("ux-flakes entries must be an array"))?;
    let scenario_source = fs::read_to_string(
        root.join("crates/perl-lsp-ux-tests/tests/ux_scenario_14_inc_conformance.rs"),
    )?;

    let scenario_rows: Vec<&Value> = entries
        .iter()
        .filter(|entry| {
            entry["test"]
                .as_str()
                .is_some_and(|test| test.starts_with("ux_scenario_14_inc_conformance::"))
        })
        .collect();

    assert_eq!(scenario_rows.len(), 11, "expected the 11 historical Scenario 14 rows");
    for entry in scenario_rows {
        let test = entry["test"].as_str().unwrap_or("<missing test>");
        assert_eq!(entry["state"], "resolved", "{test} must not remain quarantined");
        assert_ne!(entry["issue"], 7570, "{test} must not route to unrelated #7570");

        let disposition = entry["disposition"]
            .as_str()
            .ok_or_else(|| invalid_data(format!("{test} is missing disposition")))?;
        assert!(
            matches!(disposition, "stabilized" | "resolved_by_intent" | "folded" | "not_proven"),
            "{test} has non-terminal disposition {disposition}"
        );

        assert_eq!(
            entry["evidence"]["command"],
            "PERL_LSP_UX_REQUIRE_BINARY=1 just ux-tests",
            "{test} must name the hard-fail verification lane"
        );
        let replacements = entry["evidence"]["replacement_tests"]
            .as_array()
            .ok_or_else(|| invalid_data(format!("{test} is missing replacement_tests")))?;
        assert!(!replacements.is_empty(), "{test} must map to executable replacement coverage");
        for replacement in replacements {
            let replacement = replacement
                .as_str()
                .ok_or_else(|| invalid_data(format!("{test} has a non-string replacement")))?;
            assert!(
                scenario_source.contains(&format!("fn {replacement}(")),
                "{test} points to missing replacement test {replacement}"
            );
        }
    }

    assert_eq!(ledger["summary"]["active"], 0);
    assert_eq!(ledger["summary"]["resolved"], 11);
    Ok(())
}
