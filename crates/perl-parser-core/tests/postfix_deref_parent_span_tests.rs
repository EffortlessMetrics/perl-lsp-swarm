//! Discriminating span proof for arrow star-form postfix dereference (#13891).
//!
//! Each Unary must cover operator + operand (`$aref->@*`, not `$aref`). That
//! is the `op_token.end()` / `star.end()` contract. A stale
//! `previous_position()` after `tokens.next()` would pin the node on the
//! receiver only.

mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::must_some_with;

const STAR_FORMS: &[StarForm] = &[
    StarForm { text: "$sref->$*", op: "->$*", receiver: "$sref" },
    StarForm { text: "$aref->$#*", op: "->$#*", receiver: "$aref" },
    StarForm { text: "$aref->@*", op: "->@*", receiver: "$aref" },
    StarForm { text: "$href->%*", op: "->%*", receiver: "$href" },
    StarForm { text: "$cref->&*", op: "->&*", receiver: "$cref" },
    StarForm { text: "$gref->**", op: "->**", receiver: "$gref" },
];

struct StarForm {
    text: &'static str,
    op: &'static str,
    receiver: &'static str,
}

#[test]
fn every_arrow_star_deref_keeps_enclosing_untie_span_covering_child() {
    for case in STAR_FORMS {
        let source = format!("untie {};", case.text);
        assert_clean_parse(&source);
        let ast = parse(&source);
        let (untie, deref) = must_some_with(
            find_untie_with_deref(&ast, &source, case.op, case.receiver),
            "Untie wrapping a star-form Unary",
        );

        assert_eq!(source_text(deref, &source), Some(case.text), "{} deref span", case.op);
        assert_eq!(
            source_text(untie, &source),
            Some(source.trim_end_matches(';')),
            "{} enclosing span",
            case.op
        );
        assert!(
            untie.location.end >= deref.location.end,
            "{} parent ended at {} before child ended at {}",
            case.op,
            untie.location.end,
            deref.location.end
        );
    }
}

#[test]
fn star_form_unary_spans_operator_and_operand_not_receiver_only() {
    for case in STAR_FORMS {
        let source = format!("my $x = {};", case.text);
        assert_clean_parse(&source);
        let ast = parse(&source);
        let deref = must_star_unary(&ast, &source, case);
        assert_eq!(source_text(deref, &source), Some(case.text), "{} full span", case.op);
        assert_ne!(
            source_text(deref, &source),
            Some(case.receiver),
            "{} must not remain operand-only",
            case.op
        );
    }
}

#[test]
fn chained_receiver_star_form_covers_arrow_operator() {
    let source = "my @x = $href->{items}->@*;";
    assert_clean_parse(source);
    let ast = parse(source);
    let deref = must_some_with(
        find_star_unary(&ast, source, "->@*", "$href->{items}"),
        "chained ->@* Unary",
    );
    assert_eq!(source_text(deref, source), Some("$href->{items}->@*"));
    assert_ne!(source_text(deref, source), Some("$href->{items}"));
}

#[test]
fn last_index_star_form_does_not_swallow_following_arithmetic() {
    let source = "my $n = $aref->$#* + 1;";
    assert_clean_parse(source);
    let ast = parse(source);
    let deref = must_some_with(find_star_unary(&ast, source, "->$#*", "$aref"), "->$#* Unary");
    assert_eq!(source_text(deref, source), Some("$aref->$#*"));
    let plus = must_some_with(find_binary(&ast, "+"), "arithmetic parent of ->$#*");
    assert_eq!(source_text(plus, source), Some("$aref->$#* + 1"));
    assert!(
        plus.location.end > deref.location.end,
        "arithmetic parent must extend past the star, got {} vs {}",
        plus.location.end,
        deref.location.end
    );
}

#[test]
fn original_issue_repro_spans_full_postfix_deref() {
    let source = "my $aref = [1]; my @x = $aref->@*;\n";
    assert_clean_parse(source);
    let ast = parse(source);
    let deref = must_some_with(find_star_unary(&ast, source, "->@*", "$aref"), "issue repro ->@*");
    assert_eq!(source_text(deref, source), Some("$aref->@*"));
}

#[test]
fn trailing_semicolon_and_comment_are_outside_star_span() {
    let source = "my @x = $aref->@*; # keep\n";
    assert_clean_parse(source);
    let ast = parse(source);
    let deref =
        must_some_with(find_star_unary(&ast, source, "->@*", "$aref"), "->@* before trivia");
    assert_eq!(source_text(deref, source), Some("$aref->@*"));
    assert!(
        !source_text(deref, source).is_some_and(|text| text.contains('#') || text.contains(';'))
    );
}

