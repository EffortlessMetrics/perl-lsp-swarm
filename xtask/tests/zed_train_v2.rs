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
    value.as_object().ok_or_else(|| io::Error::other(format!("{context} is not an object")).into())
}

fn as_array<'a>(value: &'a Value, context: &str) -> TestResult<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("{context} is not an array")).into())
}

fn as_str<'a>(value: &'a Value, context: &str) -> TestResult<&'a str> {
    value.as_str().ok_or_else(|| io::Error::other(format!("{context} is not a string")).into())
}

fn manifest(root: &Path) -> TestResult<Value> {
    read_json(root, &format!("{TRAIN_DIR}/manifest.json"))
}

fn fragments(root: &Path, train: &Value) -> TestResult<Vec<Value>> {
    let entries =
        train.get("fragments").ok_or_else(|| io::Error::other("manifest lacks fragments"))?;
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
        let name = fragment.get("fragment").and_then(Value::as_str).unwrap_or("<unknown>");
        let entries = fragment
            .get("stages")
            .ok_or_else(|| io::Error::other(format!("{name} lacks stages")))?;
        for stage in as_array(entries, &format!("{name}.stages"))? {
            result.push(as_object(stage, &format!("{name} stage"))?);
        }
    }
    Ok(result)
}

fn stage_id(stage: &Map<String, Value>) -> TestResult<&str> {
    let value = stage.get("id").ok_or_else(|| io::Error::other("stage lacks id"))?;
    as_str(value, "stage.id")
}

fn dependencies(stage: &Map<String, Value>) -> TestResult<Vec<&str>> {
    let id = stage_id(stage)?;
    let values = stage
        .get("depends_on")
        .ok_or_else(|| io::Error::other(format!("{id} lacks depends_on")))?;
    as_array(values, &format!("{id}.depends_on"))?
        .iter()
        .map(|value| as_str(value, &format!("{id} dependency")))
        .collect()
}

fn stage_map<'a>(
    all: &'a [&Map<String, Value>],
) -> TestResult<BTreeMap<&'a str, &'a Map<String, Value>>> {
    let mut result = BTreeMap::new();
    for stage in all {
        let id = stage_id(stage)?;
        if result.insert(id, *stage).is_some() {
            return Err(io::Error::other(format!("duplicate stage `{id}`")).into());
        }
    }
    Ok(result)
}

