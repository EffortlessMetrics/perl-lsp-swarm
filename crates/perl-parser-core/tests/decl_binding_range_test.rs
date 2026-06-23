//! Declaration-anchor range parity (#2640): every lexical declaration form anchors
//! its write at the VARIABLE token (`$x`), not the enclosing statement span, for
//! both initialized and bare declarations.
//!
//! Before #2640, `extract_lexical_facts` anchored a declaration write at the
//! statement span (`my $x`, sometimes wider). The fix threads a first-class
//! `binding_range` (the variable's source span) onto `HirStmt::Let`, used by the
//! PIR lowerer for all declaration forms. These tests pin the byte ranges.
//!
//! Notes:
//! - `our $x` lowers to a `StashWrite` (package slot), which the lexical extractor
//!   does not surface; it is anchored by the same `binding_range` code path as the
//!   lexical forms below, so it is covered transitively.
//! - `local $x` is dynamic-scoping of a *package* global, not a lexical binding —
//!   it produces no lexical write fact (asserted below as a negative control).

use perl_parser_core::pir::{LexicalRole, extract_lexical_facts};
use perl_parser_core::{Parser, hir::lower_ast};

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
    // The key #2640 case: bare `my $x;` previously fell back to the statement span.
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
