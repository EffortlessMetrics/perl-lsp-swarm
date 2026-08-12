//! Corpus file discovery helpers.

use crate::api::root::{CorpusRoot, CorpusRootError, CorpusRootSource};
pub use crate::api::root::CORPUS_ROOT_ENV;
use std::env;
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

const TEST_EXTENSIONS: &[&str] = &["pl", "pm", "plx", "t", "psgi", "cgi"];

/// Common corpus paths anchored at a root directory.
///
/// The three public fields are preserved for compatibility with downstream
/// struct literals and destructuring. Selection provenance lives in
/// [`ResolvedCorpusPaths`] instead of changing this published shape.
#[derive(Debug, Clone)]
pub struct CorpusPaths {
    /// Workspace or external asset root used for discovery.
    pub root: PathBuf,
    /// Directory containing gap coverage corpus files.
    pub test_corpus: PathBuf,
    /// Directory containing fuzz regression fixtures.
    pub fuzz: PathBuf,
}

/// Validated corpus paths plus the authority that selected them.
#[derive(Debug, Clone)]
pub struct ResolvedCorpusPaths {
    /// Validated path set. Deref also exposes `CorpusPaths` methods and fields.
    pub paths: CorpusPaths,
    source: CorpusRootSource,
}

impl ResolvedCorpusPaths {
    /// Return how this validated root was selected.
    #[must_use]
    pub const fn root_source(&self) -> CorpusRootSource {
        self.source
    }

    /// Consume the validated wrapper and return the compatibility path set.
    #[must_use]
    pub fn into_paths(self) -> CorpusPaths {
        self.paths
    }
}

impl Deref for ResolvedCorpusPaths {
    type Target = CorpusPaths;

    fn deref(&self) -> &Self::Target {
        &self.paths
    }
}

/// Corpus layers managed by perl-corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusLayer {
    /// Gap coverage test corpus files.
    TestCorpus,
    /// Fuzz regression fixtures.
    Fuzz,
}

/// Corpus file with its originating layer.
#[derive(Debug, Clone)]
pub struct CorpusFile {
    /// Path to the corpus file.
    pub path: PathBuf,
    /// Layer classification for the file.
    pub layer: CorpusLayer,
}

impl CorpusPaths {
    /// Discover corpus paths through the legacy developer-convenience contract.
    ///
    /// This non-fallible compatibility path does not preserve provenance or
    /// validate the selected path. Use [`Self::try_discover`] for validated
    /// developer discovery and [`Self::resolve_authoritative`] for load-bearing
    /// work.
    pub fn discover() -> Self {
        if let Some(root) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_root(PathBuf::from(root));
        }

        match CorpusRoot::resolve_for_development(None) {
            Ok(authority) => Self::from_root(authority.into_path()),
            Err(_) => Self::from_root(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        }
    }

    /// Discover and validate corpus paths for developer use.
    pub fn try_discover() -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::resolve_for_development(None).map(resolved_from_authority)
    }

    /// Resolve and validate paths for a load-bearing operation.
    ///
    /// The explicit root takes precedence over [`CORPUS_ROOT_ENV`]. Workspace
    /// discovery is intentionally unavailable on this path.
    pub fn resolve_authoritative(
        explicit: Option<&Path>,
    ) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::resolve_authoritative(explicit).map(resolved_from_authority)
    }

    /// Validate an explicit root and build its common corpus paths.
    pub fn try_from_root(
        root: impl AsRef<Path>,
    ) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::explicit(root).map(resolved_from_authority)
    }

    /// Build corpus paths from an unchecked root.
    ///
    /// This compatibility constructor preserves the pre-existing public shape
    /// for synthetic tests and callers that perform their own validation.
    /// Load-bearing operations should use [`Self::try_from_root`] or
    /// [`Self::resolve_authoritative`].
    #[must_use]
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            test_corpus: root.join("test_corpus"),
            fuzz: root.join("crates/perl-corpus/fuzz"),
            root,
        }
    }

    /// Require the checked-in repository layers owned by the current topology.
    ///
    /// Missing, linked, non-directory, unreadable, or recursively unreadable
    /// layers fail instead of becoming an empty or partial successful corpus.
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        let authority = CorpusRoot::explicit(&self.root)?;
        let test_corpus =
            authority.require_directory(Path::new("test_corpus"), "test_corpus")?;
        validate_readable_tree(&test_corpus, "test_corpus")?;
        authority.require_directory(Path::new("test_corpus"), "test_corpus")?;

        let fuzz = authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        validate_readable_tree(&fuzz, "fuzz")?;
        authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        Ok(())
    }
}

