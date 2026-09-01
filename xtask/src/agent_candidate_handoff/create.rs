//! Producer for `agent_candidate_handoff.v1` envelopes.
//!
//! The producer never reads the worktree. Everything it transports comes from
//! committed objects, so dirty files, ignored files, credentials on disk, and
//! untracked scratch state cannot enter an envelope even by accident: an
//! untracked file becomes transportable only once it is committed into the
//! exact tree.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::check::{CheckReport, check_handoff, describe_failure};
use super::git::{git_version, is_full_object_id, run_git, run_git_with_stdin};
use super::hygiene::{
    is_proof_id, is_repository_identity, repository_identity_from_remote, scan_secrets,
};
use super::model::{
    CandidateIdentity, ChangeInventory, ChangeRecord, ChangeStatus, CommitPerson, EntryClass,
    GitlinkDisposition, GitlinkRecord, HANDOFF_MANIFEST_SCHEMA_V1, HANDOFF_RECEIPT_SCHEMA_V1,
    LimitationCode, MANIFEST_FILE_NAME, Manifest, PACK_FILE_NAME, PROOF_DIR_NAME,
    ProducerObservation, ProducerReceipt, ProofReference, RECEIPT_FILE_NAME, RepositoryIdentity,
    RepositoryIdentitySource, RepositoryIdentityStatus, Transport, TransportFile, TransportFormat,
};
use super::{HandoffOutcome, canonical_json, content_digest_hex};

/// JSON keys a proof artifact may use to name the candidate it proves.
pub const PROOF_SUBJECT_KEYS: &[&str] =
    &["commit", "candidate", "candidate_commit", "subject", "head_sha", "sha"];

/// Self-check value written while an envelope is still staged.
pub const SELF_CHECK_PENDING: &str = "pending";

/// Self-check value written only after independent validation succeeded.
pub const SELF_CHECK_VALIDATED: &str = "validated_before_publish";

/// Inputs for one envelope export.
pub struct CreateRequest {
    /// Repository to read. Never mutated.
    pub repository: PathBuf,
    /// Revision naming the candidate commit.
    pub candidate: String,
    /// Destination directory for the envelope. Must not already exist.
    pub out: PathBuf,
    /// Caller-declared `owner/name`, used when no remote can be observed.
    pub declared_repository_identity: Option<String>,
    /// Proof artifacts to carry, content-addressed and subject-bound.
    pub proofs: Vec<PathBuf>,
}

/// Build one immutable envelope and validate it before returning.
///
/// The producer runs the same independent validator a receiver would run, so a
/// `Ok` return means the envelope has already been proved reconstructable
/// rather than merely written.
pub fn create_handoff(request: &CreateRequest) -> Result<Manifest, (HandoffOutcome, String)> {
    create_handoff_with_validator(request, check_handoff)
}

