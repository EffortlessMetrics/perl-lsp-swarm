use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const TRAIN_PATH: &str = ".ci/fixtures/zed-perl-upstream/codex-train.v1.json";
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

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("value lacks array `{key}`")).into())
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("value lacks string `{key}`")).into())
}

fn boolean(value: &Value, key: &str) -> Result<bool, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| io::Error::other(format!("value lacks bool `{key}`")).into())
}

fn string_set<'a>(value: &'a Value, key: &str) -> Result<BTreeSet<&'a str>, Box<dyn Error>> {
    array(value, key)?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .ok_or_else(|| io::Error::other(format!("`{key}` entry is not a string")).into())
        })
        .collect()
}

fn core_stages(train: &Value) -> Result<&[Value], Box<dyn Error>> {
    array(train, "stages")
}

fn core_index<'a>(train: &'a Value) -> Result<BTreeMap<&'a str, &'a Value>, Box<dyn Error>> {
    let mut index = BTreeMap::new();
    for stage in core_stages(train)? {
        let id = string(stage, "id")?;
        if index.insert(id, stage).is_some() {
            return Err(io::Error::other(format!("duplicate core stage `{id}`")).into());
        }
    }
    Ok(index)
}

fn dap_sidecar(train: &Value) -> Result<&Value, Box<dyn Error>> {
    array(train, "non_blocking_sidecars")?
        .iter()
        .find(|sidecar| sidecar.get("id").and_then(Value::as_str) == Some("zed_dap"))
        .ok_or_else(|| io::Error::other("train lacks the Zed DAP sidecar").into())
}

fn dap_stages(train: &Value) -> Result<&[Value], Box<dyn Error>> {
    array(dap_sidecar(train)?, "stages")
}

fn dap_index<'a>(train: &'a Value) -> Result<BTreeMap<&'a str, &'a Value>, Box<dyn Error>> {
    let mut index = BTreeMap::new();
    for stage in dap_stages(train)? {
        let id = string(stage, "id")?;
        if index.insert(id, stage).is_some() {
            return Err(io::Error::other(format!("duplicate DAP stage `{id}`")).into());
        }
    }
    Ok(index)
}

#[test]
fn core_train_is_topologically_ordered_and_unique() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    assert_eq!(train.get("schema_version").and_then(Value::as_str), Some("zed_codex_train.v1"));
    assert_eq!(train.get("programme_issue").and_then(Value::as_u64), Some(7759));

    let mut seen = BTreeSet::new();
    for stage in core_stages(&train)? {
        let id = string(stage, "id")?;
        assert!(seen.insert(id), "duplicate core stage `{id}`");
        for dependency in string_set(stage, "depends_on")? {
            assert!(
                seen.contains(dependency),
                "core stage `{id}` appears before dependency `{dependency}`"
            );
        }
    }
    Ok(())
}

#[test]
fn live_frontier_matches_merged_and_open_pr_state() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let rules = train.get("rules").ok_or_else(|| io::Error::other("train lacks rules"))?;
    assert_eq!(
        string_set(rules, "current_core_frontier")?,
        BTreeSet::from(["C01", "P02", "P06", "P09"])
    );
    assert_eq!(string_set(rules, "current_dap_frontier")?, BTreeSet::from(["D01"]));

    let core = core_index(&train)?;
    assert_eq!(string(core["P00"], "state")?, "complete_static_substrate_execution_not_proven");
    assert_eq!(core["P00"].get("pull_request").and_then(Value::as_u64), Some(8023));
    assert_eq!(string(core["P01"], "state")?, "authority_merged_execution_not_proven");
    assert_eq!(core["P01"].get("pull_request").and_then(Value::as_u64), Some(8365));
    assert_eq!(string(core["P02"], "state")?, "implementation_pr_open");
    assert_eq!(core["P02"].get("pull_request").and_then(Value::as_u64), Some(8369));
    for ready in ["P06", "P09", "C01"] {
        assert_eq!(string(core[ready], "state")?, "ready");
    }

    let dap = dap_index(&train)?;
    assert_eq!(string(dap["D01"], "state")?, "ready");
    Ok(())
}

#[test]
fn external_writes_are_maintainer_only_stop_points() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let mut core_external = BTreeSet::new();
    for stage in core_stages(&train)? {
        let id = string(stage, "id")?;
        if boolean(stage, "external_write")? {
            assert_eq!(string(stage, "actor")?, "maintainer");
            assert_eq!(string(stage, "kind")?, "manual_checkpoint");
            assert!(stage.get("issue").is_some_and(Value::is_null));
            core_external.insert(id);
        } else {
            assert_ne!(string(stage, "actor")?, "maintainer");
        }
    }
    assert_eq!(core_external, BTreeSet::from(["M01", "M02"]));

    let mut dap_external = BTreeSet::new();
    for stage in dap_stages(&train)? {
        let id = string(stage, "id")?;
        if boolean(stage, "external_write")? {
            assert_eq!(string(stage, "actor")?, "maintainer");
            assert_eq!(string(stage, "kind")?, "manual_checkpoint");
            assert!(stage.get("issue").is_some_and(Value::is_null));
            dap_external.insert(id);
        }
    }
    assert_eq!(dap_external, BTreeSet::from(["DM01", "DM02"]));
    Ok(())
}

