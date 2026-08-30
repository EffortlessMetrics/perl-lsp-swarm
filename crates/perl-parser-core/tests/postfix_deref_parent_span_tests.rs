//! Parent-span coverage for every arrow-star postfix dereference form.

use perl_parser_core::{Node, NodeKind, Parser};

const CASES: &[(&str, &str, &str)] = &[
    ("$sref->$*", "->$*", "$sref"),
    ("$aref->$#*", "->$#*", "$aref"),
    ("$aref->@*", "->@*", "$aref"),
    ("$href->%*", "->%*", "$href"),
    ("$cref->&*", "->&*", "$cref"),
    ("$gref->**", "->**", "$gref"),
];

#[test]
fn every_arrow_star_deref_keeps_enclosing_untie_span_covering_child() -> Result<(), String> {
    for (deref_text, operator, receiver) in CASES {
        let source = format!("untie {deref_text};");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{operator}: {error:?}"))?;
        let (untie, deref) = find_untie_with_deref(&ast, &source, operator, receiver)
            .ok_or_else(|| format!("missing Untie({operator}) nodes in {}", ast.to_sexp()))?;

        assert_eq!(source_text(deref, &source), Some(*deref_text), "{operator} deref span");
        assert_eq!(
            source_text(untie, &source),
            Some(source.trim_end_matches(';')),
            "{operator} enclosing span"
        );
        assert!(
            untie.location.end >= deref.location.end,
            "{operator} parent ended at {} before child ended at {}",
            untie.location.end,
            deref.location.end
        );
    }
    Ok(())
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

fn source_text<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}
