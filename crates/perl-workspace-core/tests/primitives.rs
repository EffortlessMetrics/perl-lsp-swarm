//! Public-surface guarantees for the substrate primitives.
//!
//! These exercise the crate through its public API (as a downstream consumer
//! would) to lock the invariants the substrate promises: repo-relative-only
//! paths, deterministic identity, stable digests, honest limitations, and
//! fact-class selection.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use perl_workspace_core::{
    Confidence, DynamicBoundary, DynamicBoundaryKind, FactClasses, FileRole, ModelLimitation,
    ParseStatus, PathError, Provenance, RepoRelativePath, SourceDigest, SourceRange, SymbolId,
    classify_role, file_id_for,
};

#[test]
fn absolute_paths_never_enter_the_substrate() {
    for abs in [
        "/home/user/lib/Foo.pm",
        "/etc/passwd",
        "C:\\Users\\me\\Foo.pm",
        "C:/Users/me/Foo.pm",
        "\\\\server\\share\\Foo.pm",
    ] {
        assert!(
            matches!(RepoRelativePath::new(abs), Err(PathError::Absolute(_))),
            "expected {abs:?} to be rejected as absolute",
        );
    }
}

#[test]
fn traversal_is_rejected() {
    assert!(matches!(RepoRelativePath::new("lib/../../secret"), Err(PathError::Traversal(_))));
}

#[test]
fn file_ids_are_deterministic_across_runs() {
    let p = RepoRelativePath::new("lib/Foo/Bar.pm").expect("valid");
    let ids: Vec<_> = (0..5).map(|_| file_id_for(&p)).collect();
    assert!(ids.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn symbol_ids_are_deterministic_and_discriminating() {
    let f = file_id_for(&RepoRelativePath::new("lib/Foo.pm").expect("valid"));
    let a = SymbolId::derive(f, Some("Foo"), "new", "Method", 100, 240);
    let b = SymbolId::derive(f, Some("Foo"), "new", "Method", 100, 240);
    assert_eq!(a, b, "same coordinates must yield the same id");

    let different_kind = SymbolId::derive(f, Some("Foo"), "new", "Subroutine", 100, 240);
    assert_ne!(a, different_kind, "kind must participate in identity");
}

#[test]
fn digests_are_stable_and_content_sensitive() {
    let a = SourceDigest::of_str("package Foo;\n1;\n");
    let b = SourceDigest::of_str("package Foo;\n1;\n");
    let c = SourceDigest::of_str("package Foo;\n2;\n");
    assert_eq!(a, b);
    assert_ne!(a, c);
    // The wire form is self-describing and fixed-width.
    assert!(a.to_hex().starts_with("fnv1a64:"));
    assert_eq!(a.to_hex().len(), "fnv1a64:".len() + 16);
}

#[test]
fn file_role_classification_is_total_and_deterministic() {
    let cases = [
        ("lib/Foo/Bar.pm", FileRole::Lib),
        ("t/basic.t", FileRole::Test),
        ("bin/tool", FileRole::Script),
        ("Makefile.PL", FileRole::DistMetadata),
        ("lib/Foo.pod", FileRole::Pod),
        ("blib/lib/Foo.pm", FileRole::Generated),
        ("README.md", FileRole::Unknown),
    ];
    for (path, expected) in cases {
        let p = RepoRelativePath::new(path).expect("valid");
        assert_eq!(classify_role(&p), expected, "role mismatch for {path}");
    }
}

#[test]
fn fact_classes_support_selective_requests() {
    // A caller wanting only file + symbol facts must be able to express that,
    // and a producer must be able to tell that POD is not requested.
    let requested = FactClasses::FILES | FactClasses::SYMBOLS;
    assert!(requested.contains(FactClasses::FILES));
    assert!(requested.contains(FactClasses::SYMBOLS));
    assert!(!requested.contains(FactClasses::POD));
    assert!(!requested.intersects(FactClasses::POD | FactClasses::DIST));
}

#[test]
fn limitations_and_boundaries_express_uncertainty() {
    let f = file_id_for(&RepoRelativePath::new("lib/Dyn.pm").expect("valid"));
    let boundary = DynamicBoundary::at(
        DynamicBoundaryKind::StringEval,
        f,
        SourceRange::new(42, 90),
        "string eval; symbol set not statically known",
    );
    assert_eq!(boundary.kind, DynamicBoundaryKind::StringEval);

    let limitation = ModelLimitation::in_file(
        "pod.malformed",
        "unterminated =over cannot be attributed to an owner",
        f,
    );
    assert_eq!(limitation.code, "pod.malformed");

    // Provenance/Confidence come from the shared vocabulary, re-exported here.
    let _: Provenance = Provenance::DynamicBoundary;
    let _: Confidence = Confidence::Low;
    let _: ParseStatus = ParseStatus::Recovered;
}
