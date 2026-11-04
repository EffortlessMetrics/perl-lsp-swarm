//! Actual classifier-to-planner regressions for #17825. No injected fallback flags.

use super::*;

#[path = "../../../tests/git_test_support.rs"]
mod git_test_support;
use git_test_support::HermeticGit;

const UNOWNED_PORTFOLIO: &[&str] = &[
    "fmt",
    "ignored_tests_check_refs",
    "check_conflict_markers",
    "release_history",
    "readme_heading_check",
    "publish_closure",
    "publish_manifest_check",
    "layer_check",
    "published_crate_count_pr_fast",
    "serial_test_ratchet",
    "release_history_check",
    "source_commit_api_check",
    "perl_token_leaf_contract",
    "compile_all_targets",
    "clippy_full",
    "clippy_tests_kernel",
];

const FALLBACK_PORTFOLIO: &[&str] = &[
    "fmt",
    "ignored_tests_check_refs",
    "check_conflict_markers",
    "release_history",
    "readme_heading_check",
    "publish_closure",
    "publish_manifest_check",
    "layer_check",
    "published_crate_count_pr_fast",
    "serial_test_ratchet",
    "release_history_check",
    "source_commit_api_check",
    "inline_completion_registration",
    "lsp_registration_contract",
    "lsp_capability_snapshots",
    "inline_completion_core",
    "inline_completion_quality_receipt",
    "clippy_core",
    "unit_core",
    "perl_token_leaf_contract",
    "compile_all_targets",
    "clippy_full",
    "clippy_tests_kernel",
    "ci_subject_digest_oracle",
    "unit_control_plane_bins",
];

fn policy() -> Result<(PathBuf, Vec<GateDefinition>)> {
    let root = crate::utils::project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    Ok((root, gates_for_tier(&policy, "pr_fast")))
}

fn metadata() -> serde_json::Value {
    serde_json::json!({
        "packages": [{"name": "xtask", "id": "xtask 0.1.0",
            "manifest_path": "/workspace/xtask/Cargo.toml"}],
        "resolve": {"nodes": [{"id": "xtask 0.1.0", "deps": []}]}
    })
}

fn classify_and_plan(files: &[&str]) -> Result<GatePlan> {
    let (root, gates) = policy()?;
    let files: Vec<String> = files.iter().map(|file| (*file).to_string()).collect();
    let scope = ci_scope::classify_files(&files, &metadata(), "/workspace")?;
    plan_pr_fast_gates(&root, gates, "HEAD".to_string(), Some(scope))
}

fn assert_portfolio(plan: &GatePlan, expected: &[&str]) {
    let selected: Vec<&str> = plan.selected.iter().map(|gate| gate.gate.name.as_str()).collect();
    assert_eq!(selected, expected);
    assert!(plan.selected.iter().all(|gate| gate.gate.required), "required proof was weakened");
}

#[test]
fn pr_fast_non_rust_composition_preserves_literal_policy_portfolios() -> Result<()> {
    for files in [
        vec!["scripts/agent-preflight.sh"],
        vec!["scripts/tests/test-pre-merge-status.py"],
        vec!["README.md"],
        vec!["README.md", "scripts/agent-preflight.sh"],
        vec!["scripts/agent-preflight.sh", "README.md"],
        vec!["scripts/agent-preflight.sh", "README.md", "docs/agents/DEVELOPMENT_METHOD.md"],
        vec!["README.md", "scripts/tests/test-pre-merge-status.py", "scripts/agent-preflight.sh"],
    ] {
        let plan = classify_and_plan(&files)?;
        assert_portfolio(&plan, UNOWNED_PORTFOLIO);
        assert!(plan.scope_ok, "scope acquisition failed: {files:?}");
        assert!(!plan.fallback_used, "unnecessary Rust fallback: {files:?}");
        assert!(plan.package_args.is_empty());
    }
    Ok(())
}

