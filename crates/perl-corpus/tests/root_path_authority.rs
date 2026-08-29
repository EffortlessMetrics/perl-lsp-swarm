use perl_corpus::{
    CorpusPaths, CorpusRoot, CorpusRootError, CorpusRootSource, ResolvedCorpusPaths,
};
use same_file::Handle;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

/// Fail the build when `$type` implements `$bound`.
///
/// Two blanket impls are in scope for every type. The second one additionally
/// requires `$bound`, so it applies only when `$type` satisfies it. When both
/// apply the associated-function reference below is ambiguous and this file
/// stops compiling; when only the unconditional one applies, inference picks it
/// and the assertion costs nothing at runtime.
///
/// This lives in an integration test rather than a `compile_fail` doctest on
/// purpose: the repository's gates run `cargo test --locked --tests`, which
/// builds this target, and never run `cargo test --doc`. A boundary that is
/// only guarded by a doctest is not actually guarded here.
macro_rules! assert_does_not_implement {
    ($type:ty: $bound:path) => {
        const _: fn() = || {
            trait AmbiguousIfImplemented<Discriminant> {
                fn probe() {}
            }
            impl<T: ?Sized> AmbiguousIfImplemented<()> for T {}
            impl<T: ?Sized + $bound> AmbiguousIfImplemented<u8> for T {}

            <$type as AmbiguousIfImplemented<_>>::probe();
        };
    };
}

// `ResolvedCorpusPaths` must reach `CorpusPaths` only through the explicit
// `as_paths()` / `into_paths()` downgrade. Any implicit conversion would let a
// validated resolution satisfy a path-based compatibility API without the call
// site recording that the root authority was dropped. Re-introducing one of
// these impls breaks the build of this test target.
assert_does_not_implement!(ResolvedCorpusPaths: std::ops::Deref);
assert_does_not_implement!(ResolvedCorpusPaths: AsRef<CorpusPaths>);
assert_does_not_implement!(ResolvedCorpusPaths: std::borrow::Borrow<CorpusPaths>);

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

fn expect_error<T>(
    result: Result<T, CorpusRootError>,
    expected: impl FnOnce(&CorpusRootError) -> bool,
    context: &str,
) -> TestResult {
    match result {
        Err(error) => {
            if expected(&error) {
                Ok(())
            } else {
                Err(failure(format!("{context}: unexpected error: {error:?}")))
            }
        }
        Ok(_) => Err(failure(format!("{context}: operation unexpectedly succeeded"))),
    }
}

fn create_repository_layout(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("test_corpus"))?;
    fs::create_dir_all(root.join("crates/perl-corpus/fuzz"))
}

fn require_explicit_source(paths: &ResolvedCorpusPaths) -> TestResult {
    if paths.root_source() == CorpusRootSource::Explicit {
        Ok(())
    } else {
        Err(failure(format!("expected explicit root source, got {:?}", paths.root_source())))
    }
}

#[test]
fn strict_root_retains_shareable_directory_identity() -> TestResult {
    let root = tempfile::tempdir()?;
    create_repository_layout(root.path())?;

    let resolved = CorpusPaths::try_from_root(root.path())?;
    require_explicit_source(&resolved)?;
    resolved.require_repository_layout()?;

    let cloned_authority = resolved.root_authority().clone();
    if !resolved.root_authority().same_directory(&cloned_authority) {
        return Err(failure("cloned authority did not retain directory identity"));
    }

    let retained = Handle::from_file(resolved.root_authority().try_clone_directory()?)?;
    let path_opened = Handle::from_path(resolved.root_authority().path())?;
    if retained == path_opened {
        Ok(())
    } else {
        Err(failure("cloned retained directory handle did not match the bound root"))
    }
}

