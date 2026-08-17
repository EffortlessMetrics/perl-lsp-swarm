use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const TRAIN_DIR: &str = ".ci/fixtures/zed-perl-upstream/train-v2";
const DOC: &str = "docs/integrations/ZED_CODEX_IMPLEMENTATION_TRAIN_V2.md";

type TestResult<T> = Result<T, Box<dyn Error>>;

fn repo_root() -> TestResult<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(root.join(relative))?)?)
}

fn as_object<'a>(value: &'a Value, context: &str) -> TestResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{context} is not an object")).into())
}

fn as_array<'a>(value: &'a Value, context: &str) -> TestResult<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("{context} is not an array")).into())
}

fn as_str<'a>(value: &'a Value, context: &str) -> TestResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| io::Error::other(format!("{context} is not a string")).into())
}

fn manifest(root: &Path) -> TestResult<Value> {
    read_json(root, &format!("{TRAIN_DIR}/manifest.json"))
}

fn fragments(root: &Path, train: &Value) -> TestResult<Vec<Value>> {
    let entries = train
        .get("fragments")
        .ok_or_else(|| io::Error::other("manifest lacks fragments"))?;
    let mut result = Vec::new();
    for entry in as_array(entries, "manifest.fragments")? {
        let filename = as_str(entry, "fragment filename")?;
        result.push(read_json(root, &format!("{TRAIN_DIR}/{filename}"))?);
    }
    Ok(result)
}

fn stages(source: &[Value]) -> TestResult<Vec<&Map<String, Value>>> {
    let mut result = Vec::new();
    for fragment in source {
        let name = fragment
            .get("fragment")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let entries = fragment
            .get("stages")
            .ok_or_else(|| io::Error::other(format!("{name} lacks stages")))?;
        for stage in as_array(entries, &format!("{name}.stages"))? {
            result.push(as_object(stage, &format!("{name} stage"))?);
        }
    }
    Ok(result)
}

fn reject_mutable_keys(value: &Value, path: &str) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &[
        "state",
        "status",
        "pr",
        "pr_number",
        "current_pr",
        "head_sha",
        "base_sha",
        "merge_sha",
        "merged_sha",
        "conclusion",
        "current_frontier",
        "observed_at",
        "updated_at",
        "created_at",
    ];

    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN.contains(&key.as_str()) {
                    return Err(format!("stable train contains mutable key `{path}/{key}`"));
                }
                reject_mutable_keys(child, &format!("{path}/{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                reject_mutable_keys(child, &format!("{path}/{index}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn id(stage: &Map<String, Value>) -> TestResult<&str> {
    let value = stage
        .get("id")
        .ok_or_else(|| io::Error::other("stage lacks id"))?;
    as_str(value, "stage.id")
}

fn dependencies(stage: &Map<String, Value>) -> TestResult<Vec<&str>> {
    let stage_id = id(stage)?;
    let values = stage
        .get("depends_on")
        .ok_or_else(|| io::Error::other(format!("{stage_id} lacks depends_on")))?;
    as_array(values, &format!("{stage_id}.depends_on"))?
        .iter()
        .map(|value| as_str(value, &format!("{stage_id} dependency")))
        .collect()
}

#[test]
fn stable_train_excludes_live_github_state() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;

    assert_eq!(
        train.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_implementation_train.v2")
    );
    reject_mutable_keys(&train, "manifest").map_err(io::Error::other)?;

    let source = fragments(&root, &train)?;
    assert_eq!(source.len(), 4);
    for fragment in &source {
        assert_eq!(
            fragment.get("schema_version").and_then(Value::as_str),
            Some("zed_codex_implementation_train_fragment.v2")
        );
        reject_mutable_keys(fragment, "fragment").map_err(io::Error::other)?;
    }

    assert_eq!(
        train
            .pointer("/rules/live_github_state_forbidden")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        train
            .pointer("/rules/one_canonical_unsuperseded_pr_per_codex_stage")
            .and_then(Value::as_bool),
        Some(true)
    );
    Ok(())
}

#[test]
fn stage_graph_is_unique_topological_and_actor_bounded() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;

    let mut index = BTreeMap::new();
    let mut lane = BTreeMap::new();
    for (position, stage) in all.iter().enumerate() {
        let stage_id = id(stage)?;
        assert!(
            index.insert(stage_id, position).is_none(),
            "duplicate train stage {stage_id}"
        );
        let lane_value = stage
            .get("lane")
            .ok_or_else(|| io::Error::other(format!("{stage_id} lacks lane")))?;
        lane.insert(stage_id, as_str(lane_value, &format!("{stage_id}.lane"))?);
    }

    for (position, stage) in all.iter().enumerate() {
        let stage_id = id(stage)?;
        let actor_value = stage
            .get("actor")
            .ok_or_else(|| io::Error::other(format!("{stage_id} lacks actor")))?;
        let actor = as_str(actor_value, &format!("{stage_id}.actor"))?;
        assert!(
            matches!(actor, "codex" | "read_only_acceptance" | "maintainer"),
            "{stage_id} has unsupported actor {actor}"
        );

        let issue = stage
            .get("issue")
            .ok_or_else(|| io::Error::other(format!("{stage_id} lacks issue")))?;
        if actor == "maintainer" {
            assert!(
                issue.is_null(),
                "{stage_id} manual checkpoint must not invent an issue"
            );
        } else {
            assert!(
                issue.as_u64().is_some(),
                "{stage_id} internal or read-only stage needs an issue"
            );
        }

        for dependency in dependencies(stage)? {
            let dependency_position = index.get(dependency).ok_or_else(|| {
                io::Error::other(format!("{stage_id} has unknown dependency {dependency}"))
            })?;
            assert!(
                *dependency_position < position,
                "{stage_id} dependency {dependency} is not topologically earlier"
            );
            if lane.get(stage_id) != Some(&"dap_sidecar") {
                assert_ne!(
                    lane.get(dependency),
                    Some(&"dap_sidecar"),
                    "core stage {stage_id} depends on DAP sidecar {dependency}"
                );
            }
        }

        if lane.get(stage_id) == Some(&"dap_sidecar") {
            assert_eq!(
                stage.get("blocks_core").and_then(Value::as_bool),
                Some(false),
                "{stage_id} DAP sidecar must not block core LSP"
            );
        }
    }

    let manual_values = train
        .get("manual_stop_points")
        .ok_or_else(|| io::Error::other("manifest lacks manual_stop_points"))?;
    let manual: BTreeSet<&str> = as_array(manual_values, "manual_stop_points")?
        .iter()
        .map(|value| as_str(value, "manual stop point"))
        .collect::<TestResult<_>>()?;
    assert_eq!(manual, BTreeSet::from(["DM01", "DM02", "M01", "M02"]));
    Ok(())
}

