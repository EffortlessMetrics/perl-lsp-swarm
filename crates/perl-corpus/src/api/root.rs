//! Strict external corpus-root authority.
//!
//! The checked-in corpus is an external runtime asset. [`CorpusRoot`] binds an
//! absolute root path to a retained open directory capability so later
//! load-bearing work does not have to trust or reopen the ambient pathname.
//! Historical workspace discovery remains a separate compatibility concern in
//! [`crate::files::CorpusPaths`].

use same_file::Handle;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

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
    /// The root came from fallible compile-time workspace discovery.
    WorkspaceDiscovery,
}

impl CorpusRootSource {
    /// Return the stable machine-readable source token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Environment => "environment",
            Self::WorkspaceDiscovery => "workspace_discovery",
        }
    }
}

/// A validated external corpus root with retained directory authority.
///
/// The canonical path is retained for bounded diagnostics and identity
/// comparison. The open directory handle is the authority carried across
/// clones; cloning this type never reopens the pathname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusRoot {
    path: PathBuf,
    source: CorpusRootSource,
    directory: Arc<Handle>,
}

/// Failure to select, bind, or validate an external corpus root.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusRootError {
    /// Neither an explicit root nor [`CORPUS_ROOT_ENV`] selected a root.
    AuthoritativeRootRequired,
    /// The selected root is relative and therefore current-directory dependent.
    RelativePath {
        /// Rejected root path.
        path: PathBuf,
    },
    /// A root or required-layer path contains an unsupported component.
    InvalidPath {
        /// Rejected path.
        path: PathBuf,
        /// Stable reason token.
        reason: &'static str,
    },
    /// The selected root or one of its components does not exist.
    RootMissing {
        /// Missing path or path component.
        path: PathBuf,
    },
    /// The selected root crosses a symbolic link or Windows reparse point.
    SymlinkOrReparseUnsupported {
        /// Rejected path component.
        path: PathBuf,
    },
    /// The selected root exists but is not a directory.
    RootNotDirectory {
        /// Rejected path.
        path: PathBuf,
    },
    /// The selected root could not be inspected or opened.
    RootUnreadable {
        /// Rejected path.
        path: PathBuf,
        /// Rendered operating-system error.
        message: String,
    },
    /// This platform could not bind the selected directory as a retained capability.
    CapabilityUnavailable {
        /// Selected root path.
        path: PathBuf,
        /// Rendered capability error.
        message: String,
    },
    /// The root pathname no longer names the retained directory capability.
    RootIdentityChanged {
        /// Bound canonical root path.
        path: PathBuf,
    },
    /// Fallible developer discovery could not find a Cargo workspace.
    WorkspaceNotFound {
        /// Compile-time crate directory used as the search start.
        start: PathBuf,
    },
    /// A candidate workspace manifest could not be parsed.
    WorkspaceManifestInvalid {
        /// Invalid manifest path.
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
    /// A required repository corpus layer crosses a link or reparse point.
    RequiredLayerSymlinkOrReparse {
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
    /// A required repository corpus layer cannot be enumerated.
    RequiredLayerUnreadable {
        /// Stable layer token.
        layer: &'static str,
        /// Unreadable layer path.
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
                "corpus root must be absolute and current-directory independent: {}",
                path.display()
            ),
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid corpus path {}: {reason}", path.display())
            }
            Self::RootMissing { path } => {
                write!(formatter, "corpus root does not exist: {}", path.display())
            }
            Self::SymlinkOrReparseUnsupported { path } => write!(
                formatter,
                "corpus root cannot cross a symbolic link or reparse point: {}",
                path.display()
            ),
            Self::RootNotDirectory { path } => {
                write!(formatter, "corpus root is not a directory: {}", path.display())
            }
            Self::RootUnreadable { path, message } => write!(
                formatter,
                "corpus root cannot be inspected at {}: {message}",
                path.display()
            ),
            Self::CapabilityUnavailable { path, message } => write!(
                formatter,
                "corpus root capability cannot be bound at {}: {message}",
                path.display()
            ),
            Self::RootIdentityChanged { path } => write!(
                formatter,
                "corpus root pathname no longer names the retained directory: {}",
                path.display()
            ),
            Self::WorkspaceNotFound { start } => {
                write!(formatter, "could not discover a Cargo workspace above {}", start.display())
            }
            Self::WorkspaceManifestInvalid { path, message } => write!(
                formatter,
                "could not parse Cargo workspace candidate {}: {message}",
                path.display()
            ),
            Self::RequiredLayerMissing { layer, path } => {
                write!(formatter, "required corpus layer {layer} is missing at {}", path.display())
            }
            Self::RequiredLayerSymlinkOrReparse { layer, path } => write!(
                formatter,
                "required corpus layer {layer} crosses a symbolic link or reparse point at {}",
                path.display()
            ),
            Self::RequiredLayerNotDirectory { layer, path } => write!(
                formatter,
                "required corpus layer {layer} is not a directory at {}",
                path.display()
            ),
            Self::RequiredLayerUnreadable { layer, path, message } => write!(
                formatter,
                "required corpus layer {layer} cannot be enumerated at {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CorpusRootError {}

impl CorpusRoot {
    /// Validate and bind a caller-supplied absolute corpus root.
    pub fn explicit(path: impl AsRef<Path>) -> Result<Self, CorpusRootError> {
        Self::bind(path.as_ref(), CorpusRootSource::Explicit)
    }

    /// Resolve a load-bearing root from explicit input, then [`CORPUS_ROOT_ENV`].
    ///
    /// An invalid explicit root fails immediately and never falls through to the
    /// environment. Workspace discovery is deliberately excluded.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, CorpusRootError> {
        Self::resolve_authoritative(explicit)
    }

    /// Resolve a load-bearing root from explicit input, then [`CORPUS_ROOT_ENV`].
    ///
    /// This name makes the authority boundary explicit at call sites that also
    /// use compatibility discovery.
    pub fn resolve_authoritative(explicit: Option<&Path>) -> Result<Self, CorpusRootError> {
        resolve_authoritative_from(explicit, env::var_os(CORPUS_ROOT_ENV).as_deref())
    }

    /// Discover the compile-time workspace and bind it as strict authority.
    ///
    /// This is a fallible developer migration route, not a fallback used by
    /// [`Self::resolve_authoritative`] or legacy [`crate::files::CorpusPaths::discover`].
    pub fn try_discover() -> Result<Self, CorpusRootError> {
        let workspace = find_workspace_root_from(Path::new(env!("CARGO_MANIFEST_DIR")))?;
        Self::bind(&workspace, CorpusRootSource::WorkspaceDiscovery)
    }

    /// Return the canonical runtime path retained for diagnostics.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return how this root was selected.
    #[must_use]
    pub const fn source(&self) -> CorpusRootSource {
        self.source
    }

    /// Return whether two authorities retain the same open directory identity.
    #[must_use]
    pub fn same_directory(&self, other: &Self) -> bool {
        self.directory.as_ref() == other.directory.as_ref()
    }

    /// Clone the retained open directory without reopening its pathname.
    ///
    /// This capability handoff is intended for later component-by-component
    /// traversal. The returned file does not grant path-based member authority
    /// by itself.
    pub fn try_clone_directory(&self) -> Result<File, CorpusRootError> {
        self.directory.as_file().try_clone().map_err(|error| {
            CorpusRootError::CapabilityUnavailable {
                path: self.path.clone(),
                message: error.to_string(),
            }
        })
    }

    /// Require the two checked-in top-level corpus layers.
    ///
    /// This validates only the required directory chain. It does not recurse,
    /// select members, inspect extensions, or redefine [`crate::CorpusTopology`].
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        self.require_directory(Path::new("test_corpus"), "test_corpus")?;
        self.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        Ok(())
    }

    fn bind(path: &Path, source: CorpusRootSource) -> Result<Self, CorpusRootError> {
        let canonical = validate_absolute_directory(path)?;
        let directory = Handle::from_path(&canonical).map_err(|error| {
            CorpusRootError::CapabilityUnavailable {
                path: canonical.clone(),
                message: error.to_string(),
            }
        })?;
        let metadata = directory.as_file().metadata().map_err(|error| {
            CorpusRootError::RootUnreadable { path: canonical.clone(), message: error.to_string() }
        })?;
        if !metadata.is_dir() {
            return Err(CorpusRootError::RootNotDirectory { path: canonical });
        }

        validate_absolute_directory_identity(&canonical, &directory)?;
        Ok(Self { path: canonical, source, directory: Arc::new(directory) })
    }

    fn require_directory(
        &self,
        relative: &Path,
        layer: &'static str,
    ) -> Result<(), CorpusRootError> {
        validate_relative_directory_path(relative)?;
        self.verify_bound_path()?;

        let mut current = self.path.clone();
        for component in relative.components() {
            let Component::Normal(value) = component else {
                return Err(CorpusRootError::InvalidPath {
                    path: relative.to_path_buf(),
                    reason: "required_layer_path_must_use_normal_components",
                });
            };
            current.push(value);
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(CorpusRootError::RequiredLayerMissing { layer, path: current });
                }
                Err(error) => {
                    return Err(CorpusRootError::RequiredLayerUnreadable {
                        layer,
                        path: current,
                        message: error.to_string(),
                    });
                }
            };
            if is_link_or_reparse(&metadata) {
                return Err(CorpusRootError::RequiredLayerSymlinkOrReparse {
                    layer,
                    path: current,
                });
            }
            if !metadata.is_dir() {
                return Err(CorpusRootError::RequiredLayerNotDirectory { layer, path: current });
            }
        }

        let entries =
            fs::read_dir(&current).map_err(|error| CorpusRootError::RequiredLayerUnreadable {
                layer,
                path: current.clone(),
                message: error.to_string(),
            })?;
        for entry in entries {
            entry.map_err(|error| CorpusRootError::RequiredLayerUnreadable {
                layer,
                path: current.clone(),
                message: error.to_string(),
            })?;
        }

        self.verify_bound_path()
    }

    fn verify_bound_path(&self) -> Result<(), CorpusRootError> {
        let current = Handle::from_path(&self.path)
            .map_err(|_| CorpusRootError::RootIdentityChanged { path: self.path.clone() })?;
        if self.directory.as_ref() != &current {
            return Err(CorpusRootError::RootIdentityChanged { path: self.path.clone() });
        }
        let canonical = fs::canonicalize(&self.path)
            .map_err(|_| CorpusRootError::RootIdentityChanged { path: self.path.clone() })?;
        if canonical != self.path {
            return Err(CorpusRootError::RootIdentityChanged { path: self.path.clone() });
        }
        Ok(())
    }
}

