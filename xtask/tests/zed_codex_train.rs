use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const TRAIN_PATH: &str = ".ci/fixtures/zed-perl-upstream/codex-train.v1.json";

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
            entry
                .as_str()
                .ok_or_else(|| io::Error::other("dependency id is not a string").into())
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

#[test]
fn train_is_topologically_ordered_and_has_unique_stages() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    assert_eq!(
        train.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_train.v1")
    );
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
        ("P10", BTreeSet::from(["P01", "P06"])),
        ("P11", BTreeSet::from(["P02", "P03"])),
        ("P12", BTreeSet::from(["P04", "P11"])),
        ("P13", BTreeSet::from(["P05", "P11"])),
        ("P14", BTreeSet::from(["P07", "P10", "P11"])),
        ("P15", BTreeSet::from(["P10", "P11", "P12", "P13", "P14"])),
        ("P16", BTreeSet::from(["P15"])),
        ("P17", BTreeSet::from(["P13"])),
        ("P18", BTreeSet::from(["M01"])),
        ("P19", BTreeSet::from(["P08", "P10", "P13", "P14", "M02"])),
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
        index
            .get("P08")
            .and_then(|stage| stage.get("issue"))
            .and_then(Value::as_u64),
        Some(7912)
    );
    assert_eq!(
        index
            .get("P19")
            .and_then(|stage| stage.get("issue"))
            .and_then(Value::as_u64),
        Some(7912)
    );
    assert_ne!(
        index.get("P08").and_then(|stage| stage.get("phase")),
        index.get("P19").and_then(|stage| stage.get("phase"))
    );
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
    assert_eq!(
        index["P21"].get("closes_issue").and_then(Value::as_bool),
        Some(true)
    );
    Ok(())
}
