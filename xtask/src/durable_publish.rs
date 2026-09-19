//! Single-writer durable publication substrate (shared by the
//! `ci-route-plan` CLI (#10179) and `routed_gate_result.v1` publication
//! (#9156)).
//!
//! Treats the output artifact as a single-writer durable handoff, not a
//! shared store:
//!
//! 1. callers hand over one complete validated byte sequence
//!    (validated before any filesystem effect);
//! 2. a unique temporary file in the target directory
//!    (`.ci-route-plan.<pid>.<nanos>.<seq>.tmp`, created with
//!    create-new semantics so two writers can never share it);
//! 3. complete write + flush + `sync_all` durability step;
//! 4. read-back verification of the durable temp, so only fully verified
//!    bytes are ever promoted;
//! 5. atomic publication by rename, which replaces an existing
//!    destination on both POSIX and Windows (std uses `rename(2)` /
//!    `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` — an explicit
//!    cross-platform overwrite contract, not a Unix-only accident);
//! 6. POSIX directory sync of the rename, so the published name survives
//!    a host crash (an explicit typed no-op boundary on Windows);
//! 7. final read-back verification: success is returned only after the
//!    destination contains exactly the canonical bytes.
//!
//! Any failure (directory creation, temp creation, write, sync, rename,
//! directory sync, read-back) is a typed non-success. A failure before
//! this writer's rename promoted (temp write, sync, temp read-back,
//! rename) removes only the temporary this writer provably owns: any file
//! already at the destination was never produced by this invocation and
//! may be a concurrent publication's completed artifact, which a losing
//! writer must never delete, so the destination is left untouched. A
//! post-promotion failure (directory sync or final read-back) likewise
//! leaves the destination untouched — it may hold this writer's promoted
//! bytes or a concurrent writer's completed artifact — and the refusal
//! states that this invocation could not verify the artifact. Consumers
//! must trust the typed refusal, never the presence or absence of an
//! artifact. Multi-writer/shared-store semantics are explicitly not
//! implied; downstream consumers needing them require a separate contract.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Monotonic sequence within this process so two publications in one
/// process can never propose the same temporary name.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Typed publication failure vocabulary: one variant per contract step, in
/// pipeline order. No variant is convertible into success.
#[derive(Debug)]
pub enum PublicationError {
    CreateDirectory {
        dir: String,
        source: io::Error,
    },
    TempCreate {
        dir: String,
        source: io::Error,
    },
    Write {
        temp: PathBuf,
        source: io::Error,
    },
    Sync {
        temp: PathBuf,
        source: io::Error,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
        source: io::Error,
    },
    /// POSIX-only: the post-rename directory sync refused.
    #[cfg(unix)]
    DirSync {
        dir: String,
        source: io::Error,
    },
    ReadBack {
        path: PathBuf,
        source: io::Error,
    },
    ReadBackMismatch {
        path: PathBuf,
        expected_len: usize,
        found_len: usize,
    },
}

impl std::fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublicationError::CreateDirectory { dir, source } => {
                write!(formatter, "create directory {dir:?}: {source}")
            }
            PublicationError::TempCreate { dir, source } => {
                write!(formatter, "create unique temporary file in {dir:?}: {source}")
            }
            PublicationError::Write { temp, source } => {
                write!(formatter, "write {}: {source}", temp.display())
            }
            PublicationError::Sync { temp, source } => {
                write!(formatter, "sync {}: {source}", temp.display())
            }
            PublicationError::Rename { from, to, source } => {
                write!(formatter, "atomic rename {} -> {}: {source}", from.display(), to.display())
            }
            #[cfg(unix)]
            PublicationError::DirSync { dir, source } => {
                write!(formatter, "sync directory {dir:?}: {source}")
            }
            PublicationError::ReadBack { path, source } => {
                write!(formatter, "read back {}: {source}", path.display())
            }
            PublicationError::ReadBackMismatch { path, expected_len, found_len } => {
                write!(
                    formatter,
                    "read-back mismatch at {}: expected {expected_len} canonical bytes, found \
                     {found_len}",
                    path.display()
                )
            }
        }
    }
}

