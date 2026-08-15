//! Corpus file discovery helpers.

use crate::api::root::{CorpusRoot, CorpusRootError, CorpusRootSource};
pub use crate::api::root::CORPUS_ROOT_ENV;
use std::env;
use std::fs;
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
///
/// The wrapped path set is intentionally private. Callers may borrow it through
/// [`Self::as_paths`] or immutable deref, or explicitly leave the validated
/// state through [`Self::into_paths`]. This prevents path mutation from making
/// the recorded selection provenance stale while the value still looks bound.
#[derive(Debug, Clone)]
pub struct ResolvedCorpusPaths {
    paths: CorpusPaths,
    source: CorpusRootSource,
}

impl ResolvedCorpusPaths {
    /// Return how this validated root was selected.
    #[must_use]
    pub const fn root_source(&self) -> CorpusRootSource {
        self.source
    }

    /// Borrow the validated compatibility path set.
    #[must_use]
    pub const fn as_paths(&self) -> &CorpusPaths {
        &self.paths
    }

    /// Leave the validated state and recover the compatibility path set.
    #[must_use]
    pub fn into_paths(self) -> CorpusPaths {
        self.paths
    }
}

impl Deref for ResolvedCorpusPaths {
    type Target = CorpusPaths;

    fn deref(&self) -> &Self::Target {
        self.as_paths()
    }
}

impl CorpusPaths {
    /// Construct compatibility corpus paths without validation.
    ///
    /// This method preserves the historical path-returning API. Load-bearing
    /// work should use [`Self::try_from_root`], [`Self::try_discover`], or
    /// [`Self::resolve_authoritative`] and retain the validated wrapper.
    #[must_use]
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            test_corpus: root.join("test_corpus"),
            fuzz: root.join("crates/perl-corpus/fuzz"),
            root,
        }
    }

    /// Discover corpus paths through the historical unchecked compatibility contract.
    ///
    /// This path preserves the raw environment value or compile-time workspace
    /// path, including a symlinked workspace ancestor. It does not validate or
    /// retain provenance and must not be used as evidence authority. Use
    /// [`Self::try_discover`] for strict developer discovery and
    /// [`Self::resolve_authoritative`] for load-bearing work.
    pub fn discover() -> Self {
        if let Some(root) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_root(PathBuf::from(root));
        }

        Self::from_root(find_compatibility_workspace_root())
    }

    /// Validate an explicit root and retain its authority.
    pub fn try_from_root(root: impl AsRef<Path>) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        let authority = CorpusRoot::explicit(root)?;
        Self::from_authority(authority)
    }

    /// Resolve the authoritative external corpus root.
    ///
    /// Precedence is explicit root, then [`CORPUS_ROOT_ENV`]. This method never
    /// falls back to developer workspace discovery.
    pub fn resolve_authoritative(
        explicit: Option<&Path>,
    ) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        let authority = CorpusRoot::resolve_authoritative(explicit)?;
        Self::from_authority(authority)
    }

    /// Resolve developer-convenience corpus paths with validation.
    ///
    /// Precedence is explicit root, then [`CORPUS_ROOT_ENV`], then compile-time
    /// workspace discovery. The resulting source remains visible to callers.
    pub fn try_discover(
        explicit: Option<&Path>,
    ) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        let authority = CorpusRoot::resolve_for_development(explicit)?;
        Self::from_authority(authority)
    }

    fn from_authority(authority: CorpusRoot) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        let paths = Self::from_root(authority.path().to_path_buf());
        paths.require_repository_layout()?;
        Ok(ResolvedCorpusPaths {
            paths,
            source: authority.source(),
        })
    }

    /// Require the checked-in repository layer directories owned by the current topology.
    ///
    /// This validates root and required-directory authority only. Selected
    /// descendant traversal belongs to [`crate::CorpusTopology`], and exact
    /// opened-file bytes belong to the shared member reader tracked by #7693.
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        let authority = CorpusRoot::explicit(&self.root)?;
        authority.require_directory(Path::new("test_corpus"), "test_corpus")?;
        authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        Ok(())
    }
}

