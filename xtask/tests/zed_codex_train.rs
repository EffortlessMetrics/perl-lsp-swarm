use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const TRAIN_PATH: &str = ".ci/fixtures/zed-perl-upstream/codex-train.v1.json";
const TRAIN_DOC_PATH: &str = "docs/integrations/ZED_CODEX_IMPLEMENTATION_TRAIN.md";
const REGISTRY_MANIFEST_PATH: &str = ".ci/fixtures/zed-perl-upstream/registry/manifest.toml";

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

fn load_registry_manifest() -> Result<toml::Value, Box<dyn Error>> {
    let root = repo_root()?;
    Ok(toml::from_str(&fs::read_to_string(root.join(REGISTRY_MANIFEST_PATH))?)?)
}

/// Resolve a dotted acceptance path such as `extension.new_commit`.
fn toml_path<'a>(document: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    let mut cursor = document;
    for segment in path.split('.') {
        cursor = cursor.get(segment)?;
    }
    Some(cursor)
}

fn set_toml_path(
    document: &mut toml::Value,
    path: &str,
    value: toml::Value,
) -> Result<(), Box<dyn Error>> {
    let (parents, leaf) = path.rsplit_once('.').unwrap_or(("", path));
    let mut cursor = document;
    if !parents.is_empty() {
        for segment in parents.split('.') {
            cursor = cursor
                .get_mut(segment)
                .ok_or_else(|| io::Error::other(format!("`{path}` has no table `{segment}`")))?;
        }
    }
    cursor
        .as_table_mut()
        .ok_or_else(|| io::Error::other(format!("`{path}` does not address a table")))?
        .insert(leaf.to_string(), value);
    Ok(())
}

fn remove_toml_path(document: &mut toml::Value, path: &str) -> Result<(), Box<dyn Error>> {
    let (parents, leaf) = path.rsplit_once('.').unwrap_or(("", path));
    let mut cursor = document;
    if !parents.is_empty() {
        for segment in parents.split('.') {
            cursor = cursor
                .get_mut(segment)
                .ok_or_else(|| io::Error::other(format!("`{path}` has no table `{segment}`")))?;
        }
    }
    cursor
        .as_table_mut()
        .ok_or_else(|| io::Error::other(format!("`{path}` does not address a table")))?
        .remove(leaf);
    Ok(())
}

fn acceptance_contract<'a>(stage: &'a Value, id: &str) -> Result<&'a Value, Box<dyn Error>> {
    stage
        .get("upstream_acceptance")
        .ok_or_else(|| io::Error::other(format!("`{id}` lacks upstream_acceptance")).into())
}

/// Evaluate one stage's declared acceptance contract against an acceptance
/// manifest and return every reason the subject cannot be accepted.
///
/// An empty result is acceptance. The contract is data in the train fixture, so
/// this predicate stays honest when the contract changes rather than restating
/// the expected fields a second time.
fn acceptance_rejections(
    contract: &Value,
    manifest: &toml::Value,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut rejections = Vec::new();

    for field in string_set(contract, "required_fields")? {
        match toml_path(manifest, field).and_then(toml::Value::as_str) {
            Some(value) if !value.trim().is_empty() => {}
            Some(_) => rejections.push(format!("`{field}` is empty")),
            None => rejections.push(format!("`{field}` is missing or not a string")),
        }
    }

    for validation in string_set(contract, "required_validation")? {
        match toml_path(manifest, validation).and_then(toml::Value::as_bool) {
            Some(true) => {}
            Some(false) => rejections.push(format!("`{validation}` is not proven")),
            None => rejections.push(format!("`{validation}` is missing or not a bool")),
        }
    }

    if boolean(contract, "requires_changed_subject")? {
        for pair in array(contract, "changed_subject_fields")? {
            let candidate = string(pair, "candidate")?;
            let current = string(pair, "current")?;
            let candidate_value = toml_path(manifest, candidate).and_then(toml::Value::as_str);
            if candidate_value.is_some()
                && candidate_value == toml_path(manifest, current).and_then(toml::Value::as_str)
            {
                rejections.push(format!("`{candidate}` is unchanged from `{current}`"));
            }
        }
    }

    if boolean(contract, "requires_released_build")? {
        let identity = contract
            .get("released_build_identity")
            .ok_or_else(|| io::Error::other("released build required without an identity"))?;
        let field = string(identity, "field")?;
        let bound_by = string(identity, "bound_by")?;

        if toml_path(manifest, field)
            .and_then(toml::Value::as_str)
            .is_none_or(|build| build.trim().is_empty())
        {
            rejections.push(format!("`{field}` names no released build"));
        }
        if toml_path(manifest, bound_by).and_then(toml::Value::as_bool) != Some(true) {
            rejections
                .push(format!("`{bound_by}` does not bind the released build to the subject"));
        }
    }

    Ok(rejections)
}

