//! Controls on the production admission and typed metric seams (#13424).

use super::super::lsp_stats_impl::aggregate_admitted_receipts;
use super::*;
use serde_json::json;
use std::cell::Cell;

struct Fixture {
    _temp: tempfile::TempDir,
    receipts: PathBuf,
    matrix: PathBuf,
    schema: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir(&receipts)?;
        let matrix = temp.path().join("matrix.json");
        fs::write(
            &matrix,
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "workflows": [{"id":"known", "scenario_file":"known.rs",
                    "subsystem_owner":"test", "ci_tier":"pr",
                    "measures":["cross_file_definition_success_rate"]}]
            }))?,
        )?;
        let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.ci/schemas/ux-scenario-run.schema.json");
        Ok(Self { _temp: temp, receipts, matrix, schema })
    }

    fn write(&self, name: &str, value: &Value) -> Result<PathBuf> {
        let path = self.receipts.join(name);
        fs::write(&path, serde_json::to_vec(value)?)?;
        Ok(path)
    }

    fn load(&self) -> Result<receipt_set::AdmittedLegacyUxReceiptSet> {
        load_admitted_set(&self.receipts, &self.matrix, &self.schema)
    }

    fn error(&self) -> Result<String> {
        match self.load() {
            Ok(_) => bail!("invalid receipt set was admitted"),
            Err(error) => Ok(format!("{error:#}")),
        }
    }
}

fn receipt(test: &str) -> Value {
    json!({
        "kind":"ux_scenario_run", "schema_version":1,
        "measured_at":"2026-08-28T00:00:00Z",
        "run_identity":{"sha":"abcdef12", "platform":"Linux", "branch":"main"},
        "workflow_id":"known", "scenario_file":"known.rs", "test_name":test,
        "component":"completion", "ci_tier":"pr", "result":"pass",
        "duration_ms":10.0, "time_to_first_useful_result_ms":5.0,
        "operation_timings":[{"operation":"completion", "time_to_first_useful_result_ms":5.0}],
        "assertions":{"passed":1, "failed":0, "basis":"instrumented"},
        "canonical_repro":"cargo test fixture", "friendly_repro":"just ux-tests"
    })
}

fn failed(test: &str) -> Value {
    let mut value = receipt(test);
    value["result"] = json!("fail");
    value["failure_class"] = json!("provider_regression");
    value["assertions"]["passed"] = json!(0);
    value["assertions"]["failed"] = json!(1);
    value
}

fn normalized_scorecard(
    set: &receipt_set::AdmittedLegacyUxReceiptSet,
    matrix: &Path,
) -> Result<Value> {
    let mut value =
        serde_json::to_value(aggregate_admitted_receipts(set.receipts(), matrix, None)?)?;
    value
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("scorecard is not an object"))?
        .remove("measured_at");
    Ok(value)
}

#[test]
fn finalized_set_ignores_malformed_valid_and_removed_source() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.write("one.json", &receipt("one"))?;
    let set = fixture.load()?;
    let expected = normalized_scorecard(&set, &fixture.matrix)?;
    assert_eq!(expected["top_line"]["workflow_pass_rate"]["value"], 1.0);

    fs::write(&path, b"{")?;
    assert_eq!(normalized_scorecard(&set, &fixture.matrix)?, expected);
    // The retained metric-test adapter is the historical rereading behavior.
    // This opposite control must lose the malformed row, unlike the new path.
    let reread = super::super::lsp_stats_impl::aggregate_from_receipts(
        &fixture.receipts,
        &fixture.matrix,
        None,
    )?;
    assert_eq!(reread.top_line.workflow_pass_rate.state, "insufficient_data");
    fs::write(&path, serde_json::to_vec(&failed("one"))?)?;
    assert_eq!(normalized_scorecard(&set, &fixture.matrix)?, expected);
    let reread = super::super::lsp_stats_impl::aggregate_from_receipts(
        &fixture.receipts,
        &fixture.matrix,
        None,
    )?;
    assert_eq!(reread.top_line.workflow_pass_rate.value, Some(0.0));
    let later = fixture.load()?;
    assert_eq!(
        normalized_scorecard(&later, &fixture.matrix)?["top_line"]["workflow_pass_rate"]["value"],
        0.0
    );
    fs::rename(&fixture.receipts, fixture.receipts.with_file_name("retired-input"))?;
    assert_eq!(normalized_scorecard(&set, &fixture.matrix)?, expected);
    Ok(())
}

