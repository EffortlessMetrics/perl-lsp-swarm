//! Declaration-anchor range parity (#2643): every lexical declaration form anchors
//! its write at the VARIABLE token (`$x`), not the enclosing statement span, for
//! both initialized and bare declarations.
//!
//! Before #2643, a bare `my $x;` (no initializer) had no `Assign`-LHS range to
//! derive the variable span from, so the PIR lowerer fell back to the enclosing
//! statement span (`my $x`). The fix threads a first-class `binding_range` (the
//! variable's source span) onto `HirStmt::Let`, populated at every HIR
//! construction site and consumed by the PIR lowerer for all declaration forms.
//! These tests pin the byte ranges.
//!
//! Notes:
//! - `our $x` / `local $x` lower to a `StashWrite` (package slot), which the
//!   lexical extractor does not surface. Their `binding_range` is therefore NOT
//!   observable through `extract_lexical_facts`; the `local` case is checked
//!   directly against the HIR `Let` node below (`local_binding_range_anchors_at_variable`),
//!   because `local $x = EXPR` parses its target as an `Assignment` and needs the
//!   extra unwrap to anchor at the variable rather than the `$x = EXPR` span.

use perl_parser_core::hir::{HirStmt, lower_body};
use perl_parser_core::pir::{LexicalRole, extract_lexical_facts};
use perl_parser_core::{Parser, hir::lower_ast};

/// Name and byte range of the first `HirStmt::Let` in the lowered body of `source`.
/// Unlike `decl_write_range`, this reads the HIR `binding_range` directly, so it
/// works for declaration forms (`local`/`our`) whose write is a `StashWrite` and
/// is invisible to the lexical extractor.
fn first_let_name_and_binding(source: &str) -> Option<(String, (usize, usize))> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let body = lower_body(&output.ast);
    let root = body.block(body.root_block)?;
    for stmt_id in &root.stmts {
        if let Some(HirStmt::Let { name, binding_range, .. }) = body.stmt(*stmt_id) {
            return Some((name.clone(), (binding_range.start, binding_range.end)));
        }
    }
    None
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Byte range of the FIRST lexical Write fact for `name` — i.e. the declaration
/// site (declarations are lowered before any later assignment of the same name).
fn decl_write_range(source: &str, name: &str) -> Option<(usize, usize)> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&file);
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == name && matches!(f.role, LexicalRole::Write) {
                let r = f.source_anchor.range.as_ref()?;
                return Some((r.start, r.end));
            }
        }
    }
    None
}

/// Variable-token byte range = first occurrence of `token` in `source` (byte
/// offsets, so multi-byte chars and CRLF are accounted for naturally).
fn token_range(source: &str, token: &str) -> Option<(usize, usize)> {
    let start = source.find(token)?;
    Some((start, start + token.len()))
}

