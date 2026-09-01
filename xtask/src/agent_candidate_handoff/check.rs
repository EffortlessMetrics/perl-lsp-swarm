//! Independent validator for `agent_candidate_handoff.v1` envelopes.
//!
//! The validator trusts nothing the producer says. It imports the transport
//! into a throwaway object database and recomputes every claim from the
//! objects themselves, so the source worktree, the producing host, the
//! producer's own receipt, and network access are all irrelevant to the
//! verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::create::{
    SELF_CHECK_PENDING, SELF_CHECK_VALIDATED, build_inventory, collect_gitlinks,
    compute_identity_digest, declared_proof_subject, read_commit_identity,
};
use super::git::{is_full_object_id, run_git, run_git_with_stdin};
use super::hygiene::{
    is_proof_id, is_repository_host, is_repository_identity, is_safe_envelope_name,
    is_safe_repository_path, scan_secrets,
};
use super::model::{
    ChangeInventory, HANDOFF_MANIFEST_SCHEMA_V1, HANDOFF_RECEIPT_SCHEMA_V1, LimitationCode,
    MANIFEST_FILE_NAME, Manifest, PACK_FILE_NAME, PROOF_DIR_NAME, ProducerReceipt,
    RECEIPT_FILE_NAME, RepositoryIdentitySource, RepositoryIdentityStatus, TransportFormat,
};
use super::{HandoffOutcome, is_digest_hex};

/// Verdict for one evaluated dimension.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionVerdict {
    /// The dimension was evaluated and holds.
    Valid,
    /// The dimension was evaluated and does not hold.
    Invalid,
    /// The dimension could not be established either way.
    NotProven,
    /// The dimension was not reached because an earlier one failed.
    NotEvaluated,
}

/// One named validation dimension and its evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckDimension {
    /// Stable dimension identifier.
    pub id: String,
    /// Verdict for this dimension.
    pub verdict: DimensionVerdict,
    /// Bounded explanation. Never echoes secret material.
    pub detail: String,
}

/// Complete result of validating one envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckReport {
    /// Receipt schema identity of the check output.
    pub schema_version: String,
    /// Envelope that was evaluated.
    pub envelope: String,
    /// Candidate commit, when the manifest could be read.
    pub candidate_commit: Option<String>,
    /// Semantic identity digest, when the manifest could be read.
    pub candidate_identity_digest: Option<String>,
    /// Every dimension, in evaluation order.
    pub dimensions: Vec<CheckDimension>,
    /// Terminal classification.
    pub outcome: HandoffOutcome,
}

/// Schema identity of the check report.
pub const CHECK_REPORT_SCHEMA_V1: &str = "agent_candidate_handoff_check.v1";

/// Ceiling on the manifest and receipt documents.
///
/// An envelope is supplied by the producer, who under this format's own threat
/// model may be the less-trusted party. Every read is therefore bounded before
/// the bytes are pulled into memory, so an oversized document is refused
/// rather than allocated.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Ceiling on any single declared envelope file.
pub const MAX_ENVELOPE_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Ceiling on declared transported object ids.
pub const MAX_DECLARED_OBJECTS: usize = 2_000_000;

/// Ceiling on declared proof artifacts.
pub const MAX_DECLARED_PROOFS: usize = 256;

/// Ceiling on declared parent commits.
pub const MAX_DECLARED_PARENTS: usize = 64;

/// Ceiling on one retained producer-observation string.
pub const MAX_OBSERVATION_FIELD_BYTES: usize = 512;

/// Dimensions in evaluation order. Each is reported even when unreached.
const DIMENSION_IDS: &[&str] = &[
    "manifest_parse",
    "manifest_shape",
    "content_safety",
    "envelope_closure",
    "transport_integrity",
    "proof_binding",
    "object_import",
    // Commit identity precedes the closure comparison: the closure is derived
    // from the manifest's declared trees, so a wrong tree or a dropped parent
    // changes it too. Checking identity first keeps each defect reported as
    // itself rather than collapsing everything into a closure mismatch.
    "commit_identity",
    "object_presence",
    "limitation_completeness",
    "inventory_recomputation",
    "identity_digest",
    "repository_identity",
];

/// First failing dimension rendered as a single line, for producer errors.
#[must_use]
pub fn describe_failure(report: &CheckReport) -> String {
    report
        .dimensions
        .iter()
        .find(|dimension| {
            matches!(dimension.verdict, DimensionVerdict::Invalid | DimensionVerdict::NotProven)
        })
        .map_or_else(
            || "handoff validation failed without a named dimension".to_string(),
            |dimension| format!("{}: {}", dimension.id, dimension.detail),
        )
}

/// Accumulates dimension results so a partial evaluation still reports every
/// dimension, including the ones an earlier failure prevented.
struct Builder {
    envelope: String,
    candidate_commit: Option<String>,
    candidate_identity_digest: Option<String>,
    results: BTreeMap<&'static str, CheckDimension>,
}

impl Builder {
    fn new(envelope: &Path) -> Self {
        Self {
            envelope: envelope.to_string_lossy().replace('\\', "/"),
            candidate_commit: None,
            candidate_identity_digest: None,
            results: BTreeMap::new(),
        }
    }

    fn pass(&mut self, id: &'static str, detail: impl Into<String>) {
        self.record(id, DimensionVerdict::Valid, detail);
    }

    fn record(&mut self, id: &'static str, verdict: DimensionVerdict, detail: impl Into<String>) {
        self.results
            .insert(id, CheckDimension { id: id.to_string(), verdict, detail: detail.into() });
    }