fn reject_mutable_keys(value: &Value, path: &str) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &[
        "state",
        "status",
        "result",
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
        "checks",
        "labels",
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

fn assert_dependency(
    by_id: &BTreeMap<&str, &Map<String, Value>>,
    stage: &str,
    dependency: &str,
) -> TestResult<()> {
    let subject =
        by_id.get(stage).ok_or_else(|| io::Error::other(format!("missing stage `{stage}`")))?;
    assert!(
        dependencies(subject)?.contains(&dependency),
        "stage `{stage}` lost dependency `{dependency}`"
    );
    Ok(())
}

#[test]
fn stable_train_excludes_live_repository_and_external_state() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;

    assert_eq!(
        train.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_implementation_train.v2")
    );
    assert_eq!(
        train.get("public_state_until_final_projection").and_then(Value::as_str),
        Some("planned_not_proven")
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
        train.pointer("/rules/live_github_state_forbidden").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        train.pointer("/current_frontier_source/live_observation_issue").and_then(Value::as_u64),
        Some(10479)
    );
    assert_eq!(
        train
            .pointer("/current_frontier_source/hand_edited_frontier_forbidden")
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
    let mut manual = BTreeSet::new();

    for (position, stage) in all.iter().enumerate() {
        let id = stage_id(stage)?;
        assert!(index.insert(id, position).is_none(), "duplicate stage `{id}`");
        let stage_lane = as_str(
            stage.get("lane").ok_or_else(|| io::Error::other(format!("{id} lacks lane")))?,
            &format!("{id}.lane"),
        )?;
        lane.insert(id, stage_lane);
    }

    for (position, stage) in all.iter().enumerate() {
        let id = stage_id(stage)?;
        let actor = as_str(
            stage.get("actor").ok_or_else(|| io::Error::other(format!("{id} lacks actor")))?,
            &format!("{id}.actor"),
        )?;
        let external_write = as_str(
            stage
                .get("external_write")
                .ok_or_else(|| io::Error::other(format!("{id} lacks external_write")))?,
            &format!("{id}.external_write"),
        )?;
        let issue =
            stage.get("issue").ok_or_else(|| io::Error::other(format!("{id} lacks issue")))?;
        let closure = stage
            .get("closure_authority")
            .ok_or_else(|| io::Error::other(format!("{id} lacks closure_authority")))?;

        if actor == "maintainer" {
            assert!(issue.is_null(), "manual stage `{id}` must not invent an issue");
            assert!(closure.is_null(), "manual stage `{id}` must not invent closure authority");
            assert_eq!(external_write, "maintainer_only");
            manual.insert(id);
        } else {
            assert!(
                matches!(actor, "codex" | "read_only_acceptance"),
                "stage `{id}` has unsupported actor `{actor}`"
            );
            assert!(issue.as_u64().is_some(), "stage `{id}` needs an issue");
            assert!(closure.as_u64().is_some(), "stage `{id}` needs closure authority");
            assert_eq!(external_write, "forbidden");
        }

        for dependency in dependencies(stage)? {
            let dependency_position = index.get(dependency).ok_or_else(|| {
                io::Error::other(format!("stage `{id}` has unknown dependency `{dependency}`"))
            })?;
            assert!(
                *dependency_position < position,
                "stage `{id}` dependency `{dependency}` is not topologically earlier"
            );
            if lane.get(id) != Some(&"dap_sidecar") {
                assert_ne!(
                    lane.get(dependency),
                    Some(&"dap_sidecar"),
                    "core stage `{id}` depends on DAP stage `{dependency}`"
                );
            }
        }

        if lane.get(id) == Some(&"dap_sidecar") {
            assert_eq!(
                stage.get("blocks_core").and_then(Value::as_bool),
                Some(false),
                "DAP stage `{id}` must not block core LSP"
            );
        }
    }

    let declared_manual: BTreeSet<&str> = as_array(
        train
            .get("manual_stop_points")
            .ok_or_else(|| io::Error::other("manifest lacks manual_stop_points"))?,
        "manual_stop_points",
    )?
    .iter()
    .map(|value| as_str(value, "manual stop point"))
    .collect::<TestResult<_>>()?;
    assert_eq!(manual, declared_manual);
    assert_eq!(manual, BTreeSet::from(["M01", "M02", "QM01", "QM02"]));
    Ok(())
}

#[test]
fn newly_discovered_authorities_have_exact_stage_owners() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;
    let by_id = stage_map(&all)?;

    for (id, issue) in [
        ("C00", 10338),
        ("C01", 10479),
        ("A01", 10395),
        ("A02", 10392),
        ("A03", 10393),
        ("I00", 10340),
        ("A04", 10394),
        ("A05", 10396),
        ("E15", 10401),
        ("E16", 10343),
        ("D16", 10345),
        ("D17", 10347),
        ("U01", 10350),
        ("U02", 10351),
        ("E21", 10478),
        ("D23", 10400),
        ("QI01", 10485),
    ] {
        assert_eq!(
            by_id.get(id).and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
            Some(issue),
            "stage `{id}` owner drifted"
        );
    }

    assert!(
        all.iter().all(|stage| stage.get("issue").and_then(Value::as_u64) != Some(7759)),
        "programme controller 7759 must not masquerade as another PR stage"
    );
    assert_eq!(
        train.pointer("/closeout/implementation_issue").and_then(Value::as_u64),
        Some(10400)
    );
    assert_eq!(
        train.pointer("/closeout/controller_closed_on_success").and_then(Value::as_u64),
        Some(7759)
    );
    Ok(())
}

#[test]
fn subject_construction_and_immutable_evidence_precede_host_receipts() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;
    let by_id = stage_map(&all)?;

    for dependency in ["P02", "A02", "P03", "A03", "I00", "C02"] {
        assert_dependency(&by_id, "E11", dependency)?;
    }
    assert_dependency(&by_id, "A03", "A02")?;
    assert_dependency(&by_id, "A03", "P03")?;
    assert_dependency(&by_id, "P08", "A02")?;
    assert_dependency(&by_id, "P08", "A03")?;
    Ok(())
}

