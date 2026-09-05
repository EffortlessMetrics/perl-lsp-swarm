//! Git-aware Perl workspace file discovery.
//!
//! Finds Perl source files in a workspace root with a two-step strategy:
//! 1. Try `git ls-files` for fast, `.gitignore`-aware enumeration.
//! 2. Fall back to filesystem walking with `WalkDir` when git is unavailable.
//!
//! The resulting behavior is intentionally conservative: common non-source directories
//! are skipped in both modes (`.git`, `.hg`, `.svn`, `target`, `node_modules`, `.cache`).
//! Symlinked files and directories inside the workspace are followed so shared library
//! trees remain visible; external targets require an explicit include path. `WalkDir`'s
//! loop detection prevents cyclic links from making discovery unbounded.
//! Explicit include roots can relax that skip only for configured Perl dependency
//! trees such as `local/lib/perl5`.

use crate::ignore::{is_skipped_dir_name_with_extra, path_contains_skipped_component_with_extra};
use perl_parser_core::source_file::{
    is_perl_source_bytes, is_perl_source_extension, is_perl_source_path,
};
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Component;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use walkdir::{DirEntry, WalkDir};

const GIT_LS_FILES_ARGS: [&str; 5] =
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard"];

/// How files were discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryMethod {
    /// Files discovered via `git ls-files`.
    Git,
    /// Files discovered via `WalkDir` traversal.
    Walk,
}

/// File discovery result metadata.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Discovered Perl source files.
    pub files: Vec<PathBuf>,
    /// Discovery method used.
    pub method: DiscoveryMethod,
    /// Elapsed discovery duration.
    pub duration: Duration,
    /// Number of entries excluded by extension/skip rules.
    pub excluded_count: usize,
    /// Whether discovery stopped early because the caller requested cancellation.
    pub cancelled: bool,
}

/// Additive discovery policy overrides.
///
/// Built-in extension and skipped-directory defaults remain active. These
/// lists let a workspace add project-specific extensions or noise directories
/// without replacing the safe defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryConfig {
    /// Additional file extensions accepted by discovery, without a leading dot.
    pub extra_extensions: Vec<String>,
    /// Additional directory names skipped by discovery.
    pub extra_skipped_dirs: Vec<String>,
}

impl DiscoveryConfig {
    /// Create a normalized additive discovery policy.
    #[must_use]
    pub fn new(extra_extensions: Vec<String>, extra_skipped_dirs: Vec<String>) -> Self {
        Self {
            extra_extensions: normalize_extensions(extra_extensions),
            extra_skipped_dirs: normalize_names(extra_skipped_dirs),
        }
    }

    /// Returns `true` when `path` carries an extension this policy admits,
    /// independent of file contents.
    ///
    /// This is the extension half of the single admission authority
    /// (#14186): recognized Perl source extensions, the built-in
    /// discovery-only formats (`.xs`, `.i`), and the normalized configured
    /// extras all admit by path, so callers that share one
    /// [`DiscoveryConfig`] cannot disagree about an extension-bearing file.
    #[must_use]
    pub fn admits_extension(&self, path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
            is_perl_source_extension(ext)
                || is_builtin_discovery_extension(ext)
                || self.extra_extensions.iter().any(|candidate| candidate.eq_ignore_ascii_case(ext))
        })
    }

    /// Returns `true` when this policy admits `path` as a Perl workspace
    /// source, classifying extensionless candidates from `bytes`.
    ///
    /// This is the one admission authority shared by workspace discovery,
    /// the startup indexing final seam, watcher reclassification, and
    /// rename preflight (#14186). Extension-bearing paths admit through
    /// [`Self::admits_extension`]; extensionless paths admit only when
    /// `bytes` carry a Perl shebang, so callers must pass the bytes of the
    /// same object they intend to consume (never re-read the path here).
    #[must_use]
    pub fn admits_bytes(&self, path: &Path, bytes: &[u8]) -> bool {
        if is_perl_source_bytes(path, bytes) {
            return true;
        }
        // `is_perl_source_bytes` rejects every extension-bearing path that is
        // not a recognized Perl source; those may still be discovery-only
        // formats this policy admits.
        path.extension().is_some() && self.admits_extension(path)
    }

    fn is_discovery_path(&self, path: &Path) -> bool {
        // With no extension, `is_perl_source_path` degenerates to the
        // extensionless shebang probe on disk.
        self.admits_extension(path) || (path.extension().is_none() && is_perl_source_path(path))
    }
}

fn normalize_extensions(values: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim().trim_start_matches('.');
        if !value.is_empty() && !normalized.iter().any(|item| item.eq_ignore_ascii_case(value)) {
            normalized.push(value.to_ascii_lowercase());
        }
    }
    normalized
}

fn normalize_names(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !normalized.iter().any(|item| item == value) {
            normalized.push(value.to_string());
        }
    }
    normalized
}

/// Discover Perl source files under `root`.
///
/// Strategy:
/// 1. Attempt `git ls-files -z --cached --others --exclude-standard`
/// 2. If git is unavailable or the root is not a repository, use `WalkDir`
#[must_use]
pub fn discover_perl_files(root: &Path) -> DiscoveryResult {
    discover_perl_files_with_config(root, &[] as &[&Path], &DiscoveryConfig::default())
}

/// Discover Perl source files under `root`, honoring explicitly configured include paths.
///
/// This preserves the normal skip-by-default behavior for noisy directories,
/// but allows traversal when a skipped directory is an ancestor of an include
/// root such as `local/lib/perl5`, `blib`, or `vendor`.
#[must_use]
pub fn discover_perl_files_with_include_paths<P>(
    root: &Path,
    include_paths: &[P],
) -> DiscoveryResult
where
    P: AsRef<Path>,
{
    discover_perl_files_with_config(root, include_paths, &DiscoveryConfig::default())
}

