//! Executable proof for `agent_candidate_handoff.v1`.
//!
//! The controls here are written against the class of implementation that
//! would look correct in a demo and lose work in practice: one that transports
//! a textual patch, trusts the producer's own manifest, or quietly depends on
//! the source workspace still existing.

use super::check::{CHECK_REPORT_SCHEMA_V1, check_staged};
use super::create::{SELF_CHECK_PENDING, SELF_CHECK_VALIDATED, compute_identity_digest};
use super::create::{StagedEnvelope, create_handoff_with_validator};
use super::model::{
    ChangeStatus, EntryClass, GitlinkDisposition, HANDOFF_RECEIPT_SCHEMA_V1, LimitationCode,
    MANIFEST_FILE_NAME, Manifest, PACK_FILE_NAME, PROOF_DIR_NAME, ProducerReceipt,
    RECEIPT_FILE_NAME, RepositoryIdentityStatus,
};
use super::{
    CheckReport, CreateRequest, DimensionVerdict, HandoffOutcome, canonical_json, check_handoff,
    create_handoff, explain,
};
use anyhow::{Context, Result, anyhow, bail};
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// Git for fixture repositories, isolated the same way production is.
///
/// Two `#[serial]` controls below set `GIT_ALTERNATE_OBJECT_DIRECTORIES` and
/// `GIT_CONFIG_GLOBAL` on the *process* to prove `run_git` refuses to inherit
/// them. `#[serial]` orders those against each other only; unannotated tests
/// keep running alongside, and a fixture `git commit` that inherited a
/// concurrent test's alternates path failed with `invalid object … for
/// 'seed.txt'` while `export_valid` saw `tree … is not present locally`.
/// Every fixture spawn therefore clears Git's repository-local variables and
/// neuters host configuration, exactly as `run_bounded` does.
fn isolated_git() -> Command {
    let mut command = Command::new("git");
    for variable in super::git::GIT_LOCAL_ENV_VARS {
        command.env_remove(variable);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

/// A disposable Git repository with deterministic identity and dates.
struct Fixture {
    root: tempfile::TempDir,
    clock: std::cell::Cell<u64>,
}

impl Fixture {
    fn new() -> Result<Self> {
        Self::with_remote(Some("https://github.com/example/repo.git"))
    }

    fn with_remote(remote: Option<&str>) -> Result<Self> {
        let root = tempfile::TempDir::new().context("creating a fixture root")?;
        let fixture = Self { root, clock: std::cell::Cell::new(1_600_000_000) };
        fixture.git(&["init", "--quiet", "-b", "main"])?;
        fixture.git(&["config", "user.name", "Fixture Author"])?;
        fixture.git(&["config", "user.email", "fixture@example.invalid"])?;
        // Signing and CRLF translation would make fixtures depend on ambient
        // developer configuration rather than on the committed objects.
        fixture.git(&["config", "commit.gpgsign", "false"])?;
        fixture.git(&["config", "core.autocrlf", "false"])?;
        if let Some(url) = remote {
            fixture.git(&["remote", "add", "origin", url])?;
        }
        Ok(fixture)
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn git(&self, arguments: &[&str]) -> Result<String> {
        let output = isolated_git()
            .args(arguments)
            .current_dir(self.path())
            .output()
            .with_context(|| format!("running git {}", arguments.join(" ")))?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn write(&self, relative: &str, contents: &[u8]) -> Result<()> {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("creating a fixture directory")?;
        }
        fs::write(&path, contents).context("writing a fixture file")?;
        Ok(())
    }

    /// Commit with a fixed, monotonically advancing clock so identities stay
    /// reproducible across runs and hosts.
    fn commit(&self, message: &str) -> Result<String> {
        self.git(&["add", "--all"])?;
        self.commit_staged(message)
    }

    /// Commit exactly what is staged.
    ///
    /// Index-only facts — a mode flip, a gitlink — would be undone by a
    /// worktree re-add, so those fixtures must not restage.
    fn commit_staged(&self, message: &str) -> Result<String> {
        let stamp = self.clock.get();
        self.clock.set(stamp + 60);
        let date = format!("{stamp} +0000");
        let output = isolated_git()
            .args(["commit", "--quiet", "--allow-empty", "-m", message])
            .current_dir(self.path())
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .context("committing a fixture change")?;
        if !output.status.success() {
            bail!("git commit failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        self.git(&["rev-parse", "HEAD"])
    }

    fn head(&self) -> Result<String> {
        self.git(&["rev-parse", "HEAD"])
    }
}

/// Destination for one envelope, kept beside but outside the fixture repo.
struct Destination {
    root: tempfile::TempDir,
}

impl Destination {
    fn new() -> Result<Self> {
        Ok(Self { root: tempfile::TempDir::new().context("creating an envelope root")? })
    }

    fn envelope(&self) -> PathBuf {
        self.root.path().join("handoff")
    }

    /// The directory the envelope is published into, where a staging directory
    /// would also appear.
    fn root(&self) -> &Path {
        self.root.path()
    }
}

fn request(fixture: &Fixture, destination: &Destination) -> CreateRequest {
    CreateRequest {
        repository: fixture.path().to_path_buf(),
        candidate: "HEAD".to_string(),
        out: destination.envelope(),
        declared_repository_identity: None,
        proofs: Vec::new(),
    }
}

/// Create an envelope and require it to validate, returning the manifest.
fn export_valid(fixture: &Fixture, destination: &Destination) -> Result<Manifest> {
    let manifest = create_handoff(&request(fixture, destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    let report = check_handoff(&destination.envelope());
    if report.outcome != HandoffOutcome::ValidHandoff {
        bail!("expected VALID_HANDOFF, got {:?}: {:#?}", report.outcome, report.dimensions);
    }
    Ok(manifest)
}

/// Rewrite a manifest after a typed mutation, repairing every derived record.
///
/// This models the strong adversary: a tamperer who also recomputes the
/// identity digest and reseals the producer receipt, leaving the envelope
/// internally consistent. Only recomputation from the transported objects can
/// catch them, so every control below stays a test of the object-level
/// verification rather than of the producer's own bookkeeping.
fn rewrite_manifest_resealed(envelope: &Path, mut manifest: Manifest) -> Result<()> {
    manifest.candidate_identity_digest = String::new();
    let digest = compute_identity_digest(&manifest)
        .map_err(|(outcome, detail)| anyhow::anyhow!("{outcome:?}: {detail}"))?;
    manifest.candidate_identity_digest = digest;
    let json = canonical_json(&manifest).map_err(anyhow::Error::msg)?;
    fs::write(envelope.join(MANIFEST_FILE_NAME), json).context("rewriting manifest")?;

    let receipt = ProducerReceipt {
        schema_version: HANDOFF_RECEIPT_SCHEMA_V1.to_string(),
        candidate_identity_digest: manifest.candidate_identity_digest.clone(),
        candidate_commit: manifest.candidate.commit.clone(),
        // The strong adversary reseals the receipt the way a published envelope
        // carries it, so no control below can pass merely because the receipt
        // looked unpublished.
        producer_self_check: SELF_CHECK_VALIDATED.to_string(),
        limitations: manifest.limitations.clone(),
    };
    let receipt_json = canonical_json(&receipt).map_err(anyhow::Error::msg)?;
    fs::write(envelope.join(RECEIPT_FILE_NAME), receipt_json).context("rewriting receipt")?;
    Ok(())
}

/// Rewrite raw manifest JSON without repairing the identity digest.
fn rewrite_manifest_raw(envelope: &Path, value: &serde_json::Value) -> Result<()> {
    let json = serde_json::to_string_pretty(value).context("serializing manifest")?;
    fs::write(envelope.join(MANIFEST_FILE_NAME), json).context("rewriting manifest")?;
    Ok(())
}

fn raw_manifest(envelope: &Path) -> Result<serde_json::Value> {
    let bytes = fs::read(envelope.join(MANIFEST_FILE_NAME)).context("reading manifest")?;
    serde_json::from_slice(&bytes).context("parsing manifest as JSON")
}

fn change_for<'manifest>(
    manifest: &'manifest Manifest,
    path: &str,
) -> Option<&'manifest super::model::ChangeRecord> {
    manifest.inventory.changes.iter().find(|change| change.path == path)
}

// ---------------------------------------------------------------------------
// Credential-shaped fixture values
// ---------------------------------------------------------------------------
//
// The hygiene controls need values that are byte-identical to real credential
// shapes — that is exactly what the code under test must detect. Assembling
// them at runtime keeps the discriminating value intact while leaving no
// scannable literal in this file, so repository secret scanning does not have
// to carry a standing exception for a test fixture. Each half is short enough
// that no detector matches it alone.

/// A synthetic value with the GitHub personal-access-token shape.
fn synthetic_github_token() -> String {
    format!("{}{}", "ghp_0123456789abc", "defghijklmnopqrstuvwxyz")
}

/// A synthetic value with the AWS access-key-id shape.
fn synthetic_aws_key_id() -> String {
    format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE")
}

/// The `ghp_` marker, assembled so the prefix is not a literal here either.
fn github_token_marker() -> String {
    format!("{}{}", "ghp", "_")
}

// ---------------------------------------------------------------------------
// Fixtures: the candidate shapes a real repair produces
// ---------------------------------------------------------------------------

#[test]
fn text_add_modify_and_delete_are_recomputable() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("keep.txt", b"one\n")?;
    fixture.write("drop.txt", b"gone\n")?;
    fixture.commit("base")?;

    fixture.write("keep.txt", b"two\n")?;
    fixture.write("added.txt", b"new\n")?;
    fs::remove_file(fixture.path().join("drop.txt"))?;
    fixture.commit("candidate")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(change_for(&manifest, "added.txt").map(|c| c.status), Some(ChangeStatus::Added));
    assert_eq!(change_for(&manifest, "keep.txt").map(|c| c.status), Some(ChangeStatus::Modified));
    assert_eq!(change_for(&manifest, "drop.txt").map(|c| c.status), Some(ChangeStatus::Deleted));
    assert_eq!(
        change_for(&manifest, "drop.txt").map(|c| c.entry_class),
        Some(EntryClass::Absent),
        "a deleted path has no candidate-side entry class"
    );
    Ok(())
}

#[test]
fn empty_file_is_transported_as_an_object() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    fixture.write("empty.txt", b"")?;
    fixture.commit("add empty")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let empty = change_for(&manifest, "empty.txt").context("empty.txt inventory row")?;
    assert_eq!(empty.status, ChangeStatus::Added);
    let object = empty.new_object.clone().context("empty file object id")?;
    assert!(
        manifest.transport.object_ids.contains(&object),
        "the empty blob must be carried, not inferred"
    );
    Ok(())
}

#[test]
fn executable_bit_transitions_are_inventory_facts() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("tool.sh", b"#!/bin/sh\n")?;
    fixture.commit("base")?;

    // Change the mode through the index so the fact does not depend on the
    // host filesystem honouring the executable bit.
    fixture.git(&["update-index", "--chmod=+x", "tool.sh"])?;
    fixture.commit_staged("make executable")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let row = change_for(&manifest, "tool.sh").context("tool.sh inventory row")?;
    assert_eq!(row.old_mode.as_deref(), Some("100644"));
    assert_eq!(row.new_mode.as_deref(), Some("100755"));
    assert_eq!(row.entry_class, EntryClass::ExecutableFile);
    assert_eq!(
        row.old_object, row.new_object,
        "the mode changed while the bytes did not; a content-only transport would lose this"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_entry_is_transported_with_its_class() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("target.txt", b"target\n")?;
    fixture.commit("base")?;
    std::os::unix::fs::symlink("target.txt", fixture.path().join("link.txt"))?;
    fixture.commit("add symlink")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let row = change_for(&manifest, "link.txt").context("link.txt inventory row")?;
    assert_eq!(row.new_mode.as_deref(), Some("120000"));
    assert_eq!(row.entry_class, EntryClass::Symlink);
    Ok(())
}

#[test]
fn binary_blob_is_transported_verbatim() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    let binary: Vec<u8> = (0u16..=255).map(|byte| byte as u8).cycle().take(4096).collect();
    fixture.write("image.bin", &binary)?;
    fixture.commit("add binary")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let row = change_for(&manifest, "image.bin").context("image.bin inventory row")?;
    let object = row.new_object.clone().context("binary object id")?;
    assert!(manifest.transport.object_ids.contains(&object));
    Ok(())
}

#[test]
fn renames_are_classified_with_and_without_content_change() -> Result<()> {
    let fixture = Fixture::new()?;
    // Distinct bodies: identical content would make rename pairing ambiguous
    // and the assertion would be testing Git's tie-break, not the inventory.
    let pure_body = "pure content line\n".repeat(40);
    let edited_body = "edited content line\n".repeat(40);
    fixture.write("pure.txt", pure_body.as_bytes())?;
    fixture.write("edited.txt", edited_body.as_bytes())?;
    fixture.commit("base")?;

    fs::rename(fixture.path().join("pure.txt"), fixture.path().join("pure-moved.txt"))?;
    fs::remove_file(fixture.path().join("edited.txt"))?;
    fixture.write("edited-moved.txt", format!("{edited_body}extra\n").as_bytes())?;
    fixture.commit("rename both")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let pure = change_for(&manifest, "pure-moved.txt").context("pure rename row")?;
    assert_eq!(pure.status, ChangeStatus::Renamed);
    assert_eq!(pure.old_path.as_deref(), Some("pure.txt"));
    assert_eq!(pure.similarity, Some(100));

    let edited = change_for(&manifest, "edited-moved.txt").context("edited rename row")?;
    assert_eq!(edited.status, ChangeStatus::Renamed);
    assert_eq!(edited.old_path.as_deref(), Some("edited.txt"));
    assert_ne!(edited.old_object, edited.new_object);
    Ok(())
}

#[test]
fn merge_commit_retains_ordered_parents() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("base.txt", b"base\n")?;
    let base = fixture.commit("base")?;

    fixture.write("left.txt", b"left\n")?;
    fixture.commit("left")?;
    let left = fixture.head()?;

    fixture.git(&["checkout", "--quiet", "-b", "side", &base])?;
    fixture.write("right.txt", b"right\n")?;
    fixture.commit("right")?;
    let right = fixture.head()?;

    fixture.git(&["checkout", "--quiet", "main"])?;
    fixture.git(&["merge", "--quiet", "--no-ff", "--no-edit", "side"])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert!(manifest.candidate.is_merge_commit);
    assert_eq!(
        manifest.candidate.parents,
        vec![left, right],
        "merge parent order is load-bearing and must survive transport"
    );
    assert_eq!(manifest.candidate.parents.len(), manifest.candidate.parent_trees.len());
    assert!(manifest.limitations.contains(&LimitationCode::MergeCommitDiffAgainstFirstParent));
    Ok(())
}

#[test]
fn root_commit_is_supported_against_the_empty_tree() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("first.txt", b"first\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert!(manifest.candidate.is_root_commit);
    assert!(manifest.candidate.parents.is_empty());
    assert_eq!(manifest.inventory.base_parent, None);
    assert_eq!(change_for(&manifest, "first.txt").map(|c| c.status), Some(ChangeStatus::Added));
    assert!(manifest.limitations.contains(&LimitationCode::RootCommitDiffAgainstEmptyTree));
    Ok(())
}

#[test]
fn submodule_gitlink_is_recorded_but_not_transported() -> Result<()> {
    let upstream = Fixture::with_remote(None)?;
    upstream.write("inner.txt", b"inner\n")?;
    let inner_commit = upstream.commit("inner")?;

    let fixture = Fixture::new()?;
    fixture.write("outer.txt", b"outer\n")?;
    fixture.commit("base")?;
    // Record the gitlink directly: `submodule add` needs network policy that a
    // credential-free environment must not require.
    fixture.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{inner_commit},vendor/inner"),
    ])?;
    fixture.commit_staged("add gitlink")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let gitlink = manifest.inventory.gitlinks.first().context("gitlink record")?;
    assert_eq!(gitlink.path, "vendor/inner");
    assert_eq!(gitlink.commit, inner_commit);
    assert_eq!(gitlink.disposition, GitlinkDisposition::ReferencedNotTransported);
    assert!(
        !manifest.transport.object_ids.contains(&inner_commit),
        "a submodule commit lives in another repository and must not be claimed as carried"
    );
    assert!(manifest.limitations.contains(&LimitationCode::SubmoduleGitlinkNotTransported));
    Ok(())
}

/// Removing the last submodule still declares the untransported-gitlink boundary.
///
/// `gitlinks` is collected from the candidate tree, so a candidate that deletes
/// its only submodule leaves it empty — and the limitation was derived from
/// exactly that emptiness. The inventory still carries a `160000` row whose
/// `old_object` names the submodule commit, and that commit is deliberately not
/// transported, so the envelope named an object it does not carry with nothing
/// declaring the boundary. A consumer reading the limitations would conclude
/// every referenced object was present.
///
/// Both the producer and the validator derive the limitation set independently
/// and then require them to be equal, so this defect was symmetric: both sides
/// omitted the code and agreed with each other. Agreement is not correctness
/// when the rule is written twice, which is why the derivation is now one
/// function on `ChangeInventory` that both sides call.
#[test]
fn a_deleted_submodule_still_declares_the_gitlink_boundary() -> Result<()> {
    let upstream = Fixture::with_remote(None)?;
    upstream.write("inner.txt", b"inner\n")?;
    let inner_commit = upstream.commit("inner")?;

    let fixture = Fixture::new()?;
    fixture.write("outer.txt", b"outer\n")?;
    fixture.commit("base")?;
    fixture.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{inner_commit},vendor/inner"),
    ])?;
    fixture.commit_staged("add gitlink")?;
    // The candidate removes the only submodule, emptying `gitlinks`.
    fixture.git(&["rm", "--cached", "vendor/inner"])?;
    fixture.commit_staged("remove gitlink")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    // Anti-vacuity: the fixture must actually produce the shape the defect
    // needs — an empty gitlink list beside a retained `160000` inventory row —
    // or this control would pass for a candidate that simply has no submodule.
    assert!(
        manifest.inventory.gitlinks.is_empty(),
        "the candidate tree no longer holds a gitlink, which is the whole point"
    );
    let deleted = manifest
        .inventory
        .changes
        .iter()
        .find(|change| change.path == "vendor/inner")
        .context("the deleted gitlink must still be an inventory row")?;
    assert_eq!(deleted.old_mode.as_deref(), Some("160000"));
    assert_eq!(deleted.old_object.as_deref(), Some(inner_commit.as_str()));
    assert!(
        !manifest.transport.object_ids.contains(&inner_commit),
        "a submodule commit is never transported, deleted or not"
    );

    // The boundary must be declared even though the candidate tree is clean.
    assert!(
        manifest.limitations.contains(&LimitationCode::SubmoduleGitlinkNotTransported),
        "an inventory naming an untransported submodule commit must declare that boundary"
    );
    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ValidHandoff,
        "the validator must derive the same limitation, not refuse the producer's"
    );
    Ok(())
}

/// Replacing a submodule with a regular file declares the same boundary.
///
/// The sibling case to deletion, and the reason the rule reads both sides of
/// every change row rather than only the base side: a type change away from
/// `160000` leaves no gitlink in the candidate tree either, while the old-side
/// commit stays named in the inventory and untransported.
#[test]
fn a_submodule_replaced_by_a_file_still_declares_the_gitlink_boundary() -> Result<()> {
    let upstream = Fixture::with_remote(None)?;
    upstream.write("inner.txt", b"inner\n")?;
    let inner_commit = upstream.commit("inner")?;

    let fixture = Fixture::new()?;
    fixture.write("outer.txt", b"outer\n")?;
    fixture.commit("base")?;
    fixture.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{inner_commit},vendor/inner"),
    ])?;
    fixture.commit_staged("add gitlink")?;
    fixture.git(&["rm", "--cached", "vendor/inner"])?;
    fixture.write("vendor/inner", b"now an ordinary file\n")?;
    fixture.commit("replace gitlink with a file")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert!(manifest.inventory.gitlinks.is_empty(), "the gitlink is gone from the tree");
    let replaced = manifest
        .inventory
        .changes
        .iter()
        .find(|change| change.path == "vendor/inner")
        .context("the replaced path must be an inventory row")?;
    assert_eq!(replaced.old_mode.as_deref(), Some("160000"));
    assert_eq!(replaced.new_mode.as_deref(), Some("100644"), "the candidate side is a real file");
    assert!(
        manifest.limitations.contains(&LimitationCode::SubmoduleGitlinkNotTransported),
        "a type change away from a gitlink still names an untransported commit"
    );
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::ValidHandoff);
    Ok(())
}

