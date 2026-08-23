//! Unsupported-target behavior for the public corpus loaders.
//!
//! This test runs on every target and discriminates through the
//! production-owned [`NO_FOLLOW_REVIEWED`] predicate: on reviewed ABIs the
//! loaders must succeed, and on unreviewed ABIs they must fail closed with
//! `NoFollowUnsupported`. The alternative — cfg-gating this file to the
//! complement of the supported set — would leave the seam unexecuted on every
//! hosted target and let the test gate drift from production.

use perl_corpus::{
    CorpusLoadError, NO_FOLLOW_REVIEWED, load_plain_perl_source, load_sectioned_corpus_document,
};

#[test]
fn public_loaders_follow_the_reviewed_no_follow_predicate() -> Result<(), Box<dyn std::error::Error>>
{
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

    let plain_result = load_plain_perl_source("test_corpus/case.pl", &plain);
    let sectioned_result = load_sectioned_corpus_document("test_corpus/case.txt", &sectioned);
    if NO_FOLLOW_REVIEWED {
        assert!(plain_result.is_ok(), "reviewed target must load plain source: {plain_result:?}");
        assert!(
            sectioned_result.is_ok(),
            "reviewed target must load the sectioned document: {sectioned_result:?}"
        );
    } else {
        assert_eq!(plain_result, Err(CorpusLoadError::NoFollowUnsupported { path: plain.clone() }));
        assert!(matches!(
            sectioned_result,
            Err(CorpusLoadError::NoFollowUnsupported { path }) if path == sectioned
        ));
    }
    Ok(())
}
