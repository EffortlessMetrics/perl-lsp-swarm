//! Pure, serial pre-push proof planning for #5446.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use color_eyre::eyre::{ContextCompat, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::tasks::ci_scope;
use crate::tasks::git_context::git_stdout_with_worktree_fallback;
use crate::utils::project_root;

/// Maximum reverse dependents selected for local pre-push proof. The wider
/// closure is deferred to hosted proof; the source closure is sorted upstream,
/// so this bound selects a deterministic subset.
const MAX_BOUNDED_REVERSE_DEPENDENTS: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrePushProofPlan {
    pub schema: &'static str,
    pub base: String,
    pub head: String,
    pub base_sha: String,
    pub head_sha: String,
    pub change_set_digest: String,
    pub changed_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extension_change_classes: Vec<ExtensionChangeClass>,
    pub affected_packages: Vec<String>,
    pub reverse_dependents: Vec<String>,
    pub selected: Vec<ProofStep>,
    pub deferred: Vec<ProofStep>,
    pub posture: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionChangeClass {
    Source,
    Tests,
    Scripts,
    Dependency,
    Tsconfig,
    BundlePackage,
    Authoring,
    WorkflowAction,
    DocsOnly,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProofStep {
    pub class: String,
    pub command: String,
    pub reason: String,
}

pub fn run(base: String, head: String, format: String) -> Result<()> {
    let root = project_root()?;
    let plan = plan_for_repository(&root, &base, &head)?;
    match format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&plan)?),
        "human" => print_human(&plan),
        other => bail!("unsupported format {other:?}; expected human or json"),
    }
    Ok(())
}

