//! Validate the restart-warm `snapshot_warm_start_receipt.v1` contract (#16992).
//!
//! Same three-surface pattern as `compiler_performance_receipt`: pinned schema
//! vocabularies, JSON Schema evaluation for every fixture, then fail-closed
//! cross-field rules that missing instrumentation is not zero, work avoidance
//! cannot be inferred from bytes loaded, and correctness failures cannot admit.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SCHEMA_PATH: &str = "schemas/snapshot_warm_start_receipt.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/snapshot_warm_start_receipt.v1.schema.json";
const VERSION: &str = "snapshot_warm_start_receipt.v1";
const FIXTURE_DIR: &str = "xtask/fixtures";
const FIXTURE_PREFIX: &str = "snapshot_warm_start_receipt.v1";
const CONTROL_SUITE_PATH: &str = "xtask/src/tasks/snapshot_warm_start_receipt.rs";
const CONTROL_SUITE_FLOOR: usize = 24;

const REQUIRED: &[&str] = &[
    "schema_version",
    "receipt_id",
    "subject",
    "preparation",
    "instrument",
    "phases",
    "work",
    "correctness",
    "samples",
    "result",
    "comparison",
    "limitations",
];

const EVIDENCE_STATUS: &[&str] =
    &["measured", "not_proven", "not_applicable", "failed", "cancelled"];
const ADMISSION_DISPOSITIONS: &[&str] = &[
    "admit_default_candidate",
    "admit_opt_in_only",
    "admit_shadow_only",
    "no_material_benefit",
    "regressed",
    "not_proven",
    "product_failure",
    "instrument_failure",
    "blocked_by_correctness",
];
const CORRECTNESS_COUNTERS: &[&str] = &[
    "stale_exact",
    "false_exact",
    "unsafe_edit",
    "premature_ready",
    "mixed_generation",
    "wrong_root_or_namespace",
    "open_buffer_overwritten",
    "incompatible_or_corrupt_candidate_adopted",
    "provider_result_mismatch",
    "unexplained_success_empty",
];

const CANARY_MARKERS: &[&str] = &["CANARY_SECRET", "/home/", "BEGIN PRIVATE KEY"];

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "snapshot warm-start receipt check passed: {} required fields, {} fixtures conform to schema and cross-field rules",
        stats.required_fields, stats.fixtures
    );
    Ok(())
}

pub struct CheckStats {
    pub required_fields: usize,
    pub fixtures: usize,
}

fn validate(root: &Path) -> Result<CheckStats> {
    validate_control_suite(root)?;
    let schema = load_schema(root)?;
    validate_schema_document(&schema)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("{SCHEMA_PATH}: invalid schema: {error}"))?;
    let fixtures = discover_fixtures(root)?;
    if fixtures.is_empty() {
        bail!("{FIXTURE_DIR} holds no {FIXTURE_PREFIX}* fixture");
    }
    for path in &fixtures {
        let label = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        validate_fixture(&validator, &label, &text)?;
        println!("  {label}: conforms");
    }
    Ok(CheckStats { required_fields: REQUIRED.len(), fixtures: fixtures.len() })
}

fn validate_fixture(validator: &jsonschema::Validator, label: &str, text: &str) -> Result<()> {
    let value: Value = serde_json::from_str(text)
        .with_context(|| format!("{label}: fixture is not valid JSON"))?;
    let violations: Vec<String> =
        validator.iter_errors(&value).map(|error| error.to_string()).collect();
    if !violations.is_empty() {
        bail!("{label}: schema violations: {}", violations.join("; "));
    }
    validate_receipt(&value).with_context(|| format!("{label}: cross-field violations"))?;
    Ok(())
}

pub fn validate_receipt(receipt: &Value) -> Result<()> {
    let mut errors = Vec::new();
    if receipt.get("schema_version").and_then(Value::as_str) != Some(VERSION) {
        errors.push("schema_version must be snapshot_warm_start_receipt.v1".to_owned());
    }
    reject_canaries(receipt, &mut errors);
    reconcile_samples(receipt, &mut errors);
    validate_work_block(receipt.get("work"), &mut errors);
    validate_correctness_and_result(receipt, &mut errors);
    validate_comparison_admission(receipt, &mut errors);
    validate_phase_planes(receipt, &mut errors);
    validate_subject_comparison(receipt.get("subject"), &mut errors);
    if errors.is_empty() {
        return Ok(());
    }
    bail!("snapshot warm-start receipt violations: {}", errors.join("; "));
}

