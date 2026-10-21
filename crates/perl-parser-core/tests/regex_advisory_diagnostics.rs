//! Parser severity contract for valid regex-risk diagnostics.

use perl_parser_core::{NodeKind, ParseDiagnosticSeverity, Parser};

#[test]
fn nested_quantifier_is_advisory_on_a_clean_ast() -> Result<(), String> {
    let source = r#""abab" =~ /(?:[^b]*(?=(b)|(a))ab)*/;"#;
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();

    assert!(matches!(output.ast.kind, NodeKind::Program { .. }));
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity() == ParseDiagnosticSeverity::Advisory
                && diagnostic.to_string().contains("Nested quantifiers detected")
        }),
        "expected visible advisory diagnostic, got {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|diagnostic| !diagnostic.blocks_clean_parse()),
        "nested quantifier must not block a clean parse receipt"
    );
    Ok(())
}

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
fn malformed_source_remains_blocking() -> Result<(), String> {
    let mut parser = Parser::new("my $value = ;");
    let output = parser.parse_with_recovery();

    assert!(
        output.diagnostics.iter().any(|diagnostic| diagnostic.blocks_clean_parse()),
        "malformed source must retain a blocking diagnostic"
    );
    Ok(())
}
