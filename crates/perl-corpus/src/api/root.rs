//! Explicit corpus-root selection and validation.
//!
//! The published crate contains APIs, schemas, concepts, and generators. The
//! repository corpus remains an external asset root. Load-bearing operations
//! must bind that root explicitly or through [`CORPUS_ROOT_ENV`]; bounded
//! workspace discovery exists only as a developer convenience.

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
pub(crate) static CORPUS_ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Environment variable used to select the external repository corpus root.
pub const CORPUS_ROOT_ENV: &str = "PERL_CORPUS_ROOT";

/// How a validated corpus root was selected.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusRootSource {
    /// The caller supplied the root directly.
    Explicit,
    /// The root came from [`CORPUS_ROOT_ENV`].
    Environment,
    /// The root was found from the crate's compile-time workspace location.
    WorkspaceDiscovery,
}

impl CorpusRootSource {
    /// Stable machine-readable source token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Environment => "environment",
            Self::WorkspaceDiscovery => "workspace_discovery",
        }
    }
}

/// A validated external corpus root and its selection authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusRoot {
    path: PathBuf,
    source: CorpusRootSource,
}

/// Failure to select or validate an external corpus root.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusRootError {
    /// A load-bearing operation received neither an explicit root nor an environment override.
    AuthoritativeRootRequired,
    /// A root path is relative and would therefore depend on the process working directory.
    RelativePath {
        /// Rejected root path.
        path: PathBuf,
    },
    /// A root or layer path contains a non-normal component or changed identity.
    InvalidPath {
        /// Rejected path.
        path: PathBuf,
        /// Stable reason token.
        reason: &'static str,
    },
    /// A selected root does not exist.
    RootMissing {
        /// Missing path or path component.
        path: PathBuf,
    },
    /// A selected root or one of its path components is a symlink.
    SymlinkUnsupported {
        /// Rejected path component.
        path: PathBuf,
    },
    /// A selected root exists but is not a directory.
    RootNotDirectory {
        /// Rejected path.
        path: PathBuf,
    },
    /// Developer workspace discovery could not find a workspace manifest.
    WorkspaceNotFound {
        /// Compile-time crate directory used as the discovery start.
        start: PathBuf,
    },
    /// A candidate Cargo manifest could not be parsed while locating the workspace.
    WorkspaceManifestInvalid {
        /// Invalid Cargo manifest path.
        path: PathBuf,
        /// Rendered TOML error.
        message: String,
    },
    /// A required repository corpus layer is absent.
    RequiredLayerMissing {
        /// Stable layer token.
        layer: &'static str,
        /// Expected layer path.
        path: PathBuf,
    },
    /// A required repository corpus layer crosses a symlink.
    RequiredLayerSymlink {
        /// Stable layer token.
        layer: &'static str,
        /// Rejected path component.
        path: PathBuf,
    },
    /// A required repository corpus layer exists but is not a directory.
    RequiredLayerNotDirectory {
        /// Stable layer token.
        layer: &'static str,
        /// Rejected path.
        path: PathBuf,
    },
    /// A required repository corpus layer exists but cannot be enumerated reliably.
    RequiredLayerUnreadable {
        /// Stable layer token.
        layer: &'static str,
        /// Unreadable layer path.
        path: PathBuf,
        /// Rendered operating-system error.
        message: String,
    },
    /// Filesystem inspection failed.
    Io {
        /// Path being inspected.
        path: PathBuf,
        /// Rendered operating-system error.
        message: String,
    },
}

