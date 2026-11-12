//! Failure-oriented controls for the actual inventory validator.

use super::*;
use std::fs;
use tempfile::{TempDir, tempdir};

#[path = "../tests/git_test_support.rs"]
mod git_test_support;
use git_test_support::HermeticGit;

const POLICY: &str = "gates:\n  - name: sample\n    tier: merge_gate\n    required: true\n    command: cargo test\n    quarantine: false\n";
const POLICY_DECLARATION: &str = "- name: sample";
const WORKFLOW: &str = r#"jobs:
  check-all-targets:
    needs: [draft-pr-check, preflight-latest-check]
    if: needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'
    steps:
      - run: cargo test
"#;

fn rejects<T>(result: Result<T>, expected: &str) -> Result<()> {
    match result {
        Ok(_) => bail!("negative control unexpectedly passed; expected {expected}"),
        Err(error) => {
            ensure!(
                format!("{error:#}").contains(expected),
                "wrong failure: {error:#}; expected {expected}"
            );
            Ok(())
        }
    }
}

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
    git: HermeticGit,
}

impl Fixture {
    fn path(&self) -> &Path {
        &self.root
    }
}

fn commit(fixture: &Fixture) -> Result<()> {
    fixture.git.git(fixture.path(), &["add", "--all"])?;
    fixture.git.git(fixture.path(), &["commit", "-m", "fixture"])?;
    Ok(())
}

fn fixture() -> Result<Fixture> {
    let directory = tempdir()?;
    let root = directory.path().join("repo");
    let git = HermeticGit::at(&directory.path().join("pins"))?;
    git.init_repo(&root)?;
    let fixture = Fixture { _directory: directory, root, git };
    fs::write(fixture.path().join("source.rs"), "pub struct Original;\n")?;
    commit(&fixture)?;
    Ok(fixture)
}

fn write_fixture(fixture: &Fixture, path: &str, body: &str) -> Result<()> {
    let path = fixture.path().join(path);
    fs::create_dir_all(path.parent().context("fixture parent")?)?;
    fs::write(path, body)?;
    Ok(())
}

fn validate_root(root: &Path, registry: &Registry) -> Result<()> {
    let snapshot = Snapshot::open(root)?;
    validate(&snapshot, registry)?;
    snapshot.finish()
}

fn source_registry() -> Result<Registry> {
    let mut registry = registry()?;
    registry.claims.truncate(1);
    let claim = &mut registry.claims[0];
    claim.target = SourceRef { path: "source.rs".to_owned(), marker: "Original".to_owned() };
    claim.status = Status::Planned;
    claim.runner = None;
    claim.authority = None;
    Ok(registry)
}

fn unsafe_fixture() -> Result<(Fixture, Registry)> {
    let fixture = fixture()?;
    let mut registry = registry()?;
    registry.claims.retain(|claim| claim.id == "FV-UNSAFE-RATCHET-001");
    ensure!(registry.claims.len() == 1, "actual unsafe row missing or duplicated");
    for (path, body) in [
        ("ci/check_unsafe_prod.sh", include_str!("../../ci/check_unsafe_prod.sh")),
        (".ci/gate-policy.yaml", include_str!("../../.ci/gate-policy.yaml")),
        (".github/workflows/ci.yml", include_str!("../../.github/workflows/ci.yml")),
    ] {
        write_fixture(&fixture, path, body)?;
    }
    commit(&fixture)?;
    validate_root(fixture.path(), &registry)?;
    Ok((fixture, registry))
}

#[test]
fn registry_is_valid_at_committed_head() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("repo root")?;
    let snapshot = Snapshot::open(root)?;
    snapshot.bind_binary()?;
    validate(&snapshot, &registry()?)?;
    snapshot.finish()
}

