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
use anyhow::{Context, Result, bail};
use serial_test::serial;
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
    // Point the clone at the same remote the original observed. Repository
    // identity is a semantic input — and its *status* is a claim-strength
    // input, so an observed identity and a declared one are legitimately
    // different candidates to this format. Holding both equal is what lets
    // this control compare the whole semantic identity rather than a subset.
    let status = Command::new("git")
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
    // The declared identity differs only in `source` (observed versus declared),
    // which is excluded from the transport, so the packs must agree byte for
    // byte. This is the stronger property the contract asks for, demonstrated
    // across the ordinary cross-host difference — loose objects versus a pack —
    // at one Git version. Only the cross-Git-version case remains a declared
    // limitation, because guaranteeing it would mean writing our own packer.
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

    // Drop the blob from both the pack and the declared set, then reseal.
    let mut reduced = manifest;
    reduced.transport.object_ids.retain(|id| *id != binary_object);
    let stdin = reduced.transport.object_ids.join("\n");
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
    // `run_git` invocation now clears or overrides these variables on the
    // child explicitly, so a concurrent test cannot observe this one's value.
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
    std::os::unix::fs::symlink(&outside, destination.envelope().join(MANIFEST_FILE_NAME))?;

    let Err((outcome, detail)) = explain(&destination.envelope()) else {
        bail!("explain must not read a manifest through a symbolic link");
    };
    assert_eq!(outcome, HandoffOutcome::InvalidManifest);
    assert!(detail.contains("symbolic link"), "the refusal must name the reason: {detail}");
    Ok(())
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
    // unaffected because production sets this variable on every child
    // explicitly rather than inheriting it.
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
    Ok(())
}
