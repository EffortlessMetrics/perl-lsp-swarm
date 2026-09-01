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
    build_inventory, collect_gitlinks, compute_identity_digest, declared_proof_subject,
    read_commit_identity,
};
use super::git::{is_full_object_id, run_git, run_git_with_stdin};
use super::hygiene::{is_proof_id, is_repository_identity, is_safe_envelope_name, scan_secrets};
use super::model::{
    ChangeInventory, HANDOFF_MANIFEST_SCHEMA_V1, HANDOFF_RECEIPT_SCHEMA_V1, MANIFEST_FILE_NAME,
    Manifest, PACK_FILE_NAME, PROOF_DIR_NAME, ProducerReceipt, RECEIPT_FILE_NAME,
    RepositoryIdentityStatus, TransportFormat,
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

/// Validate one envelope with no network access, no credentials, and no
/// dependence on the producing workspace.
#[must_use]
pub fn check_handoff(envelope: &Path) -> CheckReport {
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

    if let Err(detail) = verify_envelope_closure(envelope, &manifest) {
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

    if let Err(detail) = verify_proofs(envelope, &manifest) {
        return builder.fail("proof_binding", HandoffOutcome::ProofSubjectMismatch, detail);
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

    if let Err(detail) = verify_object_presence(isolated.path(), &manifest) {
        return builder.fail("object_presence", HandoffOutcome::MissingObject, detail);
    }
    builder.pass("object_presence", "the transport carries exactly the candidate's closure");

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
            builder.pass("repository_identity", "repository identity is present and well formed");
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

    match (&manifest.repository_identity.status, &manifest.repository_identity.value) {
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

    let inventory = &manifest.inventory;
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
    for change in &manifest.inventory.changes {
        fields.push((format!("inventory.changes[{}].path", change.path), change.path.as_str()));
    }

    for (field, value) in fields {
        if let Some(finding) = scan_secrets(&field, value).first() {
            return Some(format!("`{}` contains {} material", finding.field, finding.kind));
        }
    }
    None
}

/// Reject any byte in the envelope the manifest does not account for.
fn verify_envelope_closure(envelope: &Path, manifest: &Manifest) -> Result<(), String> {
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

    verify_receipt_agrees(envelope, manifest)
}

/// Require the producer receipt to describe the manifest it sits beside.
///
/// The receipt carries no authority — the validator recomputes every claim
/// from the objects regardless — but two documents in one envelope that name
/// different candidates is a malformed envelope, most likely a manifest
/// swapped in after the fact.
fn verify_receipt_agrees(envelope: &Path, manifest: &Manifest) -> Result<(), String> {
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
fn read_envelope_file(envelope: &Path, relative: &str, limit: u64) -> Result<Vec<u8>, String> {
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
fn verify_proofs(envelope: &Path, manifest: &Manifest) -> Result<(), String> {
    for proof in &manifest.proof_references {
        if proof.candidate_subject != manifest.candidate.commit {
            return Err(format!(
                "proof `{}` is bound to {} but this candidate is {}",
                proof.id, proof.candidate_subject, manifest.candidate.commit
            ));
        }
        if proof.bytes > MAX_ENVELOPE_FILE_BYTES {
            return Err(format!("proof `{}` declares a size above the ceiling", proof.id));
        }
        let bytes = read_envelope_file(envelope, &proof.path, MAX_ENVELOPE_FILE_BYTES)?;
        if bytes.len() as u64 != proof.bytes {
            return Err(format!("proof `{}` does not match its declared size", proof.id));
        }
        if super::content_digest_hex(&bytes) != proof.sha256 {
            return Err(format!("proof `{}` does not match its declared digest", proof.id));
        }
        if let Some(declared) = declared_proof_subject(&bytes)
            && declared != manifest.candidate.commit
        {
            return Err(format!(
                "proof `{}` names candidate {declared} in its own payload",
                proof.id
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
    let init = run_git(directory.path(), &["init", "--bare", "--quiet"]).map_err(&instrument)?;
    if !init.succeeded() {
        return Err(instrument(format!(
            "could not initialize a temporary object database: {}",
            init.diagnostic()
        )));
    }

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

/// Prove the transport carries the complete object closure the candidate needs.
///
/// Checking only the declared list would be circular, and recomputing the
/// inventory is not sufficient either: `git diff-tree` reports a changed path
/// by reading trees alone, so a candidate stripped of a blob still produces a
/// complete-looking inventory. The closure is therefore derived from the trees
/// themselves and every member is required to be present.
fn verify_object_presence(odb: &Path, manifest: &Manifest) -> Result<(), String> {
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
        let output = run_git(odb, &["ls-tree", "-r", "-t", "-z", "--end-of-options", root])?;
        if !output.succeeded() {
            return Err(format!("tree {root} is not readable from the transport"));
        }
        for entry in output.stdout.split('\0').filter(|entry| !entry.is_empty()) {
            let Some((metadata, _path)) = entry.split_once('\t') else {
                continue;
            };
            let columns: Vec<&str> = metadata.split_whitespace().collect();
            let [mode, _kind, object] = columns.as_slice() else {
                continue;
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
        return Err(format!(
            "object {undeclared} is required by the candidate but not declared by the transport"
        ));
    }
    if let Some(extra) = declared.iter().find(|id| !required.contains(**id)) {
        return Err(format!(
            "object {extra} is declared by the transport but not required by the candidate"
        ));
    }

    let mut stdin = String::new();
    for id in required.iter().chain(manifest.transport.object_ids.iter()) {
        stdin.push_str(id);
        stdin.push('\n');
    }
    let output = run_git_with_stdin(odb, &["cat-file", "--batch-check"], stdin.as_bytes())?;
    if !output.succeeded() {
        return Err(format!("could not inspect imported objects: {}", output.diagnostic()));
    }
    for line in output.stdout.lines() {
        if line.ends_with(" missing") {
            let id = line.split_whitespace().next().unwrap_or(line);
            return Err(format!("object {id} is absent from the transport"));
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