#[test]
fn strict_root_distinguishes_relative_missing_and_file_inputs() -> TestResult {
    expect_error(
        CorpusRoot::explicit(Path::new("relative-root")),
        |error| matches!(error, CorpusRootError::RelativePath { .. }),
        "relative root",
    )?;

    let parent = tempfile::tempdir()?;
    let missing = parent.path().join("missing");
    expect_error(
        CorpusRoot::explicit(&missing),
        |error| matches!(error, CorpusRootError::RootMissing { path } if path == &missing),
        "missing root",
    )?;

    let file = parent.path().join("file-root");
    fs::write(&file, b"not a directory")?;
    expect_error(
        CorpusRoot::explicit(&file),
        |error| matches!(error, CorpusRootError::RootNotDirectory { path } if path == &file),
        "file root",
    )
}

#[test]
fn required_layers_fail_independently_without_recursive_population_policy() -> TestResult {
    let missing_test = tempfile::tempdir()?;
    fs::create_dir_all(missing_test.path().join("crates/perl-corpus/fuzz"))?;
    let authority = CorpusRoot::explicit(missing_test.path())?;
    expect_error(
        authority.require_repository_layout(),
        |error| matches!(error, CorpusRootError::RequiredLayerMissing { layer: "test_corpus", .. }),
        "missing test_corpus layer",
    )?;

    let missing_fuzz = tempfile::tempdir()?;
    fs::create_dir_all(missing_fuzz.path().join("test_corpus"))?;
    let authority = CorpusRoot::explicit(missing_fuzz.path())?;
    expect_error(
        authority.require_repository_layout(),
        |error| matches!(error, CorpusRootError::RequiredLayerMissing { layer: "fuzz", .. }),
        "missing fuzz layer",
    )
}

#[test]
fn required_layout_rejects_non_directory_intermediate_component() -> TestResult {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("test_corpus"))?;
    fs::write(root.path().join("crates"), b"not a directory")?;

    let authority = CorpusRoot::explicit(root.path())?;
    expect_error(
        authority.require_repository_layout(),
        |error| matches!(error, CorpusRootError::RequiredLayerNotDirectory { layer: "fuzz", .. }),
        "non-directory fuzz path",
    )
}

#[test]
fn explicit_downgrade_remains_the_supported_compatibility_boundary() -> TestResult {
    let root = tempfile::tempdir()?;
    create_repository_layout(root.path())?;

    let resolved = CorpusPaths::try_from_root(root.path())?;
    let authority_root = resolved.root_authority().path().to_path_buf();
    let borrowed = resolved.as_paths().clone();

    let downgraded = resolved.into_paths();
    if borrowed != downgraded {
        return Err(failure("borrowed compatibility view and explicit downgrade disagreed"));
    }

    if downgraded.root != authority_root {
        return Err(failure(format!(
            "downgraded root {:?} did not retain the validated root {authority_root:?}",
            downgraded.root
        )));
    }
    if downgraded.test_corpus != authority_root.join("test_corpus") {
        return Err(failure(format!(
            "downgraded test_corpus layer was not derived from the validated root: {:?}",
            downgraded.test_corpus
        )));
    }
    if downgraded.fuzz != authority_root.join("crates/perl-corpus/fuzz") {
        return Err(failure(format!(
            "downgraded fuzz layer was not derived from the validated root: {:?}",
            downgraded.fuzz
        )));
    }

    // What the downgrade drops is the retained `CorpusRoot`, not a capability
    // encoded in `CorpusPaths` itself: `CorpusPaths` has public mutable fields,
    // so the downgraded value never carried authority at the type level. The
    // narrower property worth pinning is that it is not validated by
    // construction either, so a caller wanting authority back has to go through
    // `CorpusRoot::explicit` again. Mutating `root` first forces a specific
    // `RelativePath` rejection instead of depending on whatever the validated
    // root happened to be. The `as_paths()`/`into_paths()` boundary itself is
    // guarded by `assert_does_not_implement!` above, not here.
    let mut mutated = downgraded;
    mutated.root = PathBuf::from("relative-compatibility-root");
    expect_error(
        CorpusRoot::explicit(&mutated.root),
        |error| matches!(error, CorpusRootError::RelativePath { .. }),
        "re-validating a downgraded value goes back through CorpusRoot::explicit",
    )
}

