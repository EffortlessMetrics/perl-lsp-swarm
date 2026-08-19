//! Read-only semantic interaction trigger evaluation for #4588.
//!
//! The evaluator answers whether one selected candidate needs combined-tree
//! proof. It consumes source-bound evidence and an existing proof-pack
//! selection from the repository's affected/risk authorities. It does not
//! discover GitHub state, compare sibling lanes, or invent another command
//! matrix.

// The `integration-trigger.v1` evaluator is complete and covered by its own
// tests, but no production caller has landed yet, so every item reads as dead
// to the bin target. Deleting a versioned schema and its evaluator to satisfy
// dead_code would drop the contract, not dead code. Remove this when #4588's
// consumer wires it in.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "integration-trigger.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationTriggerInput {
    pub repository: String,
    pub pr: u64,
    pub pr_head_sha: String,
    pub reviewed_head_sha: String,
    pub pr_base_sha: String,
    pub integration_base_sha: String,
    /// False means an authority needed for the decision was unavailable.
    pub source_evidence_complete: bool,
    pub evidence: Vec<InteractionEvidence>,
    /// Selection produced by an existing affected/risk authority. The
    /// evaluator validates and carries it; it does not create commands.
    pub proof_selection: Option<ProofPackSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractionEvidence {
    pub kind: TriggerKind,
    pub detail: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    TextualConflict,
    SamePublicSymbolSurface,
    GeneratedAuthority,
    WorkflowPolicy,
    DependencyFeatureClosure,
    ParserLexerContract,
    StackedPrerequisite,
    ControllingRiskPolicy,
    MergeGroupRequired,
    BehindOnly,
    AgeOnly,
    CommitDistanceOnly,
    UnrelatedChange,
    SameFileIndependent,
    AuthorityUnavailable,
}

