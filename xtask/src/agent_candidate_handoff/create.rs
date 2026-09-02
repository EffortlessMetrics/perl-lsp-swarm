//! Producer for `agent_candidate_handoff.v1` envelopes.
//!
//! The producer never reads the worktree. Everything it transports comes from
//! committed objects, so dirty files, ignored files, credentials on disk, and
//! untracked scratch state cannot enter an envelope even by accident: an
//! untracked file becomes transportable only once it is committed into the
//! exact tree.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::check::{CheckReport, check_staged, describe_failure};
use super::git::{git_version, is_full_object_id, run_git, run_git_with_stdin};
use super::hygiene::{
    is_proof_id, is_repository_identity, repository_identity_from_remote, scan_secrets,
    strip_url_userinfo,
};
use super::model::{
    CandidateIdentity, ChangeInventory, ChangeRecord, ChangeStatus, CommitPerson, EntryClass,
    GITLINK_MODE, GitlinkDisposition, GitlinkRecord, HANDOFF_MANIFEST_SCHEMA_V1,
    HANDOFF_RECEIPT_SCHEMA_V1, LimitationCode, MANIFEST_FILE_NAME, Manifest, PACK_FILE_NAME,
    PROOF_DIR_NAME, ProducerObservation, ProducerReceipt, ProofReference, RECEIPT_FILE_NAME,
    RepositoryIdentity, RepositoryIdentitySource, RepositoryIdentityStatus, Transport,
    TransportFile, TransportFormat,
};
use super::{HandoffOutcome, canonical_json, content_digest_hex};

/// JSON keys a proof artifact may use to name the candidate it proves.
pub const PROOF_SUBJECT_KEYS: &[&str] =
    &["commit", "candidate", "candidate_commit", "subject", "head_sha", "sha"];

/// Ceiling on one declared proof artifact.
///
/// Proof is evidence a human or a sibling tool reads, not bulk data, so this
/// is generous for its purpose and still bounded.
pub const MAX_PROOF_BYTES: u64 = 32 * 1024 * 1024;

/// Ceiling on the number of proof artifacts one envelope may carry.
///
/// Matches the validator's own limit, so the producer cannot build an envelope
/// its consumer would refuse to read.
pub const MAX_PROOFS: usize = 256;

/// Ceiling on the total bytes of all proof artifacts together.
///
/// The per-artifact ceiling alone bounds nothing in aggregate: the count limit
/// times the size limit is eight gigabytes, and every artifact is held in
/// memory until the envelope is staged. This is the bound that actually keeps
/// a legitimate-looking set of inputs from exhausting the producer.
pub const MAX_TOTAL_PROOF_BYTES: u64 = 128 * 1024 * 1024;

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

    let toplevel = run_git(repository, &["rev-parse", "--show-toplevel"]).map_err(&instrument)?;
    if !toplevel.succeeded() {
        // Carry Git's own diagnostic. Reporting only "not inspectable" pointed
        // the operator away from causes like dubious ownership, which name
        // themselves precisely if you let them.
        return Err(instrument(format!(
            "the requested path is not an inspectable Git worktree: {}",
            toplevel.diagnostic()
        )));
    }
    require_supported_object_format(repository)?;

    let commit = resolve_commit(repository, &request.candidate)?;
    let candidate = read_commit_identity(repository, &commit)?;
    let mut limitations: BTreeSet<LimitationCode> = BTreeSet::new();
    limitations.insert(LimitationCode::LocalProofOnly);
    limitations.insert(LimitationCode::TransportBytesNotVersionStable);
    limitations.insert(LimitationCode::TransportedObjectsNotSecretScanned);
    limitations.insert(LimitationCode::InventoryRenamesAreDetected);
    limitations.insert(LimitationCode::RepositoryIdentityNotReceiverVerifiable);
    if candidate.is_root_commit {
        limitations.insert(LimitationCode::RootCommitDiffAgainstEmptyTree);
    }
    if candidate.is_merge_commit {
        limitations.insert(LimitationCode::MergeCommitDiffAgainstFirstParent);
    }

    let repository_identity = resolve_repository_identity(repository, request, &mut limitations)?;
    let inventory = build_inventory(repository, &candidate)?;
    if inventory.references_untransported_gitlink() {
        limitations.insert(LimitationCode::SubmoduleGitlinkNotTransported);
    }

    let object_ids = enumerate_objects(repository, &candidate)?;
    let pack_bytes = build_pack(repository, &object_ids)?;

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

    // Refuse before writing anything: a secret must not reach disk in an
    // envelope that a later step might still hand onward.
    //
    // This is the validator's own scanner, not a producer-side reimplementation
    // of it. Two field lists drifted the moment one side grew a field the other
    // did not — a credential-shaped inventory path was scanned by `check` and
    // not by `create`, so it reached disk and was only refused afterwards by the
    // staged self-check. One function means the producer cannot write what the
    // validator would reject.
    if let Some(detail) = super::check::find_unsafe_content(&manifest) {
        return Err((HandoffOutcome::UnsafeContent, format!("{detail}; refusing to export")));
    }

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

