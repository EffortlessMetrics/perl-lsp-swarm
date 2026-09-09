//! Parser-backed regression coverage for the bounded legacy version authority.

use perl_pragma::{CompileTimePragmaEnvironment, FeatureBundle};

fn snapshot_for(source: &str) -> perl_pragma::PragmaSnapshot {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = parser.parse_with_recovery().ast;
    CompileTimePragmaEnvironment::build(&ast).snapshot_at(source.len() - 1)
}

#[test]
fn reviewed_two_component_vstrings_select_named_bundles() {
    for (source, expected) in [
        ("use v5.36;", FeatureBundle::Perl5_36),
        ("use v5.42;", FeatureBundle::Perl5_42),
        ("use v5.44;", FeatureBundle::Perl5_44),
    ] {
        assert_eq!(snapshot_for(source).feature_bundle(), expected, "source: {source}");
    }
}

#[test]
fn decimal_patch_and_developer_forms_fail_closed() {
    for source in ["use 5.044;", "use 5.044001;", "use 5.043008;", "use v5.44.1;"] {
        let snapshot = snapshot_for(source);
        assert_eq!(snapshot.feature_bundle(), FeatureBundle::Unknown, "source: {source}");
        assert_eq!(snapshot.state().perl_version, None, "source: {source}");
    }
}

#[test]
fn malformed_version_after_known_declaration_clears_stale_authority() {
    let snapshot = snapshot_for("use v5.44; use v5.bad;");
    assert_eq!(snapshot.feature_bundle(), FeatureBundle::Unknown);
    assert_eq!(snapshot.state().perl_version, None);
}
