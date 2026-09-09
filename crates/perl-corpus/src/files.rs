//! Corpus file discovery helpers.

pub use crate::api::root::CORPUS_ROOT_ENV;
use crate::api::root::{CorpusRoot, CorpusRootError, CorpusRootSource};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const TEST_EXTENSIONS: &[&str] = &["pl", "pm", "plx", "t", "psgi", "cgi"];

/// Common corpus paths anchored at a root directory.
///
/// This public mutable shape is retained for compatibility. It is not evidence
/// that the root was validated; use [`ResolvedCorpusPaths`] for load-bearing
/// work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusPaths {
    /// Workspace root used for discovery.
    pub root: PathBuf,
    /// Directory containing gap coverage corpus files.
    pub test_corpus: PathBuf,
    /// Directory containing fuzz regression fixtures.
    pub fuzz: PathBuf,
}

/// Validated compatibility paths plus their retained root authority.
///
/// This type deliberately does not implement [`Deref`](std::ops::Deref) or any
/// other implicit conversion into [`CorpusPaths`]. Downgrading validated paths
/// to the unchecked compatibility shape is a decision a caller has to write
/// down, either as the borrowed view [`Self::as_paths`] or the consuming
/// [`Self::into_paths`].
///
/// The examples below are illustrative, not enforcement. This repository runs
/// no `cargo test --doc` gate (see issue #13774), so a doctest here cannot fail
/// CI. The boundary is actually held by `assert_does_not_implement!` in
/// `tests/root_path_authority.rs`, which stops that test target from compiling
/// if such an impl reappears.
///
/// A resolved value is not accepted where unchecked compatibility paths are
/// expected:
///
/// ```compile_fail
/// use perl_corpus::{CorpusPaths, ResolvedCorpusPaths};
///
/// fn compatibility_discovery(_paths: &CorpusPaths) {}
///
/// fn downgrade_without_saying_so(resolved: &ResolvedCorpusPaths) {
///     // No `Deref` coercion exists, so this is a type error.
///     compatibility_discovery(resolved);
/// }
/// ```
///
/// The same call site compiles once the downgrade is explicit, which is the
/// opposite-direction control for the example above:
///
/// ```
/// use perl_corpus::{CorpusPaths, ResolvedCorpusPaths};
///
/// fn compatibility_discovery(_paths: &CorpusPaths) {}
///
/// fn downgrade_explicitly(resolved: &ResolvedCorpusPaths) {
///     compatibility_discovery(resolved.as_paths());
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCorpusPaths {
    root: CorpusRoot,
    paths: CorpusPaths,
}

impl ResolvedCorpusPaths {
    fn from_root(root: CorpusRoot) -> Self {
        let paths = CorpusPaths::from_root(root.path().to_path_buf());
        Self { root, paths }
    }

    /// Return how the authoritative root was selected.
    #[must_use]
    pub const fn root_source(&self) -> CorpusRootSource {
        self.root.source()
    }

    /// Borrow the retained root authority.
    #[must_use]
    pub const fn root_authority(&self) -> &CorpusRoot {
        &self.root
    }

    /// Borrow the derived compatibility paths.
    ///
    /// This is the intentional borrowed compatibility view. It is the only way
    /// to reach `&CorpusPaths` from a retained resolution, and it is never
    /// applied implicitly.
    #[must_use]
    pub const fn as_paths(&self) -> &CorpusPaths {
        &self.paths
    }

    /// Consume the value and explicitly downgrade to unchecked compatibility paths.
    ///
    /// Downgrading discards the retained root authority, so the resulting
    /// [`CorpusPaths`] is no longer evidence that the root was validated.
    ///
    /// The example is illustrative rather than enforced proof: no gate in this
    /// repository runs `cargo test --doc` (see issue #13774), so a false
    /// assertion here would not fail CI. The executed regression lives in
    /// `tests/root_path_authority.rs`.
    ///
    /// ```
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use perl_corpus::CorpusPaths;
    ///
    /// let root = tempfile::tempdir()?;
    /// let resolved = CorpusPaths::try_from_root(root.path())?;
    /// let authority_root = resolved.root_authority().path().to_path_buf();
    ///
    /// let compatibility = resolved.into_paths();
    /// assert_eq!(compatibility.root, authority_root);
    /// assert_eq!(compatibility.test_corpus, authority_root.join("test_corpus"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # }
    /// ```
    #[must_use]
    pub fn into_paths(self) -> CorpusPaths {
        self.paths
    }

    /// Require the repository's two checked-in top-level corpus layers.
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        self.root.require_repository_layout()
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
    /// Discover corpus paths through the historical compatibility contract.
    ///
    /// This method deliberately preserves raw environment and compile-time
    /// workspace paths. It performs no strict validation and cannot produce a
    /// [`CorpusRoot`]. Use [`Self::resolve_authoritative`] for load-bearing work.
    pub fn discover() -> Self {
        if let Ok(root) = env::var(CORPUS_ROOT_ENV) {
            return Self::from_root(PathBuf::from(root));
        }

        Self::from_root(find_workspace_root())
    }

