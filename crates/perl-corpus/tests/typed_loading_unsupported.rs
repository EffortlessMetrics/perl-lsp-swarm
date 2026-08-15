#![cfg(not(any(
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
)))]

use perl_corpus::{CorpusLoadError, load_plain_perl_source, load_sectioned_corpus_document};

#[test]
fn public_loaders_fail_closed_without_reviewed_no_follow_support()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let plain = root.path().join("case.pl");
    let sectioned = root.path().join("case.txt");
    std::fs::write(&plain, "my $value = 1;\n")?;
    std::fs::write(
        &sectioned,
        concat!(
            "==========================================\n",
            "Case\n",
            "==========================================\n",
            "my $value = 1;\n",
        ),
    )?;

    assert_eq!(
        load_plain_perl_source("test_corpus/case.pl", &plain),
        Err(CorpusLoadError::NoFollowUnsupported { path: plain.clone() })
    );
    assert!(matches!(
        load_sectioned_corpus_document("test_corpus/case.txt", &sectioned),
        Err(CorpusLoadError::NoFollowUnsupported { path }) if path == sectioned
    ));
    Ok(())
}