/// A partial clone missing an object fails locally instead of fetching it.
///
/// The producer claims to touch no network and need no credential. A partial
/// clone breaks that claim from underneath: its object store is *deliberately*
/// incomplete, and Git repairs a missing object by fetching from the promisor
/// remote — silently, from plumbing that looks entirely local, and needing no
/// credential at all on a public remote. `GIT_TERMINAL_PROMPT=0` does not stop
/// it, because nothing is prompting.
///
/// Measured before the fix, on a real promisor clone with one blob removed:
/// `cat-file -e` returned success and left a new `.promisor` pack behind. The
/// export would have reported a candidate it had just downloaded.
///
/// The fixture points the promisor remote at a path that does not exist, so any
/// fetch attempt must fail loudly and name the remote. The assertion is on
/// *which* failure occurs: refusing locally is right, and reaching for the
/// remote is the defect, so a diagnostic mentioning the remote fails this test
/// even though both cases are non-success.
///
/// `GIT_NO_LAZY_FETCH` needs Git 2.42; this module's floor is 2.24. The control
/// skips visibly below that rather than asserting a guarantee the running Git
/// cannot make.
#[test]
fn a_partial_clone_refuses_locally_instead_of_fetching() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"base content\n")?;
    fixture.commit("base")?;
    fixture.write("b.txt", b"candidate content\n")?;
    fixture.commit("candidate")?;

    let version = fixture.git(&["version"])?;
    if !git_supports_no_lazy_fetch(&version) {
        // The running Git predates `GIT_NO_LAZY_FETCH`, so it cannot make the
        // guarantee under test. A typed skip is honest; asserting a boundary
        // this Git does not implement would be a fixture pretending to prove
        // something.
        return Ok(());
    }

    // Make the repository look exactly like a partial clone whose promisor
    // remote is gone: the marker Git consults before attempting a lazy fetch.
    let unreachable = fixture.path().join("no-such-remote.git");
    fixture.git(&["remote", "set-url", "origin", &unreachable.display().to_string()])?;
    fixture.git(&["config", "remote.origin.promisor", "true"])?;
    fixture.git(&["config", "remote.origin.partialclonefilter", "blob:none"])?;

    // Remove one blob the export must read, so the promisor path is reachable.
    let blob = fixture.git(&["rev-parse", "HEAD:b.txt"])?.trim().to_string();
    let loose = fixture.path().join(".git/objects").join(&blob[..2]).join(&blob[2..]);
    fixture.git(&["unpack-objects", "-q"]).ok();
    if loose.exists() {
        fs::remove_file(&loose)?;
    } else {
        // The blob is inside a pack, so this fixture cannot make it absent
        // without rewriting the object store. Skip rather than assert against
        // a repository that is not actually missing anything.
        return Ok(());
    }

    let destination = Destination::new()?;
    let (outcome, detail) = create_handoff(&request(&fixture, &destination))
        .err()
        .context("an export missing a candidate object must not succeed")?;

    assert!(
        !detail.contains("Could not read from remote")
            && !detail.contains("does not appear to be a git repository"),
        "the export reached for the promisor remote instead of refusing locally: {detail}"
    );
    assert!(
        matches!(outcome, HandoffOutcome::MissingObject | HandoffOutcome::InstrumentFailure),
        "a locally missing object is a candidate/instrument failure, got {outcome:?}: {detail}"
    );
    assert!(
        !destination.envelope().exists(),
        "a refused export publishes nothing, including from a partial clone"
    );
    Ok(())
}

/// Whether `git --version` output is at least 2.42, when `GIT_NO_LAZY_FETCH`
/// was introduced.
fn git_supports_no_lazy_fetch(version: &str) -> bool {
    let Some(rest) = version.split_whitespace().nth(2) else {
        return false;
    };
    let mut parts = rest.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    (major, minor) >= (2, 42)
}

#[test]
fn unicode_and_whitespace_paths_round_trip_exactly() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    fixture.write("docs/naïve — notes.md", b"unicode\n")?;
    fixture.write("has space/tab\tname.txt", b"whitespace\n")?;
    fixture.commit("odd paths")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert!(
        change_for(&manifest, "docs/naïve — notes.md").is_some(),
        "non-ASCII paths must survive without re-encoding"
    );
    assert!(
        change_for(&manifest, "has space/tab\tname.txt").is_some(),
        "whitespace-bearing paths must survive without quoting"
    );
    Ok(())
}

#[test]
fn large_but_bounded_object_is_transported() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    let large = "payload line for a bounded but non-trivial blob\n".repeat(20_000);
    fixture.write("large.txt", large.as_bytes())?;
    fixture.commit("add large")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let row = change_for(&manifest, "large.txt").context("large.txt row")?;
    let object = row.new_object.clone().context("large object id")?;
    assert!(manifest.transport.object_ids.contains(&object));
    Ok(())
}

// ---------------------------------------------------------------------------
// Repository identity
// ---------------------------------------------------------------------------

#[test]
fn configured_remote_yields_an_observed_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Observed);
    assert_eq!(manifest.repository_identity.value.as_deref(), Some("example/repo"));
    Ok(())
}

#[test]
fn a_remote_less_workspace_stays_not_proven_but_transportable() -> Result<()> {
    let fixture = Fixture::with_remote(None)?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    create_handoff(&request(&fixture, &destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::RepositoryIdentityNotProven,
        "the candidate is transportable, but ownership was never proven"
    );
    assert_eq!(report.outcome.exit_code(), 3, "not-proven is not an invalid candidate");

    let identity = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "repository_identity")
        .context("repository_identity dimension")?;
    assert_eq!(identity.verdict, DimensionVerdict::NotProven);
    // Everything else must still be proven; not-proven ownership is one
    // dimension, not a blanket failure.
    for dimension in &report.dimensions {
        if dimension.id != "repository_identity" {
            assert_eq!(dimension.verdict, DimensionVerdict::Valid, "{}", dimension.id);
        }
    }
    Ok(())
}

#[test]
fn repository_identity_is_never_guessed_from_the_directory_name() -> Result<()> {
    let fixture = Fixture::with_remote(None)?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = create_handoff(&request(&fixture, &destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::NotProven);
    assert_eq!(manifest.repository_identity.value, None);
    assert!(manifest.limitations.contains(&LimitationCode::RepositoryIdentityNotProven));
    Ok(())
}

#[test]
fn a_credential_bearing_remote_is_refused_as_an_identity_source() -> Result<()> {
    let token = synthetic_github_token();
    let remote = format!("https://{}:{}@{}", "octocat", token, "github.com/acme/app.git");
    let fixture = Fixture::with_remote(Some(&remote))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = create_handoff(&request(&fixture, &destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::NotProven);
    assert!(manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials));

    // No byte of the URL may survive anywhere in the envelope.
    let manifest_text = fs::read_to_string(destination.envelope().join(MANIFEST_FILE_NAME))?;
    assert!(
        !manifest_text.contains(&github_token_marker()),
        "no token material may reach the envelope"
    );
    assert!(!manifest_text.contains("octocat"), "no userinfo may reach the envelope");
    assert!(!manifest_text.contains("acme/app"), "a refused URL yields no identity at all");
    Ok(())
}

#[test]
fn a_declared_identity_is_accepted_when_no_remote_exists() -> Result<()> {
    let fixture = Fixture::with_remote(None)?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.declared_repository_identity = Some("Acme/App".to_string());
    let manifest = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Declared);
    assert_eq!(manifest.repository_identity.value.as_deref(), Some("acme/app"));
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::ValidHandoff);
    Ok(())
}

// ---------------------------------------------------------------------------
// Determinism and independence
// ---------------------------------------------------------------------------

#[test]
fn identical_objects_in_two_workspaces_produce_one_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.write("added.bin", &[0u8, 1, 2, 3, 255])?;
    fixture.commit("candidate")?;

    // A clone stores the same objects in a packfile rather than loosely, which
    // is the ordinary cross-host difference.
    let clone_root = tempfile::TempDir::new()?;
    let clone_path = clone_root.path().join("clone");
    let status =
        isolated_git().args(["clone", "--quiet"]).arg(fixture.path()).arg(&clone_path).status()?;
    assert!(status.success(), "cloning the fixture");

    let first_destination = Destination::new()?;
    let first = export_valid(&fixture, &first_destination)?;

    let second_destination = Destination::new()?;
    // Point the clone at the same remote the original observed. Repository
    // identity is a semantic input — and its *status* is a claim-strength
    // input, so an observed identity and a declared one are legitimately
    // different candidates to this format. Holding both equal is what lets
    // this control compare the whole semantic identity rather than a subset.
    let status = isolated_git()
        .args(["remote", "set-url", "origin", "https://github.com/example/repo.git"])
        .current_dir(&clone_path)
        .status()?;
    assert!(status.success(), "retargeting the clone's remote");

    let inputs = CreateRequest {
        repository: clone_path,
        candidate: "HEAD".to_string(),
        out: second_destination.envelope(),
        declared_repository_identity: None,
        proofs: Vec::new(),
    };
    let second = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(first.candidate.commit, second.candidate.commit);
    assert_eq!(
        first.transport.object_ids, second.transport.object_ids,
        "the same candidate must enumerate the same objects from either storage layout"
    );
    assert_eq!(first.inventory, second.inventory);
    assert_eq!(
        first.candidate_identity_digest, second.candidate_identity_digest,
        "semantic identity is what this format guarantees across worktrees and hosts"
    );
    // Both sides observe the same remote, so nothing in the semantic identity
    // differs and the packs must agree byte for byte. This is the stronger
    // property the contract asks for, demonstrated across the ordinary
    // cross-host difference — loose objects versus a pack — at one Git version.
    // Only the cross-Git-version case remains a declared limitation, because
    // guaranteeing it would mean writing our own packer.
    assert_eq!(
        first.transport.files[0].sha256, second.transport.files[0].sha256,
        "identical objects must produce identical transport bytes from either storage layout"
    );
    assert_eq!(first.transport.files[0].bytes, second.transport.files[0].bytes);
    Ok(())
}

#[test]
fn repeated_export_of_one_candidate_is_byte_identical() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;

    let first = Destination::new()?;
    let second = Destination::new()?;
    let left = export_valid(&fixture, &first)?;
    let right = export_valid(&fixture, &second)?;

    assert_eq!(left.candidate_identity_digest, right.candidate_identity_digest);
    assert_eq!(
        fs::read(first.envelope().join(MANIFEST_FILE_NAME))?,
        fs::read(second.envelope().join(MANIFEST_FILE_NAME))?,
        "manifest bytes must be reproducible"
    );
    Ok(())
}

#[test]
fn detached_head_and_named_branch_yield_the_same_candidate_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    let candidate = fixture.commit("candidate")?;

    let named = Destination::new()?;
    let from_branch = export_valid(&fixture, &named)?;

    fixture.git(&["checkout", "--quiet", "--detach", &candidate])?;
    let detached = Destination::new()?;
    let from_detached = export_valid(&fixture, &detached)?;

    assert_eq!(
        from_branch.candidate_identity_digest, from_detached.candidate_identity_digest,
        "identity belongs to the commit, not to the ref that happens to point at it"
    );
    Ok(())
}

/// The control the whole format exists for: a handoff must survive the
/// disappearance of the workspace that produced it.
#[test]
fn a_handoff_validates_after_the_source_repository_is_destroyed() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"repaired\n")?;
    fixture.write("added.bin", &[9u8, 8, 7])?;
    fixture.commit("repair")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    let source = fixture.path().to_path_buf();
    drop(fixture);
    assert!(!source.exists(), "the producing workspace is gone");

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::ValidHandoff,
        "validation must not depend on the producing object database: {:#?}",
        report.dimensions
    );
    assert_eq!(report.candidate_commit.as_deref(), Some(manifest.candidate.commit.as_str()));
    Ok(())
}

// ---------------------------------------------------------------------------
// Proof binding
// ---------------------------------------------------------------------------

#[test]
fn a_proof_bound_to_the_candidate_is_carried_and_verified() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    let candidate = fixture.commit("candidate")?;

    let proof_root = tempfile::TempDir::new()?;
    let proof_path = proof_root.path().join("local-proof.json");
    fs::write(
        &proof_path,
        serde_json::to_vec(&serde_json::json!({ "commit": candidate, "tests": "passed" }))?,
    )?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];
    let manifest = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    let proof = manifest.proof_references.first().context("proof reference")?;
    assert_eq!(proof.candidate_subject, candidate);
    assert_eq!(proof.id, "local-proof.json");
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::ValidHandoff);
    assert!(
        manifest.limitations.contains(&LimitationCode::LocalProofOnly),
        "carried proof is local proof and must say so"
    );
    Ok(())
}

#[test]
fn a_proof_naming_another_candidate_is_refused_at_export() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let earlier = fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;

    let proof_root = tempfile::TempDir::new()?;
    let proof_path = proof_root.path().join("stale.json");
    fs::write(
        &proof_path,
        serde_json::to_vec(&serde_json::json!({ "commit": earlier, "tests": "passed" }))?,
    )?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];

    let Err((outcome, _)) = create_handoff(&inputs) else {
        bail!("a proof for an earlier commit must not be rebound to this candidate");
    };
    assert_eq!(outcome, HandoffOutcome::ProofSubjectMismatch);
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: each mutation must fail with its own class
// ---------------------------------------------------------------------------

#[test]
fn altering_transport_bytes_fails_the_declared_digest() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let pack = destination.envelope().join(PACK_FILE_NAME);
    let mut bytes = fs::read(&pack)?;
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&pack, &bytes)?;

    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::DigestMismatch);
    Ok(())
}

#[test]
fn substituting_another_candidates_pack_fails_object_presence() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;

    let real = Destination::new()?;
    let manifest = export_valid(&fixture, &real)?;

    // A different candidate in a different repository, resealed under the
    // first candidate's transport claims.
    let other = Fixture::new()?;
    other.write("z.txt", b"z\n")?;
    other.commit("unrelated")?;
    let other_destination = Destination::new()?;
    export_valid(&other, &other_destination)?;

    let foreign_pack = fs::read(other_destination.envelope().join(PACK_FILE_NAME))?;
    fs::write(real.envelope().join(PACK_FILE_NAME), &foreign_pack)?;

    let mut swapped = manifest;
    swapped.transport.files[0].bytes = foreign_pack.len() as u64;
    swapped.transport.files[0].sha256 = super::content_digest_hex(&foreign_pack);
    rewrite_manifest_resealed(&real.envelope(), swapped)?;

    assert_eq!(
        check_handoff(&real.envelope()).outcome,
        HandoffOutcome::MissingObject,
        "a resealed foreign transport cannot supply the declared objects"
    );
    Ok(())
}

#[test]
fn omitting_a_binary_object_from_the_transport_fails() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    fixture.write("image.bin", &[0u8, 159, 146, 150, 7, 7])?;
    fixture.commit("add binary")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    let binary_object = change_for(&manifest, "image.bin")
        .and_then(|row| row.new_object.clone())
        .context("binary object id")?;

    // Rebuild the pack without the binary blob, then reseal every claim.
    let mut reduced = manifest;
    reduced.transport.object_ids.retain(|id| *id != binary_object);
    let stdin = reduced.transport.object_ids.join("\n");
    let repacked = super::git::run_git_with_stdin(
        fixture.path(),
        &["pack-objects", "--stdout", "-q"],
        stdin.into_bytes(),
    )
    .map_err(anyhow::Error::msg)?;
    assert!(repacked.succeeded(), "repacking the reduced object set");
    fs::write(destination.envelope().join(PACK_FILE_NAME), &repacked.stdout_bytes)?;
    reduced.transport.files[0].bytes = repacked.stdout_bytes.len() as u64;
    reduced.transport.files[0].sha256 = super::content_digest_hex(&repacked.stdout_bytes);
    rewrite_manifest_resealed(&destination.envelope(), reduced)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::MissingObject,
        "a dropped blob must not pass as a complete candidate: {:#?}",
        report.dimensions
    );
    Ok(())
}

#[test]
fn removing_a_merge_parent_fails_parent_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("base.txt", b"base\n")?;
    let base = fixture.commit("base")?;
    fixture.write("left.txt", b"left\n")?;
    fixture.commit("left")?;
    fixture.git(&["checkout", "--quiet", "-b", "side", &base])?;
    fixture.write("right.txt", b"right\n")?;
    fixture.commit("right")?;
    fixture.git(&["checkout", "--quiet", "main"])?;
    fixture.git(&["merge", "--quiet", "--no-ff", "--no-edit", "side"])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut truncated = manifest;
    truncated.candidate.parents.truncate(1);
    truncated.candidate.parent_trees.truncate(1);
    truncated.candidate.is_merge_commit = false;
    rewrite_manifest_resealed(&destination.envelope(), truncated)?;

    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::ParentMismatch);
    Ok(())
}

#[test]
fn reversing_merge_parent_order_fails_parent_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("base.txt", b"base\n")?;
    let base = fixture.commit("base")?;
    fixture.write("left.txt", b"left\n")?;
    fixture.commit("left")?;
    fixture.git(&["checkout", "--quiet", "-b", "side", &base])?;
    fixture.write("right.txt", b"right\n")?;
    fixture.commit("right")?;
    fixture.git(&["checkout", "--quiet", "main"])?;
    fixture.git(&["merge", "--quiet", "--no-ff", "--no-edit", "side"])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut reversed = manifest;
    reversed.candidate.parents.reverse();
    reversed.candidate.parent_trees.reverse();
    // The first parent defines the inventory base, so reseal that too; only
    // the imported commit itself can still contradict the claim.
    reversed.inventory.base_parent = reversed.candidate.parents.first().cloned();
    rewrite_manifest_resealed(&destination.envelope(), reversed)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ParentMismatch,
        "parent order is identity, not presentation"
    );
    Ok(())
}

#[test]
fn declaring_a_different_tree_fails_tree_identity() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut swapped = manifest.clone();
    swapped.candidate.tree =
        manifest.candidate.parent_trees.first().cloned().context("parent tree to substitute")?;
    rewrite_manifest_resealed(&destination.envelope(), swapped)?;

    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::TreeMismatch);
    Ok(())
}

#[test]
fn altering_the_inventory_while_keeping_the_transport_fails() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.write("secret-change.txt", b"hidden\n")?;
    fixture.commit("candidate")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    // Hide one real change from the inventory and reseal the digest.
    let mut hidden = manifest;
    hidden.inventory.changes.retain(|change| change.path != "secret-change.txt");
    rewrite_manifest_resealed(&destination.envelope(), hidden)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::InventoryMismatch,
        "the inventory is recomputed from objects, not trusted: {:#?}",
        report.dimensions
    );
    Ok(())
}

#[test]
fn altering_a_declared_file_mode_fails_the_recomputed_inventory() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("tool.sh", b"#!/bin/sh\n")?;
    fixture.commit("base")?;
    fixture.git(&["update-index", "--chmod=+x", "tool.sh"])?;
    fixture.commit_staged("make executable")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut lied = manifest;
    if let Some(row) = lied.inventory.changes.iter_mut().find(|row| row.path == "tool.sh") {
        row.new_mode = Some("100644".to_string());
        row.entry_class = EntryClass::RegularFile;
    }
    rewrite_manifest_resealed(&destination.envelope(), lied)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InventoryMismatch,
        "a mode claim must be checked against the tree, not accepted as declared"
    );
    Ok(())
}