#[test]
fn pr_fast_non_rust_composition_retains_rust_and_unknown_authority() -> Result<()> {
    for retained in [
        "src/unowned.rs",
        "scripts/unowned.rs",
        "scripts/Cargo.toml",
        "scripts/rust-toolchain.toml",
        "policy/unknown.json",
        ".ci/unknown-authority.yaml",
        "docs/unknown-authority.json",
        "unknown/behavior.py",
        "scripts/unknown-language.bin",
        "scripts/extensionless-authority",
    ] {
        let plan = classify_and_plan(&["README.md", "scripts/agent-preflight.sh", retained])?;
        assert_portfolio(&plan, FALLBACK_PORTFOLIO);
        assert!(plan.scope_ok);
        assert!(plan.fallback_used, "lost conservative fallback: {retained}");
    }
    Ok(())
}

#[test]
fn pr_fast_non_rust_composition_retains_registered_rust_consumers() -> Result<()> {
    for files in [
        vec!["scripts/ci/classify-ripr-lane-termination"],
        vec!["scripts/ci/classify-ripr-lane-termination", "README.md"],
        vec!["xtask/src/main.rs", "README.md", "scripts/agent-preflight.sh"],
        vec!["xtask/README.md", "scripts/agent-preflight.sh"],
    ] {
        let plan = classify_and_plan(&files)?;
        assert!(!plan.fallback_used, "registered owner should use scoped proof: {files:?}");
        assert_eq!(plan.package_args, vec!["-p", "xtask"]);
        let routed = plan
            .selected
            .iter()
            .find(|gate| gate.gate.name == "unit_routed_full")
            .ok_or_else(|| eyre!("lost registered owner's integration proof: {files:?}"))?;
        assert!(routed.gate.command.contains("--tests -p xtask"));
        assert!(routed.gate.required);
        assert!(plan.selected.iter().any(|gate| gate.gate.name == "unit_control_plane_bins"));
        assert!(!plan.selected.iter().any(|gate| gate.gate.name == "clippy_core"));
    }
    Ok(())
}

#[test]
fn pr_fast_non_rust_composition_rejects_incomplete_package_metadata() -> Result<()> {
    let files = vec!["README.md".to_string(), "scripts/agent-preflight.sh".to_string()];
    let valid_package = metadata()["packages"][0].clone();
    for invalid in [
        serde_json::json!(null),
        serde_json::json!({}),
        serde_json::json!({"packages": null}),
        serde_json::json!({"packages": "unknown"}),
        serde_json::json!({"packages": []}),
        serde_json::json!({"packages": [null]}),
        serde_json::json!({"packages": [{}]}),
        serde_json::json!({"packages": [valid_package.clone(), {}]}),
        serde_json::json!({"packages": [{}, valid_package]}),
    ] {
        assert!(
            ci_scope::classify_files(&files, &invalid, "/workspace").is_err(),
            "missing metadata must not prove unowned composition: {invalid}"
        );
    }
    for field in ["name", "id", "manifest_path"] {
        for value in [serde_json::json!(null), serde_json::json!(42), serde_json::json!(" ")] {
            let mut invalid = metadata();
            invalid["packages"][0][field] = value;
            assert!(
                ci_scope::classify_files(&files, &invalid, "/workspace").is_err(),
                "incomplete {field} must not prove unowned composition: {invalid}"
            );
        }
        let mut invalid = metadata();
        invalid["packages"][0]
            .as_object_mut()
            .ok_or_else(|| eyre!("fixture package must be an object"))?
            .remove(field);
        assert!(ci_scope::classify_files(&files, &invalid, "/workspace").is_err());
    }
    let mut valid = metadata();
    valid["resolve"] = serde_json::Value::Null;
    let scope = ci_scope::classify_files(&files, &valid, "/workspace")?;
    let (root, gates) = policy()?;
    let plan = plan_pr_fast_gates(&root, gates, "HEAD".to_string(), Some(scope))?;
    assert_portfolio(&plan, UNOWNED_PORTFOLIO);
    assert!(!plan.fallback_used, "valid --no-deps metadata must remain admissible");
    Ok(())
}

#[test]
fn pr_fast_non_rust_composition_retains_failed_target_metadata() -> Result<()> {
    let (_, gates) = policy()?;
    let scope = ci_scope::classify_files(
        &["scripts/ci/classify-ripr-lane-termination".to_string(), "README.md".to_string()],
        &metadata(),
        "/workspace",
    )?;
    let missing_workspace = tempfile::tempdir()?;
    let plan =
        plan_pr_fast_gates(missing_workspace.path(), gates, "HEAD".to_string(), Some(scope))?;
    assert_portfolio(&plan, FALLBACK_PORTFOLIO);
    assert!(plan.fallback_used);
    assert!(plan.fallback_reason.as_deref().is_some_and(|reason| reason.contains("metadata")));
    Ok(())
}