    fn finish(self, outcome: HandoffOutcome) -> CheckReport {
        let dimensions = DIMENSION_IDS
            .iter()
            .map(|id| {
                self.results.get(id).cloned().unwrap_or(CheckDimension {
                    id: (*id).to_string(),
                    verdict: DimensionVerdict::NotEvaluated,
                    detail: "not reached".to_string(),
                })
            })
            .collect();
        CheckReport {
            schema_version: CHECK_REPORT_SCHEMA_V1.to_string(),
            envelope: self.envelope,
            candidate_commit: self.candidate_commit,
            candidate_identity_digest: self.candidate_identity_digest,
            dimensions,
            outcome,
        }
    }

    fn fail(
        mut self,
        id: &'static str,
        outcome: HandoffOutcome,
        detail: impl Into<String>,
    ) -> CheckReport {
        self.record(id, DimensionVerdict::Invalid, detail);
        self.finish(outcome)
    }
}

/// Which receipt a validation run is entitled to expect.
///
/// The producer validates its own staging directory *before* it writes the
/// validated receipt, so the two callers legitimately see different receipts.
/// Naming the distinction keeps the published path strict rather than widening
/// it to accommodate the internal one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptStage {
    /// A staging directory the producer has not published yet.
    Staged,
    /// A published envelope, which must carry a validated receipt.
    Published,
}

/// Validate one envelope with no network access, no credentials, and no
/// dependence on the producing workspace.
#[must_use]
pub fn check_handoff(envelope: &Path) -> CheckReport {
    check_envelope(envelope, ReceiptStage::Published)
}

/// Validate a staging directory on the producer's own pre-publication path.
#[must_use]
pub fn check_staged(envelope: &Path) -> CheckReport {
    check_envelope(envelope, ReceiptStage::Staged)
}

#[must_use]
fn check_envelope(envelope: &Path, stage: ReceiptStage) -> CheckReport {
    let mut builder = Builder::new(envelope);

    let manifest_bytes = match read_envelope_file(envelope, MANIFEST_FILE_NAME, MAX_DOCUMENT_BYTES)
    {
        Ok(bytes) => bytes,
        Err(detail) => {
            return builder.fail("manifest_parse", HandoffOutcome::InvalidManifest, detail);
        }
    };
    let manifest: Manifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return builder.fail(
                "manifest_parse",
                HandoffOutcome::InvalidManifest,
                format!("`{MANIFEST_FILE_NAME}` is not a valid manifest: {error}"),
            );
        }
    };
    builder.pass("manifest_parse", "manifest parsed under the closed v1 schema");
    builder.candidate_commit = Some(manifest.candidate.commit.clone());
    builder.candidate_identity_digest = Some(manifest.candidate_identity_digest.clone());

    if let Err((outcome, detail)) = validate_shape(&manifest) {
        return builder.fail("manifest_shape", outcome, detail);
    }
    builder.pass("manifest_shape", "identities, digests, and envelope names are well formed");

    if let Some(detail) = find_unsafe_content(&manifest) {
        return builder.fail("content_safety", HandoffOutcome::UnsafeContent, detail);
    }
    // Say exactly what was scanned. The transported objects are carried, not
    // audited, and a dimension that reported "retained content" would be
    // certifying bytes it never read.
    builder.pass("content_safety", "no credential material in retained manifest strings");

    if let Err(detail) = verify_envelope_closure(envelope, &manifest, stage) {
        return builder.fail("envelope_closure", HandoffOutcome::InvalidManifest, detail);
    }
    builder.pass(
        "envelope_closure",
        "envelope contains exactly the declared files and an agreeing receipt",
    );

    let pack_bytes = match verify_transport(envelope, &manifest) {
        Ok(bytes) => bytes,
        Err(detail) => {
            return builder.fail("transport_integrity", HandoffOutcome::DigestMismatch, detail);
        }
    };
    builder.pass("transport_integrity", "declared transport sizes and digests match the bytes");

    if let Err((outcome, detail)) = verify_proofs(envelope, &manifest) {
        return builder.fail("proof_binding", outcome, detail);
    }
    builder.pass("proof_binding", "declared proofs are content-addressed and subject-bound");

    let isolated = match import_isolated(&pack_bytes) {
        Ok(isolated) => isolated,
        Err((outcome, detail)) => return builder.fail("object_import", outcome, detail),
    };
    builder.pass("object_import", "transport imported into a temporary object database");

    match verify_commit_identity(isolated.path(), &manifest) {
        Ok(()) => builder.pass("commit_identity", "every declared commit field matches the object"),
        Err((outcome, detail)) => return builder.fail("commit_identity", outcome, detail),
    }

    if let Err((outcome, detail)) = verify_object_presence(isolated.path(), &manifest) {
        return builder.fail("object_presence", outcome, detail);
    }
    builder.pass("object_presence", "the transport carries exactly the candidate's closure");

    if let Err(detail) = verify_limitations(&manifest) {
        return builder.fail("limitation_completeness", HandoffOutcome::InvalidManifest, detail);
    }
    builder.pass("limitation_completeness", "every limitation the candidate requires is declared");

    match recompute_inventory(isolated.path(), &manifest) {
        Ok(()) => builder
            .pass("inventory_recomputation", "inventory recomputed from imported objects matches"),
        Err((outcome, detail)) => {
            return builder.fail("inventory_recomputation", outcome, detail);
        }
    }

    match compute_identity_digest(&manifest) {
        Ok(digest) if digest == manifest.candidate_identity_digest => {
            builder.pass("identity_digest", "semantic identity digest reproduces");
        }
        Ok(_) => {
            return builder.fail(
                "identity_digest",
                HandoffOutcome::InvalidManifest,
                "declared candidate identity digest does not cover the manifest content",
            );
        }
        Err((outcome, detail)) => return builder.fail("identity_digest", outcome, detail),
    }

    match manifest.repository_identity.status {
        RepositoryIdentityStatus::NotProven => {
            builder.record(
                "repository_identity",
                DimensionVerdict::NotProven,
                "no repository identity was proven; a publisher must supply an authorized target",
            );
            builder.finish(HandoffOutcome::RepositoryIdentityNotProven)
        }
        RepositoryIdentityStatus::Observed | RepositoryIdentityStatus::Declared => {
            // Deliberately not "verified": nothing in the envelope can check
            // this value, and a dimension that says more than it proved is the
            // failure mode this whole format exists to avoid.
            builder.pass(
                "repository_identity",
                "repository identity is present and well formed; its truth is the producer's \
                 claim and is not checkable from the envelope",
            );
            builder.finish(HandoffOutcome::ValidHandoff)
        }
    }
}

