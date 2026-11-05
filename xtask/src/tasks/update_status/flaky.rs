//! Flaky-test registry reader and formatter.
//!
//! Reads `.ci/flaky-tests.json` and surfaces active/resolved counts for quality.md.

use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Active/resolved counts from `.ci/flaky-tests.json`.
#[derive(Debug)]
pub(super) struct FlakyTestSummary {
    pub active: usize,
    pub resolved: usize,
}

// ---------------------------------------------------------------------------
// Collectors
// ---------------------------------------------------------------------------

/// Read `.ci/flaky-tests.json` and return active/resolved counts.
/// Returns `None` if the file is unavailable or either count is missing or invalid.
pub(super) fn collect_flaky_test_summary(root: &Path) -> Option<FlakyTestSummary> {
    let path = root.join(".ci/flaky-tests.json");
    let raw = fs::read_to_string(&path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let active = usize::try_from(doc.pointer("/summary/active")?.as_u64()?).ok()?;
    let resolved = usize::try_from(doc.pointer("/summary/resolved")?.as_u64()?).ok()?;
    Some(FlakyTestSummary { active, resolved })
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

/// Format the flaky-tests summary as a markdown table with a sourcing note.
pub(super) fn format_flaky_tests_section(summary: Option<&FlakyTestSummary>) -> String {
    let rows = match summary {
        Some(summary) => {
            format!("| Active | {} |\n| Resolved | {} |", summary.active, summary.resolved)
        }
        None => "| Active | UNVERIFIED |\n\
                 | Resolved | UNVERIFIED |\n\n\
                 Registry data is unavailable or invalid."
            .to_string(),
    };
    format!(
        "| State | Count |\n\
         |-------|-------|\n\
         {rows}\n\
         \n\
         _Source: [.ci/flaky-tests.json](../../../.ci/flaky-tests.json). \
         Run `just status-update quality` to refresh._"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Context, Result};

    #[test]
    fn test_collect_flaky_test_summary_reads_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ci_dir = dir.path().join(".ci");
        fs::create_dir_all(&ci_dir)?;
        let json = r#"{
            "schema_version": 1,
            "entries": [],
            "summary": { "total": 3, "active": 1, "resolved": 2, "by_subsystem": {} }
        }"#;
        fs::write(ci_dir.join("flaky-tests.json"), json)?;
        let summary = collect_flaky_test_summary(dir.path());
        assert_eq!(summary.map(|counts| (counts.active, counts.resolved)), Some((1, 2)));
        Ok(())
    }

    #[test]
    fn test_collect_flaky_test_summary_missing_file_is_unverified() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let summary = collect_flaky_test_summary(dir.path());
        assert!(summary.is_none());
        Ok(())
    }

    #[test]
    fn test_collect_flaky_test_summary_unreadable_file_is_unverified() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join(".ci/flaky-tests.json"))?;
        assert!(collect_flaky_test_summary(dir.path()).is_none());
        Ok(())
    }

    #[test]
    fn test_collect_flaky_test_summary_invalid_data_is_unverified() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ci_dir = dir.path().join(".ci");
        fs::create_dir_all(&ci_dir)?;
        let cases = [
            ("malformed JSON", "{not-json"),
            ("missing summary", "{}"),
            ("null summary", r#"{"summary":null}"#),
            ("missing active", r#"{"summary":{"resolved":2}}"#),
            ("missing resolved", r#"{"summary":{"active":1}}"#),
            ("string active", r#"{"summary":{"active":"0","resolved":2}}"#),
            ("null active", r#"{"summary":{"active":null,"resolved":2}}"#),
            ("negative active", r#"{"summary":{"active":-1,"resolved":2}}"#),
            ("fractional active", r#"{"summary":{"active":0.5,"resolved":2}}"#),
            ("boolean active", r#"{"summary":{"active":false,"resolved":2}}"#),
            ("string resolved", r#"{"summary":{"active":1,"resolved":"0"}}"#),
            ("null resolved", r#"{"summary":{"active":1,"resolved":null}}"#),
            ("negative resolved", r#"{"summary":{"active":1,"resolved":-1}}"#),
            ("fractional resolved", r#"{"summary":{"active":1,"resolved":0.5}}"#),
        ];
        for (case, json) in cases {
            fs::write(ci_dir.join("flaky-tests.json"), json)?;
            assert!(collect_flaky_test_summary(dir.path()).is_none(), "{case}");
        }
        Ok(())
    }

    #[test]
    fn test_collect_flaky_test_summary_preserves_explicit_zero() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ci_dir = dir.path().join(".ci");
        fs::create_dir_all(&ci_dir)?;
        fs::write(ci_dir.join("flaky-tests.json"), r#"{"summary":{"active":0,"resolved":0}}"#)?;
        let summary = collect_flaky_test_summary(dir.path());
        assert_eq!(summary.as_ref().map(|counts| (counts.active, counts.resolved)), Some((0, 0)));
        let section = format_flaky_tests_section(summary.as_ref());
        assert!(section.contains("| Active | 0 |"));
        assert!(section.contains("| Resolved | 0 |"));
        assert!(!section.contains("UNVERIFIED"));
        Ok(())
    }

    #[test]
    fn test_format_flaky_tests_section_contains_counts() {
        let summary = FlakyTestSummary { active: 2, resolved: 5 };
        let section = format_flaky_tests_section(Some(&summary));
        assert!(section.contains("| Active | 2 |"), "missing active count row");
        assert!(section.contains("| Resolved | 5 |"), "missing resolved count row");
        assert!(
            section.contains("[.ci/flaky-tests.json](../../../.ci/flaky-tests.json)"),
            "missing source link"
        );
        assert!(
            section.contains("`just status-update quality`"),
            "missing supported refresh command"
        );
        assert!(!section.contains("just status-update --only quality"));
    }

    #[test]
    fn test_format_flaky_tests_section_without_observation_is_unverified() {
        let section = format_flaky_tests_section(None);
        assert!(section.contains("| Active | UNVERIFIED |"));
        assert!(section.contains("| Resolved | UNVERIFIED |"));
        assert!(section.contains("unavailable or invalid"));
        assert!(!section.contains("| Active | 0 |"));
        assert!(!section.contains("| Resolved | 0 |"));
        assert!(section.contains("[.ci/flaky-tests.json](../../../.ci/flaky-tests.json)"));
        assert!(section.contains("`just status-update quality`"));
    }

    #[test]
    fn test_flaky_tests_json_exists_and_is_valid() -> Result<()> {
        let root = crate::utils::project_root()?;
        let path = root.join(".ci/flaky-tests.json");
        assert!(path.exists(), ".ci/flaky-tests.json must exist");
        let raw = fs::read_to_string(&path).context("reading .ci/flaky-tests.json")?;
        let doc: serde_json::Value =
            serde_json::from_str(&raw).context("parsing .ci/flaky-tests.json")?;
        assert_eq!(doc["schema_version"], 1, "schema_version must be 1");
        assert!(doc["entries"].is_array(), "entries must be an array");
        assert!(doc["summary"].is_object(), "summary must be an object");
        Ok(())
    }
}
