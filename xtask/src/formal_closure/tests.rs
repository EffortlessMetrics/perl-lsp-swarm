//! Real Git fixtures plus independent canonical-identity and refusal controls.

use super::formal_receipt::{
    ArtifactIdentity, ArtifactKind, ClaimDisposition, CorrespondenceEvidence, CorrespondenceStatus,
    ExpectedSet, FormalMethod, FreshnessEvidence, FreshnessStatus, NativeEvidence, NativeResult,
    NonPassReason, Obligations, PropertyOutcome, RolloutState, ToolComponent,
};
use super::*;
use std::{fs, path::Path};
use tempfile::TempDir;

fn commit(root: &Path) -> Result<()> {
    snapshot::git(root, &["add", "--all"])?;
    snapshot::git(
        root,
        &[
            "-c",
            "user.name=Formal Fixture",
            "-c",
            "user.email=formal@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )?;
    Ok(())
}

fn spec() -> Spec {
    let roles = [Role::Kernel, Role::Harness, Role::Property, Role::Control, Role::SymbolicDomain]
        .into_iter()
        .collect::<BTreeSet<_>>();
    Spec {
        schema_version: SPEC_VERSION.to_owned(),
        source_policy: SourcePolicy::CleanOnly,
        claim: ClaimIdentity {
            id: "FV-POSITION-LAWS-001".to_owned(),
            generation: 1,
            owner_issue: 19300,
            profile: "fixture".to_owned(),
            method: FormalMethod::BoundedProved,
            binding: ImplementationBinding::SubstrateOnly,
            rollout: RolloutState::RunnerAvailable,
        },
        members: vec![Member { path: "src/kernel.rs".to_owned(), roles: roles.clone() }],
        exclusions: ROLES
            .iter()
            .filter(|role| !roles.contains(role))
            .map(|role| (*role, "not applicable to this synthetic test boundary".to_owned()))
            .collect(),
        toolchain: ToolchainIdentity {
            provider: "kani".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            components: BTreeMap::from([(
                "kani".to_owned(),
                ToolComponent {
                    version: "fixture-version".to_owned(),
                    digest: hash(b"fixture", &[b"tool"]),
                },
            )]),
            features: vec![],
            cfg: vec!["kani".to_owned()],
        },
        bounds: ProofBounds {
            values: BTreeMap::from([("unwind".to_owned(), Value::from(32))]),
            assumptions: vec!["same immutable source".to_owned()],
            fairness: vec![],
            wall_seconds: 120,
            output_bytes: 1048576,
        },
        command: vec!["kani".to_owned(), "proof.rs".to_owned()],
        dependency_model: BTreeMap::from([("stdlib".to_owned(), hash(b"fixture", &[b"stdlib"]))]),
        environment: BTreeMap::from([("LANG".to_owned(), "C.UTF-8".to_owned())]),
        unknown_inputs: vec![],
    }
}

fn fixture() -> Result<TempDir> {
    let directory = tempfile::tempdir()?;
    snapshot::git(directory.path(), &["init", "--quiet"])?;
    for (path, content) in ENGINE
        .iter()
        .copied()
        .chain([("src/kernel.rs", "fn subject() {}\n"), (".gitignore", "target/\n")])
    {
        let path = directory.path().join(path);
        fs::create_dir_all(path.parent().context("fixture parent")?)?;
        fs::write(path, content)?;
    }
    commit(directory.path())?;
    Ok(directory)
}

fn evaluate(root: &Path, spec: &Spec) -> Result<Computed> {
    compute(&Snapshot::open(root)?, spec)
}

fn complete(ids: &[&str]) -> ExpectedSet {
    ExpectedSet {
        expected: ids.len() as u32,
        observed: ids.len() as u32,
        satisfied: ids.len() as u32,
        ids: ids.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn receipt(current: &Computed) -> FormalProofReceipt {
    FormalProofReceipt {
        check: formal_receipt::FORMAL_PROOF_CHECK.to_owned(),
        schema_version: formal_receipt::FORMAL_PROOF_SCHEMA_VERSION.to_owned(),
        claim: current.claim.clone(),
        source: current.source.clone(),
        closure: current.closure.clone(),
        toolchain: current.toolchain.clone(),
        bounds: current.bounds.clone(),
        obligations: Obligations {
            positive: complete(&["law"]),
            harness_controls: complete(&["wrong-law"]),
        },
        native: NativeEvidence {
            result: NativeResult::Pass,
            reason: None,
            process_settled: true,
            properties: BTreeMap::from([("law".to_owned(), PropertyOutcome::Pass)]),
        },
        freshness: FreshnessEvidence {
            status: FreshnessStatus::Current,
            evaluated_source_commit: current.source.commit.clone(),
            evaluated_closure_digest: current.closure.digest.clone(),
            reason: None,
        },
        correspondence: CorrespondenceEvidence {
            status: CorrespondenceStatus::Established,
            controls: complete(&["wrong-production"]),
            artifact_ids: vec!["binding".to_owned()],
            reason: None,
        },
        artifacts: vec![
            ArtifactIdentity {
                id: "native".to_owned(),
                kind: ArtifactKind::StructuredReport,
                digest: hash(b"fixture", &[b"native"]),
                path: "target/native.json".to_owned(),
                required: true,
            },
            ArtifactIdentity {
                id: "binding".to_owned(),
                kind: ArtifactKind::Correspondence,
                digest: hash(b"fixture", &[b"binding"]),
                path: "target/binding.json".to_owned(),
                required: true,
            },
        ],
        disposition: ClaimDisposition {
            status: DispositionStatus::Active,
            reason: None,
            superseded_by: None,
        },
    }
}

#[test]
fn exact_inputs_compare_current_without_accepting_native_proof() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let old = receipt(&current);
    ensure!(formal_receipt::validate_value(&serde_json::to_value(&old)?).is_empty());
    let result = compare(&old, Some(&current));
    assert_eq!(result.freshness, Freshness::Current);
    assert!(!result.proof_accepted && !result.correspondence_established);
    assert!(!result.correspondence_inputs_unchanged);
    assert_eq!(result.closure_completeness, "not_established");
    Ok(())
}

#[test]
fn unrelated_commit_and_tree_movement_preserve_semantic_identity() -> Result<()> {
    let root = fixture()?;
    let before = evaluate(root.path(), &spec())?;
    fs::write(root.path().join("unrelated.md"), "unrelated change\n")?;
    commit(root.path())?;
    let after = evaluate(root.path(), &spec())?;
    assert_ne!(before.source.commit, after.source.commit);
    assert_ne!(before.source.tree, after.source.tree);
    assert_eq!(before.closure.digest, after.closure.digest);
    assert_eq!(compare(&receipt(&before), Some(&after)).freshness, Freshness::Current);
    Ok(())
}

#[test]
fn one_production_byte_stales_and_names_the_changed_member() -> Result<()> {
    let root = fixture()?;
    let before = evaluate(root.path(), &spec())?;
    fs::write(root.path().join("src/kernel.rs"), "fn subject() { }\n")?;
    commit(root.path())?;
    let after = evaluate(root.path(), &spec())?;
    let result = compare(&receipt(&before), Some(&after));
    assert_eq!(result.freshness, Freshness::Stale);
    assert_eq!(result.first_difference.as_deref(), Some("source/src/kernel.rs"));
    Ok(())
}

#[test]
fn all_configuration_dimensions_invalidate_reuse() -> Result<()> {
    let root = fixture()?;
    let base = spec();
    let before = evaluate(root.path(), &base)?;
    for dimension in 0..15 {
        let mut changed = base.clone();
        match dimension {
            0 => {
                changed.bounds.values.insert("unwind".to_owned(), 31.into());
            }
            1 => {
                changed.bounds.assumptions.push("new assumption".to_owned());
            }
            2 => {
                changed.bounds.fairness.push("fair completion".to_owned());
            }
            3 => {
                changed.bounds.wall_seconds += 1;
            }
            4 => {
                changed.bounds.output_bytes += 1;
            }
            5 => {
                changed.toolchain.target = "aarch64-unknown-linux-gnu".to_owned();
            }
            6 => {
                changed.toolchain.features.push("feature".to_owned());
            }
            7 => {
                changed.toolchain.cfg.push("other".to_owned());
            }
            8 => {
                changed.toolchain.components.get_mut("kani").context("tool")?.version.push('x');
            }
            9 => {
                changed.toolchain.components.get_mut("kani").context("tool")?.digest =
                    hash(b"fixture", &[b"new-tool"]);
            }
            10 => {
                changed.environment.insert("LANG".to_owned(), "C".to_owned());
            }
            11 => {
                changed
                    .dependency_model
                    .insert("stdlib".to_owned(), hash(b"fixture", &[b"new-stdlib"]));
            }
            12 => {
                changed.command.push("--exact".to_owned());
            }
            13 => {
                changed.claim.profile.push('x');
            }
            _ => {
                changed.claim.generation += 1;
            }
        };
        let after = evaluate(root.path(), &changed)?;
        assert_eq!(
            compare(&receipt(&before), Some(&after)).freshness,
            Freshness::Stale,
            "dimension {dimension} did not invalidate evidence"
        );
    }
    Ok(())
}

#[test]
fn member_and_set_order_do_not_change_identity() -> Result<()> {
    let root = fixture()?;
    fs::write(root.path().join("src/other.rs"), "other\n")?;
    commit(root.path())?;
    let mut original = spec();
    original
        .members
        .push(Member { path: "src/other.rs".to_owned(), roles: BTreeSet::from([Role::Kernel]) });
    original.toolchain.features = vec!["a".to_owned(), "b".to_owned()];
    original.bounds.assumptions.push("second assumption".to_owned());
    let before = evaluate(root.path(), &original)?;
    original.members.reverse();
    original.toolchain.features.reverse();
    original.bounds.assumptions.reverse();
    let after = evaluate(root.path(), &original)?;
    assert_eq!(before.closure.digest, after.closure.digest);
    Ok(())
}

#[test]
fn rollout_changes_do_not_mint_authority_or_stale_semantics() -> Result<()> {
    let root = fixture()?;
    let mut selection = spec();
    let before = evaluate(root.path(), &selection)?;
    selection.claim.rollout = RolloutState::RequiredConsumption;
    let after = evaluate(root.path(), &selection)?;
    let output = compare(&receipt(&before), Some(&after));
    assert_eq!(output.freshness, Freshness::Current);
    assert!(!output.proof_accepted);
    Ok(())
}

#[test]
fn declared_unknown_inputs_refuse_even_identical_receipts() -> Result<()> {
    let root = fixture()?;
    let mut selection = spec();
    selection.unknown_inputs.push("proc macro expansion not captured".to_owned());
    let current = evaluate(root.path(), &selection)?;
    assert_eq!(compare(&receipt(&current), Some(&current)).freshness, Freshness::Unknown);
    Ok(())
}

#[test]
fn forged_member_digest_is_not_freshness() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.closure.members.insert("invented".to_owned(), hash(b"fixture", &[b"forged"]));
    assert_eq!(compare(&old, Some(&current)).reason, "recorded_digest_does_not_match_members");
    Ok(())
}

#[test]
fn recomputed_forged_map_still_does_not_match_actual_source() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.closure.members.remove("source/src/kernel.rs");
    old.closure.digest = closure_digest(&old.closure.members)?;
    old.freshness.evaluated_closure_digest = old.closure.digest.clone();
    assert_eq!(compare(&old, Some(&current)).freshness, Freshness::Stale);
    Ok(())
}

#[test]
fn changed_algorithm_requires_requalification() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.closure.algorithm = "another_algorithm.v1".to_owned();
    assert_eq!(compare(&old, Some(&current)).reason, "algorithm_changed");
    Ok(())
}