/// Refuse a repository whose object format this version cannot represent.
///
/// `agent_candidate_handoff.v1` fixes object ids at 40 hex characters,
/// throughout the manifest and its digest. A SHA-256 repository's ids are
/// perfectly canonical at 64, so reporting them as malformed would blame the
/// repository for a limit of this format. Naming the limit is the honest
/// refusal, and it leaves room for a v2 that carries the hash algorithm.
fn require_supported_object_format(repository: &Path) -> Result<(), (HandoffOutcome, String)> {
    let output = run_git(repository, &["rev-parse", "--show-object-format"])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    // Git versions without the flag predate SHA-256 repositories entirely, so
    // silence here means SHA-1.
    if !output.succeeded() {
        return Ok(());
    }
    let stdout = output.stdout();
    let format = stdout.trim();
    if format.is_empty() || format == "sha1" {
        return Ok(());
    }
    Err((
        HandoffOutcome::UnsupportedObjectClass,
        format!(
            "this repository uses the `{format}` object format; \
             `agent_candidate_handoff.v1` represents 40-hex SHA-1 object ids only"
        ),
    ))
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
    /// Remove the staging directory unless it was published.
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

impl StagedEnvelope {
    /// Path of the staging directory, for validating it in place.
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

/// Resolve `revision` to a full commit object id.
///
/// Abbreviated ids are refused everywhere identity is claimed, so the
/// result is checked for canonical form rather than assumed.
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
    let sha = output.stdout().trim().to_string();
    if !is_full_object_id(&sha) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("git returned a non-canonical object id for `{revision}`"),
        ));
    }
    Ok(sha)
}

/// The commit header fields this format retains.
pub(super) struct CommitHeaders {
    /// Tree the commit names.
    pub tree: String,
    /// Parent ids, in the order the object records them.
    pub parents: Vec<String>,
    /// Authoring identity and date.
    pub author: CommitPerson,
    /// Committing identity and date.
    pub committer: CommitPerson,
}

