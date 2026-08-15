use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const TRAIN_PATH: &str = ".ci/fixtures/zed-perl-upstream/codex-train.v1.json";
const REGISTRY_MANIFEST_PATH: &str = ".ci/fixtures/zed-perl-upstream/registry/manifest.toml";
const TRAIN_DOC_PATH: &str = "docs/integrations/ZED_CODEX_IMPLEMENTATION_TRAIN.md";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn load_train() -> Result<Value, Box<dyn Error>> {
    let root = repo_root()?;
    Ok(serde_json::from_slice(&fs::read(root.join(TRAIN_PATH))?)?)
}

fn load_registry_manifest() -> Result<toml::Value, Box<dyn Error>> {
    let root = repo_root()?;
    Ok(toml::from_str(&fs::read_to_string(root.join(REGISTRY_MANIFEST_PATH))?)?)
}

fn stages(train: &Value) -> Result<&[Value], Box<dyn Error>> {
    train
        .get("stages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other("Zed Codex train lacks stages").into())
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("stage lacks string `{key}`")).into())
}

fn dependency_ids(stage: &Value) -> Result<Vec<&str>, Box<dyn Error>> {
    stage
        .get("depends_on")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("stage lacks depends_on array"))?
        .iter()
        .map(|entry| {
            entry.as_str().ok_or_else(|| io::Error::other("dependency id is not a string").into())
        })
        .collect()
}

fn index_by_id<'a>(train: &'a Value) -> Result<BTreeMap<&'a str, &'a Value>, Box<dyn Error>> {
    let mut index = BTreeMap::new();
    for stage in stages(train)? {
        let id = string(stage, "id")?;
        if index.insert(id, stage).is_some() {
            return Err(io::Error::other(format!("duplicate train stage `{id}`")).into());
        }
    }
    Ok(index)
}

fn set_stage_state(train: &mut Value, id: &str, state: &str) -> Result<(), Box<dyn Error>> {
    let stage = train
        .get_mut("stages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("Zed Codex train lacks mutable stages"))?
        .iter_mut()
        .find(|stage| stage.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("missing train stage `{id}`")))?;
    stage["state"] = Value::String(state.to_string());
    Ok(())
}

fn merged_upstream_subject_is_accepted(
    train: &Value,
    registry: &toml::Value,
) -> Result<bool, Box<dyn Error>> {
    let index = index_by_id(train)?;
    index.get("P18").ok_or_else(|| io::Error::other("missing P18"))?;
    let m01_complete =
        index.get("M01").and_then(|stage| stage.get("state")).and_then(Value::as_str)
            == Some("complete");
    let upstream_acceptance_complete =
        index.get("U01").and_then(|stage| stage.get("state")).and_then(Value::as_str)
            == Some("complete");
    let acceptance = index
        .get("U01")
        .copied()
        .ok_or_else(|| io::Error::other("missing U01"))?
        .get("upstream_acceptance")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("U01 lacks upstream_acceptance"))?;
    let extension = registry.get("extension").and_then(toml::Value::as_table);
    let validation = registry.get("validation").and_then(toml::Value::as_table);
    let string_field = |table: Option<&toml::map::Map<String, toml::Value>>, key: &str| {
        table
            .and_then(|table| table.get(key))
            .and_then(toml::Value::as_str)
            .is_some_and(|value| !value.is_empty())
    };

    let new_commit = extension.and_then(|table| table.get("new_commit"));
    let current_commit = extension.and_then(|table| table.get("current_commit"));
    let new_version = extension.and_then(|table| table.get("new_version"));
    let current_version = extension.and_then(|table| table.get("current_version"));
    Ok(m01_complete
        && upstream_acceptance_complete
        && acceptance.get("repository").and_then(Value::as_str)
            == Some("tree-sitter-perl/zed-perl")
        && string_field(extension, "new_commit")
        && string_field(extension, "new_version")
        && string_field(extension, "current_commit")
        && string_field(extension, "current_version")
        && string_field(extension, "upstream_branch_containing_commit")
        && new_commit != current_commit
        && new_version != current_version
        && validation
            .and_then(|table| table.get("submodule_commit_branch_reachable"))
            .and_then(toml::Value::as_bool)
            == Some(true)
        && validation
            .and_then(|table| table.get("manifest_version_matches"))
            .and_then(toml::Value::as_bool)
            == Some(true)
        && validation
            .and_then(|table| table.get("released_build_contains_commit"))
            .and_then(toml::Value::as_bool)
            == Some(true))
}

