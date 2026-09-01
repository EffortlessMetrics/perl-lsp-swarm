//! Human and machine projections of handoff results.
//!
//! `explain` exists because the dangerous failure mode of this format is not a
//! corrupt envelope but a confident misreading of a sound one. A valid handoff
//! proves that a candidate can be reconstructed; it never proves the candidate
//! was published, reviewed, or checked. Every projection says so explicitly.

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::check::{CheckReport, DimensionVerdict};
use super::model::{LimitationCode, MANIFEST_FILE_NAME, Manifest, RepositoryIdentityStatus};
use super::{HandoffOutcome, canonical_json};

/// Statements this format never makes, restated on every `explain`.
const AUTHORITY_BOUNDARY: &[&str] = &[
    "no branch or ref was created, moved, or published",
    "no pull request was opened, updated, or claimed",
    "no hosted check, review, or merge authority is implied",
    "carried proof is local proof, not a current GitHub check",
    "transported Git objects are carried, not audited for secrets",
];

/// What one envelope claims, in a form another context can consume.
#[derive(Debug, Serialize)]
pub struct ExplainDocument {
    /// Schema identity of the explanation.
    pub schema_version: &'static str,
    /// Candidate commit under transport.
    pub candidate_commit: String,
    /// Semantic identity digest.
    pub candidate_identity_digest: String,
    /// Repository identity status as a stable token.
    pub repository_identity: String,
    /// Number of ordered parents.
    pub parent_count: usize,
    /// Number of recomputable change rows.
    pub change_count: usize,
    /// Number of submodule references recorded but not transported.
    pub gitlink_count: usize,
    /// Number of transported objects.
    pub object_count: usize,
    /// Declared proof artifact identifiers.
    pub proof_ids: Vec<String>,
    /// Limitation codes carried by the manifest.
    pub limitations: Vec<LimitationCode>,
    /// Statements this envelope does not make.
    pub does_not_establish: Vec<&'static str>,
}

/// Read a manifest and describe it without validating the transport.
pub fn explain(envelope: &Path) -> Result<ExplainDocument, (HandoffOutcome, String)> {
    let bytes = fs::read(envelope.join(MANIFEST_FILE_NAME)).map_err(|error| {
        (
            HandoffOutcome::InvalidManifest,
            format!("`{MANIFEST_FILE_NAME}` is not readable: {error}"),
        )
    })?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|error| {
        (HandoffOutcome::InvalidManifest, format!("`{MANIFEST_FILE_NAME}` is not valid: {error}"))
    })?;
    Ok(describe(&manifest))
}

/// Describe an already-parsed manifest.
#[must_use]
pub fn describe(manifest: &Manifest) -> ExplainDocument {
    ExplainDocument {
        schema_version: "agent_candidate_handoff_explain.v1",
        candidate_commit: manifest.candidate.commit.clone(),
        candidate_identity_digest: manifest.candidate_identity_digest.clone(),
        repository_identity: match manifest.repository_identity.status {
            RepositoryIdentityStatus::NotProven => "not_proven".to_string(),
            RepositoryIdentityStatus::Observed | RepositoryIdentityStatus::Declared => manifest
                .repository_identity
                .value
                .clone()
                .unwrap_or_else(|| "not_proven".to_string()),
        },
        parent_count: manifest.candidate.parents.len(),
        change_count: manifest.inventory.changes.len(),
        gitlink_count: manifest.inventory.gitlinks.len(),
        object_count: manifest.transport.object_ids.len(),
        proof_ids: manifest.proof_references.iter().map(|proof| proof.id.clone()).collect(),
        limitations: manifest.limitations.clone(),
        does_not_establish: AUTHORITY_BOUNDARY.to_vec(),
    }
}

/// Stable human projection of an explanation.
#[must_use]
pub fn render_explain_human(document: &ExplainDocument) -> String {
    let mut lines = vec![
        format!("candidate: {}", document.candidate_commit),
        format!("identity: {}", document.candidate_identity_digest),
        format!("repository: {}", document.repository_identity),
        format!(
            "shape: {} parents, {} changes, {} gitlinks, {} objects",
            document.parent_count,
            document.change_count,
            document.gitlink_count,
            document.object_count
        ),
    ];
    if document.proof_ids.is_empty() {
        lines.push("proof: none carried".to_string());
    } else {
        lines.push(format!("proof: {}", document.proof_ids.join(", ")));
    }
    for limitation in &document.limitations {
        lines.push(format!("limitation: {}", limitation_token(*limitation)));
    }
    for statement in &document.does_not_establish {
        lines.push(format!("does-not-establish: {statement}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Stable human projection of a check report.
#[must_use]
pub fn render_check_human(report: &CheckReport) -> String {
    let mut lines = vec![
        format!("agent-candidate-handoff: {}", report.outcome.as_str()),
        format!("envelope: {}", report.envelope),
        format!("candidate: {}", report.candidate_commit.as_deref().unwrap_or("not_available")),
    ];
    for dimension in &report.dimensions {
        lines.push(format!(
            "{:<24} {:<14} {}",
            dimension.id,
            verdict_token(dimension.verdict),
            dimension.detail
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Render either projection as the caller requested.
pub fn render<T: Serialize>(value: &T, human: &str, as_json: bool) -> Result<String, String> {
    if as_json { canonical_json(value) } else { Ok(human.to_string()) }
}

const fn verdict_token(verdict: DimensionVerdict) -> &'static str {
    match verdict {
        DimensionVerdict::Valid => "valid",
        DimensionVerdict::Invalid => "invalid",
        DimensionVerdict::NotProven => "not_proven",
        DimensionVerdict::NotEvaluated => "not_evaluated",
    }
}

const fn limitation_token(limitation: LimitationCode) -> &'static str {
    match limitation {
        LimitationCode::LocalProofOnly => "local_proof_only",
        LimitationCode::TransportedObjectsNotSecretScanned => {
            "transported_objects_not_secret_scanned"
        }
        LimitationCode::TransportBytesNotVersionStable => "transport_bytes_not_version_stable",
        LimitationCode::RepositoryIdentityNotProven => "repository_identity_not_proven",
        LimitationCode::RemoteUrlContainedCredentials => "remote_url_contained_credentials",
        LimitationCode::SubmoduleGitlinkNotTransported => "submodule_gitlink_not_transported",
        LimitationCode::RootCommitDiffAgainstEmptyTree => "root_commit_diff_against_empty_tree",
        LimitationCode::MergeCommitDiffAgainstFirstParent => {
            "merge_commit_diff_against_first_parent"
        }
    }
}
