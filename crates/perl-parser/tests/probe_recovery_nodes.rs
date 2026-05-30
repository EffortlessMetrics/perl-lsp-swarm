//! Empirical probe: verify MissingStatement/Identifier/Block are never emitted.
//!
//! This test documents the finding from the builder-6 investigation of issue #915:
//! the parser only emits `MissingExpression` through error recovery; the three
//! other Missing* variants (`MissingStatement`, `MissingIdentifier`, `MissingBlock`)
//! are defined in the NodeKind enum but are never constructed by any parse path.
//!
//! These variants are deprecation candidates — they appear in `RECOVERY_KIND_NAMES`
//! and the xtask allowlist, but no code path in the parser ever constructs them.
//! Adding corpus fixtures cannot help because the parser simply never emits them.
//!
//! See: <https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/915>

use perl_parser::Parser;

fn collect_kind_names(node: &perl_parser::Node, out: &mut Vec<String>) {
    out.push(node.kind.kind_name().to_string());
    node.for_each_child(|child| collect_kind_names(child, out));
}

/// Parse a snippet with recovery and return the set of kind names found in the AST.
fn parse_kinds(code: &str) -> Vec<String> {
    let mut parser = Parser::new(code);
    let output = parser.parse_with_recovery();
    let mut kinds = Vec::new();
    collect_kind_names(&output.ast, &mut kinds);
    kinds
}

// ---------------------------------------------------------------------------
// Verified trigger for MissingExpression (control: proves the harness works)
// ---------------------------------------------------------------------------

#[test]
fn test_missing_expression_is_emitted_by_infix_recovery() {
    // `1 +` — missing RHS after infix operator — emits MissingExpression.
    // This is the only recovery variant that the parser actually constructs.
    let kinds = parse_kinds("1 +");
    assert!(
        kinds.iter().any(|k| k == "MissingExpression"),
        "Expected MissingExpression in `1 +` parse; got: {kinds:?}"
    );
}

#[test]
fn test_missing_expression_is_emitted_on_missing_init() {
    // `my $x =` — missing initializer after `=` — emits MissingExpression.
    let kinds = parse_kinds("my $x =");
    assert!(
        kinds.iter().any(|k| k == "MissingExpression"),
        "Expected MissingExpression in `my $x =` parse; got: {kinds:?}"
    );
}

// ---------------------------------------------------------------------------
// Deprecation-candidate assertions:
// MissingStatement / MissingIdentifier / MissingBlock are NEVER emitted.
// ---------------------------------------------------------------------------

/// The candidates listed in the issue spec (and additional variants we tried).
const PROBE_SNIPPETS: &[&str] = &[
    // From the issue spec
    "my $;",
    "our %;",
    "sub { my $ }",
    "if (1);",
    "while (1);",
    "for (;;)",
    "{ ; }",
    "; ;",
    // Additional malformed inputs tried during investigation
    "my $x =",
    "sub foo",
    "sub foo {",
    "if ($x)",
    "while ($x)",
    "for my $x",
    "my = 42",
    "our $",
    "local $",
    "my @",
    "my %",
    "my $ =",
    "our $;",
    // Even more aggressive attempts
    "sub {}",
    "{ my $x }",
    "if ($a) { } else",
    "foreach",
    "for",
    "if",
    "sub",
    "{",
    "}",
];

#[test]
fn test_missing_statement_never_emitted() {
    // MissingStatement is defined in NodeKind but constructed by no parser code path.
    // Verified empirically: none of the malformed snippets below trigger it.
    // This variant is a deprecation candidate — see issue #915.
    for snippet in PROBE_SNIPPETS {
        let kinds = parse_kinds(snippet);
        assert!(
            !kinds.iter().any(|k| k == "MissingStatement"),
            "Unexpected MissingStatement in snippet {snippet:?}; got kinds: {kinds:?}\n\
             If MissingStatement is now emitted, remove it from the deprecation list \
             and add a positive assertion above."
        );
    }
}

#[test]
fn test_missing_identifier_never_emitted() {
    // MissingIdentifier is defined in NodeKind but constructed by no parser code path.
    // Verified empirically: none of the malformed snippets below trigger it.
    // This variant is a deprecation candidate — see issue #915.
    for snippet in PROBE_SNIPPETS {
        let kinds = parse_kinds(snippet);
        assert!(
            !kinds.iter().any(|k| k == "MissingIdentifier"),
            "Unexpected MissingIdentifier in snippet {snippet:?}; got kinds: {kinds:?}\n\
             If MissingIdentifier is now emitted, remove it from the deprecation list \
             and add a positive assertion above."
        );
    }
}

#[test]
fn test_missing_block_never_emitted() {
    // MissingBlock is defined in NodeKind but constructed by no parser code path.
    // Verified empirically: none of the malformed snippets below trigger it.
    // This variant is a deprecation candidate — see issue #915.
    for snippet in PROBE_SNIPPETS {
        let kinds = parse_kinds(snippet);
        assert!(
            !kinds.iter().any(|k| k == "MissingBlock"),
            "Unexpected MissingBlock in snippet {snippet:?}; got kinds: {kinds:?}\n\
             If MissingBlock is now emitted, remove it from the deprecation list \
             and add a positive assertion above."
        );
    }
}

#[test]
fn test_recovery_kind_names_contains_all_three_missing_variants() {
    // Verify these variants are properly listed in RECOVERY_KIND_NAMES so the
    // xtask allowlist can exclude them from the actionable_never_seen count.
    let recovery_names = perl_parser::ast::NodeKind::RECOVERY_KIND_NAMES;
    assert!(
        recovery_names.contains(&"MissingStatement"),
        "MissingStatement must be in RECOVERY_KIND_NAMES for correct allowlisting"
    );
    assert!(
        recovery_names.contains(&"MissingIdentifier"),
        "MissingIdentifier must be in RECOVERY_KIND_NAMES for correct allowlisting"
    );
    assert!(
        recovery_names.contains(&"MissingBlock"),
        "MissingBlock must be in RECOVERY_KIND_NAMES for correct allowlisting"
    );
}
