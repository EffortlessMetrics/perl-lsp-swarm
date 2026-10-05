//! Shared forensic filesystem primitives for linked-worktree evidence and backup.
//!
//! Path-safety, stable-read, and directory-race checks have one authority so the
//! observer and the backup writer cannot diverge on symlink/reparse refusal or
//! sampled-interval identity. This module does not classify recovery evidence
//! and does not restore, repair, or apply Git administration.

use color_eyre::eyre::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

pub trait StableFileReader {
    fn read_twice(&self, path: &Path, max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)>;
}

pub(crate) struct FilesystemReader;

impl StableFileReader for FilesystemReader {
    fn read_twice(&self, path: &Path, max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)> {
        let mut file = fs::File::open(path)?;
        let first = read_at_most(&mut file, max_bytes)?;
        file.seek(SeekFrom::Start(0))?;
        let second = read_at_most(&mut file, max_bytes)?;
        Ok((first, second))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StableRead {
    Stable(Vec<u8>),
    Unstable(String),
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetadataFingerprint {
    length: u64,
    modified_nanos: Option<u128>,
    #[cfg(windows)]
    file_identity: Option<crate::file_identity::WindowsFileIdentity>,
    #[cfg(not(windows))]
    file_identity: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirectoryFingerprint {
    modified_nanos: Option<u128>,
    #[cfg(windows)]
    file_identity: crate::file_identity::WindowsFileIdentity,
    #[cfg(not(windows))]
    file_identity: (u64, u64),
}

pub(crate) fn read_stable_file(
    path: &Path,
    reader: &dyn StableFileReader,
    max_bytes: u64,
) -> StableRead {
    let before = match metadata_fingerprint(path) {
        Ok(value) => value,
        Err(error) => {
            return StableRead::Unavailable(format!("reading {}: {error}", path.display()));
        }
    };
    let (first, second) = match reader.read_twice(path, max_bytes) {
        Ok(bytes) => bytes,
        Err(error) => {
            return StableRead::Unavailable(format!(
                "reading {} through an open handle: {error}",
                path.display()
            ));
        }
    };
    let after = match metadata_fingerprint(path) {
        Ok(value) => value,
        Err(error) => {
            return StableRead::Unavailable(format!("finalizing {}: {error}", path.display()));
        }
    };
    if before != after || first != second {
        return StableRead::Unstable(format!(
            "RACE_DETECTED while observing {}; path identity, metadata, or bytes changed during the open-handle read interval",
            path.display(),
        ));
    }
    // This is an observation-time guarantee. A replacement after the final
    // metadata check cannot be ruled out by a portable read-only observer;
    // callers must revalidate immediately before any separately authorized
    // action and must treat a changed digest as stale evidence.
    StableRead::Stable(first)
}

pub(crate) fn read_at_most(file: &mut fs::File, max_bytes: u64) -> io::Result<Vec<u8>> {
    let limit = max_bytes.saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::other(format!(
            "file exceeds bounded observation size of {max_bytes} bytes"
        )));
    }
    Ok(bytes)
}

pub(crate) fn metadata_fingerprint(path: &Path) -> io::Result<MetadataFingerprint> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(io::Error::other("path is not a regular non-reparse file"));
    }
    let file_identity = file_identity(path, &metadata)?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Ok(MetadataFingerprint { length: metadata.len(), modified_nanos, file_identity })
}

pub(crate) fn directory_fingerprint(path: &Path) -> io::Result<DirectoryFingerprint> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(io::Error::other("path is not a regular non-reparse directory"));
    }
    let file_identity = file_identity(path, &metadata)?.ok_or_else(|| {
        io::Error::other("stable directory identity is unavailable on this platform")
    })?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Ok(DirectoryFingerprint { modified_nanos, file_identity })
}