#[test]
fn release_projection_and_cache_lifecycle_precede_managed_evidence() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;
    let by_id = stage_map(&all)?;

    assert_dependency(&by_id, "P01", "A01")?;
    assert_dependency(&by_id, "P06", "A01")?;
    assert_dependency(&by_id, "E10", "A01")?;
    assert_dependency(&by_id, "P07", "A05")?;
    assert_dependency(&by_id, "E14", "A05")?;
    assert_dependency(&by_id, "E20", "A05")?;
    Ok(())
}

#[test]
fn compatibility_is_applied_before_fanin_and_public_projection() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;
    let by_id = stage_map(&all)?;

    assert_dependency(&by_id, "E15", "A04")?;
    for dependency in ["E10", "E11", "E12", "E13", "E14"] {
        assert_dependency(&by_id, "E15", dependency)?;
    }
    assert_dependency(&by_id, "E16", "E15")?;
    assert_dependency(&by_id, "E21", "A04")?;
    assert_dependency(&by_id, "E21", "E20")?;
    assert_dependency(&by_id, "D22", "E21")?;
    assert_dependency(&by_id, "D23", "D22")?;
    assert_dependency(&by_id, "D23", "C03")?;
    Ok(())
}

#[test]
fn dap_identity_is_independent_and_never_blocks_core_lsp() -> TestResult<()> {
    let root = repo_root()?;
    let train = manifest(&root)?;
    let source = fragments(&root, &train)?;
    let all = stages(&source)?;
    let by_id = stage_map(&all)?;

    assert_dependency(&by_id, "QE01", "QI01")?;
    assert_dependency(&by_id, "QE02", "QI01")?;
    assert_dependency(&by_id, "QE05", "QI01")?;

    for stage in &all {
        if stage.get("lane").and_then(Value::as_str) != Some("dap_sidecar") {
            for dependency in dependencies(stage)? {
                assert!(
                    !dependency.starts_with('Q'),
                    "core stage `{}` depends on DAP stage `{dependency}`",
                    stage_id(stage)?
                );
            }
        }
    }
    Ok(())
}

#[test]
fn observation_template_is_empty_and_non_evidentiary() -> TestResult<()> {
    let root = repo_root()?;
    let observation = read_json(&root, &format!("{TRAIN_DIR}/observation-template.json"))?;

    assert_eq!(
        observation.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_train_observation.v2")
    );
    assert_eq!(observation.get("result").and_then(Value::as_str), Some("not_run"));
    assert!(observation.get("observed_at").is_some_and(Value::is_null));
    assert!(observation.pointer("/main/commit").is_some_and(Value::is_null));
    assert!(observation.pointer("/main/tree").is_some_and(Value::is_null));
    assert!(observation.get("stages").and_then(Value::as_object).is_some_and(Map::is_empty));

    let frontier = observation
        .get("frontier")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("observation lacks frontier"))?;
    assert!(
        frontier.values().all(|value| value.as_array().is_some_and(Vec::is_empty)),
        "checked observation template must not invent a current frontier"
    );

    let limitations = as_array(
        observation
            .get("limitations")
            .ok_or_else(|| io::Error::other("observation lacks limitations"))?,
        "observation.limitations",
    )?;
    assert!(limitations.iter().any(|value| {
        value.as_str().is_some_and(|text| text.contains("cannot satisfy product"))
    }));
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
        "#10479",
        "#10392",
        "#10393",
        "#10394",
        "#10395",
        "#10396",
        "#10401",
        "#10478",
        "#10485",
        "#10400",
        "Submitted is not merged",
        "Merged is not released",
        "No core LSP stage depends on it",
        "receipt template                != executed receipt",
        "subject existence               != compatibility",
        "Remote branch deletion is not part of automated closeout",
    ] {
        assert!(doc.contains(needle), "human train lacks `{needle}`");
    }
    assert!(!doc.contains("Codex may submit upstream"), "human train authorizes an external write");
    Ok(())
}