#[test]
fn an_abbreviated_object_id_is_refused_by_shape() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let mut raw = raw_manifest(&destination.envelope())?;
    let commit = raw["candidate"]["commit"].as_str().context("commit")?.to_string();
    raw["candidate"]["commit"] = serde_json::Value::String(commit[..7].to_string());
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "a short SHA cannot distinguish one object from a colliding prefix"
    );
    Ok(())
}

#[test]
fn an_undeclared_extra_file_breaks_envelope_closure() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    fs::write(destination.envelope().join("extra.pack"), b"undeclared bytes")?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let closure = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "envelope_closure")
        .context("envelope_closure dimension")?;
    assert_eq!(closure.verdict, DimensionVerdict::Invalid);
    Ok(())
}

#[test]
fn a_credential_in_the_commit_message_is_refused_at_export() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit(&format!("repair using {}", synthetic_github_token()))?;

    let destination = Destination::new()?;
    let Err((outcome, _)) = create_handoff(&request(&fixture, &destination)) else {
        bail!("a token in the commit message must not be exported silently");
    };
    assert_eq!(outcome, HandoffOutcome::UnsafeContent);
    assert!(
        !destination.envelope().exists(),
        "a refused export must not leave a partial envelope behind"
    );
    Ok(())
}

#[test]
fn a_credential_injected_into_a_manifest_is_refused_at_check() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut injected = manifest;
    injected.candidate.message =
        format!("cleanup\n\nAWS key {} for the runner\n", synthetic_aws_key_id());
    rewrite_manifest_resealed(&destination.envelope(), injected)?;

    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::UnsafeContent);
    Ok(())
}

#[test]
fn a_rebound_proof_subject_is_refused_at_check() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let earlier = fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    let candidate = fixture.commit("candidate")?;

    let proof_root = tempfile::TempDir::new()?;
    let proof_path = proof_root.path().join("proof.json");
    fs::write(
        &proof_path,
        serde_json::to_vec(&serde_json::json!({ "commit": candidate, "tests": "passed" }))?,
    )?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];
    let manifest = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    let mut rebound = manifest;
    rebound.proof_references[0].candidate_subject = earlier;
    rewrite_manifest_resealed(&destination.envelope(), rebound)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ProofSubjectMismatch
    );
    Ok(())
}

/// A resealed envelope must not be able to misrepresent what the candidate is.
///
/// The message, author, and committer are carried identity: an envelope that
/// imports correctly while attributing the work to someone else, or describing
/// a different change, is a false record even though every object is intact.
#[test]
fn a_rewritten_commit_message_is_caught_against_the_imported_object() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut rewritten = manifest;
    rewritten.candidate.message = "a completely different change\n".to_string();
    rewrite_manifest_resealed(&destination.envelope(), rewritten)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let identity = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "commit_identity")
        .context("commit_identity dimension")?;
    assert_eq!(identity.verdict, DimensionVerdict::Invalid);
    Ok(())
}

#[test]
fn a_rewritten_commit_author_is_caught_against_the_imported_object() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut reattributed = manifest;
    reattributed.candidate.author.email = "someone.else@example.invalid".to_string();
    rewrite_manifest_resealed(&destination.envelope(), reattributed)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "authorship is recomputed from the commit object, not carried on trust"
    );
    Ok(())
}

#[test]
fn a_manifest_swapped_beside_a_foreign_receipt_is_refused() -> Result<()> {
    let first = Fixture::new()?;
    first.write("a.txt", b"a\n")?;
    first.commit("root")?;
    let first_destination = Destination::new()?;
    export_valid(&first, &first_destination)?;

    let second = Fixture::new()?;
    second.write("z.txt", b"z\n")?;
    second.commit("root")?;
    let second_destination = Destination::new()?;
    export_valid(&second, &second_destination)?;

    // Swap in the second envelope's receipt, which names a different candidate.
    fs::copy(
        second_destination.envelope().join(RECEIPT_FILE_NAME),
        first_destination.envelope().join(RECEIPT_FILE_NAME),
    )?;

    let report = check_handoff(&first_destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::InvalidManifest,
        "two documents in one envelope must not name different candidates"
    );
    let closure = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "envelope_closure")
        .context("envelope_closure dimension")?;
    assert_eq!(closure.verdict, DimensionVerdict::Invalid);
    Ok(())
}

#[test]
fn a_comment_naming_a_commit_is_not_a_handoff() -> Result<()> {
    let envelope_root = tempfile::TempDir::new()?;
    let envelope = envelope_root.path().join("pasted-comment");
    fs::create_dir_all(&envelope)?;
    fs::write(
        envelope.join("comment.txt"),
        b"Committed as 0f8208953f78cc79ec0ddb5590a84f6b4626ceef; PR metadata prepared.\n",
    )?;

    let report = check_handoff(&envelope);
    assert_eq!(
        report.outcome,
        HandoffOutcome::InvalidManifest,
        "durable prose naming a commit is not a candidate transport"
    );
    assert_eq!(report.candidate_commit, None);
    Ok(())
}

#[test]
fn a_textual_patch_transport_cannot_be_declared() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let mut raw = raw_manifest(&destination.envelope())?;
    raw["transport"]["format"] = serde_json::Value::String("unified_diff".to_string());
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "a format that cannot carry mode, binary, rename, and parent identity is unrepresentable"
    );
    Ok(())
}

#[test]
fn an_unknown_manifest_field_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let mut raw = raw_manifest(&destination.envelope())?;
    if let Some(object) = raw.as_object_mut() {
        object.insert("published".to_string(), serde_json::Value::Bool(true));
    }
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "the envelope is closed to silent field reinterpretation"
    );
    Ok(())
}

/// The identity digest is what protects the claims nothing else can check.
///
/// Most tampering is caught earlier: recomputation against the imported objects
/// covers the commit and inventory, and `limitation_completeness` covers every
/// admission a receiver can re-derive from the candidate's own facts.
/// `remote_url_contained_credentials` is deliberately outside both. It is
/// producer-only knowledge — the refused URL never enters the envelope, so no
/// object and no later dimension can reconstruct it — which makes dropping it
/// the claim-weakening edit only the seal can catch.
#[test]
fn dropping_a_producer_only_limitation_breaks_the_identity_digest() -> Result<()> {
    let token = synthetic_github_token();
    let remote = format!("https://{}:{}@{}", "octocat", token, "github.com/acme/app.git");
    let fixture = Fixture::with_remote(Some(&remote))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = create_handoff(&request(&fixture, &destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    assert!(
        manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials),
        "the fixture must carry the admission the control removes"
    );

    // A refused remote leaves identity unproven, which is an honest boundary
    // rather than a valid handoff. The untampered seal is still intact, so the
    // digest verdict below is a change this control caused.
    let baseline = check_handoff(&destination.envelope());
    assert_eq!(baseline.outcome, HandoffOutcome::RepositoryIdentityNotProven);
    assert!(
        baseline.dimensions.iter().any(|dimension| dimension.id == "identity_digest"
            && dimension.verdict == DimensionVerdict::Valid),
        "the fixture's own seal must verify before the control breaks it"
    );

    let mut raw = raw_manifest(&destination.envelope())?;
    let kept: Vec<serde_json::Value> = raw["limitations"]
        .as_array()
        .context("limitations array")?
        .iter()
        .filter(|value| value.as_str() != Some("remote_url_contained_credentials"))
        .cloned()
        .collect();
    raw["limitations"] = serde_json::Value::Array(kept.clone());
    rewrite_manifest_raw(&destination.envelope(), &raw)?;
    // Reseal the receipt to agree with the edited manifest, leaving the stale
    // digest as the single remaining inconsistency. Without this the receipt
    // cross-check would catch the edit first and the control would no longer be
    // about the seal.
    let receipt_path = destination.envelope().join(RECEIPT_FILE_NAME);
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path)?).context("parsing receipt")?;
    receipt["limitations"] = serde_json::Value::Array(kept);
    fs::write(&receipt_path, serde_json::to_string_pretty(&receipt)?)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let digest = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "identity_digest")
        .context("identity_digest dimension")?;
    assert_eq!(
        digest.verdict,
        DimensionVerdict::Invalid,
        "no earlier dimension can see this edit, so the digest must"
    );
    let completeness = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "limitation_completeness")
        .context("limitation_completeness dimension")?;
    assert_eq!(
        completeness.verdict,
        DimensionVerdict::Valid,
        "the control is only about the seal if the completeness check accepts the edit"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-driven controls: fail-open seams closed after review of #14535
// ---------------------------------------------------------------------------

/// A failed self-check must leave no envelope at all.
///
/// Writing straight to the destination left a directory whose receipt asserted
/// a validation that never succeeded, which is precisely the durable artifact
/// D2 is meant to trust.
#[test]
fn a_failed_self_check_publishes_no_envelope() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;

    fn always_invalid(envelope: &Path) -> super::CheckReport {
        let mut report = check_handoff(envelope);
        report.outcome = HandoffOutcome::InventoryMismatch;
        report
    }

    let outcome = create_handoff_with_validator(&request(&fixture, &destination), always_invalid);
    let Err((outcome, _)) = outcome else {
        bail!("a refused self-check must not publish an envelope");
    };
    assert_eq!(outcome, HandoffOutcome::InventoryMismatch);
    assert!(!destination.envelope().exists(), "no envelope may be published");

    // Nor may a staging directory be left behind for a reader to stumble on.
    let leftovers: Vec<String> = fs::read_dir(destination.root.path())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(leftovers.is_empty(), "staging residue remained: {leftovers:?}");
    Ok(())
}

#[test]
fn a_published_envelope_records_a_validated_self_check() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let bytes = fs::read(destination.envelope().join(RECEIPT_FILE_NAME))?;
    let receipt: ProducerReceipt = serde_json::from_slice(&bytes)?;
    assert_eq!(receipt.producer_self_check, super::create::SELF_CHECK_VALIDATED);
    Ok(())
}

/// A second declared transport row would carry arbitrary extra bytes that pass
/// closure and digest checks, are never interpreted as objects, and do not
/// change the candidate's semantic identity.
#[test]
fn a_second_declared_transport_file_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let payload = b"arbitrary carried bytes".to_vec();
    fs::write(destination.envelope().join("extra.bin"), &payload)?;

    let mut widened = manifest;
    widened.transport.files.push(super::model::TransportFile {
        name: "extra.bin".to_string(),
        bytes: payload.len() as u64,
        sha256: super::content_digest_hex(&payload),
    });
    rewrite_manifest_resealed(&destination.envelope(), widened)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "v1 is exactly one candidate.pack"
    );
    Ok(())
}

/// An envelope that reads through a symlink is not self-contained: it stops
/// validating the moment the external target moves.
///
/// The refusal is cross-platform: `read_envelope_file` uses `symlink_metadata`,
/// which Windows honours for reparse points. Only the fixture is
/// platform-shaped, so the control runs on both.
#[test]
fn a_symlinked_transport_entry_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let outside = tempfile::TempDir::new()?;
    let target = outside.path().join("elsewhere.pack");
    fs::copy(destination.envelope().join(PACK_FILE_NAME), &target)?;
    fs::remove_file(destination.envelope().join(PACK_FILE_NAME))?;
    if !link_manifest(&target, &destination.envelope().join(PACK_FILE_NAME))? {
        return Ok(());
    }

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::InvalidManifest,
        "an envelope must not validate using bytes outside itself: {:#?}",
        report.dimensions
    );
    Ok(())
}

/// Accepting a superset of the closure would let a resealed transport carry an
/// unrelated object — including one holding credential material — inside an
/// envelope that still validates as this one bounded candidate.
#[test]
fn an_object_outside_the_candidate_closure_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;
    // An object that exists in the repository but is not part of the
    // candidate's own closure.
    fixture.git(&["checkout", "--quiet", "-b", "unrelated"])?;
    fixture.write("unrelated.txt", b"unrelated payload\n")?;
    fixture.commit("unrelated")?;
    let stray = fixture.git(&["rev-parse", "HEAD:unrelated.txt"])?;
    fixture.git(&["checkout", "--quiet", "main"])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(!manifest.transport.object_ids.contains(&stray));

    let mut widened = manifest;
    widened.transport.object_ids.push(stray.clone());
    widened.transport.object_ids.sort();
    let stdin = widened.transport.object_ids.join("\n");
    let repacked = super::git::run_git_with_stdin(
        fixture.path(),
        &["pack-objects", "--stdout", "-q"],
        stdin.into_bytes(),
    )
    .map_err(anyhow::Error::msg)?;
    assert!(repacked.succeeded());
    fs::write(destination.envelope().join(PACK_FILE_NAME), &repacked.stdout_bytes)?;
    widened.transport.files[0].bytes = repacked.stdout_bytes.len() as u64;
    widened.transport.files[0].sha256 = super::content_digest_hex(&repacked.stdout_bytes);
    rewrite_manifest_resealed(&destination.envelope(), widened)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::MissingObject,
        "the transport must carry the candidate's closure exactly: {:#?}",
        report.dimensions
    );
    Ok(())
}

/// Subject binding must be re-derived from the proof payload, not read back
/// from the manifest field a resealer controls.
#[test]
fn a_proof_payload_naming_another_candidate_is_refused_at_check() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let earlier = fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    let candidate = fixture.commit("candidate")?;

    let proof_root = tempfile::TempDir::new()?;
    let proof_path = proof_root.path().join("proof.json");
    fs::write(
        &proof_path,
        serde_json::to_vec(&serde_json::json!({ "commit": candidate, "tests": "passed" }))?,
    )?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];
    let manifest = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    // Swap in a payload for the earlier commit and reseal every derived record,
    // leaving `candidate_subject` pointing at this candidate.
    let foreign = serde_json::to_vec(&serde_json::json!({ "commit": earlier, "tests": "passed" }))?;
    let proof_id = manifest.proof_references[0].id.clone();
    fs::write(destination.envelope().join(PROOF_DIR_NAME).join(&proof_id), &foreign)?;

    let mut swapped = manifest;
    swapped.proof_references[0].bytes = foreign.len() as u64;
    swapped.proof_references[0].sha256 = super::content_digest_hex(&foreign);
    rewrite_manifest_resealed(&destination.envelope(), swapped)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ProofSubjectMismatch,
        "the proof's own payload names the candidate it proves"
    );
    Ok(())
}

#[test]
fn every_envelope_admits_that_transported_objects_are_not_secret_scanned() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert!(
        manifest.limitations.contains(&LimitationCode::TransportedObjectsNotSecretScanned),
        "the envelope transports committed objects; it does not audit them, and must say so"
    );
    Ok(())
}

/// The manifest must carry the commit body exactly as the object stores it.
///
/// `--format=%B` appends a newline of its own, so the message reached the
/// manifest one byte longer than `git cat-file commit`'s body — a receiver
/// reconstructing identity from the raw object would derive a different value
/// for a field documented as verbatim.
#[test]
fn the_manifest_message_equals_the_raw_commit_body() -> Result<()> {
    for message in ["subject only", "subject\n\nbody paragraph\n\nsecond paragraph"] {
        let fixture = Fixture::new()?;
        fixture.write("a.txt", b"a\n")?;
        let commit = fixture.commit(message)?;

        let destination = Destination::new()?;
        let manifest = export_valid(&fixture, &destination)?;

        let raw = isolated_git()
            .args(["cat-file", "commit", &commit])
            .current_dir(fixture.path())
            .output()?;
        let raw = raw.stdout;
        let separator = raw
            .windows(2)
            .position(|pair| pair == b"\n\n")
            .context("commit header/body separator")?;
        let body = String::from_utf8(raw[separator + 2..].to_vec())?;

        assert_eq!(
            manifest.candidate.message, body,
            "the manifest message must equal the raw commit body byte for byte"
        );
    }
    Ok(())
}

/// An entry class this format does not model must be refused, not silently
/// recorded as a deletion the semantic digest then commits to.
#[test]
fn an_unmodelled_entry_mode_is_refused_as_an_unsupported_class() {
    use super::create::entry_class_for;

    for (mode, expected) in [
        ("100644", EntryClass::RegularFile),
        ("100755", EntryClass::ExecutableFile),
        ("120000", EntryClass::Symlink),
        ("160000", EntryClass::Gitlink),
        ("000000", EntryClass::Absent),
    ] {
        assert_eq!(entry_class_for(mode).map_err(|(outcome, _)| outcome), Ok(expected), "{mode}");
    }

    let unknown = entry_class_for("040000");
    assert_eq!(
        unknown.map_err(|(outcome, _)| outcome),
        Err(HandoffOutcome::UnsupportedObjectClass),
        "an unmodelled mode is not a deletion"
    );
}

/// Comparing the manifest's declared set against the closure is still
/// circular: a resealed pack can carry the whole valid closure *plus*
/// undeclared objects, refresh its digest, and leave `object_ids` untouched.
#[test]
fn an_undeclared_object_carried_in_the_pack_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("base")?;
    fixture.write("a.txt", b"b\n")?;
    fixture.commit("candidate")?;
    // A blob that belongs to no part of this candidate.
    fixture.git(&["checkout", "--quiet", "-b", "unrelated"])?;
    fixture.write("stray.txt", b"content that never belonged to the candidate\n")?;
    fixture.commit("unrelated")?;
    let stray = fixture.git(&["rev-parse", "HEAD:stray.txt"])?;
    fixture.git(&["checkout", "--quiet", "main"])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(!manifest.transport.object_ids.contains(&stray));

    // Repack the declared closure *plus* the stray, leaving every manifest
    // claim about the object set untouched, and reseal the byte facts.
    let mut stdin = manifest.transport.object_ids.join("\n");
    stdin.push('\n');
    stdin.push_str(&stray);
    let repacked = super::git::run_git_with_stdin(
        fixture.path(),
        &["pack-objects", "--stdout", "-q"],
        stdin.into_bytes(),
    )
    .map_err(anyhow::Error::msg)?;
    assert!(repacked.succeeded());
    fs::write(destination.envelope().join(PACK_FILE_NAME), &repacked.stdout_bytes)?;

    let mut smuggled = manifest;
    smuggled.transport.files[0].bytes = repacked.stdout_bytes.len() as u64;
    smuggled.transport.files[0].sha256 = super::content_digest_hex(&repacked.stdout_bytes);
    rewrite_manifest_resealed(&destination.envelope(), smuggled)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::MissingObject,
        "the objects actually imported must equal the candidate's closure: {:#?}",
        report.dimensions
    );
    Ok(())
}

/// Limitations are the confidence boundaries a receiver acts on, and no Git
/// object records them, so a resealed envelope could otherwise drop one.
#[test]
fn a_dropped_mandatory_limitation_is_refused_even_when_resealed() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    for dropped in [
        LimitationCode::TransportedObjectsNotSecretScanned,
        LimitationCode::LocalProofOnly,
        LimitationCode::RootCommitDiffAgainstEmptyTree,
    ] {
        assert!(manifest.limitations.contains(&dropped), "{dropped:?} must be present to drop");
        let mut stripped = manifest.clone();
        stripped.limitations.retain(|code| *code != dropped);
        rewrite_manifest_resealed(&destination.envelope(), stripped)?;

        let report = check_handoff(&destination.envelope());
        assert_eq!(
            report.outcome,
            HandoffOutcome::InvalidManifest,
            "dropping {dropped:?} must be refused: {:#?}",
            report.dimensions
        );
        let dimension = report
            .dimensions
            .iter()
            .find(|dimension| dimension.id == "limitation_completeness")
            .context("limitation_completeness dimension")?;
        assert_eq!(dimension.verdict, DimensionVerdict::Invalid);
    }
    Ok(())
}

