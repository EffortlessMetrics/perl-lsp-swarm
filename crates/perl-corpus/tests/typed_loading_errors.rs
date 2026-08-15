#![cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]

use perl_corpus::{CorpusLoadError, load_plain_perl_source};

#[test]
fn public_loader_rejects_empty_identity_missing_asset_and_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let missing = root.path().join("missing.pl");

    assert_eq!(load_plain_perl_source("", &missing), Err(CorpusLoadError::EmptyAssetId));
    assert!(matches!(
        load_plain_perl_source("test_corpus/missing.pl", &missing),
        Err(CorpusLoadError::Missing { .. })
    ));
    assert!(matches!(
        load_plain_perl_source("test_corpus/directory", root.path()),
        Err(CorpusLoadError::NotRegularFile { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn public_loader_rejects_symlink_leaf() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let target = root.path().join("target.pl");
    let link = root.path().join("link.pl");
    std::fs::write(&target, "my $value = 1;\n")?;
    symlink(&target, &link)?;

    assert!(matches!(
        load_plain_perl_source("test_corpus/link.pl", &link),
        Err(CorpusLoadError::SymlinkUnsupported { .. })
    ));
    Ok(())
}