impl fmt::Display for CorpusRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthoritativeRootRequired => write!(
                formatter,
                "an authoritative corpus root is required; provide one explicitly or set {CORPUS_ROOT_ENV}"
            ),
            Self::RelativePath { path } => write!(
                formatter,
                "corpus root must be absolute so it is independent of the current directory: {}",
                path.display()
            ),
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid corpus path {}: {reason}", path.display())
            }
            Self::RootMissing { path } => {
                write!(formatter, "corpus root does not exist: {}", path.display())
            }
            Self::SymlinkUnsupported { path } => write!(
                formatter,
                "corpus root path cannot cross a symlink: {}",
                path.display()
            ),
            Self::RootNotDirectory { path } => {
                write!(formatter, "corpus root is not a directory: {}", path.display())
            }
            Self::WorkspaceNotFound { start } => write!(
                formatter,
                "could not discover a Cargo workspace above {}",
                start.display()
            ),
            Self::WorkspaceManifestInvalid { path, message } => write!(
                formatter,
                "could not parse Cargo workspace candidate {}: {message}",
                path.display()
            ),
            Self::RequiredLayerMissing { layer, path } => write!(
                formatter,
                "required corpus layer {layer} is missing at {}",
                path.display()
            ),
            Self::RequiredLayerSymlink { layer, path } => write!(
                formatter,
                "required corpus layer {layer} crosses a symlink at {}",
                path.display()
            ),
            Self::RequiredLayerNotDirectory { layer, path } => write!(
                formatter,
                "required corpus layer {layer} is not a directory: {}",
                path.display()
            ),
            Self::RequiredLayerUnreadable { layer, path, message } => write!(
                formatter,
                "required corpus layer {layer} cannot be enumerated at {}: {message}",
                path.display()
            ),
            Self::Io { path, message } => write!(
                formatter,
                "failed to inspect corpus path {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CorpusRootError {}

impl CorpusRoot {
    /// Validate a caller-supplied absolute corpus root.
    pub fn explicit(path: impl AsRef<Path>) -> Result<Self, CorpusRootError> {
        Self::from_path(path.as_ref(), CorpusRootSource::Explicit)
    }

    /// Resolve a load-bearing root using explicit input, then [`CORPUS_ROOT_ENV`].
    ///
    /// Workspace discovery is deliberately excluded from this path so evidence
    /// cannot depend on whichever directory happened to launch the process.
    pub fn resolve_authoritative(explicit: Option<&Path>) -> Result<Self, CorpusRootError> {
        if let Some(path) = explicit {
            return Self::from_path(path, CorpusRootSource::Explicit);
        }
        if let Some(path) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_path(Path::new(&path), CorpusRootSource::Environment);
        }
        Err(CorpusRootError::AuthoritativeRootRequired)
    }

    /// Resolve a developer root using explicit input, environment, then bounded workspace discovery.
    pub fn resolve_for_development(explicit: Option<&Path>) -> Result<Self, CorpusRootError> {
        if let Some(path) = explicit {
            return Self::from_path(path, CorpusRootSource::Explicit);
        }
        if let Some(path) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_path(Path::new(&path), CorpusRootSource::Environment);
        }
        let workspace = find_workspace_root()?;
        Self::from_path(&workspace, CorpusRootSource::WorkspaceDiscovery)
    }

    /// Return the validated absolute root path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return how this root was selected.
    #[must_use]
    pub const fn source(&self) -> CorpusRootSource {
        self.source
    }

    /// Consume the authority and return its absolute root path.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    pub(crate) fn from_path(path: &Path, source: CorpusRootSource) -> Result<Self, CorpusRootError> {
        let path = validate_absolute_directory(path)?;
        Ok(Self { path, source })
    }

    pub(crate) fn require_directory(
        &self,
        relative: &Path,
        layer: &'static str,
    ) -> Result<PathBuf, CorpusRootError> {
        self.require_directory_with_probe(relative, layer, probe_directory_readable)
    }

    fn require_directory_with_probe<F>(
        &self,
        relative: &Path,
        layer: &'static str,
        mut probe: F,
    ) -> Result<PathBuf, CorpusRootError>
    where
        F: FnMut(&Path) -> io::Result<()>,
    {
        if relative.as_os_str().is_empty() || relative.is_absolute() {
            return Err(CorpusRootError::InvalidPath {
                path: relative.to_path_buf(),
                reason: "required_layer_path_must_be_nonempty_and_relative",
            });
        }

        revalidate_bound_root(&self.path)?;
        let mut current = self.path.clone();
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            let Component::Normal(value) = component else {
                return Err(CorpusRootError::InvalidPath {
                    path: relative.to_path_buf(),
                    reason: "required_layer_path_must_use_normal_components",
                });
            };
            current.push(value);
            let is_final = components.peek().is_none();
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(CorpusRootError::RequiredLayerMissing { layer, path: current });
                }
                Err(error) => {
                    return Err(CorpusRootError::Io {
                        path: current,
                        message: error.to_string(),
                    });
                }
            };
            if metadata.file_type().is_symlink() {
                return Err(CorpusRootError::RequiredLayerSymlink { layer, path: current });
            }
            if !metadata.is_dir() {
                return Err(CorpusRootError::RequiredLayerNotDirectory { layer, path: current });
            }
            if is_final {
                probe(&current).map_err(|error| CorpusRootError::RequiredLayerUnreadable {
                    layer,
                    path: current.clone(),
                    message: error.to_string(),
                })?;
                revalidate_bound_root(&self.path)?;
                return Ok(current);
            }
        }

        Err(CorpusRootError::InvalidPath {
            path: relative.to_path_buf(),
            reason: "required_layer_path_had_no_components",
        })
    }
}

