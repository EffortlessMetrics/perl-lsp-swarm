use perl_parser_core::{Node, NodeKind, Parser, SourceLocation};

fn parse_clean(src: &str) -> Result<(), String> {
    let mut parser = Parser::new(src);
    let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;
    let sexp = ast.to_sexp();
    if sexp.contains("ERROR") {
        return Err(format!("expected clean parse, got ERROR nodes in: {sexp}\nsource: {src}"));
    }
    Ok(())
}

#[test]
fn push_postfix_array_deref() -> Result<(), String> {
    parse_clean("push $aref->@*, $x;")
}

#[test]
fn push_deep_postfix_array_deref() -> Result<(), String> {
    parse_clean("push $h->{key}->@*, $val;")
}

#[test]
fn unshift_postfix_array_deref() -> Result<(), String> {
    parse_clean("unshift $aref->@*, $x;")
}

#[test]
fn push_normal_array_regression() -> Result<(), String> {
    parse_clean("push @array, $val;")
}

#[test]
fn push_brace_array_regression() -> Result<(), String> {
    parse_clean("push @{$ref}, $val;")
}

#[test]
fn last_index_postfix_deref_simple() -> Result<(), String> {
    parse_clean("my $n = $aref->$#*;")
}

#[test]
fn last_index_postfix_deref_in_for() -> Result<(), String> {
    parse_clean("for (my $i = 0; $i <= $aref->$#*; $i++) { }")
}

#[test]
fn last_index_postfix_deref_arithmetic() -> Result<(), String> {
    parse_clean("my $len = $aref->$#* + 1;")
}

#[test]
fn last_index_postfix_deref_on_hash_slot() -> Result<(), String> {
    parse_clean("my $n = $self->{data}->$#*;")
}

#[test]
fn arrow_star_deref_keeps_enclosing_untie_span_covering_child() -> Result<(), String> {
    let source = "untie $href->%*;";
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;

    let (untie, deref) = find_untie_with_deref(&ast)
        .ok_or_else(|| format!("missing Untie(->%*) nodes in {}", ast.to_sexp()))?;
    let source_text = |location: SourceLocation| {
        source
            .get(location.start..location.end)
            .ok_or_else(|| format!("invalid span {}..{}", location.start, location.end))
    };

    if source_text(deref.location)? != "$href->%*" {
        return Err(format!("arrow-star span was {:?}", source_text(deref.location)?));
    }
    if source_text(untie.location)? != source.trim_end_matches(';') {
        return Err(format!("Untie span was {:?}", source_text(untie.location)?));
    }
    if untie.location.end < deref.location.end {
        return Err(format!(
            "Untie ended at {} before dereference ended at {}",
            untie.location.end, deref.location.end
        ));
    }
    Ok(())
}

fn find_untie_with_deref(node: &Node) -> Option<(&Node, &Node)> {
    if let NodeKind::Untie { variable } = &node.kind
        && let NodeKind::Unary { op, .. } = &variable.kind
        && op == "->%*"
    {
        return Some((node, variable));
    }
    node.children().into_iter().find_map(find_untie_with_deref)
}
