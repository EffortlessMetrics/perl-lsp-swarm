//! Selection contracts only: planning must never execute a product build.
use super::*;

fn plan_paths(paths: &[&str]) -> Result<GatePlan> {
    let root = crate::utils::project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    // Independent topology oracle: xtask has no reverse dependency on the LSP.
    let metadata = serde_json::json!({
        "packages": [
            {"name": "xtask", "id": "xtask", "manifest_path": "/fixture/xtask/Cargo.toml"},
            {"name": "perl-lsp-rs", "id": "perl-lsp-rs",
             "manifest_path": "/fixture/crates/perl-lsp-rs/Cargo.toml"}
        ],
        "resolve": {"nodes": [
            {"id": "xtask", "deps": []}, {"id": "perl-lsp-rs", "deps": []}
        ]}
    });
    let files = paths.iter().map(|path| (*path).to_string()).collect::<Vec<_>>();
    let scope = ci_scope::classify_files(&files, &metadata, "/fixture")?;
    plan_pr_fast_gates(&root, gates_for_tier(&policy, "pr_fast"), "HEAD".into(), Some(scope))
}

fn selected(plan: &GatePlan, name: &str) -> bool {
    plan.selected.iter().any(|entry| entry.gate.name == name)
}

#[test]
fn installer_only_skips_unrelated_product_compilation() -> Result<()> {
    let plan = plan_paths(&["scripts/install-githooks.sh"])?;
    assert!(!plan.fallback_used);
    assert!(!selected(&plan, "inline_completion_registration"));
    assert!(selected(&plan, "fmt"), "always-on proof must remain selected");
    Ok(())
}

#[test]
fn installer_with_prose_does_not_activate_product_fallback() -> Result<()> {
    let plan = plan_paths(&["scripts/install-githooks.sh", "README.md"])?;
    assert!(!plan.fallback_used, "adding prose must not turn installer work into Rust fallback");
    assert!(!selected(&plan, "inline_completion_registration"));
    assert!(selected(&plan, "fmt"));
    Ok(())
}

#[test]
fn installer_with_owned_policy_preserves_owner_integration_tests() -> Result<()> {
    let plan = plan_paths(&["scripts/install-githooks.sh", ".ci/gate-policy.yaml"])?;
    assert!(!plan.fallback_used);
    let owner = plan.selected.iter().find(|entry| entry.gate.name == "unit_routed_full");
    assert!(
        owner.is_some_and(
            |entry| entry.gate.required && entry.gate.command.contains("--tests -p xtask")
        ),
        "non-Rust policy must execute its owner's required integration tests"
    );
    assert!(!selected(&plan, "inline_completion_registration"));
    Ok(())
}

#[test]
fn installer_with_product_source_preserves_required_registration() -> Result<()> {
    let plan = plan_paths(&["scripts/install-githooks.sh", "crates/perl-lsp-rs/src/cli.rs"])?;
    let registration =
        plan.selected.iter().find(|entry| entry.gate.name == "inline_completion_registration");
    assert!(registration.is_some_and(|entry| entry.gate.required));
    assert!(!plan.fallback_used);
    Ok(())
}

#[test]
fn installer_with_unknown_code_retains_conservative_fallback() -> Result<()> {
    let plan = plan_paths(&["scripts/install-githooks.sh", "unowned.rs"])?;
    assert!(plan.fallback_used, "unknown code must not inherit the script exemption");
    assert!(selected(&plan, "inline_completion_registration"));
    Ok(())
}

#[test]
fn explicit_doctest_proof_remains_required() -> Result<()> {
    let root = crate::utils::project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    let proof = policy.gates.iter().find(|gate| gate.name == "doctest_contract_proof");
    let proof = proof.ok_or_else(|| eyre!("missing doctest proof"))?;
    assert!(proof.required && proof.tier == "merge_gate");
    let command = proof.command.split_whitespace().collect::<Vec<_>>();
    assert!(command.starts_with(&["cargo", "test"]));
    assert!(command.contains(&"--doc") && command.contains(&"--locked"));
    for package in ["perl-token", "perl-module", "perl-corpus", "perl-evidence-envelope", "xtask"] {
        assert!(
            command.windows(2).any(|pair| pair == ["-p", package]),
            "doctest package floor lost {package}"
        );
    }
    Ok(())
}