#[cfg(unix)]
fn file_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<Option<(u64, u64)>> {
    use std::os::unix::fs::MetadataExt;
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(windows)]
fn file_identity(
    path: &Path,
    _metadata: &fs::Metadata,
) -> io::Result<Option<crate::file_identity::WindowsFileIdentity>> {
    match crate::file_identity::windows_file_identity(path) {
        Ok(Some(identity)) => Ok(Some(identity)),
        Ok(None) => Err(io::Error::other(
            "stable Windows file identity became unavailable during observation",
        )),
        Err(error) => {
            Err(io::Error::other(format!("stable Windows file identity is unavailable: {error:#}")))
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_path: &Path, _metadata: &fs::Metadata) -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}

pub(crate) fn read_bounded_entries(
    current: &Path,
    maximum: usize,
) -> io::Result<(Vec<fs::DirEntry>, bool)> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for result in fs::read_dir(current)? {
        let entry = result?;
        if entries.len() >= maximum {
            truncated = true;
            break;
        }
        entries.push(entry);
    }
    Ok((entries, truncated))
}

pub(crate) fn lexical_is_within(parent: &Path, child: &Path) -> bool {
    let parent = match lexical_normalize(&normalize_extended_prefix(parent)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    let child = match lexical_normalize(&normalize_extended_prefix(child)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    child.len() >= parent.len()
        && child.iter().zip(parent.iter()).all(|(left, right)| left == right)
}

/// Like [`lexical_is_within`], but `child` must name a path strictly below
/// `parent`: the parent itself does not count as within.
pub(crate) fn lexical_is_strictly_within(parent: &Path, child: &Path) -> bool {
    let parent = match lexical_normalize(&normalize_extended_prefix(parent)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    let child = match lexical_normalize(&normalize_extended_prefix(child)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    child.len() > parent.len() && child.iter().zip(parent.iter()).all(|(left, right)| left == right)
}

pub(crate) fn lexical_normalize(path: &Path) -> std::result::Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(String::from("path traversal escapes filesystem root"));
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

pub(crate) fn observe_existing_directory(path: &Path) -> Result<PathBuf> {
    if has_link_or_reparse_component(path) {
        bail!("repository path contains a symlink or reparse point");
    }
    let canonical = fs::canonicalize(path)
        .wrap_err_with(|| format!("resolving repository identity {}", path.display()))?;
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        bail!("repository path is not a regular directory");
    }
    Ok(canonical)
}

pub(crate) fn has_link_or_reparse_component(path: &Path) -> bool {
    let mut current = if path.is_absolute() {
        PathBuf::new()
    } else {
        match std::env::current_dir() {
            Ok(directory) => directory,
            Err(_) => return true,
        }
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(component.as_os_str()),
            Component::Normal(name) => current.push(name),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => return true,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
    }
    false
}

pub(crate) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn platform_path_key(path: &Path) -> String {
    path_components_key(&normalize_extended_prefix(path)).join("/")
}

fn path_components_key(path: &Path) -> Vec<String> {
    path.components().map(path_component_key).collect()
}

pub(crate) fn path_component_key(component: Component<'_>) -> String {
    let (tag, value) = match component {
        Component::Prefix(prefix) => ("P", prefix.as_os_str()),
        Component::RootDir => ("R", OsStr::new("/")),
        Component::CurDir => ("C", OsStr::new(".")),
        Component::ParentDir => ("U", OsStr::new("..")),
        Component::Normal(value) => ("N", value),
    };
    format!("{tag}{}", encoded_os_str(value))
}

#[cfg(unix)]
fn encoded_os_str(value: &OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    hex_bytes(value.as_bytes())
}

#[cfg(windows)]
fn encoded_os_str(value: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    let mut output = String::new();
    for unit in value.encode_wide() {
        let normalized = if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
            unit + (u16::from(b'a') - u16::from(b'A'))
        } else {
            unit
        };
        output.push_str(&format!("{normalized:04x}"));
    }
    output
}

#[cfg(not(any(unix, windows)))]
fn encoded_os_str(value: &OsStr) -> String {
    value.to_string_lossy().to_string()
}

#[cfg(not(windows))]
fn normalize_extended_prefix(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(windows)]
fn normalize_extended_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", rest))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(unix)]
fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn is_source_like_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    matches!(extension.to_ascii_lowercase().as_str(), "pl" | "pm" | "pod" | "t" | "plx" | "xs")
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn os_str_order(left: &OsStr, right: &OsStr) -> Ordering {
    left.to_string_lossy().cmp(&right.to_string_lossy())
}

pub(crate) fn paths_overlap(left: &Path, right: &Path) -> bool {
    lexical_is_within(left, right) || lexical_is_within(right, left)
}
