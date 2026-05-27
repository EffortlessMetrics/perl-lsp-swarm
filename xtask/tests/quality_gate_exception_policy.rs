//! Contract tests for the quality-gate burn-down exception ledger.

use std::fs;
use std::path::PathBuf;

use toml::Value;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn quality_gate_exception_policy_names_transitional_debt() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let policy: Value =
        toml::from_str(&fs::read_to_string(root.join("policy/quality-gate-exceptions.toml"))?)?;
    let status_doc =
        fs::read_to_string(root.join("docs/project/status/coverage_and_ripr_enforcement.md"))?;
    let exceptions = policy
        .get("exception")
        .and_then(Value::as_array)
        .ok_or("quality-gate exception policy must list [[exception]] entries")?;
    assert_eq!(
        string_field(&policy, "status"),
        Some("active"),
        "quality-gate exception policy status must be active while transition entries remain"
    );

    let ripr = exception_by_id(exceptions, "ripr-total-burndown")?;
    assert_eq!(string_field(ripr, "applies_to"), Some("ripr_total_not_zero"));
    assert_eq!(string_field(ripr, "owner"), Some("coverage-proof-lane"));
    assert_eq!(string_field(ripr, "final_target"), Some("ripr_plus.unresolved == 0"));
    assert!(
        string_array(ripr, "current_evidence")?
            .iter()
            .any(|item| item == "target/receipts/quality/ripr-plus.json"),
        "RIPR total exception must point at the repo-wide RIPR receipt"
    );
    assert!(
        string_array(ripr, "current_evidence")?
            .iter()
            .any(|item| item == "target/receipts/quality/quality-gate.md"),
        "RIPR total exception must point at the agent-facing quality-gate summary"
    );

    let coverage = exception_by_id(exceptions, "project-coverage-burndown")?;
    assert_eq!(string_field(coverage, "applies_to"), Some("project_coverage_below_target"));
    assert_eq!(string_field(coverage, "owner"), Some("coverage-proof-lane"));
    assert_eq!(string_field(coverage, "final_target"), Some("coverage.project >= 95.0"));
    assert!(
        string_array(coverage, "current_evidence")?
            .iter()
            .any(|item| item == "target/receipts/quality/coverage-baseline.json"),
        "Project coverage exception must point at the coverage receipt"
    );
    assert!(
        string_array(coverage, "current_evidence")?
            .iter()
            .any(|item| item == "target/receipts/quality/coverage-quality-gate.md"),
        "Project coverage exception must point at the coverage gate summary"
    );

    for exception in exceptions {
        let updated = string_field(&policy, "updated")
            .ok_or("quality-gate exception policy needs an updated date")?;
        let review_after = string_field(exception, "review_after");
        let expires = string_field(exception, "expires");
        assert!(review_after.is_some(), "temporary exception entries need review_after dates");
        assert!(expires.is_some(), "temporary exception entries need expiry dates");
        assert!(
            string_field(exception, "removal_criteria").is_some(),
            "temporary exception entries need concrete removal criteria"
        );
        let review_after =
            parse_policy_date(review_after.unwrap()).ok_or("review_after must use YYYY-MM-DD")?;
        let expires = parse_policy_date(expires.unwrap()).ok_or("expires must use YYYY-MM-DD")?;
        let updated = parse_policy_date(updated).ok_or("updated must use YYYY-MM-DD")?;
        assert!(
            updated <= review_after,
            "review_after must not be earlier than policy updated date"
        );
        assert!(review_after <= expires, "expires must not be earlier than review_after");
    }

    assert!(
        status_doc.contains("### Durable Policy Contract")
            && status_doc.contains("coverage target and project threshold values")
            && status_doc.contains("excluded generated or legacy files")
            && status_doc.contains("suppressed RIPR gaps")
            && status_doc.contains("temporary burn-down")
            && status_doc.contains("exceptions with owner, evidence, review date"),
        "coverage/ripr status doc must name the configurable transition-policy controls"
    );
    assert!(
        status_doc.contains("not configurable without a policy PR")
            && status_doc.contains("RIPR+ zero as the final target")
            && status_doc.contains("Codecov patch coverage enforcement")
            && status_doc.contains("Codecov project coverage enforcement after burn-down")
            && status_doc.contains("aggregate `quality-gate` receipt requirements"),
        "coverage/ripr status doc must name final proof targets that cannot be weakened silently"
    );

    Ok(())
}

fn exception_by_id<'a>(
    exceptions: &'a [Value],
    id: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    exceptions
        .iter()
        .find(|exception| string_field(exception, "id") == Some(id))
        .ok_or_else(|| format!("missing quality-gate exception {id}").into())
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|value| !value.trim().is_empty())
}

fn string_array(value: &Value, key: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let items = value.get(key).and_then(Value::as_array).ok_or_else(|| {
        format!(
            "quality-gate exception {} is missing string-array field {key}",
            string_field(value, "id").unwrap_or("unknown")
        )
    })?;
    Ok(items.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect())
}

fn parse_policy_date(value: &str) -> Option<u32> {
    let mut parts = value.split('-');
    let year = parse_date_part(parts.next()?, 4)?;
    let month = parse_date_part(parts.next()?, 2)?;
    let day = parse_date_part(parts.next()?, 2)?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(year * 10_000 + month * 100 + day)
}

fn parse_date_part(value: &str, width: usize) -> Option<u32> {
    if value.len() != width || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok()
}
