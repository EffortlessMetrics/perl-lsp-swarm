use anyhow::{Result, bail, ensure};
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use serial_test::serial;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::{TempDir, tempdir};
use xtask::worktree_forensic_backup::{
    BACKUP_SCHEMA_VERSION, BackupReceipt, BackupRole, create, create_with_limits, verify,
};
use xtask::worktree_forensic_recovery::TraversalLimits;

struct LinkedFixture {
    _temporary: TempDir,
    repository: PathBuf,
    candidate: PathBuf,
    administrative: PathBuf,
    backup_parent: PathBuf,
    sentinel: PathBuf,
}

impl LinkedFixture {
    fn create() -> Result<Self> {
        let temporary = tempdir()?;
        let repository = temporary.path().join("repository");
        run_git(temporary.path(), &["init", "-q", "-b", "main", path_text(&repository)?])?;
        run_git(&repository, &["config", "user.name", "Forensic Fixture"])?;
        run_git(&repository, &["config", "user.email", "forensic@example.invalid"])?;
        fs::write(repository.join("seed.pl"), "use strict;\n")?;
        run_git(&repository, &["add", "seed.pl"])?;
        run_git(&repository, &["commit", "-q", "-m", "seed"])?;

        let candidate = repository.join(".claude/worktrees/linked");
        fs::create_dir_all(candidate.parent().ok_or_else(|| {
            anyhow::anyhow!("candidate path has no parent: {}", candidate.display())
        })?)?;
        run_git(
            &repository,
            &["worktree", "add", "-q", "-b", "forensic-backup", path_text(&candidate)?],
        )?;
        fs::write(candidate.join("unique.pl"), "our $unique = 1;\n")?;
        fs::write(candidate.join("notes.txt"), "not source-like\n")?;
        let pointer = fs::read_to_string(candidate.join(".git"))?;
        let administrative_text = pointer
            .strip_prefix("gitdir:")
            .ok_or_else(|| anyhow::anyhow!("fixture pointer lacks gitdir record"))?
            .trim();
        let administrative = PathBuf::from(administrative_text);
        let unselected = repository.join(".claude/worktrees/unselected");
        fs::create_dir_all(&unselected)?;
        fs::write(unselected.join("must-not-be-captured.pl"), "secret\n")?;
        let backup_parent = temporary.path().join("backup-parent");
        fs::create_dir_all(&backup_parent)?;
        let sentinel = backup_parent.join("must-not-change.txt");
        fs::write(&sentinel, "sentinel\n")?;
        Ok(Self {
            _temporary: temporary,
            repository,
            candidate,
            administrative,
            backup_parent,
            sentinel,
        })
    }

    fn backup_dir(&self, name: &str) -> PathBuf {
        self.backup_parent.join(name)
    }