#[test]
fn my_with_initializer_anchors_at_variable() -> TestResult {
    let src = "my $x = 1;\nprint $x;\n";
    let got = decl_write_range(src, "x").ok_or("no write fact for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "decl write must anchor at `$x`, not the `my $x` statement");
    Ok(())
}

#[test]
fn my_without_initializer_anchors_at_variable() -> TestResult {
    // The key #2643 case: bare `my $x;` previously fell back to the statement span.
    let src = "my $x;\n$x = 1;\nprint $x;\n";
    let got = decl_write_range(src, "x").ok_or("no write fact for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "bare declaration must anchor at `$x`, not `my $x`");
    Ok(())
}

#[test]
fn state_declaration_anchors_at_variable() -> TestResult {
    let src = "sub counter { state $n = 0; return $n; }\n";
    let got = decl_write_range(src, "n").ok_or("no write fact for $n")?;
    let want = token_range(src, "$n").ok_or("$n not in source")?;
    assert_eq!(got, want);
    Ok(())
}

#[test]
fn multiple_declarations_each_anchor_at_their_variable() -> TestResult {
    let src = "my $a = 1;\nmy $b = 2;\nprint $a;\nprint $b;\n";
    let got_a = decl_write_range(src, "a").ok_or("no write fact for $a")?;
    let got_b = decl_write_range(src, "b").ok_or("no write fact for $b")?;
    assert_eq!(got_a, token_range(src, "$a").ok_or("$a not in source")?);
    assert_eq!(got_b, token_range(src, "$b").ok_or("$b not in source")?);
    Ok(())
}

#[test]
fn unicode_name_anchors_at_variable_bytes() -> TestResult {
    // `é` is 2 bytes UTF-8; the `$café` token is 6 bytes. The anchor is byte-based,
    // not UTF-16 / char-based.
    let src = "my $café = 1;\nprint $café;\n";
    let want = token_range(src, "$café").ok_or("$café not in source")?;
    assert_eq!(want.1 - want.0, 6, "sanity: `$café` spans 6 bytes");
    let got = decl_write_range(src, "café").ok_or("no write fact for $café")?;
    assert_eq!(got, want, "decl anchor must be byte-based across the 2-byte é");
    Ok(())
}

#[test]
fn crlf_source_anchors_at_variable_bytes() -> TestResult {
    // CRLF before the declaration: byte offsets must count `\r\n` as 2 bytes, so the
    // decl `$x` offset reflects the preceding `# c\r\n` line.
    let src = "# c\r\nmy $x = 1;\r\nprint $x;\r\n";
    let got = decl_write_range(src, "x").ok_or("no write fact for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "CRLF line endings must not corrupt the byte anchor");
    Ok(())
}

#[test]
fn local_is_not_a_lexical_binding() -> TestResult {
    // `local $x` dynamically scopes a package global; it is NOT a lexical
    // declaration and must not surface a lexical write fact for `$x`.
    let src = "sub f { local $x = 1; return $x; }\n";
    assert_eq!(
        decl_write_range(src, "x"),
        None,
        "`local` must not produce a lexical declaration write"
    );
    Ok(())
}

#[test]
fn bare_state_declaration_anchors_at_variable() -> TestResult {
    // No-initializer `state $n;` — the #2643 regression must hold for `state`, not
    // just `my`. `state` lowers to a LexicalWrite, so the lexical extractor sees it.
    let src = "sub counter { state $n; $n = 1; return $n; }\n";
    let got = decl_write_range(src, "n").ok_or("no write fact for $n")?;
    let want = token_range(src, "$n").ok_or("$n not in source")?;
    assert_eq!(got, want, "bare `state` declaration must anchor at `$n`, not `state $n`");
    Ok(())
}

#[test]
fn bare_local_binding_range_anchors_at_variable() -> TestResult {
    // No-initializer `local $x;` — anchors the HIR binding_range at `$x` (checked
    // directly, since `local` is a StashWrite and invisible to the lexical
    // extractor, which must still return None).
    let src = "local $x;\n$x = 1;\n";
    let (name, got) = first_let_name_and_binding(src).ok_or("no HirStmt::Let for local decl")?;
    assert_eq!(name, "x", "bare local declaration name must be `x`, not `<unknown>`");
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "bare local binding_range must anchor at `$x`");
    assert_eq!(
        decl_write_range(src, "x"),
        None,
        "bare `local` must not produce a lexical declaration write"
    );
    Ok(())
}

#[test]
fn local_binding_range_anchors_at_variable() -> TestResult {
    // `local $x = 1;` parses its target as an `Assignment` (`$x = 1`), not a bare
    // `Variable`. Without unwrapping, the HIR `Let` would carry name "<unknown>"
    // and a `binding_range` spanning `$x = 1`. Assert the declared name and the
    // anchor land on the `$x` token (#2643).
    let src = "local $x = 1;\n";
    let (name, got) = first_let_name_and_binding(src).ok_or("no HirStmt::Let for local decl")?;
    assert_eq!(name, "x", "local declaration name must be `x`, not `<unknown>`");
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "local binding_range must anchor at `$x`, not `$x = 1`");
    Ok(())
}