/// A manifest whose upstream subject has merged but has not shipped in a
/// released build. This satisfies U01 and must still fail DU01.
fn merged_unreleased_manifest() -> Result<toml::Value, Box<dyn Error>> {
    let mut manifest = load_registry_manifest()?;
    set_toml_path(&mut manifest, "extension.new_commit", "c0ffee".repeat(6).into())?;
    set_toml_path(&mut manifest, "extension.new_version", "0.5.0".into())?;
    set_toml_path(&mut manifest, "extension.upstream_branch_containing_commit", "master".into())?;
    set_toml_path(&mut manifest, "validation.submodule_commit_branch_reachable", true.into())?;
    set_toml_path(&mut manifest, "validation.manifest_version_matches", true.into())?;
    Ok(manifest)
}

/// The merged subject above, additionally present in a named released build.
fn released_manifest() -> Result<toml::Value, Box<dyn Error>> {
    let mut manifest = merged_unreleased_manifest()?;
    set_toml_path(&mut manifest, "zed_defaults.released_build", "zed-stable-0.226.1".into())?;
    set_toml_path(&mut manifest, "validation.released_build_contains_commit", true.into())?;
    Ok(manifest)
}

fn core_stages(train: &Value) -> Result<&[Value], Box<dyn Error>> {
    array(train, "stages")
}