#[test]
fn train_is_topologically_ordered_and_has_unique_stages() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    assert_eq!(train.get("schema_version").and_then(Value::as_str), Some("zed_codex_train.v1"));
    assert_eq!(train.get("programme_issue").and_then(Value::as_u64), Some(7759));

    let mut seen = BTreeSet::new();
    for stage in stages(&train)? {
        let id = string(stage, "id")?;
        assert!(seen.insert(id), "duplicate train stage `{id}`");
        for dependency in dependency_ids(stage)? {
            assert!(
                seen.contains(dependency),
                "stage `{id}` appears before dependency `{dependency}`"
            );
        }
    }
    Ok(())
}

#[test]
fn external_writes_are_maintainer_only_stop_points() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let mut external = BTreeSet::new();
    for stage in stages(&train)? {
        let id = string(stage, "id")?;
        let actor = string(stage, "actor")?;
        let kind = string(stage, "kind")?;
        let writes_external = stage
            .get("external_write")
            .and_then(Value::as_bool)
            .ok_or_else(|| io::Error::other(format!("stage `{id}` lacks external_write")))?;
        if writes_external {
            assert_eq!(actor, "maintainer");
            assert_eq!(kind, "manual_checkpoint");
            assert!(stage.get("issue").is_some_and(Value::is_null));
            external.insert(id);
        } else {
            assert_ne!(actor, "maintainer");
        }
    }
    assert_eq!(external, BTreeSet::from(["M01", "M02"]));
    Ok(())
}

#[test]
fn authority_evidence_packet_and_public_gates_remain_separate() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let index = index_by_id(&train)?;

    let required_dependencies = BTreeMap::from([
        ("P10", BTreeSet::from(["C01", "P01", "P06"])),
        ("P11", BTreeSet::from(["C01", "P02", "P03"])),
        ("P12", BTreeSet::from(["P04", "P11"])),
        ("P13", BTreeSet::from(["P05", "P11"])),
        ("P14", BTreeSet::from(["P07", "P10", "P11"])),
        ("P15", BTreeSet::from(["P10", "P11", "P12", "P13", "P14"])),
        ("P16", BTreeSet::from(["P15"])),
        ("P17", BTreeSet::from(["P13"])),
        ("U01", BTreeSet::from(["M01"])),
        ("P18", BTreeSet::from(["M01", "U01"])),
        ("P19", BTreeSet::from(["C01", "M02", "P08", "P10", "P13", "P14"])),
        ("P20", BTreeSet::from(["P09", "P19"])),
        ("P21", BTreeSet::from(["P20"])),
    ]);

    for (id, expected) in required_dependencies {
        let stage = index
            .get(id)
            .copied()
            .ok_or_else(|| io::Error::other(format!("missing required train stage `{id}`")))?;
        let actual = dependency_ids(stage)?.into_iter().collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "train dependencies drifted for `{id}`");
    }

    assert_eq!(
        index.get("P08").and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
        Some(9467)
    );
    assert_eq!(
        index.get("P09").and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
        Some(9468)
    );
    assert_eq!(
        index.get("C01").and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
        Some(9483)
    );
    assert_eq!(
        index.get("P19").and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
        Some(7912)
    );
    assert_eq!(
        index.get("P20").and_then(|stage| stage.get("issue")).and_then(Value::as_u64),
        Some(8000)
    );
    assert_eq!(string(index["P00"], "state")?, "static_substrate_complete_execution_not_proven");
    assert_eq!(string(index["P01"], "state")?, "ready_after_dependency");
    assert_eq!(string(index["P02"], "state")?, "ready_after_dependency");
    let acceptance = index["U01"]
        .get("upstream_acceptance")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("U01 lacks upstream_acceptance"))?;
    assert_eq!(acceptance.get("source_of_truth").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(
        acceptance.get("repository").and_then(Value::as_str),
        Some("tree-sitter-perl/zed-perl")
    );
    assert_eq!(
        acceptance
            .get("required_fields")
            .and_then(Value::as_array)
            .map(|fields| fields.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>()),
        Some(BTreeSet::from([
            "extension.new_commit",
            "extension.new_version",
            "extension.upstream_branch_containing_commit",
        ]))
    );
    assert_eq!(
        acceptance
            .get("required_validation")
            .and_then(Value::as_array)
            .map(|fields| fields.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>()),
        Some(BTreeSet::from([
            "validation.manifest_version_matches",
            "validation.released_build_contains_commit",
            "validation.submodule_commit_branch_reachable",
        ]))
    );
    assert_eq!(acceptance.get("requires_changed_subject").and_then(Value::as_bool), Some(true));
    assert_ne!(
        index.get("P08").and_then(|stage| stage.get("phase")),
        index.get("P19").and_then(|stage| stage.get("phase"))
    );
    Ok(())
}

