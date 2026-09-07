//! The flat HIR lowerer must not adopt the parser's synthesized implicit-topic
//! operand as a bareword (#14641).
//!
//! An unbound `s///`, `tr///` or `y///` applies to `$_`. The parser materializes
//! that implicit operand as a zero-width `NodeKind::Identifier` literally named
//! `"$_"`. Before this proof existed, the flat lowerer's generic `Identifier` arm
//! walked that fabrication into both the item stream (as `HirKind::BarewordExpr`)
//! and `bareword_table` (as a `BarewordFact` claiming `ExactAst` provenance at
//! `High` confidence) — the strongest assertion the model can make, about a name
//! that no Perl source can produce and over a range covering no text.
//!
//! The guard is stated on the *name*, not on the regex families: a bareword never
//! carries a sigil. These tests pin both halves — the fabrication is dropped, and
//! every genuinely written bareword still round-trips — so a regression in either
//! direction fails.

use perl_ast::ast::{Node, NodeKind};
use perl_parser_core::Parser;
use perl_parser_core::hir::{BarewordRole, HirFile, HirKind, lower_ast};
use perl_parser_core::pir::lower_hir;

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    lower_ast(&parser.parse_with_recovery().ast)
}

fn bareword_names(source: &str) -> Vec<String> {
    lower(source).bareword_table.facts.iter().map(|fact| fact.name.clone()).collect()
}

fn bareword_expr_names(source: &str) -> Vec<String> {
    lower(source)
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::BarewordExpr(expr) => Some(expr.name.clone()),
            _ => None,
        })
        .collect()
}

/// Every unbound regex-family form the parser fabricates a `$_` operand for.
const UNBOUND_TOPIC_FORMS: &[&str] =
    &["s/a/b/;", "s{a}{b}g;", "s/a/b/e;", "tr/a/b/;", "tr/a-z/A-Z/cds;", "y/a/b/;"];

// ---------------------------------------------------------------------------
// The claim: the fabrication is not recorded.
// ---------------------------------------------------------------------------

#[test]
fn unbound_regex_forms_contribute_no_bareword_fact() {
    for source in UNBOUND_TOPIC_FORMS {
        assert!(
            bareword_names(source).is_empty(),
            "{source:?} recorded bareword facts {:?}; the synthesized `$_` topic is not a bareword",
            bareword_names(source),
        );
    }
}

#[test]
fn unbound_regex_forms_contribute_no_bareword_item() {
    for source in UNBOUND_TOPIC_FORMS {
        assert!(
            bareword_expr_names(source).is_empty(),
            "{source:?} emitted BarewordExpr items {:?}; the same false claim must not survive \
             in the item stream, which `pir::lower` keys unsupported-construct counts on",
            bareword_expr_names(source),
        );
    }
}

/// The two forms named verbatim in #14641's reproduction, including the
/// two-statement case that produced one fabricated fact per statement.
#[test]
fn issue_reproduction_is_clean() {
    assert_eq!(bareword_names("s/a/b/; tr/a/b/;"), Vec::<String>::new());
}

/// Real idiomatic Perl reaches the same fabrication, so the defect was not
/// confined to a bare statement at file scope.
#[test]
fn unbound_topic_inside_common_idioms_records_nothing() {
    for source in ["map { s/a/b/; $_ } @z;", "for (@x) { s/a/b/ }", "while (<STDIN>) { tr/a/b/ }"] {
        assert!(
            !bareword_names(source).iter().any(|name| name == "$_"),
            "{source:?} recorded a `$$_` bareword fact: {:?}",
            bareword_names(source),
        );
    }
}

// ---------------------------------------------------------------------------
// Negative controls: the guard must not silence real barewords.
// ---------------------------------------------------------------------------

#[test]
fn a_written_bareword_is_still_recorded() {
    assert_eq!(bareword_names("foo;"), vec!["foo".to_string()]);
    assert_eq!(bareword_expr_names("foo;"), vec!["foo".to_string()]);
}

/// The discriminating pair: identical statement shape, one fabricated operand
/// and one written bareword. A guard that dropped both — or kept both — fails.
#[test]
fn fabricated_and_written_names_are_distinguished_in_one_file() {
    assert_eq!(bareword_names("foo; s/a/b/; bar;"), vec!["foo".to_string(), "bar".to_string()]);
}

/// The guard sits on the single arm every bareword role funnels through
/// (`visit_identifier_with_bareword_context` delegates to it), so each role is
/// checked rather than only the `Expression` one the fabrication happens to use.
#[test]
fn bareword_roles_other_than_expression_still_record() {
    let cases: &[(&str, &str, BarewordRole)] = &[
        ("foo;", "foo", BarewordRole::Expression),
        ("require Foo::Bar;", "Foo::Bar", BarewordRole::ModuleRequest),
        ("Widget->new;", "Widget", BarewordRole::MethodReceiver),
        ("My::Pkg::CONST;", "My::Pkg::CONST", BarewordRole::QualifiedName),
        ("new Foo::Bar(1);", "Foo::Bar", BarewordRole::IndirectObject),
    ];
    for (source, expected_name, expected_role) in cases {
        let file = lower(source);
        assert!(
            file.bareword_table
                .facts
                .iter()
                .any(|fact| fact.name == *expected_name && fact.role == *expected_role),
            "{source:?} lost its {expected_role:?} bareword {expected_name:?}; recorded {:?}",
            file.bareword_table.facts.iter().map(|f| (&f.name, f.role)).collect::<Vec<_>>(),
        );
    }
}