fn resolve_authoritative_from(
    explicit: Option<&Path>,
    environment: Option<&OsStr>,
) -> Result<CorpusRoot, CorpusRootError> {
    if let Some(path) = explicit {
        return CorpusRoot::bind(path, CorpusRootSource::Explicit);
    }
    if let Some(path) = environment {
        return CorpusRoot::bind(Path::new(path), CorpusRootSource::Environment);
    }
    Err(CorpusRootError::AuthoritativeRootRequired)
}

fn validate_absolute_directory(path: &Path) -> Result<PathBuf, CorpusRootError> {
    if !path.is_absolute() {
        return Err(CorpusRootError::RelativePath { path: path.to_path_buf() });
    }
    validate_directory_components(path)?;
    let canonical =
        fs::canonicalize(path).map_err(|error| classify_root_io(path.to_path_buf(), error))?;
    validate_directory_components(&canonical)?;
    Ok(canonical)
}

fn validate_absolute_directory_identity(
    canonical: &Path,
    retained: &Handle,
) -> Result<(), CorpusRootError> {
    let rebound = Handle::from_path(canonical)
        .map_err(|_| CorpusRootError::RootIdentityChanged { path: canonical.to_path_buf() })?;
    let recanonicalized = fs::canonicalize(canonical)
        .map_err(|_| CorpusRootError::RootIdentityChanged { path: canonical.to_path_buf() })?;
    if retained != &rebound || recanonicalized != canonical {
        return Err(CorpusRootError::RootIdentityChanged { path: canonical.to_path_buf() });
    }
    Ok(())
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
                    Err(error) => return Err(classify_root_io(current, error)),
                };
                if is_link_or_reparse(&metadata) {
                    return Err(CorpusRootError::SymlinkOrReparseUnsupported { path: current });
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
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| classify_root_io(path.to_path_buf(), error))?;
        if is_link_or_reparse(&metadata) {
            return Err(CorpusRootError::SymlinkOrReparseUnsupported { path: path.to_path_buf() });
        }
        if !metadata.is_dir() {
            return Err(CorpusRootError::RootNotDirectory { path: path.to_path_buf() });
        }
    }
    Ok(())
}