/// [`create_handoff`] with an injectable validator, so the failure path can be
/// exercised: a validator that refuses must leave no published envelope.
pub fn create_handoff_with_validator(
    request: &CreateRequest,
    validate: fn(&Path) -> CheckReport,
) -> Result<Manifest, (HandoffOutcome, String)> {
    let repository = request.repository.as_path();
    let instrument = |message: String| (HandoffOutcome::InstrumentFailure, message);

    if !run_git(repository, &["rev-parse", "--show-toplevel"]).map_err(&instrument)?.succeeded() {
        return Err(instrument(
            "the requested path is not an inspectable Git worktree".to_string(),
        ));
    }

    let commit = resolve_commit(repository, &request.candidate)?;
    let candidate = read_commit_identity(repository, &commit)?;
    let mut limitations: BTreeSet<LimitationCode> = BTreeSet::new();
    limitations.insert(LimitationCode::LocalProofOnly);
    limitations.insert(LimitationCode::TransportBytesNotVersionStable);
    limitations.insert(LimitationCode::TransportedObjectsNotSecretScanned);
    if candidate.is_root_commit {
        limitations.insert(LimitationCode::RootCommitDiffAgainstEmptyTree);
    }
    if candidate.is_merge_commit {
        limitations.insert(LimitationCode::MergeCommitDiffAgainstFirstParent);
    }

    let repository_identity = resolve_repository_identity(repository, request, &mut limitations)?;
    let inventory = build_inventory(repository, &candidate)?;
    if !inventory.gitlinks.is_empty() {
        limitations.insert(LimitationCode::SubmoduleGitlinkNotTransported);
    }

    let object_ids = enumerate_objects(repository, &candidate)?;
    let pack_bytes = build_pack(repository, &object_ids)?;

    // Refuse before writing anything: a secret must not reach disk in an
    // envelope that a later step might still hand onward.
    guard_retained_content(&candidate, &repository_identity)?;

    let proofs = collect_proofs(&request.proofs, &commit)?;

    let mut manifest = Manifest {
        schema_version: HANDOFF_MANIFEST_SCHEMA_V1.to_string(),
        candidate_identity_digest: String::new(),
        repository_identity,
        candidate,
        inventory,
        transport: Transport {
            format: TransportFormat::GitPackV2,
            closed_envelope: true,
            files: vec![TransportFile {
                name: PACK_FILE_NAME.to_string(),
                bytes: pack_bytes.len() as u64,
                sha256: content_digest_hex(&pack_bytes),
            }],
            object_ids,
        },
        proof_references: proofs.iter().map(|proof| proof.reference.clone()).collect(),
        limitations: limitations.into_iter().collect(),
        observation: ProducerObservation {
            producer_tool: "cargo-xtask-agent-candidate-handoff".to_string(),
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            git_version: git_version(repository),
        },
    };
    manifest.candidate_identity_digest = compute_identity_digest(&manifest)?;

    // Stage, validate, then publish. Writing straight to the destination would
    // leave a directory carrying a receipt that asserts a validation which
    // never succeeded whenever the check failed, the validator itself hit an
    // instrument failure, or the process died in between — and D2 is meant to
    // consume exactly that durable artifact.
    let staging = stage_envelope(&request.out, &manifest, &pack_bytes, &proofs)?;

    let report = validate(staging.path());
    if report.outcome != HandoffOutcome::ValidHandoff
        && report.outcome != HandoffOutcome::RepositoryIdentityNotProven
    {
        staging.discard();
        return Err((report.outcome, describe_failure(&report)));
    }

    staging.publish(&request.out, &manifest)?;

    Ok(manifest)
}

/// A staged envelope that is removed unless it is explicitly published.
struct StagedEnvelope {
    directory: PathBuf,
}

impl StagedEnvelope {
    fn path(&self) -> &Path {
        &self.directory
    }

    fn discard(self) {
        let _ = fs::remove_dir_all(&self.directory);
    }

    /// Write the validated receipt, then move the directory into place.
    ///
    /// The rename is the publication step and is atomic on the destination's
    /// own filesystem, so a reader never observes a half-written envelope.
    fn publish(
        self,
        destination: &Path,
        manifest: &Manifest,
    ) -> Result<(), (HandoffOutcome, String)> {
        let failed = |error: std::io::Error, what: &str| {
            (HandoffOutcome::InstrumentFailure, format!("could not {what}: {error}"))
        };

        let receipt = ProducerReceipt {
            schema_version: HANDOFF_RECEIPT_SCHEMA_V1.to_string(),
            candidate_identity_digest: manifest.candidate_identity_digest.clone(),
            candidate_commit: manifest.candidate.commit.clone(),
            producer_self_check: SELF_CHECK_VALIDATED.to_string(),
            limitations: manifest.limitations.clone(),
        };
        let receipt_json =
            canonical_json(&receipt).map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
        if let Err(error) = fs::write(self.directory.join(RECEIPT_FILE_NAME), receipt_json) {
            let _ = fs::remove_dir_all(&self.directory);
            return Err(failed(error, "write the validated receipt"));
        }

        if let Err(error) = fs::rename(&self.directory, destination) {
            let _ = fs::remove_dir_all(&self.directory);
            return Err(failed(error, "publish the envelope"));
        }
        Ok(())
    }
}

