use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};
use jsonschema::Validator;
use serde_json::Value;

type TestResult = Result<()>;

const TOP_LEVEL_REQUIRED: &[&str] = &[
    "schema_version",
    "subsystem",
    "generated_at",
    "commit",
    "cadence",
    "denominator",
    "families",
    "metrics",
    "legacy_population",
    "failure_packets",
    "gold_drift",
    "metric_runtime",
];

const DENOMINATOR_REQUIRED: &[&str] = &[
    "fixture_count",
    "fixture_family_count",
    "scored_line_count",
    "scored_symbol_count",
    "fully_labeled_region_count",
    "partial_labeled_region_count",
    "unknown_region_count",
    "negative_region_count",
    "dynamic_boundary_case_count",
    "unsupported_construct_case_count",
    "real_project_file_count",
    "generated_fixture_count",
    "hand_labeled_fixture_count",
];

const INVESTIGATION_ROW_REQUIRED: &[&str] = &[
    "metric",
    "state",
    "value",
    "sample_count",
    "transformation_profile",
    "evidence_class",
    "terminal_disposition",
    "reason",
    "packet_policy",
    "floor_eligible",
];

const LEGACY_POPULATION_REQUIRED: &[&str] = &[
    "transformation_profile",
    "population_identity",
    "aggregate_metric",
    "population_total_count",
    "population_applied_count",
    "population_unclassified_count",
    "manifest_schema_version",
];

#[test]
fn parser_accuracy_schema_declares_required_contract() -> TestResult {
    let schema = read_json(&repo_root()?.join(".ci/schemas/parser-accuracy.schema.json"))?;
    assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(schema["properties"]["subsystem"]["const"], "parser_accuracy");

    let required = string_array(&schema["required"], "top-level required fields")?;
    for field in TOP_LEVEL_REQUIRED {
        assert!(
            required.iter().any(|item| item == field),
            "schema is missing required top-level field {field}"
        );
    }

    let metric_row = &schema["$defs"]["metricRow"];
    let one_of = metric_row["oneOf"].as_array().ok_or_else(|| {
        eyre!("metricRow must define measured, insufficient-data, and investigation alternatives")
    })?;
    assert_eq!(
        one_of.len(),
        3,
        "metricRow must have exactly measured, insufficient_data, and investigation_only states"
    );
    let mut states = Vec::new();
    for alternative in one_of {
        let reference = alternative["$ref"]
            .as_str()
            .ok_or_else(|| eyre!("metricRow alternatives must be $refs"))?;
        states.push(reference);
    }
    assert!(states.iter().any(|state| state.ends_with("measuredMetric")));
    assert!(states.iter().any(|state| state.ends_with("insufficientDataMetric")));
    assert!(states.iter().any(|state| state.ends_with("investigationOnlyMetric")));

    let investigation = &schema["$defs"]["investigationOnlyMetric"];
    let investigation_required =
        string_array(&investigation["required"], "investigation row required fields")?;
    for field in INVESTIGATION_ROW_REQUIRED {
        assert!(
            investigation_required.iter().any(|item| item == field),
            "investigation rows must require trust field {field}"
        );
    }
    assert_eq!(investigation["properties"]["state"]["const"], "investigation_only");
    assert_eq!(investigation["properties"]["evidence_class"]["const"], "investigation_only");
    assert_eq!(investigation["properties"]["terminal_disposition"]["enum"][0], "not_proven");
    assert_eq!(investigation["properties"]["packet_policy"]["enum"][0], "none");
    assert_eq!(investigation["properties"]["floor_eligible"]["const"], false);

    let population = &schema["$defs"]["legacyPopulation"];
    let population_required =
        string_array(&population["required"], "legacy population required fields")?;
    for field in LEGACY_POPULATION_REQUIRED {
        assert!(
            population_required.iter().any(|item| item == field),
            "legacy population must require identity field {field}"
        );
    }
    assert_eq!(population["properties"]["population_identity"]["pattern"], "^sha256:[0-9a-f]{64}$");

    Ok(())
}