fn plan_for_repository(root: &Path, base: &str, head: &str) -> Result<PrePushProofPlan> {
    let resolved = change_set::resolve_change_set(
        ArtifactIdentity::CommitRange { base: base.to_string(), head: head.to_string() },
        root,
    )?;
    let base_sha = resolved.base_sha.clone().context("change set did not resolve a base SHA")?;
    let head_sha = resolved.head_sha.clone().context("change set did not resolve a head SHA")?;
    let checked_out_head =
        git_stdout_with_worktree_fallback(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    if checked_out_head != head_sha {
        bail!("pre-push plan head {head_sha} is not the checked-out HEAD {checked_out_head}");
    }
    let metadata = ci_scope::load_metadata(root)?;
    let workspace_root = root.to_string_lossy().replace('\\', "/");
    let scope = ci_scope::classify_files(&resolved.changed_paths, &metadata, &workspace_root)?;
    let reverse_dependents =
        scope.reverse_dep_closure.iter().map(|crate_| crate_.name.clone()).collect();
    Ok(build_plan(PlannerInput {
        base: base.to_string(),
        head: head.to_string(),
        base_sha,
        head_sha,
        changed_paths: resolved.changed_paths,
        diff_class: scope.diff_class,
        affected_packages: scope.direct_crates.into_iter().map(|crate_| crate_.name).collect(),
        reverse_dependents,
        risk_tags: scope.risk_tags,
    }))
}

struct PlannerInput {
    base: String,
    head: String,
    base_sha: String,
    head_sha: String,
    changed_paths: Vec<String>,
    diff_class: String,
    affected_packages: Vec<String>,
    reverse_dependents: Vec<String>,
    risk_tags: Vec<String>,
}

fn build_plan(input: PlannerInput) -> PrePushProofPlan {
    let PlannerInput {
        base,
        head,
        base_sha,
        head_sha,
        changed_paths,
        diff_class,
        affected_packages,
        reverse_dependents,
        risk_tags,
    } = input;
    let change_set_digest = digest(&base_sha, &head_sha, &changed_paths);
    let extension_change_classes = classify_extension_paths(&changed_paths);
    let extension_only = !extension_change_classes.is_empty()
        && changed_paths.iter().all(|path| classify_extension_path(path).is_some());
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut posture = "PROCEED";

    add_extension_proof(
        &extension_change_classes,
        &changed_paths,
        &mut selected,
        &mut deferred,
        &mut posture,
    );

    if !extension_only {
        match diff_class.as_str() {
            "prose_only" => selected.push(ProofStep {
                class: "docs_check".to_string(),
                command: "cargo xtask ci-hygiene check-doc-paths docs".to_string(),
                reason: "prose-only change has no Rust compile floor".to_string(),
            }),
            "ci_config" => {
                if changed_paths.iter().any(|path| path.starts_with(".github/workflows/")) {
                    selected.push(ProofStep {
                        class: "workflow_policy".to_string(),
                        command: "cargo xtask workflow-policy-lint".to_string(),
                        reason: "workflow changes require policy validation".to_string(),
                    });
                } else {
                    escalate_posture(&mut posture, "BROAD_FALLBACK");
                    selected.push(ProofStep {
                        class: "broad_fallback".to_string(),
                        command: "cargo xtask gates --tier pr-fast --receipt".to_string(),
                        reason:
                            "non-workflow CI/configuration change requires the bounded fallback"
                                .to_string(),
                    });
                }
                deferred.push(ProofStep {
                    class: "workspace_rust".to_string(),
                    command: "cargo test --workspace --locked".to_string(),
                    reason: "workspace Rust proof is deferred to hosted/integration proof"
                        .to_string(),
                });
            }
            "code" | "docs_as_code" => {
                if affected_packages.is_empty() {
                    escalate_posture(&mut posture, "BROAD_FALLBACK");
                    selected.push(ProofStep {
                        class: "broad_fallback".to_string(),
                        command: "cargo xtask gates --tier pr-fast --receipt".to_string(),
                        reason: "code-like change did not map to an affected package".to_string(),
                    });
                } else {
                    add_rust_package_proof(&affected_packages, &mut selected, &mut deferred);
                }
            }
            "mixed" => {
                if affected_packages.is_empty() {
                    escalate_posture(&mut posture, "BROAD_FALLBACK");
                    selected.push(ProofStep {
                        class: "broad_fallback".to_string(),
                        command: "cargo xtask gates --tier pr-fast --receipt".to_string(),
                        reason: "mixed surfaces include code not mapped to an affected package"
                            .to_string(),
                    });
                } else {
                    add_rust_package_proof(&affected_packages, &mut selected, &mut deferred);
                }
            }
            _ => {
                escalate_posture(&mut posture, "NOT_PROVEN");
                selected.push(ProofStep {
                    class: "not_proven".to_string(),
                    command: "none".to_string(),
                    reason: format!("unsupported diff classification: {diff_class}"),
                });
            }
        }
    }

    if risk_tags.iter().any(|tag| tag == "public_api") {
        if !reverse_dependents.is_empty() {
            let bounded = reverse_dependents
                .iter()
                .take(MAX_BOUNDED_REVERSE_DEPENDENTS)
                .cloned()
                .collect::<Vec<_>>();
            selected.push(ProofStep {
                class: "reverse_dependent_tests".to_string(),
                command: format!(
                    "cargo test {} --all-targets --locked",
                    bounded.iter().map(|name| format!("-p {name}")).collect::<Vec<_>>().join(" ")
                ),
                reason: "bounded reverse dependents selected for a public/shared API change"
                    .to_string(),
            });
        }
        deferred.push(ProofStep {
            class: "reverse_dependents".to_string(),
            command: "cargo test --workspace --locked".to_string(),
            reason: "public/shared API may affect reverse dependents; broader closure is deferred"
                .to_string(),
        });
    }

    dedupe_proof_steps(&mut selected);
    dedupe_proof_steps(&mut deferred);
    selected
        .sort_by(|left, right| (&left.class, &left.command).cmp(&(&right.class, &right.command)));
    deferred
        .sort_by(|left, right| (&left.class, &left.command).cmp(&(&right.class, &right.command)));
    PrePushProofPlan {
        schema: "pre_push_proof_plan.v1",
        base,
        head,
        base_sha,
        head_sha,
        change_set_digest,
        changed_paths,
        extension_change_classes,
        affected_packages,
        reverse_dependents,
        selected,
        deferred,
        posture,
    }
}

fn add_extension_proof(
    classes: &[ExtensionChangeClass],
    changed_paths: &[String],
    selected: &mut Vec<ProofStep>,
    deferred: &mut Vec<ProofStep>,
    posture: &mut &'static str,
) {
    for class in classes {
        match class {
            ExtensionChangeClass::Source | ExtensionChangeClass::Tests => {
                push_step(
                    selected,
                    "extension_format",
                    "cd vscode-extension && npm run fmt:check",
                    "extension source and tests use the extension formatter",
                );
                push_step(
                    selected,
                    "extension_lint",
                    "cd vscode-extension && npm run lint",
                    "extension source and tests use the type-aware lint budget",
                );
                push_step(
                    selected,
                    "extension_typecheck",
                    "cd vscode-extension && npm run typecheck:all",
                    "extension source and tests require all TypeScript authority configs",
                );
                push_step(
                    selected,
                    "extension_tests",
                    "cd vscode-extension && npm run test:ci",
                    "extension source and tests require the canonical unit/receipt suite",
                );
                push_step(
                    deferred,
                    "extension_hosted_smoke",
                    "cd vscode-extension && npm run test:integration",
                    "Electron integration proof is deferred to the hosted extension lane",
                );
            }
            ExtensionChangeClass::Scripts => {
                push_step(
                    selected,
                    "extension_format",
                    "cd vscode-extension && npm run fmt:check",
                    "extension tooling scripts use the extension formatter",
                );
                push_step(
                    selected,
                    "extension_lint",
                    "cd vscode-extension && npm run lint",
                    "extension tooling scripts use the type-aware lint budget",
                );
                push_step(
                    selected,
                    "extension_script_typecheck",
                    "cd vscode-extension && npm run typecheck:scripts",
                    "script changes require the script TypeScript authority config",
                );
                push_step(
                    selected,
                    "extension_script_tests",
                    "cd vscode-extension && npm run test:receipt-summary",
                    "script changes require the repository-owned Node receipt tests",
                );
            }
            ExtensionChangeClass::Dependency => {
                push_step(
                    selected,
                    "extension_doctor",
                    "cd vscode-extension && npm run doctor",
                    "dependency changes must preserve the repository Node/npm authority",
                );
                push_step(
                    selected,
                    "extension_typescript_authority",
                    "cd vscode-extension && npm run typecheck:authority",
                    "dependency changes can alter the installed compiler identity",
                );
                push_step(
                    selected,
                    "extension_typecheck",
                    "cd vscode-extension && npm run typecheck:all",
                    "dependency changes require all TypeScript authority configs",
                );
                push_step(
                    selected,
                    "extension_tooling_tests",
                    "cd vscode-extension && npm run test:receipt-summary",
                    "dependency changes require tooling and receipt contract tests",
                );
                push_step(
                    deferred,
                    "extension_clean_install",
                    "cd vscode-extension && npm ci",
                    "clean installation is deferred so pre-push does not destroy the working tree",
                );
                push_step(
                    deferred,
                    "extension_package_smoke",
                    "cd vscode-extension && npm run verify:marketplace",
                    "full VSIX/package proof is deferred to hosted packaging",
                );
            }
            ExtensionChangeClass::Tsconfig => {
                push_step(
                    selected,
                    "extension_typescript_authority",
                    "cd vscode-extension && npm run typecheck:authority",
                    "tsconfig changes can alter the effective compiler boundary",
                );
                push_step(
                    selected,
                    "extension_typecheck",
                    "cd vscode-extension && npm run typecheck:all",
                    "every TypeScript authority config must remain blocking",
                );
                push_step(
                    selected,
                    "extension_config_tests",
                    "cd vscode-extension && npm run test:ci",
                    "tsconfig changes require effective-config contract tests",
                );
            }
            ExtensionChangeClass::BundlePackage => {
                push_step(
                    selected,
                    "extension_checked_build",
                    "cd vscode-extension && npm run typecheck:all && npm run compile",
                    "bundle/package changes require a checked production bundle",
                );
                push_step(
                    selected,
                    "extension_package_inventory",
                    "cd vscode-extension && npm run check:package-inventory",
                    "bundle/package changes must preserve the VSIX inventory ratchet",
                );
                push_step(
                    selected,
                    "extension_source_map",
                    "cd vscode-extension && npm run check:source-map",
                    "bundle changes must preserve exact source-map evidence",
                );
                push_step(
                    deferred,
                    "extension_package_smoke",
                    "cd vscode-extension && npm run verify:marketplace",
                    "full binary-backed VSIX proof is deferred to hosted packaging",
                );
            }
            ExtensionChangeClass::Authoring => {
                push_step(
                    selected,
                    "extension_typecheck",
                    "cd vscode-extension && npm run typecheck:all",
                    "authoring changes must still point at a type-clean extension",
                );
                push_step(
                    selected,
                    "extension_authoring_tests",
                    "cd vscode-extension && npm run test:ci",
                    "authoring configuration requires launch/task/package contract tests",
                );
                push_step(
                    deferred,
                    "extension_host_observation",
                    "manual Extension Development Host observation",
                    "interactive editor launch proof cannot be established by local pre-push",
                );
            }
            ExtensionChangeClass::WorkflowAction => {
                push_step(
                    selected,
                    "workflow_policy",
                    "cargo xtask workflow-policy-lint",
                    "extension workflow/action changes require workflow policy validation",
                );
                push_step(
                    selected,
                    "workflow_trigger",
                    "cargo xtask workflow-trigger-lint",
                    "extension workflow/action changes require trigger validation",
                );
                push_step(
                    selected,
                    "extension_typecheck",
                    "cd vscode-extension && npm run typecheck:all",
                    "toolchain workflow changes must preserve the affected extension proof",
                );
                push_step(
                    deferred,
                    "hosted_workflow_execution",
                    "GitHub Actions hosted workflow execution",
                    "workflow execution remains hosted evidence, not a local pass",
                );
            }
            ExtensionChangeClass::DocsOnly => {
                push_step(
                    selected,
                    "extension_docs",
                    "git diff --check",
                    "extension prose changes require bounded whitespace/path hygiene",
                );
            }
            ExtensionChangeClass::Unknown => {
                let unknown = changed_paths
                    .iter()
                    .filter(|path| {
                        classify_extension_path(path) == Some(ExtensionChangeClass::Unknown)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                push_step(
                    selected,
                    "extension_unknown",
                    "none",
                    &format!(
                        "extension tooling/config paths are unclassified and cannot be narrowed safely: {unknown}"
                    ),
                );
                escalate_posture(posture, "NOT_PROVEN");
            }
        }
    }
}

fn add_rust_package_proof(
    affected_packages: &[String],
    selected: &mut Vec<ProofStep>,
    deferred: &mut Vec<ProofStep>,
) {
    let package_flags = cargo_package_flags(affected_packages);
    selected.push(ProofStep {
        class: "rust_check".to_string(),
        command: format!("cargo check {package_flags} --locked"),
        reason: "directly affected package set".to_string(),
    });
    selected.push(ProofStep {
        class: "focused_clippy".to_string(),
        command: format!("cargo clippy {package_flags} --all-targets --locked -- -D warnings"),
        reason: "directly affected package set".to_string(),
    });
    selected.push(ProofStep {
        class: "focused_tests".to_string(),
        command: format!("cargo test {package_flags} --all-targets --locked"),
        reason: "directly affected package set".to_string(),
    });
    deferred.push(ProofStep {
        class: "full_reverse_closure".to_string(),
        command: "cargo test --workspace --locked".to_string(),
        reason: "broader closure is deferred to hosted/integration proof".to_string(),
    });
}

fn push_step(steps: &mut Vec<ProofStep>, class: &str, command: &str, reason: &str) {
    steps.push(ProofStep {
        class: class.to_string(),
        command: command.to_string(),
        reason: reason.to_string(),
    });
}

fn dedupe_proof_steps(steps: &mut Vec<ProofStep>) {
    let mut grouped: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for step in steps.drain(..) {
        let entry = grouped.entry(step.command).or_default();
        entry.0.insert(step.class);
        entry.1.insert(step.reason);
    }
    *steps = grouped
        .into_iter()
        .map(|(command, (classes, reasons))| ProofStep {
            class: classes.into_iter().collect::<Vec<_>>().join("+"),
            command,
            reason: reasons.into_iter().collect::<Vec<_>>().join("; "),
        })
        .collect();
}

fn escalate_posture(posture: &mut &'static str, candidate: &'static str) {
    fn rank(value: &str) -> u8 {
        match value {
            "PROCEED" => 0,
            "BROAD_FALLBACK" => 1,
            "NOT_PROVEN" => 2,
            _ => 3,
        }
    }
    if rank(candidate) > rank(posture) {
        *posture = candidate;
    }
}

fn classify_extension_paths(paths: &[String]) -> Vec<ExtensionChangeClass> {
    paths
        .iter()
        .filter_map(|path| classify_extension_path(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn classify_extension_path(path: &str) -> Option<ExtensionChangeClass> {
    let is_typescript = path.ends_with(".ts") || path.ends_with(".tsx");
    if path.starts_with("vscode-extension/src/test/") && is_typescript {
        return Some(ExtensionChangeClass::Tests);
    }
    if path.starts_with("vscode-extension/src/") && is_typescript {
        return Some(ExtensionChangeClass::Source);
    }
    if path.starts_with("vscode-extension/test/") {
        return Some(ExtensionChangeClass::Tests);
    }
    if matches!(
        path,
        "vscode-extension/package.json"
            | "vscode-extension/package-lock.json"
            | "vscode-extension/npm-shrinkwrap.json"
    ) {
        return Some(ExtensionChangeClass::Dependency);
    }
    if path.starts_with("vscode-extension/tsconfig") && path.ends_with(".json") {
        return Some(ExtensionChangeClass::Tsconfig);
    }
    if path.starts_with("vscode-extension/.vscode/") {
        return Some(ExtensionChangeClass::Authoring);
    }
    if path.starts_with(".github/actions/setup-vscode-toolchain/") || is_extension_workflow(path) {
        return Some(ExtensionChangeClass::WorkflowAction);
    }
    if path.starts_with("vscode-extension/docs/")
        || (path.starts_with("vscode-extension/") && path.ends_with(".md"))
    {
        return Some(ExtensionChangeClass::DocsOnly);
    }
    if is_extension_bundle_or_package_path(path) {
        return Some(ExtensionChangeClass::BundlePackage);
    }
    if path.starts_with("vscode-extension/scripts/")
        && (path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".mjs"))
    {
        return Some(ExtensionChangeClass::Scripts);
    }
    if path.starts_with("vscode-extension/") && looks_like_extension_tooling(path) {
        return Some(ExtensionChangeClass::Unknown);
    }
    None
}

fn is_extension_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/vscode-")
        || matches!(
            path,
            ".github/workflows/ux-regression-gate.yml" | ".github/workflows/publish-extension.yml"
        )
}

fn is_extension_bundle_or_package_path(path: &str) -> bool {
    path.starts_with("vscode-extension/media/")
        || path.starts_with("vscode-extension/syntaxes/")
        || matches!(
            path,
            "vscode-extension/.vscodeignore"
                | "vscode-extension/rolldown.config.mjs"
                | "vscode-extension/language-configuration.json"
                | "vscode-extension/gherkin-language-configuration.json"
                | "vscode-extension/scripts/vsix-inventory-baseline.json"
                | "vscode-extension/scripts/check-vsix-inventory.js"
                | "vscode-extension/scripts/check-source-map.js"
        )
}

fn looks_like_extension_tooling(path: &str) -> bool {
    [".js", ".cjs", ".mjs", ".json", ".yml", ".yaml", ".toml"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn cargo_package_flags(packages: &[String]) -> String {
    packages.iter().map(|package| format!("-p {package}")).collect::<Vec<_>>().join(" ")
}

fn digest(base_sha: &str, head_sha: &str, paths: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base_sha.as_bytes());
    hasher.update([0]);
    hasher.update(head_sha.as_bytes());
    for path in paths {
        hasher.update([0]);
        hasher.update(path.as_bytes());
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_human(plan: &PrePushProofPlan) {
    println!("pre-push proof plan ({})", plan.schema);
    println!("base: {} ({})", plan.base, plan.base_sha);
    println!("head: {} ({})", plan.head, plan.head_sha);
    println!("change-set digest: {}", plan.change_set_digest);
    println!("posture: {}", plan.posture);
    if !plan.extension_change_classes.is_empty() {
        println!("extension change classes: {:?}", plan.extension_change_classes);
    }
    println!("selected:");
    for step in &plan.selected {
        println!("  - {}: {} ({})", step.class, step.command, step.reason);
    }
    println!("deferred:");
    for step in &plan.deferred {
        println!("  - {}: {} ({})", step.class, step.command, step.reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for(
        paths: Vec<&str>,
        diff_class: &str,
        affected_packages: Vec<&str>,
    ) -> PrePushProofPlan {
        build_plan(PlannerInput {
            base: "main".to_string(),
            head: "head".to_string(),
            base_sha: "base-sha".to_string(),
            head_sha: "head-sha".to_string(),
            changed_paths: paths.into_iter().map(str::to_string).collect(),
            diff_class: diff_class.to_string(),
            affected_packages: affected_packages.into_iter().map(str::to_string).collect(),
            reverse_dependents: Vec::new(),
            risk_tags: Vec::new(),
        })
    }

    fn has_command(plan: &PrePushProofPlan, command: &str) -> bool {
        plan.selected.iter().any(|step| step.command == command)
    }

    fn defers_command(plan: &PrePushProofPlan, command: &str) -> bool {
        plan.deferred.iter().any(|step| step.command == command)
    }

    #[test]
    fn docs_are_not_given_a_rust_compile_floor() {
        let plan = plan_for(vec!["docs/guide.md"], "prose_only", Vec::new());
        assert_eq!(plan.posture, "PROCEED");
        assert_eq!(plan.selected[0].class, "docs_check");
        assert!(plan.deferred.is_empty());
    }

    #[test]
    fn unknown_or_mixed_inputs_use_conservative_fallback() {
        let plan = plan_for(vec!["unknown"], "mixed", Vec::new());
        assert_eq!(plan.posture, "BROAD_FALLBACK");
        assert!(has_command(&plan, "cargo xtask gates --tier pr-fast --receipt"));
        assert!(plan.extension_change_classes.is_empty());
    }

    #[test]
    fn extension_sources_use_extension_proof_without_rust_steps() {
        let plan = plan_for(vec!["vscode-extension/src/extension.ts"], "code", Vec::new());
        assert_eq!(plan.posture, "PROCEED");
        assert_eq!(plan.extension_change_classes, vec![ExtensionChangeClass::Source]);
        for command in [
            "cd vscode-extension && npm run fmt:check",
            "cd vscode-extension && npm run lint",
            "cd vscode-extension && npm run typecheck:all",
            "cd vscode-extension && npm run test:ci",
        ] {
            assert!(has_command(&plan, command), "missing {command}");
        }
        assert!(!plan.selected.iter().any(|step| step.class == "rust_check"));
    }

    #[test]
    fn dependency_change_selects_authority_and_defers_clean_install() {
        let plan = plan_for(vec!["vscode-extension/package-lock.json"], "code", Vec::new());
        assert_eq!(plan.posture, "PROCEED");
        for command in [
            "cd vscode-extension && npm run doctor",
            "cd vscode-extension && npm run typecheck:authority",
            "cd vscode-extension && npm run typecheck:all",
            "cd vscode-extension && npm run test:receipt-summary",
        ] {
            assert!(has_command(&plan, command), "missing {command}");
        }
        assert!(defers_command(&plan, "cd vscode-extension && npm ci"));
        assert!(!has_command(&plan, "cargo xtask gates --tier pr-fast --receipt"));
    }

    #[test]
    fn tsconfig_change_selects_effective_config_proof() {
        let plan = plan_for(vec!["vscode-extension/tsconfig.scripts.json"], "code", Vec::new());
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:authority"));
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:all"));
        assert!(has_command(&plan, "cd vscode-extension && npm run test:ci"));
    }

    #[test]
    fn bundle_change_selects_checked_build_and_package_contracts() {
        let plan = plan_for(vec!["vscode-extension/rolldown.config.mjs"], "code", Vec::new());
        assert!(has_command(
            &plan,
            "cd vscode-extension && npm run typecheck:all && npm run compile"
        ));
        assert!(has_command(&plan, "cd vscode-extension && npm run check:package-inventory"));
        assert!(defers_command(&plan, "cd vscode-extension && npm run verify:marketplace"));
    }

    #[test]
    fn authority_script_change_selects_script_proof() {
        let plan = plan_for(
            vec!["vscode-extension/scripts/check-typescript-authority.js"],
            "code",
            Vec::new(),
        );
        assert_eq!(plan.posture, "PROCEED");
        assert_eq!(plan.extension_change_classes, vec![ExtensionChangeClass::Scripts]);
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:scripts"));
        assert!(has_command(&plan, "cd vscode-extension && npm run test:receipt-summary"));
    }

    #[test]
    fn authoring_change_defers_manual_host_observation() {
        let plan = plan_for(vec!["vscode-extension/.vscode/launch.json"], "code", Vec::new());
        assert_eq!(plan.posture, "PROCEED");
        assert_eq!(plan.extension_change_classes, vec![ExtensionChangeClass::Authoring]);
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:all"));
        assert!(has_command(&plan, "cd vscode-extension && npm run test:ci"));
        assert!(defers_command(&plan, "manual Extension Development Host observation"));
    }

    #[test]
    fn bundle_scripts_are_not_generic_scripts() {
        for path in [
            "vscode-extension/scripts/check-source-map.js",
            "vscode-extension/scripts/check-vsix-inventory.js",
        ] {
            assert_eq!(
                classify_extension_path(path),
                Some(ExtensionChangeClass::BundlePackage),
                "{path} must classify as BundlePackage"
            );
        }
        assert_eq!(
            classify_extension_path("vscode-extension/scripts/lint-canary.js"),
            Some(ExtensionChangeClass::Scripts)
        );
    }

    #[test]
    fn setup_action_selects_workflow_and_extension_proof() {
        let plan = plan_for(
            vec![".github/actions/setup-vscode-toolchain/action.yml"],
            "ci_config",
            Vec::new(),
        );
        assert!(has_command(&plan, "cargo xtask workflow-policy-lint"));
        assert!(has_command(&plan, "cargo xtask workflow-trigger-lint"));
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:all"));
        assert!(defers_command(&plan, "GitHub Actions hosted workflow execution"));
    }

    #[test]
    fn extension_docs_remain_cheap() {
        let plan = plan_for(vec!["vscode-extension/docs/development.md"], "prose_only", Vec::new());
        assert_eq!(plan.selected.len(), 1);
        assert!(has_command(&plan, "git diff --check"));
        assert!(plan.deferred.is_empty());
    }

    #[test]
    fn unknown_extension_tooling_is_not_proven() {
        let plan = plan_for(vec!["vscode-extension/tooling/new.config.cjs"], "code", Vec::new());
        assert_eq!(plan.posture, "NOT_PROVEN");
        assert!(has_command(&plan, "none"));
        assert!(plan.selected.iter().any(|step| step.reason.contains("new.config.cjs")));
    }

    #[test]
    fn mixed_rust_and_extension_changes_preserve_both_proof_families() {
        let plan = plan_for(
            vec!["crates/perl-lsp-rs/src/lib.rs", "vscode-extension/package-lock.json"],
            "mixed",
            vec!["perl-lsp-rs"],
        );
        assert!(has_command(&plan, "cd vscode-extension && npm run typecheck:authority"));
        assert!(has_command(&plan, "cargo check -p perl-lsp-rs --locked"));
        assert!(has_command(
            &plan,
            "cargo clippy -p perl-lsp-rs --all-targets --locked -- -D warnings"
        ));
    }

    #[test]
    fn extension_paths_are_classified_semantically_and_deterministically() {
        let paths = vec![
            "vscode-extension/tooling/new.config.cjs".to_string(),
            ".github/actions/setup-vscode-toolchain/action.yml".to_string(),
            "vscode-extension/tsconfig.scripts.json".to_string(),
            "vscode-extension/scripts/check-typescript-authority.js".to_string(),
            "vscode-extension/package-lock.json".to_string(),
            "vscode-extension/src/test/smoke.test.ts".to_string(),
            "vscode-extension/src/extension.ts".to_string(),
        ];
        assert_eq!(
            classify_extension_paths(&paths),
            vec![
                ExtensionChangeClass::Source,
                ExtensionChangeClass::Tests,
                ExtensionChangeClass::Scripts,
                ExtensionChangeClass::Dependency,
                ExtensionChangeClass::Tsconfig,
                ExtensionChangeClass::WorkflowAction,
                ExtensionChangeClass::Unknown,
            ]
        );
    }

    #[test]
    fn duplicate_commands_are_collapsed_without_losing_reasons() {
        let plan = plan_for(
            vec!["vscode-extension/src/extension.ts", "vscode-extension/tsconfig.json"],
            "code",
            Vec::new(),
        );
        let steps: Vec<_> = plan
            .selected
            .iter()
            .filter(|step| step.command == "cd vscode-extension && npm run typecheck:all")
            .collect();
        assert_eq!(steps.len(), 1);
        let step = steps[0];
        assert!(step.reason.contains("source"));
        assert!(step.reason.contains("authority config"));
    }

    #[test]
    fn digest_is_deterministic_for_ordered_paths() {
        let paths = vec!["a".to_string(), "b".to_string()];
        let reordered = vec!["b".to_string(), "a".to_string()];
        assert_eq!(
            digest("base", "head", &paths),
            digest("base", "head", &paths),
            "identical inputs must produce one digest"
        );
        assert_ne!(
            digest("base", "head", &paths),
            digest("base", "head", &reordered),
            "path order must change the digest"
        );
        assert_ne!(
            digest("base", "head", &paths),
            digest("base", "other-head", &paths),
            "head SHA must change the digest"
        );
    }

    #[test]
    fn cargo_package_flags_repeat_package_option() {
        assert_eq!(cargo_package_flags(&["a".to_string(), "b".to_string()]), "-p a -p b");
    }
}