/// Publish `bytes` to `path` under the single-writer durable contract
/// documented on this module. Returns only after the destination contains
/// exactly `bytes`.
pub fn publish_atomically(path: &Path, bytes: &[u8]) -> Result<(), PublicationError> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| PublicationError::CreateDirectory {
        dir: parent.display().to_string(),
        source,
    })?;
    let (temp, file) = create_unique_temp(parent).map_err(|source| {
        PublicationError::TempCreate { dir: parent.display().to_string(), source }
    })?;
    if let Err(refusal) = write_and_verify_temp(&temp, bytes, file) {
        // Pre-promotion failure: nothing of this writer reached the
        // destination. Only the temporary is provably this writer's; any
        // file at the destination may be a concurrent publication's
        // completed artifact and must never be removed by a losing writer.
        return Err(cleanup_failed_publication(&temp, refusal));
    }
    match promote(&temp, path, bytes) {
        Ok(()) => Ok(()),
        // The rename never promoted this writer's bytes, so exactly like a
        // pre-promotion failure nothing at the destination is attributable
        // to this invocation: only the temporary is removed.
        Err(refusal @ PublicationError::Rename { .. }) => {
            Err(cleanup_failed_publication(&temp, refusal))
        }
        // Post-promotion failure (directory sync, final read-back): the
        // destination now holds either this writer's promoted bytes or a
        // concurrent writer's completed artifact. Removing another
        // publication would break the two-writer contract, so the
        // destination is left and the refusal says the artifact could not
        // be verified by this invocation.
        Err(refusal) => Err(refusal),
    }
}

/// Best-effort cleanup after a publication that never promoted this
/// writer's bytes: it removes only the temporary this writer provably
/// owns. The destination is never touched — before promotion this writer
/// cannot attribute a destination file to itself, and the file there may
/// be a concurrent publication's completed artifact, which a losing
/// writer must never delete.
pub fn cleanup_failed_publication(temp: &Path, refusal: PublicationError) -> PublicationError {
    drop(fs::remove_file(temp));
    refusal
}

/// Write, flush, sync, and verify the durable temporary: only fully
/// verified bytes are ever promoted.
fn write_and_verify_temp(
    temp: &Path,
    bytes: &[u8],
    mut file: File,
) -> Result<(), PublicationError> {
    file.write_all(bytes)
        .map_err(|source| PublicationError::Write { temp: temp.to_path_buf(), source })?;
    file.flush().map_err(|source| PublicationError::Write { temp: temp.to_path_buf(), source })?;
    file.sync_all()
        .map_err(|source| PublicationError::Sync { temp: temp.to_path_buf(), source })?;
    // Close the handle before the rename so the atomic publication cannot
    // race an open writer on any platform.
    drop(file);
    verify_published(temp, bytes)
}

/// Atomically promote the verified temporary to the final path, sync the
/// rename, and read the destination back: success is returned only after
/// the destination contains exactly the canonical bytes.
fn promote(temp: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), PublicationError> {
    fs::rename(temp, final_path).map_err(|source| PublicationError::Rename {
        from: temp.to_path_buf(),
        to: final_path.to_path_buf(),
        source,
    })?;
    // POSIX durability: the rename's directory entry must itself be synced
    // before publication is reported, or a host crash can revert the name.
    // Windows is an explicit typed boundary: std cannot open directory
    // handles there and NTFS metadata journaling applies.
    sync_parent_directory(final_path)?;
    verify_published(final_path, bytes)
}

/// Sync the containing directory so the published rename survives a host
/// crash. Explicit platform boundary: a real (not accidental) no-op on
/// Windows.
#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), PublicationError> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let dir = File::open(parent).map_err(|source| PublicationError::DirSync {
        dir: parent.display().to_string(),
        source,
    })?;
    dir.sync_all()
        .map_err(|source| PublicationError::DirSync { dir: parent.display().to_string(), source })
}

#[cfg(windows)]
fn sync_parent_directory(_path: &Path) -> Result<(), PublicationError> {
    // Documented boundary: directory-handle fsync is unavailable through
    // std on Windows; the file-level sync plus atomic rename remain.
    Ok(())
}

/// Unique temporary file in `dir`, created with create-new semantics:
/// two concurrent writers can never hold the same temporary path. The
/// name embeds pid, nanosecond timestamp, and an in-process sequence.
pub fn create_unique_temp(dir: &Path) -> io::Result<(PathBuf, File)> {
    let pid = std::process::id();
    let mut sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let candidate = dir.join(format!(".ci-route-plan.{pid}.{nanos}.{sequence}.tmp"));
        match File::create_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Read-back verification: the publication is successful only when the
/// final path contains exactly the canonical bytes.
pub fn verify_published(path: &Path, expected: &[u8]) -> Result<(), PublicationError> {
    let found = fs::read(path)
        .map_err(|source| PublicationError::ReadBack { path: path.to_path_buf(), source })?;
    if found != expected {
        return Err(PublicationError::ReadBackMismatch {
            path: path.to_path_buf(),
            expected_len: expected.len(),
            found_len: found.len(),
        });
    }
    Ok(())
}