    fn snapshot(&self) -> Result<SubjectSnapshot> {
        Ok(SubjectSnapshot {
            worktrees: git_output(&self.repository, &["worktree", "list", "--porcelain", "-z"])?
                .stdout,
            refs: git_output(&self.repository, &["show-ref"])?.stdout,
            pointer: fs::read(self.candidate.join(".git"))?,
            administrative_files: file_snapshot(&self.administrative)?,
            candidate_files: file_snapshot(&self.candidate)?,
            common_config: fs::read(self.repository.join(".git/config"))?,
            packed_refs: read_optional(&self.repository.join(".git/packed-refs"))?,
            reflogs: file_snapshot(&self.repository.join(".git/logs"))?,
            unselected: file_snapshot(&self.repository.join(".claude/worktrees/unselected"))?,
            sentinel: fs::read(&self.sentinel)?,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SubjectSnapshot {
    worktrees: Vec<u8>,
    refs: Vec<u8>,
    pointer: Vec<u8>,
    administrative_files: BTreeMap<String, String>,
    candidate_files: BTreeMap<String, String>,
    common_config: Vec<u8>,
    packed_refs: Option<Vec<u8>>,
    reflogs: BTreeMap<String, String>,
    unselected: BTreeMap<String, String>,
    sentinel: Vec<u8>,
}

#[test]
fn linked_worktree_backup_is_content_addressed_and_read_only_on_the_subject() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("verified");
    let receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    ensure!(receipt.schema_version == BACKUP_SCHEMA_VERSION, "unexpected schema");
    ensure!(receipt.complete, "verified backup was incomplete");
    ensure!(!receipt.retention.auto_delete, "backup retention auto-deletes");
    ensure!(
        receipt.restore_instructions.iter().any(|line| line.contains("does not restore")),
        "restore instructions missing refusal: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| entry.role == BackupRole::Pointer),
        "pointer was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| {
            entry.role == BackupRole::CandidateSource && entry.logical_path.ends_with("unique.pl")
        }),
        "source-like candidate file was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().all(|entry| entry.role != BackupRole::CandidateSource
            || !entry.logical_path.ends_with("notes.txt")),
        "non-source candidate file was captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| entry.role == BackupRole::Admin),
        "surviving admin files were not captured: {receipt:?}"
    );
    ensure!(
        receipt
            .entries
            .iter()
            .any(|entry| entry.role == BackupRole::Refs && entry.logical_path == "HEAD"),
        "HEAD ref was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| {
            entry.role == BackupRole::Refs && entry.logical_path.contains("refs/heads/")
        }),
        "branch ref was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| entry.role == BackupRole::Reflog),
        "reflog was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| entry.role == BackupRole::Config),
        "common config was not captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().all(|entry| !entry.logical_path.contains("must-not-be-captured.pl")),
        "unselected sibling worktree was captured: {receipt:?}"
    );
    let objects = destination.join("objects");
    for entry in &receipt.entries {
        let bytes = fs::read(objects.join(&entry.sha256))?;
        ensure!(digest(&bytes) == entry.sha256, "object was not content-addressed");
        ensure!(bytes.len() as u64 == entry.bytes, "object size drifted");
    }
    let verified = backup_result(verify(&destination))?;
    ensure!(verified.verification_digest == receipt.verification_digest, "verify changed identity");
    ensure!(
        fixture.snapshot()? == before,
        "backup mutated repository, candidate, refs, config, or admin bytes"
    );
    Ok(())
}

#[test]
fn cli_backup_writes_only_the_explicit_destination() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("cli");
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("backup")
        .arg("--repository")
        .arg(&fixture.repository)
        .arg("--candidate")
        .arg(&fixture.candidate)
        .arg("--backup-dir")
        .arg(&destination)
        .arg("--json")
        .output()?;
    ensure!(
        output.status.success(),
        "cli backup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(receipt["schema_version"] == BACKUP_SCHEMA_VERSION, "cli schema drifted: {receipt}");
    ensure!(receipt["complete"] == true, "cli backup was incomplete: {receipt}");
    ensure!(destination.join("receipt.json").is_file(), "cli did not write receipt");
    ensure!(fixture.snapshot()? == before, "cli backup mutated subject bytes");
    Ok(())
}

#[test]
fn destination_inside_the_repository_is_refused() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let destination = fixture.repository.join("nested-backup");
    let error = create(&fixture.repository, &fixture.candidate, &destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("nested destination was accepted"))?;
    ensure!(
        error.to_string().contains("overlaps"),
        "nested destination error lacked overlap context: {error}"
    );
    ensure!(!destination.exists(), "refused destination was created inside the repository");
    ensure!(fixture.snapshot()? == before, "refused nested destination mutated the subject");
    Ok(())
}

#[test]
fn missing_destination_parent_is_not_created_outside_the_destination() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let destination = fixture._temporary.path().join("missing-parent").join("backup");
    let error = create(&fixture.repository, &fixture.candidate, &destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("missing parent was accepted"))?;
    ensure!(
        error.to_string().contains("parent does not exist"),
        "missing parent error lacked refusal context: {error}"
    );
    ensure!(
        !fixture._temporary.path().join("missing-parent").exists(),
        "backup created an intermediate path outside the destination"
    );
    ensure!(fixture.snapshot()? == before, "missing-parent refusal mutated the subject");
    Ok(())
}

#[test]
fn non_empty_destination_is_not_overwritten() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let destination = fixture.backup_dir("occupied");
    fs::create_dir(&destination)?;
    let preexisting = destination.join("keep.txt");
    fs::write(&preexisting, "keep\n")?;
    let error = create(&fixture.repository, &fixture.candidate, &destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("occupied destination was accepted"))?;
    ensure!(
        error.to_string().contains("not empty"),
        "occupied destination error lacked emptiness context: {error}"
    );
    ensure!(fs::read_to_string(&preexisting)? == "keep\n", "occupied destination was overwritten");
    Ok(())
}