#[test]
fn parser_accuracy_example_artifact_matches_schema_contract() -> TestResult {
    let root = repo_root()?;
    let artifact =
        read_json(&root.join("xtask/tests/fixtures/parser-accuracy/example-artifact.json"))?;

    for field in TOP_LEVEL_REQUIRED {
        assert!(artifact.get(*field).is_some(), "example artifact is missing {field}");
    }
    assert_eq!(artifact["schema_version"], 1);
    assert_eq!(artifact["subsystem"], "parser_accuracy");
    assert!(
        ["pr", "merge_gate", "nightly", "release"].contains(
            &artifact["cadence"]
                .as_str()
                .ok_or_else(|| eyre!("cadence must be a string in example artifact"))?,
        ),
        "cadence must match the schema enum"
    );

    let validator = compiled_schema_validator(&root)?;
    let violations: Vec<String> =
        validator.iter_errors(&artifact).map(|error| error.to_string()).collect();
    assert!(
        violations.is_empty(),
        "example artifact violates the parser accuracy schema: {}",
        violations.join("; ")
    );

    validate_denominator(&artifact["denominator"])?;
    validate_families(&artifact)?;
    validate_metric_rows(&artifact)?;
    validate_legacy_population(&artifact["legacy_population"])?;
    validate_gold_drift(&artifact["gold_drift"])?;
    validate_metric_runtime(&artifact["metric_runtime"])?;

    Ok(())
}

/// Negative controls: each mutation of the valid example artifact must be
/// rejected by the schema independently (#13656).
#[test]
fn parser_accuracy_schema_rejects_untyped_and_contradictory_trust_shapes() -> TestResult {
    let root = repo_root()?;
    let valid =
        read_json(&root.join("xtask/tests/fixtures/parser-accuracy/example-artifact.json"))?;
    let validator = compiled_schema_validator(&root)?;
    assert!(validator.is_valid(&valid), "the example artifact must remain the valid baseline");

    let investigation_index = valid["metrics"]
        .as_array()
        .ok_or_else(|| eyre!("example artifact must contain a metrics array"))?
        .iter()
        .position(|row| row["state"] == "investigation_only")
        .ok_or_else(|| eyre!("example artifact must contain an investigation_only row"))?;
    let measured_index = valid["metrics"]
        .as_array()
        .ok_or_else(|| eyre!("example artifact must contain a metrics array"))?
        .iter()
        .position(|row| row["state"] == "measured")
        .ok_or_else(|| eyre!("example artifact must contain a measured row"))?;

    let schema_rejects = |label: &str, artifact: &Value| {
        assert!(!validator.is_valid(artifact), "schema must reject {label}");
    };

    let mutate = |label: &str, mutate: &dyn Fn(&mut Value)| -> TestResult {
        let mut artifact = valid.clone();
        mutate(&mut artifact);
        schema_rejects(label, &artifact);
        Ok(())
    };

    // Missing required trust fields fail closed.
    mutate("an investigation row without terminal_disposition", &|artifact: &mut Value| {
        if let Some(fields) = artifact["metrics"][investigation_index].as_object_mut() {
            fields.remove("terminal_disposition");
        }
    })?;
    mutate("an investigation row without a packet policy", &|artifact: &mut Value| {
        if let Some(fields) = artifact["metrics"][investigation_index].as_object_mut() {
            fields.remove("packet_policy");
        }
    })?;
    mutate("an investigation row without floor eligibility", &|artifact: &mut Value| {
        if let Some(fields) = artifact["metrics"][investigation_index].as_object_mut() {
            fields.remove("floor_eligible");
        }
    })?;

    // Unknown trust, disposition, and packet values fail closed.
    mutate("an unknown terminal disposition", &|artifact: &mut Value| {
        artifact["metrics"][investigation_index]["terminal_disposition"] = "pass".into();
    })?;
    mutate("an unknown evidence class", &|artifact: &mut Value| {
        artifact["metrics"][investigation_index]["evidence_class"] = "trusted".into();
    })?;
    mutate("a parser-defect packet policy on investigation evidence", &|artifact: &mut Value| {
        artifact["metrics"][investigation_index]["packet_policy"] = "defect".into();
    })?;

    // Floor-admitted investigation evidence fails closed.
    mutate("floor-eligible investigation evidence", &|artifact: &mut Value| {
        artifact["metrics"][investigation_index]["floor_eligible"] = true.into();
    })?;

    // Zero-sample investigation rows are not observations.
    mutate("a zero-sample investigation row", &|artifact: &mut Value| {
        artifact["metrics"][investigation_index]["sample_count"] = 0.into();
    })?;

    // Contradictory shapes fail closed: measured rows cannot carry trust data.
    mutate("a measured row carrying evidence_class", &|artifact: &mut Value| {
        artifact["metrics"][measured_index]["evidence_class"] = "investigation_only".into();
    })?;

    // A missing retained population fails closed.
    mutate("an artifact without retained population evidence", &|artifact: &mut Value| {
        if let Some(fields) = artifact.as_object_mut() {
            fields.remove("legacy_population");
        }
    })?;

    // Population identity movement and malformed identities fail closed.
    mutate("a population identity without the sha256 tag", &|artifact: &mut Value| {
        artifact["legacy_population"]["population_identity"] = "legacy-digest".into();
    })?;
    mutate("a population identity digest that is not hexadecimal", &|artifact: &mut Value| {
        artifact["legacy_population"]["population_identity"] =
            Value::String(format!("sha256:{}", "z".repeat(64)));
    })?;
    mutate("a truncated population identity digest", &|artifact: &mut Value| {
        artifact["legacy_population"]["population_identity"] =
            Value::String(format!("sha256:{}", "a".repeat(63)));
    })?;

    // A zero population is not a retained population.
    mutate("an empty retained population", &|artifact: &mut Value| {
        artifact["legacy_population"]["population_total_count"] = 0.into();
    })?;

    Ok(())
}