fn find_compatibility_workspace_root() -> PathBuf {
    find_compatibility_workspace_root_from(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn find_compatibility_workspace_root_from(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        let Ok(contents) = fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<toml::Value>(&contents) else {
            continue;
        };
        if parsed
            .get("workspace")
            .and_then(toml::Value::as_table)
            .is_some()
        {
            return ancestor.to_path_buf();
        }
    }
    start.to_path_buf()
}

/// Return test corpus files using the compatibility discovery path.
#[must_use]
pub fn get_test_files() -> Vec<PathBuf> {
    get_test_files_from(&CorpusPaths::discover())
}

/// Return test corpus files from an explicit compatibility path set.
#[must_use]
pub fn get_test_files_from(paths: &CorpusPaths) -> Vec<PathBuf> {
    collect_perl_files(&paths.test_corpus)
}

/// Return fuzz files using the compatibility discovery path.
#[must_use]
pub fn get_fuzz_files() -> Vec<PathBuf> {
    collect_perl_files(&CorpusPaths::discover().fuzz)
}

/// Return all selected test and fuzz files using the compatibility path.
#[must_use]
pub fn get_all_test_files() -> Vec<PathBuf> {
    let paths = CorpusPaths::discover();
    let mut files = get_test_files_from(&paths);
    files.extend(collect_perl_files(&paths.fuzz));
    files.sort();
    files
}

fn collect_perl_files(root: &Path) -> Vec<PathBuf> {
    let Ok(read_dir) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if !is_hidden_path(&path) {
                files.extend(collect_perl_files(&path));
            }
            continue;
        }
        if file_type.is_file() && is_perl_source(&path) && !is_hidden_path(&path) {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn is_perl_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            TEST_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') || name.starts_with('_'))
}

/// Corpus layer identifier used by compatibility inventory results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusLayer {
    /// Main repository test corpus.
    TestCorpus,
    /// Fuzz regression fixtures.
    Fuzz,
}

/// One compatibility corpus file and its layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusFile {
    /// Source file path.
    pub path: PathBuf,
    /// Compatibility layer classification.
    pub layer: CorpusLayer,
}

/// Return compatibility corpus files from the default discovered path set.
#[must_use]
pub fn get_corpus_files() -> Vec<CorpusFile> {
    get_corpus_files_from(&CorpusPaths::discover())
}

/// Return compatibility corpus files from an explicit compatibility path set.
#[must_use]
pub fn get_corpus_files_from(paths: &CorpusPaths) -> Vec<CorpusFile> {
    let mut files: Vec<_> = get_test_files_from(paths)
        .into_iter()
        .map(|path| CorpusFile {
            path,
            layer: CorpusLayer::TestCorpus,
        })
        .chain(collect_perl_files(&paths.fuzz).into_iter().map(|path| CorpusFile {
            path,
            layer: CorpusLayer::Fuzz,
        }))
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = env::temp_dir().join(format!("{prefix}_{suffix}"));
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn from_root_preserves_public_path_shape() {
        let root = PathBuf::from("/tmp/corpus");
        let paths = CorpusPaths::from_root(root.clone());
        assert_eq!(paths.root, root);
        assert_eq!(paths.test_corpus, PathBuf::from("/tmp/corpus/test_corpus"));
        assert_eq!(
            paths.fuzz,
            PathBuf::from("/tmp/corpus/crates/perl-corpus/fuzz")
        );
    }

    #[test]
    fn compatibility_collects_sorted_supported_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_files")?;
        fs::create_dir_all(root.join("test_corpus/nested"))?;
        fs::write(root.join("test_corpus/nested/case.pl"), "1;")?;
        fs::write(root.join("test_corpus/Case.pm"), "package Case; 1;")?;
        fs::write(root.join("test_corpus/ignored.txt"), "not selected")?;

        let files = get_test_files_from(&CorpusPaths::from_root(root.clone()));
        assert_eq!(
            files,
            vec![
                root.join("test_corpus/Case.pm"),
                root.join("test_corpus/nested/case.pl")
            ]
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn compatibility_workspace_discovery_preserves_symlinked_ancestor()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = temp_root("perl_corpus_compat_symlink_workspace")?;
        let real_workspace = root.join("real-workspace");
        let crate_dir = real_workspace.join("crates/perl-corpus");
        fs::create_dir_all(&crate_dir)?;
        fs::write(
            real_workspace.join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )?;
        let linked_workspace = root.join("linked-workspace");
        symlink(&real_workspace, &linked_workspace)?;
        let linked_crate = linked_workspace.join("crates/perl-corpus");

        assert_eq!(
            find_compatibility_workspace_root_from(&linked_crate),
            linked_workspace
        );
        assert!(matches!(
            CorpusRoot::explicit(&linked_workspace),
            Err(CorpusRootError::SymlinkUnsupported { .. })
        ));

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn required_layout_leaves_excluded_metadata_symlink_to_topology()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = temp_root("perl_corpus_excluded_metadata_link")?;
        fs::create_dir_all(root.join("test_corpus"))?;
        let fuzz = root.join("crates/perl-corpus/fuzz");
        fs::create_dir_all(&fuzz)?;
        let outside = root.join("outside-readme.md");
        fs::write(&outside, "metadata only\n")?;
        symlink(&outside, fuzz.join("README.md"))?;

        CorpusPaths::try_from_root(&root)?.require_repository_layout()?;

        fs::remove_dir_all(&root)?;
        Ok(())
    }
}