/// Read `tree`, `parent`, `author`, and `committer` out of a commit's header block.
///
/// Separate from [`read_commit_identity`] so the refusal below is reachable
/// from a test: `git cat-file commit` cannot emit a header line without a
/// space, so no fixture can drive that branch through a real repository.
///
/// Skipping such a line would silently drop a `parent`, and the consequence is
/// worse here than a missing field would be. `check` reruns this same function
/// against the imported objects, so producer and validator would shrink the
/// parent list identically and agree with each other while disagreeing with the
/// commit — leaving `is_merge_commit`, the ordered parents, and the object
/// closure derived from a candidate that was never read correctly. Unexpected
/// output is refused instead.
pub(super) fn parse_commit_headers(
    headers: &str,
    commit: &str,
) -> Result<CommitHeaders, (HandoffOutcome, String)> {
    let mut tree: Option<String> = None;
    let mut parents: Vec<String> = Vec::new();
    let mut author: Option<CommitPerson> = None;
    let mut committer: Option<CommitPerson> = None;
    for line in headers.lines() {
        // A multi-line header (`gpgsig`) continues with a leading space. None
        // of the fields read here are multi-line, so continuations are skipped
        // rather than mistaken for a new header. This is the one legitimate
        // skip: the continuation belongs to a header already read.
        if line.starts_with(' ') {
            continue;
        }
        let Some((name, value)) = line.split_once(' ') else {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("commit {commit} reported a header record that is not `name value`"),
            ));
        };
        match name {
            "tree" => tree = Some(value.to_string()),
            "parent" => parents.push(value.to_string()),
            "author" => author = Some(parse_commit_person(value, commit, "author")?),
            "committer" => committer = Some(parse_commit_person(value, commit, "committer")?),
            // Headers this format does not retain — `gpgsig`, `encoding`,
            // `mergetag`. Ignoring a header whose name was read is not the same
            // as ignoring a record that could not be read at all.
            _ => {}
        }
    }

    let instrument = |what: &str| {
        (HandoffOutcome::InstrumentFailure, format!("commit {commit} did not report `{what}`"))
    };
    Ok(CommitHeaders {
        tree: tree.ok_or_else(|| instrument("tree"))?,
        parents,
        author: author.ok_or_else(|| instrument("author"))?,
        committer: committer.ok_or_else(|| instrument("committer"))?,
    })
}

/// Read one commit's complete identity from a repository or object database.
///
/// The commit *object* is the source, not `git show`'s formatted projection.
/// That distinction is load-bearing for a format whose manifest documents these
/// fields as verbatim: `git show` applies the commit's `encoding` header and
/// transcodes the message to UTF-8, and normalises date tokens, so a Latin-1
/// commit body would reach the manifest as different bytes than the object
/// holds. The validator reruns this exact function against the imported
/// objects, so the two would agree with each other and disagree with the
/// candidate — a mismatch no dimension could see.
///
/// Everything retained must still be UTF-8, because the manifest is JSON.
/// A commit whose object bytes are not UTF-8 is refused rather than mangled.
pub fn read_commit_identity(
    repository: &Path,
    commit: &str,
) -> Result<CandidateIdentity, (HandoffOutcome, String)> {
    let output = run_git(repository, &["cat-file", "commit", "--end-of-options", commit])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if !output.succeeded() {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("could not read commit {commit}: {}", output.diagnostic()),
        ));
    }

    let raw = output.stdout_bytes.as_slice();
    // The header block ends at the first empty line; everything after it is the
    // message, byte for byte, including any trailing newline the object has.
    let (header_bytes, message_bytes) = split_commit_object(raw).ok_or_else(|| {
        (
            HandoffOutcome::UnsupportedObjectClass,
            format!("commit {commit} has no header/message separator"),
        )
    })?;

    let unsupported = |what: &str| {
        (
            HandoffOutcome::UnsupportedObjectClass,
            format!(
                "commit {commit} carries non-UTF-8 {what}, which this format cannot retain verbatim"
            ),
        )
    };
    let headers = std::str::from_utf8(header_bytes).map_err(|_| unsupported("headers"))?;
    let message =
        std::str::from_utf8(message_bytes).map_err(|_| unsupported("message"))?.to_string();

    let CommitHeaders { tree, parents, author, committer } = parse_commit_headers(headers, commit)?;

    if !is_full_object_id(&tree) {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("commit {commit} reported a non-canonical tree id"),
        ));
    }
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
        author,
        committer,
    })
}