#[test]
fn broken_pointer_bytes_are_still_captured() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    fs::write(fixture.candidate.join(".git"), "gitdir: /definitely/missing/admin\n")?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("broken-pointer");
    let receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    let pointer = receipt
        .entries
        .iter()
        .find(|entry| entry.role == BackupRole::Pointer)
        .ok_or_else(|| anyhow::anyhow!("broken pointer was dropped: {receipt:?}"))?;
    let bytes = fs::read(destination.join("objects").join(&pointer.sha256))?;
    ensure!(
        bytes == b"gitdir: /definitely/missing/admin\n",
        "broken pointer bytes were not preserved"
    );
    ensure!(
        receipt.missing.iter().any(|missing| missing.role == BackupRole::Admin),
        "missing admin was not recorded: {receipt:?}"
    );
    ensure!(fixture.snapshot()? == before, "broken-pointer backup mutated the subject");
    Ok(())
}

#[test]
fn ignored_source_like_file_is_still_captured() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    fs::write(fixture.candidate.join(".gitignore"), "vendor/\n")?;
    let vendor = fixture.candidate.join("vendor");
    fs::create_dir(&vendor)?;
    fs::write(vendor.join("ignored-source.pl"), "our $ignored = 1;\n")?;
    let destination = fixture.backup_dir("ignored-source");
    let receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    ensure!(
        receipt.entries.iter().any(|entry| {
            entry.role == BackupRole::CandidateSource
                && entry.logical_path.contains("ignored-source.pl")
        }),
        "ignored source-like file was omitted: {receipt:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_candidate_is_not_followed() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let link = fixture.backup_parent.join("link-candidate");
    std::os::unix::fs::symlink(&fixture.candidate, &link)?;
    let destination = fixture.backup_dir("symlink-candidate");
    let error = create(&fixture.repository, &link, &destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("symlink candidate was accepted"))?;
    ensure!(
        error.to_string().contains("symlink") || error.to_string().contains("reparse"),
        "symlink candidate error lacked refusal context: {error}"
    );
    ensure!(!destination.exists(), "symlink candidate still created a destination");
    ensure!(fixture.snapshot()? == before, "symlink candidate refusal mutated the subject");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_inside_candidate_is_not_followed_as_an_escape() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let escape = fixture.backup_parent.join("escape.pl");
    fs::write(&escape, "outside\n")?;
    std::os::unix::fs::symlink(&escape, fixture.candidate.join("escape.pl"))?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("symlink-escape");
    let error = create(&fixture.repository, &fixture.candidate, &destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("candidate symlink escape was accepted"))?;
    ensure!(
        error.to_string().contains("symlink") || error.to_string().contains("reparse"),
        "escape error lacked refusal context: {error}"
    );
    ensure!(
        !destination.join("receipt.json").exists(),
        "symlink escape emitted a verified receipt"
    );
    ensure!(fs::read_to_string(&escape)? == "outside\n", "escape target was written");
    ensure!(fixture.snapshot()? == before, "symlink escape mutated the subject");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_destination_is_not_followed() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let real = fixture.backup_parent.join("real-dest");
    fs::create_dir(&real)?;
    let link = fixture.backup_parent.join("link-dest");
    std::os::unix::fs::symlink(&real, &link)?;
    let error = create(&fixture.repository, &fixture.candidate, &link)
        .err()
        .ok_or_else(|| anyhow::anyhow!("symlink destination was accepted"))?;
    ensure!(
        error.to_string().contains("symlink") || error.to_string().contains("reparse"),
        "symlink destination error lacked refusal context: {error}"
    );
    ensure!(read_dir_names(&real)?.is_empty(), "symlink destination was followed and written");
    ensure!(fixture.snapshot()? == before, "symlink destination refusal mutated the subject");
    Ok(())
}

#[test]
fn tampered_object_fails_verification() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let destination = fixture.backup_dir("tamper");
    let receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    let object = receipt
        .entries
        .iter()
        .find(|entry| entry.role == BackupRole::Pointer)
        .ok_or_else(|| anyhow::anyhow!("pointer missing from tamper fixture: {receipt:?}"))?;
    fs::write(destination.join("objects").join(&object.sha256), b"tampered")?;
    let error =
        verify(&destination).err().ok_or_else(|| anyhow::anyhow!("tampered backup verified"))?;
    ensure!(
        error.to_string().contains("content-address") || error.to_string().contains("verification"),
        "tamper error lacked digest context: {error}"
    );
    Ok(())
}