fn probe_directory_readable(path: &Path) -> io::Result<()> {
    let entries = fs::read_dir(path)?;
    for entry in entries {
        entry?;
    }
    Ok(())
}

fn revalidate_bound_root(bound: &Path) -> Result<(), CorpusRootError> {
    let current = validate_absolute_directory(bound)?;
    if current != bound {
        return Err(CorpusRootError::InvalidPath {
            path: bound.to_path_buf(),
            reason: "bound_root_identity_changed",
        });
    }
    Ok(())
}

fn validate_absolute_directory(path: &Path) -> Result<PathBuf, CorpusRootError> {
    if !path.is_absolute() {
        return Err(CorpusRootError::RelativePath { path: path.to_path_buf() });
    }
    validate_directory_components(path)?;
    let canonical = fs::canonicalize(path).map_err(|error| CorpusRootError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    validate_directory_components(&canonical)?;
    Ok(canonical)
}

fn validate_directory_components(path: &Path) -> Result<(), CorpusRootError> {
    let mut current = PathBuf::new();
    let mut saw_normal_component = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                saw_normal_component = true;
                current.push(value);
                let metadata = match fs::symlink_metadata(&current) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Err(CorpusRootError::RootMissing { path: current });
                    }
                    Err(error) => {
                        return Err(CorpusRootError::Io {
                            path: current,
                            message: error.to_string(),
                        });
                    }
                };
                if metadata.file_type().is_symlink() {
                    return Err(CorpusRootError::SymlinkUnsupported { path: current });
                }
                if !metadata.is_dir() {
                    return Err(CorpusRootError::RootNotDirectory { path: current });
                }
            }
            Component::CurDir | Component::ParentDir => {
                return Err(CorpusRootError::InvalidPath {
                    path: path.to_path_buf(),
                    reason: "root_path_must_not_contain_dot_components",
                });
            }
        }
    }

    if !saw_normal_component {
        let metadata = fs::symlink_metadata(path).map_err(|error| CorpusRootError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        if metadata.file_type().is_symlink() {
            return Err(CorpusRootError::SymlinkUnsupported { path: path.to_path_buf() });
        }
        if !metadata.is_dir() {
            return Err(CorpusRootError::RootNotDirectory { path: path.to_path_buf() });
        }
    }
    Ok(())
}

