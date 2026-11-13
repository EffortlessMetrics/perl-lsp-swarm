//! Source-grounded Kani 0.68 export inspection (#19317).
//!
//! This module reads observations. It neither launches a verifier nor authenticates
//! a supplied report. Custody, source closure, controls and freshness must be joined
//! by the formal runner before any proof receipt can be admitted.

mod unique_json;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_HARNESSES: usize = 256;
const MAX_CHECKS: usize = 100_000;
const TOOLS: [&str; 5] = ["kani", "rustc", "cbmc", "goto_cc", "goto_instrument"];
const COUNTS: [&str; 9] = [
    "passed",
    "failed",
    "unreachable",
    "undetermined",
    "solver_error",
    "satisfied",
    "unsatisfiable",
    "covered",
    "uncovered",
];

/// The run owner's expected identities, not identities learned from the report.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedRun {
    pub target: String,
    pub tools: BTreeMap<String, String>,
    pub cbmc_version: String,
    pub solver: String,
    pub object_bits: u32,
    /// Reviewed native property classes for this pinned tool/profile.
    pub property_categories: BTreeSet<String>,
    pub harnesses: BTreeMap<String, ExpectedHarness>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedHarness {
    pub goal: Goal,
    /// Exact emitted filename; no basename-only or suffix-based identity matching.
    pub source: String,
    /// Exact custom assertion descriptions, each required once and reachable.
    pub assertions: Vec<String>,
    /// Exact custom cover descriptions, each required once and satisfied.
    pub covers: Vec<String>,
}

/// Positive proof and deliberately failing controls have different obligations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Goal {
    Positive,
    Counterexample { assertion: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    PositiveReport,
    ReportedCounterexample,
    NotProven,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub schema_version: &'static str,
    pub evidence_class: &'static str,
    /// Deliberately always false: a JSON file cannot authenticate its execution.
    pub proof_accepted: bool,
    pub input_sha256: String,
    pub expectation_sha256: String,
    pub kind: ObservationKind,
    pub harnesses: BTreeMap<String, HarnessObservation>,
}

#[derive(Debug, Serialize)]
pub struct HarnessObservation {
    pub kind: ObservationKind,
    pub properties: u64,
    pub required_assertions: u64,
    pub required_covers: u64,
    pub satisfied_covers: u64,
    /// Observation only; a replay artifact and settled execution are still required.
    pub expected_failure_observed: bool,
    pub duration_ms: u64,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub check_id: u64,
    pub category: String,
    pub description: String,
    pub status: String,
}

fn object(value: &Value) -> Result<&serde_json::Map<String, Value>> {
    value.as_object().context("expected JSON object")
}

fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    for key in object(value)?.keys() {
        ensure!(allowed.contains(&key.as_str()), "unexpected semantic field: {key}");
    }
    Ok(())
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().context("expected JSON array")
}

fn string(value: &Value) -> Result<&str> {
    let value = value.as_str().context("expected JSON string")?;
    ensure!(!value.trim().is_empty(), "empty identity/status string");
    Ok(value)
}

fn number(value: &Value) -> Result<u64> {
    value.as_u64().context("expected nonnegative integer, not null/float/string")
}

fn same(actual: &Value, expected: &str, what: &str) -> Result<()> {
    ensure!(string(actual)? == expected, "{what} mismatch");
    Ok(())
}

fn unique_messages(values: &[String], what: &str) -> Result<BTreeSet<String>> {
    ensure!(!values.is_empty(), "empty {what} denominator");
    let mut unique = BTreeSet::new();
    for value in values {
        ensure!(!value.trim().is_empty(), "blank {what} identity");
        ensure!(unique.insert(value.clone()), "duplicate {what} identity");
    }
    Ok(unique)
}

