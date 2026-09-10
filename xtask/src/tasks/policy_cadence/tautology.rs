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

    Ok(check_tautology::cadence_rows(&path)?
        .into_iter()
        .map(|entry| {
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
                invalid_reason: None,
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

    #[test]
    fn elapsed_disposition_remains_visible_as_owned_cadence_work() -> Result<()> {
        let root = tempdir()?;
        let policy = root.path().join("policy");
        fs::create_dir_all(&policy)?;
        fs::write(
            policy.join("tautology-dispositions.toml"),
            r##"schema_version = 1
policy = "tautology-dispositions"

[[disposition]]
id = "tautology-demo"
rule = "option-is-some-or-none"
path = "crates/demo/src/lib.rs"
line = 4
shape = "value.is_some() || value.is_none()"
owner = "parser-core"
issue = "#14061"
reason = "temporary unresolved product boundary"
created = "2026-08-30"
review_after = "2026-09-01"
expires = "2026-09-02"
"##,
        )?;

        let mut raw = obligations(root.path())?;
        let item = raw
            .pop()
            .ok_or_else(|| eyre!("missing tautology cadence obligation"))?;
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
}