#[test]
fn changed_path_set_is_a_typed_unstable_input_error() -> Result<()> {
    for add in [true, false] {
        let fixture = Fixture::new()?;
        let path = fixture.write("one.json", &receipt("one"))?;
        let workflows = load_workflows(&fixture.matrix)?;
        let validator = load_receipt_validator(&fixture.schema)?;
        let result = receipt_set::load_with_source(
            &fixture.receipts,
            || receipt_set::selected_paths(&fixture.receipts),
            |candidate| {
                let bytes = fs::read(candidate)?;
                if add {
                    fixture.write("added.json", &receipt("added"))?;
                } else {
                    fs::remove_file(&path)?;
                }
                Ok(bytes)
            },
            |path, value| classify_candidate(path, value, &workflows, &validator),
        );
        match result {
            Ok(_) => bail!("changed path set was admitted"),
            Err(error) => assert!(
                error.downcast_ref::<receipt_set::UnstableReceiptInput>().is_some(),
                "{error:#}"
            ),
        }
    }
    Ok(())
}

#[test]
fn each_candidate_is_read_once_and_enumeration_order_is_irrelevant() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("z.json", &receipt("z"))?;
    fixture.write("a.json", &receipt("a"))?;
    fixture.write("companion.json", &json!({"kind":"golden_editor_workload", "rows":[]}))?;
    let stable = fixture.load()?;
    let workflows = load_workflows(&fixture.matrix)?;
    let validator = load_receipt_validator(&fixture.schema)?;
    let mut reads = BTreeMap::<PathBuf, usize>::new();
    let lists = Cell::new(0);
    let shuffled = receipt_set::load_with_source(
        &fixture.receipts,
        || {
            let mut paths = receipt_set::selected_paths(&fixture.receipts)?;
            paths.sort();
            if lists.get() == 0 {
                paths.reverse();
            }
            lists.set(lists.get() + 1);
            Ok(paths)
        },
        |path| {
            *reads.entry(path.to_path_buf()).or_default() += 1;
            let bytes = fs::read(path)?;
            // A later read would now see malformed input. Classification must
            // consume the owned bytes from this one read.
            fs::write(path, b"{")?;
            Ok(bytes)
        },
        |path, value| classify_candidate(path, value, &workflows, &validator),
    )?;
    assert_eq!(lists.get(), 2);
    assert_eq!(reads.len(), 3);
    assert!(reads.values().all(|count| *count == 1));
    assert_eq!(stable.digest(), shuffled.digest());
    assert_eq!(
        normalized_scorecard(&stable, &fixture.matrix)?,
        normalized_scorecard(&shuffled, &fixture.matrix)?
    );
    assert_eq!(shuffled.receipts().len(), 2);
    Ok(())
}

#[test]
fn equal_conflicting_and_different_component_cases_are_duplicates() -> Result<()> {
    let mut component = receipt("same");
    component["component"] = json!("hover");
    for duplicate in [receipt("same"), failed("same"), component] {
        let fixture = Fixture::new()?;
        fixture.write("first.json", &receipt("same"))?;
        fixture.write("different-name.json", &duplicate)?;
        assert!(fixture.error()?.contains("duplicate logical UX case"));
        let historical = super::super::lsp_stats_impl::aggregate_from_receipts(
            &fixture.receipts,
            &fixture.matrix,
            None,
        )?;
        assert_eq!(historical.workflows[0].pass_rate.basis[0], "2 receipts");
    }
    let fixture = Fixture::new()?;
    fixture.write("one.json", &receipt("module_a::case"))?;
    fixture.write("two.json", &receipt("module_b::case"))?;
    assert_eq!(fixture.load()?.receipts().len(), 2);
    Ok(())
}

#[test]
fn sha_platform_tier_and_hosted_subject_mixtures_fail_closed() -> Result<()> {
    for (field, value) in [
        ("sha", json!("different")),
        ("platform", json!("Windows")),
        ("run_id", json!("another-run")),
        ("attempt", json!(2)),
    ] {
        let fixture = Fixture::new()?;
        let mut first = receipt("one");
        first["run_identity"]["run_id"] = json!("run");
        first["run_identity"]["attempt"] = json!(1);
        let mut second = first.clone();
        second["test_name"] = json!("two");
        second["run_identity"][field] = value;
        fixture.write("one.json", &first)?;
        fixture.write("two.json", &second)?;
        assert!(fixture.error()?.contains("mixed compatibility receipt subject"), "{field}");
    }
    let fixture = Fixture::new()?;
    fixture.write("one.json", &receipt("one"))?;
    let mut second = receipt("two");
    second["ci_tier"] = json!("nightly");
    fixture.write("two.json", &second)?;
    assert!(fixture.error()?.contains("mixed compatibility receipt subject"));
    second["ci_tier"] = json!("pr");
    second["run_identity"]["run_id"] = json!("run");
    second["run_identity"]["attempt"] = json!(1);
    fixture.write("two.json", &second)?;
    assert!(fixture.error()?.contains("mixed compatibility receipt subject"));
    Ok(())
}