fn validate_shape(manifest: &Manifest) -> Result<(), (HandoffOutcome, String)> {
    let invalid = |detail: String| (HandoffOutcome::InvalidManifest, detail);

    if manifest.schema_version != HANDOFF_MANIFEST_SCHEMA_V1 {
        return Err(invalid(format!(
            "schema_version `{}` is not `{HANDOFF_MANIFEST_SCHEMA_V1}`",
            manifest.schema_version
        )));
    }
    if !is_digest_hex(&manifest.candidate_identity_digest) {
        return Err(invalid("candidate_identity_digest is not a SHA-256 hex digest".to_string()));
    }

    // Abbreviated object IDs are refused everywhere: a short SHA cannot
    // distinguish one object from a colliding prefix.
    let candidate = &manifest.candidate;
    for (field, value) in
        [("candidate.commit", &candidate.commit), ("candidate.tree", &candidate.tree)]
    {
        if !is_full_object_id(value) {
            return Err(invalid(format!("`{field}` is not a full 40-hex object id")));
        }
    }
    // Every parent costs the validator several Git invocations, each with its
    // own deadline but no shared budget, so an octopus commit is a lever: a
    // small envelope can buy hours of validation. Real merges are far below
    // this; an envelope above it is refused rather than worked through.
    if candidate.parents.len() > MAX_DECLARED_PARENTS {
        return Err(invalid(format!(
            "the candidate declares {} parents, above the {MAX_DECLARED_PARENTS} ceiling",
            candidate.parents.len()
        )));
    }
    if candidate.parents.len() != candidate.parent_trees.len() {
        return Err(invalid("parents and parent_trees are not positionally aligned".to_string()));
    }
    for value in candidate.parents.iter().chain(candidate.parent_trees.iter()) {
        if !is_full_object_id(value) {
            return Err(invalid("a parent identity is not a full 40-hex object id".to_string()));
        }
    }
    if candidate.is_root_commit != candidate.parents.is_empty() {
        return Err(invalid("is_root_commit disagrees with the declared parents".to_string()));
    }
    if candidate.is_merge_commit != (candidate.parents.len() > 1) {
        return Err(invalid("is_merge_commit disagrees with the declared parents".to_string()));
    }

    if manifest.transport.format != TransportFormat::GitPackV2 {
        return Err((
            HandoffOutcome::UnsupportedObjectClass,
            "transport format cannot carry object, mode, and parent identity".to_string(),
        ));
    }
    if !manifest.transport.closed_envelope {
        return Err(invalid("v1 envelopes are closed; closed_envelope must be true".to_string()));
    }
    // `git_pack_v2` is exactly one pack. Admitting a list would let a resealed
    // envelope declare and carry arbitrary extra bytes that pass closure and
    // digest checks while never being interpreted as objects — and transport
    // file identity is excluded from the semantic digest, so the candidate
    // identity would not change either.
    let [pack] = manifest.transport.files.as_slice() else {
        return Err(invalid(format!(
            "`{}` declares exactly one transport file; {} were declared",
            TransportFormat::GitPackV2.as_str(),
            manifest.transport.files.len()
        )));
    };
    if pack.name != PACK_FILE_NAME {
        return Err(invalid(format!(
            "the transport file must be named `{PACK_FILE_NAME}`, not `{}`",
            pack.name
        )));
    }
    if !is_safe_envelope_name(&pack.name) {
        return Err(invalid(format!("transport file name `{}` is not envelope-safe", pack.name)));
    }
    if !is_digest_hex(&pack.sha256) {
        return Err(invalid(format!("transport file `{}` has no SHA-256 digest", pack.name)));
    }
    if pack.bytes > MAX_ENVELOPE_FILE_BYTES {
        return Err(invalid(format!(
            "transport declares {} bytes, above the {MAX_ENVELOPE_FILE_BYTES}-byte ceiling",
            pack.bytes
        )));
    }
    if manifest.transport.object_ids.len() > MAX_DECLARED_OBJECTS {
        return Err(invalid(format!(
            "manifest declares {} objects, above the {MAX_DECLARED_OBJECTS} ceiling",
            manifest.transport.object_ids.len()
        )));
    }
    for id in &manifest.transport.object_ids {
        if !is_full_object_id(id) {
            return Err(invalid("a transported object id is not a full 40-hex id".to_string()));
        }
    }
    if manifest.proof_references.len() > MAX_DECLARED_PROOFS {
        return Err(invalid(format!(
            "manifest declares {} proofs, above the {MAX_DECLARED_PROOFS} ceiling",
            manifest.proof_references.len()
        )));
    }
    if !manifest.transport.object_ids.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(invalid("transported object ids are not sorted and unique".to_string()));
    }
    if !manifest.transport.object_ids.contains(&candidate.commit) {
        return Err(invalid(
            "the candidate commit is not among the transported objects".to_string(),
        ));
    }

    // Proof order is part of the semantic digest, so an unconstrained order
    // would let one candidate with one proof set carry several different
    // identities — and a resealed envelope could duplicate a valid reference
    // without changing anything a later dimension recomputes. Sorted and unique
    // is the same rule the transported object ids already follow.
    if !manifest.proof_references.windows(2).all(|pair| pair[0].id < pair[1].id) {
        return Err(invalid("proof references are not sorted and unique".to_string()));
    }
    for proof in &manifest.proof_references {
        if !is_proof_id(&proof.id) {
            return Err(invalid(format!("proof id `{}` is not a stable token", proof.id)));
        }
        if proof.path != format!("{PROOF_DIR_NAME}/{}", proof.id)
            || !is_safe_envelope_name(&proof.path)
        {
            return Err(invalid(format!("proof `{}` has a non-canonical path", proof.id)));
        }
        if !is_digest_hex(&proof.sha256) {
            return Err(invalid(format!("proof `{}` has no SHA-256 digest", proof.id)));
        }
    }

    // Status, source, value, and host are one closed tuple, not four
    // independent fields. Checking them separately would let a resealed
    // manifest upgrade a caller's declaration into an observation, or present
    // an observed identity with no host — claims whose strength a reader would
    // otherwise take at face value.
    let identity = &manifest.repository_identity;
    match (identity.status, identity.source) {
        (RepositoryIdentityStatus::Observed, RepositoryIdentitySource::GitRemoteOrigin) => {
            let Some(host) = &identity.host else {
                return Err(invalid(
                    "an observed repository identity must name the host it was read from"
                        .to_string(),
                ));
            };
            if !is_repository_host(host) {
                return Err(invalid(format!("`{host}` is not a lowercase host name")));
            }
        }
        (RepositoryIdentityStatus::Declared, RepositoryIdentitySource::CallerDeclared)
        | (RepositoryIdentityStatus::NotProven, RepositoryIdentitySource::Unavailable) => {
            if identity.host.is_some() {
                return Err(invalid(
                    "only an observed repository identity may name a host".to_string(),
                ));
            }
        }
        (status, source) => {
            return Err(invalid(format!(
                "repository identity status `{status:?}` cannot come from source `{source:?}`"
            )));
        }
    }
    match (identity.status, &identity.value) {
        (RepositoryIdentityStatus::NotProven, Some(_)) => {
            return Err(invalid(
                "an unproven repository identity must not carry a value".to_string(),
            ));
        }
        (RepositoryIdentityStatus::NotProven, None) => {}
        (_, None) => {
            return Err(invalid(
                "a proven repository identity must carry an owner/name value".to_string(),
            ));
        }
        (_, Some(value)) => {
            if !is_repository_identity(value) {
                return Err(invalid(format!("`{value}` is not a lowercase owner/name identity")));
            }
        }
    }

    // The observation block is outside the semantic digest by design, so its
    // size is bounded here instead: without a rule, a resealed envelope could
    // grow the manifest without limit and without changing candidate identity.
    let observation = &manifest.observation;
    for (field, value) in [
        ("producer_tool", &observation.producer_tool),
        ("producer_version", &observation.producer_version),
        ("git_version", &observation.git_version),
    ] {
        if value.len() > MAX_OBSERVATION_FIELD_BYTES {
            return Err(invalid(format!(
                "observation.{field} is {} bytes, above the {MAX_OBSERVATION_FIELD_BYTES}-byte ceiling",
                value.len()
            )));
        }
    }

    // Inventory paths are read and reported by consumers, and a tree entry can
    // be crafted to contain one. Git refuses to check such a tree out, but this
    // format hands the paths onward as data, so they carry the same shape rule
    // the envelope's own file names do.
    let inventory = &manifest.inventory;
    for change in &inventory.changes {
        for (field, value) in [("path", Some(&change.path)), ("old_path", change.old_path.as_ref())]
        {
            if let Some(value) = value
                && !is_safe_repository_path(value)
            {
                return Err(invalid(format!(
                    "inventory change `{field}` `{value}` is not a safe repository path"
                )));
            }
        }
    }
    for gitlink in &inventory.gitlinks {
        if !is_safe_repository_path(&gitlink.path) {
            return Err(invalid(format!(
                "gitlink path `{}` is not a safe repository path",
                gitlink.path
            )));
        }
    }
    if inventory.base_parent.as_ref() != candidate.parents.first() {
        return Err(invalid(
            "inventory base_parent is not the candidate's first parent".to_string(),
        ));
    }
    Ok(())
}

