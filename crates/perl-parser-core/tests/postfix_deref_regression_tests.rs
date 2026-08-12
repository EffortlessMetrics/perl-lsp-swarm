use perl_parser_core::{Node, NodeKind, Parser};

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

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
fn postfix_deref_forms_have_exact_spans() -> Result<(), String> {
    let cases = [
        ("->@*", "->@*"),
        ("->%*", "->%*"),
        ("->$*", "->$*"),
        ("->&*", "->&*"),
        ("->**", "->**"),
        ("->$#*", "->$#*"),
        ("->[0]", "->[]"),
        ("->{key}", "->{}"),
    ];

    for (suffix, operator) in cases {
        let source = format!("my $value = $object{suffix};");
        let expected_start = source
            .find("$object")
            .ok_or_else(|| format!("test source lost object for {suffix}"))?;
        let expected_end = expected_start + "$object".len() + suffix.len();
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{suffix}: {error:?}"))?;
        let mut matches = Vec::new();

        walk(&ast, &mut |node| match &node.kind {
            NodeKind::Unary { op, .. } | NodeKind::Binary { op, .. } if op == operator => {
                matches.push((node.location.start, node.location.end));
            }
            _ => {}
        });

        if matches != vec![(expected_start, expected_end)] {
            return Err(format!(
                "{suffix}: expected one {operator} span {expected_start}..{expected_end}, got {matches:?}"
            ));
        }
    }

    Ok(())
}
