//! Bounded source snapshots and read-only currentness checks.

use crate::app::{ExitClass, Failure};
use std::fs::{self, File, Metadata, Permissions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    // Retain the original file identity while the selected input is in use.
    _file: File,
    stamp: Stamp,
}

fn filesystem_error(path: &Path, action: &str, error: impl std::fmt::Display) -> Failure {
    Failure::new(ExitClass::FilesystemError, format!("{action} {}: {error}", path.display()))
}

fn stale(path: &Path) -> Failure {
    Failure::new(
        ExitClass::StaleSource,
        format!("source or observed metadata changed after reading: {}", path.display()),
    )
}

#[cfg(unix)]
fn open_source(path: &Path) -> std::io::Result<File> {
    // O_NONBLOCK prevents a raced FIFO replacement from blocking before the
    // handle's regular-file metadata can be checked. No write handle is opened.
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

pub(crate) fn read_snapshot(path: &Path, limit: u64) -> Result<(Snapshot, String), Failure> {
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
    let source = read_bounded(&mut file, limit)?;
    let after = file
        .metadata()
        .map_err(|error| filesystem_error(path, "cannot recheck opened file", error))?;
    if stamp != Stamp::of(&after) || source.len() as u64 != stamp.length {
        return Err(stale(path));
    }
    Ok((Snapshot { path: path.to_path_buf(), _file: file, stamp }, source))
}

pub(crate) fn verify_snapshot(snapshot: &Snapshot, expected: &str) -> Result<(), Failure> {
    let path = &snapshot.path;
    let stamp = &snapshot.stamp;
    let before = fs::symlink_metadata(path).map_err(|_| stale(path))?;
    if !before.is_file() || Stamp::of(&before) != *stamp {
        return Err(stale(path));
    }
    let mut file = open_source(path).map_err(|_| stale(path))?;
    let opened = file.metadata().map_err(|_| stale(path))?;
    if !opened.is_file() || Stamp::of(&opened) != *stamp {
        return Err(stale(path));
    }
    let actual = read_bounded(&mut file, expected.len() as u64).map_err(|_| stale(path))?;
    let after = file.metadata().map_err(|_| stale(path))?;
    if actual != expected || Stamp::of(&after) != *stamp {
        return Err(stale(path));
    }
    Ok(())
}

pub(crate) fn read_bounded(reader: &mut dyn Read, limit: u64) -> Result<String, Failure> {
    let mut bytes = Vec::new();
    reader.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|error| {
        Failure::new(ExitClass::FilesystemError, format!("cannot read input: {error}"))
    })?;
    if bytes.len() as u64 > limit {
        return Err(Failure::new(
            ExitClass::ResourceLimited,
            format!("input exceeds the {limit}-byte limit"),
        ));
    }
    String::from_utf8(bytes).map_err(|error| {
        Failure::new(ExitClass::InvalidInput, format!("input is not valid UTF-8: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn unchanged_snapshot_revalidates_and_changed_bytes_do_not() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("source.pl");
        fs::write(&path, "my $x = 1;\n")?;
        let (snapshot, source) = read_snapshot(&path, 1024).map_err(|error| error.message)?;
        verify_snapshot(&snapshot, &source).map_err(|error| error.message)?;

        // Preserve length, identity and modification time: a metadata-only
        // checker would miss this edit. Exact source bytes must be rechecked.
        let modified = fs::metadata(&path)?.modified()?;
        fs::write(&path, "my $x = 2;\n")?;
        File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(modified))?;
        assert_eq!(Stamp::of(&fs::metadata(&path)?), snapshot.stamp);
        let result = verify_snapshot(&snapshot, &source);
        assert!(matches!(result, Err(Failure { class: ExitClass::StaleSource, .. })));
        assert_eq!(fs::read_to_string(&path)?, "my $x = 2;\n");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_substituted_after_reading_is_stale() -> TestResult {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("source.pl");
        let other = directory.path().join("other.pl");
        fs::write(&path, "my $x = 1;\n")?;
        fs::write(&other, "my $x = 1;\n")?;
        let (snapshot, source) = read_snapshot(&path, 1024).map_err(|error| error.message)?;
        fs::remove_file(&path)?;
        symlink(&other, &path)?;
        let result = verify_snapshot(&snapshot, &source);
        assert!(matches!(result, Err(Failure { class: ExitClass::StaleSource, .. })));
        assert!(fs::symlink_metadata(&path)?.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&other)?, source);
        Ok(())
    }
}