#[test]
fn forged_pointer_outside_admin_namespace_does_not_copy_host_files() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let host_dir = fixture.backup_parent.join("outside-admin");
    fs::create_dir(&host_dir)?;
    let secret = host_dir.join("secret.pl");
    fs::write(&secret, "SECRET_MUST_NOT_BE_COPIED\n")?;
    fs::write(host_dir.join("HEAD"), "ref: refs/heads/stolen\n")?;
    fs::write(fixture.candidate.join(".git"), format!("gitdir: {}\n", path_text(&host_dir)?))?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("forged-pointer");
    let receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    ensure!(
        receipt.missing.iter().any(|missing| missing.role == BackupRole::Admin
            && missing.detail.contains("worktrees namespace")),
        "outside-namespace admin was not recorded: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().all(|entry| entry.role != BackupRole::Admin),
        "outside-namespace admin files were captured: {receipt:?}"
    );
    let objects = destination.join("objects");
    for entry in &receipt.entries {
        let bytes = fs::read(objects.join(&entry.sha256))?;
        ensure!(
            !bytes
                .windows(b"SECRET_MUST_NOT_BE_COPIED".len())
                .any(|window| window == b"SECRET_MUST_NOT_BE_COPIED"),
            "host secret was copied into backup object {}",
            entry.logical_path
        );
    }
    ensure!(fs::read_to_string(&secret)? == "SECRET_MUST_NOT_BE_COPIED\n", "host secret mutated");
    ensure!(fixture.snapshot()? == before, "forged-pointer backup mutated the subject");
    Ok(())
}

#[test]
fn pointer_naming_the_worktrees_namespace_root_captures_no_sibling_administration() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let namespace_root = fixture.repository.join(".git").join("worktrees");
    let candidate = fixture.repository.join("root-pointer-candidate");
    fs::create_dir(&candidate)?;
    fs::write(candidate.join(".git"), format!("gitdir: {}\n", path_text(&namespace_root)?))?;
    let before = fixture.snapshot()?;
    let destination = fixture.backup_dir("namespace-root-pointer");
    let receipt = backup_result(create(&fixture.repository, &candidate, &destination))?;
    ensure!(
        receipt.missing.iter().any(|missing| missing.role == BackupRole::Admin
            && missing.detail.contains("worktrees namespace")),
        "namespace-root pointer was not recorded as missing admin administration: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().all(|entry| entry.role != BackupRole::Admin),
        "namespace-root sibling administration was captured: {receipt:?}"
    );
    ensure!(
        receipt.entries.iter().any(|entry| entry.role == BackupRole::Pointer),
        "the pointer bytes themselves were not captured: {receipt:?}"
    );
    ensure!(fixture.snapshot()? == before, "namespace-root backup mutated the subject");
    Ok(())
}

// Process cwd is global; #1269 requires #[serial] rather than a registry row.
#[test]
#[serial]
fn relative_backup_dir_without_slash_uses_current_directory_parent() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let before = fixture.snapshot()?;
    let original = std::env::current_dir()?;
    let restore = RestoreDir(original);
    std::env::set_current_dir(&fixture.backup_parent)?;
    let receipt = backup_result(create(
        &fixture.repository,
        &fixture.candidate,
        Path::new("relative-backup"),
    ))?;
    drop(restore);
    ensure!(receipt.complete, "relative destination backup was incomplete");
    ensure!(
        fixture.backup_dir("relative-backup").join("receipt.json").is_file(),
        "relative destination was not created under the current directory"
    );
    ensure!(fixture.snapshot()? == before, "relative destination backup mutated the subject");
    Ok(())
}

#[test]
fn directory_bound_refuses_a_verified_receipt() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let destination = fixture.backup_dir("dir-bound");
    let error = create_with_limits(
        &fixture.repository,
        &fixture.candidate,
        &destination,
        TraversalLimits { max_directories: 1, ..TraversalLimits::default() },
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("directory bound emitted a verified receipt"))?;
    ensure!(
        error.to_string().contains("maximum directories"),
        "directory bound error lacked bound context: {error}"
    );
    ensure!(
        !destination.join("receipt.json").exists(),
        "directory bound still wrote a verified receipt"
    );
    Ok(())
}