impl TriggerKind {
    fn is_required(self) -> bool {
        !matches!(
            self,
            Self::BehindOnly
                | Self::AgeOnly
                | Self::CommitDistanceOnly
                | Self::UnrelatedChange
                | Self::SameFileIndependent
                | Self::AuthorityUnavailable
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofPackSelection {
    pub class: String,
    pub pack_ids: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationTriggerResult {
    NotRequired,
    Required,
    Blocked,
    NotProven,
    ReturnToReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationTriggerFinding {
    pub kind: TriggerKind,
    pub detail: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationTriggerPacket {
    pub schema: String,
    pub repository: String,
    pub pr: u64,
    pub pr_head_sha: String,
    pub reviewed_head_sha: String,
    pub pr_base_sha: String,
    pub current_integration_base_sha: String,
    pub triggers: Vec<IntegrationTriggerFinding>,
    pub diagnostics: Vec<String>,
    pub proof_selection: Option<ProofPackSelection>,
    pub result: IntegrationTriggerResult,
    pub next_action: String,
}

/// Evaluate one candidate against one integration basis.
///
/// This is deliberately a pure reduction over evidence collected by other
/// authorities. Empty or incomplete authority input is `NOT_PROVEN`; it is
/// never treated as a clean non-trigger result.
pub fn evaluate(input: IntegrationTriggerInput) -> IntegrationTriggerPacket {
    let IntegrationTriggerInput {
        repository,
        pr,
        pr_head_sha,
        reviewed_head_sha,
        pr_base_sha,
        integration_base_sha,
        source_evidence_complete,
        evidence,
        proof_selection,
    } = input;
    let mut diagnostics = Vec::new();
    let mut findings = Vec::new();
    let mut result = IntegrationTriggerResult::NotRequired;
    let mut next_action = "No combined-tree proof selected".to_string();
    let evidence_missing = evidence.is_empty();

    if repository.trim().is_empty() {
        diagnostics.push("repository identity is missing".to_string());
    }
    if pr == 0 {
        diagnostics.push("PR number must be positive".to_string());
    }
    for (label, identity) in [
        ("PR head", pr_head_sha.as_str()),
        ("reviewed head", reviewed_head_sha.as_str()),
        ("PR base", pr_base_sha.as_str()),
        ("integration base", integration_base_sha.as_str()),
    ] {
        if !is_full_sha(identity) {
            diagnostics
                .push(format!("{label} identity must be a 40-character hexadecimal Git object ID"));
        }
    }
    let has_identity_diagnostics = !diagnostics.is_empty();
    let mut has_evidence_metadata_diagnostics = false;

    for evidence in &evidence {
        if evidence.detail.trim().is_empty() {
            diagnostics.push(format!("{:?} evidence detail is missing", evidence.kind));
            has_evidence_metadata_diagnostics = true;
        }
        if evidence.references.is_empty()
            || evidence.references.iter().all(|reference| reference.trim().is_empty())
        {
            diagnostics.push(format!("{:?} evidence references are missing", evidence.kind));
            has_evidence_metadata_diagnostics = true;
        }
        if evidence.kind.is_required() || evidence.kind == TriggerKind::AuthorityUnavailable {
            findings.push(IntegrationTriggerFinding {
                kind: evidence.kind,
                detail: evidence.detail.clone(),
                references: evidence.references.clone(),
            });
        }
    }
    let has_actual_trigger = findings.iter().any(|finding| finding.kind.is_required());
    let has_textual_conflict =
        findings.iter().any(|finding| finding.kind == TriggerKind::TextualConflict);
    let has_unavailable_authority =
        findings.iter().any(|finding| finding.kind == TriggerKind::AuthorityUnavailable);

    if !diagnostics.is_empty() {
        result = IntegrationTriggerResult::NotProven;
        next_action = if has_identity_diagnostics {
            "Resolve invalid candidate identities before deciding".to_string()
        } else if has_evidence_metadata_diagnostics {
            "Repair missing interaction evidence metadata before deciding".to_string()
        } else {
            "Resolve invalid trigger input before deciding".to_string()
        };
    } else if pr_head_sha != reviewed_head_sha {
        result = IntegrationTriggerResult::ReturnToReview;
        next_action =
            "PR head moved; return changed seams to review and required proof".to_string();
    } else if !source_evidence_complete {
        result = IntegrationTriggerResult::NotProven;
        next_action = "Obtain the missing source or ownership evidence before deciding".to_string();
    } else if has_unavailable_authority {
        result = IntegrationTriggerResult::NotProven;
        next_action = "Resolve unavailable authority evidence before selecting proof".to_string();
    } else if evidence_missing {
        result = IntegrationTriggerResult::NotProven;
        next_action = "Obtain source interaction evidence before deciding".to_string();
    } else if has_textual_conflict {
        result = IntegrationTriggerResult::Blocked;
        next_action =
            "Resolve the textual conflict before selecting combined-tree proof".to_string();
    } else if has_actual_trigger {
        match proof_selection.as_ref() {
            Some(selection) if selection_is_complete(selection) => {
                result = IntegrationTriggerResult::Required;
                next_action = "Run only the selected bounded integration proof pack".to_string();
            }
            _ => {
                result = IntegrationTriggerResult::NotProven;
                next_action =
                    "Provide an existing bounded proof-pack selection with reasons".to_string();
            }
        }
    }

    IntegrationTriggerPacket {
        schema: SCHEMA_VERSION.to_string(),
        repository,
        pr,
        pr_head_sha,
        reviewed_head_sha,
        pr_base_sha,
        current_integration_base_sha: integration_base_sha,
        triggers: findings,
        diagnostics,
        proof_selection,
        result,
        next_action,
    }
}

fn selection_is_complete(selection: &ProofPackSelection) -> bool {
    !selection.class.trim().is_empty()
        && !selection.pack_ids.is_empty()
        && selection.pack_ids.iter().all(|pack| !pack.trim().is_empty())
        && !selection.reasons.is_empty()
        && selection.reasons.iter().all(|reason| !reason.trim().is_empty())
}

fn is_full_sha(identity: &str) -> bool {
    identity.len() == 40 && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
    const BASE: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

    fn input(evidence: Vec<InteractionEvidence>) -> IntegrationTriggerInput {
        IntegrationTriggerInput {
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            pr: 4588,
            pr_head_sha: HEAD.to_string(),
            reviewed_head_sha: HEAD.to_string(),
            pr_base_sha: BASE.to_string(),
            integration_base_sha: OTHER.to_string(),
            source_evidence_complete: true,
            evidence,
            proof_selection: None,
        }
    }

    fn evidence(kind: TriggerKind) -> InteractionEvidence {
        InteractionEvidence {
            kind,
            detail: format!("evidence for {kind:?}"),
            references: vec!["crates/example/src/lib.rs".to_string()],
        }
    }

    fn selection() -> ProofPackSelection {
        ProofPackSelection {
            class: "rust_public_api".to_string(),
            pack_ids: vec!["affected-proof-rust-public-api".to_string()],
            reasons: vec!["same public symbol surface".to_string()],
        }
    }

    #[test]
    fn behind_only_and_same_file_independence_are_not_triggers() {
        let packet = evaluate(input(vec![
            evidence(TriggerKind::BehindOnly),
            evidence(TriggerKind::SameFileIndependent),
        ]));
        assert_eq!(packet.result, IntegrationTriggerResult::NotRequired);
        assert!(packet.triggers.is_empty());
    }

    #[test]
    fn semantic_collision_requires_existing_selection() {
        let mut input = input(vec![evidence(TriggerKind::SamePublicSymbolSurface)]);
        assert_eq!(evaluate(input.clone()).result, IntegrationTriggerResult::NotProven);

        input.proof_selection = Some(selection());
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::Required);
        assert_eq!(packet.proof_selection, Some(selection()));
    }

    #[test]
    fn unavailable_authority_never_becomes_clean() {
        let packet = evaluate(input(vec![evidence(TriggerKind::AuthorityUnavailable)]));
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.next_action.contains("unavailable"));
    }

    #[test]
    fn changed_head_returns_to_review_before_integration_selection() {
        let mut input = input(vec![evidence(TriggerKind::SamePublicSymbolSurface)]);
        input.reviewed_head_sha = BASE.to_string();
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::ReturnToReview);
        assert!(packet.next_action.contains("return"));
    }

    #[test]
    fn incomplete_source_evidence_is_not_proven() {
        let mut input = input(Vec::new());
        input.source_evidence_complete = false;
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.next_action.contains("missing"));
    }

    #[test]
    fn empty_authority_evidence_is_not_proven() {
        let packet = evaluate(input(Vec::new()));
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.next_action.contains("interaction evidence"));
    }

    #[test]
    fn invalid_identity_is_not_proven() {
        let mut input = input(Vec::new());
        input.integration_base_sha = "placeholder".to_string();
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(!packet.diagnostics.is_empty());
    }

    #[test]
    fn zero_pr_is_not_proven() {
        let mut input = input(Vec::new());
        input.pr = 0;
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.diagnostics.iter().any(|diagnostic| diagnostic.contains("PR number")));
    }