fn find_unsafe_content(manifest: &Manifest) -> Option<String> {
    let candidate = &manifest.candidate;
    let mut fields: Vec<(String, &str)> = vec![
        ("candidate.message".to_string(), candidate.message.as_str()),
        ("candidate.author.name".to_string(), candidate.author.name.as_str()),
        ("candidate.author.email".to_string(), candidate.author.email.as_str()),
        ("candidate.committer.name".to_string(), candidate.committer.name.as_str()),
        ("candidate.committer.email".to_string(), candidate.committer.email.as_str()),
    ];
    if let Some(value) = &manifest.repository_identity.value {
        fields.push(("repository_identity.value".to_string(), value.as_str()));
    }
    if let Some(host) = &manifest.repository_identity.host {
        fields.push(("repository_identity.host".to_string(), host.as_str()));
    }
    for change in &manifest.inventory.changes {
        fields.push((format!("inventory.changes[{}].path", change.path), change.path.as_str()));
        // A rename retains the *old* path too, and a credential-named file
        // renamed to something innocuous put the secret in `old_path` where
        // nothing looked for it.
        if let Some(old_path) = &change.old_path {
            fields
                .push((format!("inventory.changes[{}].old_path", change.path), old_path.as_str()));
        }
    }
    for gitlink in &manifest.inventory.gitlinks {
        fields.push((format!("inventory.gitlinks[{}].path", gitlink.path), gitlink.path.as_str()));
    }
    // The producer observation block is retained, rendered, and — because it is
    // deliberately outside the semantic digest so exports stay comparable — the
    // one region a reseal can rewrite without changing candidate identity.
    // Excluding it from the scan made `content_safety` assert something false
    // about the very strings least protected by anything else.
    let observation = &manifest.observation;
    fields.push(("observation.producer_tool".to_string(), observation.producer_tool.as_str()));
    fields
        .push(("observation.producer_version".to_string(), observation.producer_version.as_str()));
    fields.push(("observation.git_version".to_string(), observation.git_version.as_str()));

    for (field, value) in fields {
        if let Some(finding) = scan_secrets(&field, value).first() {
            return Some(format!("`{}` contains {} material", finding.field, finding.kind));
        }
    }
    None
}