#[test]
fn absent_current_source_is_unknown_not_retired() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    assert_eq!(compare(&receipt(&current), None).freshness, Freshness::Unknown);
    Ok(())
}

#[test]
fn explicit_terminal_history_does_not_need_current_source() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    for retired in [true, false] {
        let mut old = receipt(&current);
        old.disposition.reason = Some("replaced under owner issue".to_owned());
        old.freshness.reason = Some("historical evidence".to_owned());
        if retired {
            old.disposition.status = DispositionStatus::Retired;
            old.claim.rollout = RolloutState::Retired;
            old.freshness.status = FreshnessStatus::Retired;
        } else {
            old.disposition.status = DispositionStatus::Superseded;
            old.disposition.superseded_by = Some("FV-POSITION-LAWS-002".to_owned());
            old.freshness.status = FreshnessStatus::Superseded;
        }
        let output = compare(&old, None);
        assert_eq!(
            output.freshness,
            if retired { Freshness::Retired } else { Freshness::Superseded }
        );
        assert!(!output.proof_accepted);
        assert_eq!(old.native.result, NativeResult::Pass);
    }
    Ok(())
}

#[test]
fn currentness_never_upgrades_a_native_nonpass() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.native.result = NativeResult::NotProven;
    old.native.reason = Some(NonPassReason::Timeout);
    old.native.properties.clear();
    old.obligations.positive.observed = 0;
    old.obligations.positive.satisfied = 0;
    let output = compare(&old, Some(&current));
    assert_eq!(output.freshness, Freshness::Current);
    assert!(!output.proof_accepted);
    assert_eq!(old.native.result, NativeResult::NotProven);
    Ok(())
}

