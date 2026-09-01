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

use super::check::{CheckReport, check_staged, describe_failure};
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
    create_handoff_with_validator(request, check_staged)
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
///
/// Cleanup is owned by the type rather than written at each error site. Every
/// failure between creating the staging directory and renaming it — a write
/// error, a refused validation, an early return added later — drops this value
/// and removes the directory, so a partially written envelope cannot survive a
/// failed export and be mistaken for a real one.
pub(super) struct StagedEnvelope {
    directory: PathBuf,
    published: bool,
}

impl Drop for StagedEnvelope {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

impl StagedEnvelope {
    pub(super) fn path(&self) -> &Path {
        &self.directory
    }

    /// Construct a staging handle directly, for proving the cleanup invariant.
    ///
    /// The production path always goes through `stage_envelope`; this exists so
    /// the ownership rule can be tested at the point that owns it.
    #[cfg(test)]
    pub(super) fn for_test(directory: PathBuf, published: bool) -> Self {
        Self { directory, published }
    }

    /// Drop the staging directory without publishing it.
    ///
    /// The removal itself happens in `Drop`; naming the intent at the call site
    /// keeps the refusal path readable.
    fn discard(self) {}

    /// Write the validated receipt, then move the directory into place.
    ///
    /// The rename is the publication step and is atomic on the destination's
    /// own filesystem, so a reader never observes a half-written envelope.
    fn publish(
        mut self,
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
        // Both failure paths leave `published` false, so `Drop` removes the
        // staging directory on the way out.
        if let Err(error) = fs::write(self.directory.join(RECEIPT_FILE_NAME), receipt_json) {
            return Err(failed(error, "write the validated receipt"));
        }
        if let Err(error) = fs::rename(&self.directory, destination) {
            return Err(failed(error, "publish the envelope"));
        }

        // The directory no longer exists under its staging name; claiming it
        // does would make `Drop` remove whatever later occupies that path.
        self.published = true;
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

    // Read the bytes, not Git's lossy text projection. `GitOutput::stdout` is
    // built with `from_utf8_lossy`, so a commit whose message or identity is
    // Latin-1 or otherwise non-UTF-8 would arrive with U+FFFD substituted and
    // be retained under a field documented as verbatim. The validator recomputes
    // through the same reader, so the two would agree and the envelope would
    // pass while misrepresenting the candidate. Refusing is the honest outcome,
    // and it matches how non-UTF-8 paths are already handled.
    let text = std::str::from_utf8(&output.stdout_bytes).map_err(|_| {
        (
            HandoffOutcome::UnsupportedObjectClass,
            format!(
                "commit {commit} carries non-UTF-8 metadata, which this format cannot retain verbatim"
            ),
        )
    })?;

    let mut lines = text.splitn(9, '\n');
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
    // `--format=%B` terminates its output with a newline of its own, so the
    // final field carries one more than the commit object's body. Dropping it
    // keeps `message` byte-identical to `git cat-file commit`'s body, which is
    // what "preserved verbatim" has to mean for a receiver that reconstructs
    // the identity from the raw object.
    let raw_message = next("message")?;
    let message = raw_message.strip_suffix('\n').unwrap_or(&raw_message).to_string();

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
    let tree = output.stdout.trim().to_string();
    // This value becomes a declared identity covered by the semantic digest,
    // so it gets the same canonical-id check its sibling readers apply.
    if !is_full_object_id(&tree) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("git returned a non-canonical tree id for {commit}"),
        ));
    }
    Ok(tree)
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
            // The caller named an `owner/name`, not a host. Inventing one would
            // turn their declaration into an observation.
            host: None,
            source: RepositoryIdentitySource::CallerDeclared,
        });
    }

    let output = run_git(repository, &["config", "--get", "remote.origin.url"])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if output.succeeded() {
        match repository_identity_from_remote(output.stdout.trim()) {
            Ok(Some(identity)) => {
                return Ok(RepositoryIdentity {
                    status: RepositoryIdentityStatus::Observed,
                    value: Some(identity.value),
                    host: Some(identity.host),
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
        host: None,
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

    let mut changes = parse_raw_diff(&raw.stdout_bytes)?;
    changes.sort_by(|left, right| {
        left.path.cmp(&right.path).then_with(|| left.old_path.cmp(&right.old_path))
    });

    let gitlinks = collect_gitlinks(repository, &candidate.tree)?;

    Ok(ChangeInventory { base_parent, changes, gitlinks })
}

/// Parse `git diff-tree --raw -z` records from raw bytes.
///
/// The NUL-delimited form is the only one safe for the paths this envelope
/// must carry: quoted output would re-encode Unicode and whitespace-heavy
/// paths. Parsing the exact bytes rather than a lossy string keeps that
/// guarantee — a lossy conversion would silently substitute replacement
/// characters and the manifest would then claim a path the tree does not have.
fn parse_raw_diff(stdout: &[u8]) -> Result<Vec<ChangeRecord>, (HandoffOutcome, String)> {
    let malformed = |detail: &str| {
        (HandoffOutcome::InstrumentFailure, format!("malformed raw diff record: {detail}"))
    };
    let mut fields = stdout.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let mut records = Vec::new();

    while let Some(header) = fields.next() {
        let header = decode_utf8(header, "a raw diff header")?;
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
            (
                Some(decode_utf8(source, "a rename source path")?),
                decode_utf8(destination, "a rename destination path")?,
            )
        } else {
            let single = fields.next().ok_or_else(|| malformed("path missing"))?;
            (None, decode_utf8(single, "a changed path")?)
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
            entry_class: entry_class_for(new_mode)?,
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

/// Decode one Git-reported byte field, refusing what this envelope cannot
/// represent.
///
/// A manifest is JSON, so a non-UTF-8 path has no faithful representation.
/// Refusing with a typed outcome is honest; a lossy conversion would put a
/// path in the inventory that does not exist in the tree, and the digest would
/// then commit to that false claim.
fn decode_utf8(bytes: &[u8], what: &str) -> Result<String, (HandoffOutcome, String)> {
    std::str::from_utf8(bytes).map(str::to_string).map_err(|_| {
        (
            HandoffOutcome::UnsupportedObjectClass,
            format!("{what} is not valid UTF-8 and cannot be represented in this envelope"),
        )
    })
}

/// Classify the candidate-side entry, refusing a mode this format does not
/// model.
///
/// Only `000000` means the entry is absent. Mapping every unrecognised mode to
/// `Absent` would record an unmodelled entry as a deletion, and the semantic
/// digest would commit to that as a fact.
pub fn entry_class_for(mode: &str) -> Result<EntryClass, (HandoffOutcome, String)> {
    match mode {
        "100644" => Ok(EntryClass::RegularFile),
        "100755" => Ok(EntryClass::ExecutableFile),
        "120000" => Ok(EntryClass::Symlink),
        "160000" => Ok(EntryClass::Gitlink),
        "000000" => Ok(EntryClass::Absent),
        other => Err((
            HandoffOutcome::UnsupportedObjectClass,
            format!("mode `{other}` is not an entry class this format can carry"),
        )),
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
    // Parse the exact bytes for the same reason the diff parser does: a
    // gitlink path must reach the manifest as the tree spells it.
    for entry in output.stdout_bytes.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let Some(separator) = entry.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let metadata = decode_utf8(&entry[..separator], "an ls-tree entry")?;
        let columns: Vec<&str> = metadata.split_whitespace().collect();
        let [mode, kind, object] = columns.as_slice() else {
            continue;
        };
        if *mode == "160000" && *kind == "commit" {
            if !is_full_object_id(object) {
                return Err((
                    HandoffOutcome::InstrumentFailure,
                    format!("tree {tree} reported a non-canonical gitlink object id"),
                ));
            }
            let path = decode_utf8(&entry[separator + 1..], "a gitlink path")?;
            gitlinks.push(GitlinkRecord {
                path,
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
    let staged = StagedEnvelope { directory: staging, published: false };

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
