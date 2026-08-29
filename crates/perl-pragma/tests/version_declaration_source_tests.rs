//! Source-level regressions for lexical `use VERSION` authority.
//!
//! These tests parse real Perl source so the declared version components must
//! survive the actual parser representation, not only hand-built AST nodes.
//! `v5.44.1` must keep its patch component, and the decimal form `5.044001`
//! must normalize to 5.44.1 instead of reading as minor 44001.

use perl_pragma::{CompileTimePragmaEnvironment, PerlVersion, features_enabled_by_version};

fn snapshot_for(source: &str) -> perl_pragma::PragmaSnapshot {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = parser.parse_with_recovery().ast;
    let environment = CompileTimePragmaEnvironment::build(&ast);
    environment.snapshot_at(source.len() - 1)
}

#[test]
fn v_string_patch_component_survives_source_tracking() -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = snapshot_for("use v5.44.1;");
    assert_eq!(
        snapshot.perl_version(),
        Some(PerlVersion::with_patch(5, 44, 1)),
        "the declared v5.44.1 patch component must remain the tracked authority"
    );
    Ok(())
}

#[test]
fn decimal_fraction_groups_normalize_in_source_tracking() -> Result<(), Box<dyn std::error::Error>>
{
    let snapshot = snapshot_for("use 5.044001;");
    assert_eq!(
        snapshot.perl_version(),
        Some(PerlVersion::with_patch(5, 44, 1)),
        "5.044001 is Perl decimal notation for 5.44.1, not minor 44001"
    );

    let developer = snapshot_for("use 5.043_008;");
    assert_eq!(
        developer.perl_version(),
        Some(PerlVersion::with_patch(5, 43, 8)),
        "the developer release component must survive source tracking"
    );
    Ok(())
}

#[test]
fn decimal_developer_release_orders_below_next_stable_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let snapshot = snapshot_for("use 5.043008;");
    let version = snapshot.perl_version().ok_or("5.043008 must track a version")?;
    assert_eq!(version, PerlVersion::with_patch(5, 43, 8));
    assert!(
        version < PerlVersion::new(5, 44),
        "5.43.8 must not order as newer than the 5.44 bundle authority"
    );
    assert_eq!(
        features_enabled_by_version(version),
        features_enabled_by_version(PerlVersion::new(5, 42)),
        "5.43.8 must select the 5.42 feature bundle"
    );
    Ok(())
}

#[test]
fn two_component_forms_keep_their_stable_reading() -> Result<(), Box<dyn std::error::Error>> {
    for (source, expected) in [
        ("use v5.44;", PerlVersion::new(5, 44)),
        ("use 5.044;", PerlVersion::new(5, 44)),
        ("use v5.36;", PerlVersion::new(5, 36)),
    ] {
        let snapshot = snapshot_for(source);
        assert_eq!(snapshot.perl_version(), Some(expected), "source: {source}");
    }
    Ok(())
}