fn validate_expected(expected: &ExpectedRun) -> Result<()> {
    ensure!(
        !expected.harnesses.is_empty() && expected.harnesses.len() <= MAX_HARNESSES,
        "invalid expected harness denominator"
    );
    let keys: BTreeSet<_> = expected.tools.keys().map(String::as_str).collect();
    ensure!(keys == TOOLS.into_iter().collect(), "expected tool identity set differs");
    ensure!(
        expected.tools.get("kani").map(String::as_str) == Some("0.68.0"),
        "unsupported Kani version"
    );
    for value in
        expected.tools.values().chain([&expected.target, &expected.cbmc_version, &expected.solver])
    {
        ensure!(!value.trim().is_empty(), "blank expected tool/target identity");
    }
    ensure!(
        expected.property_categories.contains("assertion")
            && expected.property_categories.contains("cover")
            && expected.property_categories.iter().all(|value| !value.trim().is_empty())
            && !expected.property_categories.contains("code_coverage"),
        "invalid expected property category set"
    );
    ensure!((1..=64).contains(&expected.object_bits), "invalid expected object bits");
    for (id, harness) in &expected.harnesses {
        ensure!(
            !id.trim().is_empty() && !harness.source.trim().is_empty(),
            "blank expected harness/source"
        );
        let assertions = unique_messages(&harness.assertions, "assertion")?;
        let covers = expected_covers(harness)?;
        if let Goal::Counterexample { assertion } = &harness.goal {
            ensure!(
                assertions.contains(assertion),
                "counterexample target is not a required assertion"
            );
        }
        ensure!(assertions.is_disjoint(&covers), "assertion/cover identities overlap");
    }
    Ok(())
}

fn expected_covers(harness: &ExpectedHarness) -> Result<BTreeSet<String>> {
    if matches!(&harness.goal, Goal::Counterexample { .. }) && harness.covers.is_empty() {
        Ok(BTreeSet::new())
    } else {
        unique_messages(&harness.covers, "cover")
    }
}

/// Join independently ordered native arrays by ID, never by their position.
fn rows<'a>(
    value: &'a Value,
    key: &str,
    expected: &ExpectedRun,
) -> Result<BTreeMap<String, &'a Value>> {
    let values = array(value)?;
    ensure!(values.len() == expected.harnesses.len(), "native harness denominator differs");
    let mut result = BTreeMap::new();
    for row in values {
        object(row)?;
        let id = string(&row[key])?;
        ensure!(expected.harnesses.contains_key(id), "foreign native harness: {id}");
        ensure!(result.insert(id.to_owned(), row).is_none(), "duplicate native harness: {id}");
    }
    Ok(result)
}

fn count_key(status: &str) -> Result<&'static str> {
    Ok(match status {
        "Success" => "passed",
        "Failure" => "failed",
        "Unreachable" => "unreachable",
        "Undetermined" | "Unknown" => "undetermined",
        "Error" => "solver_error",
        "Satisfied" => "satisfied",
        "Unsatisfiable" => "unsatisfiable",
        "Covered" => "covered",
        "Uncovered" => "uncovered",
        _ => bail!("unknown native property status: {status}"),
    })
}

