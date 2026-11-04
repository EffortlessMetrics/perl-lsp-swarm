//! Regression coverage for Perl's postfix hash-slice dereference form.
//!
//! `EXPR->@{KEYS}` is the postfix equivalent of `@{EXPR}{KEYS}`. It is
//! distinct from both hash-element access (`EXPR->{KEY}`) and postfix array
//! slicing (`EXPR->@[INDICES]`), so the parser must retain a `HashSlice` node.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), String>;

/// Assert the shared clean-parse contract, then parse once more for
/// parser-specific structural assertions.
fn clean_ast(source: &str) -> Node {
    assert_clean_parse(source);
    parse(source)
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

fn find_all<'a, F>(node: &'a Node, predicate: &F, found: &mut Vec<&'a Node>)
where
    F: Fn(&Node) -> bool,
{
    if predicate(node) {
        found.push(node);
    }
    for child in node.children() {
        find_all(child, predicate, found);
    }
}

fn hash_slice_index_parents<'a>(node: &'a Node, found: &mut Vec<&'a Node>) {
    if let NodeKind::Binary { op, left, .. } = &node.kind
        && op == "[]"
        && matches!(&left.kind, NodeKind::HashSlice { .. })
    {
        found.push(node);
    }
    for child in node.children() {
        hash_slice_index_parents(child, found);
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

/// Assert the slice's target is exactly the given variable spelling.
fn assert_slice_target(source: &str, slice: &Node, expected_target: &str) -> Result<(), String> {
    let NodeKind::HashSlice { target, .. } = &slice.kind else {
        return Err(format!("expected HashSlice, got {}", slice.kind.kind_name()));
    };
    if source_text(source, target)? != expected_target {
        return Err(format!(
            "unexpected HashSlice target: {:?} (expected {expected_target:?})",
            source_text(source, target)?
        ));
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_preserves_target_keys_and_full_span() -> TestResult {
    let source = "$href->@{'alpha', $key};";
    let ast = clean_ast(source);
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
    let source = "my @values = $href->@{qw(alpha beta)};";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;

    assert_slice_target(source, slice, "$href")?;
    let NodeKind::HashSlice { keys, .. } = &slice.kind else {
        return Err(format!("expected HashSlice, got {}", slice.kind.kind_name()));
    };
    if source_text(source, keys)? != "qw(alpha beta)" {
        return Err(format!("unexpected qw key list span: {:?}", source_text(source, keys)?));
    }
    let NodeKind::ArrayLiteral { elements } = &keys.kind else {
        return Err(format!("expected an ArrayLiteral key list, got {}", keys.kind.kind_name()));
    };
    if elements.len() != 2 {
        return Err(format!("expected two qw key operands, got {}", elements.len()));
    }
    for (element, expected) in elements.iter().zip(["'alpha'", "'beta'"]) {
        if !matches!(&element.kind, NodeKind::String { value, interpolated: false }
            if value == expected)
        {
            return Err(format!(
                "expected single-quoted string key {expected:?}, got {}",
                element.kind.kind_name()
            ));
        }
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_with_variable_keys() -> TestResult {
    let source = "my @values = $href->@{@keys};";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;

    assert_slice_target(source, slice, "$href")?;
    let NodeKind::HashSlice { keys, .. } = &slice.kind else {
        return Err(format!("expected HashSlice, got {}", slice.kind.kind_name()));
    };
    if !matches!(&keys.kind, NodeKind::Variable { sigil, name } if sigil == "@" && name == "keys") {
        return Err(format!("expected @keys as the key expression, got {}", keys.kind.kind_name()));
    }
    if source_text(source, keys)? != "@keys" {
        return Err(format!("unexpected key span: {:?}", source_text(source, keys)?));
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_remains_an_lvalue() -> TestResult {
    let source = "$href->@{qw(alpha beta)} = (1, 2);";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;

    let mut assignments = Vec::new();
    find_all(
        &ast,
        &|node| matches!(&node.kind, NodeKind::Assignment { op, .. } if op == "="),
        &mut assignments,
    );
    if assignments.len() != 1 {
        return Err(format!("expected exactly one `=` assignment, found {}", assignments.len()));
    }
    let assignment = assignments
        .into_iter()
        .next()
        .ok_or_else(|| "the one assignment result was not retained".to_string())?;
    let NodeKind::Assignment { lhs, rhs, op } = &assignment.kind else {
        return Err("the assignment changed shape during inspection".to_string());
    };
    if op != "=" {
        return Err(format!("expected `=` operator, got {op}"));
    }
    if !std::ptr::eq(lhs.as_ref(), slice) {
        return Err("the HashSlice is not the assignment's left-hand side".to_string());
    }
    let NodeKind::ArrayLiteral { elements } = &rhs.kind else {
        return Err(format!("expected an ArrayLiteral rhs, got {}", rhs.kind.kind_name()));
    };
    if elements.len() != 2 {
        return Err(format!("expected two rhs operands, got {}", elements.len()));
    }
    for (element, expected) in elements.iter().zip(["1", "2"]) {
        if !matches!(&element.kind, NodeKind::Number { value } if value == expected) {
            return Err(format!(
                "expected number {expected:?} in rhs, got {}",
                element.kind.kind_name()
            ));
        }
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_after_chained_receiver() -> TestResult {
    let source = "my @values = $object->{payload}->@{qw(alpha beta)};";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;

    if source_text(source, slice)? != "$object->{payload}->@{qw(alpha beta)}" {
        return Err(format!("unexpected HashSlice span: {:?}", source_text(source, slice)?));
    }
    let NodeKind::HashSlice { target, keys } = &slice.kind else {
        return Err(format!("expected HashSlice, got {}", slice.kind.kind_name()));
    };
    if source_text(source, target)? != "$object->{payload}" {
        return Err(format!("unexpected chained target: {:?}", source_text(source, target)?));
    }
    let NodeKind::Binary { op, left, right } = &target.kind else {
        return Err(format!("expected a `->{{}}` deref target, got {}", target.kind.kind_name()));
    };
    if op != "->{}" {
        return Err(format!("expected `->{{}}` deref operator, got {op}"));
    }
    if !matches!(&left.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "object")
    {
        return Err(format!("expected $object receiver, got {}", left.kind.kind_name()));
    }
    if !matches!(&right.kind, NodeKind::Identifier { name } if name == "payload") {
        return Err(format!("expected payload key, got {}", right.kind.kind_name()));
    }
    let NodeKind::ArrayLiteral { elements } = &keys.kind else {
        return Err(format!("expected an ArrayLiteral key list, got {}", keys.kind.kind_name()));
    };
    if elements.len() != 2 {
        return Err(format!("expected two key operands, got {}", elements.len()));
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_preserves_utf8_key_spans() -> TestResult {
    let source = "my @values = $href->@{'naïve', '東京'};";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;
    let (target, keys) = match &slice.kind {
        NodeKind::HashSlice { target, keys } => (target, keys),
        other => return Err(format!("expected HashSlice, got {}", other.kind_name())),
    };

    if source_text(source, slice)? != "$href->@{'naïve', '東京'}"
        || source_text(source, target)? != "$href"
        || source_text(source, keys)? != "'naïve', '東京'"
    {
        return Err(format!(
            "UTF-8 HashSlice spans were not retained: slice={:?}, target={:?}, keys={:?}",
            source_text(source, slice)?,
            source_text(source, target)?,
            source_text(source, keys)?
        ));
    }
    let NodeKind::ArrayLiteral { elements } = &keys.kind else {
        return Err(format!("expected an ArrayLiteral key list, got {}", keys.kind.kind_name()));
    };
    if elements.len() != 2 {
        return Err(format!("expected two key operands, got {}", elements.len()));
    }
    for (element, expected) in elements.iter().zip(["'naïve'", "'東京'"]) {
        if !matches!(&element.kind, NodeKind::String { value, interpolated: false }
            if value == expected)
        {
            return Err(format!(
                "expected UTF-8 string key {expected:?}, got {}",
                element.kind.kind_name()
            ));
        }
        if source_text(source, element)? != expected {
            return Err(format!("unexpected UTF-8 key span: {:?}", source_text(source, element)?));
        }
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_keeps_postfix_precedence() -> TestResult {
    let source = "my $value = $href->@{'alpha'}[0];";
    let ast = clean_ast(source);
    let slice = one_hash_slice(source, &ast)?;
    let mut parents = Vec::new();
    hash_slice_index_parents(&ast, &mut parents);
    if parents.len() != 1 {
        return Err(format!(
            "expected exactly one Binary [] parent for the HashSlice, found {}",
            parents.len()
        ));
    }
    let parent = parents
        .into_iter()
        .next()
        .ok_or_else(|| "no Binary [] parent retained the HashSlice".to_string())?;
    if !matches!(&parent.kind, NodeKind::Binary { op, .. } if op == "[]") {
        return Err(format!("expected a Binary [] parent, got {}", parent.kind.kind_name()));
    }
    let NodeKind::Binary { left, right, .. } = &parent.kind else {
        return Err("the postfix index parent changed shape during inspection".to_string());
    };
    if !std::ptr::eq(left.as_ref(), slice) {
        return Err(
            "the Binary [] parent does not own the HashSlice as its left operand".to_string()
        );
    }
    if source_text(source, right)? != "0" {
        return Err(format!(
            "postfix index parent retained an unexpected right operand: {:?}",
            source_text(source, right)?
        ));
    }
    if source_text(source, parent)? != "$href->@{'alpha'}[0]" {
        return Err(format!("unexpected postfix parent span: {:?}", source_text(source, parent)?));
    }
    Ok(())
}

#[test]
fn neighboring_postfix_forms_keep_their_existing_nodes() -> TestResult {
    let source = "my @values = $aref->@[0, 2]; my %pairs = $href->%{qw(alpha beta)};";
    let ast = clean_ast(source);

    let mut reclassified = Vec::new();
    find_all(
        &ast,
        &|node| matches!(&node.kind, NodeKind::HashSlice { .. } | NodeKind::KeyValueSlice { .. }),
        &mut reclassified,
    );
    if !reclassified.is_empty() {
        return Err(format!(
            "array and key/value postfix slices were reclassified as slice nodes: {}",
            ast.to_sexp()
        ));
    }

    let mut arrow_array_slices = Vec::new();
    find_all(
        &ast,
        &|node| matches!(&node.kind, NodeKind::Binary { op, .. } if op == "->@[]"),
        &mut arrow_array_slices,
    );
    if arrow_array_slices.len() != 1 {
        return Err(format!(
            "expected exactly one `->@[]` node, found {}",
            arrow_array_slices.len()
        ));
    }
    let NodeKind::Binary { op, left, right } = &arrow_array_slices[0].kind else {
        return Err("the `->@[]` node changed shape during inspection".to_string());
    };
    if op != "->@[]" {
        return Err(format!("expected `->@[]` operator, got {op}"));
    }
    if !matches!(&left.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "aref") {
        return Err(format!("expected $aref receiver, got {}", left.kind.kind_name()));
    }
    let NodeKind::ArrayLiteral { elements } = &right.kind else {
        return Err(format!("expected an ArrayLiteral index list, got {}", right.kind.kind_name()));
    };
    if !matches!(&elements.first().map(|n| &n.kind), Some(NodeKind::Number { value }) if value == "0")
    {
        return Err("expected index 0 as the first `->@[]` operand".to_string());
    }
    if !matches!(&elements.get(1).map(|n| &n.kind), Some(NodeKind::Number { value }) if value == "2")
    {
        return Err("expected index 2 as the second `->@[]` operand".to_string());
    }

    let mut arrow_key_value_slices = Vec::new();
    find_all(
        &ast,
        &|node| matches!(&node.kind, NodeKind::Binary { op, .. } if op == "->%{}"),
        &mut arrow_key_value_slices,
    );
    if arrow_key_value_slices.len() != 1 {
        return Err(format!(
            "expected exactly one `->%{{}}` node, found {}",
            arrow_key_value_slices.len()
        ));
    }
    let NodeKind::Binary { op, left, right } = &arrow_key_value_slices[0].kind else {
        return Err("the `->%{{}}` node changed shape during inspection".to_string());
    };
    if op != "->%{}" {
        return Err(format!("expected `->%{{}}` operator, got {op}"));
    }
    if !matches!(&left.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "href") {
        return Err(format!("expected $href receiver, got {}", left.kind.kind_name()));
    }
    let NodeKind::ArrayLiteral { elements } = &right.kind else {
        return Err(format!("expected an ArrayLiteral key list, got {}", right.kind.kind_name()));
    };
    for (element, expected) in elements.iter().zip(["'alpha'", "'beta'"]) {
        if !matches!(&element.kind, NodeKind::String { value, interpolated: false }
            if value == expected)
        {
            return Err(format!(
                "expected key {expected:?} in the `->%{{}}` operand list, got {}",
                element.kind.kind_name()
            ));
        }
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
fn malformed_postfix_hash_slice_does_not_create_a_clean_hash_slice() -> TestResult {
    let source = "$href->@{};";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if output.diagnostics.is_empty() {
        return Err("empty postfix hash slice retained no recovery diagnostics".to_string());
    }
    let mut slices = Vec::new();
    hash_slices(&output.ast, &mut slices);
    if !slices.is_empty() {
        return Err(format!(
            "empty postfix hash slice was classified as clean HashSlice: {}",
            output.ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn truncated_postfix_hash_slice_keeps_following_statement_recoverable() -> TestResult {
    let source = "$href->@{'alpha'; my $after = 1;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if output.diagnostics.is_empty() {
        return Err("truncated postfix hash slice retained no recovery diagnostics".to_string());
    }
    if !source_text(source, &output.ast)?.contains("my $after = 1") {
        return Err("postfix hash-slice recovery discarded the following statement".to_string());
    }
    Ok(())
}

#[test]
fn postfix_hash_slice_reaches_canonical_hir_lowering() -> TestResult {
    use perl_parser_core::hir::{HirExpr, HirExprId, lower_ast};

    let source = "my @values = $href->@{'alpha', $key};";
    let ast = clean_ast(source);
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