#[test]
fn mutated_required_gate_markers_are_stale() -> Result<()> {
    let (fixture, original) = unsafe_fixture()?;
    for field in ["policy", "workflow"] {
        for (marker, failure) in [
            ("", "empty source marker"),
            (" \t", "empty source marker"),
            ("removed-formal-inventory-binding", "stale source marker"),
        ] {
            let mut registry = original.clone();
            {
                let authority =
                    registry.claims[0].authority.as_mut().context("unsafe authority")?;
                let source = match field {
                    "policy" => authority.policy.as_mut(),
                    "workflow" => authority.workflow.as_mut(),
                    _ => bail!("unknown fixture field"),
                }
                .context("required-gate source")?;
                source.marker = marker.to_owned();
            }
            rejects(validate_root(fixture.path(), &registry), failure)?;
        }
        let mut registry = original.clone();
        {
            let authority = registry.claims[0].authority.as_mut().context("unsafe authority")?;
            let source = match field {
                "policy" => authority.policy.as_mut(),
                "workflow" => authority.workflow.as_mut(),
                _ => bail!("unknown fixture field"),
            }
            .context("required-gate source")?;
            source.path = "docs/how-to/FORMAL_VERIFICATION.md".to_owned();
        }
        rejects(
            validate_root(fixture.path(), &registry),
            &format!("required gate {field} path must be"),
        )?;
    }
    Ok(())
}

#[test]
fn paths_reject_ambiguous_platform_and_pathspec_forms() {
    for path in [
        "",
        "/tmp/x",
        "../x",
        "a/../x",
        "a/./x",
        "a//x",
        "a/",
        "C:/x",
        "a\\x",
        ":(glob)*",
        "a*",
        "archive/x",
        "a\nx",
        "./source.rs",
    ] {
        assert!(!valid_path(path), "accepted {path:?}");
    }
    assert!(valid_path(".github/workflows/ci.yml"));
    assert!(valid_path("xtask/src/bin/formal-verification-claims.rs"));
}

#[test]
fn bound_requires_a_positive_number_not_protocol_digits() {
    assert!(explicit_bound("At most 8 scalars and 32 bytes."));
    assert!(!explicit_bound("UTF-8 and UTF-16 bounded inputs"));
    assert!(!explicit_bound("0 operations"));
    assert!(!explicit_bound("-1 operations"));
}

#[test]
fn claim_ids_have_closed_grammar() {
    assert!(valid_claim_id("FV-SOURCE-IDENTITY-001"));
    for id in ["FV-!1", "FV--001", "FV-A B-001", "FV-a-001", "FV-A-1", "FV-A-0001"] {
        assert!(!valid_claim_id(id), "accepted {id}");
    }
}

#[test]
fn snapshot_reads_blobs_not_modified_working_files() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    fs::write(directory.path().join("source.rs"), "pub struct Modified;\n")?;
    assert_eq!(snapshot.read("source.rs")?, "pub struct Original;\n");
    rejects(snapshot.finish(), "dirty working tree")
}

#[test]
fn dirty_and_untracked_inputs_refuse_commit_attribution() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    fs::write(directory.path().join("untracked.rs"), "pub struct Other;\n")?;
    rejects(snapshot.read("untracked.rs"), "missing or ambiguous committed input")?;
    rejects(Snapshot::open(directory.path()), "dirty working tree")
}

#[test]
fn changed_head_refuses_mixed_identity() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    fs::write(directory.path().join("source.rs"), "pub struct Later;\n")?;
    commit(&directory)?;
    assert_eq!(snapshot.read("source.rs")?, "pub struct Original;\n");
    rejects(snapshot.finish(), "HEAD changed")
}

#[cfg(unix)]
#[test]
fn committed_symlink_is_not_an_evidence_source() -> Result<()> {
    let directory = fixture()?;
    directory.git.git(directory.path(), &["config", "core.symlinks", "true"])?;
    std::os::unix::fs::symlink("source.rs", directory.path().join("alias.rs"))?;
    commit(&directory)?;
    let snapshot = Snapshot::open(directory.path())?;
    rejects(snapshot.read("alias.rs"), "not a regular committed file")
}

#[test]
fn source_markers_cannot_be_empty_or_stale() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    for (marker, failure) in
        [("", "empty source marker"), ("pub struct Absent;", "stale source marker")]
    {
        rejects(
            snapshot.source(&SourceRef { path: "source.rs".to_owned(), marker: marker.to_owned() }),
            failure,
        )?;
    }
    Ok(())
}

