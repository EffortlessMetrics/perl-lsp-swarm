//! Parser-backed coverage for the bounded declaration profile and its limitations.

use std::collections::BTreeSet;
use std::error::Error;

use perl_pragma::{
    CompileTimePragmaEnvironment, FeatureBundle, PerlVersion, PragmaSnapshot, PragmaState,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn check(condition: bool, message: impl Into<String>) -> TestResult {
    if condition { Ok(()) } else { Err(message.into().into()) }
}

fn environment_for(source: &str) -> TestResult<CompileTimePragmaEnvironment> {
    let ast = perl_parser_core::Parser::new(source).parse()?;
    Ok(CompileTimePragmaEnvironment::build(&ast))
}

fn snapshot_for(source: &str) -> TestResult<PragmaSnapshot> {
    Ok(environment_for(source)?.snapshot_at(source.len().saturating_sub(1)))
}

fn recovered_snapshot_for(source: &str) -> TestResult<PragmaSnapshot> {
    let ast = perl_parser_core::Parser::new(source).parse_with_recovery().ast;
    let environment = CompileTimePragmaEnvironment::build(&ast);
    check(
        environment
            .as_map()
            .iter()
            .any(|(_, snapshot)| snapshot.state().perl_version == Some(PerlVersion::new(5, 44))),
        format!("recovery lost the known declaration preceding the malformed input: {source}"),
    )?;
    Ok(environment.snapshot_at(source.len().saturating_sub(1)))
}

fn check_unknown(snapshot: &PragmaSnapshot, source: &str) -> TestResult {
    check(
        snapshot.feature_bundle() == FeatureBundle::Unknown
            && snapshot.state().perl_version.is_none(),
        format!("unsupported declaration retained authority: {source}"),
    )
}

#[test]
fn stable_profiles_have_distinct_identity_and_the_reviewed_membership() -> TestResult {
    // Independent membership oracle: perldoc feature, FEATURE BUNDLES, Perl 5.44.
    let expected: BTreeSet<_> = [
        "bitwise",
        "current_sub",
        "evalbytes",
        "fc",
        "isa",
        "module_true",
        "postderef_qq",
        "say",
        "signatures",
        "state",
        "try",
        "unicode_eval",
        "unicode_strings",
    ]
    .into_iter()
    .collect();
    for (source, profile) in
        [("use v5.42;", FeatureBundle::Perl5_42), ("use v5.44;", FeatureBundle::Perl5_44)]
    {
        let snapshot = snapshot_for(source)?;
        let actual: BTreeSet<_> = snapshot.state().features.iter().copied().collect();
        check(snapshot.feature_bundle() == profile, format!("wrong named profile: {source}"))?;
        check(
            actual == expected && snapshot.state().features.len() == expected.len(),
            format!("wrong or duplicated implicit feature membership: {source}"),
        )?;
        check(!snapshot.has_feature("enhanced_xx"), format!("implicit enhanced_xx: {source}"))?;
        check(
            snapshot.strict_enabled() && snapshot.warnings_enabled(),
            format!("missing implicit strict/warnings: {source}"),
        )?;
    }
    Ok(())
}

#[test]
fn older_stable_aliases_retain_legacy_admission_without_claiming_a_new_named_profile() -> TestResult
{
    for minor in [0, 6, 8, 10, 14, 18, 20, 22, 26, 30, 32, 36, 40] {
        let source = format!("use v5.{minor};");
        let snapshot = snapshot_for(&source)?;
        check(
            snapshot.state().perl_version == Some(PerlVersion::new(5, minor)),
            format!("stable alias lost admission: {source}"),
        )?;
        check(
            snapshot.feature_bundle() == FeatureBundle::Unknown,
            format!("slice invented a named profile: {source}"),
        )?;
    }
    Ok(())
}

#[test]
fn unsupported_literals_do_not_admit_or_reuse_an_earlier_profile() -> TestResult {
    for declaration in [
        "use 5.044;",
        "use 5.044001;",
        "use 5.043008;",
        "use 5.043_008;",
        "use 5.0441;",
        "use 5.04308;",
        "use 5.44001;",
        "use 5.44_1;",
        "use 5.42;",
        "use 5.036;",
        "use 5.36;",
        "use 5.036000;",
        "use v5;",
        "use v5.43;",
        "use v5.46;",
        "use v6.0;",
        "use v5.44.1;",
        "use v5.44.0;",
    ] {
        for prefix in ["", "use v5.44; "] {
            let source = format!("{prefix}{declaration}");
            check_unknown(&snapshot_for(&source)?, &source)?;
        }
    }
    Ok(())
}

#[test]
fn unsupported_admission_does_not_silently_rewrite_legacy_use_semantics() -> TestResult {
    for source in ["use 5.044;", "use 5.044001;", "use 5.043008;", "use v5.44.1;"] {
        let snapshot = snapshot_for(source)?;
        check_unknown(&snapshot, source)?;
        check(
            snapshot.strict_enabled() && snapshot.warnings_enabled(),
            format!("lost existing use strict/warnings projection: {source}"),
        )?;
        check(
            snapshot.has_feature("say")
                && snapshot.has_feature("signatures")
                && snapshot.has_feature("try")
                && !snapshot.has_feature("smartmatch")
                && !snapshot.has_feature("enhanced_xx"),
            format!("changed broad compatibility features: {source}"),
        )?;
    }
    Ok(())
}

#[test]
fn malformed_recovered_declarations_cannot_reuse_known_authority() -> TestResult {
    for tail in
        ["use v5.bad;", "use v5.44.;", "use v5.44.1.2;", "use v5.44.1.;", "use v5.44 qw(foo);"]
    {
        let source = format!("use v5.44; {tail}");
        check_unknown(&recovered_snapshot_for(&source)?, &source)?;
    }
    Ok(())
}

#[test]
fn ordinary_module_imports_preserve_retained_authority() -> TestResult {
    for module in [
        "Foo",
        "v5::Foo",
        "v5Foo",
        "if $enabled, Foo, v5.44, extra",
        "unless $enabled, Foo, v5.44, extra",
    ] {
        let source = format!("use v5.42; use {module};");
        let snapshot = snapshot_for(&source)?;
        check(
            snapshot.state().perl_version == Some(PerlVersion::new(5, 42))
                && snapshot.feature_bundle() == FeatureBundle::Perl5_42,
            format!("ordinary module invalidated a declaration: {source}"),
        )?;
    }
    Ok(())
}

#[test]
fn conditional_version_targets_never_establish_unconditional_authority() -> TestResult {
    for declaration in [
        "use if 0, v5.44;",
        "use if 1, v5.44;",
        "use if $enabled, v5.44;",
        "use unless 1, v5.44;",
        "use unless 0, v5.44;",
        "use unless $enabled, v5.44;",
        "use if $enabled, v5.bad;",
        "use unless $enabled, v5.bad;",
        // Flattened arguments cannot distinguish this import argument from a
        // version target. Unknown avoids manufacturing unconditional authority.
        "use if $enabled, Foo, v5.44;",
        "use unless $enabled, Foo, v5.44;",
    ] {
        for prefix in ["", "use v5.42; "] {
            let source = format!("{prefix}{declaration}");
            check_unknown(&snapshot_for(&source)?, &source)?;
        }
    }
    Ok(())
}

#[test]
fn require_version_leaves_the_entire_lexical_state_unchanged() -> TestResult {
    for prefix in ["", "use v5.42; use feature 'enhanced_xx'; no warnings; use utf8; "] {
        let before = snapshot_for(&format!("{prefix}1;"))?;
        for requirement in ["require v5.44;", "require 5.044001;"] {
            let source = format!("{prefix}{requirement}");
            let after = snapshot_for(&source)?;
            check(
                after.state() == before.state(),
                format!("require changed lexical state: {source}"),
            )?;
            if prefix.is_empty() {
                check(
                    environment_for(&source)?.as_map().is_empty(),
                    format!("pure requirement emitted a lexical transition: {source}"),
                )?;
            }
        }
    }
    Ok(())
}

#[test]
fn nested_known_unknown_and_runtime_require_states_restore_the_outer_profile() -> TestResult {
    for (inner, profile, version) in [
        ("use v5.44;", FeatureBundle::Perl5_44, Some(PerlVersion::new(5, 44))),
        ("use 5.044001;", FeatureBundle::Unknown, None),
        ("require v5.44;", FeatureBundle::Perl5_42, Some(PerlVersion::new(5, 42))),
    ] {
        let source = format!("use v5.42; {{ {inner} my $inside; }} my $outside;");
        let environment = environment_for(&source)?;
        let inside = source.find("$inside").ok_or("missing inner marker")?;
        let outside = source.find("$outside").ok_or("missing outer marker")?;
        let inner_snapshot = environment.snapshot_at(inside);
        let outer_snapshot = environment.snapshot_at(outside);
        check(
            inner_snapshot.feature_bundle() == profile
                && inner_snapshot.state().perl_version == version,
            format!("wrong inner authority: {source}"),
        )?;
        check(
            outer_snapshot.feature_bundle() == FeatureBundle::Perl5_42
                && outer_snapshot.state().perl_version == Some(PerlVersion::new(5, 42)),
            format!("inner authority leaked out of scope: {source}"),
        )?;
    }
    Ok(())
}

#[test]
fn enhanced_xx_is_explicit_and_does_not_manufacture_declaration_authority() -> TestResult {
    for (prefix, profile) in [
        ("", FeatureBundle::Unknown),
        ("use v5.42; ", FeatureBundle::Perl5_42),
        ("use v5.44; ", FeatureBundle::Perl5_44),
    ] {
        for enable in
            ["use feature 'enhanced_xx';", "use feature ':all';", "use experimental 'enhanced_xx';"]
        {
            let source = format!("{prefix}{enable}");
            let enabled = snapshot_for(&source)?;
            check(
                enabled.has_feature("enhanced_xx") && enabled.feature_bundle() == profile,
                format!("explicit feature/profile distinction failed: {source}"),
            )?;
            for disable in [
                "no feature 'enhanced_xx';",
                "no feature ':all';",
                "no experimental 'enhanced_xx';",
            ] {
                let source = format!("{source} {disable}");
                let disabled = snapshot_for(&source)?;
                check(
                    !disabled.has_feature("enhanced_xx") && disabled.feature_bundle() == profile,
                    format!("feature disable changed declaration or retained feature: {source}"),
                )?;
            }
        }
    }
    Ok(())
}

#[test]
fn named_profile_is_a_state_projection_not_feature_vector_attestation() -> TestResult {
    let mut state =
        PragmaState { perl_version: Some(PerlVersion::new(5, 44)), ..PragmaState::default() };
    state.features.clear();
    let snapshot = PragmaSnapshot::from_state(state);
    check(
        snapshot.feature_bundle() == FeatureBundle::Perl5_44,
        "manual state projection unexpectedly depended on feature membership",
    )?;
    let snapshot = snapshot_for("use v5.44; no feature 'say';")?;
    check(
        !snapshot.has_feature("say") && snapshot.feature_bundle() == FeatureBundle::Perl5_44,
        "explicit feature override lost the selected declaration",
    )
}