#[test]
fn required_identity_and_partial_hosted_identity_are_not_proven() -> Result<()> {
    for field in ["sha", "platform"] {
        for missing in [true, false] {
            let fixture = Fixture::new()?;
            let mut value = receipt("one");
            if missing {
                value["run_identity"]
                    .as_object_mut()
                    .ok_or_else(|| color_eyre::eyre::eyre!("identity"))?
                    .remove(field);
            } else {
                value["run_identity"][field] = json!("   ");
            }
            fixture.write("one.json", &value)?;
            assert!(fixture.error()?.contains(&format!("missing or empty run_identity.{field}")));
        }
    }
    for (field, value) in [("run_id", json!("run")), ("attempt", json!(1))] {
        let fixture = Fixture::new()?;
        let mut value_with_partial_identity = receipt("one");
        value_with_partial_identity["run_identity"][field] = value;
        fixture.write("one.json", &value_with_partial_identity)?;
        assert!(fixture.error()?.contains("partial hosted compatibility identity"));
    }
    Ok(())
}

#[test]
fn branch_is_informational_and_valid_hosted_sets_are_admitted() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut first = receipt("one");
    first["run_identity"]["run_id"] = json!("run");
    first["run_identity"]["attempt"] = json!(1);
    let mut second = first.clone();
    second["test_name"] = json!("two");
    second["run_identity"]["branch"] = json!("other-branch");
    fixture.write("one.json", &first)?;
    fixture.write("two.json", &second)?;
    let set = fixture.load()?;
    assert_eq!(set.receipts().len(), 2);
    assert_eq!(set.receipts()[1].run_identity.branch.as_deref(), Some("other-branch"));
    Ok(())
}

#[test]
fn companions_cannot_smuggle_ux_identity_or_enter_the_denominator() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("one.json", &receipt("one"))?;
    fixture.write("companion.json", &json!({"kind":"golden_editor_workload", "schema_version":3, "rows":[], "projects":[], "rollup":{}}))?;
    fixture.write("unrelated.json", &json!({"$schema":"schema-document"}))?;
    let set = fixture.load()?;
    assert_eq!(set.receipts().len(), 1);
    assert_eq!(
        normalized_scorecard(&set, &fixture.matrix)?["workflows"][0]["pass_rate"]["basis"][0],
        "1 receipts"
    );
    let mut contradiction = receipt("hidden");
    contradiction["kind"] = json!("golden_editor_workload");
    fixture.write("companion.json", &contradiction)?;
    assert!(fixture.error()?.contains("unsupported or malformed kind"));
    Ok(())
}

#[test]
fn coherent_unique_metrics_and_empty_population_keep_their_meaning() -> Result<()> {
    let fixture = Fixture::new()?;
    let empty = fixture.load()?;
    assert!(empty.describe().contains("v1 predeclared completeness is not proven"));
    assert_eq!(
        normalized_scorecard(&empty, &fixture.matrix)?["top_line"]["workflow_pass_rate"]["state"],
        "insufficient_data"
    );
    fixture.write("pass1.json", &receipt("pass1"))?;
    fixture.write("pass2.json", &receipt("pass2"))?;
    fixture.write("fail.json", &failed("fail"))?;
    fixture.write("quarantine.json", &failed("quarantine"))?;
    let mut skipped = failed("skipped");
    skipped["result"] = json!("skipped");
    skipped["skip_reason"] = json!("fixture skip");
    fixture.write("skip.json", &skipped)?;
    let ledger = fixture.matrix.with_file_name("flakes.json");
    fs::write(
        &ledger,
        serde_json::to_vec(
            &json!({"schema_version":1, "entries":[{"test":"quarantine", "state":"active", "first_seen":chrono::Utc::now().date_naive().to_string()}]}),
        )?,
    )?;
    let set = fixture.load()?;
    let scorecard = aggregate_admitted_receipts(set.receipts(), &fixture.matrix, Some(&ledger))?;
    assert_eq!(scorecard.top_line.workflow_pass_rate.value, Some(2.0 / 3.0));
    assert_eq!(scorecard.top_line.workflow_stability_rate.value, Some(0.5));
    assert_eq!(scorecard.top_line.p95_time_to_first_useful_result_ms.value, Some(5.0));
    assert_eq!(scorecard.components.cross_file_definition_success_rate.value, Some(2.0 / 3.0));
    assert_eq!(scorecard.workflows[0].quarantine_age_days, Some(0));
    Ok(())
}