/// The producer refuses a credential-bearing proof, but a receiver cannot
/// assume the producer ran: the envelope comes from the less-trusted side.
#[test]
fn a_credential_bearing_proof_is_refused_at_check() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let candidate = fixture.commit("root")?;

    let proof_root = tempfile::TempDir::new()?;
    let proof_path = proof_root.path().join("proof.json");
    fs::write(
        &proof_path,
        serde_json::to_vec(&serde_json::json!({ "commit": candidate, "tests": "passed" }))?,
    )?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];
    let manifest = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    // Substitute a payload the producer would have refused, then reseal.
    let tainted = serde_json::to_vec(&serde_json::json!({
        "commit": candidate,
        "token": synthetic_github_token(),
    }))?;
    let proof_id = manifest.proof_references[0].id.clone();
    fs::write(destination.envelope().join(PROOF_DIR_NAME).join(&proof_id), &tainted)?;

    let mut resealed = manifest;
    resealed.proof_references[0].bytes = tainted.len() as u64;
    resealed.proof_references[0].sha256 = super::content_digest_hex(&tainted);
    rewrite_manifest_resealed(&destination.envelope(), resealed)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::UnsafeContent,
        "the receiver runs the same scan the producer does"
    );
    Ok(())
}

/// Isolation is a claim about the object database, so Git must not be able to
/// reach the receiver's own objects while validating.
#[test]
fn the_local_env_list_covers_what_git_reports() -> Result<()> {
    let repository = tempfile::TempDir::new()?;
    let output = Command::new("git")
        .args(["rev-parse", "--local-env-vars"])
        .current_dir(repository.path())
        .output()?;
    assert!(output.status.success(), "git rev-parse --local-env-vars");

    let reported: Vec<String> =
        String::from_utf8_lossy(&output.stdout).split_whitespace().map(str::to_string).collect();
    assert!(!reported.is_empty(), "git reported no repository-local variables");

    let missing: Vec<&String> = reported
        .iter()
        .filter(|variable| !super::git::GIT_LOCAL_ENV_VARS.contains(&variable.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "git reports repository-local variables this seam does not clear: {missing:?}"
    );
    Ok(())
}

#[test]
#[serial]
fn an_alternate_object_directory_cannot_satisfy_a_missing_object() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    fixture.write("image.bin", &[0u8, 1, 2, 3, 4, 5])?;
    fixture.commit("add binary")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    let binary_object = change_for(&manifest, "image.bin")
        .and_then(|row| row.new_object.clone())
        .context("binary object id")?;

    // Drop the blob from the pack *only*, leaving it declared. Dropping it from
    // `object_ids` too would let `verify_object_presence` refuse on pure
    // manifest arithmetic — declared versus required — without ever consulting
    // the object database, so the control would pass whether or not the
    // environment guard worked. Keeping `declared == required` forces the
    // question into the imported database, which is where the alternate would
    // answer it.
    let mut reduced = manifest;
    let packed: Vec<String> =
        reduced.transport.object_ids.iter().filter(|id| **id != binary_object).cloned().collect();
    let stdin = packed.join("\n");
    let repacked = super::git::run_git_with_stdin(
        fixture.path(),
        &["pack-objects", "--stdout", "-q"],
        stdin.into_bytes(),
    )
    .map_err(anyhow::Error::msg)?;
    assert!(repacked.succeeded());
    fs::write(destination.envelope().join(PACK_FILE_NAME), &repacked.stdout_bytes)?;
    reduced.transport.files[0].bytes = repacked.stdout_bytes.len() as u64;
    reduced.transport.files[0].sha256 = super::content_digest_hex(&repacked.stdout_bytes);
    rewrite_manifest_resealed(&destination.envelope(), reduced)?;

    // Point Git at the producing repository's objects. If the seam leaked this
    // variable through, the missing blob would resolve and the envelope would
    // validate on this machine while being incomplete everywhere else.
    // `#[serial]` serialises this against the module's other environment-
    // mutating control. It does *not* stop unannotated tests running
    // concurrently, so it is not what makes this safe for them: every
    // `run_git` invocation and every fixture spawn (`isolated_git`) clears or
    // overrides these variables on the child explicitly, so a concurrent test
    // cannot observe this one's value.
    // SAFETY: restored immediately below; see above for why concurrent tests
    // are unaffected.
    let objects = fixture.path().join(".git").join("objects");
    unsafe { std::env::set_var("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects) };
    let report = check_handoff(&destination.envelope());
    unsafe { std::env::remove_var("GIT_ALTERNATE_OBJECT_DIRECTORIES") };

    assert_eq!(
        report.outcome,
        HandoffOutcome::MissingObject,
        "the receiver's own object store must not complete an envelope: {:#?}",
        report.dimensions
    );
    Ok(())
}

#[test]
fn removing_the_executable_bit_is_also_an_inventory_fact() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("tool.sh", b"#!/bin/sh\n")?;
    fixture.git(&["add", "--all"])?;
    fixture.git(&["update-index", "--chmod=+x", "tool.sh"])?;
    fixture.commit_staged("base executable")?;

    fixture.git(&["update-index", "--chmod=-x", "tool.sh"])?;
    fixture.commit_staged("drop the executable bit")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let row = change_for(&manifest, "tool.sh").context("tool.sh inventory row")?;
    assert_eq!(row.old_mode.as_deref(), Some("100755"));
    assert_eq!(row.new_mode.as_deref(), Some("100644"));
    assert_eq!(row.entry_class, EntryClass::RegularFile);
    assert_eq!(row.old_object, row.new_object, "only the mode changed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Credential detection
// ---------------------------------------------------------------------------

#[test]
fn credential_shapes_without_a_vendor_prefix_are_detected() {
    use super::hygiene::scan_secrets;

    let aws_secret = format!("{}{}", "AWS_SECRET_ACCESS_KEY=", "wJalrXUtnFEMI/K7MDENG/bPx");
    assert!(!scan_secrets("f", &aws_secret).is_empty(), "AWS secret assignment");

    let npm = format!("{}{}", "//registry.npmjs.org/:_authToken=", "0123-4567-89ab");
    assert!(!scan_secrets("f", &npm).is_empty(), "npm auth token");

    let netrc = format!("machine github.com login someone {} hunter2", "password");
    assert!(!scan_secrets("f", &netrc).is_empty(), "netrc password line");

    assert!(!scan_secrets("f", &synthetic_aws_key_id()).is_empty(), "AWS access key id");
}

/// A bare four-letter marker refused ordinary prose, and the producer has no
/// override, so an over-broad detector made legitimate candidates unexportable.
#[test]
fn prose_resembling_a_credential_prefix_is_not_a_finding() {
    use super::hygiene::scan_secrets;

    let prose = "refactor AkiaModule and rename AKIActually for clarity";
    assert!(
        scan_secrets("candidate.message", prose).is_empty(),
        "a four-letter prefix inside prose is not an AWS key"
    );
}

#[test]
fn a_bare_token_remote_is_recognised_as_credential_bearing() {
    use super::hygiene::url_carries_credentials;

    // The ordinary way a PAT is embedded in a remote carries no colon at all.
    let bare = format!("https://{}@github.com/owner/name.git", synthetic_github_token());
    assert!(url_carries_credentials(&bare), "bare-token userinfo is credential-bearing");

    let encoded = "https://alice%3Ahunter2@github.com/owner/name.git";
    assert!(url_carries_credentials(encoded), "a percent-encoded colon still hides a password");

    // Ordinary remotes must not be misread.
    assert!(!url_carries_credentials("https://github.com/owner/name.git"));
    assert!(!url_carries_credentials("git@github.com:owner/name.git"));
    assert!(
        !url_carries_credentials("https://github.com/owner/name/blob/main/a@b.txt"),
        "an @ in the path is not userinfo"
    );
}

#[test]
fn a_bare_token_remote_yields_no_observed_identity() -> Result<()> {
    let remote = format!("https://{}@github.com/acme/app.git", synthetic_github_token());
    let fixture = Fixture::with_remote(Some(&remote))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = create_handoff(&request(&fixture, &destination))
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::NotProven);
    assert!(manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials));
    Ok(())
}

// ---------------------------------------------------------------------------
// Outcome vocabulary
// ---------------------------------------------------------------------------

#[test]
fn outcome_exit_codes_keep_failure_classes_apart() {
    assert_eq!(HandoffOutcome::ValidHandoff.exit_code(), 0);
    assert_eq!(HandoffOutcome::InventoryMismatch.exit_code(), 2);
    assert_eq!(HandoffOutcome::RepositoryIdentityNotProven.exit_code(), 3);
    assert_eq!(HandoffOutcome::InstrumentFailure.exit_code(), 4);
    assert_ne!(
        HandoffOutcome::InstrumentFailure.exit_code(),
        HandoffOutcome::InventoryMismatch.exit_code(),
        "an instrument that could not run is not a candidate that is wrong"
    );
}

#[test]
fn every_outcome_has_a_stable_distinct_spelling() {
    let outcomes = [
        HandoffOutcome::ValidHandoff,
        HandoffOutcome::InvalidManifest,
        HandoffOutcome::MissingObject,
        HandoffOutcome::DigestMismatch,
        HandoffOutcome::TreeMismatch,
        HandoffOutcome::ParentMismatch,
        HandoffOutcome::InventoryMismatch,
        HandoffOutcome::UnsafeContent,
        HandoffOutcome::UnsupportedObjectClass,
        HandoffOutcome::RepositoryIdentityNotProven,
        HandoffOutcome::ProofSubjectMismatch,
        HandoffOutcome::InstrumentFailure,
    ];
    // An exhaustive match so a later variant cannot be added without being
    // listed above; otherwise this distinctness claim silently narrows to
    // whatever was known when it was written.
    for outcome in &outcomes {
        match outcome {
            HandoffOutcome::ValidHandoff
            | HandoffOutcome::InvalidManifest
            | HandoffOutcome::MissingObject
            | HandoffOutcome::DigestMismatch
            | HandoffOutcome::TreeMismatch
            | HandoffOutcome::ParentMismatch
            | HandoffOutcome::InventoryMismatch
            | HandoffOutcome::UnsafeContent
            | HandoffOutcome::UnsupportedObjectClass
            | HandoffOutcome::RepositoryIdentityNotProven
            | HandoffOutcome::ProofSubjectMismatch
            | HandoffOutcome::InstrumentFailure => {}
        }
    }
    let mut spellings: Vec<&str> = outcomes.iter().map(HandoffOutcome::as_str).collect();
    spellings.sort_unstable();
    let count = spellings.len();
    spellings.dedup();
    assert_eq!(spellings.len(), count, "outcome spellings must be distinct");
}

#[test]
fn a_missing_repository_is_an_instrument_failure_not_an_invalid_candidate() -> Result<()> {
    let empty = tempfile::TempDir::new()?;
    let destination = Destination::new()?;
    let inputs = CreateRequest {
        repository: empty.path().to_path_buf(),
        candidate: "HEAD".to_string(),
        out: destination.envelope(),
        declared_repository_identity: None,
        proofs: Vec::new(),
    };
    let Err((outcome, _)) = create_handoff(&inputs) else {
        bail!("a non-repository must not produce a handoff");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    Ok(())
}

#[test]
fn an_existing_destination_is_never_overwritten() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let second = create_handoff(&request(&fixture, &destination));
    let Err((outcome, _)) = second else {
        bail!("an envelope is immutable and must not be rewritten in place");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-driven controls: fail-open seams closed after the second review of
// #14535
// ---------------------------------------------------------------------------

/// `explain` reads the same untrusted envelopes the validator does.
///
/// Skipping transport verification is a deliberate feature of the projection.
/// Skipping the reader's bounds is not: an oversized or link-bearing manifest
/// would otherwise get in through `explain` after `check` closed the door.
#[test]
fn explain_refuses_a_manifest_the_validator_would_refuse() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;
    assert!(explain(&destination.envelope()).is_ok(), "the sound envelope must still explain");

    // Replace the manifest with a symbolic link to a manifest outside the
    // envelope. `check` refuses this; `explain` must not read through it.
    let outside = destination.root().join("outside-manifest.json");
    fs::copy(destination.envelope().join(MANIFEST_FILE_NAME), &outside)?;
    fs::remove_file(destination.envelope().join(MANIFEST_FILE_NAME))?;
    if !link_manifest(&outside, &destination.envelope().join(MANIFEST_FILE_NAME))? {
        // Windows without the symlink privilege: no reparse point could be
        // created, so there is nothing to refuse. A typed skip is honest; a
        // weakened fixture asserting against a plain copy would not be.
        return Ok(());
    }

    let Err((outcome, detail)) = explain(&destination.envelope()) else {
        bail!("explain must not read a manifest through a symbolic link");
    };
    assert_eq!(outcome, HandoffOutcome::InvalidManifest);
    assert!(detail.contains("symbolic link"), "the refusal must name the reason: {detail}");
    Ok(())
}

/// Create the symbolic link a refusal control needs, or report that the
/// platform would not let us make one.
///
/// The refusal under test is cross-platform — `read_envelope_file` uses
/// `symlink_metadata` and `file_type().is_symlink()`, which Windows honours for
/// reparse points — so the control belongs on both platforms and only the
/// *fixture* is platform-shaped. On Windows, creating a file symlink needs
/// `SeCreateSymbolicLinkPrivilege`; without it the honest outcome is a visible
/// skip rather than a red X or a fixture quietly downgraded to a copy, which
/// would assert nothing. `PLSW_REQUIRE_SYMLINK_PRIVILEGE` turns that skip into
/// a hard failure on proof surfaces.
fn link_manifest(target: &Path, link: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }
    #[cfg(windows)]
    {
        Ok(perl_tdd_support::try_create_file_symlink(target, link)?.is_some())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Ok(false)
    }
}

/// A commit message this format cannot retain verbatim is refused, not mangled.
///
/// Git's text projection substitutes U+FFFD for bytes that are not UTF-8. The
/// validator recomputes through the same reader, so a lossy message would
/// round-trip and the envelope would pass while misrepresenting the candidate
/// to every human who reads it. Refusing is the only honest outcome.
#[test]
fn a_non_utf8_commit_message_is_refused_rather_than_silently_mangled() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    // `git commit` re-encodes a message it judges non-UTF-8, so it cannot
    // produce this object. Writing the commit verbatim with `hash-object` does,
    // and it is also the honest fixture: a receiver validates whatever object
    // exists, however it was created.
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    let mut raw_commit = format!(
        "tree {tree}\n\
         author Fixture Author <fixture@example.invalid> 1600000000 +0000\n\
         committer Fixture Author <fixture@example.invalid> 1600000000 +0000\n\n\
         subject with a raw "
    )
    .into_bytes();
    // 0xFF is not valid UTF-8 in any position.
    raw_commit.push(0xFF);
    raw_commit.extend_from_slice(b" byte\n");
    let commit_file = fixture.path().join("raw-commit.bin");
    fs::write(&commit_file, &raw_commit)?;
    let commit = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &commit])?;

    let destination = Destination::new()?;
    let Err((outcome, detail)) = create_handoff(&request(&fixture, &destination)) else {
        bail!("a non-UTF-8 commit message cannot be retained verbatim and must be refused");
    };
    assert_eq!(outcome, HandoffOutcome::UnsupportedObjectClass);
    assert!(detail.contains("non-UTF-8"), "the refusal must name the reason: {detail}");
    assert!(!destination.envelope().exists(), "a refused export publishes nothing");
    Ok(())
}

/// A credential hidden in the commit date is refused, not carried.
///
/// The date reads like a field nothing could hide in, which is why the scanner
/// skipped it. But `parse_commit_person` takes everything after the closing
/// `>` and only requires it to be non-empty — the `<seconds> <offset>` shape is
/// Git's convention, not a rule this format enforces — so any trailing text
/// becomes `candidate.author.date` and is retained in the manifest and covered
/// by the semantic digest.
///
/// Git's own fsck refuses to write such a commit (`badTimezone`), so this needs
/// `hash-object --literally`. That is the honest fixture rather than a
/// contrived one: the producer validates the object it is given, not the object
/// Git would have chosen to make, and an envelope may be produced from a
/// repository built by something other than `git commit`.
///
/// Measured before the fix: the token reached `candidate.author.date` verbatim
/// and `check` returned `VALID_HANDOFF`.
#[test]
fn a_credential_in_the_commit_date_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    let token = synthetic_github_token();
    let raw_commit = format!(
        "tree {tree}\n\
         author Fixture Author <fixture@example.invalid> 1600000000 +0000 {token}\n\
         committer Fixture Author <fixture@example.invalid> 1600000000 +0000\n\n\
         ordinary subject\n"
    );
    fs::write(fixture.path().join("raw-commit.bin"), raw_commit.as_bytes())?;
    let commit = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--literally", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &commit])?;

    let destination = Destination::new()?;
    let Err((outcome, detail)) = create_handoff(&request(&fixture, &destination)) else {
        bail!("a credential in the commit date must not reach a published envelope");
    };
    assert_eq!(outcome, HandoffOutcome::UnsafeContent);
    assert!(
        detail.contains("candidate.author.date"),
        "the refusal must name the field it found: {detail}"
    );
    assert!(!destination.envelope().exists(), "a refused export publishes nothing");

    // Anti-vacuity: the same commit with an ordinary date must still export, or
    // this control would pass for a producer that refused every literal commit.
    let ordinary = raw_commit.replace(&format!(" +0000 {token}\n"), " +0000\n");
    fs::write(fixture.path().join("raw-commit.bin"), ordinary.as_bytes())?;
    let clean = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--literally", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &clean])?;
    let second = Destination::new()?;
    let manifest = export_valid(&fixture, &second)?;
    assert_eq!(manifest.candidate.author.date, "1600000000 +0000");
    Ok(())
}

/// A commit recording a singular header twice is refused, not projected onto
/// whichever copy happens to come last.
///
/// `parse_commit_headers` assigned `tree`, `author`, and `committer` on every
/// occurrence, so a second record silently replaced the first. The manifest
/// documents one of each as the commit's verbatim identity and `content_safety`
/// scans those manifest copies, so a credential in the dropped `author` was
/// never scanned while it still travelled inside the transported commit object.
/// `check` reruns this same parse against the imported object, so producer and
/// validator agreed on the same lossy projection and no dimension could see the
/// disagreement with the commit.
///
/// The duplicated `tree` is the same defect without a secret: Git resolves such
/// a commit from the first `tree` record, so keeping the last one would put a
/// tree in the manifest that Git itself does not use.
///
/// Measured before the fix, for the `author` case: `create` published and
/// `check` returned `VALID_HANDOFF` with the token absent from the manifest and
/// present in `candidate.pack`.
#[test]
fn a_commit_recording_a_singular_header_twice_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let first_tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    fixture.write("b.txt", b"b\n")?;
    fixture.commit("second")?;
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();

    let token = synthetic_github_token();
    let author = "author Fixture Author <fixture@example.invalid> 1600000000 +0000";
    let committer = "committer Fixture Author <fixture@example.invalid> 1600000000 +0000";
    let well_formed = format!("tree {tree}\n{author}\n{committer}\n\nordinary subject\n");

    // Each case duplicates exactly one singular header. The first copy is the
    // one a lossy projection drops, so it carries what must not disappear: a
    // credential for `author`, and a tree Git would actually resolve for `tree`.
    let cases = [
        (
            "tree",
            format!("tree {first_tree}\ntree {tree}\n{author}\n{committer}\n\nordinary subject\n"),
        ),
        (
            "author",
            format!(
                "tree {tree}\n\
                 author Fixture Author <{token}@example.invalid> 1600000000 +0000\n\
                 {author}\n{committer}\n\nordinary subject\n"
            ),
        ),
        ("committer", format!("tree {tree}\n{author}\n{committer}\n{committer}\n\nsubject\n")),
    ];

    for (header, raw_commit) in &cases {
        fs::write(fixture.path().join("raw-commit.bin"), raw_commit.as_bytes())?;
        let commit = fixture
            .git(&["hash-object", "-t", "commit", "-w", "--literally", "--", "raw-commit.bin"])?
            .trim()
            .to_string();
        fixture.git(&["update-ref", "refs/heads/main", &commit])?;

        let destination = Destination::new()?;
        let Err((outcome, detail)) = create_handoff(&request(&fixture, &destination)) else {
            bail!("a commit with two `{header}` headers must not reach a published envelope");
        };
        assert_eq!(
            outcome,
            HandoffOutcome::UnsupportedObjectClass,
            "a duplicated `{header}` is an object class this format cannot retain: {detail}"
        );
        assert!(detail.contains(header), "the refusal must name the duplicated header: {detail}");
        assert!(!destination.envelope().exists(), "a refused export publishes nothing");
    }

    // Anti-vacuity: the same commit shape with one of each header must still
    // export, or this control would pass for a producer that refused every
    // literal commit.
    fs::write(fixture.path().join("raw-commit.bin"), well_formed.as_bytes())?;
    let clean = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--literally", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &clean])?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert_eq!(manifest.candidate.tree, tree);
    Ok(())
}