#[test]
fn unchecked_compatibility_paths_require_explicit_validation_upgrade() -> TestResult {
    let unchecked = CorpusPaths::from_root(PathBuf::from("relative-compatibility-root"));
    if unchecked.root.as_path() != Path::new("relative-compatibility-root") {
        return Err(failure("compatibility constructor rewrote the raw root"));
    }

    expect_error(
        CorpusRoot::explicit(&unchecked.root),
        |error| matches!(error, CorpusRootError::RelativePath { .. }),
        "compatibility-to-authority upgrade",
    )
}

#[cfg(unix)]
#[test]
fn strict_root_rejects_intermediate_symlink_components() -> TestResult {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir()?;
    let actual_parent = parent.path().join("actual");
    let actual_root = actual_parent.join("repo");
    let linked_parent = parent.path().join("linked");
    fs::create_dir_all(&actual_root)?;
    symlink(&actual_parent, &linked_parent)?;

    expect_error(
        CorpusRoot::explicit(linked_parent.join("repo")),
        |error| matches!(error, CorpusRootError::SymlinkOrReparseUnsupported { .. }),
        "intermediate symlink root",
    )
}

#[cfg(unix)]
#[test]
fn required_layout_rejects_linked_layer_but_not_nested_member_policy() -> TestResult {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    symlink(external.path(), root.path().join("test_corpus"))?;
    fs::create_dir_all(root.path().join("crates/perl-corpus/fuzz"))?;

    let authority = CorpusRoot::explicit(root.path())?;
    expect_error(
        authority.require_repository_layout(),
        |error| {
            matches!(
                error,
                CorpusRootError::RequiredLayerSymlinkOrReparse { layer: "test_corpus", .. }
            )
        },
        "linked top-level layer",
    )?;

    fs::remove_file(root.path().join("test_corpus"))?;
    create_repository_layout(root.path())?;
    symlink(external.path(), root.path().join("test_corpus/nested-link"))?;
    authority
        .require_repository_layout()
        .map_err(|error| failure(format!("nested member was recursively policed: {error:?}")))
}

#[cfg(windows)]
#[test]
fn strict_root_and_required_layer_reject_windows_reparse_points() -> TestResult {
    use perl_tdd_support::try_create_dir_symlink;

    let parent = tempfile::tempdir()?;
    let actual = parent.path().join("actual");
    let linked = parent.path().join("linked");
    fs::create_dir(&actual)?;
    if try_create_dir_symlink(&actual, &linked)?.is_none() {
        return Ok(());
    }
    expect_error(
        CorpusRoot::explicit(&linked),
        |error| matches!(error, CorpusRootError::SymlinkOrReparseUnsupported { .. }),
        "Windows reparse root",
    )?;

    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("crates/perl-corpus/fuzz"))?;
    let external = tempfile::tempdir()?;
    if try_create_dir_symlink(external.path(), &root.path().join("test_corpus"))?.is_none() {
        return Ok(());
    }
    let authority = CorpusRoot::explicit(root.path())?;
    expect_error(
        authority.require_repository_layout(),
        |error| {
            matches!(
                error,
                CorpusRootError::RequiredLayerSymlinkOrReparse { layer: "test_corpus", .. }
            )
        },
        "Windows reparse layer",
    )
}

#[cfg(unix)]
#[test]
fn retained_handle_is_not_redirected_by_path_replacement() -> TestResult {
    let parent = tempfile::tempdir()?;
    let root = parent.path().join("root");
    let moved = parent.path().join("moved");
    fs::create_dir(&root)?;

    let authority = CorpusRoot::explicit(&root)?;
    let retained = Handle::from_file(authority.try_clone_directory()?)?;

    fs::rename(&root, &moved)?;
    fs::create_dir(&root)?;

    let moved_handle = Handle::from_path(&moved)?;
    let replacement_handle = Handle::from_path(&root)?;
    if retained != moved_handle || retained == replacement_handle {
        return Err(failure("retained authority followed the replacement pathname"));
    }

    expect_error(
        authority.require_repository_layout(),
        |error| matches!(error, CorpusRootError::RootIdentityChanged { .. }),
        "replaced root pathname",
    )
}
