//! Distribution and external-root contract for the published perl-corpus crate.

use perl_corpus::{CorpusPaths, CorpusRootError, CorpusRootSource};
use std::fs;

#[test]
fn package_manifest_declares_external_repository_assets()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(manifest_path)?;
    let parsed: toml::Value = toml::from_str(&manifest)?;
    let metadata = parsed
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("perl-corpus"))
        .ok_or_else(|| std::io::Error::other("missing package.metadata.perl-corpus"))?;

    assert_eq!(
        metadata.get("repository-assets").and_then(toml::Value::as_str),
        Some("external-root")
    );
    assert_eq!(
        metadata.get("root-environment").and_then(toml::Value::as_str),
        Some("PERL_CORPUS_ROOT")
    );

    let include = parsed
        .get("package")
        .and_then(|package| package.get("include"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing package include list"))?;
    let included: Vec<_> = include.iter().filter_map(toml::Value::as_str).collect();
    assert!(!included.iter().any(|entry| entry.starts_with("test_corpus")));
    assert!(!included.iter().any(|entry| entry.starts_with("fuzz")));
    Ok(())
}

#[test]
fn isolated_package_root_does_not_become_empty_repository_corpus()
-> Result<(), Box<dyn std::error::Error>> {
    let package_root = tempfile::tempdir()?;
    fs::create_dir_all(package_root.path().join("src"))?;
    fs::write(package_root.path().join("Cargo.toml"), "[package]\nname='isolated'\n")?;

    let paths = CorpusPaths::try_from_root(package_root.path())?;
    assert_eq!(paths.root_source(), CorpusRootSource::Explicit);
    assert!(matches!(
        paths.require_repository_layout(),
        Err(CorpusRootError::RequiredLayerMissing { layer: "test_corpus", .. })
    ));
    Ok(())
}