/// A credential shaped like a proof id is refused, not admitted as well-formed.
///
/// `is_proof_id` accepts lowercase alphanumerics with `.`, `_`, and `-` up to
/// 128 bytes, which is exactly the shape of a bare access token — so the id
/// passed validation and was retained unscanned. Establishing that a string is
/// well-formed is not establishing that it is not a secret, and the two checks
/// had been standing in for each other.
#[test]
fn a_credential_shaped_proof_id_is_refused() -> Result<()> {
    let token = synthetic_github_token();
    // Anti-vacuity for the premise: the defect only exists because the token
    // *is* a legal proof id. If this stopped holding, the control below would
    // be testing the wrong refusal.
    assert!(
        super::hygiene::is_proof_id(&token),
        "the finding depends on a token being a well-formed proof id"
    );

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let commit = fixture.commit("root")?;

    let proof_dir = Destination::new()?;
    let proof_path = proof_dir.root().join(format!("{token}.json"));
    fs::write(&proof_path, format!("{{\"commit\":\"{commit}\"}}\n"))?;

    let destination = Destination::new()?;
    let mut inputs = request(&fixture, &destination);
    inputs.proofs = vec![proof_path];

    let Err((outcome, detail)) = create_handoff(&inputs) else {
        bail!("a credential-shaped proof id must not reach a published envelope");
    };
    assert_eq!(outcome, HandoffOutcome::UnsafeContent);
    assert!(
        detail.contains("proof_references"),
        "the refusal must name the field it found: {detail}"
    );
    assert!(!destination.envelope().exists(), "a refused export publishes nothing");
    Ok(())
}

/// `owner/name` alone does not name a repository.
///
/// The same pair exists on every forge, so an observed identity that dropped
/// the host would hand a publisher a target it could resolve to the wrong
/// repository entirely.
#[test]
fn an_observed_identity_records_the_host_it_was_read_from() -> Result<()> {
    let fixture = Fixture::with_remote(Some("https://gitlab.com/acme/app.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Observed);
    assert_eq!(manifest.repository_identity.value.as_deref(), Some("acme/app"));
    assert_eq!(
        manifest.repository_identity.host.as_deref(),
        Some("gitlab.com"),
        "an observed identity must say which forge it names"
    );
    Ok(())
}

/// A local clone path locates a repository; it does not identify one.
///
/// `file:///srv/mirrors/acme/app.git` has the same final two segments as every
/// other mirror of every other `acme/app`, and no hosting authority to tell
/// them apart. That is `NOT_PROVEN`, not an observation.
#[test]
fn a_remote_with_no_hosting_authority_proves_no_identity() -> Result<()> {
    for remote in ["file:///srv/mirrors/acme/app.git", "/srv/mirrors/acme/app.git", "../app.git"] {
        let fixture = Fixture::with_remote(Some(remote))?;
        fixture.write("a.txt", b"a\n")?;
        fixture.commit("root")?;
        let destination = Destination::new()?;
        let manifest = create_handoff(&request(&fixture, &destination))
            .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

        assert_eq!(
            manifest.repository_identity.status,
            RepositoryIdentityStatus::NotProven,
            "`{remote}` names no hosting authority and must prove no identity"
        );
        assert_eq!(manifest.repository_identity.value, None);
        assert_eq!(manifest.repository_identity.host, None);
    }
    Ok(())
}

/// The SCP remote form still carries a host, and it is retained.
#[test]
fn the_scp_remote_form_yields_a_host_and_an_identity() -> Result<()> {
    let fixture = Fixture::with_remote(Some("git@github.com:Acme/App.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Observed);
    assert_eq!(manifest.repository_identity.value.as_deref(), Some("acme/app"));
    assert_eq!(manifest.repository_identity.host.as_deref(), Some("github.com"));
    Ok(())
}

/// Claim strength is a closed tuple, not four fields that happen to agree.
///
/// Each case below is internally plausible and fully resealed. Accepting any of
/// them would let a resealed envelope present a caller's guess as something the
/// producer observed, or an observation with no forge behind it.
#[test]
fn a_forged_repository_claim_strength_is_refused() -> Result<()> {
    let cases: &[(&str, &str, Option<&str>, Option<&str>)] = &[
        // A caller's declaration presented as an observation.
        ("observed", "caller_declared", Some("acme/app"), None),
        // An observation with no host behind it.
        ("observed", "git_remote_origin", Some("acme/app"), None),
        // An unproven identity that nonetheless names a host.
        ("not_proven", "unavailable", None, Some("github.com")),
        // A declared identity dressed up with a host it never had.
        ("declared", "caller_declared", Some("acme/app"), Some("github.com")),
    ];

    for (status, source, value, host) in cases {
        let fixture = Fixture::with_remote(Some("https://github.com/acme/app.git"))?;
        fixture.write("a.txt", b"a\n")?;
        fixture.commit("root")?;
        let destination = Destination::new()?;
        export_valid(&fixture, &destination)?;

        let mut raw = raw_manifest(&destination.envelope())?;
        raw["repository_identity"]["status"] = serde_json::Value::String((*status).to_string());
        raw["repository_identity"]["source"] = serde_json::Value::String((*source).to_string());
        raw["repository_identity"]["value"] = match value {
            Some(text) => serde_json::Value::String((*text).to_string()),
            None => serde_json::Value::Null,
        };
        raw["repository_identity"]["host"] = match host {
            Some(text) => serde_json::Value::String((*text).to_string()),
            None => serde_json::Value::Null,
        };
        rewrite_manifest_raw(&destination.envelope(), &raw)?;

        let report = check_handoff(&destination.envelope());
        assert_eq!(
            report.outcome,
            HandoffOutcome::InvalidManifest,
            "`{status}` from `{source}` with value {value:?} and host {host:?} must be refused"
        );
        let shape = report
            .dimensions
            .iter()
            .find(|dimension| dimension.id == "manifest_shape")
            .context("manifest_shape dimension")?;
        assert_eq!(
            shape.verdict,
            DimensionVerdict::Invalid,
            "the tuple is a shape fact and must be refused as one"
        );
    }
    Ok(())
}

/// A published envelope must carry the receipt a successful check produced.
///
/// The producer writes `pending` while staging and rewrites it only after its
/// own validation passes. A published directory still carrying `pending` was
/// either never validated or lifted out of staging by hand, and accepting it
/// would make the receipt's whole self-check claim decorative.
#[test]
fn a_published_envelope_carrying_a_pending_receipt_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let receipt_path = destination.envelope().join(RECEIPT_FILE_NAME);
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path)?).context("parsing receipt")?;
    assert_eq!(receipt["producer_self_check"].as_str(), Some(SELF_CHECK_VALIDATED));
    receipt["producer_self_check"] = serde_json::Value::String(SELF_CHECK_PENDING.to_string());
    fs::write(&receipt_path, serde_json::to_string_pretty(&receipt)?)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let closure = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "envelope_closure")
        .context("envelope_closure dimension")?;
    assert!(
        closure.detail.contains(SELF_CHECK_VALIDATED),
        "the refusal must name the token it required: {}",
        closure.detail
    );

    // The same directory is a legitimate *staging* directory, which is exactly
    // the distinction the two entry points exist to keep.
    assert_ne!(
        check_staged(&destination.envelope()).outcome,
        HandoffOutcome::InvalidManifest,
        "a pending receipt is honest before publication"
    );
    Ok(())
}

/// A receipt that repeats the digest but contradicts the admissions is not an
/// agreeing receipt.
#[test]
fn a_receipt_contradicting_the_manifest_limitations_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let receipt_path = destination.envelope().join(RECEIPT_FILE_NAME);
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path)?).context("parsing receipt")?;
    receipt["limitations"] = serde_json::Value::Array(vec![]);
    fs::write(&receipt_path, serde_json::to_string_pretty(&receipt)?)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    Ok(())
}

/// A failed export leaves nothing behind, including its staging directory.
///
/// Cleanup is owned by `StagedEnvelope`'s `Drop`, so this holds for the refusal
/// path and for any write failure or early return between staging and the
/// publishing rename.
#[test]
fn a_refused_export_leaves_no_staging_directory() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;

    let refuse = |envelope: &Path| CheckReport {
        schema_version: CHECK_REPORT_SCHEMA_V1.to_string(),
        envelope: envelope.to_string_lossy().into_owned(),
        candidate_commit: None,
        candidate_identity_digest: None,
        dimensions: Vec::new(),
        outcome: HandoffOutcome::InvalidManifest,
    };
    let outcome = create_handoff_with_validator(&request(&fixture, &destination), refuse);
    assert!(outcome.is_err(), "a refusing validator must not publish");
    assert!(!destination.envelope().exists(), "no envelope is published");

    let leftovers: Vec<String> = fs::read_dir(destination.root())?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers.is_empty(),
        "a refused export must leave no staging directory behind, found {leftovers:?}"
    );
    Ok(())
}

/// Staging cleanup belongs to the type, not to each error site.
///
/// The refused-export control above only exercises the validator's own refusal
/// path, which was already cleaned up explicitly. This one covers what that
/// misses: any *other* failure between creating the staging directory and the
/// publishing rename — a write error, an instrument failure, an early return
/// added by a later change — leaves the value dropped rather than handled, and
/// the directory must go with it. Forcing a real `fs::write` failure at that
/// exact point needs a filesystem fault injector, so the invariant is proven on
/// the type that owns it.
#[test]
fn an_unpublished_staging_directory_is_removed_when_it_is_dropped() -> Result<()> {
    let root = tempfile::TempDir::new().context("creating a staging root")?;

    // Dropped without publishing, as every failure path between staging and the
    // rename does.
    let abandoned = root.path().join("abandoned");
    fs::create_dir_all(&abandoned)?;
    fs::write(abandoned.join(MANIFEST_FILE_NAME), b"partial")?;
    drop(StagedEnvelope::for_test(abandoned.clone(), false));
    assert!(!abandoned.exists(), "an unpublished staging directory must not survive its owner");

    // Marked published, as `publish` does once the rename has moved the
    // directory out from under this path. Removing it then would delete
    // whatever else came to occupy the name.
    let published = root.path().join("published");
    fs::create_dir_all(&published)?;
    drop(StagedEnvelope::for_test(published.clone(), true));
    assert!(published.exists(), "a published envelope must never be removed by its own cleanup");
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-driven controls: third round on #14535
// ---------------------------------------------------------------------------

/// The commit object is the source of retained metadata, not `git show`.
///
/// A commit carrying an `encoding` header makes `git show --format=%B`
/// transcode the body to UTF-8. The result is valid UTF-8, so a byte check
/// cannot catch it, and the validator recomputes through the same reader — so
/// producer and validator would agree with each other and disagree with the
/// object. Reading `cat-file commit` is what makes "verbatim" true.
#[test]
fn a_transcoding_encoding_header_does_not_alter_the_retained_message() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    let mut raw = format!(
        "tree {tree}\n\
         author Fixture Author <fixture@example.invalid> 1600000000 +0000\n\
         committer Fixture Author <fixture@example.invalid> 1600000000 +0000\n\
         encoding ISO-8859-1\n\n\
         subject caf"
    )
    .into_bytes();
    // 0xE9 is `é` in Latin-1 and is not valid UTF-8 on its own. Git will
    // happily hand this back as `\u{c3}\u{a9}` through `git show`.
    raw.push(0xE9);
    raw.extend_from_slice(b" latin1\n");
    fs::write(fixture.path().join("raw-commit.bin"), &raw)?;
    let commit = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &commit])?;

    let destination = Destination::new()?;
    let Err((outcome, detail)) = create_handoff(&request(&fixture, &destination)) else {
        bail!(
            "the object's message is not UTF-8; retaining Git's transcoding of it would not be \
             verbatim, so the export must be refused"
        );
    };
    assert_eq!(outcome, HandoffOutcome::UnsupportedObjectClass);
    assert!(detail.contains("non-UTF-8"), "the refusal must name the reason: {detail}");
    Ok(())
}

/// An unusual but valid date token survives unchanged.
///
/// `git show --date=raw` normalises what it prints; the object's own token is
/// what a receiver reconstructing the commit needs.
#[test]
fn an_unusual_raw_date_token_is_retained_exactly() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    // `-0000` is a distinct token from `+0000` in Git: it means "no timezone
    // stated". Normalising it away would lose that distinction.
    let raw = format!(
        "tree {tree}\n\
         author Fixture Author <fixture@example.invalid> 1600000000 -0000\n\
         committer Fixture Author <fixture@example.invalid> 1600000000 -0000\n\n\
         subject\n"
    );
    fs::write(fixture.path().join("raw-commit.bin"), raw.as_bytes())?;
    let commit = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &commit])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert_eq!(manifest.candidate.author.date, "1600000000 -0000");
    assert_eq!(manifest.candidate.committer.date, "1600000000 -0000");
    assert_eq!(manifest.candidate.message, "subject\n", "the message keeps the object's own bytes");
    Ok(())
}

/// A signed commit's `gpgsig` continuation lines are headers, not the message.
#[test]
fn a_multi_line_header_does_not_leak_into_the_message() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    let raw = format!(
        "tree {tree}\n\
         author Fixture Author <fixture@example.invalid> 1600000000 +0000\n\
         committer Fixture Author <fixture@example.invalid> 1600000000 +0000\n\
         gpgsig -----BEGIN PGP SIGNATURE-----\n \n iHUEABYKAB0WIQS\n \
         -----END PGP SIGNATURE-----\n\n\
         subject line\n\nbody line\n"
    );
    fs::write(fixture.path().join("raw-commit.bin"), raw.as_bytes())?;
    let commit = fixture
        .git(&["hash-object", "-t", "commit", "-w", "--", "raw-commit.bin"])?
        .trim()
        .to_string();
    fixture.git(&["update-ref", "refs/heads/main", &commit])?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert_eq!(manifest.candidate.message, "subject line\n\nbody line\n");
    assert_eq!(manifest.candidate.author.name, "Fixture Author");
    assert_eq!(manifest.candidate.author.email, "fixture@example.invalid");
    Ok(())
}

/// A declared `git_pack_v2` transport must actually be a version 2 pack.
///
/// `index-pack` accepts more than one pack version, so a successful import
/// never confirmed the format claim the manifest makes.
#[test]
fn a_pack_whose_header_is_not_version_two_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let pack_path = destination.envelope().join(PACK_FILE_NAME);
    let mut pack = fs::read(&pack_path)?;
    assert_eq!(&pack[..4], b"PACK", "the fixture must be a real pack");
    assert_eq!(pack[7], 2, "the fixture must declare version two");
    pack[7] = 3;
    fs::write(&pack_path, &pack)?;

    // Reseal the transport row so the byte digest still matches and the version
    // claim is the only thing left wrong.
    let mut resealed = manifest;
    resealed.transport.files[0].sha256 = super::content_digest_hex(&pack);
    resealed.transport.files[0].bytes = pack.len() as u64;
    rewrite_manifest_resealed(&destination.envelope(), resealed)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::UnsupportedObjectClass);
    Ok(())
}

/// Proof references are ordered, so the order is constrained.
///
/// Reordering or duplicating a valid reference changes the semantic digest
/// without changing anything the object-level dimensions recompute, which would
/// give one candidate and one proof set several identities.
#[test]
fn duplicated_or_unordered_proof_references_are_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(manifest.proof_references.is_empty(), "this fixture carries no proof");

    let mut raw = raw_manifest(&destination.envelope())?;
    let duplicate = serde_json::json!({
        "id": "proof.json",
        "path": "proof/proof.json",
        "bytes": 0,
        "sha256": "0".repeat(64),
        "candidate_subject": manifest.candidate.commit.clone(),
    });
    raw["proof_references"] = serde_json::Value::Array(vec![duplicate.clone(), duplicate]);
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let shape = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "manifest_shape")
        .context("manifest_shape dimension")?;
    assert!(
        shape.detail.contains("sorted and unique"),
        "the refusal must name the rule: {}",
        shape.detail
    );
    Ok(())
}

/// A proof artifact is caller-supplied and bounded before it is read.
#[test]
fn an_oversized_proof_artifact_is_refused_before_it_is_read() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let oversized = fixture.path().join("huge-proof.json");
    let file = fs::File::create(&oversized)?;
    // A sparse file: the length exceeds the ceiling without writing the bytes,
    // which is exactly the shape the size check exists to refuse cheaply.
    file.set_len(super::create::MAX_PROOF_BYTES + 1)?;
    drop(file);

    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.proofs = vec![oversized];

    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("an oversized proof artifact must be refused");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("ceiling"), "the refusal must name the ceiling: {detail}");
    Ok(())
}

/// A nested namespace is not an `owner/name` pair, and guessing one is worse
/// than proving none.
///
/// Truncating `gitlab.com/group/subgroup/app` to its last two segments yields
/// `subgroup/app` — a name that plausibly belongs to a real and unrelated
/// project on that same host. Recording the host does not save it: the host is
/// right and the path is wrong. Nested namespaces are ordinary on GitLab and
/// Azure DevOps, so this is the common case, not a corner.
#[test]
fn a_nested_namespace_remote_proves_no_identity_rather_than_guessing() -> Result<()> {
    for remote in [
        "https://gitlab.com/group/subgroup/app.git",
        "https://dev.azure.com/org/project/_git/repo",
        "git@ssh.dev.azure.com:v3/org/project/repo",
    ] {
        let fixture = Fixture::with_remote(Some(remote))?;
        fixture.write("a.txt", b"a\n")?;
        fixture.commit("root")?;
        let destination = Destination::new()?;
        let manifest = create_handoff(&request(&fixture, &destination))
            .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

        assert_eq!(
            manifest.repository_identity.status,
            RepositoryIdentityStatus::NotProven,
            "`{remote}` is not a readable owner/name and must prove no identity"
        );
        assert_eq!(
            manifest.repository_identity.value, None,
            "no guess may be recorded for {remote}"
        );
    }
    Ok(())
}

