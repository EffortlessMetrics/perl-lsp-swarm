//! Executable proof for `agent_candidate_handoff.v1`.
//!
//! The controls here are written against the class of implementation that
//! would look correct in a demo and lose work in practice: one that transports
//! a textual patch, trusts the producer's own manifest, or quietly depends on
//! the source workspace still existing.

use super::create::compute_identity_digest;
use super::model::{
    ChangeStatus, EntryClass, GitlinkDisposition, HANDOFF_RECEIPT_SCHEMA_V1, LimitationCode,
    MANIFEST_FILE_NAME, Manifest, PACK_FILE_NAME, PROOF_DIR_NAME, ProducerReceipt,
    RECEIPT_FILE_NAME, RepositoryIdentityStatus,
};
use super::{
    CreateRequest, DimensionVerdict, HandoffOutcome, canonical_json, check_handoff, create_handoff,
};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

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
        let output = Command::new("git")
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
        let output = Command::new("git")
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
        producer_self_check: "validated_after_write".to_string(),
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
    let status = Command::new("git")
        .args(["clone", "--quiet"])
        .arg(fixture.path())
        .arg(&clone_path)
        .status()?;
    assert!(status.success(), "cloning the fixture");

    let first_destination = Destination::new()?;
    let first = export_valid(&fixture, &first_destination)?;

    let second_destination = Destination::new()?;
    let inputs = CreateRequest {
        repository: clone_path,
        candidate: "HEAD".to_string(),
        out: second_destination.envelope(),
        declared_repository_identity: None,
        proofs: Vec::new(),
    };
    // The clone's `origin` points at a local path, so no identity is declared
    // here. This control compares objects and inventory across storage
    // layouts; repository identity is a separate semantic input.
    let second = create_handoff(&inputs)
        .map_err(|(outcome, detail)| anyhow::anyhow!("create failed {outcome:?}: {detail}"))?;

    assert_eq!(first.candidate.commit, second.candidate.commit);
    assert_eq!(
        first.transport.object_ids, second.transport.object_ids,
        "the same candidate must enumerate the same objects from either storage layout"
    );
    assert_eq!(first.inventory, second.inventory);
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
        stdin.as_bytes(),
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

/// The identity digest is what protects the claims Git objects cannot check.
///
/// Most tampering is caught earlier, by recomputation against the imported
/// objects. Limitation codes are different: no object can confirm that an
/// envelope's proof is local-only, so dropping that admission is exactly the
/// claim-weakening edit the digest has to catch on its own.
#[test]
fn dropping_a_limitation_without_resealing_breaks_the_identity_digest() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("a.txt", b"a\n")?;
    fixture.commit("root")?;
    let destination = Destination::new()?;
    let manifest = export_valid(&fixture, &destination)?;
    assert!(
        manifest.limitations.contains(&LimitationCode::LocalProofOnly),
        "the fixture must carry the admission the control removes"
    );

    let mut raw = raw_manifest(&destination.envelope())?;
    let kept: Vec<serde_json::Value> = raw["limitations"]
        .as_array()
        .context("limitations array")?
        .iter()
        .filter(|value| value.as_str() != Some("local_proof_only"))
        .cloned()
        .collect();
    raw["limitations"] = serde_json::Value::Array(kept);
    rewrite_manifest_raw(&destination.envelope(), &raw)?;

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

    let outcome = super::create::create_handoff_with_validator(
        &request(&fixture, &destination),
        always_invalid,
    );
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
#[cfg(unix)]
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
    std::os::unix::fs::symlink(&target, destination.envelope().join(PACK_FILE_NAME))?;

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
        stdin.as_bytes(),
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

        let raw = Command::new("git")
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
