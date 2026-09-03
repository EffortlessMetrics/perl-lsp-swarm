//! Regression coverage for truncated postfix dereferences and recovered slice spans (#14174).

use perl_parser_core::{Node, NodeKind, ParseError, Parser, error::RecoveryKind};

fn contains_truncated_chain(diagnostics: &[ParseError]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        matches!(diagnostic, ParseError::Recovered { kind: RecoveryKind::TruncatedChain, .. })
    })
}

fn find_hash_slice(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::HashSlice { .. }) {
        return Some(node);
    }
    node.children().into_iter().find_map(find_hash_slice)
}

#[test]
fn every_truncated_postfix_dereference_sigil_records_recovery() {
    let mut missing = Vec::new();
    for suffix in ["$", "@", "%", "&", "*", "$#"] {
        let source = format!("my $value = $ref->{suffix}");
        let mut parser = Parser::new(&source);
        let output = parser.parse_with_recovery();

        if !contains_truncated_chain(&output.diagnostics) {
            missing.push((source, output.diagnostics, output.ast.to_sexp()));
        }
    }
    assert!(missing.is_empty(), "truncated postfix cases without recovery: {missing:?}");
}

#[test]
fn recovered_hash_slice_contains_its_selector_and_preserves_following_statement()
-> Result<(), String> {
    let source = "my @values = @$ref{'alpha';\nmy $next = 1;\n";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();

    let slice = find_hash_slice(&output.ast).ok_or("recovered hash slice not found")?;
    let NodeKind::HashSlice { target, keys } = &slice.kind else {
        return Err("find_hash_slice returned a non-slice node".to_string());
    };
    assert!(slice.location.start <= target.location.start);
    assert!(
        slice.location.end >= keys.location.end,
        "slice span {:?} must contain selector span {:?}",
        slice.location,
        keys.location
    );

    assert!(
        matches!(&output.ast.kind, NodeKind::Program { statements } if statements.len() >= 2),
        "recovery must preserve the following statement: {}",
        output.ast.to_sexp()
    );
    Ok(())
}