/// Reject any byte in the envelope the manifest does not account for.
fn verify_envelope_closure(
    envelope: &Path,
    manifest: &Manifest,
    stage: ReceiptStage,
) -> Result<(), String> {
    let mut declared: BTreeSet<String> = BTreeSet::new();
    declared.insert(MANIFEST_FILE_NAME.to_string());
    declared.insert(RECEIPT_FILE_NAME.to_string());
    for file in &manifest.transport.files {
        declared.insert(file.name.clone());
    }
    for proof in &manifest.proof_references {
        declared.insert(proof.path.clone());
    }

    let mut present: BTreeSet<String> = BTreeSet::new();
    collect_relative_files(envelope, envelope, &mut present)?;

    let undeclared: Vec<&String> = present.difference(&declared).collect();
    if let Some(extra) = undeclared.first() {
        return Err(format!("`{extra}` is present but not declared by the manifest"));
    }
    let missing: Vec<&String> = declared.difference(&present).collect();
    if let Some(absent) = missing.first() {
        return Err(format!("`{absent}` is declared but absent from the envelope"));
    }

    verify_receipt_agrees(envelope, manifest, stage)
}

/// Require the producer receipt to describe the manifest it sits beside.
///
/// The receipt carries no authority — the validator recomputes every claim
/// from the objects regardless — but two documents in one envelope that name
/// different candidates is a malformed envelope, most likely a manifest
/// swapped in after the fact.
fn verify_receipt_agrees(
    envelope: &Path,
    manifest: &Manifest,
    stage: ReceiptStage,
) -> Result<(), String> {
    let bytes = read_envelope_file(envelope, RECEIPT_FILE_NAME, MAX_DOCUMENT_BYTES)?;
    let receipt: ProducerReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("`{RECEIPT_FILE_NAME}` is not a valid receipt: {error}"))?;

    if receipt.schema_version != HANDOFF_RECEIPT_SCHEMA_V1 {
        return Err(format!(
            "receipt schema `{}` is not `{HANDOFF_RECEIPT_SCHEMA_V1}`",
            receipt.schema_version
        ));
    }
    if receipt.candidate_commit != manifest.candidate.commit {
        return Err("the receipt and the manifest name different candidates".to_string());
    }
    if receipt.candidate_identity_digest != manifest.candidate_identity_digest {
        return Err("the receipt and the manifest name different candidate identities".to_string());
    }
    // A receipt that repeats the digest but contradicts the limitation list is
    // not an agreeing receipt. Limitations are the manifest's admissions, and a
    // receipt is the producer's statement that it validated *those* admissions.
    if receipt.limitations != manifest.limitations {
        return Err("the receipt and the manifest declare different limitations".to_string());
    }
    match stage {
        // Before publication the producer has not yet run its own check, so
        // `pending` is the only honest token; `create` validates in this mode.
        ReceiptStage::Staged => {
            if receipt.producer_self_check != SELF_CHECK_PENDING
                && receipt.producer_self_check != SELF_CHECK_VALIDATED
            {
                return Err(format!(
                    "receipt self-check `{}` is not a recognised token",
                    receipt.producer_self_check
                ));
            }
        }
        // A published envelope carries the receipt `publish` rewrote after the
        // check passed. Accepting `pending` here would let a directory that was
        // never validated — or one lifted out of staging — read as one that was.
        ReceiptStage::Published => {
            if receipt.producer_self_check != SELF_CHECK_VALIDATED {
                return Err(format!(
                    "receipt self-check is `{}`, not `{SELF_CHECK_VALIDATED}`; this envelope was \
                     never published by a successful producer check",
                    receipt.producer_self_check
                ));
            }
        }
    }
    Ok(())
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    into: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not read `{}`: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read a directory entry: {error}"))?;
        let path = entry.path();
        // `DirEntry::file_type` reports the link itself, so a symlink is
        // rejected here rather than silently traversed or read through.
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not classify `{}`: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("`{}` escaped the envelope root", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        if file_type.is_symlink() {
            return Err(format!(
                "`{relative}` is a symbolic link; an envelope must be self-contained"
            ));
        }
        if file_type.is_dir() {
            collect_relative_files(root, &path, into)?;
        } else if file_type.is_file() {
            into.insert(relative);
        } else {
            return Err(format!("`{relative}` is not a regular file"));
        }
    }
    Ok(())
}