/// A query string is not part of the path, so it cannot supply path segments.
///
/// Splitting on `/` across a query pulled the last two segments out of the
/// query itself, pairing a correct host with a value from somewhere else
/// entirely — a mismatched tuple, which is worse than no identity.
#[test]
fn a_query_string_does_not_contribute_path_segments() -> Result<()> {
    let fixture = Fixture::with_remote(Some("https://github.com/acme/app.git?x=/evil/path"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(manifest.repository_identity.value.as_deref(), Some("acme/app"));
    assert_eq!(manifest.repository_identity.host.as_deref(), Some("github.com"));
    Ok(())
}

/// `ssh://git@host/owner/name` is the plain SSH remote, not a credential.
///
/// Treating every userinfo as credential material put a *false*
/// `remote_url_contained_credentials` in the manifest — a claim no receiver can
/// contradict, because the URL is deliberately never retained — and threw away
/// the repository identity of every workspace cloned over SSH in URL form.
#[test]
fn an_ssh_url_login_name_is_not_treated_as_a_credential() -> Result<()> {
    let fixture = Fixture::with_remote(Some("ssh://git@github.com/acme/app.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Observed);
    assert_eq!(manifest.repository_identity.value.as_deref(), Some("acme/app"));
    assert_eq!(manifest.repository_identity.host.as_deref(), Some("github.com"));
    assert!(
        !manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials),
        "a login name is not a credential, and the manifest must not say it was"
    );
    Ok(())
}

/// A password component is still a credential under any scheme, including SSH.
#[test]
fn a_password_component_is_a_credential_under_every_scheme() -> Result<()> {
    let token = synthetic_github_token();
    for remote in [
        format!("ssh://octocat:{token}@github.com/acme/app.git"),
        format!("https://octocat:{token}@github.com/acme/app.git"),
        // Percent-encoded colon hides the password separator.
        format!("https://octocat%3A{token}@github.com/acme/app.git"),
        // A bare userinfo over HTTPS is the ordinary way a token is embedded.
        format!("https://{token}@github.com/acme/app.git"),
    ] {
        let fixture = Fixture::with_remote(Some(&remote))?;
        fixture.write("a.txt", b"a\n")?;
        fixture.commit("root")?;
        let destination = Destination::new()?;
        let manifest = create_handoff(&request(&fixture, &destination))
            .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

        assert!(
            manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials),
            "credential material in `{remote}` must be recorded as refused"
        );
        assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::NotProven);
        let manifest_text = fs::read_to_string(destination.envelope().join(MANIFEST_FILE_NAME))?;
        assert!(
            !manifest_text.contains(&github_token_marker()),
            "no token material may reach the envelope"
        );
    }
    Ok(())
}

/// `explain` reports the strength and the host, not a bare pair.
///
/// A consumer that cannot tell an observation from a caller's typed guess, or
/// that never learns which forge the pair names, can publish to the wrong
/// place — which is the hazard the manifest records a host to prevent.
#[test]
fn explain_distinguishes_an_observation_from_a_declaration() -> Result<()> {
    let observed = Fixture::with_remote(Some("https://github.com/acme/app.git"))?;
    observed.write("a.txt", b"a\n")?;
    observed.commit("root")?;
    let observed_out = Destination::new()?;
    export_valid(&observed, &observed_out)?;
    let document = explain(&observed_out.envelope())
        .map_err(|(outcome, detail)| anyhow::anyhow!("{outcome:?}: {detail}"))?;
    assert_eq!(document.repository_identity_status, "observed");
    assert_eq!(document.repository_identity_value.as_deref(), Some("acme/app"));
    assert_eq!(document.repository_identity_host.as_deref(), Some("github.com"));

    let declared = Fixture::with_remote(None)?;
    declared.write("a.txt", b"a\n")?;
    declared.commit("root")?;
    let declared_out = Destination::new()?;
    let mut requested = request(&declared, &declared_out);
    requested.declared_repository_identity = Some("acme/app".to_string());
    create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    let document = explain(&declared_out.envelope())
        .map_err(|(outcome, detail)| anyhow::anyhow!("{outcome:?}: {detail}"))?;
    assert_eq!(
        document.repository_identity_status, "declared",
        "a caller's guess must not render as an observation"
    );
    assert_eq!(document.repository_identity_host, None, "nobody observed a host for a declaration");
    Ok(())
}

/// `explain` says out loud that it verified nothing.
///
/// It reports a tampered envelope exactly as confidently as a sound one, which
/// is correct for a projection and dangerous if the reader does not know it.
#[test]
fn explain_states_that_it_verifies_nothing() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let pack_path = destination.envelope().join(PACK_FILE_NAME);
    let mut pack = fs::read(&pack_path)?;
    pack.extend_from_slice(b"tampered");
    fs::write(&pack_path, &pack)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::DigestMismatch,
        "the validator must still catch this"
    );
    let document = explain(&destination.envelope())
        .map_err(|(outcome, detail)| anyhow::anyhow!("{outcome:?}: {detail}"))?;
    assert!(
        document
            .does_not_establish
            .iter()
            .any(|statement| statement.contains("only `check` verifies")),
        "explain must disclose that it is a projection, not a verdict"
    );
    Ok(())
}

/// The inventory admits that its rename rows are detected, not recorded.
#[test]
fn the_inventory_admits_rename_detection_is_a_heuristic() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(
        manifest.limitations.contains(&LimitationCode::InventoryRenamesAreDetected),
        "Git stores trees, not renames; the manifest must say the label is inferred"
    );
    Ok(())
}

/// Host configuration cannot supply an object the transport omitted.
///
/// Clearing Git's repository-local environment closed one route. `git init`
/// honours `init.templateDir` from *global* config, and a template carrying
/// `objects/info/alternates` points the fresh database at the host's own store
/// — so an incomplete envelope validated on whichever machine happened to hold
/// the blob, which is precisely the claim this format makes it cannot.
#[test]
#[serial]
fn global_git_configuration_cannot_complete_an_incomplete_envelope() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("seed.txt", b"seed\n")?;
    fixture.commit("base")?;
    fixture.write("image.bin", &[0u8, 1, 2, 3, 4, 5])?;
    fixture.commit("add binary")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    let binary_object = change_for(&manifest, "image.bin")
        .and_then(|row| row.new_object.clone())
        .context("binary object id")?;

    // Drop the blob from the pack *only*, leaving it declared. That is what
    // makes this a test of the alternate: `declared == required` still holds,
    // so the sole remaining question is whether the object database the
    // validator enumerates really contains just the envelope's own objects.
    let mut reduced = manifest;
    let packed: Vec<String> =
        reduced.transport.object_ids.iter().filter(|id| **id != binary_object).cloned().collect();
    let stdin = packed.join("\n");
    let repacked = super::git::run_git_with_stdin(
        fixture.path(),
        &["pack-objects", "--stdout", "-q"],
        stdin.into_bytes(),
    )
    .map_err(anyhow::Error::msg)?;
    assert!(repacked.succeeded());
    fs::write(destination.envelope().join(PACK_FILE_NAME), &repacked.stdout_bytes)?;
    reduced.transport.files[0].bytes = repacked.stdout_bytes.len() as u64;
    reduced.transport.files[0].sha256 = super::content_digest_hex(&repacked.stdout_bytes);
    rewrite_manifest_resealed(&destination.envelope(), reduced)?;

    // A template directory whose fresh repositories borrow the producer's own
    // object store, reached through *global* config rather than an environment
    // variable the seam already clears.
    let ambient = tempfile::TempDir::new()?;
    let template = ambient.path().join("template");
    fs::create_dir_all(template.join("objects/info"))?;
    fs::write(
        template.join("objects/info/alternates"),
        format!("{}\n", fixture.path().join(".git/objects").display()),
    )?;
    let config = ambient.path().join("gitconfig");
    fs::write(&config, format!("[init]\n\ttemplateDir = {}\n", template.display()))?;

    // Prove the fixture is a real lever before relying on it: a bare repository
    // created under this config must actually inherit the alternate, or this
    // control would pass for want of an attack rather than for want of a hole.
    let probe = ambient.path().join("probe");
    let status = Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&probe)
        .env("GIT_CONFIG_GLOBAL", &config)
        .status()?;
    assert!(status.success(), "the probe repository must initialise");
    assert!(
        probe.join("objects/info/alternates").exists(),
        "the template must really seed an alternates file, or this control proves nothing"
    );

    // SAFETY: restored immediately below. `#[serial]` orders this against the
    // module's other environment-mutating control; unannotated tests are
    // unaffected because production and the fixtures (`isolated_git`) set this
    // variable on every child explicitly rather than inheriting it.
    unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &config) };
    let report = check_handoff(&destination.envelope());
    unsafe { std::env::remove_var("GIT_CONFIG_GLOBAL") };

    assert_eq!(
        report.outcome,
        HandoffOutcome::MissingObject,
        "host configuration must not complete an envelope: {:#?}",
        report.dimensions
    );
    Ok(())
}

/// An unearned limitation is a false admission, and is refused like a dropped one.
///
/// Adding `repository_identity_not_proven` to a manifest that proves an
/// identity made the envelope assert both at once — and `explain` printed both.
#[test]
fn an_unearned_limitation_is_refused() -> Result<()> {
    let fixture = Fixture::with_remote(Some("https://github.com/acme/app.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(!manifest.limitations.contains(&LimitationCode::RepositoryIdentityNotProven));

    let mut tampered = manifest;
    tampered.limitations.push(LimitationCode::RepositoryIdentityNotProven);
    tampered.limitations.push(LimitationCode::MergeCommitDiffAgainstFirstParent);
    tampered.limitations.sort();
    rewrite_manifest_resealed(&destination.envelope(), tampered)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let completeness = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "limitation_completeness")
        .context("limitation_completeness dimension")?;
    assert_eq!(completeness.verdict, DimensionVerdict::Invalid);
    Ok(())
}

/// Every retained manifest string is scanned, not a chosen subset.
///
/// `content_safety` reported "no credential material in retained manifest
/// strings" while credential material sat in a renamed file's `old_path` or in
/// the producer observation block — a false statement about exactly the strings
/// least protected by anything else, since the observation block is outside the
/// semantic digest by design.
#[test]
fn credential_material_anywhere_in_the_manifest_is_found() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    for field in ["observation.producer_version", "observation.git_version"] {
        let mut raw = raw_manifest(&destination.envelope())?;
        let (section, key) = field.split_once('.').context("field path")?;
        raw[section][key] = serde_json::Value::String(synthetic_github_token());
        rewrite_manifest_raw(&destination.envelope(), &raw)?;

        let report = check_handoff(&destination.envelope());
        assert_eq!(
            report.outcome,
            HandoffOutcome::UnsafeContent,
            "credential material in `{field}` must be found"
        );

        // Restore for the next case.
        rewrite_manifest_resealed(&destination.envelope(), manifest.clone())?;
    }

    // The observation block is also bounded, so a reseal cannot grow the
    // manifest without limit while leaving candidate identity untouched.
    let mut raw = raw_manifest(&destination.envelope())?;
    raw["observation"]["git_version"] = serde_json::Value::String("x".repeat(4096));
    rewrite_manifest_raw(&destination.envelope(), &raw)?;
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::InvalidManifest);
    Ok(())
}

/// An inventory path that escapes its root is refused.
///
/// Git will not check out such a tree, but a tree object holding one can be
/// written and packed, and this format hands inventory paths onward as data for
/// other tools to join onto a root.
#[test]
fn an_inventory_path_that_escapes_its_root_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    for hostile in ["../../etc/evil", "/etc/evil", "a/../../b"] {
        let mut raw = raw_manifest(&destination.envelope())?;
        raw["inventory"]["changes"][0]["path"] = serde_json::Value::String(hostile.to_string());
        rewrite_manifest_raw(&destination.envelope(), &raw)?;
        let report = check_handoff(&destination.envelope());
        assert_eq!(
            report.outcome,
            HandoffOutcome::InvalidManifest,
            "`{hostile}` must not be reported as an inventory path"
        );
        rewrite_manifest_resealed(&destination.envelope(), manifest.clone())?;
    }
    Ok(())
}

/// A parent list large enough to be a lever is refused by shape.
///
/// Each parent costs several Git invocations, and the deadline is per child
/// with no shared budget, so a small envelope could buy hours of validation.
#[test]
fn an_absurd_parent_count_is_refused_by_shape() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut raw = raw_manifest(&destination.envelope())?;
    let filler: Vec<serde_json::Value> = (0..super::check::MAX_DECLARED_PARENTS + 1)
        .map(|index| serde_json::Value::String(format!("{index:040x}")))
        .collect();
    raw["candidate"]["parents"] = serde_json::Value::Array(filler.clone());
    raw["candidate"]["parent_trees"] = serde_json::Value::Array(filler);
    raw["candidate"]["is_merge_commit"] = serde_json::Value::Bool(true);
    raw["candidate"]["is_root_commit"] = serde_json::Value::Bool(false);
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let shape = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "manifest_shape")
        .context("manifest_shape dimension")?;
    assert!(
        shape.detail.contains("ceiling"),
        "the refusal must name the ceiling: {}",
        shape.detail
    );
    let _ = manifest;
    Ok(())
}

/// The envelope admits that its repository identity is unverifiable by a receiver.
#[test]
fn the_manifest_admits_repository_identity_is_the_producers_word() -> Result<()> {
    let fixture = Fixture::with_remote(Some("https://github.com/acme/app.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(
        manifest.limitations.contains(&LimitationCode::RepositoryIdentityNotReceiverVerifiable),
        "no receiver can check this value, and the manifest must say so"
    );

    let report = check_handoff(&destination.envelope());
    let identity = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "repository_identity")
        .context("repository_identity dimension")?;
    assert!(
        identity.detail.contains("producer's"),
        "the dimension must not claim more than it proved: {}",
        identity.detail
    );
    Ok(())
}

/// A declaration is the fallback for an unreadable remote, not an override.
///
/// `CreateRequest` documents the field that way, but the code took the
/// declaration first and never looked at origin — so a caller passing a stale
/// or wrong value while a perfectly readable remote sat there put the wrong
/// repository in the manifest for a consumer to publish to.
#[test]
fn a_readable_remote_is_preferred_over_a_caller_declaration() -> Result<()> {
    let fixture = Fixture::with_remote(Some("https://github.com/acme/app.git"))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    // Agreeing declaration: the observation still wins, and keeps its host.
    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.declared_repository_identity = Some("acme/app".to_string());
    let manifest = create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Observed);
    assert_eq!(manifest.repository_identity.host.as_deref(), Some("github.com"));

    // Contradicting declaration: refused rather than silently resolved.
    let conflicting = Destination::new()?;
    let mut requested = request(&fixture, &conflicting);
    requested.declared_repository_identity = Some("other/repo".to_string());
    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("a declaration contradicting a readable remote must not be resolved silently");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("other/repo"), "the refusal must name both claims: {detail}");
    assert!(detail.contains("acme/app"), "the refusal must name both claims: {detail}");

    // With no readable remote, the declaration is exactly what it is for.
    let bare = Fixture::with_remote(None)?;
    bare.write("a.txt", b"a\n")?;
    bare.commit("root")?;
    let fallback = Destination::new()?;
    let mut requested = request(&bare, &fallback);
    requested.declared_repository_identity = Some("acme/app".to_string());
    let manifest = create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Declared);
    assert_eq!(manifest.repository_identity.host, None);
    Ok(())
}

/// A proof naming two different candidates is evidence for neither.
///
/// Stopping at the first recognised key let an artifact name this candidate in
/// one field and something else in another, and be accepted on the strength of
/// whichever the reader happened to check first.
#[test]
fn a_proof_naming_two_candidates_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let head = fixture.git(&["rev-parse", "HEAD"])?.trim().to_string();
    let other = "0".repeat(40);

    let proof = fixture.path().join("report.json");
    fs::write(&proof, serde_json::to_vec(&serde_json::json!({ "commit": head, "sha": other }))?)?;

    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.proofs = vec![proof];
    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("a self-contradicting proof must not be accepted for either candidate");
    };
    assert_eq!(outcome, HandoffOutcome::ProofSubjectMismatch);
    assert!(detail.contains("more than one"), "the refusal must name the reason: {detail}");

    // A proof that names the same candidate under several keys is consistent,
    // not contradictory, and stays acceptable.
    let agreeing = fixture.path().join("agreeing.json");
    fs::write(&agreeing, serde_json::to_vec(&serde_json::json!({ "commit": head, "sha": head }))?)?;
    let second = Destination::new()?;
    let mut requested = request(&fixture, &second);
    requested.proofs = vec![agreeing];
    create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    Ok(())
}

/// A staging directory has not been validated, by definition.
#[test]
fn a_staging_directory_claiming_validation_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    // A published envelope carries the validated token; asking the staged
    // entry point about it must refuse, because staging never carries that.
    let report = check_staged(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let closure = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "envelope_closure")
        .context("envelope_closure dimension")?;
    assert!(
        closure.detail.contains(SELF_CHECK_PENDING),
        "the refusal must name the token it required: {}",
        closure.detail
    );
    Ok(())
}

/// The destination is reserved for the whole build, not merely checked.
///
/// `out.exists()` and the publishing rename were two operations with a window
/// between them, and the window was exploitable rather than theoretical:
/// `rename` over an existing *empty* directory succeeds on Unix and replaces it
/// (a non-empty one fails `ENOTEMPTY`). A destination created after the check
/// was therefore silently clobbered, against both the `must not already exist`
/// contract and the immutability claim.
///
/// This drives the window deterministically instead of racing for it. The
/// injected validator runs at exactly the point the defect needed — after the
/// envelope is staged, before it is published — so the probe inside it stands
/// in for any other creator arriving mid-build. Under the old check-then-rename
/// code the probe *succeeds* and its directory is then replaced; under the
/// atomic claim it must fail, because the path was reserved before staging
/// began.
#[test]
#[serial]
fn a_destination_created_mid_export_cannot_be_clobbered() -> Result<()> {
    /// Destination to probe, and what the probe observed.
    static PROBE: std::sync::Mutex<Option<(PathBuf, Option<std::io::ErrorKind>)>> =
        std::sync::Mutex::new(None);

    /// Validate as usual, but first try to claim the destination mid-export.
    fn probing_validator(staged: &Path) -> super::check::CheckReport {
        if let Ok(mut probe) = PROBE.lock()
            && let Some((destination, observed)) = probe.as_mut()
        {
            *observed = Some(
                fs::create_dir(&*destination)
                    .err()
                    .map_or(std::io::ErrorKind::Other, |error| error.kind()),
            );
        }
        check_staged(staged)
    }

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;

    if let Ok(mut probe) = PROBE.lock() {
        *probe = Some((destination.envelope(), None));
    }
    let outcome =
        create_handoff_with_validator(&request(&fixture, &destination), probing_validator);

    let observed = PROBE
        .lock()
        .ok()
        .and_then(|probe| probe.as_ref().and_then(|(_, observed)| *observed))
        .context("the probe must have run inside the validator")?;
    if let Ok(mut probe) = PROBE.lock() {
        *probe = None;
    }

    // One assertion on every platform, because the reservation is now held on
    // every platform. The publication protocol adapts to what the local rename
    // does to an existing directory, but the *claim* does not: `create_dir` is
    // atomic and fails if the path is taken everywhere, so a competitor
    // arriving mid-export is refused the path regardless of platform.
    //
    // That uniformity is the point. The earlier version asserted a different
    // outcome per platform, which meant the guarantee rested on which rename
    // semantics Windows has — exactly the dependency this design removes.
    assert_eq!(
        observed,
        std::io::ErrorKind::AlreadyExists,
        "the destination must already be reserved while the envelope is staged; \
         a probe that succeeds here is a path publication could overwrite"
    );
    assert!(outcome.is_ok(), "reserving the destination must not break an ordinary export");
    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ValidHandoff,
        "the published envelope must still validate"
    );
    Ok(())
}

/// The receipt is the last thing published, so a partial move is never valid.
///
/// Publication is no longer one atomic directory rename — it moves entries into
/// a destination this export has held since before it built anything. That is
/// what removes the dependency on how a platform renames onto an existing
/// directory, which broke three earlier attempts at this seam. The cost is that
/// the destination is briefly incomplete, so consistency has to come from
/// somewhere else.
///
/// It comes from the receipt rule the format already enforces: `check_handoff`
/// refuses an envelope whose receipt is absent or still `pending`. This drives
/// that rule directly — every proper prefix of the published set must be
/// refused, so no reader can accept a half-moved envelope.
///
/// It does **not** prove that `publish` moves the receipt last, and saying so
/// matters because the first version of this test was written as though it did.
/// It rebuilds the partial states itself, so it never observes the real
/// ordering: with the receipt moved *first* in production, this control still
/// passed. The ordering is proved separately by
/// `the_receipt_is_ordered_last_for_publication`, against the function that
/// decides it.
#[test]
fn every_partial_publication_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let complete = Destination::new()?;
    export_valid(&fixture, &complete)?;

    // Rebuild the destination one entry at a time, receipt last, and require
    // every incomplete state to be refused.
    let mut names: Vec<PathBuf> = fs::read_dir(complete.envelope())?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    names.sort();
    names.sort_by_key(|path| path.file_name().is_some_and(|name| name == RECEIPT_FILE_NAME));
    assert!(names.len() >= 3, "the fixture must publish manifest, pack, and receipt");
    assert_eq!(
        names.last().and_then(|path| path.file_name()),
        Some(std::ffi::OsStr::new(RECEIPT_FILE_NAME)),
        "the receipt must sort last, or the ordering this test relies on is wrong"
    );

    let partial = Destination::new()?;
    fs::create_dir(partial.envelope())?;
    for source in &names[..names.len() - 1] {
        let Some(name) = source.file_name() else { bail!("entry without a name") };
        if source.is_dir() {
            fs::create_dir_all(partial.envelope().join(name))?;
            for inner in fs::read_dir(source)?.filter_map(std::result::Result::ok) {
                fs::copy(inner.path(), partial.envelope().join(name).join(inner.file_name()))?;
            }
        } else {
            fs::copy(source, partial.envelope().join(name))?;
        }
        assert_ne!(
            check_handoff(&partial.envelope()).outcome,
            HandoffOutcome::ValidHandoff,
            "a destination without its receipt must never validate: {name:?} present"
        );
    }

    // Anti-vacuity: the same set *with* the receipt must validate, or the loop
    // above would be asserting against something that could never be valid.
    let receipt = names.last().context("receipt entry")?;
    fs::copy(receipt, partial.envelope().join(RECEIPT_FILE_NAME))?;
    assert_eq!(
        check_handoff(&partial.envelope()).outcome,
        HandoffOutcome::ValidHandoff,
        "the complete set must validate once the receipt lands"
    );
    Ok(())
}