/// Compute the semantic identity digest over a manifest's stable projection.
pub fn compute_identity_digest(manifest: &Manifest) -> Result<String, (HandoffOutcome, String)> {
    let identity = manifest.semantic_identity();
    let json =
        canonical_json(&identity).map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    Ok(content_digest_hex(json.as_bytes()))
}

fn resolve_commit(repository: &Path, revision: &str) -> Result<String, (HandoffOutcome, String)> {
    if revision.trim().is_empty() || revision.starts_with('-') {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("`{revision}` is not a usable revision"),
        ));
    }
    let argument = format!("{revision}^{{commit}}");
    let output = run_git(repository, &["rev-parse", "--verify", "--end-of-options", &argument])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not resolve `{revision}` to a commit: {}", output.diagnostic()),
        ));
    }
    let sha = output.stdout.trim().to_string();
    if !is_full_object_id(&sha) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("git returned a non-canonical object id for `{revision}`"),
        ));
    }
    Ok(sha)
}

/// Read one commit's complete identity from a repository or object database.
///
/// The validator reruns this exact function against the imported objects, so
/// producer and consumer cannot disagree about how a commit is read — every
/// retained field is recomputed rather than carried on trust.
pub fn read_commit_identity(
    repository: &Path,
    commit: &str,
) -> Result<CandidateIdentity, (HandoffOutcome, String)> {
    // A single formatted read keeps author, committer, parents, and message
    // from one commit object; separate invocations could straddle a change.
    let format = "%T%n%P%n%an%n%ae%n%ad%n%cn%n%ce%n%cd%n%B";
    let output = run_git(
        repository,
        &["show", "-s", "--date=raw", &format!("--format={format}"), "--end-of-options", commit],
    )
    .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not read commit {commit}: {}", output.diagnostic()),
        ));
    }

    let mut lines = output.stdout.splitn(9, '\n');
    let mut next = |field: &str| -> Result<String, (HandoffOutcome, String)> {
        lines.next().map(str::to_string).ok_or_else(|| {
            (HandoffOutcome::InstrumentFailure, format!("commit {commit} did not report `{field}`"))
        })
    };

    let tree = next("tree")?;
    let parents_raw = next("parents")?;
    let author_name = next("author name")?;
    let author_email = next("author email")?;
    let author_date = next("author date")?;
    let committer_name = next("committer name")?;
    let committer_email = next("committer email")?;
    let committer_date = next("committer date")?;
    let message = next("message")?;

    if !is_full_object_id(&tree) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("commit {commit} reported a non-canonical tree id"),
        ));
    }

    let parents: Vec<String> = parents_raw.split_whitespace().map(str::to_string).collect();
    for parent in &parents {
        if !is_full_object_id(parent) {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("commit {commit} reported a non-canonical parent id"),
            ));
        }
    }

    let mut parent_trees = Vec::with_capacity(parents.len());
    for parent in &parents {
        parent_trees.push(resolve_tree(repository, parent)?);
    }

    Ok(CandidateIdentity {
        commit: commit.to_string(),
        tree,
        is_root_commit: parents.is_empty(),
        is_merge_commit: parents.len() > 1,
        parents,
        parent_trees,
        message,
        author: CommitPerson { name: author_name, email: author_email, date: author_date },
        committer: CommitPerson {
            name: committer_name,
            email: committer_email,
            date: committer_date,
        },
    })
}

fn resolve_tree(repository: &Path, commit: &str) -> Result<String, (HandoffOutcome, String)> {
    let argument = format!("{commit}^{{tree}}");
    let output = run_git(repository, &["rev-parse", "--verify", "--end-of-options", &argument])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::MissingObject,
            format!("tree of {commit} is not present locally: {}", output.diagnostic()),
        ));
    }
    Ok(output.stdout.trim().to_string())
}