#[test]
fn source_binding_cannot_be_promoted_by_matching_hashes() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.claim.binding = ImplementationBinding::ModelOnly;
    old.correspondence.status = CorrespondenceStatus::NotApplicable;
    old.correspondence.controls = complete(&[]);
    old.correspondence.artifact_ids.clear();
    assert_eq!(compare(&old, Some(&current)).reason, "claim_or_binding_changed");
    Ok(())
}

#[test]
fn dirty_and_untracked_working_trees_are_refused() -> Result<()> {
    let root = fixture()?;
    fs::write(root.path().join("src/kernel.rs"), "changed")?;
    assert!(Snapshot::open(root.path()).is_err());
    snapshot::git(root.path(), &["checkout", "--", "src/kernel.rs"])?;
    fs::write(root.path().join("untracked"), "foreign")?;
    assert!(Snapshot::open(root.path()).is_err());
    Ok(())
}

#[test]
fn immutable_reads_and_changed_head_refusal() -> Result<()> {
    let root = fixture()?;
    let selected = Snapshot::open(root.path())?;
    let before = selected.read("src/kernel.rs")?.bytes;
    fs::write(root.path().join("src/kernel.rs"), "changed")?;
    assert_eq!(selected.read("src/kernel.rs")?.bytes, before);
    assert!(selected.finish().is_err());
    commit(root.path())?;
    assert!(selected.finish().is_err());
    Ok(())
}

