use perl_corpus::{
    CorpusLoadError, SectionedCorpusLoadError, load_plain_perl_source,
    load_sectioned_corpus_document,
};
use std::fs;

#[test]
fn public_section_loader_rejects_partial_malformed_population()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("partial.txt");
    fs::write(
        &path,
        "====\nValid\n====\nmy $value = 1;\n====\nBroken\nmy $value = 2;\n",
    )?;

    assert!(matches!(
        load_sectioned_corpus_document("fixtures/partial.txt", &path),
        Err(SectionedCorpusLoadError::MalformedHeader {
            line: 5,
            reason: "missing_closing_delimiter",
            ..
        })
    ));
    Ok(())
}

#[test]
fn public_loaders_reject_whitespace_only_identity_before_reading()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.txt");

    assert_eq!(
        load_plain_perl_source("  ", &missing),
        Err(CorpusLoadError::EmptyAssetId)
    );
    assert!(matches!(
        load_sectioned_corpus_document("\t", &missing),
        Err(SectionedCorpusLoadError::Source(CorpusLoadError::EmptyAssetId))
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