fn resolve_repository_identity(
    repository: &Path,
    request: &CreateRequest,
    limitations: &mut BTreeSet<LimitationCode>,
) -> Result<RepositoryIdentity, (HandoffOutcome, String)> {
    if let Some(declared) = &request.declared_repository_identity {
        let normalized = declared.trim().to_lowercase();
        if !is_repository_identity(&normalized) {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("`{declared}` is not a lowercase owner/name repository identity"),
            ));
        }
        return Ok(RepositoryIdentity {
            status: RepositoryIdentityStatus::Declared,
            value: Some(normalized),
            source: RepositoryIdentitySource::CallerDeclared,
        });
    }

    let output = run_git(repository, &["config", "--get", "remote.origin.url"])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if output.succeeded() {
        match repository_identity_from_remote(output.stdout.trim()) {
            Ok(Some(value)) => {
                return Ok(RepositoryIdentity {
                    status: RepositoryIdentityStatus::Observed,
                    value: Some(value),
                    source: RepositoryIdentitySource::GitRemoteOrigin,
                });
            }
            Ok(None) => {}
            Err(_) => {
                // The URL carried credentials. Record the refusal as a code and
                // retain none of the URL bytes.
                limitations.insert(LimitationCode::RemoteUrlContainedCredentials);
            }
        }
    }

    limitations.insert(LimitationCode::RepositoryIdentityNotProven);
    Ok(RepositoryIdentity {
        status: RepositoryIdentityStatus::NotProven,
        value: None,
        source: RepositoryIdentitySource::Unavailable,
    })
}

/// Recompute the changed-path inventory from trees.
///
/// Diffing trees rather than commits keeps merge handling explicit: Git
/// suppresses merge diffs by default, and silently emitting nothing for a
/// merge candidate would be an empty inventory that looks complete.
pub fn build_inventory(
    repository: &Path,
    candidate: &CandidateIdentity,
) -> Result<ChangeInventory, (HandoffOutcome, String)> {
    let base_parent = candidate.parents.first().cloned();
    let raw = if candidate.is_root_commit {
        run_git(
            repository,
            &[
                "diff-tree",
                "-r",
                "-M",
                "--raw",
                "-z",
                "--no-commit-id",
                "--root",
                "--end-of-options",
                &candidate.commit,
            ],
        )
    } else {
        let base_tree = candidate.parent_trees.first().map(String::as_str).unwrap_or_default();
        run_git(
            repository,
            &[
                "diff-tree",
                "-r",
                "-M",
                "--raw",
                "-z",
                "--no-commit-id",
                "--end-of-options",
                base_tree,
                &candidate.tree,
            ],
        )
    }
    .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;

    if !raw.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not diff candidate {}: {}", candidate.commit, raw.diagnostic()),
        ));
    }

    let mut changes = parse_raw_diff(&raw.stdout)?;
    changes.sort_by(|left, right| {
        left.path.cmp(&right.path).then_with(|| left.old_path.cmp(&right.old_path))
    });

    let gitlinks = collect_gitlinks(repository, &candidate.tree)?;

    Ok(ChangeInventory { base_parent, changes, gitlinks })
}

