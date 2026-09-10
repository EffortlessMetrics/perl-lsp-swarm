use super::RawObligation;
use crate::tasks::check_tautology;
use color_eyre::eyre::Result;
use std::path::Path;

const SOURCE_PATH: &str = "policy/tautology-dispositions.toml";

pub(super) fn obligations(root: &Path) -> Result<Vec<RawObligation>> {
    let path = root.join(SOURCE_PATH);
    if !path.is_file() {
        return Ok(Vec::new());
    }

    // Liveness comes from the scanner, not the ledger alone: a disposition
    // matching no current finding carries its unused reason so cadence
    // reports it as `Invalid` owner work instead of evidence-backed proof.
    Ok(check_tautology::cadence_rows_with_liveness(root, &path)?
        .into_iter()
        .map(|(entry, unused_reason)| {
            let line = entry.line.map_or_else(|| "*".to_string(), |line| line.to_string());
            let shape = entry.shape.as_deref().unwrap_or("*");
            RawObligation {
                record_id: entry.id,
                source_kind: "tautology_disposition".to_string(),
                source_path: SOURCE_PATH.to_string(),
                owner: entry.owner,
                owner_issue: Some(entry.issue),
                review_after: entry.review_after,
                expires: Some(entry.expires),
                evidence_identity: Some(format!(
                    "{}:{}:{line}:{shape}:created={}",
                    entry.rule, entry.path, entry.created
                )),
                expected_debt_class: Some(entry.rule),
                required_decision: format!(
                    "remove after proof, narrow, or re-justify the exact disposition: {}",
                    entry.reason
                ),
                reproduce: Some("cargo xtask check-tautology --check".to_string()),
                disposition: None,
                falsifier: None,
                invalid_reason: unused_reason,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{CadenceState, classify, parse_date};
    use super::*;
    use color_eyre::eyre::eyre;
    use std::fs;
    use tempfile::tempdir;

    fn write_demo_fixture(root: &std::path::Path, shape: &str) -> Result<()> {
        let policy = root.join("policy");
        fs::create_dir_all(&policy)?;
        fs::write(
            policy.join("tautology-dispositions.toml"),
            format!(
                r##"schema_version = 1
policy = "tautology-dispositions"

[[disposition]]
id = "tautology-demo"
rule = "option-is-some-or-none"
path = "crates/demo/src/lib.rs"
line = 4
shape = "{shape}"
owner = "parser-core"
issue = "#14061"
reason = "temporary unresolved product boundary"
created = "2026-08-30"
review_after = "2026-09-01"
expires = "2026-09-02"
"##
            ),
        )?;
        Ok(())
    }

    fn write_demo_source(root: &std::path::Path) -> Result<()> {
        let dir = root.join("crates/demo/src");
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("lib.rs"),
            "pub fn demo(value: Option<u32>) -> bool {\n    let _ = value;\n    // Suppression rationale lives in the disposition ledger.\n    assert!(value.is_some() || value.is_none());\n    true\n}\n",
        )?;
        Ok(())
    }

    #[test]
    fn elapsed_disposition_remains_visible_as_owned_cadence_work() -> Result<()> {
        let root = tempdir()?;
        write_demo_fixture(root.path(), "is_some() || is_none()")?;
        write_demo_source(root.path())?;

        let mut raw = obligations(root.path())?;
        let item = raw.pop().ok_or_else(|| eyre!("missing tautology cadence obligation"))?;
        let item = classify(item, parse_date("2026-09-10", "fixture")?);

        assert_eq!(item.state, CadenceState::Expired);
        assert_eq!(item.record_id, "tautology-demo");
        assert_eq!(item.owner, "parser-core");
        assert_eq!(item.owner_issue.as_deref(), Some("#14061"));
        assert_eq!(item.source_path, SOURCE_PATH);
        assert_eq!(item.expected_debt_class.as_deref(), Some("option-is-some-or-none"));
        assert!(
            item.evidence_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("crates/demo/src/lib.rs:4"))
        );
        Ok(())
    }

    #[test]
    fn unused_disposition_cannot_project_as_evidence_backed_work() -> Result<()> {
        let root = tempdir()?;
        write_demo_fixture(root.path(), "is_some() || is_none()")?;

        let mut raw = obligations(root.path())?;
        let item = raw.pop().ok_or_else(|| eyre!("missing tautology cadence obligation"))?;
        let item = classify(item, parse_date("2026-09-10", "fixture")?);

        assert_eq!(item.state, CadenceState::Invalid);
        assert!(
            item.not_proven_reason
                .as_deref()
                .is_some_and(|reason| { reason.contains("matches no current scanner finding") }),
            "unused disposition must carry its unused reason, got {:?}",
            item.not_proven_reason
        );
        Ok(())
    }
}