    #[test]
    fn evidence_without_source_metadata_is_not_proven() {
        let mut item = evidence(TriggerKind::SamePublicSymbolSurface);
        item.detail.clear();
        item.references.clear();
        let mut input = input(vec![item]);
        input.proof_selection = Some(selection());
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.diagnostics.iter().any(|diagnostic| diagnostic.contains("detail")));
        assert!(packet.diagnostics.iter().any(|diagnostic| diagnostic.contains("references")));
        assert!(packet.next_action.contains("evidence metadata"));
    }

    #[test]
    fn textual_conflict_blocks_before_proof_selection() {
        let mut input = input(vec![evidence(TriggerKind::TextualConflict)]);
        input.proof_selection = Some(selection());
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::Blocked);
        assert!(packet.next_action.contains("textual conflict"));
    }

    #[test]
    fn invalid_identity_remains_primary_over_incomplete_evidence() {
        let mut input = input(vec![evidence(TriggerKind::SamePublicSymbolSurface)]);
        input.integration_base_sha = "placeholder".to_string();
        input.source_evidence_complete = false;
        let packet = evaluate(input);
        assert_eq!(packet.result, IntegrationTriggerResult::NotProven);
        assert!(packet.next_action.contains("candidate identities"));
    }

    #[test]
    fn result_tokens_are_stable() -> serde_json::Result<()> {
        assert_eq!(serde_json::to_string(&IntegrationTriggerResult::NotProven)?, "\"NOT_PROVEN\"");
        assert_eq!(
            serde_json::to_string(&TriggerKind::SamePublicSymbolSurface)?,
            "\"same_public_symbol_surface\""
        );
        Ok(())
    }
}