/// Parse `git diff-tree --raw -z` records.
///
/// The NUL-delimited form is the only one safe for the paths this envelope
/// must carry: quoted output would re-encode Unicode and whitespace-heavy
/// paths, and the inventory must round-trip them exactly.
fn parse_raw_diff(stdout: &str) -> Result<Vec<ChangeRecord>, (HandoffOutcome, String)> {
    let malformed = |detail: &str| {
        (HandoffOutcome::InstrumentFailure, format!("malformed raw diff record: {detail}"))
    };
    let mut fields = stdout.split('\0').filter(|field| !field.is_empty());
    let mut records = Vec::new();

    while let Some(header) = fields.next() {
        let header = header.strip_prefix(':').ok_or_else(|| malformed("missing leading colon"))?;
        let parts: Vec<&str> = header.split(' ').collect();
        let [old_mode, new_mode, old_object, new_object, status_field] = parts.as_slice() else {
            return Err(malformed("expected five header fields"));
        };

        let (status_letter, score) = split_status(status_field);
        let status = match status_letter {
            'A' => ChangeStatus::Added,
            'M' => ChangeStatus::Modified,
            'D' => ChangeStatus::Deleted,
            'R' => ChangeStatus::Renamed,
            'C' => ChangeStatus::Copied,
            'T' => ChangeStatus::TypeChanged,
            other => return Err(malformed(&format!("unsupported status `{other}`"))),
        };

        let (old_path, path) = if matches!(status, ChangeStatus::Renamed | ChangeStatus::Copied) {
            let source = fields.next().ok_or_else(|| malformed("rename source missing"))?;
            let destination =
                fields.next().ok_or_else(|| malformed("rename destination missing"))?;
            (Some(source.to_string()), destination.to_string())
        } else {
            let single = fields.next().ok_or_else(|| malformed("path missing"))?;
            (None, single.to_string())
        };

        records.push(ChangeRecord {
            status,
            path,
            old_path,
            old_mode: mode_option(old_mode),
            new_mode: mode_option(new_mode),
            old_object: object_option(old_object),
            new_object: object_option(new_object),
            similarity: score,
            entry_class: entry_class_for(new_mode),
        });
    }

    Ok(records)
}

fn split_status(field: &str) -> (char, Option<u32>) {
    let mut characters = field.chars();
    let letter = characters.next().unwrap_or('?');
    let score = characters.as_str();
    (letter, if score.is_empty() { None } else { score.parse::<u32>().ok() })
}

fn mode_option(mode: &str) -> Option<String> {
    (mode != "000000").then(|| mode.to_string())
}

fn object_option(object: &str) -> Option<String> {
    (!object.chars().all(|character| character == '0')).then(|| object.to_string())
}

fn entry_class_for(mode: &str) -> EntryClass {
    match mode {
        "100644" => EntryClass::RegularFile,
        "100755" => EntryClass::ExecutableFile,
        "120000" => EntryClass::Symlink,
        "160000" => EntryClass::Gitlink,
        _ => EntryClass::Absent,
    }
}

/// Record every submodule reference in the candidate tree.
pub fn collect_gitlinks(
    repository: &Path,
    tree: &str,
) -> Result<Vec<GitlinkRecord>, (HandoffOutcome, String)> {
    let output = run_git(repository, &["ls-tree", "-r", "-z", "--end-of-options", tree])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not list tree {tree}: {}", output.diagnostic()),
        ));
    }

    let mut gitlinks = Vec::new();
    for entry in output.stdout.split('\0').filter(|entry| !entry.is_empty()) {
        let Some((metadata, path)) = entry.split_once('\t') else {
            continue;
        };
        let columns: Vec<&str> = metadata.split_whitespace().collect();
        let [mode, kind, object] = columns.as_slice() else {
            continue;
        };
        if *mode == "160000" && *kind == "commit" {
            gitlinks.push(GitlinkRecord {
                path: path.to_string(),
                commit: (*object).to_string(),
                disposition: GitlinkDisposition::ReferencedNotTransported,
            });
        }
    }
    gitlinks.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(gitlinks)
}