fn compiled_schema_validator(root: &Path) -> Result<Validator> {
    let schema = read_json(&root.join(".ci/schemas/parser-accuracy.schema.json"))?;
    jsonschema::validator_for(&schema)
        .map_err(|error| eyre!("parser accuracy schema must compile: {error}"))
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .ok_or_else(|| eyre!("xtask manifest should have a workspace parent"))?;
    Ok(root.to_path_buf())
}

fn read_json(path: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn string_array(value: &Value, label: &str) -> Result<Vec<String>> {
    let items = value.as_array().ok_or_else(|| eyre!("{label} must be an array"))?;
    let mut output = Vec::with_capacity(items.len());
    for item in items {
        let text = item.as_str().ok_or_else(|| eyre!("{label} must contain strings"))?;
        output.push(text.to_string());
    }
    Ok(output)
}

fn validate_denominator(value: &Value) -> TestResult {
    let object = value.as_object().ok_or_else(|| eyre!("denominator must be an object"))?;
    for field in DENOMINATOR_REQUIRED {
        object
            .get(*field)
            .and_then(Value::as_u64)
            .ok_or_else(|| eyre!("denominator field {field} must be a non-negative integer"))?;
    }
    Ok(())
}

fn validate_families(artifact: &Value) -> TestResult {
    let families =
        artifact["families"].as_array().ok_or_else(|| eyre!("families must be an array"))?;
    assert!(!families.is_empty(), "example artifact should include at least one family");
    for family in families {
        assert!(
            family["family"].as_str().is_some_and(|value| !value.is_empty()),
            "family rows require a non-empty family name"
        );
        assert!(family["fixture_count"].as_u64().is_some(), "family rows require fixture_count");
        let label_modes = family["label_modes"]
            .as_array()
            .ok_or_else(|| eyre!("label_modes must be an array"))?;
        assert!(!label_modes.is_empty(), "family rows require label_modes");
    }
    Ok(())
}

fn validate_metric_rows(artifact: &Value) -> TestResult {
    let metrics =
        artifact["metrics"].as_array().ok_or_else(|| eyre!("metrics must be an array"))?;
    assert!(!metrics.is_empty(), "example artifact should include metric rows");

    let mut saw_measured = false;
    let mut saw_measured_line = false;
    let mut saw_measured_ast = false;
    let mut saw_measured_symbol = false;
    let mut saw_investigation = false;

    for metric in metrics {
        let state =
            metric["state"].as_str().ok_or_else(|| eyre!("metric state must be a string"))?;
        let sample_count = metric["sample_count"]
            .as_u64()
            .ok_or_else(|| eyre!("metric sample_count must be a non-negative integer"))?;
        match state {
            "measured" => {
                saw_measured = true;
                assert!(sample_count > 0, "measured rows require a positive sample_count");
                assert!(metric.get("value").and_then(Value::as_f64).is_some());
                assert!(metric.get("direction").and_then(Value::as_str).is_some());
                assert!(metric.get("confidence").and_then(Value::as_str).is_some());
                if metric["metric"].as_str() == Some("line_construct_f1") {
                    saw_measured_line = true;
                }
                if metric["metric"].as_str() == Some("ast_node_kind_f1") {
                    saw_measured_ast = true;
                }
                if metric["metric"].as_str() == Some("symbol_decl_f1") {
                    saw_measured_symbol = true;
                }
            }
            "insufficient_data" => {
                assert!(metric.get("reason").and_then(Value::as_str).is_some());
            }
            "investigation_only" => {
                saw_investigation = true;
                assert!(sample_count > 0, "investigation rows require a positive sample_count");
                for field in INVESTIGATION_ROW_REQUIRED {
                    assert!(
                        metric.get(*field).is_some(),
                        "investigation rows require trust field {field}"
                    );
                }
                assert_eq!(metric["evidence_class"], "investigation_only");
                assert_eq!(metric["terminal_disposition"], "not_proven");
                assert_eq!(metric["packet_policy"], "none");
                assert_eq!(metric["floor_eligible"], false);
            }
            other => return Err(eyre!("unexpected metric state {other}")),
        }
    }

    assert!(saw_measured, "example should include at least one measured denominator row");
    assert!(saw_measured_line, "example should include measured line F1 row");
    assert!(saw_measured_ast, "example should include measured AST F1 row");
    assert!(saw_measured_symbol, "example should include measured symbol F1 row");
    assert!(saw_investigation, "example should include a typed investigation row");
    Ok(())
}

fn validate_legacy_population(population: &Value) -> TestResult {
    let object =
        population.as_object().ok_or_else(|| eyre!("legacy_population must be an object"))?;
    for field in LEGACY_POPULATION_REQUIRED {
        assert!(object.get(*field).is_some(), "legacy_population requires identity field {field}");
    }
    let identity = object
        .get("population_identity")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("legacy_population identity must be a string"))?;
    let digest = identity
        .strip_prefix("sha256:")
        .ok_or_else(|| eyre!("population identity must be sha256-tagged"))?;
    assert_eq!(digest.len(), 64, "population identity digest must be 64 hex characters");
    assert!(
        digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "population identity digest must be hexadecimal"
    );
    let total = object
        .get("population_total_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("legacy_population total count must be an integer"))?;
    let applied = object
        .get("population_applied_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("legacy_population applied count must be an integer"))?;
    let unclassified = object
        .get("population_unclassified_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("legacy_population unclassified count must be an integer"))?;
    assert!(
        applied + unclassified == total,
        "legacy population counts must close over retained rows"
    );
    Ok(())
}

fn validate_gold_drift(value: &Value) -> TestResult {
    let object = value.as_object().ok_or_else(|| eyre!("gold_drift must be an object"))?;
    for (key, count) in object {
        assert!(count.as_u64().is_some(), "gold_drift field {key} must be a non-negative integer");
    }
    Ok(())
}

fn validate_metric_runtime(value: &Value) -> TestResult {
    assert!(
        value["runtime_ms"].as_f64().is_some_and(|runtime| runtime >= 0.0),
        "metric_runtime.runtime_ms must be non-negative"
    );
    for field in ["timeout_count", "flake_count", "artifact_size_bytes"] {
        assert!(
            value[field].as_u64().is_some(),
            "metric_runtime.{field} must be a non-negative integer"
        );
    }
    if !value["allocated_bytes"].is_null() {
        assert!(
            value["allocated_bytes"].as_u64().is_some(),
            "metric_runtime.allocated_bytes must be a non-negative integer when present"
        );
    }
    if !value["allocation_count"].is_null() {
        assert!(
            value["allocation_count"].as_u64().is_some(),
            "metric_runtime.allocation_count must be a non-negative integer when present"
        );
    }
    Ok(())
}