#[test]
fn core_authority_evidence_and_public_gates_remain_separate() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let core = core_index(&train)?;
    let expected = BTreeMap::from([
        ("P07", BTreeSet::from(["P01", "P02"])),
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

    for (id, dependencies) in expected {
        assert_eq!(
            string_set(core[id], "depends_on")?,
            dependencies,
            "core dependencies drifted for `{id}`"
        );
    }

    assert_eq!(string(core["P01"], "kind")?, "authority");
    assert_eq!(string(core["P10"], "kind")?, "evidence");
    assert_eq!(string(core["P19"], "kind")?, "public_evidence");
    assert_eq!(string(core["P20"], "kind")?, "projection");
    assert_eq!(string(core["P21"], "kind")?, "closeout");
    Ok(())
}

#[test]
fn support_and_closeout_require_public_receipt_and_projection() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let rules = train.get("rules").ok_or_else(|| io::Error::other("train lacks rules"))?;
    assert!(!boolean(rules, "templates_are_evidence")?);
    assert!(boolean(rules, "external_writes_are_manual")?);
    assert!(!boolean(rules, "dap_in_scope")?);
    assert_eq!(string_set(rules, "public_support_requires")?, BTreeSet::from(["P19", "P20"]));

    let core = core_index(&train)?;
    assert_eq!(string_set(core["P20"], "depends_on")?, BTreeSet::from(["P09", "P19"]));
    assert_eq!(string_set(core["P21"], "depends_on")?, BTreeSet::from(["P20"]));
    assert!(boolean(core["P21"], "closes_issue")?);
    Ok(())
}

#[test]
fn upstream_acceptance_contracts_fail_closed() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let core = core_index(&train)?;
    let core_acceptance = core["U01"]
        .get("upstream_acceptance")
        .ok_or_else(|| io::Error::other("U01 lacks upstream_acceptance"))?;
    assert_eq!(string(core_acceptance, "repository")?, "tree-sitter-perl/zed-perl");
    assert!(boolean(core_acceptance, "requires_changed_subject")?);
    assert_eq!(
        string_set(core_acceptance, "required_fields")?,
        BTreeSet::from([
            "extension.new_commit",
            "extension.new_version",
            "extension.upstream_branch_containing_commit",
        ])
    );
    assert_eq!(
        string_set(core_acceptance, "required_validation")?,
        BTreeSet::from([
            "validation.manifest_version_matches",
            "validation.submodule_commit_branch_reachable",
        ])
    );

    let dap = dap_index(&train)?;
    let dap_acceptance = dap["DU01"]
        .get("upstream_acceptance")
        .ok_or_else(|| io::Error::other("DU01 lacks upstream_acceptance"))?;
    assert_eq!(string(dap_acceptance, "repository")?, "tree-sitter-perl/zed-perl");
    assert!(boolean(dap_acceptance, "requires_changed_subject")?);
    assert!(boolean(dap_acceptance, "requires_released_build")?);
    assert_eq!(
        string_set(dap_acceptance, "required_validation")?,
        BTreeSet::from([
            "validation.manifest_version_matches",
            "validation.released_build_contains_commit",
            "validation.submodule_commit_branch_reachable",
        ])
    );
    Ok(())
}

#[test]
fn dap_sidecar_is_non_blocking_and_has_independent_asset_evidence() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let sidecar = dap_sidecar(&train)?;
    assert_eq!(sidecar.get("controller_issue").and_then(Value::as_u64), Some(9484));
    assert!(!boolean(sidecar, "blocks_programme_closeout")?);

    let expected_issues = BTreeMap::from([
        ("D01", Some(9485)),
        ("DA01", Some(9516)),
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
    for stage in dap_stages(&train)? {
        let id = string(stage, "id")?;
        assert!(seen.insert(id), "duplicate DAP stage `{id}`");
        assert_eq!(
            stage.get("issue").and_then(Value::as_u64),
            expected_issues[id],
            "DAP issue drifted for `{id}`"
        );
        for dependency in string_set(stage, "depends_on_sidecar")? {
            assert!(
                seen.contains(dependency),
                "DAP stage `{id}` appears before dependency `{dependency}`"
            );
        }
    }
    assert_eq!(seen.len(), expected_issues.len());

    let dap = dap_index(&train)?;
    assert_eq!(string(dap["DA01"], "kind")?, "evidence");
    assert_eq!(string(dap["DA01"], "phase")?, "public_perl_dap_asset_receipts");
    assert_eq!(string_set(dap["DA01"], "depends_on_sidecar")?, BTreeSet::from(["D01"]));
    assert_eq!(string_set(dap["D02"], "depends_on_core")?, BTreeSet::from(["P02", "P03"]));
    assert_eq!(
        string_set(dap["D05"], "depends_on_sidecar")?,
        BTreeSet::from(["D02", "DA01", "DM02"])
    );
    assert_eq!(string_set(dap["D05"], "depends_on_core")?, BTreeSet::from(["C01"]));
    assert_eq!(string_set(dap["D06"], "depends_on_sidecar")?, BTreeSet::from(["D05"]));
    assert_eq!(string_set(dap["D06"], "depends_on_core")?, BTreeSet::from(["P09"]));
    Ok(())
}

#[test]
fn prose_and_machine_train_preserve_critical_edges() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let prose = fs::read_to_string(root.join(TRAIN_DOC_PATH))?;
    for edge in [
        "`P01 -> P07`",
        "`P07 -> P14`",
        "`P11 -> P12`",
        "`P11 -> P13`",
        "`P11 -> P14`",
        "`M01 -> U01`",
        "`U01 -> P18`",
        "`D01 -> DA01`",
        "`DA01 -> D05`",
        "`D02 -> D05`",
        "`DM02 -> D05`",
        "`C01 -> D05`",
        "`D05 -> D06`",
        "`P09 -> D06`",
    ] {
        assert!(prose.contains(edge), "prose train lacks edge {edge}");
    }
    for required in ["#8369", "#8661", "#9468", "#9483", "#9485", "#9516"] {
        assert!(
            prose.contains(required),
            "prose train lacks current frontier reference {required}"
        );
    }
    Ok(())
}