/// Split a raw commit object into its header block and its message bytes.
///
/// Returns `None` when no blank line separates the two, which is not a
/// well-formed commit object.
fn split_commit_object(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    if let Some(rest) = raw.strip_prefix(b"\n") {
        return Some((&[], rest));
    }
    raw.windows(2).position(|pair| pair == b"\n\n").map(|index| (&raw[..index], &raw[index + 2..]))
}

/// Parse one `NAME <EMAIL> <unix seconds> <tz offset>` identity line.
///
/// The date is kept as the object's own two tokens rather than a reformatted
/// value, so an unusual but valid timestamp survives the round trip unchanged.
fn parse_commit_person(
    value: &str,
    commit: &str,
    role: &str,
) -> Result<CommitPerson, (HandoffOutcome, String)> {
    let malformed = || {
        (
            HandoffOutcome::UnsupportedObjectClass,
            format!("commit {commit} has a malformed `{role}` identity line"),
        )
    };
    let open = value.rfind(" <").ok_or_else(malformed)?;
    let close = value[open..].find('>').map(|index| open + index).ok_or_else(malformed)?;
    let name = value[..open].to_string();
    let email = value[open + 2..close].to_string();
    let date = value[close + 1..].trim().to_string();
    if date.is_empty() {
        return Err(malformed());
    }
    Ok(CommitPerson { name, email, date })
}

/// Read the tree id of `commit`, refusing a non-canonical answer.
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
    let tree = output.stdout().trim().to_string();
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