fn is_admission(disposition: &str) -> bool {
    matches!(disposition, "admit_default_candidate" | "admit_opt_in_only" | "admit_shadow_only")
}

fn counter_sum(node: Option<&Value>, names: &[&str]) -> u64 {
    let Some(object) = node.and_then(Value::as_object) else {
        return 0;
    };
    names.iter().filter_map(|name| object.get(*name).and_then(Value::as_u64)).sum()
}

fn validate_correctness_and_result(receipt: &Value, errors: &mut Vec<String>) {
    let correctness = receipt.get("correctness");
    let correctness_total = counter_sum(correctness, CORRECTNESS_COUNTERS);
    let disposition = receipt
        .get("result")
        .and_then(|node| node.get("disposition"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if correctness_total > 0 && disposition != "blocked_by_correctness" {
        errors.push(
            "result: nonzero correctness counters require disposition blocked_by_correctness"
                .to_owned(),
        );
    }
    if is_admission(disposition) && correctness_total > 0 {
        errors.push("result: correctness failure cannot produce admission".to_owned());
    }
    if is_admission(disposition)
        && correctness.and_then(|node| node.get("status")).and_then(Value::as_str)
            != Some("measured")
    {
        errors.push("result: admission requires measured correctness counters".to_owned());
    }
}

fn validate_work_block(work: Option<&Value>, errors: &mut Vec<String>) {
    let Some(work) = work else {
        errors.push("work: missing".to_owned());
        return;
    };
    let status = work.get("status").and_then(Value::as_str).unwrap_or("");
    let avoidance = work.get("avoidance_evidence").and_then(Value::as_str).unwrap_or("");
    let avoided = work.get("semantic_work_avoided").and_then(Value::as_u64).unwrap_or(0);
    if avoided > 0 && avoidance != "validated" {
        errors.push("work: semantic_work_avoided requires avoidance_evidence validated".to_owned());
    }
    let bytes_read = work.get("bytes_read").and_then(Value::as_u64).unwrap_or(0);
    let constructed = work.get("semantic_work_constructed").and_then(Value::as_u64).unwrap_or(0);
    let hit_full = work.get("cache_hit_followed_by_full_work").and_then(Value::as_u64).unwrap_or(0);
    if bytes_read > 0 && avoided > 0 && avoidance != "validated" {
        errors.push(
            "work: bytes loaded cannot imply work avoidance without validated evidence".to_owned(),
        );
    }
    if status == "measured" && bytes_read > 0 && constructed > 0 && hit_full > 0 && avoided > 0 {
        errors.push(
            "work: cache hit followed by equivalent full work cannot also claim avoidance"
                .to_owned(),
        );
    }
    if status != "measured" {
        for name in ["bytes_read", "semantic_work_avoided", "shards_adopted", "facts_adopted"] {
            if work.get(name).is_some() {
                errors.push(format!(
                    "work: {name} present while status is {status}; missing instrumentation is not zero"
                ));
            }
        }
    }
}

fn reconcile_samples(receipt: &Value, errors: &mut Vec<String>) {
    let Some(samples) = receipt.get("samples") else {
        errors.push("samples: missing".to_owned());
        return;
    };
    let total = samples.get("total").and_then(Value::as_u64).unwrap_or(0);
    let non_success = samples.get("non_success").and_then(Value::as_u64).unwrap_or(0);
    let Some(outcomes) = samples.get("outcomes").and_then(Value::as_object) else {
        errors.push("samples.outcomes: missing".to_owned());
        return;
    };
    let sum: u64 = outcomes.values().filter_map(Value::as_u64).sum();
    if sum != total {
        errors.push(format!(
            "samples: outcome counts sum to {sum} but total is {total}; dropped samples must remain in the denominator"
        ));
    }
    let non_success_computed: u64 = outcomes
        .iter()
        .filter(|(key, _)| *key != "success")
        .filter_map(|(_, value)| value.as_u64())
        .sum();
    if non_success_computed != non_success {
        errors.push(
            "samples: non_success must equal the sum of every non-success outcome bucket"
                .to_owned(),
        );
    }
}

fn validate_comparison_admission(receipt: &Value, errors: &mut Vec<String>) {
    let disposition = receipt
        .get("result")
        .and_then(|node| node.get("disposition"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(comparison) = receipt.get("comparison") else {
        return;
    };
    if comparison.get("status").and_then(Value::as_str) != Some("measured") {
        return;
    }
    let median = comparison.get("median_improvement_ratio").and_then(Value::as_f64).unwrap_or(0.0);
    let p95 = comparison.get("p95_regression_ratio").and_then(Value::as_f64).unwrap_or(0.0);
    let readiness =
        comparison.get("readiness_regression").and_then(Value::as_bool).unwrap_or(false);
    let setup_in_product =
        comparison.get("setup_cost_in_product_plane").and_then(Value::as_bool).unwrap_or(false);
    if is_admission(disposition) && p95 > 0.0 && median > 0.0 {
        errors.push(
            "comparison: median improvement cannot erase material p95 regression under admission"
                .to_owned(),
        );
    }
    if is_admission(disposition) && readiness {
        errors.push("comparison: readiness regression cannot admit".to_owned());
    }
    if is_admission(disposition) && setup_in_product {
        errors.push(
            "comparison: setup/oracle cost folded into product plane cannot admit".to_owned(),
        );
    }
}

fn validate_phase_planes(receipt: &Value, errors: &mut Vec<String>) {
    let Some(phases) = receipt.get("phases").and_then(Value::as_array) else {
        errors.push("phases: must be a non-empty array".to_owned());
        return;
    };
    if phases.is_empty() {
        errors.push("phases: empty".to_owned());
    }
    let mut seen = BTreeSet::new();
    for phase in phases {
        let name = phase.get("name").and_then(Value::as_str).unwrap_or("");
        if !seen.insert(name.to_owned()) {
            errors.push(format!("phases: duplicate row for {name}"));
        }
        let plane = phase.get("plane").and_then(Value::as_str).unwrap_or("");
        if plane == "setup"
            && receipt
                .get("result")
                .and_then(|node| node.get("disposition"))
                .and_then(Value::as_str)
                .is_some_and(is_admission)
            && phase.get("timing").and_then(|node| node.get("status")).and_then(Value::as_str)
                == Some("measured")
            && receipt
                .get("comparison")
                .and_then(|node| node.get("setup_cost_in_product_plane"))
                .and_then(Value::as_bool)
                == Some(true)
        {
            errors.push(
                "phases: measured setup timing cannot be folded into product admission".to_owned(),
            );
        }
    }
}

fn validate_subject_comparison(subject: Option<&Value>, errors: &mut Vec<String>) {
    let Some(subject) = subject else {
        errors.push("subject: missing".to_owned());
        return;
    };
    let Some(comparison) = subject.get("comparison") else {
        errors.push("subject.comparison: missing".to_owned());
        return;
    };
    let status = comparison.get("status").and_then(Value::as_str).unwrap_or("");
    match status {
        "comparable" if comparison.get("peer_receipt_id").is_none() => errors
            .push("subject.comparison: comparable subjects require peer_receipt_id".to_owned()),
        "incomparable" => {
            let families = comparison
                .get("difference_families")
                .and_then(Value::as_array)
                .map(|values| values.len())
                .unwrap_or(0);
            if families == 0 {
                errors.push(
                    "subject.comparison: incomparable subjects require difference_families"
                        .to_owned(),
                );
            }
        }
        _ => {}
    }
}

fn reject_canaries(value: &Value, errors: &mut Vec<String>) {
    fn walk(node: &Value, errors: &mut Vec<String>) {
        match node {
            Value::String(text) => {
                for marker in CANARY_MARKERS {
                    if text.contains(marker) {
                        errors.push(format!(
                            "private-safe: field contains forbidden marker {marker:?}"
                        ));
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, errors)),
            Value::Object(map) => map.values().for_each(|item| walk(item, errors)),
            _ => {}
        }
    }
    walk(value, errors);
}

/// Deterministic semantic identity digest: sorted-key canonical JSON of the
/// load-bearing subject identity, excluding timestamps and host-private paths.
pub fn semantic_identity_digest(receipt: &Value) -> Result<String> {
    let subject = receipt
        .get("subject")
        .ok_or_else(|| color_eyre::eyre::eyre!("subject required for semantic identity"))?;
    let preparation = receipt
        .get("preparation")
        .ok_or_else(|| color_eyre::eyre::eyre!("preparation required for semantic identity"))?;
    let mut map = Map::new();
    map.insert("subject".to_owned(), subject.clone());
    map.insert("preparation".to_owned(), preparation.clone());
    let canonical = canonical_json(&Value::Object(map))?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

fn canonical_json(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("null".to_owned()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(text) => Ok(format!("\"{}\"", escape_json_string(text))),
        Value::Array(items) => {
            let mut encoded = Vec::with_capacity(items.len());
            for item in items {
                encoded.push(canonical_json(item)?);
            }
            Ok(format!("[{}]", encoded.join(",")))
        }
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut members = Vec::with_capacity(keys.len());
            for key in keys {
                let item = map.get(&key).expect("sorted key exists");
                members.push(format!("\"{}\":{}", escape_json_string(&key), canonical_json(item)?));
            }
            Ok(format!("{{{}}}", members.join(",")))
        }
    }
}

fn escape_json_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn validate_control_suite(root: &Path) -> Result<()> {
    let path = root.join(CONTROL_SUITE_PATH);
    let source =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let controls = source.lines().filter(|line| line.trim() == "#[test]").count();
    if controls < CONTROL_SUITE_FLOOR {
        bail!(
            "{}: {controls} controls found, expected at least {CONTROL_SUITE_FLOOR}",
            path.display()
        );
    }
    Ok(())
}

fn load_schema(root: &Path) -> Result<Value> {
    let path = root.join(SCHEMA_PATH);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn validate_schema_document(schema: &Value) -> Result<()> {
    let mut errors = Vec::new();
    require_string(schema, &["$id"], SCHEMA_ID, &mut errors);
    require_string(schema, &["properties", "schema_version", "const"], VERSION, &mut errors);
    require_set(schema, &["required"], REQUIRED, &mut errors);
    require_set(schema, &["$defs", "evidence_status", "enum"], EVIDENCE_STATUS, &mut errors);
    require_set(
        schema,
        &["$defs", "admission_disposition", "enum"],
        ADMISSION_DISPOSITIONS,
        &mut errors,
    );
    if lookup(schema, &["$defs", "git_object_name", "pattern"]).and_then(Value::as_str)
        != Some("^[0-9a-f]{40}$")
    {
        errors.push("$defs.git_object_name.pattern must pin exact 40-hex object names".to_owned());
    }
    if !errors.is_empty() {
        bail!("snapshot warm-start receipt schema violations: {}", errors.join("; "));
    }
    Ok(())
}

fn discover_fixtures(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let dir = root.join(FIXTURE_DIR);
    let mut found = Vec::new();
    collect_fixtures(&dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn collect_fixtures(dir: &Path, found: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry.with_context(|| format!("failed to read {}", dir.display()))?.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        let claims_receipt_name = path.file_name().is_some_and(|name| {
            let bytes = name.as_encoded_bytes();
            bytes.starts_with(FIXTURE_PREFIX.as_bytes()) && bytes.ends_with(b".json")
        });
        if metadata.is_symlink() {
            if claims_receipt_name {
                bail!("{}: fixture discovery refuses symlink receipt", path.display());
            }
            continue;
        }
        if metadata.is_dir() {
            collect_fixtures(&path, found)?;
            continue;
        }
        if claims_receipt_name {
            found.push(path);
        }
    }
    Ok(())
}

fn lookup<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for part in path {
        value = value.get(*part)?;
    }
    Some(value)
}

fn require_string(schema: &Value, path: &[&str], expected: &str, errors: &mut Vec<String>) {
    if lookup(schema, path).and_then(Value::as_str) != Some(expected) {
        errors.push(format!("{} must be {expected:?}", path.join(".")));
    }
}

fn require_set(schema: &Value, path: &[&str], expected: &[&str], errors: &mut Vec<String>) {
    let Some(values) = lookup(schema, path).and_then(Value::as_array) else {
        errors.push(format!("{} must be a string array", path.join(".")));
        return;
    };
    let actual = values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    for missing in wanted.difference(&actual) {
        errors.push(format!("{} missing {missing:?}", path.join(".")));
    }
    for extra in actual.difference(&wanted) {
        errors.push(format!("{} contains unsupported {extra:?}", path.join(".")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_with;
    use serde_json::json;

    const MEASURED: &str = include_str!("../../fixtures/snapshot_warm_start_receipt.v1.json");
    const UNINSTRUMENTED: &str =
        include_str!("../../fixtures/snapshot_warm_start_receipt.v1.uninstrumented.json");

    fn schema_value() -> Value {
        must_with(
            serde_json::from_str(include_str!(
                "../../../schemas/snapshot_warm_start_receipt.v1.schema.json"
            )),
            "schema JSON",
        )
    }

    fn validator() -> jsonschema::Validator {
        must_with(jsonschema::validator_for(&schema_value()), "schema compiles")
    }

    fn measured() -> Value {
        must_with(serde_json::from_str(MEASURED), "measured fixture JSON")
    }

    fn uninstrumented() -> Value {
        must_with(serde_json::from_str(UNINSTRUMENTED), "uninstrumented fixture JSON")
    }

    fn check(value: &Value) -> Result<()> {
        validate_fixture(&validator(), "candidate", &value.to_string())
    }

    #[test]
    fn committed_fixtures_conform() {
        for (label, value) in [("measured", measured()), ("uninstrumented", uninstrumented())] {
            if let Err(error) = check(&value) {
                panic!("{label} fixture: {error:#}");
            }
        }
    }

    #[test]
    fn negative_control_bytes_loaded_while_work_still_runs() {
        let mut value = measured();
        value["work"]["semantic_work_avoided"] = json!(50);
        value["work"]["avoidance_evidence"] = json!("not_claimed");
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_faster_stale_provider_is_not_admission() {
        let mut value = measured();
        value["correctness"]["stale_exact"] = json!(1);
        value["result"]["disposition"] = json!("admit_default_candidate");
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_median_improvement_with_p95_regression() {
        let mut value = measured();
        value["result"]["disposition"] = json!("admit_shadow_only");
        value["comparison"]["median_improvement_ratio"] = json!(0.2);
        value["comparison"]["p95_regression_ratio"] = json!(0.15);
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_warm_mislabeled_cold_is_incomparable() {
        let mut value = measured();
        value["preparation"]["mode"] = json!("cold");
        value["subject"]["comparison"]["status"] = json!("comparable");
        value["subject"]["comparison"]["peer_receipt_id"] = json!("peer");
        assert!(check(&value).is_ok());
        value["subject"]["comparison"]["status"] = json!("incomparable");
        value["subject"]["comparison"]["difference_families"] = json!(["preparation_mode"]);
        must_with(check(&value), "incomparable with reason");
    }

    #[test]
    fn negative_control_different_subjects_incomparable() {
        let mut value = measured();
        value["subject"]["comparison"]["status"] = json!("incomparable");
        value["subject"]["comparison"]["difference_families"] = json!(["backend"]);
        value["subject"]["comparison"].as_object_mut().unwrap().remove("peer_receipt_id");
        must_with(check(&value), "backend difference");
    }

    #[test]
    fn negative_control_corrupt_samples_remain_in_denominator() {
        let mut value = measured();
        value["samples"]["outcomes"]["corrupt"] = json!(1);
        value["samples"]["total"] = json!(5);
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_missing_work_counter_not_zero() {
        let mut value = uninstrumented();
        value["work"]["bytes_read"] = json!(10);
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_nonzero_correctness_requires_blocked_disposition() {
        let mut value = measured();
        value["correctness"]["false_exact"] = json!(2);
        value["result"]["disposition"] = json!("no_material_benefit");
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_setup_cost_cannot_admit() {
        let mut value = measured();
        value["result"]["disposition"] = json!("admit_opt_in_only");
        value["comparison"]["setup_cost_in_product_plane"] = json!(true);
        assert!(check(&value).is_err());
    }

    #[test]
    fn negative_control_input_order_does_not_change_semantic_digest() {
        let first = semantic_identity_digest(&measured()).expect("digest");
        let second = semantic_identity_digest(&measured()).expect("digest");
        assert_eq!(first, second);
        let mut reordered = json!({
            "preparation": measured()["preparation"].clone(),
            "subject": measured()["subject"].clone()
        });
        let from_reordered = semantic_identity_digest(&json!({
            "subject": reordered["subject"].clone(),
            "preparation": reordered["preparation"].clone()
        }))
        .expect("digest");
        assert_eq!(first, from_reordered);
        let _ = reordered;
    }

    #[test]
    fn negative_control_secret_canaries_leak() {
        let mut value = measured();
        value["limitations"] = json!(["contains CANARY_SECRET marker"]);
        assert!(check(&value).is_err());
    }

    #[test]
    fn serialization_is_deterministic() {
        let receipt = measured();
        let first = must_with(serde_json::to_string(&receipt), "serialize");
        let second = must_with(serde_json::to_string(&receipt), "serialize");
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let mut value = measured();
        value["schema_version"] = json!("snapshot_warm_start_receipt.v2");
        assert!(check(&value).is_err());
    }

    #[test]
    fn schema_pins_evidence_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["evidence_status"]["enum"] = json!(["measured"]);
        assert!(validate_schema_document(&schema).is_err());
    }

    #[test]
    fn discovery_reaches_nested_fixtures() {
        let root = must_with(tempfile::tempdir(), "tempdir");
        let fixtures = root.path().join(FIXTURE_DIR);
        let nested = fixtures.join("nested");
        must_with(fs::create_dir_all(&nested), "mkdir");
        let top = fixtures.join(format!("{FIXTURE_PREFIX}.json"));
        let deep = nested.join(format!("{FIXTURE_PREFIX}.deep.json"));
        must_with(fs::write(&top, MEASURED), "write top");
        must_with(fs::write(&deep, MEASURED), "write deep");
        let found = must_with(discover_fixtures(root.path()), "discover");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn validate_control_suite_floor_is_live() {
        let root = project_root().expect("root");
        must_with(validate_control_suite(&root), "control suite floor");
    }

    #[test]
    fn canonical_json_sorts_object_keys() {
        let left = json!({"b": 1, "a": 2});
        let right = json!({"a": 2, "b": 1});
        assert_eq!(canonical_json(&left).expect("left"), canonical_json(&right).expect("right"));
    }

    #[test]
    fn rejects_empty_phases() {
        let mut value = measured();
        value["phases"] = json!([]);
        assert!(check(&value).is_err());
    }

    #[test]
    fn rejects_duplicate_phase_rows() {
        let mut value = measured();
        value["phases"] = json!([
            {"name": "manifest_integrity_load", "plane": "storage", "sample_outcome": "success", "timing": {"status": "measured", "wall_ns": 1}, "instrumentation": "complete"},
            {"name": "manifest_integrity_load", "plane": "storage", "sample_outcome": "success", "timing": {"status": "measured", "wall_ns": 2}, "instrumentation": "complete"}
        ]);
        assert!(check(&value).is_err());
    }

    #[test]
    fn open_buffer_invalidation_is_a_correctness_counter() {
        let mut value = measured();
        value["correctness"]["open_buffer_overwritten"] = json!(1);
        value["result"]["disposition"] = json!("blocked_by_correctness");
        must_with(check(&value), "blocked");
    }

    #[test]
    fn cache_hit_with_full_work_and_avoidance_conflict() {
        let mut value = measured();
        value["work"]["cache_hit_followed_by_full_work"] = json!(1);
        value["work"]["semantic_work_avoided"] = json!(3);
        value["work"]["avoidance_evidence"] = json!("validated");
        assert!(check(&value).is_err());
    }

    #[test]
    fn instrument_schema_is_pinned() {
        let mut value = measured();
        value["instrument"]["schema"] = json!("legacy");
        assert!(check(&value).is_err());
    }

    #[test]
    fn benchmark_identities_extend_editor_and_runner_authorities() {
        let subject = measured()["subject"]["benchmark_identities"].clone();
        assert_eq!(subject["editor_work"]["schema"], "editor_work.v1");
        assert_eq!(subject["stable_runner"]["schema"], "stable_runner.v1");
        assert_eq!(subject["compiler_pipeline"]["schema"], "compiler_pipeline.v1");
    }
}
