//! Focused regression proof for #13333: a `catch` variable head must not
//! redeclare the automatically declared catch variable. Perl has never
//! accepted `catch (my $e)`; Perl 5.44 names the fatal diagnostic
//! `Can't redeclare catch variable as "%s"`. The parser records one typed
//! diagnostic on the keyword and retains the catch binding exactly as for
//! the bare form, without creating an ordinary lexical declaration.

use perl_parser_core::{Node, NodeKind, ParseError, Parser};
use perl_tdd_support::must;

fn parse(source: &str) -> (Node, Vec<ParseError>) {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    (ast, parser.errors().to_vec())
}

fn redeclarations(errors: &[ParseError]) -> Vec<(String, usize, usize)> {
    errors
        .iter()
        .filter_map(|error| match error {
            ParseError::CatchVariableRedeclaration { keyword, range } => {
                Some((keyword.clone(), range.start, range.end))
            }
            _ => None,
        })
        .collect()
}

fn first_try(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::Try { .. }) {
        return Some(node);
    }
    node.children().iter().find_map(|child| first_try(child))
}

fn catch_bindings(ast: &Node) -> Vec<Option<String>> {
    let try_node = must_some_try(ast);
    match &try_node.kind {
        NodeKind::Try { catch_blocks, .. } => catch_blocks
            .iter()
            .map(|(binding, _)| binding.as_ref().map(|(spelling, _)| spelling.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

fn must_some_try(ast: &Node) -> &Node {
    match first_try(ast) {
        Some(node) => node,
        None => panic!("expected a Try node in {}", ast.to_sexp()),
    }
}

#[test]
fn bare_catch_variable_is_the_clean_control() {
    let source = "try { die } catch ($e) { print $e }\n";
    let (ast, errors) = parse(source);
    assert!(redeclarations(&errors).is_empty(), "bare form must not diagnose: {errors:?}");
    assert_eq!(catch_bindings(&ast), vec![Some("$e".to_string())]);
}

#[test]
fn declarator_keywords_in_catch_head_are_typed_diagnostics() {
    for keyword in ["my", "our", "state"] {
        let source = format!("try {{ die }} catch ({keyword} $e) {{ print $e }}\n");
        let (ast, errors) = parse(&source);
        let found = redeclarations(&errors);
        assert_eq!(found.len(), 1, "exactly one typed diagnostic for `{source}`: {errors:?}");
        let (found_keyword, start, end) = &found[0];
        assert_eq!(found_keyword, keyword);
        assert_eq!(
            &source[*start..*end],
            keyword,
            "diagnostic range must cover exactly the keyword in `{source}`"
        );
        // The binding is retained as for the bare form.
        assert_eq!(
            catch_bindings(&ast),
            vec![Some("$e".to_string())],
            "binding lost for `{source}`"
        );
    }
}

#[test]
fn whitespace_and_comment_variations_still_diagnose() {
    let source = "try { die } catch (my # note\n    $e) { print $e }\n";
    let (ast, errors) = parse(source);
    let found = redeclarations(&errors);
    assert_eq!(found.len(), 1, "one diagnostic expected: {errors:?}");
    assert_eq!(&source[found[0].1..found[0].2], "my");
    assert_eq!(catch_bindings(&ast), vec![Some("$e".to_string())]);
}

#[test]
fn keyword_without_a_variable_is_not_a_redeclaration() {
    let source = "try { die } catch (my) { print 1 }\n";
    let (ast, errors) = parse(source);
    assert!(
        redeclarations(&errors).is_empty(),
        "`catch (my)` has no variable to redeclare: {errors:?}"
    );
    assert_eq!(catch_bindings(&ast), vec![None]);
}

#[test]
fn declarators_in_the_catch_body_are_not_head_diagnostics() {
    let source = "try { die } catch ($e) { my $x = $e; our $y; state $z = 1 }\n";
    let (_ast, errors) = parse(source);
    assert!(redeclarations(&errors).is_empty(), "body declarations must not diagnose: {errors:?}");
}

#[test]
fn unicode_before_the_head_keeps_keyword_geometry() {
    let source = "my $caf\u{00e9} = 1; try { die } catch (our $e) { print $e }\n";
    let (_ast, errors) = parse(source);
    let found = redeclarations(&errors);
    assert_eq!(found.len(), 1, "one diagnostic expected: {errors:?}");
    assert_eq!(&source[found[0].1..found[0].2], "our");
}