/// Establish which repository the candidate belongs to, or prove none.
///
/// The observation is preferred and the caller's declaration is the fallback,
/// which is what `CreateRequest` documents: a declaration is a weaker claim,
/// and taking it while a readable remote sat right there could put a wrong
/// repository in the manifest for a consumer to publish to.
///
/// A declaration that *contradicts* a readable remote is refused rather than
/// silently resolved either way. The caller's intent and the workspace
/// disagree, and quietly picking one of them is the failure this field exists
/// to prevent.
fn resolve_repository_identity(
    repository: &Path,
    request: &CreateRequest,
    limitations: &mut BTreeSet<LimitationCode>,
) -> Result<RepositoryIdentity, (HandoffOutcome, String)> {
    let declared = match &request.declared_repository_identity {
        Some(declared) => {
            let normalized = declared.trim().to_lowercase();
            if !is_repository_identity(&normalized) {
                return Err((
                    HandoffOutcome::InstrumentFailure,
                    format!("`{declared}` is not a lowercase owner/name repository identity"),
                ));
            }
            Some(normalized)
        }
        None => None,
    };

    let output = run_git(repository, &["config", "--get", "remote.origin.url"])
        .map_err(|error| (HandoffOutcome::InstrumentFailure, error))?;
    if output.succeeded() {
        match repository_identity_from_remote(output.stdout().trim()) {
            Ok(Some(identity)) => {
                if let Some(declared) = &declared
                    && *declared != identity.value
                {
                    return Err((
                        HandoffOutcome::InstrumentFailure,
                        format!(
                            "the configured remote names `{}` but `{declared}` was declared; \
                             refusing rather than choosing one",
                            identity.value
                        ),
                    ));
                }
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
                //
                // The identity inside such a URL is still readable, and a
                // caller's declaration must not silently disagree with it:
                // standing in a clone of `acme/app` and stamping
                // `totally/unrelated` is the very substitution the conflict
                // rule above exists to stop, and a credential-bearing remote is
                // the ordinary shape for the workspaces this format targets.
                // Comparing an `owner/name` is not retaining a URL.
                if let Some(declared) = &declared
                    && let Ok(Some(identity)) =
                        repository_identity_from_remote(&strip_url_userinfo(output.stdout().trim()))
                    && *declared != identity.value
                {
                    return Err((
                        HandoffOutcome::InstrumentFailure,
                        format!(
                            "the configured remote names `{}` but `{declared}` was declared; \
                             refusing rather than choosing one",
                            identity.value
                        ),
                    ));
                }
                limitations.insert(LimitationCode::RemoteUrlContainedCredentials);
            }
        }
    }

    // No remote could be read. This is where a declaration belongs.
    if let Some(value) = declared {
        return Ok(RepositoryIdentity {
            status: RepositoryIdentityStatus::Declared,
            value: Some(value),
            // The caller named an `owner/name`, not a host. Inventing one would
            // turn their declaration into an observation.
            host: None,
            source: RepositoryIdentitySource::CallerDeclared,
        });
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

/// Split a raw status field into its letter and rename/copy score.
fn split_status(field: &str) -> (char, Option<u32>) {
    let mut characters = field.chars();
    let letter = characters.next().unwrap_or('?');
    let score = characters.as_str();
    (letter, if score.is_empty() { None } else { score.parse::<u32>().ok() })
}

/// Absent-side modes are all zeroes; report them as no mode at all.
fn mode_option(mode: &str) -> Option<String> {
    (mode != "000000").then(|| mode.to_string())
}

/// Absent-side object ids are all zeroes; report them as no object.
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
        GITLINK_MODE => Ok(EntryClass::Gitlink),
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
    // A record this reader cannot parse is unexpected Git output, not an
    // absent submodule. Skipping it would silently shrink the derived set: a
    // dropped `160000` row leaves `gitlinks` empty, the manifest never declares
    // `SubmoduleGitlinkNotTransported`, and an envelope that quietly omits a
    // submodule reference reads as one that had none. The validator already
    // treats the identical condition as an instrument failure.
    let malformed = || {
        (
            HandoffOutcome::InstrumentFailure,
            format!("tree {tree} produced an unreadable entry record"),
        )
    };
    for entry in output.stdout_bytes.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let Some(separator) = entry.iter().position(|byte| *byte == b'\t') else {
            return Err(malformed());
        };
        let metadata = decode_utf8(&entry[..separator], "an ls-tree entry")?;
        let columns: Vec<&str> = metadata.split_whitespace().collect();
        let [mode, kind, object] = columns.as_slice() else {
            return Err(malformed());
        };
        if *mode == GITLINK_MODE && *kind == "commit" {
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
    // `--no-object-names` is load-bearing, not a tidiness flag. Without it
    // `rev-list --objects` emits `<id> <path>` lines, and a tracked path may
    // contain a newline: the text after it becomes its own line, and if that
    // text begins with forty hex characters it parses as another transported
    // object. The candidate would then declare an object its closure does not
    // contain, and `check` would refuse an envelope for a candidate that was
    // never malformed. Asking Git not to print paths removes the ambiguity at
    // the source rather than trying to out-parse it.
    let mut arguments: Vec<&str> = vec![
        "rev-list",
        "--objects",
        "--no-object-names",
        "--no-walk",
        "--end-of-options",
        &candidate.commit,
    ];
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

    let ids = parse_object_records(&output.stdout())?;
    if !ids.contains(&candidate.commit) {
        return Err((
            HandoffOutcome::MissingObject,
            "the candidate commit object was not enumerated".to_string(),
        ));
    }
    Ok(ids.into_iter().collect())
}

/// Read `rev-list --objects --no-object-names` records into a declared object set.
///
/// Separate from [`enumerate_objects`] so the refusal below is reachable from a
/// test. With `--no-object-names` in place Git cannot emit a record that is not
/// an object id, so driving this through a real repository can never exercise
/// the failing branch; a rule that cannot be executed is not a proven rule.
///
/// Every record is an object id and nothing else, so a record that is not one is
/// unexpected Git output rather than something to step over. Skipping it would
/// silently shrink the declared set — the same failure this module refuses in
/// the inventory and gitlink readers — and an incomplete `object_ids` is exactly
/// what makes an envelope validate as a candidate it does not carry.
pub(super) fn parse_object_records(
    stdout: &str,
) -> Result<BTreeSet<String>, (HandoffOutcome, String)> {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for line in stdout.lines() {
        let id = line.trim();
        if id.is_empty() {
            continue;
        }
        if !is_full_object_id(id) {
            return Err((
                HandoffOutcome::InstrumentFailure,
                "object enumeration produced a record that is not an object id".to_string(),
            ));
        }
        ids.insert(id.to_string());
    }
    Ok(ids)
}

/// Pack the candidate's object closure into transport bytes.
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
        run_git_with_stdin(repository, &["pack-objects", "--stdout", "-q"], stdin.into_bytes())
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

/// A proof artifact and the bytes that back it.
pub(super) struct PreparedProof {
    reference: ProofReference,
    bytes: Vec<u8>,
}

/// Read, bound, and subject-bind each declared proof artifact.
///
/// A proof naming a different candidate is stale evidence rather than this
/// candidate's proof, and is refused rather than silently rebound.
fn collect_proofs(
    paths: &[PathBuf],
    commit: &str,
) -> Result<Vec<PreparedProof>, (HandoffOutcome, String)> {
    collect_proofs_within(paths, commit, &ProofBudget::FORMAT)
}

/// The ceilings `collect_proofs` enforces.
///
/// Taken as a value rather than read from the constants directly so the bound
/// can be exercised at a size a test can actually reach. Proving the aggregate
/// ceiling against the format's own 128 MiB would mean writing and buffering
/// 128 MiB of adversarially-sized artifacts inside a unit test; proving it at
/// eight bytes exercises the same arithmetic. `ProofBudget::FORMAT` is the only
/// budget production ever uses.
pub(super) struct ProofBudget {
    /// Most artifacts accepted in one export.
    pub(super) max_count: usize,
    /// Most bytes retained from any single artifact.
    pub(super) max_each: u64,
    /// Most bytes retained across every artifact together.
    pub(super) max_total: u64,
}

impl ProofBudget {
    /// The ceilings `agent_candidate_handoff.v1` defines.
    pub(super) const FORMAT: Self =
        Self { max_count: MAX_PROOFS, max_each: MAX_PROOF_BYTES, max_total: MAX_TOTAL_PROOF_BYTES };
}

/// `collect_proofs` against an explicit budget.
pub(super) fn collect_proofs_within(
    paths: &[PathBuf],
    commit: &str,
    budget: &ProofBudget,
) -> Result<Vec<PreparedProof>, (HandoffOutcome, String)> {
    let max_count = budget.max_count;
    let max_each = budget.max_each;
    let max_total = budget.max_total;
    if paths.len() > max_count {
        return Err((
            HandoffOutcome::InstrumentFailure,
            format!("{} proof artifacts were supplied, above the {max_count} ceiling", paths.len()),
        ));
    }

    let mut prepared: Vec<PreparedProof> = Vec::new();
    let mut total_bytes: u64 = 0;
    for path in paths {
        // Check the size before the read, not after. A proof artifact is
        // caller-supplied and unbounded; reading it first and rejecting it
        // afterwards would already have allocated whatever it pointed at.
        // `symlink_metadata`, not `metadata`: a `--proof` argument pointing at
        // a link would otherwise be followed, copying whatever it targets into
        // the envelope. The secret scan catches recognised credential shapes,
        // but "a file the caller did not mean to publish" is a much larger set
        // than "a string that looks like a token", and an envelope is handed
        // onward. Refusing the link is the boundary; naming the file directly
        // is always available to a caller who means it.
        let io = |error: std::io::Error| {
            (
                HandoffOutcome::InstrumentFailure,
                format!("could not read proof artifact `{}`: {error}", path.display()),
            )
        };
        // Refuse a link on the path, because `File::open` would follow one.
        let linkage = fs::symlink_metadata(path).map_err(io)?;
        if linkage.file_type().is_symlink() {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!(
                    "proof artifact `{}` is a symbolic link; name the file itself so the \
                     envelope carries what the caller chose",
                    path.display()
                ),
            ));
        }

        // Everything after that is decided on the open handle. Checking the
        // path and then reading it again leaves a window in which the entry can
        // be replaced with a link, so the bytes copied into the envelope need
        // not be the bytes that were vetted — the same race the validator's own
        // reader closes.
        let mut file = fs::File::open(path).map_err(io)?;
        let metadata = file.metadata().map_err(io)?;
        if !metadata.is_file() {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("proof artifact `{}` is not a regular file", path.display()),
            ));
        }
        if metadata.len() > max_each {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!(
                    "proof artifact `{}` is {} bytes, above the {max_each}-byte ceiling",
                    path.display(),
                    metadata.len()
                ),
            ));
        }
        // Refuse on the declared sizes first, so a set that cannot possibly fit
        // is rejected before anything is read at all.
        if total_bytes.saturating_add(metadata.len()) > max_total {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!(
                    "the supplied proof artifacts total more than the {max_total}-byte ceiling"
                ),
            ));
        }

        // The aggregate ceiling has to bound the bytes actually *retained*, not
        // the bytes each file claimed before it was opened. Accounting
        // `metadata.len()` and then permitting a growing file to deliver up to
        // MAX_PROOF_BYTES let a set of files that measured as empty hand back
        // MAX_PROOFS × MAX_PROOF_BYTES, so the ceiling bounded nothing the
        // producer had to hold. Each read is therefore capped by whichever is
        // smaller: this file's own ceiling, or what the aggregate budget has
        // left. Reading one byte past that cap distinguishes "it fits" from
        // "it grew", so an artifact that changed under us is refused rather
        // than silently truncated into the envelope.
        let remaining = max_total.saturating_sub(total_bytes);
        let ceiling = max_each.min(remaining);
        let mut bytes = Vec::with_capacity(metadata.len().min(ceiling) as usize);
        file.by_ref().take(ceiling.saturating_add(1)).read_to_end(&mut bytes).map_err(io)?;
        let retained = bytes.len() as u64;
        if retained > max_each {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!(
                    "proof artifact `{}` grew past the {max_each}-byte ceiling while it was \
                     being read",
                    path.display()
                ),
            ));
        }
        if retained > remaining {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!(
                    "the supplied proof artifacts total more than the {max_total}-byte ceiling"
                ),
            ));
        }
        total_bytes = total_bytes.saturating_add(retained);

        let id = proof_id_for(path)?;
        if prepared.iter().any(|existing| existing.reference.id == id) {
            return Err((
                HandoffOutcome::InstrumentFailure,
                format!("two proof artifacts resolve to the same id `{id}`"),
            ));
        }

        // A proof that names a different candidate is stale evidence, not
        // this candidate's proof, and must not be silently rebound.
        match declared_proof_subject(&bytes) {
            ProofSubject::Stated(declared) if declared != commit => {
                return Err((
                    HandoffOutcome::ProofSubjectMismatch,
                    format!("proof `{id}` names candidate {declared}, not {commit}"),
                ));
            }
            ProofSubject::Conflicting => {
                return Err((
                    HandoffOutcome::ProofSubjectMismatch,
                    format!("proof `{id}` names more than one candidate"),
                ));
            }
            ProofSubject::Unusable => {
                return Err((
                    HandoffOutcome::ProofSubjectMismatch,
                    format!("proof `{id}` states a subject that is not a full object id"),
                ));
            }
            ProofSubject::Stated(_) | ProofSubject::Unstated => {}
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

/// Derive a proof's stable id from its file name.
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

/// What a proof artifact says about the candidate it proves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProofSubject {
    /// The artifact names no candidate: opaque evidence, bound by the manifest.
    Unstated,
    /// The artifact names exactly one candidate, consistently.
    Stated(String),
    /// The artifact names more than one candidate and contradicts itself.
    Conflicting,
    /// A recognised subject key holds a value that is not a canonical object
    /// id — abbreviated, uppercase, or malformed.
    Unusable,
}