#[test]
fn removed_gate_with_surviving_comment_is_rejected() -> Result<()> {
    required_gate(POLICY, POLICY_DECLARATION, "cargo test")?;
    let commented = POLICY.lines().map(|line| format!("# {line}\n")).collect::<String>();
    rejects(
        required_gate(&format!("gates: []\n{commented}"), POLICY_DECLARATION, "cargo test"),
        "missing or duplicated",
    )
}

#[test]
fn advisory_wrong_command_and_duplicate_gates_are_rejected() -> Result<()> {
    rejects(
        required_gate(
            &POLICY.replace("required: true", "required: false"),
            POLICY_DECLARATION,
            "cargo test",
        ),
        "not required",
    )?;
    rejects(
        required_gate(&POLICY.replace("merge_gate", "advisory"), POLICY_DECLARATION, "cargo test"),
        "wrong tier",
    )?;
    rejects(
        required_gate(
            &POLICY.replace("cargo test", "echo skipped"),
            POLICY_DECLARATION,
            "cargo test",
        ),
        "command differs",
    )?;
    rejects(
        required_gate(
            &POLICY.replace("quarantine: false", "quarantine: true"),
            POLICY_DECLARATION,
            "cargo test",
        ),
        "quarantined",
    )?;
    let (_, row) = POLICY.split_once('\n').context("fixture policy row")?;
    rejects(
        required_gate(&format!("{POLICY}{row}"), POLICY_DECLARATION, "cargo test"),
        "missing or duplicated",
    )
}

#[test]
fn commented_or_optional_workflow_steps_are_rejected() -> Result<()> {
    required_step(WORKFLOW, "check-all-targets", "cargo test")?;
    let comment = WORKFLOW
        .replace("    steps:\n      - run: cargo test\n", "    steps: []\n# - run: cargo test\n");
    rejects(required_step(&comment, "check-all-targets", "cargo test"), "missing or duplicated")?;
    let optional = WORKFLOW.replace("      - run:", "      - if: false\n        run:");
    rejects(required_step(&optional, "check-all-targets", "cargo test"), "is optional")?;
    let ignored = WORKFLOW.replace("      - run:", "      - continue-on-error: true\n        run:");
    rejects(required_step(&ignored, "check-all-targets", "cargo test"), "is optional")?;
    let skipped = WORKFLOW.replace(CI_ADMISSION_PREDICATE, "false");
    rejects(
        required_step(&skipped, "check-all-targets", "cargo test"),
        "admission predicate changed",
    )?;
    let missing_dependency = WORKFLOW
        .replace("needs: [draft-pr-check, preflight-latest-check]", "needs: [draft-pr-check]");
    rejects(
        required_step(&missing_dependency, "check-all-targets", "cargo test"),
        "dependencies changed",
    )
}

#[test]
fn forged_rollout_and_empty_runners_are_rejected() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    let mut registry = registry()?;
    registry.claims.truncate(1);
    registry.claims[0].target =
        SourceRef { path: "source.rs".to_owned(), marker: "Original".to_owned() };
    registry.claims[0].status = Status::Planned;
    rejects(validate(&snapshot, &registry), "planned claim advertises evidence")?;
    registry.claims[0].status = Status::ImplementedUnwired;
    registry.claims[0].runner = Some("  ".to_owned());
    rejects(validate(&snapshot, &registry), "empty/multiline runner")
}

#[test]
fn unknown_schema_fields_and_enum_values_fail_closed() -> Result<()> {
    let mut value: serde_json::Value = serde_json::from_str(claim_registry::REGISTRY_JSON)?;
    value["claims"][0]["native_pass"] = true.into();
    rejects(serde_json::from_value::<Registry>(value).map_err(Into::into), "unknown field")?;
    let mut value: serde_json::Value = serde_json::from_str(claim_registry::REGISTRY_JSON)?;
    value["claims"][0]["status"] = "fully_proved".into();
    rejects(serde_json::from_value::<Registry>(value).map_err(Into::into), "unknown variant")
}