#[test]
fn evaluator_source_mismatch_refuses_old_binary() -> Result<()> {
    let root = fixture()?;
    fs::write(root.path().join(ENGINE[0].0), "changed evaluator")?;
    commit(root.path())?;
    assert!(evaluate(root.path(), &spec()).is_err());
    Ok(())
}

#[test]
fn missing_declared_member_is_not_a_smaller_successful_closure() -> Result<()> {
    let root = fixture()?;
    fs::remove_file(root.path().join("src/kernel.rs"))?;
    commit(root.path())?;
    assert!(evaluate(root.path(), &spec()).is_err());
    Ok(())
}

#[test]
fn omitted_roles_duplicate_paths_and_blank_exclusions_are_refused() {
    let mut value = spec();
    value.exclusions.remove(&Role::Lockfile);
    assert!(normalize(&value).is_err());
    let mut value = spec();
    value.members.push(value.members[0].clone());
    assert!(normalize(&value).is_err());
    let mut value = spec();
    value.exclusions.insert(Role::Lockfile, " ".to_owned());
    assert!(normalize(&value).is_err());
    let mut value = spec();
    value.exclusions.insert(Role::Kernel, "contradicts member".to_owned());
    assert!(normalize(&value).is_err());
}

#[test]
fn generated_and_live_routes_require_their_extra_inputs() {
    let mut value = spec();
    value.claim.binding = ImplementationBinding::LiveConsumerBound;
    assert!(normalize(&value).is_err());
    let mut value = spec();
    value.exclusions.remove(&Role::GeneratedSource);
    value.members[0].roles.insert(Role::GeneratedSource);
    assert!(normalize(&value).is_err());
}

