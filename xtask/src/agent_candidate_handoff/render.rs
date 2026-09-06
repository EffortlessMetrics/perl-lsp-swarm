//! Human and machine projections of handoff results.
//!
//! `explain` exists because the dangerous failure mode of this format is not a
//! corrupt envelope but a confident misreading of a sound one. A valid handoff
//! proves that a candidate can be reconstructed; it never proves the candidate
//! was published, reviewed, or checked. Every projection says so explicitly.

use std::path::Path;

use serde::Serialize;

use super::check::{CheckReport, DimensionVerdict, MAX_DOCUMENT_BYTES, read_envelope_file};
use super::model::{LimitationCode, MANIFEST_FILE_NAME, Manifest, RepositoryIdentityStatus};
use super::{HandoffOutcome, canonical_json};

/// Statements this format never makes, restated on every `explain`.
const AUTHORITY_BOUNDARY: &[&str] = &[
    "no branch or ref was created, moved, or published",
    "no pull request was opened, updated, or claimed",
    "no hosted check, review, or merge authority is implied",
    "carried proof is local proof, not a current GitHub check",
    "transported Git objects are carried, not audited for secrets",
    // `explain` reads the manifest and stops. Every number and identity below
    // it is the envelope's own claim about itself, and a corrupt envelope
    // explains exactly as confidently as a sound one. Saying so here is the
    // difference between a projection and a verdict.
    "this projection reports claims; only `check` verifies them",
    "the repository identity is the producer's word; no receiver can check it",
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
    /// Strength of the repository claim, as a stable token.
    pub repository_identity_status: String,
    /// Lowercase `owner/name`, absent when no identity is proven.
    pub repository_identity_value: Option<String>,
    /// Hosting authority an observed identity was read from.
    ///
    /// Carried through the projection rather than dropped: `owner/name` names a
    /// different repository on every forge, so a consumer handed the bare pair
    /// could publish to the wrong one — which is the whole reason the manifest
    /// records a host at all.
    pub repository_identity_host: Option<String>,
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
///
/// `explain` skips transport verification, but it does not skip the reader's
/// bounds: it is pointed at the same untrusted envelopes `check` is, so it uses
/// the validator's own size-capped, symlink-refusing read. Reading the manifest
/// unboundedly here would give an oversized or link-bearing envelope a way in
/// through the projection that the validator closes.
pub fn explain(envelope: &Path) -> Result<ExplainDocument, (HandoffOutcome, String)> {
    let bytes = read_envelope_file(envelope, MANIFEST_FILE_NAME, MAX_DOCUMENT_BYTES)
        .map_err(|error| (error.outcome, error.detail))?;
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
        // Status is reported as status, never collapsed into the value.
        // `Observed` and `Declared` are different claims — one was read from a
        // remote, the other was typed by the caller and verified by nobody —
        // and a projection that renders them identically invites a consumer to
        // act on a guess as though it were an observation.
        repository_identity_status: match manifest.repository_identity.status {
            RepositoryIdentityStatus::NotProven => "not_proven",
            RepositoryIdentityStatus::Observed => "observed",
            RepositoryIdentityStatus::Declared => "declared",
        }
        .to_string(),
        repository_identity_value: manifest.repository_identity.value.clone(),
        repository_identity_host: manifest.repository_identity.host.clone(),
        parent_count: manifest.candidate.parents.len(),
        change_count: manifest.inventory.changes.len(),
        gitlink_count: manifest.inventory.gitlinks.len(),
        object_count: manifest.transport.object_ids.len(),
        proof_ids: manifest.proof_references.iter().map(|proof| proof.id.clone()).collect(),
        limitations: manifest.limitations.clone(),
        does_not_establish: AUTHORITY_BOUNDARY.to_vec(),
    }
}

/// Render one producer-supplied string safely into a terminal projection.
///
/// `explain` deliberately performs no shape validation, so every string it
/// prints is whatever the manifest happened to contain. A commit id, repository
/// name, or proof id carrying an ESC sequence would otherwise reach the
/// terminal as control codes and repaint lines the reader has already seen —
/// a projection that reports claims must not let a claim rewrite the report
/// around it. Control characters are shown as their code point instead of
/// executed, so the reader sees that the field contained one.
///
/// The JSON projection needs no equivalent: `serde_json` already escapes every
/// character below U+0020, and nothing interprets the result as a terminal
/// stream.
fn plain(value: &str) -> String {
    if !value.chars().any(|character| character.is_control()) {
        return value.to_string();
    }
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                format!("<U+{:04X}>", character as u32)
            } else {
                character.to_string()
            }
        })
        .collect()
}

/// Stable human projection of an explanation.
#[must_use]
pub fn render_explain_human(document: &ExplainDocument) -> String {
    let mut lines = vec![
        format!("candidate: {}", plain(&document.candidate_commit)),
        format!("identity: {}", plain(&document.candidate_identity_digest)),
        format!(
            "repository: {} ({})",
            match (&document.repository_identity_value, &document.repository_identity_host) {
                (Some(value), Some(host)) => format!("{}/{}", plain(host), plain(value)),
                (Some(value), None) => plain(value),
                (None, _) => "none".to_string(),
            },
            document.repository_identity_status
        ),
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
        let ids: Vec<String> = document.proof_ids.iter().map(|id| plain(id)).collect();
        lines.push(format!("proof: {}", ids.join(", ")));
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
        format!("envelope: {}", plain(&report.envelope)),
        format!(
            "candidate: {}",
            plain(report.candidate_commit.as_deref().unwrap_or("not_available"))
        ),
    ];
    for dimension in &report.dimensions {
        // A dimension detail quotes the envelope's own strings — a file name, a
        // proof id, a serde error carrying manifest text — so it is producer
        // content on the same footing as the manifest fields above.
        lines.push(format!(
            "{:<24} {:<14} {}",
            dimension.id,
            verdict_token(dimension.verdict),
            plain(&dimension.detail)
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Render either projection as the caller requested.
pub fn render<T: Serialize>(value: &T, human: &str, as_json: bool) -> Result<String, String> {
    if as_json { canonical_json(value) } else { Ok(human.to_string()) }
}

/// Stable token for one dimension verdict.
const fn verdict_token(verdict: DimensionVerdict) -> &'static str {
    match verdict {
        DimensionVerdict::Valid => "valid",
        DimensionVerdict::Invalid => "invalid",
        DimensionVerdict::NotProven => "not_proven",
        DimensionVerdict::NotEvaluated => "not_evaluated",
    }
}

/// Stable token for one limitation code.
///
/// Exhaustive by construction: a new code will not compile until it is
/// given a token here, so no limitation can reach a reader unnamed.
const fn limitation_token(limitation: LimitationCode) -> &'static str {
    match limitation {
        LimitationCode::LocalProofOnly => "local_proof_only",
        LimitationCode::TransportedObjectsNotSecretScanned => {
            "transported_objects_not_secret_scanned"
        }
        LimitationCode::InventoryRenamesAreDetected => "inventory_renames_are_detected",
        LimitationCode::RepositoryIdentityNotReceiverVerifiable => {
            "repository_identity_not_receiver_verifiable"
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