/// Read one manifest-declared envelope file without following a symbolic link
/// and without reading past `limit` bytes.
///
/// Reading through a link would let an envelope validate using bytes outside
/// its own directory, so it would stop being reconstructable the moment that
/// external target moved — exactly the property the format exists to provide.
pub(super) fn read_envelope_file(
    envelope: &Path,
    relative: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let path = envelope.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("`{relative}` is not readable: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("`{relative}` is a symbolic link; an envelope must be self-contained"));
    }
    if !metadata.is_file() {
        return Err(format!("`{relative}` is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(format!(
            "`{relative}` is {} bytes, above the {limit}-byte ceiling",
            metadata.len()
        ));
    }
    fs::read(&path).map_err(|error| format!("`{relative}` is not readable: {error}"))
}

fn verify_transport(envelope: &Path, manifest: &Manifest) -> Result<Vec<u8>, String> {
    // Shape validation already established there is exactly one transport row.
    let Some(file) = manifest.transport.files.first() else {
        return Err("no transport file is declared".to_string());
    };
    let bytes = read_envelope_file(envelope, &file.name, MAX_ENVELOPE_FILE_BYTES)?;
    if bytes.len() as u64 != file.bytes {
        return Err(format!(
            "transport `{}` declares {} bytes but carries {}",
            file.name,
            file.bytes,
            bytes.len()
        ));
    }
    if super::content_digest_hex(&bytes) != file.sha256 {
        return Err(format!("transport `{}` does not match its declared digest", file.name));
    }
    Ok(bytes)
}

/// Verify each proof artifact's bytes, size, digest, and declared subject.
///
/// The manifest's `candidate_subject` is a producer claim, so it is not the
/// only thing checked: an artifact that names a commit in its own payload is
/// re-read here and must name this candidate. Otherwise a resealed envelope
/// could carry another candidate's proof under a correct-looking binding.
fn verify_proofs(envelope: &Path, manifest: &Manifest) -> Result<(), (HandoffOutcome, String)> {
    let mismatch = |detail: String| (HandoffOutcome::ProofSubjectMismatch, detail);
    for proof in &manifest.proof_references {
        if proof.candidate_subject != manifest.candidate.commit {
            return Err(mismatch(format!(
                "proof `{}` is bound to {} but this candidate is {}",
                proof.id, proof.candidate_subject, manifest.candidate.commit
            )));
        }
        if proof.bytes > MAX_ENVELOPE_FILE_BYTES {
            return Err(mismatch(format!(
                "proof `{}` declares a size above the ceiling",
                proof.id
            )));
        }
        let bytes = read_envelope_file(envelope, &proof.path, MAX_ENVELOPE_FILE_BYTES)
            .map_err(&mismatch)?;
        if bytes.len() as u64 != proof.bytes {
            return Err(mismatch(format!("proof `{}` does not match its declared size", proof.id)));
        }
        if super::content_digest_hex(&bytes) != proof.sha256 {
            return Err(mismatch(format!(
                "proof `{}` does not match its declared digest",
                proof.id
            )));
        }
        if let Some(declared) = declared_proof_subject(&bytes)
            && declared != manifest.candidate.commit
        {
            return Err(mismatch(format!(
                "proof `{}` names candidate {declared} in its own payload",
                proof.id
            )));
        }
        // The producer refuses a credential-bearing proof, but a receiver
        // cannot assume the producer ran: an envelope is supplied by the
        // less-trusted party, so the same scan runs on this side too.
        let text = String::from_utf8_lossy(&bytes);
        if let Some(finding) = scan_secrets(&format!("proof.{}", proof.id), &text).first() {
            return Err((
                HandoffOutcome::UnsafeContent,
                format!("proof `{}` contains {} material", proof.id, finding.kind),
            ));
        }
    }
    Ok(())
}

/// A throwaway bare repository holding only the transported objects.
struct IsolatedOdb {
    directory: tempfile::TempDir,
}

impl IsolatedOdb {
    fn path(&self) -> &Path {
        self.directory.path()
    }
}

fn import_isolated(pack_bytes: &[u8]) -> Result<IsolatedOdb, (HandoffOutcome, String)> {
    let instrument = |detail: String| (HandoffOutcome::InstrumentFailure, detail);

    let directory = tempfile::TempDir::new().map_err(|error| {
        instrument(format!("could not create a temporary object database: {error}"))
    })?;
    // An explicitly empty template, so no template directory can seed the fresh
    // database with refs, hooks, or an `objects/info/alternates` file.
    let init = run_git(directory.path(), &["init", "--bare", "--quiet", "--template="])
        .map_err(&instrument)?;
    if !init.succeeded() {
        return Err(instrument(format!(
            "could not initialize a temporary object database: {}",
            init.diagnostic()
        )));
    }

    // `index-pack` accepts more than one pack version, so importing
    // successfully does not confirm the `git_pack_v2` the manifest declares.
    // The header is the only place that claim can be checked, and an unchecked
    // format claim is exactly the kind a resealed envelope would exploit.
    verify_pack_version(pack_bytes)?;

    let imported = run_git_with_stdin(directory.path(), &["index-pack", "--stdin"], pack_bytes)
        .map_err(&instrument)?;
    if !imported.succeeded() {
        // The bytes matched their declared digest but do not yield objects, so
        // the transport cannot supply the candidate at all.
        return Err((
            HandoffOutcome::MissingObject,
            format!("the transport is not a usable object pack: {}", imported.diagnostic()),
        ));
    }
    Ok(IsolatedOdb { directory })
}