/// Read the candidate a JSON proof artifact claims to be about.
///
/// Non-JSON and subject-free artifacts return `None`: they are carried as
/// opaque evidence bound by the manifest, not silently rejected.
pub fn declared_proof_subject(bytes: &[u8]) -> ProofSubject {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return ProofSubject::Unstated;
    };
    let Some(object) = value.as_object() else {
        return ProofSubject::Unstated;
    };

    let mut found: Option<String> = None;
    for key in PROOF_SUBJECT_KEYS {
        let Some(text) = object.get(*key).and_then(serde_json::Value::as_str) else {
            continue;
        };
        // A recognised key with a malformed value is *not* an absent subject.
        // Treating it as absent let the producer stamp the candidate onto
        // evidence that named something else in an abbreviated or uppercase
        // form — rebinding stale proof rather than refusing it.
        if !is_full_object_id(text) {
            return ProofSubject::Unusable;
        }
        match &found {
            // Stopping at the first recognised key let an artifact name this
            // candidate in one field and a different commit in another, and be
            // accepted on the strength of whichever came first.
            Some(existing) if existing != text => return ProofSubject::Conflicting,
            Some(_) => {}
            None => found = Some(text.to_string()),
        }
    }
    found.map_or(ProofSubject::Unstated, ProofSubject::Stated)
}