#[test]
fn reports_preserve_identity_limits_and_not_evaluated_result() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    snapshot.read("source.rs")?;
    let registry = registry()?;
    let report = report(&snapshot, &registry);
    let json = serde_json::to_value(&report)?;
    let text = markdown(&report)?;
    assert_eq!(json["native_proof_result"], "not_evaluated");
    assert_eq!(json["inventory_inputs"]["source.rs"], snapshot.inputs.borrow()["source.rs"]);
    assert!(text.contains(&snapshot.commit) && text.contains(&snapshot.tree));
    for claim in &registry.claims {
        for value in
            [&claim.id, &claim.bound, &claim.assumptions, &claim.limitations, &claim.counterexample]
        {
            assert!(text.contains(value), "report omitted {value}");
        }
        assert!(text.contains(claim.status.as_str()));
        assert!(text.contains(claim.evidence.as_str()));
    }
    Ok(())
}

#[test]
fn native_proof_cannot_be_promoted_by_metadata() -> Result<()> {
    let directory = fixture()?;
    let snapshot = Snapshot::open(directory.path())?;
    let mut registry = registry()?;
    registry.claims.truncate(1);
    let claim = &mut registry.claims[0];
    claim.target = SourceRef { path: "source.rs".to_owned(), marker: "Original".to_owned() };
    claim.status = Status::Enforced;
    claim.evidence = Evidence::BoundedModel;
    claim.bound = "2 operations".to_owned();
    rejects(validate(&snapshot, &registry), "native proof admission is not implemented")
}

/// Snapshot the caller's repository, including index, objects, config, and dirty files.
fn repository_bytes(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                files.insert(entry.path().strip_prefix(root)?.to_owned(), fs::read(entry.path())?);
            }
        }
    }
    Ok(files)
}