fn validate_relative_directory_path(relative: &Path) -> Result<(), CorpusRootError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(CorpusRootError::InvalidPath {
            path: relative.to_path_buf(),
            reason: "required_layer_path_must_be_nonempty_and_relative",
        });
    }
    if relative.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(CorpusRootError::InvalidPath {
            path: relative.to_path_buf(),
            reason: "required_layer_path_must_use_normal_components",
        });
    }
    Ok(())
}

fn classify_root_io(path: PathBuf, error: io::Error) -> CorpusRootError {
    if error.kind() == io::ErrorKind::NotFound {
        CorpusRootError::RootMissing { path }
    } else {
        CorpusRootError::RootUnreadable { path, message: error.to_string() }
    }
}

fn is_link_or_reparse(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn find_workspace_root_from(start: &Path) -> Result<PathBuf, CorpusRootError> {
    for ancestor in start.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        let contents = match fs::read_to_string(&manifest) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(CorpusRootError::RootUnreadable {
                    path: manifest,
                    message: error.to_string(),
                });
            }
        };
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
    use crate::api::CWD_TEST_LOCK;
    use std::error::Error;

    struct CurrentDirGuard {
        original: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(path: &Path) -> Result<Self, Box<dyn Error>> {
            let original = env::current_dir()?;
            env::set_current_dir(path)?;
            Ok(Self { original })
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    fn test_failure(message: impl Into<String>) -> Box<dyn Error> {
        io::Error::other(message.into()).into()
    }

    #[test]
    fn source_tokens_are_stable() -> Result<(), Box<dyn Error>> {
        let actual = [
            CorpusRootSource::Explicit.as_str(),
            CorpusRootSource::Environment.as_str(),
            CorpusRootSource::WorkspaceDiscovery.as_str(),
        ];
        let expected = ["explicit", "environment", "workspace_discovery"];
        if actual == expected {
            Ok(())
        } else {
            Err(test_failure(format!("unexpected source tokens: {actual:?}")))
        }
    }

    #[test]
    fn explicit_precedence_never_falls_through_to_environment() -> Result<(), Box<dyn Error>> {
        let explicit = tempfile::tempdir()?;
        let environment = tempfile::tempdir()?;
        let selected = resolve_authoritative_from(
            Some(explicit.path()),
            Some(environment.path().as_os_str()),
        )?;
        if selected.source() != CorpusRootSource::Explicit
            || selected.path() != explicit.path().canonicalize()?
        {
            return Err(test_failure("explicit root did not win"));
        }

        let invalid = Path::new("relative-explicit");
        match resolve_authoritative_from(Some(invalid), Some(environment.path().as_os_str())) {
            Err(CorpusRootError::RelativePath { path }) if path == invalid => Ok(()),
            other => Err(test_failure(format!(
                "invalid explicit root unexpectedly fell through: {other:?}"
            ))),
        }
    }

    #[test]
    fn relative_environment_root_fails_closed() -> Result<(), Box<dyn Error>> {
        let relative = OsStr::new("relative-environment");
        match resolve_authoritative_from(None, Some(relative)) {
            Err(CorpusRootError::RelativePath { path })
                if path == Path::new("relative-environment") =>
            {
                Ok(())
            }
            other => {
                Err(test_failure(format!("relative environment root was not rejected: {other:?}")))
            }
        }
    }

    #[test]
    fn explicit_resolution_is_current_directory_independent() -> Result<(), Box<dyn Error>> {
        let _lock = CWD_TEST_LOCK
            .lock()
            .map_err(|_| test_failure("current-directory test lock was poisoned"))?;
        let root = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;

        let before = CorpusRoot::explicit(root.path())?;
        let _guard = CurrentDirGuard::enter(other.path())?;
        let after = CorpusRoot::explicit(root.path())?;

        if before.path() == after.path() && before.same_directory(&after) {
            Ok(())
        } else {
            Err(test_failure("same explicit root changed identity with current directory"))
        }
    }
}