fn fixture_git(git: &HermeticGit, root: &Path, args: &[&str]) -> Result<String> {
    git.git(root, args).map_err(|error| eyre!("{error:#}"))
}

fn repository_fixture() -> Result<(tempfile::TempDir, HermeticGit, PathBuf)> {
    let temp = tempfile::tempdir()?;
    let git = HermeticGit::at(&temp.path().join("pins")).map_err(|error| eyre!("{error:#}"))?;
    let root = temp.path().join("repo");
    git.init_repo(&root).map_err(|error| eyre!("{error:#}"))?;
    fs::write(root.join("README.md"), "before\n")?;
    fixture_git(&git, &root, &["add", "README.md"])?;
    fixture_git(&git, &root, &["commit", "-m", "base"])?;
    Ok((temp, git, root))
}

#[test]
fn pr_fast_non_rust_composition_retains_failed_scope_metadata() -> Result<()> {
    let (_, gates) = policy()?;
    let (_temp, git, root) = repository_fixture()?;
    let base = fixture_git(&git, &root, &["rev-parse", "HEAD"])?;
    fs::create_dir_all(root.join("scripts"))?;
    fs::write(root.join("scripts/fixture.py"), "print('fixture')\n")?;
    fs::write(root.join("README.md"), "after\n")?;
    fixture_git(&git, &root, &["add", "."])?;
    fixture_git(&git, &root, &["commit", "-m", "known composition without Cargo metadata"])?;
    // Real acquisition reaches cargo metadata, which must fail (no manifest).
    let plan = plan_pr_fast_gates(&root, gates, base, None)?;
    assert_portfolio(&plan, FALLBACK_PORTFOLIO);
    assert!(!plan.scope_ok);
    assert!(plan.fallback_used);
    Ok(())
}

#[test]
fn pr_fast_non_rust_composition_retains_git_rename_and_delete_inputs() -> Result<()> {
    for operation in ["rename", "delete"] {
        let (_, gates) = policy()?;
        let (_temp, git, root) = repository_fixture()?;
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("scripts"))?;
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = 'fixture'\nversion = '0.1.0'\nedition = '2024'\n[lib]\npath = 'placeholder.rs'\n",
        )?;
        fs::write(root.join("placeholder.rs"), "")?;
        fs::write(root.join("src/legacy.rs"), "// unchanged source identity\n")?;
        fixture_git(&git, &root, &["add", "."])?;
        fixture_git(&git, &root, &["commit", "-m", "Rust input"])?;
        let base = fixture_git(&git, &root, &["rev-parse", "HEAD"])?;
        if operation == "rename" {
            fixture_git(&git, &root, &["mv", "src/legacy.rs", "scripts/legacy.py"])?;
        } else {
            fixture_git(&git, &root, &["rm", "src/legacy.rs"])?;
        }
        fs::write(root.join("README.md"), "after\n")?;
        fixture_git(&git, &root, &["add", "."])?;
        fixture_git(&git, &root, &["commit", "-m", operation])?;
        for mode in [change_set::DiffMode::MergeBaseThreeDot, change_set::DiffMode::DirectTwoDot] {
            let resolved = change_set::resolve_change_set_with_mode(
                ArtifactIdentity::CommitRange { base: base.clone(), head: "HEAD".to_string() },
                &root,
                mode,
            )?;
            let expected = if operation == "rename" {
                vec!["README.md", "scripts/legacy.py", "src/legacy.rs"]
            } else {
                vec!["README.md", "src/legacy.rs"]
            };
            assert_eq!(resolved.changed_paths, expected, "lost old Rust authority: {mode:?}");
        }
        let plan = plan_pr_fast_gates(&root, gates, base, None)?;
        assert_portfolio(&plan, FALLBACK_PORTFOLIO);
        assert!(plan.scope_ok);
        assert!(plan.fallback_used, "lost Rust {operation} proof");
    }
    Ok(())
}
