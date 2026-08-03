//! Typed synthetic squash-integration evidence for #4556.
//!
//! This module records the three identities involved in a synthetic
//! integration proof separately. It does not read GitHub, mutate branches, or
//! authorize a merge.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntheticSquashInput {
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub observation: SyntheticObservation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyntheticObservation {
    Success,
    Failure,
    Missing,
    Skipped,
    Cancelled,
    InstrumentFailure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyntheticVerdict {
    Success,
    Failure,
    NotProven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntheticSquashReceipt {
    pub schema_version: String,
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub observation: SyntheticObservation,
    pub verdict: SyntheticVerdict,
    pub findings: Vec<String>,
}

pub fn evaluate_synthetic_squash(input: SyntheticSquashInput) -> SyntheticSquashReceipt {
    let mut findings = Vec::new();
    for (label, identity) in [
        ("PR head", input.pr_head.as_str()),
        ("integration basis", input.integration_basis.as_str()),
        ("synthetic tree", input.synthetic_tree.as_str()),
    ] {
        let identity = identity.trim();
        if identity.is_empty() {
            findings.push(format!("{label} identity is missing"));
        } else if !is_git_object_id(identity) {
            findings
                .push(format!("{label} identity must be a 40-character hexadecimal Git object ID"));
        }
    }

    if matches!(
        input.observation,
        SyntheticObservation::Missing
            | SyntheticObservation::Skipped
            | SyntheticObservation::Cancelled
            | SyntheticObservation::InstrumentFailure
    ) {
        findings.push(format!("synthetic observation is {:?}", input.observation));
    }

    let verdict = if !findings.is_empty() {
        SyntheticVerdict::NotProven
    } else {
        match input.observation {
            SyntheticObservation::Success => SyntheticVerdict::Success,
            SyntheticObservation::Failure => SyntheticVerdict::Failure,
            SyntheticObservation::Missing
            | SyntheticObservation::Skipped
            | SyntheticObservation::Cancelled
            | SyntheticObservation::InstrumentFailure => SyntheticVerdict::NotProven,
        }
    };

    SyntheticSquashReceipt {
        schema_version: "synthetic-squash.v1".to_string(),
        pr_head: input.pr_head,
        integration_basis: input.integration_basis,
        synthetic_tree: input.synthetic_tree,
        observation: input.observation,
        verdict,
        findings,
    }
}

fn is_git_object_id(identity: &str) -> bool {
    identity.len() == 40 && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(observation: SyntheticObservation) -> SyntheticSquashInput {
        SyntheticSquashInput {
            pr_head: "0123456789abcdef0123456789abcdef01234567".to_string(),
            integration_basis: "89abcdef0123456789abcdef0123456789abcdef".to_string(),
            synthetic_tree: "fedcba9876543210fedcba9876543210fedcba98".to_string(),
            observation,
        }
    }

    #[test]
    fn success_preserves_separate_integration_identities() {
        let receipt = evaluate_synthetic_squash(input(SyntheticObservation::Success));
        assert_eq!(receipt.verdict, SyntheticVerdict::Success);
        assert_eq!(receipt.pr_head, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(receipt.integration_basis, "89abcdef0123456789abcdef0123456789abcdef");
        assert_eq!(receipt.synthetic_tree, "fedcba9876543210fedcba9876543210fedcba98");
    }

    #[test]
    fn observed_failure_is_distinct_from_not_proven() {
        let receipt = evaluate_synthetic_squash(input(SyntheticObservation::Failure));
        assert_eq!(receipt.verdict, SyntheticVerdict::Failure);
        assert!(receipt.findings.is_empty());
    }

    #[test]
    fn incomplete_observations_are_not_proven() {
        for observation in [
            SyntheticObservation::Missing,
            SyntheticObservation::Skipped,
            SyntheticObservation::Cancelled,
            SyntheticObservation::InstrumentFailure,
        ] {
            let receipt = evaluate_synthetic_squash(input(observation));
            assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
            assert!(!receipt.findings.is_empty());
        }
    }

    #[test]
    fn missing_identity_overrides_success_observation() {
        let mut input = input(SyntheticObservation::Success);
        input.integration_basis.clear();
        let receipt = evaluate_synthetic_squash(input);
        assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
        assert!(receipt.findings.iter().any(|finding| finding.contains("integration basis")));
    }

    #[test]
    fn malformed_identity_and_incomplete_observation_are_both_reported() {
        let mut input = input(SyntheticObservation::Skipped);
        input.synthetic_tree = "placeholder".to_string();
        let receipt = evaluate_synthetic_squash(input);
        assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
        assert!(receipt.findings.iter().any(|finding| finding.contains("synthetic tree")));
        assert!(receipt.findings.iter().any(|finding| finding.contains("synthetic observation")));
    }

    #[test]
    fn verdict_serializes_with_established_evidence_tokens() -> serde_json::Result<()> {
        let not_proven = serde_json::to_string(&SyntheticVerdict::NotProven)?;
        let failure = serde_json::to_string(&SyntheticVerdict::Failure)?;
        assert_eq!(not_proven, "\"NOT_PROVEN\"");
        assert_eq!(failure, "\"FAILURE\"");
        Ok(())
    }
}