#[test]
fn p18_rejects_m01_without_merged_upstream_acceptance() -> Result<(), Box<dyn Error>> {
    let mut train = load_train()?;
    set_stage_state(&mut train, "M01", "complete")?;
    let blocked_registry = load_registry_manifest()?;

    assert_eq!(string(index_by_id(&train)?["U01"], "state")?, "blocked_on_external_subject");
    assert!(!merged_upstream_subject_is_accepted(&train, &blocked_registry)?);

    set_stage_state(&mut train, "U01", "complete")?;
    assert!(!merged_upstream_subject_is_accepted(&train, &blocked_registry)?);

    let mut merged_registry = blocked_registry.clone();
    let extension = merged_registry
        .get_mut("extension")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| io::Error::other("registry manifest lacks extension table"))?;
    extension.insert(
        "new_commit".to_string(),
        toml::Value::String("0123456789abcdef0123456789abcdef01234567".to_string()),
    );
    extension.insert("new_version".to_string(), toml::Value::String("0.5.0".to_string()));
    extension.insert(
        "upstream_branch_containing_commit".to_string(),
        toml::Value::String("master".to_string()),
    );
    let validation = merged_registry
        .get_mut("validation")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| io::Error::other("registry manifest lacks validation table"))?;
    validation.insert("submodule_commit_branch_reachable".to_string(), toml::Value::Boolean(true));
    validation.insert("manifest_version_matches".to_string(), toml::Value::Boolean(true));
    validation.insert("released_build_contains_commit".to_string(), toml::Value::Boolean(true));

    for missing_field in ["current_commit", "current_version"] {
        let mut missing_captured_subject = merged_registry.clone();
        missing_captured_subject
            .get_mut("extension")
            .and_then(toml::Value::as_table_mut)
            .ok_or_else(|| io::Error::other("registry manifest lacks extension table"))?
            .remove(missing_field);
        assert!(
            !merged_upstream_subject_is_accepted(&train, &missing_captured_subject)?,
            "missing captured subject field `{missing_field}` was accepted"
        );
    }

    let mut missing_released_build_evidence = merged_registry.clone();
    missing_released_build_evidence
        .get_mut("validation")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| io::Error::other("registry manifest lacks validation table"))?
        .remove("released_build_contains_commit");
    assert!(!merged_upstream_subject_is_accepted(&train, &missing_released_build_evidence)?);

    let mut false_released_build_evidence = merged_registry.clone();
    false_released_build_evidence
        .get_mut("validation")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| io::Error::other("registry manifest lacks validation table"))?
        .insert("released_build_contains_commit".to_string(), toml::Value::Boolean(false));
    assert!(!merged_upstream_subject_is_accepted(&train, &false_released_build_evidence)?);

    assert!(merged_upstream_subject_is_accepted(&train, &merged_registry)?);
    Ok(())
}

#[test]
fn prose_dependency_edges_match_the_machine_checked_fan_in() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let prose = fs::read_to_string(root.join(TRAIN_DOC_PATH))?;
    for (dependency, stage) in [
        ("P01", "P07"),
        ("P07", "P14"),
        ("P11", "P12"),
        ("P11", "P13"),
        ("P11", "P14"),
        ("M01", "U01"),
        ("U01", "P18"),
    ] {
        let edge = format!("`{dependency} -> {stage}`");
        assert!(prose.contains(&edge), "prose graph lacks dependency edge {edge}");
    }
    Ok(())
}