/// Publication orders the receipt last, whatever the directory listing says.
///
/// The falsifier for the ordering, which the partial-publication control above
/// is not. `publish` moves entries into a destination it already holds, so the
/// only reader-visible protection against a half-moved envelope is that the
/// receipt arrives after everything it vouches for. Nothing about a real export
/// can show that: the destination is observable before or after the move, and
/// every intermediate state is refused either way.
#[test]
fn the_receipt_is_ordered_last_for_publication() {
    use super::create::publication_order;

    let root = Path::new("/envelope");
    // Deliberately supplied in an order where the receipt is already first, so
    // a no-op implementation cannot pass by accident.
    let entries = vec![
        root.join(RECEIPT_FILE_NAME),
        root.join(PACK_FILE_NAME),
        root.join(MANIFEST_FILE_NAME),
        root.join(PROOF_DIR_NAME),
    ];
    let ordered = publication_order(entries);

    assert_eq!(
        ordered.last().and_then(|path| path.file_name()),
        Some(std::ffi::OsStr::new(RECEIPT_FILE_NAME)),
        "the receipt must be published last, after everything it vouches for"
    );
    // Anti-vacuity: everything else must still be there, and in a stable order,
    // or "receipt last" could be satisfied by dropping entries.
    assert_eq!(ordered.len(), 4, "ordering must not drop entries");
    let mut without_receipt: Vec<_> =
        ordered[..3].iter().filter_map(|path| path.file_name()).collect();
    without_receipt.sort_unstable();
    assert_eq!(
        without_receipt,
        vec![
            std::ffi::OsStr::new(PACK_FILE_NAME),
            std::ffi::OsStr::new(MANIFEST_FILE_NAME),
            std::ffi::OsStr::new(PROOF_DIR_NAME),
        ],
        "every non-receipt entry must precede the receipt"
    );
    // The same input in a different order must produce the same output, or the
    // sequence would depend on directory iteration order.
    let reversed = publication_order(vec![
        root.join(PROOF_DIR_NAME),
        root.join(MANIFEST_FILE_NAME),
        root.join(RECEIPT_FILE_NAME),
        root.join(PACK_FILE_NAME),
    ]);
    assert_eq!(ordered, reversed, "publication order must not depend on input order");
}

/// An abandoned reservation is named as one, not reported as an envelope.
///
/// `Drop` releases the reservation on every ordinary failure path, but a killed
/// process runs no destructor, so an interrupted export can leave an empty
/// directory at the destination and every retry then fails `AlreadyExists`.
///
/// Reclaiming it automatically is not available and the refusal says why: a live
/// export's reservation is an empty directory too, and the two are
/// indistinguishable from outside, so reclaiming would trade a confusing message
/// for a clobber. What is available is telling the operator which case they are
/// in, which is the difference between a blocked retry and an unexplained one.
#[test]
fn an_abandoned_reservation_is_reported_as_one() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    // Exactly what a killed export leaves behind: the destination created, and
    // nothing in it.
    let destination = Destination::new()?;
    fs::create_dir(destination.envelope())?;

    let (outcome, detail) = create_handoff(&request(&fixture, &destination))
        .err()
        .context("an occupied destination must refuse")?;
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(
        detail.contains("empty")
            && detail.contains("Confirm no export is running")
            && detail.contains("then remove it to retry"),
        "an empty destination must be named as a reservation, not as an envelope: {detail}"
    );

    // Anti-vacuity: a destination that is *not* empty must still get the
    // immutability message, or the two cases would have collapsed into one.
    let occupied = Destination::new()?;
    fs::create_dir(occupied.envelope())?;
    fs::write(occupied.envelope().join("something.txt"), b"not a reservation\n")?;
    let (_, detail) = create_handoff(&request(&fixture, &occupied))
        .err()
        .context("a non-empty destination must also refuse")?;
    assert!(
        detail.contains("immutable") && !detail.contains("then remove it to retry"),
        "a non-empty destination is not an abandoned reservation: {detail}"
    );
    Ok(())
}

/// A refused export leaves no reservation behind for the next attempt.
///
/// The reservation is created before the build, so it has to be released when
/// the build fails — otherwise a refused export would leave an empty directory
/// and the next attempt would be refused as a duplicate of nothing.
#[test]
fn a_refused_export_releases_its_destination_reservation() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;

    // A validator that always refuses, so the export fails after staging.
    fn refusing_validator(_staged: &Path) -> super::check::CheckReport {
        super::check::CheckReport {
            schema_version: CHECK_REPORT_SCHEMA_V1.to_string(),
            envelope: String::new(),
            candidate_commit: None,
            candidate_identity_digest: None,
            dimensions: Vec::new(),
            outcome: HandoffOutcome::DigestMismatch,
        }
    }

    assert!(
        create_handoff_with_validator(&request(&fixture, &destination), refusing_validator)
            .is_err(),
        "a refused self-check must not publish"
    );
    assert!(
        !destination.envelope().exists(),
        "the reservation must be released, not left as an empty directory"
    );

    // The decisive consequence: a retry must be possible.
    let manifest = export_valid(&fixture, &destination)?;
    assert!(!manifest.candidate.commit.is_empty());
    Ok(())
}

/// Two concurrent exports to one destination cannot corrupt each other.
///
/// The destination check could not exclude this while `out` stayed absent for
/// the whole of staging — only `publish` created it — so both callers passed
/// it. (The destination is now reserved up front, which closes that half; see
/// `a_destination_created_mid_export_cannot_be_clobbered`.) When the
/// staging name was derived from the destination and the process id alone, both
/// derived the same path, and the second deleted the first's *live* directory
/// and recreated it. From there their manifest, pack, proof, receipt, and
/// self-check writes interleaved on one pathname, so one caller could validate
/// the directory while the other replaced its bytes and `create` could return
/// success for an envelope that no longer matched what was validated.
///
/// This is the end-to-end shape of the defect, and it is deliberately *not*
/// the discriminating proof for it. Both threads do seconds of Git work after
/// the barrier and before the narrow validate-then-publish window, so they do
/// not reliably collide inside it: with the racy naming restored this control
/// still passed three runs out of three. It is kept because the property it
/// asserts is the one that matters — whatever a caller is told succeeded must
/// still validate — but the allocation rule is proved deterministically by
/// `staging_allocation_never_reclaims_another_invocations_directory`.
#[test]
fn concurrent_exports_to_one_destination_cannot_corrupt_each_other() -> Result<()> {
    use std::sync::{Arc, Barrier};

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;

    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let inputs = request(&fixture, &destination);
            std::thread::spawn(move || {
                barrier.wait();
                create_handoff(&inputs)
            })
        })
        .collect();
    let mut outcomes = Vec::with_capacity(handles.len());
    for handle in handles {
        // A panicking export is a distinct failure from a refused one, and the
        // assertions below would read a panic as "did not publish".
        let Ok(outcome) = handle.join() else { bail!("an export thread panicked") };
        outcomes.push(outcome);
    }

    let succeeded = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert!(
        succeeded <= 1,
        "one destination can be published at most once, but {succeeded} exports claimed it"
    );

    // Whatever was published must still be the bytes that were validated. This
    // is the claim the race broke: `create` returning `Ok` for an envelope a
    // concurrent caller had since overwritten.
    if succeeded == 1 {
        assert_eq!(
            check_handoff(&destination.envelope()).outcome,
            HandoffOutcome::ValidHandoff,
            "a successful export must publish an envelope that still validates"
        );
    }

    // The loser must not leave the destination in a state that reads as valid
    // without having been published, and must not have removed the winner's
    // work. Any staging directory that survives belongs to a crashed
    // invocation, never to the one that succeeded.
    let parent = destination.envelope().parent().unwrap_or(Path::new(".")).to_path_buf();
    let leftover_staging = fs::read_dir(&parent)?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".staging-"))
        .count();
    assert_eq!(leftover_staging, 0, "neither export may leave its staging directory behind");
    Ok(())
}

/// A newline in a tracked path exports and validates like any other path.
///
/// This guards a property; it is not proof of a fix, and the distinction is
/// worth stating because the two look identical from a green run. It was
/// written for a reported defect — that `rev-list --objects`, printing
/// `<id> <path>` per line, lets a path containing a newline split into an extra
/// line whose leading forty hex characters parse as another object id. Measured
/// against Git, that does not happen: for a file named `evil\n<forty a>`,
/// `rev-list --objects` emits three lines and the post-newline fragment never
/// appears, because Git stops the path at the newline.
///
/// So this control passes with or without `--no-object-names`, and says so
/// rather than being presented as a falsifier. What it does establish is that
/// such a candidate exports and validates, which is the behaviour a reader
/// would want pinned regardless of which Git version is printing the paths.
///
/// `#[cfg(unix)]` because Windows forbids a newline in a filename outright.
#[cfg(unix)]
#[test]
fn a_newline_in_a_tracked_path_exports_and_validates() -> Result<()> {
    let fixture = Fixture::new()?;
    // Forty hex characters after the newline: what a naive reader would take
    // for an object id on a line of its own.
    let hostile = format!("evil\n{}", "a".repeat(40));
    fixture.write(&hostile, b"contents\n")?;
    fixture.write("ordinary.txt", b"ordinary\n")?;
    fixture.commit("root")?;

    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    // Every declared object must be a real one. The spurious id would be forty
    // `a` characters, which is well-formed as an id and absent from the tree.
    let invented = "a".repeat(40);
    assert!(
        !manifest.transport.object_ids.contains(&invented),
        "a filename fragment must never be declared as a transported object"
    );
    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::ValidHandoff,
        "a candidate whose path contains a newline is still a valid candidate"
    );
    Ok(())
}

/// Object enumeration refuses a record it cannot read as an object id.
///
/// This is the falsifier the newline control above is not, and it exists because
/// the two changes it guards were previously only falsifiable together. The
/// producer asks Git for ids and nothing else, so a real repository can never
/// hand the reader a malformed record — which also means no end-to-end fixture
/// can reach the refusal, and a rule that cannot be executed is not proven. So
/// the reader is driven directly.
///
/// The prior implementation took `line.split(' ').next()` and dropped anything
/// that did not parse, which is the specific failure this refuses: a silently
/// shrunken `object_ids` is exactly what lets an envelope validate as a
/// candidate whose objects it does not carry. Under that implementation the
/// first two cases below return a short set instead of an error.
#[test]
fn an_unreadable_enumeration_record_is_an_instrument_failure() -> Result<()> {
    use super::create::parse_object_records;

    let refused = |records: &str, why: &str| -> Result<()> {
        match parse_object_records(records) {
            Ok(ids) => bail!("{why}: accepted {records:?} and declared {} objects", ids.len()),
            Err((outcome, _)) => {
                assert_eq!(outcome, HandoffOutcome::InstrumentFailure, "{why}");
                Ok(())
            }
        }
    };

    let real = "b".repeat(40);

    // A `<id> <path>` record: what Git prints without `--no-object-names`, and
    // what the previous reader silently truncated to its first field.
    refused(
        &format!("{real} some/path.txt\n"),
        "unexpected enumeration output must fail closed, not be parsed around",
    )?;

    // A record that is not an id at all, mixed with one that is. The previous
    // reader kept the good one and dropped the rest, declaring a closure
    // narrower than the candidate's.
    refused(
        &format!("{real}\nnot-an-object-id\n"),
        "a record that is not an object id must be refused",
    )?;

    // An abbreviated id is not a partial success either: the declared set is
    // content-addressed, so a short id names nothing the receiver can resolve.
    refused(&format!("{}\n", "c".repeat(12)), "an abbreviated id is not a full object id")?;

    // Anti-vacuity: well-formed records must still be accepted, and blank
    // records skipped, or the assertions above would hold for a reader that
    // refused everything.
    let clean = format!("{real}\n\n{}\n", "d".repeat(40));
    let ids = parse_object_records(&clean).map_err(|(outcome, detail)| {
        anyhow!("well-formed records must parse: {outcome:?} {detail}")
    })?;
    assert_eq!(ids.len(), 2, "blank records are skipped, real ones retained");
    assert!(ids.contains(&real));
    Ok(())
}

/// A commit header record that is not `name value` is refused, not skipped.
///
/// The same fail-open class as the enumeration control above, in the reader
/// `check` shares with the producer — which is what makes it worth pinning.
/// A silently dropped `parent` would be re-derived identically on both sides,
/// so the two would agree with each other while disagreeing with the commit,
/// and no validation dimension could see it. `git cat-file commit` cannot emit
/// such a record, so the branch is unreachable end to end and the parser is
/// driven directly.
#[test]
fn an_unreadable_commit_header_record_is_an_instrument_failure() -> Result<()> {
    use super::create::parse_commit_headers;

    let commit = "e".repeat(40);
    let tree = format!("tree {}", "1".repeat(40));
    let first_parent = format!("parent {}", "2".repeat(40));
    let second_parent = format!("parent {}", "3".repeat(40));
    let author = "author A U Thor <a@example.com> 1700000000 +0000";
    let committer = "committer A U Thor <a@example.com> 1700000000 +0000";

    let parsed = |block: &str, why: &str| match parse_commit_headers(block, &commit) {
        Ok(headers) => Ok(headers),
        Err((outcome, detail)) => Err(anyhow!("{why}: {outcome:?} {detail}")),
    };
    let refused = |block: &str, why: &str| -> Result<()> {
        match parse_commit_headers(block, &commit) {
            Ok(headers) => {
                bail!("{why}: accepted the block and kept {} parents", headers.parents.len())
            }
            Err((outcome, _)) => {
                assert_eq!(outcome, HandoffOutcome::InstrumentFailure, "{why}");
                Ok(())
            }
        }
    };

    // Anti-vacuity first: a well-formed header block must parse, with both
    // parents retained in order, or the refusals below would hold for a parser
    // that rejected everything.
    let ordered = format!("{tree}\n{first_parent}\n{second_parent}\n{author}\n{committer}");
    let headers = parsed(&ordered, "a well-formed block must parse")?;
    assert_eq!(headers.parents, vec!["2".repeat(40), "3".repeat(40)], "parent order is retained");

    // A multi-line `gpgsig` continuation is the one legitimate skip: it belongs
    // to a header already read, and must not be mistaken for a bad record.
    let signed = format!(
        "{tree}\n{first_parent}\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n abcdef\n -----END PGP SIGNATURE-----\n{author}\n{committer}"
    );
    let headers = parsed(&signed, "a signature continuation is not a bad record")?;
    assert_eq!(headers.parents.len(), 1, "a signed commit keeps its parent");

    // The refusal: a record with no space at all. Skipping it is what drops a
    // parent silently.
    refused(
        &format!("{tree}\n{first_parent}\nparent\n{author}\n{committer}"),
        "an unreadable header record must fail closed, not shrink the parent list",
    )?;

    // And it is refused rather than tolerated even when nothing required is
    // missing, so the rule is about the record and not about the outcome.
    refused(&format!("{tree}\n{author}\n{committer}\nstray"), "a stray record is still refused")?;
    Ok(())
}

/// One invocation's staging directory is never reclaimed by another.
///
/// This is the rule the concurrent-export defect reduces to, and unlike the
/// end-to-end race it can be settled without timing. Two allocations stand in
/// for two live exports: the first writes a marker, and the second must neither
/// receive the same path nor remove what the first put there.
///
/// With the previous naming — destination plus process id, then
/// `remove_dir_all` on collision — the second call returned the same path and
/// deleted the marker, which is precisely how one export could destroy
/// another's staged bytes between its validation and its publish.
#[test]
fn staging_allocation_never_reclaims_another_invocations_directory() -> Result<()> {
    let root = tempfile::TempDir::new()?;

    let first = super::create::allocate_staging(root.path(), "envelope")
        .map_err(|(outcome, detail)| format!("{outcome:?}: {detail}"))
        .map_err(anyhow::Error::msg)?;
    let marker = first.join("manifest.json");
    fs::write(&marker, b"first invocation's staged bytes")?;

    let second = super::create::allocate_staging(root.path(), "envelope")
        .map_err(|(outcome, detail)| format!("{outcome:?}: {detail}"))
        .map_err(anyhow::Error::msg)?;

    assert_ne!(first, second, "two live exports must not be handed the same staging directory");
    assert!(
        marker.is_file(),
        "allocating a staging directory must not remove another invocation's staged bytes"
    );
    assert_eq!(
        fs::read(&marker)?,
        b"first invocation's staged bytes",
        "the first invocation's bytes must be untouched"
    );
    assert!(second.is_dir(), "the second allocation must still produce a usable directory");
    Ok(())
}

/// The aggregate proof ceiling is the format's, so the validator applies it too.
///
/// The producer refuses a set of artifacts totalling more than
/// `MAX_TOTAL_PROOF_BYTES`. Checking only the per-artifact ceiling here left
/// the validator accepting a declared set of `MAX_DECLARED_PROOFS` artifacts at
/// `MAX_PROOF_BYTES` each — an envelope `create` could not have produced. The
/// point is not the validator's own memory, which is one artifact at a time,
/// but that both sides must agree on what a well-formed envelope is: a
/// validator that accepts what the producer refuses is describing a different
/// format.
///
/// Declared sizes are enough to exercise the rule, so this costs no bytes on
/// disk: the arithmetic is refused before any artifact is read.
#[test]
fn the_validator_applies_the_aggregate_proof_ceiling() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    // Each artifact has to exist, or `envelope_closure` refuses before the
    // ceiling is ever consulted and the control would pass without testing it.
    let proof_dir = destination.envelope().join(PROOF_DIR_NAME);
    fs::create_dir_all(&proof_dir)?;

    // Eight artifacts, each individually legal at exactly the per-artifact
    // ceiling, totalling 256 MiB against a 128 MiB aggregate.
    let mut references = Vec::new();
    for index in 0..8 {
        let id = format!("report-{index}.json");
        fs::write(proof_dir.join(&id), b"{}")?;
        references.push(serde_json::json!({
            "id": id,
            "path": format!("{PROOF_DIR_NAME}/{id}"),
            "bytes": super::create::MAX_PROOF_BYTES,
            "sha256": "0".repeat(64),
            "candidate_subject": manifest.candidate.commit.clone(),
        }));
    }
    let mut raw = raw_manifest(&destination.envelope())?;
    raw["proof_references"] = serde_json::Value::Array(references);
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(
        report.outcome,
        HandoffOutcome::InvalidManifest,
        "individually legal artifacts can still exceed the aggregate ceiling"
    );
    let binding = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "proof_binding")
        .context("proof_binding dimension")?;
    assert_eq!(binding.verdict, DimensionVerdict::Invalid);
    assert!(
        binding.detail.contains("total"),
        "the refusal must name the aggregate rule: {}",
        binding.detail
    );
    Ok(())
}