/// Require the pack header to be the version the manifest declares.
///
/// A pack begins with the four bytes `PACK` and a big-endian version word.
/// `TransportFormat::GitPackV2` is the only v1 transport, so any other version
/// is an unsupported object class rather than a corrupt pack: the bytes may be
/// a perfectly good pack, just not one this format claims to carry.
fn verify_pack_version(pack_bytes: &[u8]) -> Result<(), (HandoffOutcome, String)> {
    let Some(header) = pack_bytes.get(..8) else {
        return Err((
            HandoffOutcome::MissingObject,
            "the transport is too short to be an object pack".to_string(),
        ));
    };
    if &header[..4] != b"PACK" {
        return Err((
            HandoffOutcome::MissingObject,
            "the transport does not begin with a pack signature".to_string(),
        ));
    }
    let version = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    if version != 2 {
        return Err((
            HandoffOutcome::UnsupportedObjectClass,
            format!("the transport is a version {version} pack, but `git_pack_v2` was declared"),
        ));
    }
    Ok(())
}

/// Prove the transport carries the complete object closure the candidate needs.
///
/// Checking only the declared list would be circular, and recomputing the
/// inventory is not sufficient either: `git diff-tree` reports a changed path
/// by reading trees alone, so a candidate stripped of a blob still produces a
/// complete-looking inventory. The closure is therefore derived from the trees
/// themselves and every member is required to be present.
fn verify_object_presence(odb: &Path, manifest: &Manifest) -> Result<(), (HandoffOutcome, String)> {
    // Git failing to run is not the candidate failing to carry an object. A
    // deadline breach or an output-cap breach here would otherwise be reported
    // as MISSING_OBJECT, telling automation the envelope is bad (exit 2) when
    // the truth is that the instrument did not produce an answer (exit 4).
    let instrument = |detail: String| (HandoffOutcome::InstrumentFailure, detail);
    let missing = |detail: String| (HandoffOutcome::MissingObject, detail);
    let candidate = &manifest.candidate;
    let mut required: BTreeSet<String> = BTreeSet::new();
    required.insert(candidate.commit.clone());
    required.insert(candidate.tree.clone());
    for id in candidate.parents.iter().chain(candidate.parent_trees.iter()) {
        required.insert(id.clone());
    }

    let mut roots = vec![candidate.tree.clone()];
    roots.extend(candidate.parent_trees.iter().cloned());
    for root in &roots {
        let output = run_git(odb, &["ls-tree", "-r", "-t", "-z", "--end-of-options", root])
            .map_err(&instrument)?;
        if !output.succeeded() {
            return Err(missing(format!("tree {root} is not readable from the transport")));
        }
        for entry in output.stdout.split('\0').filter(|entry| !entry.is_empty()) {
            // A record this reader cannot parse is unexpected Git output, not
            // an absent object. Skipping it would silently shrink the derived
            // closure and let an incomplete transport look authoritative, so it
            // is an instrument failure instead.
            let malformed =
                || instrument(format!("tree {root} produced an unreadable entry record"));
            let (metadata, _path) = entry.split_once('\t').ok_or_else(malformed)?;
            let columns: Vec<&str> = metadata.split_whitespace().collect();
            let [mode, _kind, object] = columns.as_slice() else {
                return Err(malformed());
            };
            // A gitlink names a commit in another repository and is recorded
            // rather than transported, so it is not part of this closure.
            if *mode != "160000" {
                required.insert((*object).to_string());
            }
        }
    }

    // The declared set must equal the derived closure, not merely contain it.
    // Accepting a superset would let a resealed envelope carry an unrelated
    // object — a blob from another branch, or one holding credential material
    // — inside a transport that still validates as this one bounded candidate.
    let declared: BTreeSet<&String> = manifest.transport.object_ids.iter().collect();
    if let Some(undeclared) = required.iter().find(|id| !declared.contains(id)) {
        return Err(missing(format!(
            "object {undeclared} is required by the candidate but not declared by the transport"
        )));
    }
    if let Some(extra) = declared.iter().find(|id| !required.contains(**id)) {
        return Err(missing(format!(
            "object {extra} is declared by the transport but not required by the candidate"
        )));
    }

    // Comparing the manifest against itself would still be circular: a
    // resealed pack can carry the whole valid closure *plus* undeclared
    // objects, refresh its size and digest, and leave `object_ids` untouched.
    // The object database was empty before this envelope's pack was imported,
    // so enumerating it now yields exactly what the transport really carried.
    let present = run_git(odb, &["cat-file", "--batch-all-objects", "--batch-check=%(objectname)"])
        .map_err(&instrument)?;
    if !present.succeeded() {
        return Err(instrument(format!(
            "could not enumerate imported objects: {}",
            present.diagnostic()
        )));
    }
    let carried: BTreeSet<String> = present.stdout.split_whitespace().map(str::to_string).collect();

    if let Some(absent) = required.iter().find(|id| !carried.contains(*id)) {
        return Err(missing(format!("object {absent} is absent from the transport")));
    }
    if let Some(stowaway) = carried.iter().find(|id| !required.contains(*id)) {
        return Err(missing(format!(
            "the transport carries object {stowaway}, which the candidate does not require"
        )));
    }
    Ok(())
}

/// Limitation codes the candidate's own facts make mandatory.
///
/// Limitations are the confidence boundaries a receiver acts on, and nothing
/// in the objects records them, so the semantic digest is their only guard —
/// and that digest is recomputable by whoever edited the manifest. Deriving
/// the mandatory set here means a resealed envelope cannot quietly drop an
/// admission such as "these objects were not secret-scanned".
fn mandatory_limitations(manifest: &Manifest) -> BTreeSet<LimitationCode> {
    let mut required = BTreeSet::new();
    required.insert(LimitationCode::LocalProofOnly);
    required.insert(LimitationCode::TransportBytesNotVersionStable);
    required.insert(LimitationCode::TransportedObjectsNotSecretScanned);
    required.insert(LimitationCode::InventoryRenamesAreDetected);
    required.insert(LimitationCode::RepositoryIdentityNotReceiverVerifiable);
    if manifest.candidate.is_root_commit {
        required.insert(LimitationCode::RootCommitDiffAgainstEmptyTree);
    }
    if manifest.candidate.is_merge_commit {
        required.insert(LimitationCode::MergeCommitDiffAgainstFirstParent);
    }
    if !manifest.inventory.gitlinks.is_empty() {
        required.insert(LimitationCode::SubmoduleGitlinkNotTransported);
    }
    if manifest.repository_identity.status == RepositoryIdentityStatus::NotProven {
        required.insert(LimitationCode::RepositoryIdentityNotProven);
    }
    // `RemoteUrlContainedCredentials` is producer-only knowledge — a receiver
    // cannot derive it — so it is permitted but never required.
    required
}

