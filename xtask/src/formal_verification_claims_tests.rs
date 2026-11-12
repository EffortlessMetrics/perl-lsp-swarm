//! Failure-oriented controls for the actual inventory validator.

use super::*;
use std::fs;
use tempfile::{TempDir, tempdir};

const POLICY: &str = "gates:\n  - name: sample\n    tier: merge_gate\n    required: true\n    command: cargo test\n    quarantine: false\n";
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

fn commit(root: &Path) -> Result<()> {
    git(root, &["add", "--all"])?;
    git(
        root,
        &[
            "-c",
            "user.name=Formal fixture",
            "-c",
            "user.email=formal@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    )?;
    Ok(())
}

fn fixture() -> Result<TempDir> {
    let directory = tempdir()?;
    git(directory.path(), &["init", "--quiet"])?;
    fs::write(directory.path().join("source.rs"), "pub struct Original;\n")?;
    commit(directory.path())?;
    Ok(directory)
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("repo root")?;
    for field in ["policy", "workflow"] {
        let snapshot = Snapshot::open(root)?;
        let mut registry = registry()?;
        let claim = registry
            .claims
            .iter_mut()
            .find(|claim| claim.id == "FV-UNSAFE-RATCHET-001")
            .context("unsafe ratchet claim")?;
        let authority = claim.authority.as_mut().context("unsafe ratchet authority")?;
        {
            let source = match field {
                "policy" => authority.policy.as_mut(),
                "workflow" => authority.workflow.as_mut(),
                _ => bail!("unknown fixture field"),
            }
            .context("required-gate source")?;
            let path = source.path.clone();
            source.marker.push_str("\n    stale_marker_control: true");
            rejects(validate(&snapshot, &registry), &format!("stale source marker: {path}"))?;
        }
        let source = match field {
            "policy" => authority.policy.as_mut(),
            "workflow" => authority.workflow.as_mut(),
            _ => bail!("unknown fixture field"),
        }
        .context("required-gate source")?;
        source.path = "docs/how-to/FORMAL_VERIFICATION.md".to_owned();
        rejects(validate(&snapshot, &registry), &format!("required gate {field} path must be"))?;
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
    commit(directory.path())?;
    assert_eq!(snapshot.read("source.rs")?, "pub struct Original;\n");
    rejects(snapshot.finish(), "HEAD changed")
}

#[cfg(unix)]
#[test]
fn committed_symlink_is_not_an_evidence_source() -> Result<()> {
    let directory = fixture()?;
    std::os::unix::fs::symlink("source.rs", directory.path().join("alias.rs"))?;
    commit(directory.path())?;
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
    required_gate(POLICY, "sample", "cargo test")?;
    let commented = POLICY.lines().map(|line| format!("# {line}\n")).collect::<String>();
    rejects(
        required_gate(&format!("gates: []\n{commented}"), "sample", "cargo test"),
        "missing or duplicated",
    )
}

#[test]
fn advisory_wrong_command_and_duplicate_gates_are_rejected() -> Result<()> {
    rejects(
        required_gate(&POLICY.replace("required: true", "required: false"), "sample", "cargo test"),
        "not required",
    )?;
    rejects(
        required_gate(&POLICY.replace("merge_gate", "advisory"), "sample", "cargo test"),
        "wrong tier",
    )?;
    rejects(
        required_gate(&POLICY.replace("cargo test", "echo skipped"), "sample", "cargo test"),
        "command differs",
    )?;
    rejects(
        required_gate(
            &POLICY.replace("quarantine: false", "quarantine: true"),
            "sample",
            "cargo test",
        ),
        "quarantined",
    )?;
    let (_, row) = POLICY.split_once('\n').context("fixture policy row")?;
    rejects(
        required_gate(&format!("{POLICY}{row}"), "sample", "cargo test"),
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