#[test]
fn git_selectors_cannot_redirect_snapshot_or_fixture_mutations() -> Result<()> {
    const PROBE_ROOT: &str = "FORMAL_INVENTORY_GIT_PROBE_ROOT";
    const PROBE_HEAD: &str = "FORMAL_INVENTORY_GIT_PROBE_HEAD";
    const TEST_NAME: &str = "tests::git_selectors_cannot_redirect_snapshot_or_fixture_mutations";
    if let Some(root) = env::var_os(PROBE_ROOT) {
        let root = Path::new(&root);
        let snapshot = Snapshot::open(root)?;
        ensure!(snapshot.commit == env::var(PROBE_HEAD)?, "snapshot read the caller's HEAD");
        validate(&snapshot, &source_registry()?)?;
        snapshot.finish()?;
        // Creating and committing a second fixture must also ignore the caller's selectors.
        let own = fixture()?;
        write_fixture(&own, "own.txt", "fixture-local mutation\n")?;
        commit(&own)?;
        ensure!(
            own.git.git(own.path(), &["rev-parse", "HEAD"])?.len() == 40,
            "fixture inherited the caller's object-format default"
        );
        return Ok(());
    }
    let target = fixture()?;
    write_fixture(&target, "source.rs", "pub struct Original;\n// target-only object closure\n")?;
    commit(&target)?;
    let caller = fixture()?;
    write_fixture(&caller, "source.rs", "pub struct Caller;\n")?;
    commit(&caller)?;
    write_fixture(&caller, "source.rs", "pub struct Staged;\n")?;
    caller.git.git(caller.path(), &["add", "source.rs"])?;
    write_fixture(&caller, "source.rs", "pub struct Unstaged;\n")?;
    write_fixture(&caller, "untracked.txt", "caller-owned untracked bytes\n")?;
    let before = repository_bytes(caller.path())?;
    let head = target.git.git(target.path(), &["rev-parse", "HEAD"])?;
    let git_dir = caller.path().join(".git");
    let ambient = tempdir()?;
    let ambient_config = ambient.path().join("git-config");
    fs::write(&ambient_config, "[init]\n\tdefaultObjectFormat = sha256\n")?;
    for mode in ["hook", "objects", "config", "ambient-defaults"] {
        let mut child = Command::new(env::current_exe()?);
        target.git.apply_env(&mut child);
        child
            .args(["--exact", TEST_NAME, "--nocapture", "--color", "never"])
            .env(PROBE_ROOT, target.path())
            .env(PROBE_HEAD, &head);
        match mode {
            "hook" => {
                child
                    .env("GIT_DIR", &git_dir)
                    .env("GIT_COMMON_DIR", &git_dir)
                    .env("GIT_WORK_TREE", caller.path())
                    .env("GIT_INDEX_FILE", git_dir.join("index"))
                    .env("GIT_PREFIX", "foreign/");
            }
            "objects" => {
                child
                    .env("GIT_OBJECT_DIRECTORY", git_dir.join("objects"))
                    .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", git_dir.join("objects"));
            }
            "config" => {
                child
                    .env("GIT_CONFIG_COUNT", "1")
                    .env("GIT_CONFIG_KEY_0", "core.bare")
                    .env("GIT_CONFIG_VALUE_0", "true");
            }
            _ => {
                child
                    .env("GIT_CONFIG_NOSYSTEM", "0")
                    .env("GIT_CONFIG_GLOBAL", &ambient_config)
                    .env("GIT_CONFIG_SYSTEM", &ambient_config);
            }
        }
        let output = child.output()?;
        ensure!(
            output.status.success(),
            "{mode} probe failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout)?;
        ensure!(
            stdout.contains(&format!("test {TEST_NAME} ... ok"))
                && stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
            "{mode} child did not execute the named probe: {stdout}"
        );
        ensure!(
            repository_bytes(caller.path())? == before,
            "{mode} probe changed caller-owned bytes"
        );
    }
    Ok(())
}

#[test]
fn containing_job_admission_cannot_be_disabled_or_replaced() -> Result<()> {
    let fixture = fixture()?;
    let mut registry = source_registry()?;
    let claim = &mut registry.claims[0];
    claim.status = Status::Enforced;
    claim.runner = Some("cargo test".to_owned());
    claim.authority = Some(Authority {
        kind: AuthorityKind::SourceMarker,
        source: Some(SourceRef {
            path: ".github/workflows/ci.yml".to_owned(),
            marker: "run: cargo test".to_owned(),
        }),
        policy: None,
        workflow: None,
        inventory_controls: None,
    });
    write_fixture(&fixture, ".github/workflows/ci.yml", WORKFLOW)?;
    commit(&fixture)?;
    validate_root(fixture.path(), &registry)?;
    for predicate in ["false", "true", "always()", "needs.unrelated.outputs.ok == 'true'"] {
        let workflow = WORKFLOW.replace(CI_ADMISSION_PREDICATE, predicate);
        write_fixture(&fixture, ".github/workflows/ci.yml", &workflow)?;
        commit(&fixture)?;
        rejects(
            validate_root(fixture.path(), &registry),
            "required job admission predicate changed",
        )?;
    }
    let absent = WORKFLOW.replace(&format!("    if: {CI_ADMISSION_PREDICATE}\n"), "");
    write_fixture(&fixture, ".github/workflows/ci.yml", &absent)?;
    commit(&fixture)?;
    rejects(validate_root(fixture.path(), &registry), "required job admission predicate changed")?;
    let ignored = WORKFLOW.replace("    steps:", "    continue-on-error: true\n    steps:");
    write_fixture(&fixture, ".github/workflows/ci.yml", &ignored)?;
    commit(&fixture)?;
    rejects(validate_root(fixture.path(), &registry), "required job may ignore failure")
}

#[test]
fn required_gate_markers_bind_active_rows() -> Result<()> {
    let (fixture, mut registry) = unsafe_fixture()?;
    let policy = registry.claims[0]
        .authority
        .as_mut()
        .context("unsafe authority")?
        .policy
        .as_mut()
        .context("unsafe policy")?;
    policy.marker = "[{name: unsafe_prod_check, required: false}]".to_owned();
    let body = fs::read_to_string(fixture.path().join(&policy.path))?;
    write_fixture(&fixture, &policy.path, &format!("{body}\n# historical: {}\n", policy.marker))?;
    commit(&fixture)?;
    rejects(
        validate_root(fixture.path(), &registry),
        "gate marker differs from active policy row",
    )?;

    let (fixture, registry) = unsafe_fixture()?;
    let workflow = registry.claims[0]
        .authority
        .as_ref()
        .context("unsafe authority")?
        .workflow
        .as_ref()
        .context("unsafe workflow")?;
    let body = fs::read_to_string(fixture.path().join(&workflow.path))?;
    // Keep the gate selected, but preserve its former row prefix only in a comment.
    let changed = body.replace(
        &workflow.marker,
        "gates: unsafe_prod_check docs_agents_contract_workflows msrv_authority_sync",
    );
    ensure!(changed != body, "workflow falsifier did not apply");
    let changed = format!("{changed}\n# historical: {}\n", workflow.marker);
    write_fixture(&fixture, &workflow.path, &changed)?;
    commit(&fixture)?;
    rejects(
        validate_root(fixture.path(), &registry),
        "workflow marker differs from active shard row",
    )
}

#[test]
fn removed_actual_unsafe_gate_with_retained_marker_is_rejected() -> Result<()> {
    let (fixture, registry) = unsafe_fixture()?;
    let binding = registry.claims[0]
        .authority
        .as_ref()
        .context("unsafe authority")?
        .policy
        .as_ref()
        .context("unsafe policy")?;
    let mut body: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(fixture.path().join(&binding.path))?)?;
    body["gates"]
        .as_sequence_mut()
        .context("actual policy gates")?
        .retain(|gate| gate["name"].as_str() != Some("unsafe_prod_check"));
    // Preserve the complete shipped marker as non-executing YAML data outside gates.
    let changed =
        format!("{}\nretained_old_gate:\n{}\n", serde_yaml_ng::to_string(&body)?, binding.marker);
    ensure!(changed.contains(&binding.marker), "retained marker disappeared");
    write_fixture(&fixture, &binding.path, &changed)?;
    commit(&fixture)?;
    rejects(validate_root(fixture.path(), &registry), "required gate is missing or duplicated")
}

