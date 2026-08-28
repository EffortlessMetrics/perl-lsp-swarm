//! Regression coverage for Perl's postfix hash-slice dereference form.
//!
//! `EXPR->@{KEYS}` is the postfix equivalent of `@{EXPR}{KEYS}`. It is
//! distinct from both hash-element access (`EXPR->{KEY}`) and postfix array
//! slicing (`EXPR->@[INDICES]`), so the parser must retain a `HashSlice` node.

use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), String>;

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn source_text<'a>(source: &'a str, node: &Node) -> Result<&'a str, String> {
    source.get(node.location.start..node.location.end).ok_or_else(|| {
        format!(
            "node span {}..{} is outside source of {} bytes",
            node.location.start,
            node.location.end,
            source.len()
        )
    })
}

fn hash_slices<'a>(node: &'a Node, found: &mut Vec<&'a Node>) {
    if matches!(&node.kind, NodeKind::HashSlice { .. }) {
        found.push(node);
    }
    for child in node.children() {
        hash_slices(child, found);
    }
}

fn one_hash_slice<'a>(source: &str, ast: &'a Node) -> Result<&'a Node, String> {
    let mut slices = Vec::new();
    hash_slices(ast, &mut slices);
    if slices.len() == 1 {
        slices
            .into_iter()
            .next()
            .ok_or_else(|| "the one HashSlice result was not retained".to_string())
    } else {
        Err(format!(
            "expected exactly one HashSlice for source:\n{source}\n\nAST:\n{}\nfound {}",
            ast.to_sexp(),
            slices.len()
        ))
    }
}

fn check_one_hash_slice(source: &str) -> TestResult {
    let ast = parse_clean(source)?;
    let slice = one_hash_slice(source, &ast)?;
    match &slice.kind {
        NodeKind::HashSlice { .. } => Ok(()),
        other => Err(format!("expected HashSlice, got {}", other.kind_name())),
    }
}

#[test]
fn postfix_hash_slice_preserves_target_keys_and_full_span() -> TestResult {
    let source = "$href->@{'alpha', $key};";
    let ast = parse_clean(source)?;
    let slice = one_hash_slice(source, &ast)?;

    if source_text(source, slice)? != "$href->@{'alpha', $key}" {
        return Err(format!("unexpected HashSlice span: {:?}", source_text(source, slice)?));
    }

    let (target, keys) = match &slice.kind {
        NodeKind::HashSlice { target, keys } => (target, keys),
        other => return Err(format!("expected HashSlice, got {}", other.kind_name())),
    };
    if source_text(source, target)? != "$href" {
        return Err(format!("unexpected HashSlice target: {:?}", source_text(source, target)?));
    }
    if source_text(source, keys)? != "'alpha', $key" {
        return Err(format!("unexpected HashSlice keys: {:?}", source_text(source, keys)?));
    }
    if !matches!(&target.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "href")
    {
        return Err(format!("expected $href target, got {}", target.kind.kind_name()));
    }

    let elements = match &keys.kind {
        NodeKind::ArrayLiteral { elements } => elements,
        other => {
            return Err(format!("expected an ArrayLiteral key list, got {}", other.kind_name()));
        }
    };
    if elements.len() != 2 {
        return Err(format!("expected two key operands, got {}", elements.len()));
    }
    let second =
        elements.get(1).ok_or_else(|| "the second key operand was not retained".to_string())?;
    if !matches!(&second.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "key")
    {
        return Err(format!("expected $key as second operand, got {}", second.kind.kind_name()));
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_with_qw_keys() -> TestResult {
    check_one_hash_slice("my @values = $href->@{qw(alpha beta)};")
}

#[test]
fn postfix_hash_slice_with_variable_keys() -> TestResult {
    check_one_hash_slice("my @values = $href->@{@keys};")
}

#[test]
fn postfix_hash_slice_remains_an_lvalue() -> TestResult {
    check_one_hash_slice("$href->@{qw(alpha beta)} = (1, 2);")
}

#[test]
fn postfix_hash_slice_after_chained_receiver() -> TestResult {
    check_one_hash_slice("my @values = $object->{payload}->@{qw(alpha beta)};")
}

#[test]
fn postfix_hash_slice_keeps_postfix_precedence() -> TestResult {
    let source = "my $value = $href->@{'alpha'}[0];";
    let ast = parse_clean(source)?;
    let slice = one_hash_slice(source, &ast)?;
    let parent = ast
        .children()
        .into_iter()
        .find(|node| source_text(source, node).ok() == Some("my $value = $href->@{'alpha'}[0]"))
        .ok_or_else(|| {
            "the declaration containing the postfix chain was not retained".to_string()
        })?;
    if !matches!(&parent.kind, NodeKind::VariableDeclaration { .. }) {
        return Err(format!("expected a variable declaration, got {}", parent.kind.kind_name()));
    }
    let slice_end = slice.location.end;
    let parent_text = source_text(source, parent)?;
    if slice_end >= parent.location.end || !parent_text.ends_with("[0]") {
        return Err(format!("HashSlice did not remain the left operand of [0]: {parent_text:?}"));
    }
    Ok(())
}

#[test]
fn neighboring_postfix_forms_keep_their_existing_nodes() -> TestResult {
    let source = "my @values = $aref->@[0, 2]; my %pairs = $href->%{qw(alpha beta)};";
    let ast = parse_clean(source)?;
    let mut slices = Vec::new();
    hash_slices(&ast, &mut slices);
    if !slices.is_empty() {
        return Err(format!(
            "array and key/value postfix slices were reclassified as HashSlice: {}",
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn incomplete_postfix_hash_slice_recovers_without_panicking() -> TestResult {
    let source = "$href->@{'alpha', $key;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if output.diagnostics.is_empty() {
        return Err("truncated hash slice retained no recovery diagnostics".to_string());
    }
    if !matches!(output.ast.kind, NodeKind::Program { .. }) {
        return Err(format!("recovery returned {}, not a Program", output.ast.kind.kind_name()));
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_reaches_canonical_hir_lowering() -> TestResult {
    use perl_parser_core::hir::{HirExpr, HirExprId, lower_ast};

    let source = "my @values = $href->@{'alpha', $key};";
    let ast = parse_clean(source)?;
    let file = lower_ast(&ast);
    let body =
        file.root_body().ok_or_else(|| "lower_ast did not expose a root body".to_string())?;
    let mut hash_slice_calls = Vec::new();
    for index in 0..body.exprs.len() {
        if let Some(HirExpr::Call { ast_kind, args, .. }) = body.expr(HirExprId(index as u32))
            && ast_kind == "HashSlice"
        {
            let range = body
                .source_map
                .expr_ranges
                .get(index)
                .ok_or_else(|| format!("HIR expression {index} has no source range"))?;
            hash_slice_calls.push((args.len(), *range));
        }
    }
    if hash_slice_calls.len() != 1 {
        return Err(format!("expected one lowered HashSlice call, got {}", hash_slice_calls.len()));
    }
    let (argument_count, range) = hash_slice_calls
        .into_iter()
        .next()
        .ok_or_else(|| "the lowered HashSlice call was not retained".to_string())?;
    if argument_count != 3 {
        return Err(format!(
            "lowering retained {argument_count} operands, expected target plus two keys"
        ));
    }
    let lowered_text = source.get(range.start..range.end).ok_or_else(|| {
        format!("HIR source range {}..{} is outside the source", range.start, range.end)
    })?;
    if lowered_text != "$href->@{'alpha', $key}" {
        return Err(format!("unexpected lowered HashSlice span: {lowered_text:?}"));
    }
    Ok(())
}