/// Discover Perl source files with include-path and additive policy overrides.
#[must_use]
pub fn discover_perl_files_with_config<P>(
    root: &Path,
    include_paths: &[P],
    config: &DiscoveryConfig,
) -> DiscoveryResult
where
    P: AsRef<Path>,
{
    discover_perl_files_with_config_and_cancel(root, include_paths, config, || false)
}

/// Discover Perl source files while cooperatively observing cancellation.
///
/// The callback is checked while waiting for `git ls-files`, while parsing its
/// output, and during filesystem walking. A cancelled result contains any
/// files discovered before cancellation and sets [`DiscoveryResult::cancelled`]
/// to `true`.
#[must_use]
pub fn discover_perl_files_with_config_and_cancel<P, F>(
    root: &Path,
    include_paths: &[P],
    config: &DiscoveryConfig,
    should_cancel: F,
) -> DiscoveryResult
where
    P: AsRef<Path>,
    F: Fn() -> bool,
{
    let allowlist = DiscoveryIncludeAllowlist::from_include_paths(root, include_paths, config);
    discover_perl_files_with_allowlist(root, &allowlist, config, &should_cancel)
}

fn discover_perl_files_with_allowlist(
    root: &Path,
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
    should_cancel: &impl Fn() -> bool,
) -> DiscoveryResult {
    let start = Instant::now();

    match try_git_discovery(root, start, allowlist, config, should_cancel) {
        Ok(GitDiscoveryOutcome::Complete(result)) => result,
        Ok(GitDiscoveryOutcome::Cancelled) => DiscoveryResult {
            files: Vec::new(),
            method: DiscoveryMethod::Git,
            duration: start.elapsed(),
            excluded_count: 0,
            cancelled: true,
        },
        Err(_) => walk_discovery_with_allowlist(root, start, allowlist, config, should_cancel),
    }
}

/// Returns `true` if `path` should be considered discoverable by workspace
/// indexing.
///
/// This intentionally includes XS implementation files, SWIG interface files,
/// and common Perl templating formats so editor discovery can surface them
/// even though they are not classified as Perl source files by the shared
/// source-file helper.
#[must_use]
pub fn is_perl_discovery_path(path: &Path) -> bool {
    DiscoveryConfig::default().is_discovery_path(path)
}

fn is_builtin_discovery_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("i") || extension.eq_ignore_ascii_case("xs")
}

fn try_git_discovery(
    root: &Path,
    start: Instant,
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
    should_cancel: &impl Fn() -> bool,
) -> Result<GitDiscoveryOutcome, std::io::Error> {
    if should_cancel() {
        return Ok(GitDiscoveryOutcome::Cancelled);
    }

    let mut child = std::process::Command::new("git")
        .args(GIT_LS_FILES_ARGS)
        .current_dir(root)
        // `git ls-files` never reads standard input; without an explicit
        // null stdin the child inherits the server's transport pipe. On
        // Windows a spawned git that inherits an open, non-console stdin
        // pipe blocks instead of exiting (observed: `git ls-files` outside
        // a repository stays alive until the next client write on that
        // pipe), coupling background scan completion to unrelated client
        // traffic and stalling the index coordinator in a Building state.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("git ls-files did not provide a stdout pipe"))?;
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });

    let status = loop {
        if should_cancel() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Ok(GitDiscoveryOutcome::Cancelled);
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = reader
        .join()
        .map_err(|_| std::io::Error::other("git ls-files reader thread panicked"))??;

    if !status.success() {
        return Err(std::io::Error::other("git ls-files failed"));
    }

    let (mut files, mut excluded_count, cancelled) =
        parse_git_ls_files_output_with_cancel(root, &stdout, allowlist, config, should_cancel);
    if cancelled {
        return Ok(GitDiscoveryOutcome::Cancelled);
    }

    // `git ls-files` reports a tracked directory symlink as one entry and does
    // not enumerate the linked tree. Expand those entries separately so a
    // linked library remains visible in the fast path as well as the walk
    // fallback. The same skip and extension policy is applied to the expansion.
    let tracked_paths = cached_git_paths(root).unwrap_or_default();
    let (linked_files, linked_excluded, linked_cancelled) = discover_linked_git_directories(
        root,
        &stdout,
        &tracked_paths,
        allowlist,
        config,
        should_cancel,
    );
    if linked_cancelled {
        return Ok(GitDiscoveryOutcome::Cancelled);
    }
    files.extend(linked_files);
    excluded_count += linked_excluded;
    sort_paths_lexically(&mut files);
    files.dedup();

    let result = DiscoveryResult {
        files,
        method: DiscoveryMethod::Git,
        duration: start.elapsed(),
        excluded_count,
        cancelled: false,
    };

    log_discovery(&result);
    Ok(GitDiscoveryOutcome::Complete(result))
}

fn discover_linked_git_directories(
    root: &Path,
    stdout: &[u8],
    tracked_paths: &HashSet<PathBuf>,
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
    should_cancel: &impl Fn() -> bool,
) -> (Vec<PathBuf>, usize, bool) {
    let mut files = Vec::new();
    let mut excluded_count = 0;

    for entry in stdout.split(|byte| *byte == b'\0').filter(|entry| !entry.is_empty()) {
        if should_cancel() {
            return (files, excluded_count, true);
        }

        let relative_path = PathBuf::from(bytes_to_os_string(entry));
        if !tracked_paths.contains(&relative_path) {
            continue;
        }
        let path = root.join(&relative_path);
        let is_linked_directory = std::fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
            && std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir());
        if !is_linked_directory
            || !is_allowed_link_target(root, &path, allowlist)
            || is_skipped_path(root, &relative_path, allowlist)
        {
            continue;
        }

        let mut candidates = Vec::new();
        for linked_entry in
            WalkDir::new(&path).follow_links(true).into_iter().filter_entry(|entry| {
                !should_skip_dir_with_allowlist(root, entry, allowlist, config)
                    && is_allowed_link_target(root, entry.path(), allowlist)
            })
        {
            if should_cancel() {
                return (files, excluded_count, true);
            }
            let linked_entry = match linked_entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !linked_entry.file_type().is_file() {
                continue;
            }
            candidates.push(linked_entry.path().to_path_buf());
        }
        let ignored = git_ignored_paths(root, &candidates);
        for linked_path in candidates {
            let relative = linked_path.strip_prefix(root).unwrap_or(&linked_path);
            if ignored.contains(relative) {
                excluded_count += 1;
            } else if config.is_discovery_path(&linked_path) {
                files.push(linked_path);
            } else {
                excluded_count += 1;
            }
        }
    }

    (files, excluded_count, false)
}