#[test]
fn exact_byte_limit_admits_capture_and_does_not_refuse_later_skipped_files() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    fs::write(fixture.candidate.join("zzz-not-source.txt"), "later skipped\n")?;
    let measured = fixture.backup_dir("byte-measure");
    let baseline = backup_result(create(&fixture.repository, &fixture.candidate, &measured))?;
    let captured_bytes: u64 = baseline.entries.iter().map(|entry| entry.bytes).sum();
    ensure!(captured_bytes > 0, "measured backup captured no bytes");

    let exact = fixture.backup_dir("byte-exact");
    let receipt = backup_result(create_with_limits(
        &fixture.repository,
        &fixture.candidate,
        &exact,
        TraversalLimits { max_bytes: captured_bytes, ..TraversalLimits::default() },
    ))?;
    ensure!(receipt.complete, "exact byte limit refused a capture that lands on the bound");
    ensure!(
        receipt.entries.iter().all(|entry| !entry.logical_path.ends_with("zzz-not-source.txt")),
        "non-source file was captured under the exact byte limit"
    );

    let over = fixture.backup_dir("byte-over");
    let error = create_with_limits(
        &fixture.repository,
        &fixture.candidate,
        &over,
        TraversalLimits {
            max_bytes: captured_bytes.saturating_sub(1),
            ..TraversalLimits::default()
        },
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("over-budget capture emitted a verified receipt"))?;
    ensure!(
        error.to_string().contains("maximum bytes"),
        "over-budget error lacked byte-limit context: {error}"
    );
    Ok(())
}

#[test]
fn verify_refuses_object_names_that_escape_the_objects_directory() -> Result<()> {
    let fixture = LinkedFixture::create()?;
    let destination = fixture.backup_dir("object-escape");
    let mut receipt = backup_result(create(&fixture.repository, &fixture.candidate, &destination))?;
    let escape = destination.join("escape");
    fs::write(&escape, b"should-not-be-read")?;
    let Some(entry) = receipt.entries.first_mut() else {
        bail!("verified backup had no entries to retarget");
    };
    entry.sha256 = String::from("../escape");
    let mut copy = receipt.clone();
    copy.created_at.clear();
    copy.verification_digest.clear();
    receipt.verification_digest = digest(&serde_json::to_vec(&copy)?);
    fs::write(destination.join("receipt.json"), serde_json::to_vec_pretty(&receipt)?)?;
    let error = verify(&destination)
        .err()
        .ok_or_else(|| anyhow::anyhow!("path-escaping object name verified"))?;
    ensure!(
        error.to_string().contains("hex digest") || error.to_string().contains("object name"),
        "escape error lacked object-name context: {error}"
    );
    ensure!(fs::read(&escape)? == b"should-not-be-read", "escape target was consumed as an object");
    Ok(())
}

#[test]
fn missing_backup_dir_flag_is_a_usage_error() -> Result<()> {
    let output = cargo_bin_cmd!("xtask")
        .arg("worktree-recovery")
        .arg("backup")
        .arg("--repository")
        .arg(".")
        .arg("--candidate")
        .arg(".")
        .output()?;
    ensure!(output.status.code() == Some(2), "missing backup-dir was not a usage error");
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("--backup-dir"),
        "usage did not identify backup-dir"
    );
    Ok(())
}

fn backup_result(result: color_eyre::eyre::Result<BackupReceipt>) -> Result<BackupReceipt> {
    result.map_err(|error| anyhow::anyhow!("{error:#}"))
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let output = git_output(root, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(Into::into)
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| anyhow::anyhow!("non-UTF-8 fixture path: {}", path.display()))
}

fn file_snapshot(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut snapshot = BTreeMap::new();
    if !root.exists() {
        return Ok(snapshot);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            snapshot.insert(relative_key(root, &path)?, String::from("symlink"));
            continue;
        }
        if metadata.is_dir() {
            let mut children = fs::read_dir(&path)?.collect::<std::io::Result<Vec<_>>>()?;
            children.sort_by_key(|entry| entry.file_name());
            for child in children.into_iter().rev() {
                stack.push(child.path());
            }
            continue;
        }
        if metadata.is_file() {
            snapshot.insert(relative_key(root, &path)?, digest(&fs::read(&path)?));
        }
    }
    Ok(snapshot)
}

fn relative_key(root: &Path, path: &Path) -> Result<String> {
    Ok(path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_dir_names(path: &Path) -> Result<Vec<String>> {
    let mut names = fs::read_dir(path)?
        .map(|entry| entry.map(|value| value.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

struct RestoreDir(PathBuf);

impl Drop for RestoreDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}
