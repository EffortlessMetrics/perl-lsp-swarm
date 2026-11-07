//! Quantifier-follows-nothing diagnostics for issue #16608.
//!
//! Moved into a dedicated file per `crates/perl-parser-core/CLAUDE.md`:
//! new tests go in a new file under `tests/` rather than appending to
//! existing test files.

use std::error::Error;

use perl_parser_core::{Parser, parse_source_with_regex_analysis};

#[test]
fn quantifier_follows_nothing_is_blocking_on_the_legacy_path() -> Result<(), String> {
    // Issue #16608 class (1) without a retained session: the legacy
    // per-operator scan must name the same Perl diagnostic and block.
    let source = "my $s = 'x'; my $m = $s =~ /*abc/;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();

    let expected = source.find('*').ok_or("quantifier not found")?;
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic.location() == Some(expected)
                && diagnostic.blocks_clean_parse()
                && diagnostic.to_string().contains("Quantifier follows nothing in regex")
        }),
        "expected blocking quantifier-follows-nothing at {expected}, got {:?}",
        output.diagnostics
    );
    Ok(())
}

#[test]
fn quantifier_follows_nothing_is_blocking_in_retained_analysis() -> Result<(), Box<dyn Error>> {
    // Issue #16608 class (1): perl rejects each of these with
    // `Quantifier follows nothing in regex`; the retained scan must too.
    for (source, needle) in [
        ("my $s = 'x'; my $m = $s =~ /*abc/;", "*abc"),
        ("my $s = 'x'; my $m = $s =~ /+abc/;", "+abc"),
        ("my $s = 'x'; my $m = $s =~ /a|*b/;", "*b"),
        ("my $s = 'x'; my $m = $s =~ /(?:*a)/;", "*a"),
        ("my $r = qr/*abc/;", "*abc"),
        ("my $r = qr{*abc};", "*abc"),
        ("my $s = 'x'; $s =~ m{*abc};", "*abc"),
        ("my $s = 'x'; $s =~ s/*abc/x/;", "*abc"),
    ] {
        let output = parse_source_with_regex_analysis(source);
        let expected = source.find(needle).ok_or("needle not found")?;
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                diagnostic.location() == Some(expected)
                    && diagnostic.blocks_clean_parse()
                    && diagnostic.to_string().contains("Quantifier follows nothing in regex")
            }),
            "{source:?} must carry a blocking quantifier-follows-nothing diagnostic at {expected}, got {:?}",
            output.diagnostics
        );
        let record = output.regex_analysis.records.first().ok_or("missing regex record")?;
        let pattern = record.pattern.as_ref().ok_or("missing pattern analysis")?;
        assert!(
            pattern.structural.diagnostics.iter().any(|diagnostic| {
                diagnostic.code
                    == perl_regex::validator::RegexDiagnosticCode::QuantifierFollowsNothing
            }),
            "{source:?} retained analysis must carry the typed code"
        );
    }
    Ok(())
}

#[test]
fn quantifier_follows_nothing_after_ignored_x_whitespace_is_blocking_on_both_paths()
-> Result<(), Box<dyn Error>> {
    // Perl oracle: `/ *a/x` is `Quantifier follows nothing in regex`
    // because /x ignores the leading space, so the `*` has no atom. The
    // legacy per-operator scan must agree with the retained scan here;
    // analyzing the body with default modifiers would mistake the space
    // for an atom and stay silent.
    let source = "my $s = 'x'; my $m = $s =~ / *a/x;";
    let expected = source.find('*').ok_or("quantifier not found")?;

    let mut parser = Parser::new(source);
    let legacy = parser.parse_with_recovery();
    assert!(
        legacy.diagnostics.iter().any(|diagnostic| {
            diagnostic.location() == Some(expected)
                && diagnostic.blocks_clean_parse()
                && diagnostic.to_string().contains("Quantifier follows nothing in regex")
        }),
        "legacy path must flag / *a/x at {expected}, got {:?}",
        legacy.diagnostics
    );

    let retained = parse_source_with_regex_analysis(source);
    assert!(
        retained.diagnostics.iter().any(|diagnostic| {
            diagnostic.location() == Some(expected)
                && diagnostic.blocks_clean_parse()
                && diagnostic.to_string().contains("Quantifier follows nothing in regex")
        }),
        "retained path must flag / *a/x at {expected}, got {:?}",
        retained.diagnostics
    );
    Ok(())
}

#[test]
fn escaped_space_is_an_atom_under_x_on_both_paths() -> Result<(), Box<dyn Error>> {
    // In /x, `\ ` is a literal space atom, so the `*` after it quantifies
    // that atom and the pattern compiles. Neither path may flag it.
    let source = "my $s = 'x'; my $m = $s =~ /\\ *a/x;";
    let mut parser = Parser::new(source);
    let legacy = parser.parse_with_recovery();
    assert!(
        !legacy
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.to_string().contains("Quantifier follows nothing")),
        "legacy path must not flag an escaped-space atom, got {:?}",
        legacy.diagnostics
    );
    let retained = parse_source_with_regex_analysis(source);
    assert!(
        !retained
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.to_string().contains("Quantifier follows nothing")),
        "retained path must not flag an escaped-space atom, got {:?}",
        retained.diagnostics
    );
    Ok(())
}

#[test]
fn quantifier_follows_nothing_guards_valid_patterns_in_retained_analysis()
-> Result<(), Box<dyn Error>> {
    // Verified against perl 5.42: every source below compiles clean.
    for source in [
        "my $s = 'x'; my $m = $s =~ /[*+?]/;",
        "my $s = 'x'; my $m = $s =~ /a*?b/;",
        "my $s = 'x'; my $m = $s =~ /a*+b/;",
        "my $s = 'x'; my $m = $s =~ /(?:a|b)*c/;",
        "my $s = 'x'; my $m = $s =~ /{2}abc/;",
        "my $s = 'x'; my $m = $s =~ /\\*abc/;",
        "my $s = 'x'; my $m = $s =~ /(*COMMIT)a/;",
        "my $s = 'x'; my $m = $s =~ s/a/*b/;",
    ] {
        let output = parse_source_with_regex_analysis(source);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.to_string().contains("Quantifier follows nothing")),
            "{source:?} must not be flagged, got {:?}",
            output.diagnostics
        );
    }
    Ok(())
}
