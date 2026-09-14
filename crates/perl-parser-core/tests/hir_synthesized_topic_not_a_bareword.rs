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
//! The guard is stated on the *node*, not on the regex families: the fabrication
//! is identified by a span containing no source text (zero-width) whose name
//! could not be a bareword anyway (it carries a sigil).
//!
//! The zero-width half is the load-bearing one. The sigil alone is **not**
//! sufficient, and an earlier revision that relied on it was wrong: `new $class`
//! is a legal dynamic indirect constructor whose *written* receiver the parser
//! records as a real, nonzero-width `Identifier` named `"$class"`, which that
//! revision silently discarded.
//!
//! These tests pin three directions — the fabrication is dropped, every written
//! bareword still round-trips, and a source-backed sigil-prefixed name is
//! preserved — and separately *state the limit* that the sigil half is defence in
//! depth rather than a discriminator provable today.

use perl_ast::ast::{Node, NodeKind};
use perl_parser_core::Parser;
use perl_parser_core::hir::{BarewordRole, HirFile, HirKind, lower_ast};
use perl_parser_core::pir::lower_hir;

type TestResult = Result<(), String>;

fn require(condition: bool, message: impl FnOnce() -> String) -> TestResult {
    if condition { Ok(()) } else { Err(message()) }
}

fn require_equal<T: PartialEq + std::fmt::Debug>(
    actual: T,
    expected: T,
    message: impl FnOnce() -> String,
) -> TestResult {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{}: actual {actual:?}, expected {expected:?}", message()))
    }
}

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
fn unbound_regex_forms_contribute_no_bareword_fact() -> TestResult {
    for source in UNBOUND_TOPIC_FORMS {
        require(bareword_names(source).is_empty(), || {
            format!(
                "{source:?} recorded bareword facts {:?}; the synthesized `$_` topic is not a bareword",
                bareword_names(source)
            )
        })?;
    }
    Ok(())
}

#[test]
fn unbound_regex_forms_contribute_no_bareword_item() -> TestResult {
    for source in UNBOUND_TOPIC_FORMS {
        require(bareword_expr_names(source).is_empty(), || {
            format!(
                "{source:?} emitted BarewordExpr items {:?}; the same false claim must not survive \
             in the item stream, which `pir::lower` keys unsupported-construct counts on",
                bareword_expr_names(source)
            )
        })?;
    }
    Ok(())
}

/// The two forms named verbatim in #14641's reproduction, including the
/// two-statement case that produced one fabricated fact per statement.
#[test]
fn issue_reproduction_is_clean() -> TestResult {
    require_equal(bareword_names("s/a/b/; tr/a/b/;"), Vec::<String>::new(), String::new)?;
    Ok(())
}