/// Enumerate the bounded object closure the envelope must carry.
///
/// `--no-walk` stops at the candidate and its parents, so the envelope stays
/// proportional to one commit instead of the whole history, while still
/// carrying both sides of every diff the receiver recomputes.
fn enumerate_objects(
    repository: &Path,
    candidate: &CandidateIdentity,
) -> Result<Vec<String>, (HandoffOutcome, String)> {
    let mut arguments: Vec<&str> =
        vec!["rev-list", "--objects", "--no-walk", "--end-of-options", &candidate.commit];
    for parent in &candidate.parents {
        arguments.push(parent);
    }
    let output = run_git(repository, &arguments)
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::MissingObject,
            format!("could not enumerate candidate objects: {}", output.diagnostic()),
        ));
    }

    let mut ids: BTreeSet<String> = BTreeSet::new();
    for line in output.stdout.lines() {
        let id = line.split(' ').next().unwrap_or_default().trim();
        if is_full_object_id(id) {
            ids.insert(id.to_string());
        }
    }
    if !ids.contains(&candidate.commit) {
        return Err((
            HandoffOutcome::MissingObject,
            "the candidate commit object was not enumerated".to_string(),
        ));
    }
    Ok(ids.into_iter().collect())
}

fn build_pack(
    repository: &Path,
    object_ids: &[String],
) -> Result<Vec<u8>, (HandoffOutcome, String)> {
    let mut stdin = String::new();
    for id in object_ids {
        stdin.push_str(id);
        stdin.push('\n');
    }
    let output =
        run_git_with_stdin(repository, &["pack-objects", "--stdout", "-q"], stdin.as_bytes())
            .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not pack candidate objects: {}", output.diagnostic()),
        ));
    }
    if output.stdout_bytes.is_empty() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            "pack-objects produced no transport bytes".to_string(),
        ));
    }
    Ok(output.stdout_bytes)
}

/// Refuse to export content that would carry credentials across a boundary.
fn guard_retained_content(
    candidate: &CandidateIdentity,
    repository_identity: &RepositoryIdentity,
) -> Result<(), (HandoffOutcome, String)> {
    let mut fields: Vec<(&str, &str)> = vec![
        ("candidate.message", candidate.message.as_str()),
        ("candidate.author.name", candidate.author.name.as_str()),
        ("candidate.author.email", candidate.author.email.as_str()),
        ("candidate.committer.name", candidate.committer.name.as_str()),
        ("candidate.committer.email", candidate.committer.email.as_str()),
    ];
    if let Some(value) = &repository_identity.value {
        fields.push(("repository_identity.value", value.as_str()));
    }

    for (field, value) in fields {
        let findings = scan_secrets(field, value);
        if let Some(finding) = findings.first() {
            return Err((
                HandoffOutcome::UnsafeContent,
                format!(
                    "`{}` contains {} material; refusing to export",
                    finding.field, finding.kind
                ),
            ));
        }
    }
    Ok(())
}

/// A proof artifact and the bytes that back it.
struct PreparedProof {
    reference: ProofReference,
    bytes: Vec<u8>,
}

fn collect_proofs(
    paths: &[PathBuf],
    commit: &str,
) -> Result<Vec<PreparedProof>, (HandoffOutcome, String)> {
    let mut prepared: Vec<PreparedProof> = Vec::new();
    for path in paths {
        let bytes = fs::read(path).map_err(|error| {
            (
                HandoffOutcome::InstrumentFailure,
                format!("could not read proof artifact `{}`: {error}", path.display()),
            )
        })?;

        let id = proof_id_for(path)?;
        if prepared.iter().any(|existing| existing.reference.id == id) {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("two proof artifacts resolve to the same id `{id}`"),
            ));
        }

        // A proof that names a different candidate is stale evidence, not
        // this candidate's proof, and must not be silently rebound.
        if let Some(declared) = declared_proof_subject(&bytes)
            && declared != commit
        {
            return Err((
                HandoffOutcome::ProofSubjectMismatch,
                format!("proof `{id}` names candidate {declared}, not {commit}"),
            ));
        }

        let text = String::from_utf8_lossy(&bytes);
        if let Some(finding) = scan_secrets(&format!("proof.{id}"), &text).first() {
            return Err((
                HandoffOutcome::UnsafeContent,
                format!("proof `{id}` contains {} material; refusing to export", finding.kind),
            ));
        }

        prepared.push(PreparedProof {
            reference: ProofReference {
                path: format!("{PROOF_DIR_NAME}/{id}"),
                bytes: bytes.len() as u64,
                sha256: content_digest_hex(&bytes),
                candidate_subject: commit.to_string(),
                id,
            },
            bytes,
        });
    }
    prepared.sort_by(|left, right| left.reference.id.cmp(&right.reference.id));
    Ok(prepared)
}