/// Ceiling on attempts to claim an unused staging directory name.
///
/// Each attempt fails only because another live export already holds that
/// name, so exhausting this many in a row means something other than
/// contention is wrong and the export should say so rather than spin.
const MAX_STAGING_ATTEMPTS: u32 = 1024;

/// Counter distinguishing concurrent exports inside one process.
static STAGING_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Claim a staging directory this invocation exclusively owns.
///
/// The name cannot be derived from the destination and the process id alone.
/// Two library calls in one process exporting to the same absent destination
/// both pass the `out.exists()` check — `out` stays absent for the whole of
/// staging, and only `publish` creates it — so they would derive one identical
/// path, and the second would delete the first's *live* directory and recreate
/// it. From there both wrote a manifest, pack, proofs, and receipt over each
/// other, and one caller could validate the directory while the other replaced
/// its bytes, so `create` could return success for an envelope that no longer
/// matched what was validated. That is the exact guarantee staging exists to
/// provide, so the name has to be unique per invocation rather than per
/// process.
///
/// `create_dir` is the allocation primitive because it is atomic: it fails with
/// `AlreadyExists` rather than joining a directory somebody else owns, so the
/// winner of a race is decided by the filesystem instead of by a check this
/// code performs and then acts on. Nothing here removes a directory it did not
/// create — a name already taken is skipped, never reclaimed — because a
/// directory that exists may belong to a live export.
///
/// The consequence is that a crashed producer leaves its staging directory
/// behind, since no later run will clear it. That is the deliberate trade:
/// a leaked temporary directory is recoverable by hand, and deleting another
/// export's live state is not. `StagedEnvelope`'s `Drop` still removes the one
/// this invocation created on every path except publication.
pub(super) fn allocate_staging(
    parent: &Path,
    file_name: &str,
) -> Result<PathBuf, (HandoffOutcome, String)> {
    let process = std::process::id();
    for _ in 0..MAX_STAGING_ATTEMPTS {
        let sequence = STAGING_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.staging-{process}-{sequence}"));
        return match fs::create_dir(&candidate) {
            Ok(()) => Ok(candidate),
            // Taken by another live export, or left by a crashed one. Either
            // way it is not this invocation's to use or to remove.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => Err((
                HandoffOutcome::InstrumentFailure,
                format!("could not write the staging directory: {error}"),
            )),
        };
    }
    Err((
        HandoffOutcome::InstrumentFailure,
        format!("could not claim a staging directory after {MAX_STAGING_ATTEMPTS} attempts"),
    ))
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

    let staging = allocate_staging(parent, &file_name)?;
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
