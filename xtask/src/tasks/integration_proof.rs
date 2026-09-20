//! Bounded synthetic integration-proof caller for #4589.
//!
//! This caller consumes an already evaluated #4588 trigger packet, constructs
//! one synthetic squash tree, and runs only the proof commands supplied by an
//! existing authority. It does not discover GitHub state, select commands,
//! mutate contributor branches, or authorize a merge.

use super::command_evidence::{self, ProofSetCommand, ProofSetItem, ResultClass};
use super::integration_trigger::{
    IntegrationTriggerPacket, IntegrationTriggerResult, SCHEMA_VERSION as TRIGGER_SCHEMA,
};
use super::merge_integration::{SyntheticSquashRequest, with_synthetic_squash};
use color_eyre::eyre::{Context, Report, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "integration-proof.v1";

#[derive(Debug, Clone, Deserialize)]
pub struct IntegrationProofSpec {
    pub schema: String,
    pub repository_path: PathBuf,
    pub trigger: IntegrationTriggerPacket,
    pub commands: Vec<ProofSetCommand>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationProofResult {
    NotRequired,
    Success,
    Failure,
    Blocked,
    NotProven,
    ReturnToReview,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationProofReceipt {
    pub schema: &'static str,
    pub trigger_schema: String,
    pub repository: String,
    pub pr: u64,
    pub pr_head_sha: String,
    pub reviewed_head_sha: String,
    pub pr_base_sha: String,
    pub integration_basis_sha: String,
    pub synthetic_tree_sha: Option<String>,
    pub trigger_result: IntegrationTriggerResult,
    pub trigger_findings: Vec<super::integration_trigger::IntegrationTriggerFinding>,
    pub proof_selection: Option<super::integration_trigger::ProofPackSelection>,
    pub selected_commands: Vec<String>,
    pub command_evidence: Vec<ProofSetItem>,
    pub result: IntegrationProofResult,
    pub findings: Vec<String>,
    pub next_action: String,
}

pub fn run(spec: IntegrationProofSpec) -> Result<IntegrationProofReceipt> {
    if spec.schema != SCHEMA_VERSION {
        bail!("unsupported integration-proof schema {:?}; expected {SCHEMA_VERSION}", spec.schema);
    }
    let trigger = spec.trigger;
    if trigger.schema != TRIGGER_SCHEMA {
        bail!("unsupported trigger schema {:?}; expected {TRIGGER_SCHEMA}", trigger.schema);
    }

    let selected_commands = spec.commands.iter().map(|command| command.id.clone()).collect();
    let mut receipt = IntegrationProofReceipt {
        schema: SCHEMA_VERSION,
        trigger_schema: trigger.schema.clone(),
        repository: trigger.repository.clone(),
        pr: trigger.pr,
        pr_head_sha: trigger.pr_head_sha.clone(),
        reviewed_head_sha: trigger.reviewed_head_sha.clone(),
        pr_base_sha: trigger.pr_base_sha.clone(),
        integration_basis_sha: trigger.current_integration_base_sha.clone(),
        synthetic_tree_sha: None,
        trigger_result: trigger.result,
        trigger_findings: trigger.triggers.clone(),
        proof_selection: trigger.proof_selection.clone(),
        selected_commands,
        command_evidence: Vec::new(),
        result: IntegrationProofResult::NotProven,
        findings: trigger.diagnostics.clone(),
        next_action: trigger.next_action.clone(),
    };

    match trigger.result {
        IntegrationTriggerResult::NotRequired => {
            receipt.result = IntegrationProofResult::NotRequired;
            receipt.next_action = "Leave the candidate on its PR-head proof path".to_string();
            return Ok(receipt);
        }
        IntegrationTriggerResult::ReturnToReview => {
            receipt.result = IntegrationProofResult::ReturnToReview;
            receipt.next_action =
                "Return the changed PR head to review before integration proof".to_string();
            return Ok(receipt);
        }
        IntegrationTriggerResult::Required => {}
        IntegrationTriggerResult::Blocked => {
            receipt.result = IntegrationProofResult::Blocked;
            receipt.next_action =
                "Resolve the blocked integration trigger before invoking proof".to_string();
            return Ok(receipt);
        }
        IntegrationTriggerResult::NotProven => {
            receipt.next_action =
                "Resolve trigger evidence before invoking integration proof".to_string();
            return Ok(receipt);
        }
    }

    if !proof_selection_is_complete(trigger.proof_selection.as_ref()) {
        receipt
            .findings
            .push("required trigger is missing a complete proof-pack selection".to_string());
        receipt.result = IntegrationProofResult::NotProven;
        receipt.next_action =
            "Provide the existing bounded proof-pack selection before invoking integration proof"
                .to_string();
        return Ok(receipt);
    }

    if let Err(error) = validate_commands(&spec.commands, &trigger.pr_head_sha) {
        receipt.findings.push(error.to_string());
        receipt.result = IntegrationProofResult::NotProven;
        receipt.next_action =
            "Provide complete selected commands with the reviewed PR head identity".to_string();
        return Ok(receipt);
    }

    let evidence = with_synthetic_squash(
        SyntheticSquashRequest {
            repository: &spec.repository_path,
            pr_base: &trigger.pr_base_sha,
            pr_head: &trigger.pr_head_sha,
            integration_basis: &trigger.current_integration_base_sha,
        },
        |construction, worktree| {
            receipt.synthetic_tree_sha = Some(construction.synthetic_tree.clone());
            command_evidence::run_proof_commands_in_dir(spec.commands, worktree)
        },
    );
    let evidence = match evidence {
        Ok(evidence) => evidence,
        Err(error) => {
            let message = error.to_string();
            receipt.findings.push(message);
            receipt.result = if is_patch_application_failure(&error) {
                IntegrationProofResult::Blocked
            } else {
                IntegrationProofResult::NotProven
            };
            receipt.next_action = if receipt.result == IntegrationProofResult::Blocked {
                "Resolve the synthetic integration conflict before rerunning proof".to_string()
            } else {
                "Repair the selected command evidence before rerunning integration proof"
                    .to_string()
            };
            return Ok(receipt);
        }
    };
    receipt.command_evidence = evidence.commands;
    receipt.result = match evidence.result {
        ResultClass::Success => IntegrationProofResult::Success,
        ResultClass::Failure => IntegrationProofResult::Failure,
        _ => IntegrationProofResult::NotProven,
    };
    receipt.next_action = match receipt.result {
        IntegrationProofResult::Success => {
            "Retain the integration receipt with the candidate evidence".to_string()
        }
        IntegrationProofResult::Failure => {
            "Route the observed proof failure to the affected seam".to_string()
        }
        _ => {
            "Resolve incomplete command evidence before treating integration as proven".to_string()
        }
    };
    Ok(receipt)
}

fn is_patch_application_failure(error: &Report) -> bool {
    error.chain().any(|cause| cause.to_string().starts_with("git apply failed with "))
}

pub fn run_from_file(spec_path: &Path, receipt_path: &Path) -> Result<()> {
    let raw = fs::read_to_string(spec_path)
        .with_context(|| format!("reading integration proof spec {}", spec_path.display()))?;
    let spec: IntegrationProofSpec = serde_json::from_str(&raw)
        .with_context(|| format!("parsing integration proof spec {}", spec_path.display()))?;
    let receipt = run(spec)?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing integration proof receipt {}", receipt_path.display()))?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    if matches!(
        receipt.result,
        IntegrationProofResult::Success | IntegrationProofResult::NotRequired
    ) {
        Ok(())
    } else {
        bail!("integration proof result: {:?}", receipt.result)
    }
}

fn proof_selection_is_complete(
    selection: Option<&super::integration_trigger::ProofPackSelection>,
) -> bool {
    selection.is_some_and(|selection| {
        !selection.class.trim().is_empty()
            && !selection.pack_ids.is_empty()
            && selection.pack_ids.iter().all(|pack| !pack.trim().is_empty())
            && !selection.reasons.is_empty()
            && selection.reasons.iter().all(|reason| !reason.trim().is_empty())
    })
}

fn validate_commands(commands: &[ProofSetCommand], candidate: &str) -> Result<()> {
    if commands.is_empty() {
        bail!("required integration proof must supply at least one selected command");
    }
    for command in commands {
        if command.candidate.as_deref() != Some(candidate) {
            bail!("proof command {:?} must carry the reviewed PR head identity", command.id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::integration_trigger::{
        IntegrationTriggerFinding, ProofPackSelection, TriggerKind,
    };
    use color_eyre::eyre::eyre;
    use std::process::Command;
    use tempfile::tempdir;

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
    const BASE: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

    fn trigger(result: IntegrationTriggerResult) -> IntegrationTriggerPacket {
        IntegrationTriggerPacket {
            schema: TRIGGER_SCHEMA.to_string(),
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            pr: 4589,
            pr_head_sha: HEAD.to_string(),
            reviewed_head_sha: HEAD.to_string(),
            pr_base_sha: BASE.to_string(),
            current_integration_base_sha: OTHER.to_string(),
            triggers: vec![IntegrationTriggerFinding {
                kind: TriggerKind::SamePublicSymbolSurface,
                detail: "same export".to_string(),
                references: vec!["src/lib.rs".to_string()],
            }],
            diagnostics: Vec::new(),
            proof_selection: Some(ProofPackSelection {
                class: "focused".to_string(),
                pack_ids: vec!["pack".to_string()],
                reasons: vec!["semantic interaction".to_string()],
            }),
            result,
            next_action: "run".to_string(),
        }
    }

    fn command(candidate: &str) -> ProofSetCommand {
        ProofSetCommand {
            id: "git-version".to_string(),
            program: "git".to_string(),
            args: vec!["--version".to_string()],
            cwd: None,
            candidate: Some(candidate.to_string()),
            timeout_secs: Some(10),
            out_dir: None,
        }
    }

    #[test]
    fn non_required_trigger_does_not_construct_or_run_commands() -> Result<()> {
        let spec = IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: PathBuf::from("does-not-exist"),
            trigger: trigger(IntegrationTriggerResult::NotRequired),
            commands: Vec::new(),
        };
        let receipt = run(spec)?;
        assert_eq!(receipt.result, IntegrationProofResult::NotRequired);
        assert!(receipt.synthetic_tree_sha.is_none());
        Ok(())
    }

    #[test]
    fn changed_head_is_returned_to_review() -> Result<()> {
        let mut packet = trigger(IntegrationTriggerResult::Required);
        packet.reviewed_head_sha = BASE.to_string();
        packet.result = IntegrationTriggerResult::ReturnToReview;
        let receipt = run(IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: PathBuf::from("does-not-exist"),
            trigger: packet,
            commands: vec![command(HEAD)],
        })?;
        assert_eq!(receipt.result, IntegrationProofResult::ReturnToReview);
        assert!(receipt.synthetic_tree_sha.is_none());
        Ok(())
    }

    #[test]
    fn blocked_trigger_remains_blocked() -> Result<()> {
        let receipt = run(IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: PathBuf::from("does-not-exist"),
            trigger: trigger(IntegrationTriggerResult::Blocked),
            commands: Vec::new(),
        })?;
        assert_eq!(receipt.result, IntegrationProofResult::Blocked);
        Ok(())
    }

    #[test]
    fn wrapped_patch_failure_remains_blocked() {
        let error = eyre!("git apply failed with conflict").wrap_err("applying the PR net patch");
        assert!(is_patch_application_failure(&error));
    }

    #[test]
    fn missing_command_identity_is_not_proven() -> Result<()> {
        let spec = IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: PathBuf::from("does-not-exist"),
            trigger: trigger(IntegrationTriggerResult::Required),
            commands: vec![command("different-head")],
        };
        let receipt = run(spec)?;
        assert_eq!(receipt.result, IntegrationProofResult::NotProven);
        assert!(receipt.synthetic_tree_sha.is_none());
        assert!(receipt.findings.iter().any(|finding| finding.contains("reviewed PR head")));
        Ok(())
    }

    #[test]
    fn required_trigger_without_selection_is_not_proven() -> Result<()> {
        let mut packet = trigger(IntegrationTriggerResult::Required);
        packet.proof_selection = None;
        let receipt = run(IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: PathBuf::from("does-not-exist"),
            trigger: packet,
            commands: vec![command(HEAD)],
        })?;
        assert_eq!(receipt.result, IntegrationProofResult::NotProven);
        assert!(receipt.synthetic_tree_sha.is_none());
        assert!(receipt.findings.iter().any(|finding| finding.contains("proof-pack selection")));
        Ok(())
    }

    #[test]
    fn required_trigger_links_synthetic_tree_and_command_evidence() -> Result<()> {
        let scratch = tempdir()?;
        Command::new("git").args(["init", "--quiet"]).current_dir(&scratch).output()?;
        let repository = scratch.path();
        Command::new("git")
            .args(["config", "user.email", "test@example.invalid"])
            .current_dir(repository)
            .output()?;
        Command::new("git")
            .args(["config", "user.name", "integration-test"])
            .current_dir(repository)
            .output()?;
        fs::write(repository.join("base.txt"), "base\n")?;
        Command::new("git").args(["add", "."]).current_dir(repository).output()?;
        Command::new("git")
            .args(["commit", "--quiet", "-m", "base"])
            .current_dir(repository)
            .output()?;
        let base = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repository)
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        fs::write(repository.join("candidate.txt"), "candidate\n")?;
        Command::new("git").args(["add", "."]).current_dir(repository).output()?;
        Command::new("git")
            .args(["commit", "--quiet", "-m", "candidate"])
            .current_dir(repository)
            .output()?;
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repository)
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let mut packet = trigger(IntegrationTriggerResult::Required);
        packet.pr_base_sha = base.clone();
        packet.pr_head_sha = head.clone();
        packet.reviewed_head_sha = head.clone();
        packet.current_integration_base_sha = base;
        let receipt = run(IntegrationProofSpec {
            schema: SCHEMA_VERSION.to_string(),
            repository_path: repository.to_path_buf(),
            trigger: packet,
            commands: vec![command(&head)],
        })?;
        assert_eq!(receipt.result, IntegrationProofResult::Success);
        assert!(receipt.synthetic_tree_sha.is_some());
        assert_eq!(receipt.command_evidence.len(), 1);
        assert_eq!(receipt.command_evidence[0].receipt.result, ResultClass::Success);
        assert_eq!(
            Path::new(&receipt.command_evidence[0].receipt.cwd)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("integration"),
            "proof commands must execute inside the synthetic worktree"
        );
        Ok(())
    }
}
