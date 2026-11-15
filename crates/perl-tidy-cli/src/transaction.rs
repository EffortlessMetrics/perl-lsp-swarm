//! Guarded local-file replacement, separate from native formatting semantics.
//!
//! Each rename replaces one file atomically. The batch is plan-first with
//! guarded compensating rollback; it is not a crash-atomic filesystem
//! transaction. Advisory locks cannot constrain non-cooperating writers.

use crate::app::{ExitClass, Failure, read_bounded};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File, Metadata, Permissions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    permissions: Permissions,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    owner: u32,
    #[cfg(unix)]
    group: u32,
    #[cfg(unix)]
    links: u64,
}

impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            owner: metadata.uid(),
            #[cfg(unix)]
            group: metadata.gid(),
            #[cfg(unix)]
            links: metadata.nlink(),
        }
    }
}

pub(crate) struct Snapshot {
    path: PathBuf,
    // Keep the original handle and its advisory lock alive through the batch.
    _file: File,
    stamp: Stamp,
}

pub(crate) struct WriteInput<'a> {
    pub(crate) snapshot: &'a Snapshot,
    pub(crate) original: &'a str,
    pub(crate) rendered: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct WriteReport {
    pub(crate) exit_class: ExitClass,
    pub(crate) error: Option<String>,
    pub(crate) effects: BTreeMap<String, &'static str>,
    pub(crate) recovery_files: Vec<String>,
    pub(crate) unverified_restore_paths: Vec<String>,
    pub(crate) cleanup_failures: Vec<String>,
    pub(crate) guarantee: &'static str,
}

impl WriteReport {
    pub(crate) fn effect_for(&self, path: &str) -> &'static str {
        self.effects.get(path).copied().unwrap_or("not_attempted")
    }
}

fn filesystem_error(path: &Path, action: &str, error: impl std::fmt::Display) -> Failure {
    Failure::new(ExitClass::FilesystemError, format!("{action} {}: {error}", path.display()))
}

fn stale(path: &Path) -> Failure {
    Failure::new(
        ExitClass::StaleSource,
        format!("source or metadata changed after planning: {}", path.display()),
    )
}