#[test]
fn key_control_and_acceptance_stages_have_exact_owners() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;

    let mut issues = BTreeMap::new();
    let mut deps = BTreeMap::new();
    for stage in all {
        let stage_id = id(stage)?;
        issues.insert(stage_id, stage.get("issue").and_then(Value::as_u64));
        deps.insert(stage_id, dependencies(stage)?);
    }

    for (stage_id, issue) in [
        ("C00", 10338),
        ("I00", 10340),
        ("P15", 10343),
        ("P16", 10345),
        ("P17", 10347),
        ("U01", 10350),
        ("U02", 10351),
        ("C02", 10352),
        ("DU01", 10353),
    ] {
        assert_eq!(
            issues.get(stage_id),
            Some(&Some(issue)),
            "{stage_id} owner drifted"
        );
    }

    for dependency in ["P10", "P11", "P12", "P13", "P14"] {
        assert!(
            deps.get("P15")
                .is_some_and(|values| values.contains(&dependency)),
            "P15 lost child evidence dependency {dependency}"
        );
    }
    assert!(
        deps.get("P17")
            .is_some_and(|values| values.contains(&"P16"))
    );
    assert_eq!(deps.get("U01"), Some(&vec!["M01"]));
    assert_eq!(deps.get("P19"), Some(&vec!["U01"]));
    assert_eq!(deps.get("M02"), Some(&vec!["P19"]));
    assert!(deps.get("U02").is_some_and(|values| {
        values.contains(&"M02") && values.contains(&"U01")
    }));
    assert!(
        deps.get("P20")
            .is_some_and(|values| values.contains(&"U02"))
    );
    assert!(
        deps.get("P21")
            .is_some_and(|values| values.contains(&"P20"))
    );
    assert!(
        deps.get("P22")
            .is_some_and(|values| values.contains(&"P21"))
    );
    Ok(())
}

#[test]
fn observation_template_is_empty_and_non_evidentiary() -> TestResult<()> {
    let root = repo_root()?;
    let observation = read_json(
        &root,
        &format!("{TRAIN_DIR}/observation-template.json"),
    )?;

    assert_eq!(
        observation.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_train_observation.v1")
    );
    assert_eq!(
        observation.get("result").and_then(Value::as_str),
        Some("not_run")
    );
    assert!(
        observation
            .get("observed_at")
            .is_some_and(Value::is_null)
    );
    assert!(observation.get("main_sha").is_some_and(Value::is_null));
    assert!(
        observation
            .get("stages")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
    );

    let limitations = observation
        .get("limitations")
        .ok_or_else(|| io::Error::other("observation lacks limitations"))?;
    assert!(
        as_array(limitations, "observation limitations")?
            .iter()
            .any(|value| {
                value
                    .as_str()
                    .is_some_and(|text| text.contains("cannot satisfy product"))
            }),
        "observation template must deny product evidence"
    );
    Ok(())
}

#[test]
fn human_train_preserves_delivery_and_evidence_boundaries() -> TestResult<()> {
    let root = repo_root()?;
    let doc = fs::read_to_string(root.join(DOC))?;

    for needle in [
        "planned / not proven",
        "perlnavigator-server -> Perl Navigator",
        "perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp",
        "perllsp              -> EffortlessMetrics/perl-lsp",
        "deliver-pr",
        "M01",
        "U01",
        "M02",
        "U02",
        "canonical `perl_lsp.binary_identity.v1`",
        "Submitted is not merged",
        "merged is not released",
        "DAP sidecar may run",
    ] {
        assert!(doc.contains(needle), "human train lacks `{needle}`");
    }
    assert!(
        !doc.contains("Codex may submit upstream"),
        "human train authorizes an external write"
    );
    Ok(())
}
