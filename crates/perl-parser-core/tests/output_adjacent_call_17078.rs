//! Perl 5.40.1 `-MO=Deparse,-p` distinguishes `print FOO(1)` (a call
//! argument) from `print FOO (1)` (an explicit handle when FOO is unknown).
//! Keep that source adjacency in the parser instead of hiding PL109 downstream.

use perl_parser_core::{Node, NodeKind, Parser};

fn parse(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    assert!(parser.errors().is_empty(), "{source}: {:?}", parser.errors());
    Ok(ast)
}

fn find_call<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::FunctionCall { name: actual, .. } if actual == name) {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_call(child, name))
}

fn find_indirect<'a>(node: &'a Node, method: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::IndirectCall { method: actual, .. } if actual == method) {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_indirect(child, method))
}

#[test]
fn adjacent_uppercase_and_qualified_calls_are_output_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    for operator in ["print", "printf", "say"] {
        for callable in ["FOO", "Foo::bar", "Acme::Counter::next_value", "STDOUT"] {
            for prefix in ["", "my $ok = ", "return "] {
                let source = format!("use feature 'say'; {prefix}{operator} {callable}(1), 2;");
                let ast = parse(&source)?;
                assert!(
                    find_indirect(&ast, operator).is_none(),
                    "call argument became a filehandle: {source}: {}",
                    ast.to_sexp()
                );
                let output = find_call(&ast, operator).ok_or("missing output call")?;
                let call = find_call(output, callable).ok_or_else(|| {
                    format!("missing nested {callable} call: {source}: {}", ast.to_sexp())
                })?;
                assert_eq!(
                    &source[call.location.start..call.location.end],
                    format!("{callable}(1)"),
                    "nested call must retain its complete source range"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn separated_parentheses_keep_explicit_output_handles() -> Result<(), Box<dyn std::error::Error>> {
    for operator in ["print", "printf", "say"] {
        for handle in ["STDOUT", "FOO", "Foo::HANDLE"] {
            for separator in [" ", "\t", "\n", " # handle\n"] {
                let source = format!("use feature 'say'; {operator} {handle}{separator}(1, 2);");
                let ast = parse(&source)?;
                let indirect = find_indirect(&ast, operator)
                    .ok_or_else(|| format!("lost explicit handle: {source}: {}", ast.to_sexp()))?;
                let NodeKind::IndirectCall { object, args, .. } = &indirect.kind else {
                    return Err("expected indirect call".into());
                };
                assert!(matches!(&object.kind, NodeKind::Identifier { name } if name == handle));
                assert_eq!(&source[object.location.start..object.location.end], handle);
                assert!(!args.is_empty(), "explicit output list disappeared: {source}");
                assert!(find_call(indirect, handle).is_none(), "handle became a nested call");
            }
        }
    }
    Ok(())
}

#[test]
fn scalar_and_braced_output_handles_keep_their_role() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "print $fh (1, 2);",
        "print $fh(1, 2);",
        "print { $fh } (1, 2);",
        "print { $fh }(1, 2);",
        "print STDOUT 1;",
    ] {
        let ast = parse(source)?;
        assert!(find_indirect(&ast, "print").is_some(), "{source}: {}", ast.to_sexp());
    }
    Ok(())
}