fn verify_limitations(manifest: &Manifest) -> Result<(), String> {
    let declared: BTreeSet<LimitationCode> = manifest.limitations.iter().copied().collect();
    if declared.len() != manifest.limitations.len() {
        return Err("limitation codes are duplicated".to_string());
    }
    if !manifest.limitations.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err("limitation codes are not sorted".to_string());
    }
    let required = mandatory_limitations(manifest);
    if let Some(missing) = required.difference(&declared).next() {
        return Err(format!(
            "the candidate's own facts require limitation `{missing:?}`, which is not declared"
        ));
    }
    // Superset was not enough. An *unearned* code is a false admission, and a
    // false admission is as dishonest as a dropped one: adding
    // `repository_identity_not_proven` to a manifest that proves an identity
    // makes the envelope say both things at once, and `explain` prints both.
    // Every code except one is derivable here, so equality costs nothing.
    for extra in declared.difference(&required) {
        if *extra != LimitationCode::RemoteUrlContainedCredentials {
            return Err(format!(
                "limitation `{extra:?}` is declared but the candidate's own facts do not support it"
            ));
        }
    }
    Ok(())
}

/// Recompute the candidate's whole identity from the imported objects.
///
/// Every retained field is re-derived by the same reader the producer used, so
/// nothing the manifest carries is accepted on trust. Verifying only the tree
/// and parents would leave the message, author, and committer free to drift: a
/// resealed envelope could import correctly while misrepresenting to a human
/// reader what the candidate is.
fn verify_commit_identity(odb: &Path, manifest: &Manifest) -> Result<(), (HandoffOutcome, String)> {
    let declared = &manifest.candidate;

    // Absence is its own class: a transport that simply does not carry the
    // candidate is a missing object, not a disagreement about its content.
    let present = run_git(odb, &["cat-file", "-e", "--end-of-options", &declared.commit])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !present.succeeded() {
        return Err((
            HandoffOutcome::MissingObject,
            format!("candidate commit {} is not in the transport", declared.commit),
        ));
    }

    let observed = read_commit_identity(odb, &declared.commit)?;

    if observed.tree != declared.tree {
        return Err((
            HandoffOutcome::TreeMismatch,
            format!(
                "imported commit points at tree {}, not the declared {}",
                observed.tree, declared.tree
            ),
        ));
    }
    if observed.parents != declared.parents {
        return Err((
            HandoffOutcome::ParentMismatch,
            "imported commit parents differ from the declared ordered parents".to_string(),
        ));
    }
    if observed.parent_trees != declared.parent_trees {
        return Err((
            HandoffOutcome::TreeMismatch,
            "a declared parent tree differs from the imported parent's tree".to_string(),
        ));
    }

    for (field, observed_value, declared_value) in [
        ("message", &observed.message, &declared.message),
        ("author.name", &observed.author.name, &declared.author.name),
        ("author.email", &observed.author.email, &declared.author.email),
        ("author.date", &observed.author.date, &declared.author.date),
        ("committer.name", &observed.committer.name, &declared.committer.name),
        ("committer.email", &observed.committer.email, &declared.committer.email),
        ("committer.date", &observed.committer.date, &declared.committer.date),
    ] {
        if observed_value != declared_value {
            return Err((
                HandoffOutcome::InvalidManifest,
                format!("declared `candidate.{field}` differs from the imported commit object"),
            ));
        }
    }
    Ok(())
}

fn recompute_inventory(odb: &Path, manifest: &Manifest) -> Result<(), (HandoffOutcome, String)> {
    let recomputed = build_inventory(odb, &manifest.candidate)?;
    let gitlinks = collect_gitlinks(odb, &manifest.candidate.tree)?;
    let recomputed = ChangeInventory { gitlinks, ..recomputed };

    if recomputed != manifest.inventory {
        return Err((
            HandoffOutcome::InventoryMismatch,
            inventory_difference(&manifest.inventory, &recomputed),
        ));
    }
    Ok(())
}

/// Name the first concrete inventory disagreement rather than reporting that
/// two large structures differ.
fn inventory_difference(declared: &ChangeInventory, recomputed: &ChangeInventory) -> String {
    if declared.base_parent != recomputed.base_parent {
        return "declared inventory base parent differs from the imported commit".to_string();
    }
    if declared.changes.len() != recomputed.changes.len() {
        return format!(
            "declared inventory lists {} changes but the objects yield {}",
            declared.changes.len(),
            recomputed.changes.len()
        );
    }
    for (declared_change, recomputed_change) in declared.changes.iter().zip(&recomputed.changes) {
        if declared_change != recomputed_change {
            return format!(
                "declared change for `{}` differs from the recomputed change",
                declared_change.path
            );
        }
    }
    if declared.gitlinks != recomputed.gitlinks {
        return "declared submodule references differ from the candidate tree".to_string();
    }
    "declared inventory differs from the recomputed inventory".to_string()
}