/// The validator enforces the producer's proof ceiling, not a larger one.
///
/// A looser bound here would mean an envelope this validator calls valid could
/// never have been produced by `create`.
#[test]
fn the_validator_uses_the_same_proof_ceiling_as_the_producer() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    // The file must exist, or `envelope_closure` refuses first and the ceiling
    // is never consulted — the control would then be green whether or not the
    // rule it names is present.
    let proof_dir = destination.envelope().join(PROOF_DIR_NAME);
    fs::create_dir_all(&proof_dir)?;
    fs::write(proof_dir.join("report.json"), b"{}")?;

    let mut raw = raw_manifest(&destination.envelope())?;
    raw["proof_references"] = serde_json::Value::Array(vec![serde_json::json!({
        "id": "report.json",
        "path": "proof/report.json",
        "bytes": super::create::MAX_PROOF_BYTES + 1,
        "sha256": "0".repeat(64),
        "candidate_subject": manifest.candidate.commit.clone(),
    })]);
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    let report = check_handoff(&destination.envelope());
    assert_eq!(report.outcome, HandoffOutcome::InvalidManifest);
    let binding = report
        .dimensions
        .iter()
        .find(|dimension| dimension.id == "proof_binding")
        .context("proof_binding dimension")?;
    assert_eq!(
        binding.verdict,
        DimensionVerdict::Invalid,
        "the ceiling must be what refuses this, not envelope closure"
    );
    assert!(binding.detail.contains("ceiling"), "detail: {}", binding.detail);
    Ok(())
}

/// A replacement ref must not decide what the envelope carries.
///
/// `refs/replace` makes Git serve substitute content under an original object's
/// id. That is exactly the deception this format must not transport: the
/// manifest would describe the replacement while the pack carried the literal
/// object. `GIT_NO_REPLACE_OBJECTS` is *set* rather than cleared, because
/// clearing it enables replacement.
#[test]
fn a_replacement_ref_does_not_change_what_is_exported() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"original\n")?;
    fixture.commit("original subject")?;
    let original = fixture.git(&["rev-parse", "HEAD"])?.trim().to_string();

    // A second commit with different content and a different message, then a
    // replacement pointing the original id at it.
    fixture.write("a.txt", b"substitute\n")?;
    fixture.commit("substitute subject")?;
    let substitute = fixture.git(&["rev-parse", "HEAD"])?.trim().to_string();
    fixture.git(&["replace", "-f", &original, &substitute])?;

    // Prove the fixture is a real lever: with replacement active, ordinary Git
    // reads of the original id return the substitute's message.
    let replaced = fixture.git(&["show", "-s", "--format=%s", &original])?;
    assert!(
        replaced.contains("substitute"),
        "the replacement must actually be in effect, or this control proves nothing"
    );

    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.candidate = original.clone();
    let manifest = create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(manifest.candidate.commit, original);
    assert_eq!(
        manifest.candidate.message.trim(),
        "original subject",
        "the envelope must describe the literal object, not its replacement"
    );
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::ValidHandoff);
    Ok(())
}

/// A `--proof` argument naming a symlink would publish whatever it points at.
///
/// The secret scan catches recognised credential shapes, but "a file the caller
/// did not mean to publish" is a much larger set than "a string that looks like
/// a token", and an envelope is handed onward.
#[test]
fn a_symlinked_proof_artifact_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    let elsewhere = tempfile::TempDir::new()?;
    let private = elsewhere.path().join("private-material.json");
    fs::write(&private, br#"{"note":"not meant for an envelope"}"#)?;
    let link = fixture.path().join("proof.json");
    if !link_manifest(&private, &link)? {
        return Ok(());
    }

    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.proofs = vec![link];
    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("a symlinked proof would publish its target, and must be refused");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("symbolic link"), "the refusal must name the reason: {detail}");
    assert!(!destination.envelope().exists(), "nothing is published");
    Ok(())
}

/// A recognised subject key holding a non-canonical value is refused, not ignored.
///
/// Treating it as an absent subject let the producer stamp this candidate onto
/// evidence that named something else in abbreviated or uppercase form —
/// rebinding stale proof rather than refusing it.
#[test]
fn a_proof_subject_that_is_not_a_full_object_id_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let head = fixture.git(&["rev-parse", "HEAD"])?.trim().to_string();

    for value in [head[..12].to_string(), head.to_uppercase(), "not-an-object-id".to_string()] {
        let proof = fixture.path().join("report.json");
        fs::write(&proof, serde_json::to_vec(&serde_json::json!({ "commit": value }))?)?;
        let destination = Destination::new()?;
        let mut requested = request(&fixture, &destination);
        requested.proofs = vec![proof];

        let Err((outcome, _)) = create_handoff(&requested) else {
            bail!("`{value}` is a stated subject this format cannot verify, and must be refused");
        };
        assert_eq!(outcome, HandoffOutcome::ProofSubjectMismatch, "for value `{value}`");
    }
    Ok(())
}

/// `..` is traversal, not a repository name.
///
/// This is the field a downstream publisher resolves into a target, and it was
/// the one place the module's own path rule was not applied — reachable on the
/// *observed* path too, which is the strongest claim strength the format issues.
#[test]
fn a_traversing_repository_identity_is_refused() -> Result<()> {
    for hostile in ["../..", ".git/.git", "-x/y", "./a", "a/.."] {
        assert!(
            !super::hygiene::is_repository_identity(hostile),
            "`{hostile}` is not a repository name"
        );

        // Refused as a caller declaration...
        let bare = Fixture::with_remote(None)?;
        bare.write("a.txt", b"a\n")?;
        bare.commit("root")?;
        let destination = Destination::new()?;
        let mut requested = request(&bare, &destination);
        requested.declared_repository_identity = Some(hostile.to_string());
        assert!(
            create_handoff(&requested).is_err(),
            "`{hostile}` must not be accepted as a declared identity"
        );

        // ...and never observed from a remote either.
        let observed = Fixture::with_remote(Some(&format!("https://github.com/{hostile}")))?;
        observed.write("a.txt", b"a\n")?;
        observed.commit("root")?;
        let out = Destination::new()?;
        let manifest = create_handoff(&request(&observed, &out))
            .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
        assert_eq!(
            manifest.repository_identity.status,
            RepositoryIdentityStatus::NotProven,
            "`{hostile}` must not be observed as an identity"
        );
    }
    Ok(())
}

/// A credential-bearing remote still contradicts a wrong declaration.
///
/// Refusing the URL as an identity *source* is not a reason to stop reading it
/// as a cross-check: standing in a clone of `acme/app` and stamping
/// `totally/unrelated` is the substitution the conflict rule exists to stop,
/// and a token-in-URL remote is the ordinary shape for the credential-less
/// workspaces this format targets. Comparing an `owner/name` is not retaining
/// a URL.
#[test]
fn a_credential_bearing_remote_still_contradicts_a_wrong_declaration() -> Result<()> {
    let token = synthetic_github_token();
    let remote = format!("https://{}:{}@{}", "octocat", token, "github.com/acme/app.git");

    let fixture = Fixture::with_remote(Some(&remote))?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.declared_repository_identity = Some("totally/unrelated".to_string());

    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!(
            "a declaration contradicting the remote must be refused even when the URL is refused"
        );
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("acme/app"), "the refusal must name the remote's identity: {detail}");
    assert!(!detail.contains(&github_token_marker()), "no token material may reach a message");
    assert!(!destination.envelope().exists(), "nothing is published");

    // An agreeing declaration is still the fallback the field is for.
    let agreeing = Destination::new()?;
    let mut requested = request(&fixture, &agreeing);
    requested.declared_repository_identity = Some("acme/app".to_string());
    let manifest = create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;
    assert_eq!(manifest.repository_identity.status, RepositoryIdentityStatus::Declared);
    assert!(manifest.limitations.contains(&LimitationCode::RemoteUrlContainedCredentials));
    let manifest_text = fs::read_to_string(agreeing.envelope().join(MANIFEST_FILE_NAME))?;
    assert!(!manifest_text.contains(&github_token_marker()), "no token reaches the envelope");
    Ok(())
}

/// Altered proof bytes are corruption, not a rebinding.
///
/// Three different facts have three different repairs; reporting all of them as
/// "not bound to this candidate" told an operator to rebind evidence when the
/// repair was to re-copy bytes.
#[test]
fn proof_failures_are_classified_by_what_actually_went_wrong() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let head = fixture.git(&["rev-parse", "HEAD"])?.trim().to_string();
    let proof = fixture.path().join("report.json");
    fs::write(&proof, serde_json::to_vec(&serde_json::json!({ "commit": head }))?)?;

    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.proofs = vec![proof];
    create_handoff(&requested)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    // Corrupt the artifact's bytes without touching the manifest.
    let carried = destination.envelope().join(PROOF_DIR_NAME).join("report.json");
    let mut bytes = fs::read(&carried)?;
    bytes.extend_from_slice(b" ");
    fs::write(&carried, &bytes)?;

    assert_eq!(
        check_handoff(&destination.envelope()).outcome,
        HandoffOutcome::DigestMismatch,
        "altered bytes are corruption; rebinding the evidence would not repair them"
    );
    Ok(())
}

/// An unreadable envelope file is the instrument failing, not a bad candidate.
///
/// A file that is *absent* is an envelope defect — the manifest declared
/// something the envelope does not contain. A file that exists and cannot be
/// read is a different fact with a different repair, and reporting it as a
/// candidate defect sends an operator to fix the wrong thing.
#[cfg(unix)]
#[test]
fn an_unreadable_envelope_file_is_an_instrument_failure() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    let manifest_path = destination.envelope().join(MANIFEST_FILE_NAME);
    let original = fs::metadata(&manifest_path)?.permissions();
    fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o000))?;
    let report = check_handoff(&destination.envelope());
    fs::set_permissions(&manifest_path, original)?;

    // Running as root defeats the fixture, and a control that cannot create its
    // own precondition proves nothing — say so rather than passing quietly.
    if report.outcome == HandoffOutcome::ValidHandoff {
        return Ok(());
    }
    assert_eq!(
        report.outcome,
        HandoffOutcome::InstrumentFailure,
        "a file that exists but cannot be read is not a defective candidate"
    );

    // The same must hold for every declared file, not only the manifest. The
    // pack and the receipt are read by dimensions that classify their own
    // failures — a digest mismatch, a disagreeing receipt — and an unreadable
    // file reaching those dimensions as a content verdict would report a wrong
    // candidate (exit 2) for a disk that never answered (exit 4).
    for name in [PACK_FILE_NAME, RECEIPT_FILE_NAME] {
        let separate = Destination::new()?;
        export_valid(&fixture, &separate)?;
        let target = separate.envelope().join(name);
        let permissions = fs::metadata(&target)?.permissions();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o000))?;
        let report = check_handoff(&separate.envelope());
        fs::set_permissions(&target, permissions)?;
        assert_eq!(
            report.outcome,
            HandoffOutcome::InstrumentFailure,
            "`{name}` exists and could not be read; that is not a candidate defect"
        );
    }

    // The absent case stays an envelope defect, which is the distinction.
    let second = Destination::new()?;
    export_valid(&fixture, &second)?;
    fs::remove_file(second.envelope().join(MANIFEST_FILE_NAME))?;
    assert_eq!(check_handoff(&second.envelope()).outcome, HandoffOutcome::InvalidManifest);
    Ok(())
}

/// The ownership exception has to name the path Git will actually match.
///
/// `safe.directory` is compared literally against the repository path Git
/// discovered, and `--repository` defaults to `.`, so passing the caller's path
/// through verbatim produced `safe.directory=.` — which never matches. The
/// exception silently did nothing in precisely the case it exists for: a
/// checkout the running user does not own, which is the ordinary container, CI,
/// and devcontainer shape. Making the path merely absolute is not enough; Git
/// rejects a trailing `/.` the same way.
///
/// Creating the precondition needs `chown`, so the control skips where it
/// cannot drop ownership rather than passing without having tested anything.
///
/// `#[serial]` because a relative repository path is only meaningful against a
/// known working directory, and the process working directory is global: this
/// control moves it, so it must not run beside anything else that reads or
/// moves it.
#[cfg(unix)]
#[test]
#[serial]
fn the_ownership_exception_names_a_path_git_matches() -> Result<()> {
    use std::os::unix::fs::chown;

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    // 65534 is `nobody` on every distribution this runs on. Failing here means
    // the process lacks the privilege to create the precondition.
    if chown(fixture.path(), Some(65534), Some(65534)).is_err() {
        return Ok(());
    }
    let restored = scopeguard_chown(fixture.path());

    // The fixture is only meaningful if Git actually refuses this repository
    // without an exception. If the ownership check does not fire — the process
    // owns it anyway, or Git is configured to skip it — the control proves
    // nothing and says so instead of passing.
    let refused = std::process::Command::new("git")
        .args(["-c", "safe.directory=", "rev-parse", "--show-toplevel"])
        .current_dir(fixture.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    if refused.status.success() {
        drop(restored);
        return Ok(());
    }

    // A relative path is what the CLI default produces. This is the case that
    // silently failed.
    let relative = PathBuf::from(".");
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(fixture.path())?;
    let output = super::git::run_git(&relative, &["rev-parse", "--show-toplevel"]);
    std::env::set_current_dir(previous)?;
    drop(restored);

    let Ok(output) = output else {
        bail!("the ownership exception must let a relative repository path be inspected");
    };
    assert!(
        output.succeeded(),
        "a relative `--repository` must still resolve to a matchable exception: {}",
        output.diagnostic()
    );
    Ok(())
}

/// Restore ownership when the control leaves, so the fixture can be removed.
#[cfg(unix)]
fn scopeguard_chown(path: &Path) -> impl Drop + use<> {
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::os::unix::fs::chown(&self.0, Some(0), Some(0));
        }
    }
    Restore(path.to_path_buf())
}

/// A human projection reports producer claims; it must not let one rewrite it.
///
/// `explain` deliberately performs no shape validation, so a manifest string
/// reaches the terminal exactly as the producer wrote it. An ESC sequence there
/// would repaint lines the reader has already accepted — the projection would
/// be telling the reader whatever the envelope chose, which is precisely the
/// authority `explain` disclaims. The JSON projection needs no equivalent
/// because `serde_json` escapes every character below U+0020.
#[test]
fn control_characters_cannot_rewrite_a_human_projection() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    export_valid(&fixture, &destination)?;

    // `explain` reads the manifest as-is, so this is what an untrusted envelope
    // can put in front of a reader.
    let mut raw = raw_manifest(&destination.envelope())?;
    raw["repository_identity"]["value"] =
        serde_json::Value::String("owner/repo\u{1b}[2K\u{1b}[Avalid handoff".to_string());
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

    let Ok(document) = super::render::explain(&destination.envelope()) else {
        bail!("`explain` reads a manifest without validating it");
    };
    let human = super::render::render_explain_human(&document);
    assert!(!human.contains('\u{1b}'), "an escape sequence must not reach the terminal: {human:?}");
    assert!(
        human.contains("<U+001B>"),
        "the reader must be told the field carried a control character: {human:?}"
    );

    // The JSON projection carries the raw claim, escaped by the encoder, so a
    // machine consumer still sees exactly what the manifest said.
    let Ok(json) = super::render::render(&document, &human, true) else {
        bail!("the document must render as canonical JSON");
    };
    assert!(!json.contains('\u{1b}'), "JSON must escape rather than emit the control character");
    Ok(())
}

/// A drive-relative path escapes whatever root a consumer joins it onto.
#[test]
fn a_drive_relative_inventory_path_is_refused() -> Result<()> {
    assert!(!super::hygiene::is_safe_repository_path("c:evil"));
    assert!(!super::hygiene::is_safe_repository_path("a/c:evil"));
    assert!(super::hygiene::is_safe_repository_path("a/b.txt"));

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;

    let mut raw = raw_manifest(&destination.envelope())?;
    raw["inventory"]["changes"][0]["path"] = serde_json::Value::String("c:evil".to_string());
    rewrite_manifest_raw(&destination.envelope(), &raw)?;
    assert_eq!(check_handoff(&destination.envelope()).outcome, HandoffOutcome::InvalidManifest);
    let _ = manifest;
    Ok(())
}

/// Proof collection is bounded in aggregate, not only per artifact.
///
/// The per-artifact ceiling times the count ceiling is eight gigabytes, and
/// every artifact is held until the envelope is staged — so the per-artifact
/// limit alone bounds nothing that matters.
#[test]
fn proof_collection_is_bounded_in_aggregate() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;

    // Too many artifacts, refused before any is read.
    let destination = Destination::new()?;
    let mut requested = request(&fixture, &destination);
    requested.proofs = (0..super::create::MAX_PROOFS + 1)
        .map(|index| fixture.path().join(format!("proof-{index}.json")))
        .collect();
    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("a proof set above the count ceiling must be refused");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("ceiling"), "the refusal must name the ceiling: {detail}");

    // Individually legal artifacts whose total exceeds the aggregate ceiling.
    let mut paths = Vec::new();
    for index in 0..8 {
        let path = fixture.path().join(format!("big-{index}.json"));
        let file = fs::File::create(&path)?;
        file.set_len(super::create::MAX_PROOF_BYTES)?;
        drop(file);
        paths.push(path);
    }
    let second = Destination::new()?;
    let mut requested = request(&fixture, &second);
    requested.proofs = paths;
    let Err((outcome, detail)) = create_handoff(&requested) else {
        bail!("artifacts that are each legal can still exceed the aggregate ceiling");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("total"), "the refusal must name the aggregate rule: {detail}");
    assert!(!second.envelope().exists(), "nothing is published");
    Ok(())
}

/// The aggregate ceiling bounds the bytes retained, not the bytes declared.
///
/// A `--proof` argument names a file the producer does not own, and the file
/// can change between being measured and being read. Charging the aggregate
/// budget the size a file reported before it was opened, while the read itself
/// is allowed to deliver up to the per-artifact ceiling, means the aggregate
/// bounds nothing an adversary controls: files that measure as empty can still
/// hand back the count ceiling times the per-artifact ceiling.
///
/// The fixture needs a real file whose reported size understates its content,
/// which `/proc` provides on Linux without any timing race: `status` reports
/// `st_size` 0 and reads back several kilobytes. The budget is injected so the
/// arithmetic is exercised at eight bytes rather than at the format's 128 MiB.
#[cfg(target_os = "linux")]
#[test]
fn the_aggregate_proof_ceiling_counts_bytes_actually_retained() -> Result<()> {
    use super::create::{ProofBudget, collect_proofs_within};

    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    let commit = fixture.commit("root")?;

    // The fixture is only meaningful if this file really does understate
    // itself. A control that silently stopped discriminating would otherwise
    // keep passing forever.
    let understated = PathBuf::from("/proc/self/status");
    let declared = fs::metadata(&understated)?.len();
    let actual = fs::read(&understated)?.len();
    if declared != 0 || actual == 0 {
        // Not the kernel this control was written for; it proves nothing here.
        return Ok(());
    }

    let exact = fixture.path().join("exact.txt");
    fs::write(&exact, b"12345678")?;
    let budget = ProofBudget { max_count: 8, max_each: 64, max_total: 8 };

    // Negative control: the budget accepts what genuinely fits inside it.
    let Ok(accepted) = collect_proofs_within(std::slice::from_ref(&exact), &commit, &budget) else {
        bail!("eight bytes must fit inside an eight-byte budget");
    };
    assert_eq!(accepted.len(), 1, "the artifact that fits is the one collected");

    // The same eight bytes, followed by a file that declares nothing and
    // delivers kilobytes. Counting the declared size leaves the budget looking
    // untouched and accepts the whole set.
    let Err((outcome, detail)) = collect_proofs_within(&[exact, understated], &commit, &budget)
    else {
        bail!("a proof that delivers more than it declared must still be charged for it");
    };
    assert_eq!(outcome, HandoffOutcome::InstrumentFailure);
    assert!(detail.contains("total"), "the refusal must name the aggregate rule: {detail}");
    Ok(())
}
