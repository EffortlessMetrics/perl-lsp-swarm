//! Protect the current-main behavior established while reconciling #13933.
//! These tests cover parser topology and byte spans, not runtime magic values.
use perl_parser_core::{Node, NodeKind, Parser};

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("{source}: {error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("{source}: {:?}", parser.errors()));
    }
    Ok(ast)
}

fn walk<'a>(node: &'a Node, nodes: &mut Vec<&'a Node>) {
    nodes.push(node);
    for child in node.children() {
        walk(child, nodes);
    }
}

fn check_repetition(source: &str, ast: &Node, magic: &str) -> Result<(), String> {
    let mut nodes = Vec::new();
    walk(ast, &mut nodes);
    let binaries: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x"))
        .collect();
    let [binary] = binaries.as_slice() else {
        return Err(format!("expected exactly one repetition in {ast:?}"));
    };
    let NodeKind::Binary { left, right, .. } = &binary.kind else {
        return Err("expected binary repetition".into());
    };
    if !matches!(&left.kind, NodeKind::String { value, .. } if value == "\"x\"")
        || !matches!(&right.kind, NodeKind::FunctionCall { name, args } if name == magic && args.is_empty())
        || source.get(left.location.start..left.location.end) != Some("\"x\"")
        || source.get(right.location.start..right.location.end) != Some(magic)
        || source.get(binary.location.start..binary.location.end)
            != Some(format!("\"x\" x {magic}").as_str())
    {
        return Err(format!("wrong repetition topology or span: {binary:?}"));
    }
    Ok(())
}

#[test]
fn repetition_magic_constants_preserve_rhs_and_source_bounds() -> Result<(), String> {
    for magic in ["__LINE__", "__FILE__", "__PACKAGE__"] {
        // A multibyte prefix distinguishes byte offsets from character offsets.
        for prefix in ["", "# λ\n"] {
            let source = format!("{prefix}my $value = \"x\" x {magic};");
            check_repetition(&source, &parse_clean(&source)?, magic)?;
        }
    }
    Ok(())
}

#[test]
fn unknown_double_underscore_rhs_remains_an_identifier() -> Result<(), String> {
    let source = "my $value = \"x\" x __UNKNOWN__;";
    let ast = parse_clean(source)?;
    let mut nodes = Vec::new();
    walk(&ast, &mut nodes);
    let repetitions: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(&node.kind,NodeKind::Binary {op,..} if op == "x"))
        .collect();
    let [binary] = repetitions.as_slice() else {
        return Err("missing unique repetition".into());
    };
    let NodeKind::Binary { right, .. } = &binary.kind else {
        return Err("missing binary".into());
    };
    if !matches!(&right.kind, NodeKind::Identifier {name} if name == "__UNKNOWN__")
        || source.get(right.location.start..right.location.end) != Some("__UNKNOWN__")
    {
        return Err(format!("unknown RHS changed identity or span: {right:?}"));
    }
    Ok(())
}
