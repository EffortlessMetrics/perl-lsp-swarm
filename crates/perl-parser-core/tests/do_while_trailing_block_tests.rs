//! Do-while condition and trailing-block regression tests (#15649).
//!
//! These live in the owning crate: the do-while condition guard is enforced
//! in `perl-parser-core`'s postfix parser and the `DoWhileTrailingBlock`
//! error variant is defined here.

use perl_parser_core::Parser;

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
fn do_while_block_condition() -> Result<(), String> {
    // Perl supports do { ... } while/until CONDITION;
    parse_clean("do { $x++ } while $x < 10;")?;
    parse_clean("do { $x-- } until $x == 0;")
}

#[test]
fn do_while_condition_keeps_chained_subscripts() -> Result<(), String> {
    // A `{` after an already-subscripted term is a chained subscript, not a
    // trailing block (#15649 review): the first `{k}` turns the condition
    // into a `{}`-op binary, and the guard must keep consuming.
    parse_clean("do { $s++ } while $h{k}{j};")?;
    parse_clean("do { $s++ } while $h{k}{j}{l};")?;

    // A subscripted variable inside a parenthesized condition is also an
    // ordinary condition shape.
    parse_clean("do { $s++ } while ($h{k});")
}

#[test]
fn do_while_rejects_trailing_block() -> Result<(), String> {
    // Real `perl -c` rejects a block after the do-while condition with
    // `syntax error at ... near ") {"` (issue #15649). The parse fails
    // outright rather than recovering.
    for code in [
        r#"do { $i++; } while ($i < 3) { print "continue\n"; }"#,
        r#"do { $i--; } until ($i == 0) { print "done\n"; }"#,
        r#"do { $i++; } while ($i < 3 && $j > 0) { print "x\n"; }"#,
    ] {
        let mut parser = Parser::new(code);
        if parser.parse().is_ok() {
            return Err(format!("expected outright parse failure for `{code}`"));
        }
    }

    // A semicolon before the block still separates two valid statements.
    parse_clean(r#"do { $i++; } while ($i < 3); { print "once\n"; }"#)
}

#[test]
fn trailing_block_error_anchors_at_the_brace() -> Result<(), String> {
    // The rejected `{` carries its byte position through
    // `ParseError::location()` so diagnostics report the offending brace
    // instead of falling back to EOF (#15649 review).
    let code = r#"do { $i++; } while ($i < 3) { print "x\n"; }"#;
    let mut parser = Parser::new(code);
    let error = match parser.parse() {
        Ok(_) => return Err("expected outright parse failure".to_string()),
        Err(error) => error,
    };
    let anchor = error
        .location()
        .ok_or_else(|| format!("error carries no location: {error:?}"))?;
    let brace = code.rfind('{').ok_or("trailing brace not found")?;
    if anchor != brace {
        return Err(format!("error anchors at {anchor}, trailing brace is at {brace}"));
    }
    Ok(())
}