fn resolved_from_authority(authority: CorpusRoot) -> ResolvedCorpusPaths {
    let source = authority.source();
    let paths = CorpusPaths::from_root(authority.into_path());
    ResolvedCorpusPaths { paths, source }
}

fn validate_readable_tree(root: &Path, layer: &'static str) -> Result<(), CorpusRootError> {
    validate_readable_tree_with_probe(root, layer, |_| Ok(()))
}

fn validate_readable_tree_with_probe<F>(
    root: &Path,
    layer: &'static str,
    mut probe: F,
) -> Result<(), CorpusRootError>
where
    F: FnMut(&Path) -> io::Result<()>,
{
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        validate_tree_directory(&directory, layer)?;
        probe(&directory).map_err(|error| CorpusRootError::RequiredLayerUnreadable {
            layer,
            path: directory.clone(),
            message: error.to_string(),
        })?;

        let entries = fs::read_dir(&directory).map_err(|error| {
            CorpusRootError::RequiredLayerUnreadable {
                layer,
                path: directory.clone(),
                message: error.to_string(),
            }
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| CorpusRootError::RequiredLayerUnreadable {
                layer,
                path: directory.clone(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            let file_type =
                entry
                    .file_type()
                    .map_err(|error| CorpusRootError::RequiredLayerUnreadable {
                        layer,
                        path: path.clone(),
                        message: error.to_string(),
                    })?;
            if file_type.is_symlink() {
                return Err(CorpusRootError::RequiredLayerSymlink { layer, path });
            }
            if file_type.is_dir() {
                stack.push(path);
            }
        }
        validate_tree_directory(&directory, layer)?;
    }
    Ok(())
}

fn validate_tree_directory(path: &Path, layer: &'static str) -> Result<(), CorpusRootError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CorpusRootError::RequiredLayerMissing {
                layer,
                path: path.to_path_buf(),
            }
        } else {
            CorpusRootError::RequiredLayerUnreadable {
                layer,
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CorpusRootError::RequiredLayerSymlink {
            layer,
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_dir() {
        return Err(CorpusRootError::RequiredLayerNotDirectory {
            layer,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Return test corpus files (gap coverage fixtures).
pub fn get_test_files() -> Vec<PathBuf> {
    get_test_files_from(&CorpusPaths::discover())
}

/// Return test corpus files using a specific root.
pub fn get_test_files_from(paths: &CorpusPaths) -> Vec<PathBuf> {
    collect_files(&paths.test_corpus, TEST_EXTENSIONS)
}

/// Return fuzz regression fixtures (Perl sources only).
pub fn get_fuzz_files() -> Vec<PathBuf> {
    get_fuzz_files_from(&CorpusPaths::discover())
}

/// Return fuzz regression fixtures from an explicit root.
pub fn get_fuzz_files_from(paths: &CorpusPaths) -> Vec<PathBuf> {
    collect_files(&paths.fuzz, &["pl"])
}

/// Return corpus files with their layer annotations.
pub fn get_corpus_files() -> Vec<CorpusFile> {
    get_corpus_files_from(&CorpusPaths::discover())
}

/// Return corpus files with layers from an explicit root.
pub fn get_corpus_files_from(paths: &CorpusPaths) -> Vec<CorpusFile> {
    let mut files: Vec<CorpusFile> = get_test_files_from(paths)
        .into_iter()
        .map(|path| CorpusFile {
            path,
            layer: CorpusLayer::TestCorpus,
        })
        .collect();

    files.extend(
        get_fuzz_files_from(paths)
            .into_iter()
            .map(|path| CorpusFile {
                path,
                layer: CorpusLayer::Fuzz,
            }),
    );

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    files
}

/// Return all available Perl sources across corpus layers.
pub fn get_all_test_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = get_corpus_files()
        .into_iter()
        .map(|file| file.path)
        .collect();
    files.sort();
    files.dedup();
    files
}

fn collect_files(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !root.exists() {
        return files;
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();

            if file_name.starts_with('.') || file_name.starts_with('_') {
                continue;
            }

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if file_type.is_file() && has_allowed_extension(&path, extensions) {
                files.push(path);
            }
        }
    }

    files.sort();
    files.dedup();
    files
}

fn has_allowed_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            extensions
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> io::Result<PathBuf> {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("{}_{}_{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = env::var_os(key);
            // SAFETY: Tests in this module set process environment variables in a
            // controlled way and restore them on drop.
            unsafe { env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => {
                    // SAFETY: This restores the original value captured by the
                    // guard when it was created.
                    unsafe { env::set_var(self.key, previous) };
                }
                None => {
                    // SAFETY: The guard created this variable and now removes it.
                    unsafe { env::remove_var(self.key) };
                }
            }
        }
    }

    #[test]
    fn public_corpus_paths_struct_literal_remains_valid()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_public_shape")?;
        let paths = CorpusPaths {
            test_corpus: root.join("test_corpus"),
            fuzz: root.join("crates/perl-corpus/fuzz"),
            root: root.clone(),
        };
        assert_eq!(paths.root, root);
        fs::remove_dir_all(&paths.root)?;
        Ok(())
    }

    #[test]
    fn collect_files_filters_extensions_and_skips_hidden()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_files")?;
        let keep_dir = root.join("keep");
        fs::create_dir_all(&keep_dir)?;
        fs::create_dir_all(root.join("_skip"))?;
        fs::create_dir_all(root.join(".hidden_dir"))?;
        let fixtures = [
            root.join("case.pl"),
            root.join("case.pm"),
            root.join("case.plx"),
            root.join("case.t"),
            root.join("case.psgi"),
            root.join("case.cgi"),
            keep_dir.join("nested.pl"),
        ];
        for fixture in &fixtures {
            fs::write(fixture, "print 1;\n")?;
        }
        fs::write(root.join("case.txt"), "ignore\n")?;
        fs::write(root.join(".hidden.pl"), "ignore\n")?;
        fs::write(root.join("_skip/inner.pl"), "ignore\n")?;
        fs::write(root.join(".hidden_dir/inner.pm"), "ignore\n")?;
        let files = collect_files(&root, TEST_EXTENSIONS);
        let mut names: Vec<_> = files
            .iter()
            .map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "case.cgi",
                "case.pl",
                "case.plx",
                "case.pm",
                "case.psgi",
                "case.t",
                "nested.pl",
            ]
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn corpus_paths_try_discover_records_environment_provenance()
    -> Result<(), Box<dyn std::error::Error>> {
        let _lock = crate::api::root::CORPUS_ENV_TEST_LOCK
            .lock()
            .map_err(|_| io::Error::other("environment lock poisoned"))?;
        let root = temp_root("perl_corpus_validated_env_root")?;
        let _env_guard = EnvVarGuard::set(CORPUS_ROOT_ENV, &root);
        let discovered = CorpusPaths::try_discover()?;
        assert_eq!(discovered.root, root.canonicalize()?);
        assert_eq!(discovered.root_source(), CorpusRootSource::Environment);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn authoritative_paths_require_repository_layers()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_required_layers")?;
        let resolved = CorpusPaths::try_from_root(&root)?;
        assert!(matches!(
            resolved.require_repository_layout(),
            Err(CorpusRootError::RequiredLayerMissing {
                layer: "test_corpus",
                ..
            })
        ));
        fs::create_dir_all(root.join("test_corpus"))?;
        assert!(matches!(
            resolved.require_repository_layout(),
            Err(CorpusRootError::RequiredLayerMissing { layer: "fuzz", .. })
        ));
        fs::create_dir_all(root.join("crates/perl-corpus/fuzz"))?;
        resolved.require_repository_layout()?;
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn nested_probe_failure_is_not_green() -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_nested_probe")?;
        let nested = root.join("test_corpus/nested");
        fs::create_dir_all(&nested)?;
        let result = validate_readable_tree_with_probe(
            &root.join("test_corpus"),
            "test_corpus",
            |path| {
                if path == nested {
                    Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(
            result,
            Err(CorpusRootError::RequiredLayerUnreadable {
                layer: "test_corpus",
                path,
                ..
            }) if path == nested
        ));
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn nested_symlink_is_not_a_valid_readable_tree()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let root = temp_root("perl_corpus_nested_symlink")?;
        let tree = root.join("test_corpus");
        let outside = root.join("outside");
        fs::create_dir_all(&tree)?;
        fs::create_dir_all(&outside)?;
        let link = tree.join("linked");
        symlink(&outside, &link)?;
        assert!(matches!(
            validate_readable_tree(&tree, "test_corpus"),
            Err(CorpusRootError::RequiredLayerSymlink {
                layer: "test_corpus",
                path,
            }) if path == link
        ));
        fs::remove_dir_all(&root)?;
        Ok(())
    }
}