fn is_skipped_path(
    root: &Path,
    relative_path: &Path,
    allowlist: &DiscoveryIncludeAllowlist,
) -> bool {
    !is_safe_relative_git_path(relative_path)
        || allowlist.has_unallowed_skipped_component(relative_path)
        || !root.join(relative_path).is_dir()
}

#[derive(Debug)]
enum GitDiscoveryOutcome {
    Complete(DiscoveryResult),
    Cancelled,
}

#[cfg(test)]
fn parse_git_ls_files_output(root: &Path, stdout: &[u8]) -> (Vec<PathBuf>, usize) {
    parse_git_ls_files_output_with_allowlist(
        root,
        stdout,
        &DiscoveryIncludeAllowlist::default(),
        &DiscoveryConfig::default(),
    )
}

#[cfg(test)]
fn parse_git_ls_files_output_with_allowlist(
    root: &Path,
    stdout: &[u8],
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
) -> (Vec<PathBuf>, usize) {
    let (files, excluded_count, _) =
        parse_git_ls_files_output_with_cancel(root, stdout, allowlist, config, &|| false);
    (files, excluded_count)
}

fn parse_git_ls_files_output_with_cancel(
    root: &Path,
    stdout: &[u8],
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
    should_cancel: &impl Fn() -> bool,
) -> (Vec<PathBuf>, usize, bool) {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut excluded_count: usize = 0;

    for entry in stdout.split(|byte| *byte == b'\0') {
        if should_cancel() {
            sort_paths_lexically(&mut files);
            return (files, excluded_count, true);
        }
        if entry.is_empty() {
            continue;
        }

        let relative_path = PathBuf::from(bytes_to_os_string(entry));
        let relative_path = relative_path.as_path();
        if !is_safe_relative_git_path(relative_path) {
            excluded_count += 1;
            continue;
        }
        if allowlist.has_unallowed_skipped_component(relative_path) {
            excluded_count += 1;
            continue;
        }

        let path = root.join(relative_path);
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && !is_allowed_link_target(root, &path, allowlist)
        {
            excluded_count += 1;
            continue;
        }
        if !config.is_discovery_path(&path) {
            excluded_count += 1;
            continue;
        }

        if should_require_existing_git_files(root) && !is_existing_regular_file(&path) {
            excluded_count += 1;
            continue;
        }

        if seen.insert(path.clone()) {
            files.push(path);
        } else {
            excluded_count += 1;
        }
    }

    sort_paths_lexically(&mut files);
    (files, excluded_count, false)
}

#[cfg(unix)]
fn bytes_to_os_string(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(not(unix))]
fn bytes_to_os_string(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

fn should_require_existing_git_files(root: &Path) -> bool {
    root.is_dir()
}

fn is_existing_regular_file(path: &Path) -> bool {
    // `metadata` follows symlinks. Git reports symlink entries through `ls-files`,
    // but the workspace should index a linked Perl file just like its target.
    std::fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn cached_git_paths(root: &Path) -> std::io::Result<HashSet<PathBuf>> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "--cached"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("git ls-files cached failed"));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|entry| !entry.is_empty())
        .map(bytes_to_os_string)
        .map(PathBuf::from)
        .collect())
}

fn git_ignored_paths(root: &Path, paths: &[PathBuf]) -> HashSet<PathBuf> {
    let relative_paths: Vec<PathBuf> = paths
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .collect();
    if relative_paths.is_empty() {
        return HashSet::new();
    }

    let mut child = match std::process::Command::new("git")
        .args(["check-ignore", "-z", "--stdin", "--no-index"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return HashSet::new(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        for path in &relative_paths {
            let _ = stdin.write_all(path.to_string_lossy().as_bytes());
            let _ = stdin.write_all(&[0]);
        }
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(_) => return HashSet::new(),
    };
    output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|entry| !entry.is_empty())
        .map(bytes_to_os_string)
        .map(PathBuf::from)
        .collect()
}

fn is_allowed_link_target(root: &Path, link: &Path, allowlist: &DiscoveryIncludeAllowlist) -> bool {
    let Ok(target) = link.canonicalize() else {
        return false;
    };
    let Ok(workspace_root) = root.canonicalize() else {
        return false;
    };
    target.starts_with(workspace_root)
        || allowlist.external_include_roots.iter().any(|allowed| target.starts_with(allowed))
}

#[cfg(test)]
fn walk_discovery(root: &Path, start: Instant) -> DiscoveryResult {
    walk_discovery_with_allowlist(
        root,
        start,
        &DiscoveryIncludeAllowlist::default(),
        &DiscoveryConfig::default(),
        &|| false,
    )
}

fn walk_discovery_with_allowlist(
    root: &Path,
    start: Instant,
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
    should_cancel: &impl Fn() -> bool,
) -> DiscoveryResult {
    let mut files = Vec::new();
    let mut excluded_count: usize = 0;
    let mut skipped_dir_count: usize = 0;
    let mut cancelled = false;

    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_entry(|entry| {
        if should_skip_dir_with_allowlist(root, entry, allowlist, config) {
            skipped_dir_count += 1;
            return false;
        }
        is_allowed_link_target(root, entry.path(), allowlist)
    }) {
        if should_cancel() {
            cancelled = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        if config.is_discovery_path(entry.path()) {
            files.push(entry.path().to_path_buf());
        } else {
            excluded_count += 1;
        }
    }
    excluded_count += skipped_dir_count;
    sort_paths_lexically(&mut files);

    let result = DiscoveryResult {
        files,
        method: DiscoveryMethod::Walk,
        duration: start.elapsed(),
        excluded_count,
        cancelled,
    };

    log_discovery(&result);
    result
}

#[cfg(test)]
fn should_skip_dir(entry: &DirEntry) -> bool {
    should_skip_dir_with_allowlist(
        Path::new(""),
        entry,
        &DiscoveryIncludeAllowlist::default(),
        &DiscoveryConfig::default(),
    )
}

fn should_skip_dir_with_allowlist(
    root: &Path,
    entry: &DirEntry,
    allowlist: &DiscoveryIncludeAllowlist,
    config: &DiscoveryConfig,
) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    if !is_skipped_dir_name_with_extra(
        &entry.file_name().to_string_lossy(),
        &config.extra_skipped_dirs,
    ) {
        return false;
    }

    let Ok(relative_path) = entry.path().strip_prefix(root) else {
        return true;
    };
    if relative_path.as_os_str().is_empty() {
        return false;
    }

    !allowlist.should_traverse_skipped_dir(relative_path)
}

fn sort_paths_lexically(paths: &mut [PathBuf]) {
    paths.sort_unstable_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
}

fn is_safe_relative_git_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path.components().any(|component| matches!(component, Component::ParentDir))
}

