//! Public controls for the strict sectioned-document contract.
//!
//! These prove the boundary through the exported crate API only: a valid first
//! section must not be able to hide a malformed later section, and identity is
//! rejected before any filesystem access.

#![cfg(any(
    windows,
    all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv32",
            target_arch = "riscv64"
        )
    ),
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
use perl_corpus::{CorpusLoadError, load_plain_perl_source, load_sectioned_corpus_document};
use std::fs;

#[test]
fn public_section_loader_rejects_partial_malformed_population()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("partial.txt");
    fs::write(&path, "====\nValid\n====\nmy $value = 1;\n====\nBroken\nmy $value = 2;\n")?;

    assert!(matches!(
        load_sectioned_corpus_document("fixtures/partial.txt", &path),
        Err(CorpusLoadError::MalformedSection { line: 5, reason: "missing_closing_delimiter", .. })
    ));
    Ok(())
}

#[test]
fn public_loaders_reject_whitespace_only_identity_before_reading()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.txt");

    assert!(matches!(load_plain_perl_source("  ", &missing), Err(CorpusLoadError::EmptyAssetId)));
    assert!(matches!(
        load_sectioned_corpus_document("\t", &missing),
        Err(CorpusLoadError::EmptyAssetId)
    ));
    Ok(())
}

#[test]
fn public_section_loader_preserves_crlf_source_but_normalizes_case_body()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crlf.txt");
    let source = "====\r\nCRLF case\r\n====\r\nmy $value = 1;\r\n";
    fs::write(&path, source)?;

    let document = load_sectioned_corpus_document("fixtures/crlf.txt", &path)?;
    assert_eq!(document.source, source);
    assert_eq!(document.cases.len(), 1);
    assert_eq!(document.cases[0].section.body, "my $value = 1;");
    Ok(())
}

#[cfg(windows)]
#[test]
fn public_plain_loader_rejects_windows_reparse_point() -> Result<(), Box<dyn std::error::Error>> {
    use perl_tdd_support::try_create_file_symlink;

    // Typed skip when the Windows session lacks the symlink privilege
    // (os error 1314): without the privilege the reparse-point fixture cannot
    // exist. With the privilege present the rejection semantics run in full.
    if perl_tdd_support::symlink_test_decision().skip_visibly() {
        return Ok(());
    }

    let directory = tempfile::tempdir()?;
    let target = directory.path().join("source.pl");
    let link = directory.path().join("source-link.pl");
    fs::write(&target, "my $value = 1;\n")?;
    if try_create_file_symlink(&target, &link)?.is_none() {
        // Unprivileged Windows session: reparse-point rejection cannot be
        // exercised without symlink capability; junctions and copies do not
        // admit the same proof, so the typed skip is the honest outcome.
        return Ok(());
    }

    assert!(matches!(
        load_plain_perl_source("fixtures/source-link.pl", &link),
        Err(CorpusLoadError::SymlinkUnsupported { path }) if path == link
    ));
    Ok(())
}