fn find_workspace_root() -> Result<PathBuf, CorpusRootError> {
    find_workspace_root_from(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn find_workspace_root_from(start: &Path) -> Result<PathBuf, CorpusRootError> {
    for ancestor in start.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        let metadata = match fs::symlink_metadata(&manifest) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(CorpusRootError::Io {
                    path: manifest,
                    message: error.to_string(),
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&manifest).map_err(|error| CorpusRootError::Io {
            path: manifest.clone(),
            message: error.to_string(),
        })?;
        let parsed = toml::from_str::<toml::Value>(&contents).map_err(|error| {
            CorpusRootError::WorkspaceManifestInvalid {
                path: manifest.clone(),
                message: error.to_string(),
            }
        })?;
        if parsed.get("workspace").and_then(toml::Value::as_table).is_some() {
            return Ok(ancestor.to_path_buf());
        }
    }
    Err(CorpusRootError::WorkspaceNotFound { start: start.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvVarGuard {
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(path: &Path) -> Self {
            let previous = env::var_os(CORPUS_ROOT_ENV);
            // SAFETY: The test holds CORPUS_ENV_TEST_LOCK and restores the value on drop.
            unsafe { env::set_var(CORPUS_ROOT_ENV, path) };
            Self { previous }
        }

        fn unset() -> Self {
            let previous = env::var_os(CORPUS_ROOT_ENV);
            // SAFETY: The test holds CORPUS_ENV_TEST_LOCK and restores the value on drop.
            unsafe { env::remove_var(CORPUS_ROOT_ENV) };
            Self { previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => {
                    // SAFETY: This restores the value captured while the lock is held.
                    unsafe { env::set_var(CORPUS_ROOT_ENV, value) };
                }
                None => {
                    // SAFETY: This restores the previously absent state while the lock is held.
                    unsafe { env::remove_var(CORPUS_ROOT_ENV) };
                }
            }
        }
    }

    #[test]
    fn authoritative_resolution_requires_explicit_or_environment_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let _lock = CORPUS_ENV_TEST_LOCK
            .lock()
            .map_err(|_| io::Error::other("environment lock poisoned"))?;
        let _guard = EnvVarGuard::unset();
        assert_eq!(
            CorpusRoot::resolve_authoritative(None),
            Err(CorpusRootError::AuthoritativeRootRequired)
        );
        Ok(())
    }

    #[test]
    fn explicit_root_precedes_environment_root() -> Result<(), Box<dyn std::error::Error>> {
        let _lock = CORPUS_ENV_TEST_LOCK
            .lock()
            .map_err(|_| io::Error::other("environment lock poisoned"))?;
        let explicit = tempfile::tempdir()?;
        let environment = tempfile::tempdir()?;
        let _guard = EnvVarGuard::set(environment.path());
        let root = CorpusRoot::resolve_authoritative(Some(explicit.path()))?;
        assert_eq!(root.path(), explicit.path().canonicalize()?);
        assert_eq!(root.source(), CorpusRootSource::Explicit);
        Ok(())
    }

    #[test]
    fn environment_root_is_recorded() -> Result<(), Box<dyn std::error::Error>> {
        let _lock = CORPUS_ENV_TEST_LOCK
            .lock()
            .map_err(|_| io::Error::other("environment lock poisoned"))?;
        let environment = tempfile::tempdir()?;
        let _guard = EnvVarGuard::set(environment.path());
        let root = CorpusRoot::resolve_authoritative(None)?;
        assert_eq!(root.path(), environment.path().canonicalize()?);
        assert_eq!(root.source(), CorpusRootSource::Environment);
        Ok(())
    }

    #[test]
    fn relative_root_is_rejected() {
        assert!(matches!(
            CorpusRoot::explicit(Path::new("relative/root")),
            Err(CorpusRootError::RelativePath { .. })
        ));
    }

    #[test]
    fn missing_root_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let parent = tempfile::tempdir()?;
        let missing = parent.path().join("missing");
        assert!(matches!(
            CorpusRoot::explicit(&missing),
            Err(CorpusRootError::RootMissing { .. })
        ));
        Ok(())
    }

    #[test]
    fn file_root_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            CorpusRoot::explicit(file.path()),
            Err(CorpusRootError::RootNotDirectory { .. })
        ));
        Ok(())
    }

    #[test]
    fn required_layer_probe_failure_is_not_green() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let layer = root.path().join("test_corpus");
        fs::create_dir(&layer)?;
        let authority = CorpusRoot::explicit(root.path())?;
        let result = authority.require_directory_with_probe(
            Path::new("test_corpus"),
            "test_corpus",
            |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        );
        assert!(matches!(
            result,
            Err(CorpusRootError::RequiredLayerUnreadable {
                layer: "test_corpus",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn workspace_discovery_parses_the_workspace_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let nested = root.path().join("crates/example/src");
        fs::create_dir_all(&nested)?;
        fs::write(root.path().join("Cargo.toml"), "[workspace] # inline comment\nmembers = []\n")?;
        assert_eq!(find_workspace_root_from(&nested)?, root.path());
        Ok(())
    }

    #[test]
    fn workspace_text_inside_a_value_is_not_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let nested = root.path().join("crates/example");
        fs::create_dir_all(&nested)?;
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = 'example'\nversion = '0.0.0'\ndescription = '''\n[workspace]\n'''\n",
        )?;
        assert!(matches!(
            find_workspace_root_from(&nested),
            Err(CorpusRootError::WorkspaceNotFound { .. })
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_root_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let parent = tempfile::tempdir()?;
        let target = parent.path().join("target");
        let link = parent.path().join("link");
        fs::create_dir(&target)?;
        symlink(&target, &link)?;
        assert!(matches!(
            CorpusRoot::explicit(&link),
            Err(CorpusRootError::SymlinkUnsupported { .. })
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replaced_root_symlink_is_rejected_before_layer_resolution()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let parent = tempfile::tempdir()?;
        let root_path = parent.path().join("root");
        let moved_path = parent.path().join("moved");
        let outside_path = parent.path().join("outside");
        fs::create_dir_all(root_path.join("test_corpus"))?;
        fs::create_dir_all(&outside_path)?;
        let authority = CorpusRoot::explicit(&root_path)?;
        fs::rename(&root_path, &moved_path)?;
        symlink(&outside_path, &root_path)?;
        assert!(matches!(
            authority.require_directory(Path::new("test_corpus"), "test_corpus"),
            Err(CorpusRootError::SymlinkUnsupported { .. })
        ));
        Ok(())
    }
}
