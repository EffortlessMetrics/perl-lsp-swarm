use perl_corpus::{
    CorpusAsset, CorpusAssetPath, CorpusAssetPathError, CorpusPaths, CorpusTopology,
    CorpusTopologyError,
};
use std::fs;
use std::path::Path;

#[test]
fn fixed_portable_vector_round_trips_transparently_through_serde()
-> Result<(), Box<dyn std::error::Error>> {
    let path = CorpusAssetPath::parse("test_corpus/a/b.pl")?;

    assert_eq!(path.as_str(), "test_corpus/a/b.pl");
    assert_eq!(path.to_string(), "test_corpus/a/b.pl");
    assert_eq!(serde_json::to_string(&path)?, r#""test_corpus/a/b.pl""#);
    assert_eq!(serde_json::from_str::<CorpusAssetPath>(r#""test_corpus/a/b.pl""#)?, path);
    Ok(())
}

#[test]
fn portable_parser_treats_backslash_as_data_not_a_separator()
-> Result<(), Box<dyn std::error::Error>> {
    let literal = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;
    let nested = CorpusAssetPath::parse("test_corpus/a/b.pl")?;

    assert_ne!(literal, nested);
    assert_eq!(literal.components().collect::<Vec<_>>(), vec!["test_corpus", r"a\b.pl"]);
    assert_eq!(nested.components().collect::<Vec<_>>(), vec!["test_corpus", "a", "b.pl"]);
    Ok(())
}

#[test]
fn portable_parser_rejects_noncanonical_and_traversing_vectors_with_stable_reasons() {
    let vectors = [
        ("", "empty"),
        ("/tmp/case.pl", "absolute_or_prefixed"),
        ("C:/case.pl", "absolute_or_prefixed"),
        (r"\\server\share\case.pl", "absolute_or_prefixed"),
        (r"\\?\C:\case.pl", "absolute_or_prefixed"),
        ("test_corpus/", "non_canonical_serialization"),
        ("test_corpus//case.pl", "non_canonical_serialization"),
        ("test_corpus/./case.pl", "current_component"),
        ("test_corpus/../case.pl", "parent_component"),
    ];

    for (input, expected_reason) in vectors {
        let result = CorpusAssetPath::parse(input);
        assert_eq!(result.as_ref().map_err(CorpusAssetPathError::reason), Err(expected_reason));
    }
}

#[test]
fn component_constructor_rejects_empty_and_embedded_separator_components() {
    assert_eq!(
        CorpusAssetPath::try_from_components(["test_corpus", "", "case.pl"]),
        Err(CorpusAssetPathError::EmptyComponent { index: 1 })
    );
    assert_eq!(
        CorpusAssetPath::try_from_components(["test_corpus", "nested/case.pl"]),
        Err(CorpusAssetPathError::SeparatorInComponent { index: 1 })
    );
}

#[test]
fn host_components_and_portable_parsing_converge_when_representable()
-> Result<(), Box<dyn std::error::Error>> {
    let host = Path::new("test_corpus").join("nested").join("case.pl");
    let from_host = CorpusAssetPath::from_host_path(&host)?;
    let from_portable = CorpusAssetPath::parse("test_corpus/nested/case.pl")?;

    assert_eq!(from_host, from_portable);
    assert_eq!(from_portable.to_host_path()?, host);
    Ok(())
}

#[cfg(unix)]
#[test]
fn literal_backslash_component_materializes_injectively_on_unix()
-> Result<(), Box<dyn std::error::Error>> {
    let portable = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;
    let host = portable.to_host_path()?;

    assert_eq!(CorpusAssetPath::from_host_path(&host)?, portable);
    assert_eq!(host, Path::new("test_corpus").join(r"a\b.pl"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn literal_backslash_component_is_explicitly_unsupported_on_windows()
-> Result<(), Box<dyn std::error::Error>> {
    let portable = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;

    assert_eq!(portable.to_host_path(), Err(CorpusAssetPathError::UnsupportedOnHost));
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_host_component_fails_explicitly() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path =
        Path::new("test_corpus").join(OsString::from_vec(vec![b'b', 0xff, b'.', b'p', b'l']));
    assert!(matches!(
        CorpusAssetPath::from_host_path(&path),
        Err(CorpusAssetPathError::NonUtf8Component { index: 1 })
    ));
}

#[test]
fn topology_membership_and_portable_shape_remain_distinct_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("test_corpus"))?;
    fs::create_dir_all(root.path().join("crates/perl-corpus/fuzz"))?;
    fs::write(root.path().join("test_corpus/included.pl"), "1;\n")?;

    let topology = CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()))?;
    let included = topology
        .assets
        .first()
        .ok_or_else(|| std::io::Error::other("expected one discovered corpus asset"))?;
    assert_eq!(included.portable_path()?.as_str(), "test_corpus/included.pl");
    assert_eq!(topology.member_path(included)?.as_str(), "test_corpus/included.pl");

    let outsider: CorpusAsset = serde_json::from_value(serde_json::json!({
        "id": "test_corpus/outsider.pl",
        "layer": "test_corpus",
        "kind": "perl_source",
        "relative_path": "test_corpus/outsider.pl",
        "requirement": "required"
    }))?;
    assert_eq!(outsider.portable_path()?.as_str(), "test_corpus/outsider.pl");
    assert_eq!(
        topology.member_path(&outsider),
        Err(CorpusTopologyError::AssetNotInTopology { id: "test_corpus/outsider.pl".to_owned() })
    );
    Ok(())
}
