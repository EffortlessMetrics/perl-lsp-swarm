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

use perl_corpus::{
    load_plain_perl_source, load_sectioned_corpus_document, NewlineStyle,
};
use std::path::Path;

#[test]
fn checked_in_plain_and_sectioned_assets_use_distinct_loaders()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/loading");
    let plain_path = root.join("plain_delimiters.pl");
    let sectioned_path = root.join("sectioned.txt");

    let plain = load_plain_perl_source("fixtures/loading/plain_delimiters.pl", &plain_path)?;
    assert_eq!(plain.newline_style, NewlineStyle::Lf);
    assert!(plain.source.contains("=========================================="));
    assert!(plain.source.contains("---"));

    let sectioned =
        load_sectioned_corpus_document("fixtures/loading/sectioned.txt", &sectioned_path)?;
    assert_eq!(sectioned.cases.len(), 2);
    assert_eq!(
        sectioned.cases[0].id.asset_id,
        "fixtures/loading/sectioned.txt"
    );
    assert_eq!(sectioned.cases[0].id.section_id, "checked.first");
    assert_eq!(sectioned.cases[1].id.section_id, "checked.second");
    assert_eq!(sectioned.cases[0].section.body, "my $first = 1;");
    assert_eq!(sectioned.cases[1].section.body, "my $second = 2;");
    Ok(())
}
