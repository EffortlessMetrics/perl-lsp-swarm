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

use perl_corpus::load_sectioned_corpus_document;

const GENERATED_CASE: &str = concat!(
    "==========================================\n",
    "Generated case\n",
    "==========================================\n",
    "my $value = 1;\n",
);

#[test]
fn parent_asset_identity_disambiguates_legacy_generated_section_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let first_path = root.path().join("first.txt");
    let second_path = root.path().join("second.txt");
    std::fs::write(&first_path, GENERATED_CASE)?;
    std::fs::write(&second_path, GENERATED_CASE)?;

    let first = load_sectioned_corpus_document("alpha/stable.txt", &first_path)?;
    let second = load_sectioned_corpus_document("beta/stable.txt", &second_path)?;

    // The compatibility Section ID is still a leaf-derived legacy field and may
    // collide. The structured parent-plus-section identity is authoritative.
    assert_eq!(
        first.cases[0].section.id.as_str(),
        second.cases[0].section.id.as_str()
    );
    assert_ne!(&first.cases[0].id, &second.cases[0].id);
    assert_eq!(first.cases[0].id.asset_id.as_str(), "alpha/stable.txt");
    assert_eq!(second.cases[0].id.asset_id.as_str(), "beta/stable.txt");
    Ok(())
}
