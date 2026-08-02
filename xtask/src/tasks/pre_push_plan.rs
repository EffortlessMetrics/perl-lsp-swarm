//! Pure, serial pre-push proof planning for #5446.

use std::path::Path;

use color_eyre::eyre::{ContextCompat, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::tasks::ci_scope;
use crate::utils::project_root;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrePushProofPlan {
    pub schema: &'static str,
    pub base: String,
    pub head: String,
    pub base_sha: String,
    pub head_sha: String,
    pub change_set_digest: String,
    pub changed_paths: Vec<String>,
    pub affected_packages: Vec<String>,
    pub reverse_dependents: Vec<String>,
    pub selected: Vec<ProofStep>,
    pub deferred: Vec<ProofStep>,
    pub posture: &'static str,
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
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut posture = "PROCEED";

    if is_typescript_only(&changed_paths) {
        selected.extend([
            ProofStep {
                class: "extension_format".to_string(),
                command: "cd vscode-extension && npm run fmt:check".to_string(),
                reason: "TypeScript-only change uses the extension formatter".to_string(),
            },
            ProofStep {
                class: "extension_lint".to_string(),
                command: "cd vscode-extension && npm run lint".to_string(),
                reason: "TypeScript-only change uses the extension lint budget".to_string(),
            },
            ProofStep {
                class: "extension_typecheck".to_string(),
                command: "cd vscode-extension && npm run typecheck:all".to_string(),
                reason: "TypeScript-only change requires extension type coverage".to_string(),
            },
            ProofStep {
                class: "extension_tests".to_string(),
                command: "cd vscode-extension && npm run test:ci".to_string(),
                reason: "TypeScript-only change requires focused extension tests".to_string(),
            },
        ]);
    } else {
        match diff_class.as_str() {
            "prose_only" => selected.push(ProofStep {
                class: "docs_check".to_string(),
                command: "cargo xtask ci-hygiene check-doc-paths docs".to_string(),
                reason: "prose-only change has no Rust compile floor".to_string(),
            }),
            "ci_config" => {
                selected.push(ProofStep {
                    class: "workflow_policy".to_string(),
                    command: "cargo xtask workflow-policy-lint".to_string(),
                    reason: "CI/configuration paths require policy validation".to_string(),
                });
                deferred.push(ProofStep {
                    class: "workspace_rust".to_string(),
                    command: "cargo test --workspace --locked".to_string(),
                    reason:
                        "workspace Rust proof is not the default floor for workflow-only changes"
                            .to_string(),
                });
            }
            "code" | "docs_as_code" => {
                if affected_packages.is_empty() {
                    posture = "BROAD_FALLBACK";
                    selected.push(ProofStep {
                        class: "broad_fallback".to_string(),
                        command: "cargo xtask pr-fast".to_string(),
                        reason: "code-like change did not map to an affected package".to_string(),
                    });
                } else {
                    let packages = affected_packages.join(",");
                    selected.push(ProofStep {
                        class: "rust_check".to_string(),
                        command: format!("cargo check -p {packages} --locked"),
                        reason: "directly affected package set".to_string(),
                    });
                    selected.push(ProofStep {
                        class: "focused_clippy".to_string(),
                        command: format!("cargo clippy -p {packages} --locked -- -D warnings"),
                        reason: "directly affected package set".to_string(),
                    });
                    selected.push(ProofStep {
                        class: "focused_tests".to_string(),
                        command: format!("cargo test -p {packages} --all-targets --locked"),
                        reason: "directly affected package set".to_string(),
                    });
                    deferred.push(ProofStep {
                        class: "full_reverse_closure".to_string(),
                        command: "cargo test --workspace --locked".to_string(),
                        reason: "broader closure is deferred to hosted/integration proof"
                            .to_string(),
                    });
                }
            }
            "mixed" => {
                posture = "BROAD_FALLBACK";
                selected.push(ProofStep {
                    class: "broad_fallback".to_string(),
                    command: "cargo xtask pr-fast".to_string(),
                    reason: "mixed surfaces cannot be safely narrowed by this bounded planner"
                        .to_string(),
                });
            }
            _ => {
                posture = "NOT_PROVEN";
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
            let bounded = reverse_dependents.iter().take(3).cloned().collect::<Vec<_>>();
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
        affected_packages,
        reverse_dependents,
        selected,
        deferred,
        posture,
    }
}

fn is_typescript_only(paths: &[String]) -> bool {
    !paths.is_empty()
        && paths.iter().all(|path| {
            path.starts_with("vscode-extension/")
                && (path.ends_with(".ts") || path.ends_with(".tsx"))
        })
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

    #[test]
    fn docs_are_not_given_a_rust_compile_floor() {
        let plan = build_plan(PlannerInput {
            base: "main".to_string(),
            head: "head".to_string(),
            base_sha: "base-sha".to_string(),
            head_sha: "head-sha".to_string(),
            changed_paths: vec!["docs/guide.md".to_string()],
            diff_class: "prose_only".to_string(),
            affected_packages: Vec::new(),
            reverse_dependents: Vec::new(),
            risk_tags: Vec::new(),
        });
        assert_eq!(plan.posture, "PROCEED");
        assert_eq!(plan.selected[0].class, "docs_check");
        assert!(plan.deferred.is_empty());
    }

    #[test]
    fn unknown_or_mixed_inputs_use_conservative_fallback() {
        let plan = build_plan(PlannerInput {
            base: "main".to_string(),
            head: "head".to_string(),
            base_sha: "base-sha".to_string(),
            head_sha: "head-sha".to_string(),
            changed_paths: vec!["unknown".to_string()],
            diff_class: "mixed".to_string(),
            affected_packages: Vec::new(),
            reverse_dependents: Vec::new(),
            risk_tags: Vec::new(),
        });
        assert_eq!(plan.posture, "BROAD_FALLBACK");
        assert_eq!(plan.selected[0].class, "broad_fallback");
    }

    #[test]
    fn extension_sources_use_extension_proof_without_rust_steps() {
        let plan = build_plan(PlannerInput {
            base: "main".to_string(),
            head: "head".to_string(),
            base_sha: "base-sha".to_string(),
            head_sha: "head-sha".to_string(),
            changed_paths: vec!["vscode-extension/src/extension.ts".to_string()],
            diff_class: "code".to_string(),
            affected_packages: Vec::new(),
            reverse_dependents: Vec::new(),
            risk_tags: Vec::new(),
        });
        assert_eq!(plan.posture, "PROCEED");
        assert!(plan.selected.iter().any(|step| step.class == "extension_format"));
        assert!(plan.selected.iter().any(|step| step.class == "extension_lint"));
        assert!(plan.selected.iter().any(|step| step.class == "extension_typecheck"));
        assert!(!plan.selected.iter().any(|step| step.class == "rust_check"));
    }

    #[test]
    fn digest_is_deterministic_for_ordered_paths() {
        let paths = vec!["a".to_string(), "b".to_string()];
        assert_eq!(digest("base", "head", &paths), digest("base", "head", &paths));
    }
}