fn core_index(train: &Value) -> Result<BTreeMap<&str, &Value>, Box<dyn Error>> {
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

fn dap_index(train: &Value) -> Result<BTreeMap<&str, &Value>, Box<dyn Error>> {
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

/// Stage states that assert a landed increment on `main`.
const LANDED_STATES: [&str; 2] =
    ["complete_static_substrate_execution_not_proven", "authority_merged_execution_not_proven"];

#[test]
fn live_frontier_matches_merged_and_open_pr_state() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let rules = train.get("rules").ok_or_else(|| io::Error::other("train lacks rules"))?;
    assert_eq!(
        string_set(rules, "current_core_frontier")?,
        BTreeSet::from(["C01", "P03", "P06", "P09"])
    );
    assert_eq!(string_set(rules, "current_dap_frontier")?, BTreeSet::from(["D01"]));

    let core = core_index(&train)?;
    assert_eq!(string(core["P00"], "state")?, "complete_static_substrate_execution_not_proven");
    assert_eq!(core["P00"].get("pull_request").and_then(Value::as_u64), Some(8023));

    // P01, P02, P04, and P05 merged their authority increments onto `main`; their
    // owning issues stay open because execution is a separate later stage.
    for (merged, pull_request) in [("P01", 8365), ("P02", 8369), ("P04", 8373), ("P05", 8379)] {
        assert_eq!(
            string(core[merged], "state")?,
            "authority_merged_execution_not_proven",
            "`{merged}` no longer records its merged authority increment"
        );
        assert_eq!(
            core[merged].get("pull_request").and_then(Value::as_u64),
            Some(pull_request),
            "`{merged}` pull request drifted"
        );
    }

    for ready in ["P03", "P06", "P09", "C01"] {
        assert_eq!(string(core[ready], "state")?, "ready");
    }

    let dap = dap_index(&train)?;
    assert_eq!(string(dap["D01"], "state")?, "ready");
    Ok(())
}

#[test]
fn landed_stages_never_depend_on_unlanded_stages() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let core = core_index(&train)?;

    // A stage cannot claim a merged increment while a stage it declares as a
    // prerequisite has not landed. This is the invariant that catches a train
    // left behind by an actual merge.
    for (id, stage) in &core {
        if !LANDED_STATES.contains(&string(stage, "state")?) {
            continue;
        }
        for dependency in string_set(stage, "depends_on")? {
            let declared = core.get(dependency).ok_or_else(|| {
                io::Error::other(format!("`{id}` depends on unknown `{dependency}`"))
            })?;
            assert!(
                LANDED_STATES.contains(&string(declared, "state")?),
                "landed core stage `{id}` depends on unlanded `{dependency}`"
            );
        }
    }

    for stage in dap_stages(&train)? {
        let id = string(stage, "id")?;
        if !LANDED_STATES.contains(&string(stage, "state")?) {
            continue;
        }
        for dependency in string_set(stage, "depends_on_core")? {
            let declared = core.get(dependency).ok_or_else(|| {
                io::Error::other(format!("`{id}` depends on unknown `{dependency}`"))
            })?;
            assert!(
                LANDED_STATES.contains(&string(declared, "state")?),
                "landed DAP stage `{id}` depends on unlanded core `{dependency}`"
            );
        }
    }
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
fn stage_instruction_binds_the_repository_delivery_route() -> Result<(), Box<dyn Error>> {
    let prose = fs::read_to_string(repo_root()?.join(TRAIN_DOC_PATH))?;

    // Every stage is one claim taken through the repository's own public flow.
    // Naming the route here is what stops a stage from inventing a parallel
    // lifecycle, so it cannot quietly drop out of the instruction.
    assert!(
        prose.contains("`deliver-pr`"),
        "the reusable stage instruction no longer names the `deliver-pr` route"
    );

    // A merged authority increment is not a finished stage. The instruction has
    // to keep saying so, because that is the distinction the train encodes.
    assert!(
        prose.contains("authority_merged_execution_not_proven"),
        "the stage instruction no longer separates merged authority from execution"
    );
    Ok(())
}

#[test]
fn registry_manifest_declares_each_field_once_per_table() -> Result<(), Box<dyn Error>> {
    let raw = fs::read_to_string(repo_root()?.join(REGISTRY_MANIFEST_PATH))?;

    // A repeated key or table is a TOML parse error, so a successful parse is
    // itself the one-declaration proof. The expected key sets then reject a
    // silently added or dropped declaration that still parses.
    let manifest: toml::Value = toml::from_str(&raw)?;

    let expected = BTreeMap::from([
        (
            "registry",
            BTreeSet::from([
                "repository",
                "branch",
                "captured_base_commit",
                "captured_tree",
                "extensions_toml_blob",
                "gitmodules_blob",
            ]),
        ),
        (
            "extension",
            BTreeSet::from([
                "id",
                "submodule_path",
                "submodule_remote",
                "current_version",
                "current_commit",
                "new_version",
                "new_commit",
                "upstream_branch_containing_commit",
                "license",
            ]),
        ),
        ("zed_defaults", BTreeSet::from(["issue", "state", "released_build"])),
        (
            "validation",
            BTreeSet::from([
                "pnpm_sort_extensions",
                "registry_package_check",
                "registry_danger_check",
                "diff_sha256",
                "submodule_commit_branch_reachable",
                "manifest_version_matches",
                "released_build_contains_commit",
                "https_remote_verified",
            ]),
        ),
        (
            "submission",
            BTreeSet::from([
                "pr_title",
                "pr_body",
                "expected_changed_paths",
                "blockers",
                "claim_boundary",
            ]),
        ),
    ]);

    for (table_name, keys) in &expected {
        let table = manifest
            .get(table_name)
            .and_then(toml::Value::as_table)
            .ok_or_else(|| io::Error::other(format!("registry manifest lacks `{table_name}`")))?;
        let declared: BTreeSet<&str> = table.keys().map(String::as_str).collect();
        assert_eq!(&declared, keys, "registry manifest table `{table_name}` drifted");
    }

    // Negative control: the parse-based claim above is only meaningful if a
    // second declaration actually fails to parse.
    let duplicated = format!("{raw}\n[validation]\nmanifest_version_matches = true\n");
    assert!(
        toml::from_str::<toml::Value>(&duplicated).is_err(),
        "a duplicate table declaration must not parse"
    );
    Ok(())
}

#[test]
fn released_build_identity_is_non_empty_and_subject_bound() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let dap = dap_index(&train)?;
    let contract = acceptance_contract(dap["DU01"], "DU01")?;

    assert!(boolean(contract, "requires_released_build")?);
    let identity = contract
        .get("released_build_identity")
        .ok_or_else(|| io::Error::other("DU01 lacks released_build_identity"))?;

    // The named build must itself be a required non-empty field, and the
    // containment validation must be required, so a released-build claim can
    // never be satisfied by an empty identity or an unbound assertion.
    let field = string(identity, "field")?;
    let bound_by = string(identity, "bound_by")?;
    let binds_subject = string(identity, "binds_subject")?;
    assert!(
        string_set(contract, "required_fields")?.contains(field),
        "released build field `{field}` is not a required field"
    );
    assert!(
        string_set(contract, "required_validation")?.contains(bound_by),
        "released build binding `{bound_by}` is not a required validation"
    );

    let subjects: BTreeSet<&str> = array(contract, "changed_subject_fields")?
        .iter()
        .map(|pair| string(pair, "candidate"))
        .collect::<Result<_, _>>()?;
    assert!(
        subjects.contains(binds_subject),
        "released build binds `{binds_subject}`, which is not a changed subject"
    );

    // U01 accepts a merged subject and must not require a released build.
    let core = core_index(&train)?;
    assert!(!boolean(acceptance_contract(core["U01"], "U01")?, "requires_released_build")?);
    Ok(())
}