fn inspect_properties(
    result: &Value,
    details: &Value,
    expected: &ExpectedHarness,
    categories: &BTreeSet<String>,
) -> Result<HarnessObservation> {
    fields(result, &["harness_id", "status", "duration_ms", "checks"])?;
    fields(
        details,
        &[
            "total_properties",
            "passed",
            "failed",
            "unreachable",
            "undetermined",
            "solver_error",
            "satisfied",
            "unsatisfiable",
            "covered",
            "uncovered",
            "error",
        ],
    )?;
    let checks = array(&result["checks"])?;
    ensure!(
        !checks.is_empty() && checks.len() <= MAX_CHECKS,
        "invalid native property denominator"
    );
    let required_assertions = unique_messages(&expected.assertions, "assertion")?;
    let required_covers = expected_covers(expected)?;
    let positive = matches!(&expected.goal, Goal::Positive);
    let mut expected_failure_observed = false;
    let mut assertion_seen = BTreeSet::new();
    let mut cover_seen = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut counts: BTreeMap<&str, u64> = COUNTS.into_iter().map(|key| (key, 0)).collect();
    let mut findings = Vec::new();
    let mut incomplete = false;
    let mut counterexample = false;
    for check in checks {
        fields(check, &["id", "function", "status", "description", "location", "category"])?;
        let id = number(&check["id"])?;
        ensure!(
            id > 0 && id <= checks.len() as u64 && ids.insert(id),
            "duplicate/invalid native check ID"
        );
        let status = string(&check["status"])?;
        let category = string(&check["category"])?;
        ensure!(categories.contains(category), "unrecognized property category: {category}");
        let description = string(&check["description"])?;
        let key = count_key(status)?;
        *counts.get_mut(key).context("missing status counter")? += 1;
        let is_cover = category == "cover";
        if is_cover {
            ensure!(required_covers.contains(description), "foreign cover: {description}");
            ensure!(cover_seen.insert(description.to_owned()), "duplicate cover description");
            ensure!(
                matches!(
                    status,
                    "Satisfied"
                        | "Unsatisfiable"
                        | "Unreachable"
                        | "Undetermined"
                        | "Unknown"
                        | "Error"
                ),
                "invalid status for Kani cover"
            );
            incomplete |= status != "Satisfied";
        } else {
            ensure!(
                matches!(
                    status,
                    "Success"
                        | "Failure"
                        | "Unreachable"
                        | "Undetermined"
                        | "Unknown"
                        | "Error"
                ),
                "invalid status for non-cover property"
            );
            if required_assertions.contains(description) {
                ensure!(category == "assertion", "required assertion category mismatch");
                ensure!(
                    assertion_seen.insert(description.to_owned()),
                    "duplicate required assertion"
                );
                incomplete |= positive && status == "Unreachable";
                if let Goal::Counterexample { assertion } = &expected.goal {
                    expected_failure_observed |= assertion == description && status == "Failure";
                }
            }
            let limit_or_unsupported = matches!(
                category,
                "unwind" | "recursion" | "unsupported_construct" | "missing_definition"
            );
            incomplete |= limit_or_unsupported && status == "Failure";
            counterexample |= status == "Failure" && !limit_or_unsupported;
            incomplete |= matches!(status, "Undetermined" | "Unknown" | "Error");
        }
        // Unreachable ordinary checks remain distinguishable from required covers
        // and required assertions; they do not satisfy a required witness.
        if !matches!(status, "Success" | "Satisfied") {
            findings.push(Finding {
                check_id: id,
                category: category.to_owned(),
                description: description.to_owned(),
                status: status.to_owned(),
            });
        }
    }
    ensure!(assertion_seen == required_assertions, "required assertion set incomplete");
    ensure!(cover_seen == required_covers, "required cover set incomplete");
    ensure!(
        number(&details["total_properties"])? == checks.len() as u64,
        "property total contradicts checks"
    );
    for (key, observed) in &counts {
        ensure!(number(&details[*key])? == *observed, "property count contradicts checks: {key}");
    }
    ensure!(details.get("error").is_none(), "unmeasured properties cannot be admitted");
    incomplete |= !positive && !expected_failure_observed;
    let kind = if incomplete {
        ObservationKind::NotProven
    } else if counterexample {
        ObservationKind::ReportedCounterexample
    } else {
        ObservationKind::PositiveReport
    };
    Ok(HarnessObservation {
        kind,
        properties: checks.len() as u64,
        required_assertions: required_assertions.len() as u64,
        required_covers: required_covers.len() as u64,
        satisfied_covers: *counts.get("satisfied").context("missing cover counter")?,
        expected_failure_observed,
        duration_ms: number(&result["duration_ms"])?,
        findings,
    })
}

