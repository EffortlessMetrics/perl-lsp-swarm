use perl_corpus::{CorpusPaths, CorpusRoot, CorpusRootError, CorpusRootSource};
use std::fs;

#[test]
fn root_source_tokens_are_stable() {
    assert_eq!(CorpusRootSource::Explicit.as_str(), "explicit");
    assert_eq!(CorpusRootSource::Environment.as_str(), "environment");
    assert_eq!(CorpusRootSource::WorkspaceDiscovery.as_str(), "workspace_discovery");
}

#[test]
fn required_layout_rejects_a_non_directory_intermediate_component()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("test_corpus"))?;
    fs::write(root.path().join("crates"), "not a directory")?;

    let paths = CorpusPaths::try_from_root(root.path())?;
    assert!(matches!(
        paths.require_repository_layout(),
        Err(CorpusRootError::RequiredLayerNotDirectory {
            layer: "fuzz",
            ..
        })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn explicit_root_rejects_an_intermediate_symlink_component()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir()?;
    let actual_parent = parent.path().join("actual");
    let actual_root = actual_parent.join("repo");
    let linked_parent = parent.path().join("linked");
    fs::create_dir_all(&actual_root)?;
    symlink(&actual_parent, &linked_parent)?;

    assert!(matches!(
        CorpusRoot::explicit(linked_parent.join("repo")),
        Err(CorpusRootError::SymlinkUnsupported { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn required_layout_rejects_a_symlinked_layer()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    symlink(external.path(), root.path().join("test_corpus"))?;
    fs::create_dir_all(root.path().join("crates/perl-corpus/fuzz"))?;

    let paths = CorpusPaths::try_from_root(root.path())?;
    assert!(matches!(
        paths.require_repository_layout(),
        Err(CorpusRootError::RequiredLayerSymlink {
            layer: "test_corpus",
            ..
        })
    ));
    Ok(())
}