fn proof_id_for(path: &Path) -> Result<String, (HandoffOutcome, String)> {
    let raw =
        path.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    let id: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if !is_proof_id(&id) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("`{}` does not yield a stable proof id", path.display()),
        ));
    }
    Ok(id)
}

/// Read the candidate a JSON proof artifact claims to be about.
///
/// Non-JSON and subject-free artifacts return `None`: they are carried as
/// opaque evidence bound by the manifest, not silently rejected.
pub fn declared_proof_subject(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    for key in PROOF_SUBJECT_KEYS {
        if let Some(text) = object.get(*key).and_then(serde_json::Value::as_str)
            && is_full_object_id(text)
        {
            return Some(text.to_string());
        }
    }
    None
}

/// Write the envelope into a staging directory beside its destination.
///
/// The staging directory is a sibling so the later rename stays on one
/// filesystem and is therefore atomic. The receipt is deliberately written
/// with the pending self-check value here; only [`StagedEnvelope::publish`]
/// records a validated one.
fn stage_envelope(
    out: &Path,
    manifest: &Manifest,
    pack_bytes: &[u8],
    proofs: &[PreparedProof],
) -> Result<StagedEnvelope, (HandoffOutcome, String)> {
    let io = |error: std::io::Error, what: &str| {
        (HandoffOutcome::InstrumentFailure, format!("could not write {what}: {error}"))
    };

    if out.exists() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("`{}` already exists; a handoff envelope is immutable", out.display()),
        ));
    }
    let Some(file_name) = out.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("`{}` is not a usable envelope destination", out.display()),
        ));
    };
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| io(error, "the destination directory"))?;

    let staging = parent.join(format!(".{file_name}.staging-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| io(error, "a stale staging directory"))?;
    }
    fs::create_dir_all(&staging).map_err(|error| io(error, "the staging directory"))?;
    let staged = StagedEnvelope { directory: staging };

    let write =
        |relative: &Path, bytes: &[u8], what: &str| -> Result<(), (HandoffOutcome, String)> {
            fs::write(staged.directory.join(relative), bytes).map_err(|error| io(error, what))
        };

    let manifest_json =
        canonical_json(manifest).map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    write(Path::new(MANIFEST_FILE_NAME), manifest_json.as_bytes(), MANIFEST_FILE_NAME)?;
    write(Path::new(PACK_FILE_NAME), pack_bytes, PACK_FILE_NAME)?;

    if !proofs.is_empty() {
        let proof_dir = staged.directory.join(PROOF_DIR_NAME);
        fs::create_dir_all(&proof_dir).map_err(|error| io(error, "the proof directory"))?;
        for proof in proofs {
            fs::write(proof_dir.join(&proof.reference.id), &proof.bytes)
                .map_err(|error| io(error, "a proof artifact"))?;
        }
    }

    let receipt = ProducerReceipt {
        schema_version: HANDOFF_RECEIPT_SCHEMA_V1.to_string(),
        candidate_identity_digest: manifest.candidate_identity_digest.clone(),
        candidate_commit: manifest.candidate.commit.clone(),
        producer_self_check: SELF_CHECK_PENDING.to_string(),
        limitations: manifest.limitations.clone(),
    };
    let receipt_json =
        canonical_json(&receipt).map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    write(Path::new(RECEIPT_FILE_NAME), receipt_json.as_bytes(), RECEIPT_FILE_NAME)?;

    Ok(staged)
}