/// Inspect before canonicalization: canonicalization must not hide a selected
/// symlink or a symlink in its parent chain.
pub(crate) fn admit_write_path(path: &Path) -> Result<(), Failure> {
    if !cfg!(target_os = "linux") {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            "--write currently requires the Linux metadata-preservation backend; use stdout on this platform",
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| filesystem_error(path, "cannot resolve", error))?
            .join(path)
    };
    let mut prefix = PathBuf::new();
    for component in absolute.components() {
        prefix.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&prefix)
            .map_err(|error| filesystem_error(&prefix, "cannot inspect", error))?;
        if metadata.file_type().is_symlink() {
            return Err(Failure::new(
                ExitClass::FilesystemError,
                format!("--write refuses symlink components: {}", prefix.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_source(path: &Path) -> std::io::Result<File> {
    // O_NONBLOCK also prevents a raced FIFO replacement from hanging the
    // process before the handle's regular-file metadata can be checked.
    rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(std::io::Error::from)
}

#[cfg(not(unix))]
fn open_source(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

pub(crate) fn read_snapshot(
    path: &Path,
    limit: u64,
    for_write: bool,
) -> Result<(Snapshot, String), Failure> {
    let before = fs::symlink_metadata(path)
        .map_err(|error| filesystem_error(path, "cannot inspect", error))?;
    if !before.file_type().is_file() {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!("only regular files are accepted: {}", path.display()),
        ));
    }
    let mut file =
        open_source(path).map_err(|error| filesystem_error(path, "cannot open", error))?;
    let opened = file
        .metadata()
        .map_err(|error| filesystem_error(path, "cannot inspect opened file", error))?;
    let stamp = Stamp::of(&opened);
    if !opened.is_file() || stamp != Stamp::of(&before) {
        return Err(stale(path));
    }
    if for_write {
        admit_write_path(path)?;
        check_write_metadata(path, &file, &opened)?;
        file.try_lock()
            .map_err(|error| filesystem_error(path, "cannot exclusively lock", error))?;
    }
    let source = read_bounded(&mut file, limit)?;
    let after = file
        .metadata()
        .map_err(|error| filesystem_error(path, "cannot recheck opened file", error))?;
    if stamp != Stamp::of(&after) || source.len() as u64 != stamp.length {
        return Err(stale(path));
    }
    Ok((Snapshot { path: path.to_path_buf(), _file: file, stamp }, source))
}

#[cfg(target_os = "linux")]
fn check_write_metadata(path: &Path, file: &File, metadata: &Metadata) -> Result<(), Failure> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use xattr::FileExt;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!(
                "--write requires a regular file with exactly one hard link: {}",
                path.display()
            ),
        ));
    }
    if metadata.permissions().readonly() {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!("--write refuses a readonly file: {}", path.display()),
        ));
    }
    if metadata.permissions().mode() & 0o7000 != 0 {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!("--write refuses special permission bits: {}", path.display()),
        ));
    }
    if metadata.modified().is_err() {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!("--write cannot establish modification metadata: {}", path.display()),
        ));
    }
    if file
        .list_xattr()
        .map_err(|error| filesystem_error(path, "cannot inspect extended attributes", error))?
        .next()
        .is_some()
    {
        return Err(Failure::new(
            ExitClass::FilesystemError,
            format!(
                "--write refuses extended attributes or ACL metadata it cannot preserve: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn check_write_metadata(path: &Path, _file: &File, _metadata: &Metadata) -> Result<(), Failure> {
    Err(Failure::new(
        ExitClass::FilesystemError,
        format!(
            "--write metadata preservation is unavailable on this platform: {}",
            path.display()
        ),
    ))
}

fn verify_current(path: &Path, stamp: &Stamp, expected: &str) -> Result<(), Failure> {
    admit_write_path(path)?;
    verify_snapshot(path, stamp, expected, true)
}

pub(crate) fn verify_read_snapshot(snapshot: &Snapshot, expected: &str) -> Result<(), Failure> {
    verify_snapshot(&snapshot.path, &snapshot.stamp, expected, false)
}

fn verify_snapshot(
    path: &Path,
    stamp: &Stamp,
    expected: &str,
    for_write: bool,
) -> Result<(), Failure> {
    let before = fs::symlink_metadata(path).map_err(|_| stale(path))?;
    if !before.is_file() || Stamp::of(&before) != *stamp {
        return Err(stale(path));
    }
    let mut file = open_source(path).map_err(|_| stale(path))?;
    let opened = file.metadata().map_err(|_| stale(path))?;
    if Stamp::of(&opened) != *stamp {
        return Err(stale(path));
    }
    if for_write {
        check_write_metadata(path, &file, &opened)?;
    }
    let actual = read_bounded(&mut file, expected.len() as u64).map_err(|_| stale(path))?;
    let after = file.metadata().map_err(|_| stale(path))?;
    if actual != expected || Stamp::of(&after) != *stamp {
        return Err(stale(path));
    }
    Ok(())
}

struct PreparedFile {
    file: NamedTempFile,
    stamp: Stamp,
}

#[cfg(test)]
thread_local! {
    static DENY_STAGING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn stage_text(snapshot: &Snapshot, text: &str, prefix: &str) -> Result<PreparedFile, Failure> {
    #[cfg(test)]
    if DENY_STAGING.get() {
        return Err(Failure::new(ExitClass::FilesystemError, "injected unavailable staging"));
    }
    let parent = snapshot
        .path
        .parent()
        .ok_or_else(|| Failure::new(ExitClass::FilesystemError, "selected file has no parent"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(prefix)
        .tempfile_in(parent)
        .map_err(|error| filesystem_error(parent, "cannot stage replacement in", error))?;
    let prepared = (|| -> Result<Stamp, Failure> {
        temporary
            .as_file()
            .try_lock()
            .map_err(|error| filesystem_error(temporary.path(), "cannot lock stage", error))?;
        temporary
            .write_all(text.as_bytes())
            .map_err(|error| filesystem_error(temporary.path(), "cannot write stage", error))?;
        temporary.as_file().set_permissions(snapshot.stamp.permissions.clone()).map_err(
            |error| filesystem_error(temporary.path(), "cannot preserve permissions for", error),
        )?;
        let metadata = temporary
            .as_file()
            .metadata()
            .map_err(|error| filesystem_error(temporary.path(), "cannot inspect stage", error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != snapshot.stamp.owner || metadata.gid() != snapshot.stamp.group {
                return Err(Failure::new(
                    ExitClass::FilesystemError,
                    format!(
                        "replacement cannot preserve owner/group for {}",
                        snapshot.path.display()
                    ),
                ));
            }
        }
        check_write_metadata(temporary.path(), temporary.as_file(), &metadata)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| filesystem_error(temporary.path(), "cannot sync stage", error))?;
        // Verify through the staged handle's identity before any destination changes.
        let stage_stamp = Stamp::of(&temporary.as_file().metadata().map_err(|error| {
            filesystem_error(temporary.path(), "cannot inspect synced stage", error)
        })?);
        verify_current(temporary.path(), &stage_stamp, text)?;
        Ok(stage_stamp)
    })();
    match prepared {
        Ok(stamp) => Ok(PreparedFile { file: temporary, stamp }),
        Err(error) => {
            let path = temporary.path().to_string_lossy().into_owned();
            if let Err(cleanup_error) = temporary.close() {
                return Err(Failure::new(
                    error.class,
                    format!("{}; cannot remove temporary {path}: {cleanup_error}", error.message),
                ));
            }
            Err(error)
        }
    }
}

struct Stage<'a> {
    input: &'a WriteInput<'a>,
    replacement: Option<PreparedFile>,
    backup: Option<PreparedFile>,
    committed: Option<(File, Stamp)>,
}

pub(crate) fn commit(inputs: &[WriteInput<'_>]) -> WriteReport {
    commit_with_hook(inputs, |_, _| Ok(()))
}

fn commit_with_hook(
    inputs: &[WriteInput<'_>],
    mut before_commit: impl FnMut(usize, &Path) -> Result<(), Failure>,
) -> WriteReport {
    let mut report = WriteReport {
        exit_class: ExitClass::Success,
        error: None,
        effects: inputs
            .iter()
            .map(|input| (input.snapshot.path.to_string_lossy().into_owned(), "not_attempted"))
            .collect(),
        recovery_files: Vec::new(),
        unverified_restore_paths: Vec::new(),
        cleanup_failures: Vec::new(),
        guarantee: "plan_first_with_guarded_rollback; individual_atomic_replacements; not_crash_atomic; advisory_locks",
    };
    let mut stages = Vec::new();
    let result = (|| -> Result<(), Failure> {
        // Revalidate the whole selection, including no-change rows, before
        // creating replacement files or mutating any selected destination.
        for input in inputs {
            verify_current(&input.snapshot.path, &input.snapshot.stamp, input.original)?;
        }
        for input in inputs {
            if input.original == input.rendered {
                report
                    .effects
                    .insert(input.snapshot.path.to_string_lossy().into_owned(), "unchanged");
                continue;
            }
            let replacement = stage_text(input.snapshot, input.rendered, ".perl-tidy-stage-")?;
            stages.push(Stage {
                input,
                replacement: Some(replacement),
                backup: None,
                committed: None,
            });
            let backup = stage_text(input.snapshot, input.original, ".perl-tidy-recovery-")?;
            let stage = stages
                .last_mut()
                .ok_or_else(|| Failure::new(ExitClass::InstrumentFailure, "new stage missing"))?;
            stage.backup = Some(backup);
        }
        // Staging itself can take time. No stale no-change row may authorize a
        // batch merely because all changed rows are still current.
        for input in inputs {
            verify_current(&input.snapshot.path, &input.snapshot.stamp, input.original)?;
        }
        for (index, stage) in stages.iter_mut().enumerate() {
            let path = &stage.input.snapshot.path;
            before_commit(index, path)?;
            verify_current(path, &stage.input.snapshot.stamp, stage.input.original)?;
            let replacement = stage.replacement.take().ok_or_else(|| {
                Failure::new(ExitClass::InstrumentFailure, "replacement stage missing")
            })?;
            if let Err(error) =
                verify_current(replacement.file.path(), &replacement.stamp, stage.input.rendered)
            {
                stage.replacement = Some(replacement);
                return Err(error);
            }
            let committed = match replacement.file.persist(path) {
                Ok(file) => file,
                Err(error) => {
                    let message = format!("cannot replace {}: {}", path.display(), error.error);
                    stage.replacement =
                        Some(PreparedFile { file: error.file, stamp: replacement.stamp });
                    return Err(Failure::new(ExitClass::FilesystemError, message));
                }
            };
            let metadata = committed
                .metadata()
                .map_err(|error| filesystem_error(path, "cannot inspect replacement", error));
            // Once persist succeeded, record the changed subject even if a
            // later verification fails. Otherwise rollback would omit it.
            let stamp = match metadata {
                Ok(metadata) => Stamp::of(&metadata),
                Err(error) => {
                    stage.committed = Some((committed, stage.input.snapshot.stamp.clone()));
                    return Err(error);
                }
            };
            stage.committed = Some((committed, stamp.clone()));
            report.effects.insert(path.to_string_lossy().into_owned(), "committed");
            verify_current(path, &stamp, stage.input.rendered)?;
            sync_parent(path)?;
        }
        for input in inputs {
            if input.original == input.rendered {
                verify_current(&input.snapshot.path, &input.snapshot.stamp, input.original)?;
            }
        }
        for stage in &stages {
            if let Some((_, stamp)) = &stage.committed {
                verify_current(&stage.input.snapshot.path, stamp, stage.input.rendered)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.exit_class = error.class;
        report.error = Some(error.message);
        rollback(&mut stages, &mut report);
    }
    cleanup(&mut stages, &mut report);
    report
}

fn sync_parent(path: &Path) -> Result<(), Failure> {
    let parent = path
        .parent()
        .ok_or_else(|| Failure::new(ExitClass::FilesystemError, "selected file has no parent"))?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|error| filesystem_error(parent, "cannot sync directory", error))
}

fn rollback(stages: &mut [Stage<'_>], report: &mut WriteReport) {
    for stage in stages.iter_mut().rev() {
        let Some((_, stamp)) = &stage.committed else {
            continue;
        };
        let path = &stage.input.snapshot.path;
        let mut moved_backup = false;
        let restored = (|| -> Result<(), Failure> {
            // A later writer's bytes take priority over our rollback. Keep a
            // recovery copy rather than overwrite a source we no longer own.
            verify_current(path, stamp, stage.input.rendered)?;
            let backup = stage.backup.as_ref().ok_or_else(|| {
                Failure::new(ExitClass::InstrumentFailure, "original recovery stage missing")
            })?;
            verify_current(backup.file.path(), &backup.stamp, stage.input.original)?;
            let backup = stage.backup.take().ok_or_else(|| {
                Failure::new(ExitClass::InstrumentFailure, "original recovery stage missing")
            })?;
            let restored_stamp = backup.stamp;
            let _file = match backup.file.persist(path) {
                Ok(file) => file,
                Err(error) => {
                    let message = format!("cannot restore {}: {}", path.display(), error.error);
                    stage.backup = Some(PreparedFile { file: error.file, stamp: restored_stamp });
                    return Err(Failure::new(ExitClass::FilesystemError, message));
                }
            };
            moved_backup = true;
            verify_current(path, &restored_stamp, stage.input.original)?;
            sync_parent(path)?;
            Ok(())
        })();
        match restored {
            Ok(()) => {
                report.effects.insert(path.to_string_lossy().into_owned(), "rolled_back");
            }
            Err(error) => {
                report.exit_class = ExitClass::RecoveryRequired;
                report.effects.insert(path.to_string_lossy().into_owned(), "recovery_required");
                let prior = report.error.take().unwrap_or_default();
                report.error = Some(format!(
                    "{prior}; rollback withheld or failed for {}: {}",
                    path.display(),
                    error.message
                ));
                if let Some(backup) = stage.backup.take() {
                    let backup_path = backup.file.path().to_string_lossy().into_owned();
                    if let Err(mut error) = backup.file.keep() {
                        // Retain the recovery copy even if the platform could
                        // not mark it permanent. Losing it would be worse than
                        // returning a precise recovery-required disposition.
                        error.file.disable_cleanup(true);
                    }
                    report.recovery_files.push(backup_path);
                } else if moved_backup {
                    // The prepared original was renamed to this destination,
                    // but final verification/durability was not established.
                    // Do not call it a separately retained verified backup.
                    report.unverified_restore_paths.push(path.to_string_lossy().into_owned());
                }
            }
        }
    }
}

fn cleanup(stages: &mut [Stage<'_>], report: &mut WriteReport) {
    for stage in stages {
        for temporary in [stage.replacement.take(), stage.backup.take()].into_iter().flatten() {
            let path = temporary.file.path().to_string_lossy().into_owned();
            if let Err(error) = temporary.file.close() {
                report.cleanup_failures.push(path.clone());
                if report.exit_class == ExitClass::Success {
                    report.exit_class = ExitClass::FilesystemError;
                }
                let prior = report.error.take().unwrap_or_default();
                report.error = Some(format!("{prior}; cannot remove temporary {path}: {error}"));
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct AllowStagingOnDrop;
    impl Drop for AllowStagingOnDrop {
        fn drop(&mut self) {
            DENY_STAGING.set(false);
        }
    }

    fn snapshot(path: &Path) -> Result<(Snapshot, String), Box<dyn std::error::Error>> {
        read_snapshot(path, 1024, true).map_err(|error| error.message.into())
    }

    fn no_temporaries(directory: &Path) -> TestResult {
        for entry in fs::read_dir(directory)? {
            assert!(!entry?.file_name().to_string_lossy().starts_with(".perl-tidy-"));
        }
        Ok(())
    }

    #[test]
    fn stale_no_change_member_blocks_the_complete_batch() -> TestResult {
        let directory = tempfile::tempdir()?;
        let a = directory.path().join("a.pl");
        let b = directory.path().join("b.pl");
        fs::write(&a, "old a\n")?;
        fs::write(&b, "old b\n")?;
        let (sa, a_before) = snapshot(&a)?;
        let (sb, b_before) = snapshot(&b)?;
        fs::write(&b, "new user b\n")?;
        let report = commit(&[
            WriteInput { snapshot: &sa, original: &a_before, rendered: "formatted a\n" },
            WriteInput { snapshot: &sb, original: &b_before, rendered: &b_before },
        ]);
        assert_eq!(report.exit_class, ExitClass::StaleSource);
        assert_eq!(fs::read_to_string(&a)?, a_before);
        assert_eq!(fs::read_to_string(&b)?, "new user b\n");
        no_temporaries(directory.path())
    }

    #[test]
    fn rollback_reuses_prepared_original_when_new_staging_is_unavailable() -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let _restore_staging = AllowStagingOnDrop;
        let directory = tempfile::tempdir()?;
        let a = directory.path().join("a.pl");
        let b = directory.path().join("b.pl");
        fs::write(&a, "old a\n")?;
        fs::write(&b, "old b\n")?;
        fs::set_permissions(&a, Permissions::from_mode(0o751))?;
        let (sa, a_before) = snapshot(&a)?;
        let (sb, b_before) = snapshot(&b)?;
        let report = commit_with_hook(
            &[
                WriteInput { snapshot: &sa, original: &a_before, rendered: "formatted a\n" },
                WriteInput { snapshot: &sb, original: &b_before, rendered: "formatted b\n" },
            ],
            |index, _| {
                if index == 1 {
                    // A rollback implementation that creates/writes another
                    // temporary now fails. The original backup is already
                    // complete and must suffice to restore the first file.
                    DENY_STAGING.set(true);
                    Err(Failure::new(ExitClass::FilesystemError, "injected commit failure"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(report.exit_class, ExitClass::FilesystemError);
        assert_eq!(report.effect_for(&a.to_string_lossy()), "rolled_back");
        assert_eq!(fs::read_to_string(&a)?, a_before);
        assert_eq!(fs::metadata(&a)?.permissions().mode() & 0o777, 0o751);
        assert_eq!(fs::read_to_string(&b)?, b_before);
        no_temporaries(directory.path())
    }

    #[test]
    fn rollback_does_not_overwrite_another_writer_and_retains_original() -> TestResult {
        let directory = tempfile::tempdir()?;
        let a = directory.path().join("a.pl");
        let b = directory.path().join("b.pl");
        fs::write(&a, "old a\n")?;
        fs::write(&b, "old b\n")?;
        let (sa, a_before) = snapshot(&a)?;
        let (sb, b_before) = snapshot(&b)?;
        let report = commit_with_hook(
            &[
                WriteInput { snapshot: &sa, original: &a_before, rendered: "formatted a\n" },
                WriteInput { snapshot: &sb, original: &b_before, rendered: "formatted b\n" },
            ],
            |index, _| {
                if index == 1 {
                    fs::write(&a, "concurrent user a\n")
                        .map_err(|error| filesystem_error(&a, "test write", error))?;
                    return Err(Failure::new(
                        ExitClass::FilesystemError,
                        "injected commit failure",
                    ));
                }
                Ok(())
            },
        );
        assert_eq!(report.exit_class, ExitClass::RecoveryRequired);
        assert_eq!(fs::read_to_string(&a)?, "concurrent user a\n");
        assert_eq!(fs::read_to_string(&b)?, b_before);
        assert_eq!(report.recovery_files.len(), 1);
        assert_eq!(fs::read_to_string(&report.recovery_files[0])?, a_before);
        Ok(())
    }
}