// Kani 0.68 call_cbmc::determine_failed_properties gives Error precedence,
// then separates assertion-only failures from other failed properties.
fn reconcile_error_class(error: &Value, observation: &HarnessObservation) -> Result<()> {
    let failed: Vec<_> = observation.findings.iter().filter(|f| f.status == "Failure").collect();
    let (kind, description) = if observation.findings.iter().any(|f| f.status == "Error") {
        ("Error", "property_error")
    } else if failed.is_empty() {
        ("None", "unknown_failure")
    } else if failed.iter().all(|f| f.category == "assertion") {
        ("PanicsOnly", "assertion_failure")
    } else {
        ("Other", "verification_failure")
    };
    same(&error["failed_properties_type"], kind, "failed property classification")?;
    same(&error["error_type"], description, "error classification")
}

/// Parse an expectation file without allowing duplicate JSON keys or extra fields.
pub fn parse_expected(bytes: &[u8]) -> Result<ExpectedRun> {
    let value = unique_json::parse(bytes, MAX_INPUT_BYTES)?;
    let expected: ExpectedRun = serde_json::from_value(value)?;
    validate_expected(&expected)?;
    Ok(expected)
}

/// Inspect one complete native export. Unknown data fields remain in the retained
/// raw bytes; unknown status meaning and contradictory identity/counts fail closed.
/// The SHA-256 binds these exact bytes, including any unrecognized metadata.
pub fn inspect(bytes: &[u8], expected: &ExpectedRun) -> Result<Inspection> {
    validate_expected(expected)?;
    let doc = unique_json::parse(bytes, MAX_INPUT_BYTES)?;
    fields(
        &doc,
        &[
            "metadata",
            "project",
            "tools",
            "coverage",
            "harness_metadata",
            "verification_results",
            "property_details",
            "error_details",
            "cbmc",
        ],
    )?;
    fields(&doc["tools"], &["kani", "rustc", "cbmc", "goto_cc", "goto_instrument", "solvers"])?;
    fields(&doc["verification_results"], &["summary", "results"])?;
    same(&doc["metadata"]["version"], "1.0", "export schema")?;
    same(&doc["metadata"]["kani_version"], "0.68.0", "Kani metadata version")?;
    same(&doc["metadata"]["target"], &expected.target, "target")?;
    for (tool, version) in &expected.tools {
        same(&doc["tools"][tool], version, &format!("tool {tool}"))?;
    }
    let solvers = array(&doc["tools"]["solvers"])?;
    ensure!(solvers.len() == 1, "solver set differs");
    fields(&solvers[0], &["name", "version"])?;
    same(&solvers[0]["name"], &expected.solver, "solver")?;
    // The first profile admits only CBMC's bundled solver. External solver
    // identity requires its own executable custody, not a nullable version.
    ensure!(
        matches!(expected.solver.as_str(), "cadical" | "minisat"),
        "external solver unsupported"
    );
    ensure!(solvers[0].get("version") == Some(&Value::Null), "bundled solver version must be null");
    fields(&doc["coverage"], &["enabled"])?;
    ensure!(doc["coverage"]["enabled"].as_bool() == Some(false), "coverage mode unsupported");
    let metadata = rows(&doc["harness_metadata"], "pretty_name", expected)?;
    let results = rows(&doc["verification_results"]["results"], "harness_id", expected)?;
    let details = rows(&doc["property_details"], "harness_id", expected)?;
    let errors = rows(&doc["error_details"], "harness_id", expected)?;
    let cbmc = rows(&doc["cbmc"], "harness_id", expected)?;
    let summary = &doc["verification_results"]["summary"];
    fields(
        summary,
        &["total_harnesses", "executed", "status", "successful", "failed", "duration_ms"],
    )?;
    same(&summary["status"], "completed", "native completion")?;
    for field in ["total_harnesses", "executed"] {
        ensure!(
            number(&summary[field])? == expected.harnesses.len() as u64,
            "summary harness denominator differs"
        );
    }
    let mut observations = BTreeMap::new();
    let mut successes = 0;
    let mut duration = 0_u64;
    for (id, required) in &expected.harnesses {
        let meta = metadata.get(id).context("missing metadata")?;
        same(&meta["source"]["file"], &required.source, "harness source")?;
        same(&meta["attributes"]["kind"], "Proof", "harness kind")?;
        for value in [
            &meta["attributes"]["should_panic"],
            &meta["is_automatically_generated"],
            &meta["has_loop_contracts"],
            &meta["is_ctor_based"],
            &meta["is_bounded"],
        ] {
            ensure!(
                value.as_bool() == Some(false),
                "panic/automatic/loop-contract harness unsupported"
            );
        }
        fields(&meta["attributes"], &["kind", "should_panic"])?;
        fields(&meta["contract"], &["contracted_function_name", "recursion_tracker"])?;
        for field in ["contracted_function_name", "recursion_tracker"] {
            ensure!(
                meta["contract"].get(field) == Some(&Value::Null),
                "contract harness unsupported"
            );
        }
        let result = results.get(id).context("missing result")?;
        let error = errors.get(id).context("missing error details")?;
        fields(
            error,
            &["harness_id", "has_errors", "error_type", "failed_properties_type", "exit_status"],
        )?;
        let native_success = match string(&result["status"])? {
            "Success" => true,
            "Failure" => false,
            other => bail!("unknown native harness status: {other}"),
        };
        ensure!(
            error["has_errors"].as_bool() == Some(!native_success),
            "error/result contradiction"
        );
        if native_success {
            ensure!(
                error.get("error_type").is_none()
                    && error.get("exit_status").is_none()
                    && error.get("failed_properties_type").is_none(),
                "success carries failure details"
            );
            successes += 1;
        } else {
            same(&error["exit_status"], "properties_failed", "native termination")?;
        }
        let tool = cbmc.get(id).context("missing CBMC details")?;
        fields(&tool["configuration"], &["object_bits", "solver"])?;
        same(&tool["cbmc_metadata"]["version"], &expected.cbmc_version, "CBMC version")?;
        same(&tool["configuration"]["solver"], &expected.solver, "harness solver")?;
        ensure!(
            number(&tool["configuration"]["object_bits"])? == u64::from(expected.object_bits),
            "object bits mismatch"
        );
        let detail = details.get(id).context("missing property details")?;
        fields(detail, &["harness_id", "property_details"])?;
        let observation = inspect_properties(
            result,
            &detail["property_details"],
            required,
            &expected.property_categories,
        )
        .with_context(|| format!("harness {id}"))?;
        if native_success {
            ensure!(
                !observation
                    .findings
                    .iter()
                    .any(|f| matches!(f.status.as_str(), "Failure" | "Error")),
                "native success contradicts properties"
            );
        } else {
            ensure!(
                observation.kind != ObservationKind::PositiveReport,
                "native failure has only positive properties"
            );
            reconcile_error_class(error, &observation)?;
        }
        duration = duration.checked_add(observation.duration_ms).context("duration overflow")?;
        observations.insert(id.clone(), observation);
    }
    ensure!(number(&summary["successful"])? == successes, "successful count contradicts harnesses");
    ensure!(
        number(&summary["failed"])? == expected.harnesses.len() as u64 - successes,
        "failed count contradicts harnesses"
    );
    ensure!(number(&summary["duration_ms"])? == duration, "duration contradicts harnesses");
    let kind = if observations.values().any(|o| o.kind == ObservationKind::NotProven) {
        ObservationKind::NotProven
    } else if observations.values().any(|o| o.kind == ObservationKind::ReportedCounterexample) {
        ObservationKind::ReportedCounterexample
    } else {
        ObservationKind::PositiveReport
    };
    Ok(Inspection {
        schema_version: "kani_export_inspection.v1",
        evidence_class: "untrusted_artifact_inspection",
        proof_accepted: false,
        input_sha256: format!("{:x}", Sha256::digest(bytes)),
        kind,
        expectation_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(expected)?)),
        harnesses: observations,
    })
}

#[cfg(test)]
mod tests;