fn log_discovery(result: &DiscoveryResult) {
    tracing::debug!(
        files = result.files.len(),
        method = ?result.method,
        duration_ms = result.duration.as_secs_f64() * 1000.0,
        excluded = result.excluded_count,
        "workspace discovery complete"
    );
}

#[derive(Debug, Default)]
struct DiscoveryIncludeAllowlist {
    include_roots: Vec<PathBuf>,
    external_include_roots: Vec<PathBuf>,
    extra_skipped_dirs: Vec<String>,
}

impl DiscoveryIncludeAllowlist {
    fn from_include_paths<P>(
        workspace_root: &Path,
        include_paths: &[P],
        config: &DiscoveryConfig,
    ) -> Self
    where
        P: AsRef<Path>,
    {
        let mut include_roots = Vec::new();
        let mut external_include_roots = Vec::new();
        let mut seen = HashSet::new();

        for include_path in include_paths {
            if include_path.as_ref().is_absolute()
                && !include_path.as_ref().starts_with(workspace_root)
            {
                if let Ok(path) = include_path.as_ref().canonicalize() {
                    external_include_roots.push(path);
                }
                continue;
            }
            let Some(relative_path) = normalize_include_path(workspace_root, include_path.as_ref())
            else {
                continue;
            };

            if relative_path.as_os_str().is_empty()
                || !path_contains_skipped_component_with_extra(
                    &relative_path,
                    &config.extra_skipped_dirs,
                )
            {
                continue;
            }

            if seen.insert(relative_path.clone()) {
                include_roots.push(relative_path);
            }
        }

        Self {
            include_roots,
            external_include_roots,
            extra_skipped_dirs: config.extra_skipped_dirs.clone(),
        }
    }

    fn has_unallowed_skipped_component(&self, relative_path: &Path) -> bool {
        if !path_contains_skipped_component_with_extra(relative_path, &self.extra_skipped_dirs) {
            return false;
        }

        if let Some(remainder) = self.allowed_include_remainder(relative_path) {
            return path_contains_skipped_component_with_extra(remainder, &self.extra_skipped_dirs);
        }

        true
    }

    fn should_traverse_skipped_dir(&self, relative_dir: &Path) -> bool {
        self.include_roots.iter().any(|root| root == relative_dir || root.starts_with(relative_dir))
    }

    fn allowed_include_remainder<'a>(&self, relative_path: &'a Path) -> Option<&'a Path> {
        self.include_roots
            .iter()
            .filter(|root| relative_path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .and_then(|root| relative_path.strip_prefix(root).ok())
    }
}