#[test]
fn inventory_surface_binds_declared_controls() -> Result<()> {
    let fixture = fixture()?;
    let mut registry = registry()?;
    registry.claims.truncate(1);
    registry.claims[0].target =
        SourceRef { path: "source.rs".to_owned(), marker: "Original".to_owned() };
    let producer = "scripts/ci/required-merge-surface.sh";
    write_fixture(&fixture, producer, include_str!("../../scripts/ci/required-merge-surface.sh"))?;
    write_fixture(
        &fixture,
        ".github/workflows/ci.yml",
        &WORKFLOW.replace("run: cargo test", "run: bash scripts/ci/required-merge-surface.sh"),
    )?;
    commit(&fixture)?;
    validate_root(fixture.path(), &registry)?;
    let original = fs::read_to_string(fixture.path().join(producer))?;

    let removed_name =
        original.replacen("  paths_reject_ambiguous_platform_and_pathspec_forms \\\n", "", 1);
    ensure!(removed_name != original, "named-control mutation did not apply");
    write_fixture(&fixture, producer, &removed_name)?;
    commit(&fixture)?;
    rejects(
        validate_root(fixture.path(), &registry),
        "inventory control block differs from registry declaration",
    )?;

    let wrong_count = original.replacen("ok\\. 22 passed", "ok\\. 21 passed", 1);
    ensure!(wrong_count != original, "denominator mutation did not apply");
    write_fixture(&fixture, producer, &wrong_count)?;
    commit(&fixture)?;
    rejects(validate_root(fixture.path(), &registry), "test result: ok\\. 22 passed")?;

    write_fixture(&fixture, producer, &original)?;
    commit(&fixture)?;
    let mut sibling = super::registry()?;
    sibling.claims.retain(|claim| claim.id == "FV-WORKSPACE-OWNERSHIP-001");
    ensure!(sibling.claims.len() == 1, "workspace ownership row missing or duplicated");
    sibling.claims[0].target =
        SourceRef { path: "source.rs".to_owned(), marker: "Original".to_owned() };
    validate_root(fixture.path(), &sibling)?;

    sibling.claims[0].authority.as_mut().context("sibling authority")?.inventory_controls =
        Some(vec!["registry_is_valid_at_committed_head".to_owned()]);
    rejects(validate_root(fixture.path(), &sibling), "unexpected inventory controls")
}