#[test]
fn crlf_geometry_keeps_full_star_spans() {
    for case in STAR_FORMS {
        let lf = format!("my $x = {};\nmy $y = 1;\n", case.text);
        let source = lf.replace('\n', "\r\n");
        assert_clean_parse(&source);
        let ast = parse(&source);
        let deref = must_star_unary(&ast, &source, case);
        assert_eq!(source_text(deref, &source), Some(case.text), "{} CRLF full span", case.op);
        let following =
            must_some_with(find_variable(&ast, &source, "$y"), "following $y after CRLF star-form");
        assert!(
            following.location.start >= deref.location.end,
            "{} following sibling started inside the deref span",
            case.op
        );
    }
}

#[test]
fn repeated_star_forms_bind_two_distinct_full_spans() {
    let source = "my @x = $aref->@*; my @y = $aref->@*;";
    assert_clean_parse(source);
    let ast = parse(source);
    let found = collect_star_unary(&ast, source, "->@*", "$aref");
    assert_eq!(found.len(), 2, "repeated ->@* must bind twice, got {}", found.len());
    for node in &found {
        assert_eq!(source_text(node, source), Some("$aref->@*"));
    }
    assert!(
        found[0].location.start != found[1].location.start,
        "repeated ->@* bound one source occurrence twice"
    );
}

#[test]
fn incomplete_sigil_without_star_is_not_star_form_unary() {
    let source = "my $x = $aref->@;";
    let ast = parse(source);
    let found = collect_star_unary(&ast, source, "->@*", "$aref");
    assert!(
        found.is_empty(),
        "truncated ->@ must not become Unary(->@*), got {:#?}",
        ast.to_sexp()
    );
}

#[test]
fn slice_forms_remain_non_unary_star() {
    let source = "my @x = $aref->@[0, 2]; my %y = $href->%{qw(a b)};";
    assert_clean_parse(source);
    let ast = parse(source);

    let array_slice = must_some_with(find_binary(&ast, "->@[]"), "->@[] stays Binary");
    assert_eq!(source_text(array_slice, source), Some("$aref->@[0, 2]"));
    assert!(
        collect_star_unary(&ast, source, "->@*", "$aref").is_empty(),
        "array slice must not satisfy the star-form Unary"
    );

    let hash_slice = must_some_with(find_binary(&ast, "->%{}"), "->%{} stays Binary");
    assert_eq!(source_text(hash_slice, source), Some("$href->%{qw(a b)}"));
    assert!(
        collect_star_unary(&ast, source, "->%*", "$href").is_empty(),
        "hash slice must not satisfy the star-form Unary"
    );
}

fn must_star_unary<'a>(ast: &'a Node, source: &str, case: &StarForm) -> &'a Node {
    must_some_with(find_star_unary(ast, source, case.op, case.receiver), "star-form Unary present")
}

fn find_untie_with_deref<'a>(
    node: &'a Node,
    source: &str,
    operator: &str,
    receiver: &str,
) -> Option<(&'a Node, &'a Node)> {
    if let NodeKind::Untie { variable } = &node.kind
        && let NodeKind::Unary { op, operand } = &variable.kind
        && op == operator
        && source_text(operand, source) == Some(receiver)
    {
        return Some((node, variable));
    }
    node.children()
        .into_iter()
        .find_map(|child| find_untie_with_deref(child, source, operator, receiver))
}

fn find_star_unary<'a>(node: &'a Node, source: &str, op: &str, receiver: &str) -> Option<&'a Node> {
    let found = collect_star_unary(node, source, op, receiver);
    match found.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

fn collect_star_unary<'a>(node: &'a Node, source: &str, op: &str, receiver: &str) -> Vec<&'a Node> {
    let mut found = Vec::new();
    collect_star_unary_into(node, source, op, receiver, &mut found);
    found
}

fn collect_star_unary_into<'a>(
    node: &'a Node,
    source: &str,
    op: &str,
    receiver: &str,
    found: &mut Vec<&'a Node>,
) {
    if let NodeKind::Unary { op: node_op, operand } = &node.kind
        && node_op == op
        && source_text(operand, source) == Some(receiver)
    {
        found.push(node);
    }
    for child in node.children() {
        collect_star_unary_into(child, source, op, receiver, found);
    }
}

fn find_binary<'a>(node: &'a Node, op: &str) -> Option<&'a Node> {
    if let NodeKind::Binary { op: node_op, .. } = &node.kind
        && node_op == op
    {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_binary(child, op))
}

fn find_variable<'a>(node: &'a Node, source: &str, name: &str) -> Option<&'a Node> {
    if source_text(node, source) == Some(name)
        && let NodeKind::Variable { .. } = &node.kind
    {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_variable(child, source, name))
}

fn source_text<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}