#[test]
fn path_aliases_and_git_metadata_are_not_semantic_sources() {
    for path in [
        "",
        "../outside",
        "a/../b",
        "/tmp/a",
        "C:/a",
        "a\\b",
        "a//b",
        "a/./b",
        "a/",
        ":(glob)*",
        "a*",
        ".git/config",
        "a/.GIT/file",
        "archive/old",
    ] {
        assert!(!snapshot::valid_path(path), "accepted {path}");
    }
    assert!(snapshot::valid_path(".ci/config.toml"));
}

#[cfg(unix)]
#[test]
fn committed_symlinks_are_not_followed() -> Result<()> {
    let root = fixture()?;
    std::os::unix::fs::symlink("kernel.rs", root.path().join("src/alias.rs"))?;
    commit(root.path())?;
    assert!(Snapshot::open(root.path())?.read("src/alias.rs").is_err());
    Ok(())
}

#[test]
fn duplicate_json_keys_unknown_fields_and_floats_are_refused() -> Result<()> {
    assert!(unique_json::parse(br#"{"a":1,"a":2}"#).is_err());
    assert!(unique_json::parse(br#"{"x":[{"a":1,"a":2}]}"#).is_err());
    assert!(unique_json::parse(br#"{"a":1.5}"#).is_err());
    let mut value = serde_json::to_value(spec())?;
    value["complete"] = true.into();
    assert!(parse_spec(&serde_json::to_vec(&value)?).is_err());
    Ok(())
}

#[test]
fn canonical_bytes_match_independent_literal_and_hash_vector() -> Result<()> {
    let value = serde_json::json!({"b": [2, 1], "a": {"z": true, "x": "é"}});
    assert_eq!(canonical(&value)?, "{\"a\":{\"x\":\"é\",\"z\":true},\"b\":[2,1]}".as_bytes());
    assert_ne!(hash(b"a", &[b"bc", b"d"]), hash(b"a", &[b"b", b"cd"]));
    assert_ne!(hash(b"a", &[b"b"]), hash(b"b", &[b"b"]));
    let members = BTreeMap::from([("input".to_owned(), format!("sha256:{}", "0".repeat(64)))]);
    assert_eq!(
        closure_digest(&members)?,
        "sha256:b24372de9c6be4736e37abd0a9d6c8dd04946a15acd7827913939705573bf739"
    );
    Ok(())
}

#[test]
fn each_semantic_source_role_participates_in_identity() -> Result<()> {
    let root = fixture()?;
    let mut selection = spec();
    selection.exclusions.clear();
    selection.members.clear();
    selection.claim.binding = ImplementationBinding::LiveConsumerBound;
    for (index, role) in ROLES.iter().enumerate() {
        let path = format!("src/member-{index}.rs");
        fs::write(root.path().join(&path), format!("role {index}\n"))?;
        selection.members.push(Member { path, roles: BTreeSet::from([*role]) });
    }
    commit(root.path())?;
    let before = evaluate(root.path(), &selection)?;
    let comparison = compare(&receipt(&before), Some(&before));
    assert!(comparison.correspondence_inputs_unchanged);
    assert!(!comparison.correspondence_established);
    for member in &selection.members {
        let path = root.path().join(&member.path);
        let original = fs::read(&path)?;
        fs::write(&path, b"changed input\n")?;
        commit(root.path())?;
        let changed = evaluate(root.path(), &selection)?;
        assert_eq!(compare(&receipt(&before), Some(&changed)).freshness, Freshness::Stale);
        fs::write(&path, original)?;
        commit(root.path())?;
    }
    let restored = evaluate(root.path(), &selection)?;
    assert_eq!(compare(&receipt(&before), Some(&restored)).freshness, Freshness::Current);
    Ok(())
}

#[test]
fn nonclean_historical_source_cannot_inherit_current_identity() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    for state in [SourceState::Dirty, SourceState::Unknown] {
        let mut old = receipt(&current);
        old.source.state = state;
        if state == SourceState::Dirty {
            old.source.evaluated_content_digest = Some(hash(b"fixture", &[b"dirty"]));
        }
        assert_eq!(compare(&old, Some(&current)).freshness, Freshness::Unknown);
    }
    Ok(())
}

#[test]
fn tampered_current_closure_is_not_accepted_by_library_api() -> Result<()> {
    let root = fixture()?;
    let mut current = evaluate(root.path(), &spec())?;
    let old = receipt(&current);
    current.closure.digest = hash(b"fixture", &[b"wrong"]);
    assert_eq!(compare(&old, Some(&current)).reason, "inconsistent_current_identity");
    Ok(())
}

#[test]
fn serialized_spec_roundtrip_preserves_identity_without_timestamp_fields() -> Result<()> {
    let root = fixture()?;
    let original = spec();
    let decoded = parse_spec(&serde_json::to_vec_pretty(&original)?)?;
    assert_eq!(
        evaluate(root.path(), &original)?.closure.digest,
        evaluate(root.path(), &decoded)?.closure.digest
    );
    let mut with_timestamp = serde_json::to_value(original)?;
    with_timestamp["recorded_at"] = "today".into();
    assert!(parse_spec(&serde_json::to_vec(&with_timestamp)?).is_err());
    Ok(())
}

#[test]
fn unknown_version_empty_domain_and_duplicate_features_are_refused() {
    let mut selection = spec();
    selection.schema_version = "formal_closure_spec.v2".to_owned();
    assert!(normalize(&selection).is_err());
    let mut selection = spec();
    selection.members.clear();
    assert!(normalize(&selection).is_err());
    let mut selection = spec();
    selection.toolchain.cfg.push("kani".to_owned());
    assert!(normalize(&selection).is_err());
}

#[test]
fn receipt_configuration_cannot_be_relabelled_around_an_unchanged_digest() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    old.bounds.values.insert("unwind".to_owned(), 99.into());
    assert_eq!(compare(&old, Some(&current)).reason, "receipt_configuration_changed");
    Ok(())
}

#[test]
fn retired_current_selection_cannot_masquerade_as_active_currentness() {
    let mut selection = spec();
    selection.claim.rollout = RolloutState::Retired;
    assert!(normalize(&selection).is_err());
}

#[cfg(unix)]
#[test]
fn executable_mode_change_is_semantic_even_with_identical_bytes() -> Result<()> {
    let root = fixture()?;
    let before = evaluate(root.path(), &spec())?;
    snapshot::git(root.path(), &["update-index", "--chmod=+x", "src/kernel.rs"])?;
    // Match the staged mode in the working tree rather than leave it dirty.
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(root.path().join("src/kernel.rs"), fs::Permissions::from_mode(0o755))?;
    commit(root.path())?;
    let after = evaluate(root.path(), &spec())?;
    assert_ne!(before.closure.digest, after.closure.digest);
    assert_eq!(compare(&receipt(&before), Some(&after)).freshness, Freshness::Stale);
    Ok(())
}

#[test]
fn comparison_is_bound_to_the_receipt_it_actually_compared() -> Result<()> {
    let root = fixture()?;
    let current = evaluate(root.path(), &spec())?;
    let mut old = receipt(&current);
    let before = compare(&old, Some(&current));
    old.artifacts[0].digest = hash(b"fixture", &[b"different-native-artifact"]);
    let after = compare(&old, Some(&current));
    assert_eq!(before.freshness, Freshness::Current);
    assert_eq!(after.freshness, Freshness::Current);
    assert!(before.compared_receipt_identity.is_some());
    assert_ne!(before.compared_receipt_identity, after.compared_receipt_identity);
    assert!(!after.proof_accepted);
    Ok(())
}