#[test]
fn support_and_closeout_cannot_precede_public_receipt() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let rules = train
        .get("rules")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("train lacks rules"))?;
    assert_eq!(rules.get("templates_are_evidence").and_then(Value::as_bool), Some(false));
    assert_eq!(rules.get("external_writes_are_manual").and_then(Value::as_bool), Some(true));
    assert_eq!(rules.get("dap_in_scope").and_then(Value::as_bool), Some(false));

    let support = rules
        .get("public_support_requires")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("train lacks public_support_requires"))?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(support, BTreeSet::from(["P19", "P20"]));

    let index = index_by_id(&train)?;
    assert_eq!(string(index["P19"], "kind")?, "public_evidence");
    assert_eq!(string(index["P20"], "kind")?, "projection");
    assert_eq!(string(index["P21"], "kind")?, "closeout");
    assert_eq!(index["P21"].get("closes_issue").and_then(Value::as_bool), Some(true));
    Ok(())
}

#[test]
fn dap_sidecar_is_explicitly_non_blocking_and_has_manual_publication_stops()
-> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let sidecars = train
        .get("non_blocking_sidecars")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("train lacks non_blocking_sidecars"))?;
    assert_eq!(sidecars.len(), 1);

    let dap = &sidecars[0];
    assert_eq!(dap.get("id").and_then(Value::as_str), Some("zed_dap"));
    assert_eq!(dap.get("controller_issue").and_then(Value::as_u64), Some(9484));
    assert_eq!(dap.get("blocks_programme_closeout").and_then(Value::as_bool), Some(false));

    let dap_stages = dap
        .get("stages")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("zed_dap sidecar lacks stages"))?;
    let expected = BTreeMap::from([
        ("D01", Some(9485)),
        ("D02", Some(9486)),
        ("D03", Some(9490)),
        ("DM01", None),
        ("DU01", None),
        ("D04", Some(9491)),
        ("DM02", None),
        ("D05", Some(9487)),
        ("D06", Some(9489)),
        ("D07", Some(9484)),
    ]);
    let mut seen = BTreeSet::new();
    let mut external = BTreeSet::new();

    for stage in dap_stages {
        let id = string(stage, "id")?;
        assert!(seen.insert(id), "duplicate DAP sidecar stage `{id}`");
        assert_eq!(
            stage.get("issue").and_then(Value::as_u64),
            expected.get(id).copied().flatten(),
            "DAP sidecar issue drifted for `{id}`"
        );

        for dependency in
            stage.get("depends_on_sidecar").and_then(Value::as_array).ok_or_else(|| {
                io::Error::other(format!("DAP stage `{id}` lacks sidecar dependencies"))
            })?
        {
            let dependency = dependency
                .as_str()
                .ok_or_else(|| io::Error::other("DAP dependency is not a string"))?;
            assert!(
                seen.contains(dependency),
                "DAP stage `{id}` appears before dependency `{dependency}`"
            );
        }

        let writes_external = stage
            .get("external_write")
            .and_then(Value::as_bool)
            .ok_or_else(|| io::Error::other(format!("DAP stage `{id}` lacks external_write")))?;
        if writes_external {
            assert_eq!(string(stage, "actor")?, "maintainer");
            assert_eq!(string(stage, "kind")?, "manual_checkpoint");
            external.insert(id);
        }
    }

    let dap_index = dap_stages
        .iter()
        .map(|stage| Ok((string(stage, "id")?, stage)))
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    assert_eq!(string(dap_index["DU01"], "kind")?, "acceptance");
    assert_eq!(string(dap_index["DU01"], "state")?, "blocked_on_external_subject");
    assert_eq!(
        dap_index["DU01"].get("depends_on_sidecar").and_then(Value::as_array).map(|dependencies| {
            dependencies.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>()
        }),
        Some(BTreeSet::from(["DM01"]))
    );
    assert_eq!(
        dap_index["DU01"]
            .get("upstream_acceptance")
            .and_then(Value::as_object)
            .and_then(|acceptance| acceptance.get("requires_released_build"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dap_index["D04"].get("depends_on_sidecar").and_then(Value::as_array).map(|dependencies| {
            dependencies.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>()
        }),
        Some(BTreeSet::from(["DU01"]))
    );
    assert_eq!(external, BTreeSet::from(["DM01", "DM02"]));
    assert_eq!(expected.len(), dap_stages.len());
    Ok(())
}