/// A bound operator never had the fabrication, and must stay that way: this
/// pins that the guard did not change the already-correct path.
#[test]
fn a_bound_operator_records_no_topic_fact() {
    for source in ["$x =~ s/a/b/;", "$x =~ tr/a/b/;", "$_ =~ s/a/b/;"] {
        assert!(
            bareword_names(source).is_empty(),
            "{source:?} recorded {:?}",
            bareword_names(source),
        );
    }
}

// ---------------------------------------------------------------------------
// Downstream effect, pinned rather than left silent.
// ---------------------------------------------------------------------------

/// `pir::lower` keys `unsupported_construct_counts` on the HIR item kind, so
/// dropping the fabricated item is receipt-visible: an unbound `s///` used to
/// contribute a phantom `BarewordExpr` to the unsupported tally. The count now
/// reflects only barewords that are in the source, and a file mixing the two
/// reports one rather than two.
#[test]
fn pir_receipt_no_longer_counts_a_phantom_bareword() {
    fn bareword_count(source: &str) -> usize {
        let graph = lower_hir(&lower(source));
        graph.receipt.unsupported_construct_counts.get("BarewordExpr").copied().unwrap_or(0)
    }

    assert_eq!(bareword_count("s/a/b/;"), 0, "unbound s/// contains no bareword to count");
    assert_eq!(bareword_count("tr/a/b/;"), 0, "unbound tr/// contains no bareword to count");
    assert_eq!(bareword_count("foo;"), 1, "a written bareword is still counted");
    assert_eq!(
        bareword_count("foo; s/a/b/;"),
        1,
        "only the written bareword counts; the fabricated topic must not inflate the tally",
    );
}

// ---------------------------------------------------------------------------
// The premise the guard rests on.
// ---------------------------------------------------------------------------

/// The guard is only as safe as its premise: that a sigil-prefixed
/// `NodeKind::Identifier` is *always* a parser fabrication and never scanned
/// source. This walks the AST of the forms most likely to falsify that —
/// symbolic dereferences, globs, `goto`, hash keys, interpolation — and pins
/// that the only sigil-prefixed identifiers produced are zero-width `$_`
/// topics. If the parser ever starts emitting a real one, this fails and the
/// guard must be revisited before it silently swallows a genuine name.
#[test]
fn no_written_source_yields_a_sigil_prefixed_identifier() {
    fn walk(node: &Node, found: &mut Vec<(String, usize, usize)>) {
        if let NodeKind::Identifier { name } = &node.kind
            && name.chars().next().is_some_and(|c| matches!(c, '$' | '@' | '%' | '&' | '*'))
        {
            found.push((name.clone(), node.location.start, node.location.end));
        }
        node.for_each_child(|child| walk(child, found));
    }

    let sources = [
        "${foo};",
        "${$x};",
        "${ $name };",
        "@{$r};",
        "%{$h};",
        "&{$c};",
        "*{$g};",
        "@{[ $x ]};",
        "my $y = ${\"name\"};",
        "$$x;",
        "@$x;",
        "*foo = \\&bar;",
        "*{$name} = sub {};",
        "&$code();",
        "&{$code}();",
        "goto &foo;",
        "goto $label;",
        "goto LABEL;",
        "$h{foo};",
        "$h{-bar};",
        "$h{$k};",
        "my %h = (foo => 1);",
        "Foo::Bar->new;",
        "print STDERR \"x\";",
        "new Foo(1);",
        "require Foo::Bar;",
        "my $s = \"$x and @y\";",
        "sort { $a <=> $b } @x;",
        "grep { /x/ } @y;",
        "local $_ = 1;",
        "/foo/;",
        "m/foo/;",
        "qr/foo/;",
    ];

    for source in sources {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        let mut found = Vec::new();
        walk(&output.ast, &mut found);
        assert!(
            found.is_empty(),
            "{source:?} produced sigil-prefixed Identifier node(s) {found:?} from written \
             source; `is_sigil_prefixed` would now discard a real name",
        );
    }

    // The converse: the fabrication really is present in the AST, so the tests
    // above are dropping something rather than asserting over an empty set.
    let mut parser = Parser::new("s/a/b/;");
    let output = parser.parse_with_recovery();
    let mut found = Vec::new();
    walk(&output.ast, &mut found);
    assert_eq!(
        found,
        vec![("$_".to_string(), 0, 0)],
        "the parser no longer fabricates a zero-width `$$_` operand; this suite's subject is gone",
    );
}