/// Real idiomatic Perl reaches the same fabrication, so the defect was not
/// confined to a bare statement at file scope.
#[test]
fn unbound_topic_inside_common_idioms_records_nothing() -> TestResult {
    for source in ["map { s/a/b/; $_ } @z;", "for (@x) { s/a/b/ }", "while (<STDIN>) { tr/a/b/ }"] {
        require(!bareword_names(source).iter().any(|name| name == "$_"), || {
            format!("{source:?} recorded a `$$_` bareword fact: {:?}", bareword_names(source))
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: the guard must not silence real barewords.
// ---------------------------------------------------------------------------

#[test]
fn a_written_bareword_is_still_recorded() -> TestResult {
    require_equal(bareword_names("foo;"), vec!["foo".to_string()], String::new)?;
    require_equal(bareword_expr_names("foo;"), vec!["foo".to_string()], String::new)?;
    Ok(())
}

/// The discriminating pair: identical statement shape, one fabricated operand
/// and one written bareword. A guard that dropped both — or kept both — fails.
#[test]
fn fabricated_and_written_names_are_distinguished_in_one_file() -> TestResult {
    require_equal(
        bareword_names("foo; s/a/b/; bar;"),
        vec!["foo".to_string(), "bar".to_string()],
        String::new,
    )?;
    Ok(())
}

/// The guard sits on the single arm every bareword role funnels through
/// (`visit_identifier_with_bareword_context` delegates to it), so each role is
/// checked rather than only the `Expression` one the fabrication happens to use.
#[test]
fn bareword_roles_other_than_expression_still_record() -> TestResult {
    let cases: &[(&str, &str, BarewordRole)] = &[
        ("foo;", "foo", BarewordRole::Expression),
        ("require Foo::Bar;", "Foo::Bar", BarewordRole::ModuleRequest),
        ("Widget->new;", "Widget", BarewordRole::MethodReceiver),
        ("My::Pkg::CONST;", "My::Pkg::CONST", BarewordRole::QualifiedName),
        ("new Foo::Bar(1);", "Foo::Bar", BarewordRole::IndirectObject),
    ];
    for (source, expected_name, expected_role) in cases {
        let file = lower(source);
        require(
            file.bareword_table
                .facts
                .iter()
                .any(|fact| fact.name == *expected_name && fact.role == *expected_role),
            || {
                format!(
                    "{source:?} lost its {expected_role:?} bareword {expected_name:?}; recorded {:?}",
                    file.bareword_table.facts.iter().map(|f| (&f.name, f.role)).collect::<Vec<_>>()
                )
            },
        )?;
    }
    Ok(())
}

/// A bound operator never had the fabrication, and must stay that way: this
/// pins that the guard did not change the already-correct path.
#[test]
fn a_bound_operator_records_no_topic_fact() -> TestResult {
    for source in ["$x =~ s/a/b/;", "$x =~ tr/a/b/;", "$_ =~ s/a/b/;"] {
        require(bareword_names(source).is_empty(), || {
            format!("{source:?} recorded {:?}", bareword_names(source))
        })?;
    }
    Ok(())
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
fn pir_receipt_no_longer_counts_a_phantom_bareword() -> TestResult {
    fn bareword_count(source: &str) -> usize {
        let graph = lower_hir(&lower(source));
        graph.receipt.unsupported_construct_counts.get("BarewordExpr").copied().unwrap_or(0)
    }

    require_equal(bareword_count("s/a/b/;"), 0, || {
        "unbound s/// contains no bareword to count".to_owned()
    })?;
    require_equal(bareword_count("tr/a/b/;"), 0, || {
        "unbound tr/// contains no bareword to count".to_owned()
    })?;
    require_equal(bareword_count("foo;"), 1, || "a written bareword is still counted".to_owned())?;
    require_equal(bareword_count("foo; s/a/b/;"), 1, || {
        "only the written bareword counts; the fabricated topic must not inflate the tally"
            .to_owned()
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The premise the guard rests on.
// ---------------------------------------------------------------------------

fn sigil_prefixed_identifiers(source: &str) -> Vec<(String, usize, usize)> {
    fn walk(node: &Node, found: &mut Vec<(String, usize, usize)>) {
        if let NodeKind::Identifier { name } = &node.kind
            && name.chars().next().is_some_and(|c| matches!(c, '$' | '@' | '%' | '&' | '*'))
        {
            found.push((name.clone(), node.location.start, node.location.end));
        }
        node.for_each_child(|child| walk(child, found));
    }
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut found = Vec::new();
    walk(&output.ast, &mut found);
    found
}

/// **The sigil alone does not identify a fabrication.**
///
/// `new $class` is a legal dynamic indirect constructor, and the parser records
/// its *written* receiver as a real `Identifier { name: "$class" }` spanning the
/// characters the author typed. An earlier revision of this guard keyed on the
/// sigil alone and silently discarded that source-backed fact; the defect was
/// caught in review by Codex, and this test is the control that was missing.
///
/// The receiver keeps exactly the behavior it had before this PR — recorded, as
/// an `IndirectObject`. Whether a sigil-prefixed name should be a *bareword* at
/// all is a separate defect (#15031), deliberately not changed here.
#[test]
fn a_written_dynamic_constructor_receiver_is_preserved() -> TestResult {
    for (source, name, start, end) in [
        ("new $class;", "$class", 4usize, 10usize),
        ("my $o = new $class;", "$class", 12, 18),
        ("my $o = new $class(@args);", "$class", 12, 18),
        ("new $class::Foo;", "$class::Foo", 4, 15),
    ] {
        // The node really is source-backed: sigil-prefixed but *not* zero-width.
        require_equal(
            sigil_prefixed_identifiers(source),
            vec![(name.to_string(), start, end)],
            || format!("{source:?} no longer yields a source-backed sigil-prefixed Identifier"),
        )?;

        let file = lower(source);
        require(
            file.bareword_table.facts.iter().any(|fact| {
                fact.name == name
                    && fact.range.start == start
                    && fact.range.end == end
                    && fact.role == BarewordRole::IndirectObject
            }),
            || {
                format!(
                    "{source:?} lost its written receiver {name:?}; the guard must not discard a \
             source-backed name. Recorded: {:?}",
                    file.bareword_table.facts.iter().map(|f| (&f.name, f.role)).collect::<Vec<_>>()
                )
            },
        )?;
    }
    Ok(())
}

/// The premise the guard actually rests on: the fabricated operand is
/// distinguishable from written source by being **zero-width**, not merely by
/// carrying a sigil.
#[test]
fn the_fabrication_is_zero_width_and_written_names_are_not() -> TestResult {
    require_equal(sigil_prefixed_identifiers("s/a/b/;"), vec![("$_".to_string(), 0, 0)], || {
        "the parser no longer fabricates a zero-width `$$_` operand; this suite's subject is gone"
            .to_owned()
    })?;

    for (source, _) in [("new $class;", ()), ("my $o = new $class;", ())] {
        for (name, start, end) in sigil_prefixed_identifiers(source) {
            require(start != end, || {
                format!(
                    "{source:?} yielded a zero-width written name {name:?}; the guard's \
                 discriminator would misclassify it as synthesized (start={start}, end={end})"
                )
            })?;
        }
    }
    Ok(())
}

/// **A stated limit, not a passing claim.**
///
/// `is_synthesized_operand` requires *both* an empty range and a sigil. Only the
/// empty-range half is falsifiable by test today: mutating the guard to drop the
/// sigil condition breaks nothing, because no zero-width `Identifier` with a
/// non-sigil name was observed in the finite recovery corpus below.
///
/// This test measures that fact instead of asserting the sigil half is
/// load-bearing when it is not. It probes malformed and recovery-path inputs —
/// where a synthesized placeholder is most likely to appear — and pins that every
/// zero-width `Identifier` produced by this corpus is the `"$_"` topic. If recovery
/// produces a non-topic placeholder for one of these inputs, this test fails
/// and the sigil condition becomes a real, testable guard rather than defence in
/// depth.
#[test]
fn the_sigil_condition_is_defence_in_depth_not_a_proven_discriminator() -> TestResult {
    fn zero_width_identifiers(source: &str) -> Vec<String> {
        fn walk(node: &Node, found: &mut Vec<String>) {
            if let NodeKind::Identifier { name } = &node.kind
                && node.location.start == node.location.end
            {
                found.push(name.clone());
            }
            node.for_each_child(|child| walk(child, found));
        }
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        let mut found = Vec::new();
        walk(&output.ast, &mut found);
        found
    }

    let recovery_inputs = [
        "foo(",
        "foo(;",
        "my $x = ;",
        "sub {",
        "if (",
        "use ;",
        "require ;",
        "$h{",
        "@{",
        "${",
        "Foo->",
        "->bar;",
        "package ;",
        "new ;",
        "sort ;",
        "1 +",
        "}",
        "{",
        "sub foo {",
        "while (",
        "for (",
        "s/a/",
        "tr/a/",
        "qw(",
        "Foo::",
        "::bar;",
        "&;",
        "*;",
        "%;",
        "@;",
    ];
    for source in recovery_inputs {
        let names = zero_width_identifiers(source);
        require(names.iter().all(|name| name == "$_"), || {
            format!(
                "{source:?} produced zero-width Identifier(s) {names:?} with a non-sigil name. The \
             sigil half of `is_synthesized_operand` is now load-bearing and must be given a \
             discriminating test rather than left as defence in depth."
            )
        })?;
    }

    // Positive control: the probe can see zero-width nodes at all.
    require_equal(zero_width_identifiers("s/a/b/;"), vec!["$_".to_string()], String::new)?;
    Ok(())
}

/// The remaining half of the premise: for these forms the parser emits no
/// sigil-prefixed `Identifier` at all, so the guard never sees them. This is a
/// falsification attempt over the shapes most likely to produce one —
/// dereferences, globs, `goto`, hash keys, interpolation — not a proof of
/// impossibility. A new counterexample fails here first.
#[test]
fn these_written_forms_yield_no_sigil_prefixed_identifier() -> TestResult {
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
        "new $class(1);",
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
        require(sigil_prefixed_identifiers(source).is_empty(), || {
            format!(
                "{source:?} produced sigil-prefixed Identifier node(s) {:?}; if any is zero-width \
             the guard would now discard it — add it to the preserved-name control instead",
                sigil_prefixed_identifiers(source)
            )
        })?;
    }
    Ok(())
}