#[test]
fn acceptance_predicates_reject_the_current_blocked_registry_subject() -> Result<(), Box<dyn Error>>
{
    let train = load_train()?;
    let manifest = load_registry_manifest()?;

    // Both acceptances must name the concrete packet this test evaluates, so
    // neither can drift onto a source the predicate never reads.
    assert_eq!(
        toml_path(&manifest, "schema_version").and_then(toml::Value::as_str),
        Some("zed-perl-registry-update.v1")
    );

    // The committed packet is blocked pending upstream merge. Both acceptances
    // must fail closed against it today.
    for (id, stage) in [("U01", core_index(&train)?["U01"]), ("DU01", dap_index(&train)?["DU01"])] {
        let contract = acceptance_contract(stage, id)?;
        assert_eq!(
            string(contract, "acceptance_manifest")?,
            REGISTRY_MANIFEST_PATH,
            "`{id}` acceptance manifest is not the registry packet"
        );
        assert!(
            !acceptance_rejections(contract, &manifest)?.is_empty(),
            "`{id}` accepted the current blocked registry subject"
        );
    }
    Ok(())
}

#[test]
fn dap_acceptance_requires_a_released_build_that_upstream_acceptance_does_not()
-> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let upstream = acceptance_contract(core_index(&train)?["U01"], "U01")?;
    let dap = acceptance_contract(dap_index(&train)?["DU01"], "DU01")?;

    // Positive control: a complete released subject is accepted by both, so the
    // rejections below cannot come from a predicate that never accepts.
    let released = released_manifest()?;
    assert_eq!(acceptance_rejections(upstream, &released)?, Vec::<String>::new());
    assert_eq!(acceptance_rejections(dap, &released)?, Vec::<String>::new());

    // Discriminator: merged upstream is enough for U01 and never enough for DU01.
    let unreleased = merged_unreleased_manifest()?;
    assert_eq!(
        acceptance_rejections(upstream, &unreleased)?,
        Vec::<String>::new(),
        "U01 must accept a merged subject that has not shipped"
    );
    assert!(
        !acceptance_rejections(dap, &unreleased)?.is_empty(),
        "DU01 must reject a merged subject that has not shipped in a released build"
    );
    Ok(())
}

#[test]
fn dap_acceptance_rejects_every_single_subject_defect() -> Result<(), Box<dyn Error>> {
    let train = load_train()?;
    let contract = acceptance_contract(dap_index(&train)?["DU01"], "DU01")?;
    let released = released_manifest()?;

    let current_commit = toml_path(&released, "extension.current_commit")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("registry manifest lacks a current commit"))?
        .to_string();
    let current_version = toml_path(&released, "extension.current_version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("registry manifest lacks a current version"))?
        .to_string();

    // Each row degrades exactly one cell of an otherwise acceptable subject.
    let defects: [(&str, &str, toml::Value); 9] = [
        ("empty commit", "extension.new_commit", String::new().into()),
        ("unchanged commit", "extension.new_commit", current_commit.into()),
        ("empty version", "extension.new_version", String::new().into()),
        ("unchanged version", "extension.new_version", current_version.into()),
        ("empty branch", "extension.upstream_branch_containing_commit", String::new().into()),
        ("unreachable branch", "validation.submodule_commit_branch_reachable", false.into()),
        ("mismatched manifest version", "validation.manifest_version_matches", false.into()),
        ("unnamed released build", "zed_defaults.released_build", String::new().into()),
        (
            "released build without the subject",
            "validation.released_build_contains_commit",
            false.into(),
        ),
    ];

    for (label, path, value) in defects {
        let mut degraded = released.clone();
        set_toml_path(&mut degraded, path, value)?;
        assert!(
            !acceptance_rejections(contract, &degraded)?.is_empty(),
            "DU01 accepted a subject with {label}"
        );
    }

    // A removed declaration is not the same defect as an empty one, and must
    // also fail closed rather than resolving to a default.
    for path in [
        "extension.new_commit",
        "extension.upstream_branch_containing_commit",
        "zed_defaults.released_build",
        "validation.released_build_contains_commit",
    ] {
        let mut degraded = released.clone();
        remove_toml_path(&mut degraded, path)?;
        assert!(
            !acceptance_rejections(contract, &degraded)?.is_empty(),
            "DU01 accepted a subject with `{path}` removed"
        );
    }
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
        "`P02 -> D02`",
        "`P03 -> D02`",
        "`DA01 -> D05`",
        "`D02 -> D05`",
        "`DM02 -> D05`",
        "`C01 -> D05`",
        "`D05 -> D06`",
        "`P09 -> D06`",
    ] {
        assert!(prose.contains(edge), "prose train lacks edge {edge}");
    }
    for required in ["#8647", "#8661", "#9468", "#9483", "#9485", "#9516"] {
        assert!(
            prose.contains(required),
            "prose train lacks current frontier reference {required}"
        );
    }
    Ok(())
}