fn normalize_include_path(workspace_root: &Path, include_path: &Path) -> Option<PathBuf> {
    let relative_path = if include_path.is_absolute() {
        include_path.strip_prefix(workspace_root).ok()?
    } else {
        include_path
    };

    normalize_relative_path(relative_path)
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryConfig, DiscoveryIncludeAllowlist, DiscoveryMethod, parse_git_ls_files_output,
        parse_git_ls_files_output_with_allowlist, should_skip_dir, walk_discovery,
    };
    use crate::ignore::path_contains_skipped_component;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn create_file(root: &Path, relative: &str) -> TestResult {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, "# synthetic\n")?;
        Ok(())
    }

    #[test]
    fn parses_git_output_and_filters_entries() {
        let root = Path::new("/tmp/workspace");
        let payload = b"lib/Foo.pm\0README.md\0node_modules/pkg.pm\0script.pl\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|path| path.ends_with("lib/Foo.pm")));
        assert!(files.iter().any(|path| path.ends_with("script.pl")));
        assert_eq!(excluded_count, 2);
    }

    #[test]
    fn skipped_component_detection_is_consistent() {
        assert!(path_contains_skipped_component(Path::new("/repo/node_modules/pkg.pm")));
        assert!(path_contains_skipped_component(Path::new("/repo/target/build/generated.pm")));
        assert!(!path_contains_skipped_component(Path::new("/repo/lib/My/Module.pm")));
    }

    /// #14186: the extension half of the admission authority must accept
    /// exactly what discovery admits — recognized Perl extensions, the
    /// built-in `.xs`/`.i` formats, and configured extras after the same
    /// normalization `DiscoveryConfig::new` applies to raw user config.
    #[test]
    fn admits_extension_covers_perl_builtins_and_normalized_extras() {
        let configured =
            DiscoveryConfig::new(vec![".FOO".to_string(), " Bar ".to_string()], Vec::new());

        assert!(configured.admits_extension(Path::new("lib/Mod.pm")));
        assert!(configured.admits_extension(Path::new("src/Native.xs")));
        assert!(configured.admits_extension(Path::new("src/NATIVE.XS")));
        assert!(configured.admits_extension(Path::new("swig/API.i")));
        assert!(configured.admits_extension(Path::new("assets/thing.foo")));
        assert!(configured.admits_extension(Path::new("assets/thing.FOO")));
        assert!(configured.admits_extension(Path::new("tpl/page.bar")));
        assert!(!configured.admits_extension(Path::new("README")));
        assert!(!configured.admits_extension(Path::new("assets/notes.txt")));
        // Extras are additive per policy: the default policy keeps its
        // built-ins only.
        assert!(!DiscoveryConfig::default().admits_extension(Path::new("assets/thing.foo")));
    }

    /// #14186: the bytes half of the admission authority classifies
    /// extensionless candidates from the provided bytes while extension
    /// admission stays path-only, so startup, watcher, and rename seams
    /// share one decision.
    #[test]
    fn admits_bytes_classifies_extensionless_from_shebang_bytes() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let script = tmp.path().join("deploy_hook");
        fs::write(&script, "#!/usr/bin/env perl\nprint 1;\n")?;
        let notes = tmp.path().join("notes");
        fs::write(&notes, "plain documentation\n")?;

        let script_bytes = fs::read(&script)?;
        assert!(
            DiscoveryConfig::default().admits_bytes(&script, &script_bytes),
            "extensionless shebang script must stay admitted from bytes"
        );
        let notes_bytes = fs::read(&notes)?;
        assert!(
            !DiscoveryConfig::default().admits_bytes(&notes, &notes_bytes),
            "extensionless non-Perl bytes must stay rejected"
        );
        // Discovery-only builtins admit by extension even with non-Perl
        // body bytes; configured extras keep the same admission after
        // normalization of raw ".FOO"-style config spellings.
        assert!(DiscoveryConfig::default().admits_bytes(Path::new("src/Native.xs"), b"MODULE"));
        let configured = DiscoveryConfig::new(vec![".FOO".to_string()], Vec::new());
        assert!(configured.admits_bytes(Path::new("assets/thing.foo"), b"\x00\x01"));
        Ok(())
    }

    #[test]
    fn parse_git_output_ignores_skipped_names_in_workspace_root_path() {
        let root = Path::new("/tmp/target/workspace");
        let payload = b"lib/Foo.pm\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("lib/Foo.pm"));
        assert_eq!(excluded_count, 0);
    }

    #[test]
    fn walk_discovery_ignores_skipped_directories() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "lib/Foo.pm")?;
        create_file(root, "node_modules/pkg.pm")?;
        create_file(root, "target/build/generated.pm")?;
        create_file(root, ".cache/precompiled.pm")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.method, DiscoveryMethod::Walk);
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib/Foo.pm"));

        Ok(())
    }

    #[test]
    fn walk_discovery_counts_skipped_directories_as_excluded() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "lib/Foo.pm")?;
        create_file(root, "node_modules/pkg.pm")?;
        create_file(root, "target/build/generated.pm")?;
        create_file(root, ".cache/precompiled.pm")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.method, DiscoveryMethod::Walk);
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib/Foo.pm"));
        assert_eq!(result.excluded_count, 3);

        Ok(())
    }

    fn git_on_path() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// Call-observation for the discovery git spawn: on a root outside any
    /// git repository, `git ls-files` fails fast and discovery returns the
    /// error variant that triggers the caller's walk fallback. The spawned
    /// git gets an explicit null stdin — a git inheriting an open,
    /// non-console stdin pipe blocks instead of exiting on Windows, which
    /// stalled background workspace scans until unrelated client input
    /// arrived. The bounded-time assertion observes that completion contract
    /// end to end through the spawn.
    #[test]
    fn try_git_discovery_errors_promptly_on_non_repo_root_without_caller_stdin() -> TestResult {
        if !git_on_path() {
            return Ok(());
        }
        let tmp = tempfile::tempdir()?;
        create_file(tmp.path(), "lib/One.pm")?;

        let started = Instant::now();
        let outcome = super::try_git_discovery(
            tmp.path(),
            started,
            &DiscoveryIncludeAllowlist::default(),
            &DiscoveryConfig::default(),
            &|| false,
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(30),
            "git discovery must complete without waiting on caller input; took {elapsed:?}"
        );
        assert!(
            outcome.is_err(),
            "git ls-files outside a repository must surface the error variant that selects the walk fallback, got {outcome:?}"
        );

        Ok(())
    }

    /// Call-observation for the success arm: inside a git repository the
    /// spawned `git ls-files` reports tracked files with `DiscoveryMethod::Git`.
    #[test]
    fn try_git_discovery_completes_from_git_repository() -> TestResult {
        if !git_on_path() {
            return Ok(());
        }
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        create_file(root, "lib/One.pm")?;
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()?;
        assert!(init.success(), "git init must succeed for the fixture");
        let add = std::process::Command::new("git")
            .args(["add", "lib/One.pm"])
            .current_dir(root)
            .status()?;
        assert!(add.success(), "git add must succeed for the fixture");

        let outcome = super::try_git_discovery(
            root,
            Instant::now(),
            &DiscoveryIncludeAllowlist::default(),
            &DiscoveryConfig::default(),
            &|| false,
        );

        match outcome {
            Ok(super::GitDiscoveryOutcome::Complete(result)) => {
                assert_eq!(result.method, DiscoveryMethod::Git);
                assert!(result.files.iter().any(|path| path.ends_with("lib/One.pm")));
            }
            other => panic!("expected a complete git discovery, got {other:?}"),
        }

        Ok(())
    }

    /// Exact boundary variant for the pre-spawn cancellation checkpoint: a
    /// `should_cancel()` observer that is already cancelled returns the
    /// Cancelled outcome without spawning the git child.
    #[test]
    fn try_git_discovery_cancelled_before_spawn_returns_cancelled() -> TestResult {
        if !git_on_path() {
            return Ok(());
        }
        let tmp = tempfile::tempdir()?;
        create_file(tmp.path(), "lib/One.pm")?;

        let outcome = super::try_git_discovery(
            tmp.path(),
            Instant::now(),
            &DiscoveryIncludeAllowlist::default(),
            &DiscoveryConfig::default(),
            &|| true,
        );

        assert!(
            matches!(outcome, Ok(super::GitDiscoveryOutcome::Cancelled)),
            "pre-spawn cancellation must return the Cancelled variant, got {outcome:?}"
        );

        Ok(())
    }

    /// Exact boundary variant for the child-wait checkpoint: cancelling
    /// after the spawn kills the git child and still returns Cancelled
    /// instead of blocking on the child or falling through to success.
    #[test]
    fn try_git_discovery_cancelled_during_child_wait_returns_cancelled() -> TestResult {
        if !git_on_path() {
            return Ok(());
        }
        let tmp = tempfile::tempdir()?;
        create_file(tmp.path(), "lib/One.pm")?;

        // First should_cancel() check (pre-spawn) passes; the next
        // checkpoint — the child wait loop — cancels.
        let checks = AtomicUsize::new(0);
        let outcome = super::try_git_discovery(
            tmp.path(),
            Instant::now(),
            &DiscoveryIncludeAllowlist::default(),
            &DiscoveryConfig::default(),
            &|| checks.fetch_add(1, Ordering::SeqCst) > 0,
        );

        assert!(
            matches!(outcome, Ok(super::GitDiscoveryOutcome::Cancelled)),
            "wait-loop cancellation must return the Cancelled variant, got {outcome:?}"
        );

        Ok(())
    }

    #[test]
    fn cancellable_discovery_stops_during_walk() -> TestResult {
        let tmp = tempfile::tempdir()?;
        for index in 0..256 {
            create_file(tmp.path(), &format!("lib/Module{index}.pm"))?;
        }
        let checks = AtomicUsize::new(0);
        let result = super::super::discovery::discover_perl_files_with_config_and_cancel(
            tmp.path(),
            &[] as &[&Path],
            &DiscoveryConfig::default(),
            || checks.fetch_add(1, Ordering::Relaxed) >= 3,
        );

        assert!(result.cancelled, "discovery should report cooperative cancellation");
        assert!(
            result.files.len() < 256,
            "cancelled discovery should not enumerate the complete workspace"
        );
        Ok(())
    }

    #[test]
    fn should_skip_dir_matches_conventional_noise_directories() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        fs::create_dir_all(root.join(".git"))?;
        fs::create_dir_all(root.join("node_modules"))?;
        fs::create_dir_all(root.join("src"))?;

        let mut seen_git = false;
        let mut seen_node_modules = false;
        let mut seen_src = false;

        for entry in walkdir::WalkDir::new(root).max_depth(1).into_iter().flatten() {
            if entry.path() == root {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            match name.as_ref() {
                ".git" => {
                    seen_git = true;
                    assert!(should_skip_dir(&entry));
                }
                "node_modules" => {
                    seen_node_modules = true;
                    assert!(should_skip_dir(&entry));
                }
                "src" => {
                    seen_src = true;
                    assert!(!should_skip_dir(&entry));
                }
                _ => {}
            }
        }

        assert!(seen_git);
        assert!(seen_node_modules);
        assert!(seen_src);

        Ok(())
    }

    // --- Additional coverage: parse_git_ls_files_output edge cases ---

    #[test]
    fn parse_git_output_empty_input_returns_nothing() {
        let root = Path::new("/tmp/workspace");
        let (files, excluded_count) = parse_git_ls_files_output(root, b"");
        assert_eq!(files.len(), 0);
        assert_eq!(excluded_count, 0);
    }

    #[test]
    fn parse_git_output_only_null_separators() {
        let root = Path::new("/tmp/workspace");
        let (files, excluded_count) = parse_git_ls_files_output(root, b"\0\0\0");
        assert_eq!(files.len(), 0);
        assert_eq!(excluded_count, 0);
    }

    #[test]
    fn parse_git_output_recognizes_all_perl_extensions() {
        let root = Path::new("/tmp/workspace");
        let payload =
            b"lib/Foo.pm\0scripts/run.pl\0t/basic.t\0app/main.psgi\0ext/native.xs\0templates/page.html.ep\0templates/page.tt\0templates/layout.tt2\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 8);
        assert!(files.iter().any(|p| p.ends_with("Foo.pm")));
        assert!(files.iter().any(|p| p.ends_with("run.pl")));
        assert!(files.iter().any(|p| p.ends_with("basic.t")));
        assert!(files.iter().any(|p| p.ends_with("main.psgi")));
        assert!(files.iter().any(|p| p.ends_with("native.xs")));
        assert!(files.iter().any(|p| p.ends_with("page.html.ep")));
        assert!(files.iter().any(|p| p.ends_with("page.tt")));
        assert!(files.iter().any(|p| p.ends_with("layout.tt2")));
        assert_eq!(excluded_count, 0);
    }

    #[test]
    fn parse_git_output_counts_non_perl_as_excluded() {
        let root = Path::new("/tmp/workspace");
        let payload = b"README.md\0Makefile\0config.yaml\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 0);
        assert_eq!(excluded_count, 3);
    }

    #[test]
    fn parse_git_output_excludes_all_skipped_directories() {
        let root = Path::new("/tmp/workspace");
        let payload = b".git/hooks/pre-commit.pl\0.hg/config.pm\0.svn/entries.pm\0target/out.pm\0node_modules/dep.pm\0.cache/fast.pm\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 0);
        assert_eq!(excluded_count, 6);
    }

    #[test]
    fn parse_git_output_allows_configured_local_lib_perl5_only() {
        let root = Path::new("/tmp/workspace");
        let include_paths = vec!["local/lib/perl5".to_string()];
        let allowlist = DiscoveryIncludeAllowlist::from_include_paths(
            root,
            &include_paths,
            &DiscoveryConfig::default(),
        );
        let payload = b"lib/Foo.pm\0local/lib/perl5/Remote/Module.pm\0local/Other.pm\0local/lib/perl5/.cache/Skipped.pm\0";

        let (files, excluded_count) = parse_git_ls_files_output_with_allowlist(
            root,
            payload,
            &allowlist,
            &DiscoveryConfig::default(),
        );

        assert_eq!(
            files,
            vec![root.join("lib/Foo.pm"), root.join("local/lib/perl5/Remote/Module.pm")]
        );
        assert_eq!(excluded_count, 2);
    }

    #[test]
    fn parse_git_output_joins_root_to_relative_paths() {
        let root = Path::new("/home/user/project");
        let payload = b"lib/Module.pm\0";
        let (files, _) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0], Path::new("/home/user/project/lib/Module.pm"));
    }

    #[test]
    fn parse_git_output_filters_stale_and_non_file_entries_for_existing_roots() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "lib/Current.pm")?;
        fs::create_dir_all(root.join("lib/Directory.pm"))?;

        let payload = b"lib/Current.pm\0lib/Deleted.pm\0lib/Directory.pm\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files, vec![root.join("lib/Current.pm")]);
        assert_eq!(excluded_count, 2);

        Ok(())
    }

    #[test]
    fn parse_git_output_excludes_parent_directory_components() {
        let root = Path::new("/tmp/workspace");
        let payload = b"../outside.pm\0lib/ok.pm\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files, vec![root.join("lib/ok.pm")]);
        assert_eq!(excluded_count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn parse_git_output_excludes_absolute_paths() {
        let root = Path::new("/tmp/workspace");
        // git ls-files should never emit absolute paths, but defend against
        // a corrupted or adversarial git output that attempts path escape.
        let payload = b"/etc/passwd\0lib/ok.pm\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files, vec![root.join("lib/ok.pm")]);
        assert_eq!(excluded_count, 1);
    }

    #[test]
    fn parse_git_output_excludes_embedded_parent_directory_traversal() {
        let root = Path::new("/tmp/workspace");
        // Embedded `..` must be rejected even when not at the start of the path.
        let payload = b"lib/../../etc/passwd\0lib/ok.pm\0";
        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files, vec![root.join("lib/ok.pm")]);
        assert_eq!(excluded_count, 1);
    }

    #[test]
    fn parse_git_output_deduplicates_duplicate_entries() {
        let root = Path::new("/tmp/workspace");
        let payload = b"lib/Foo.pm\0lib/Foo.pm\0script.pl\0script.pl\0README.md\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|p| p.ends_with("lib/Foo.pm")));
        assert!(files.iter().any(|p| p.ends_with("script.pl")));
        // Two duplicate Perl paths + one non-Perl file.
        assert_eq!(excluded_count, 3);
    }

    #[cfg(unix)]
    #[test]
    fn parse_git_output_handles_non_utf8_paths() {
        use std::os::unix::ffi::OsStrExt;

        let root = Path::new("/tmp/workspace");
        let payload = b"lib/\xFFfoo.pm\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(files.len(), 1);
        assert_eq!(excluded_count, 0);
        assert!(files[0].as_os_str().as_bytes().ends_with(b"lib/\xFFfoo.pm"));
    }

    // --- Additional coverage: path_contains_skipped_component ---

    #[test]
    fn skipped_component_detects_each_directory_individually() {
        let skipped = [".git", ".hg", ".svn", "target", "node_modules", ".cache"];
        for dir in skipped {
            let path_str = format!("lib/{dir}/nested.pm");
            assert!(
                path_contains_skipped_component(Path::new(&path_str)),
                "expected {dir} to be skipped"
            );
        }
    }

    #[test]
    fn skipped_component_allows_safe_directories() {
        let safe = ["lib", "src", "bin", "t", "scripts"];
        for dir in safe {
            let path_str = format!("{dir}/Module.pm");
            assert!(
                !path_contains_skipped_component(Path::new(&path_str)),
                "expected {dir} to be allowed"
            );
        }
    }

    #[test]
    fn skipped_component_rejects_blib_directory() {
        assert!(path_contains_skipped_component(Path::new("blib/Module.pm")));
    }

    #[test]
    fn skipped_component_empty_path_returns_false() {
        assert!(!path_contains_skipped_component(Path::new("")));
    }

    #[test]
    fn skipped_component_single_filename_returns_false() {
        assert!(!path_contains_skipped_component(Path::new("Module.pm")));
    }

    #[test]
    fn skipped_component_deeply_nested() {
        assert!(path_contains_skipped_component(Path::new("a/b/c/node_modules/d/e/f.pm")));
    }

    // --- Additional coverage: walk_discovery edge cases ---

    #[test]
    fn walk_discovery_empty_directory() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let result = walk_discovery(tmp.path(), Instant::now());

        assert_eq!(result.method, DiscoveryMethod::Walk);
        assert_eq!(result.files.len(), 0);
        assert_eq!(result.excluded_count, 0);

        Ok(())
    }

    #[test]
    fn walk_discovery_only_non_perl_files() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "README.md")?;
        create_file(root, "Makefile")?;
        create_file(root, "config.yaml")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.method, DiscoveryMethod::Walk);
        assert_eq!(result.files.len(), 0);
        assert_eq!(result.excluded_count, 3);

        Ok(())
    }

    #[test]
    fn walk_discovery_finds_all_perl_extensions() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "lib/Foo.pm")?;
        create_file(root, "bin/run.pl")?;
        create_file(root, "t/basic.t")?;
        create_file(root, "app/main.psgi")?;
        create_file(root, "xs/native.xs")?;
        create_file(root, "templates/page.html.ep")?;
        create_file(root, "templates/page.tt")?;
        create_file(root, "templates/layout.tt2")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.files.len(), 8);
        assert!(result.files.iter().any(|p| p.ends_with("page.html.ep")));
        assert!(result.files.iter().any(|p| p.ends_with("page.tt")));
        assert!(result.files.iter().any(|p| p.ends_with("layout.tt2")));

        Ok(())
    }

    #[test]
    fn walk_discovery_deeply_nested_perl_files() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "a/b/c/d/e/Deep.pm")?;
        create_file(root, "x/y/z/script.pl")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().any(|p| p.ends_with("Deep.pm")));
        assert!(result.files.iter().any(|p| p.ends_with("script.pl")));

        Ok(())
    }

    #[test]
    fn walk_discovery_skips_all_six_noise_directories() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, ".git/hooks/hook.pm")?;
        create_file(root, ".hg/config.pm")?;
        create_file(root, ".svn/entries.pm")?;
        create_file(root, "target/build/out.pm")?;
        create_file(root, "node_modules/dep.pm")?;
        create_file(root, ".cache/fast.pm")?;
        create_file(root, "lib/Visible.pm")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib/Visible.pm"));

        Ok(())
    }

    #[test]
    fn walk_discovery_records_duration() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let result = walk_discovery(tmp.path(), Instant::now());
        // Duration should be non-zero (or at least not panic)
        let _ = result.duration.as_nanos();

        Ok(())
    }

    #[test]
    fn walk_discovery_ignores_subdirectories_themselves() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        // Create a directory that looks like a .pm file (edge case)
        fs::create_dir_all(root.join("lib/Fake.pm/nested"))?;
        create_file(root, "lib/Real.pm")?;

        let result = walk_discovery(root, Instant::now());
        // Only the actual file should be found, not the directory
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib/Real.pm"));

        Ok(())
    }

    // --- Additional coverage: should_skip_dir for non-directory entries ---

    #[test]
    fn should_skip_dir_returns_false_for_files() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        // Create a file (not a directory)
        fs::write(root.join("target.txt"), "data")?;

        for entry in walkdir::WalkDir::new(root).max_depth(1).into_iter().flatten() {
            if entry.path() == root {
                continue;
            }
            if entry.file_type().is_file() {
                // Files should never be skipped by should_skip_dir
                assert!(!should_skip_dir(&entry));
            }
        }

        Ok(())
    }

    #[test]
    fn should_skip_dir_covers_all_six_directories() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        let dirs = [".git", ".hg", ".svn", "target", "node_modules", ".cache"];
        for d in dirs {
            fs::create_dir_all(root.join(d))?;
        }

        let mut matched = 0usize;
        for entry in walkdir::WalkDir::new(root).max_depth(1).into_iter().flatten() {
            if entry.path() == root {
                continue;
            }
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy();
                if dirs.contains(&name.as_ref()) {
                    assert!(should_skip_dir(&entry), "expected {name} to be skipped");
                    matched += 1;
                }
            }
        }

        assert_eq!(matched, dirs.len());
        Ok(())
    }

    // --- Additional coverage: DiscoveryMethod traits ---

    #[test]
    fn discovery_method_debug_and_equality() {
        let git = DiscoveryMethod::Git;
        let walk = DiscoveryMethod::Walk;
        let git2 = DiscoveryMethod::Git;

        assert_eq!(git, git2);
        assert_ne!(git, walk);
        // Debug is derivable, just verify it doesn't panic
        let _ = format!("{git:?}");
        let _ = format!("{walk:?}");
    }

    #[test]
    fn discovery_method_clone_and_copy() {
        let original = DiscoveryMethod::Git;
        let cloned = original;
        let copied = original;

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    // --- Additional coverage: DiscoveryResult ---

    #[test]
    fn discovery_result_clone_and_debug() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        create_file(root, "lib/Foo.pm")?;

        let result = walk_discovery(root, Instant::now());
        let cloned = result.clone();

        assert_eq!(cloned.files.len(), result.files.len());
        assert_eq!(cloned.method, result.method);
        assert_eq!(cloned.excluded_count, result.excluded_count);
        // Debug format should not panic
        let _ = format!("{result:?}");

        Ok(())
    }

    // --- Additional coverage: mixed Perl and non-Perl content ---

    #[test]
    fn walk_discovery_mixed_content_accurate_counts() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        // 3 Perl files
        create_file(root, "lib/A.pm")?;
        create_file(root, "bin/b.pl")?;
        create_file(root, "t/c.t")?;
        // 2 non-Perl files
        create_file(root, "README.md")?;
        create_file(root, "Makefile")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(result.files.len(), 3);
        assert_eq!(result.excluded_count, 2);

        Ok(())
    }

    #[test]
    fn parse_git_output_mixed_content_accurate_counts() {
        let root = Path::new("/tmp/workspace");
        let payload =
            b"lib/A.pm\0bin/b.pl\0t/c.t\0app/d.psgi\0README.md\0Makefile\0node_modules/e.pm\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);
        assert_eq!(files.len(), 4);
        // README.md + Makefile (non-perl) + node_modules/e.pm (skipped dir)
        assert_eq!(excluded_count, 3);
    }

    #[test]
    fn parse_git_output_sorts_paths_lexically_for_determinism() {
        let root = Path::new("/tmp/workspace");
        let payload = b"zeta/Z.pm\0alpha/A.pm\0mid/M.pm\0";

        let (files, excluded_count) = parse_git_ls_files_output(root, payload);

        assert_eq!(excluded_count, 0);
        assert_eq!(
            files,
            vec![root.join("alpha/A.pm"), root.join("mid/M.pm"), root.join("zeta/Z.pm"),]
        );
    }

    #[test]
    fn walk_discovery_sorts_paths_lexically_for_determinism() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();

        create_file(root, "zeta/Z.pm")?;
        create_file(root, "alpha/A.pm")?;
        create_file(root, "mid/M.pm")?;

        let result = walk_discovery(root, Instant::now());
        assert_eq!(
            result.files,
            vec![root.join("alpha/A.pm"), root.join("mid/M.pm"), root.join("zeta/Z.pm"),]
        );

        Ok(())
    }
}