    /// Discover the compile-time workspace and bind it as strict authority.
    pub fn try_discover() -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::try_discover().map(ResolvedCorpusPaths::from_root)
    }

    /// Resolve strict authority from explicit input, then [`CORPUS_ROOT_ENV`].
    pub fn resolve_authoritative(
        explicit: Option<&Path>,
    ) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::resolve_authoritative(explicit).map(ResolvedCorpusPaths::from_root)
    }

    /// Validate an explicit root and derive its common corpus paths.
    pub fn try_from_root(root: impl AsRef<Path>) -> Result<ResolvedCorpusPaths, CorpusRootError> {
        CorpusRoot::explicit(root).map(ResolvedCorpusPaths::from_root)
    }

    /// Build corpus paths from an unchecked explicit root.
    ///
    /// This compatibility constructor preserves the existing public shape for
    /// synthetic tests and callers that own their own validation.
    #[must_use]
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            test_corpus: root.join("test_corpus"),
            fuzz: root.join("crates/perl-corpus/fuzz"),
            root,
        }
    }
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
        .map(|path| CorpusFile { path, layer: CorpusLayer::TestCorpus })
        .collect();

    files.extend(
        get_fuzz_files_from(paths)
            .into_iter()
            .map(|path| CorpusFile { path, layer: CorpusLayer::Fuzz }),
    );

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    files
}

/// Return all available Perl sources across corpus layers.
pub fn get_all_test_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = get_corpus_files().into_iter().map(|file| file.path).collect();
    files.sort();
    files.dedup();
    files
}

fn find_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        if let Ok(contents) = fs::read_to_string(&cargo_toml)
            && contents.contains("[workspace]")
        {
            return ancestor.to_path_buf();
        }
    }

    manifest_dir
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
        .map(|ext| extensions.iter().any(|allowed| ext.eq_ignore_ascii_case(allowed)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> std::io::Result<PathBuf> {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
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
    fn collect_files_filters_extensions_and_skips_hidden() -> Result<(), Box<dyn std::error::Error>>
    {
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
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            })
            .collect();
        names.sort();

        let expected =
            vec!["case.cgi", "case.pl", "case.plx", "case.pm", "case.psgi", "case.t", "nested.pl"];
        assert_eq!(names, expected);

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn corpus_files_include_layer_info() -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_layers")?;
        let test_dir = root.join("test_corpus");
        let fuzz_dir = root.join("crates/perl-corpus/fuzz");
        fs::create_dir_all(&test_dir)?;
        fs::create_dir_all(&fuzz_dir)?;

        let test_file = test_dir.join("case.pl");
        let fuzz_file = fuzz_dir.join("fuzz_case.pl");
        fs::write(&test_file, "print 1;\n")?;
        fs::write(&fuzz_file, "print 2;\n")?;

        let paths = CorpusPaths::from_root(root.clone());
        let files = get_corpus_files_from(&paths);

        assert!(
            files
                .iter()
                .any(|file| file.layer == CorpusLayer::TestCorpus && file.path == test_file),
            "Expected test corpus file in results"
        );
        assert!(
            files.iter().any(|file| file.layer == CorpusLayer::Fuzz && file.path == fuzz_file),
            "Expected fuzz file in results"
        );

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn collect_files_matches_extensions_case_insensitively()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_case_insensitive_ext")?;
        let fixtures = [
            root.join("upper.PL"),
            root.join("mixed.Pm"),
            root.join("suite.T"),
            root.join("tool.PlX"),
            root.join("app.PsGi"),
            root.join("legacy.CgI"),
        ];

        for fixture in &fixtures {
            fs::write(fixture, "print 1;\n")?;
        }

        let files = collect_files(&root, TEST_EXTENSIONS);
        let mut names: Vec<_> = files
            .iter()
            .map(|path| {
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            })
            .collect();
        names.sort();

        let expected =
            vec!["app.PsGi", "legacy.CgI", "mixed.Pm", "suite.T", "tool.PlX", "upper.PL"];
        assert_eq!(names, expected);

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn corpus_paths_discover_prefers_env_root() -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_env_root")?;
        let _env_guard = EnvVarGuard::set(CORPUS_ROOT_ENV, &root);

        let discovered = CorpusPaths::discover();
        assert_eq!(discovered.root, root);
        assert_eq!(discovered.test_corpus, discovered.root.join("test_corpus"));
        assert_eq!(discovered.fuzz, discovered.root.join("crates/perl-corpus/fuzz"));

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn get_all_test_files_is_sorted_and_deduplicated() -> Result<(), Box<dyn std::error::Error>> {
        let root = temp_root("perl_corpus_all_files")?;
        let test_dir = root.join("test_corpus");
        let fuzz_dir = root.join("crates/perl-corpus/fuzz");
        fs::create_dir_all(&test_dir)?;
        fs::create_dir_all(&fuzz_dir)?;

        let shared = test_dir.join("shared.pl");
        fs::write(&shared, "print 1;\n")?;
        fs::write(test_dir.join("zzz.pm"), "1;\n")?;
        fs::write(fuzz_dir.join("aaa.pl"), "print 2;\n")?;

        let paths = CorpusPaths::from_root(root.clone());
        let mut all: Vec<PathBuf> =
            get_corpus_files_from(&paths).into_iter().map(|file| file.path).collect();
        all.sort();
        all.dedup();

        let mut from_api = {
            let _env_guard = EnvVarGuard::set(CORPUS_ROOT_ENV, &root);
            get_all_test_files()
        };
        from_api.sort();
        from_api.dedup();

        assert_eq!(from_api, all);
        assert_eq!(
            from_api.first().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
            Some("aaa.pl")
        );
        assert_eq!(
            from_api.last().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
            Some("zzz.pm")
        );

        fs::remove_dir_all(&root)?;
        Ok(())
    }
}
