//! Do-while condition and trailing-block regression tests (#15649).
//!
//! These live in the owning crate: the do-while condition guard is enforced
//! in `perl-parser-core`'s postfix parser and the `DoWhileTrailingBlock`
//! error variant is defined here.

use perl_parser_core::{ParseError, Parser};

fn parse_clean(src: &str) -> Result<(), String> {
    let mut parser = Parser::new(src);
    let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;
    let sexp = ast.to_sexp();
    if sexp.contains("ERROR") {
        return Err(format!("expected clean parse, got ERROR nodes in: {sexp}\nsource: {src}"));
    }
    Ok(())
}

/// Parse `src` cleanly and count subscript Binary nodes in the serialized
/// AST as `(total, brace-op)` counts: the observable effect of the
/// keep-consuming seam taking the consume path. Distinct op spellings
/// (`{}`, `[]`, `->{}`) serialize distinctly, so the pair pins the exact
/// chain shape the seam consumed. Counted on the sexp rather than the node
/// traversal so the observation cannot miss nested operands.
fn subscript_shape_count(src: &str) -> Result<(usize, usize), String> {
    let mut parser = Parser::new(src);
    let ast = parser.parse().map_err(|error| format!("parse error for `{src}`: {error:?}"))?;
    let sexp = ast.to_sexp();
    if sexp.contains("ERROR") {
        return Err(format!("expected clean parse, got ERROR nodes for `{src}`"));
    }
    Ok((sexp.matches("(binary_").count(), sexp.matches("(binary_{}").count()))
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

    // The whole subscript family chains on unparenthesized conditions:
    // arrow and index subscripts produce their own binary ops (#15649
    // review wave 3).
    parse_clean("do { $s++ } while $a[0]{k};")?;
    parse_clean("do { $s++ } while $self->{a}{b};")?;
    parse_clean("do { $s++ } while $self->{a}[0];")?;

    // A subscripted variable inside a parenthesized condition is also an
    // ordinary condition shape.
    parse_clean("do { $s++ } while ($h{k});")?;

    // Nested grouping inside a parenthesized condition: subscripts inside the
    // groups keep parsing, and the parse stays clean after both groups close.
    parse_clean("do { $s++ } while (($h{k}));")?;

    // A lowercase bareword before `{` is the block-call form — ordinary
    // condition shape, stays allowed (#15649 review wave 3 insurance).
    parse_clean("sub foo { 0 } do { $i++ } while foo { $i++; };")
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
        // Parenthesized shapes whose grouping is AST-transparent: the `{`
        // sits after the condition's own closing `)`, so it can no longer be
        // a subscript even though the expression node is a bare variable or
        // bareword (#15649 review).
        r#"do { $i++; } while ($flag) { $i++; }"#,
        r#"do { $i++; } while (Foo) { $i++; }"#,
        // Same after the group closes for a subscripted condition: the
        // chain cannot continue past the `)` (#15649 review wave 3).
        r#"do { $i++; } while ($h{k}) { $i++; }"#,
    ] {
        let mut parser = Parser::new(code);
        if parser.parse().is_ok() {
            return Err(format!("expected outright parse failure for `{code}`"));
        }
    }

    // A semicolon before the block still separates two valid statements.
    parse_clean(r#"do { $i++; } while ($i < 3); { print "once\n"; }"#)
}

/// Discriminator 1 — op-arm boundary for the closed-group gate: every
/// subscript op in the chain family (`{}`, `[]`, `->{}`) keeps consuming
/// while bare but rejects once the condition's own `)` has closed. The
/// `{}` arm is covered by `do_while_rejects_trailing_block`; these pin the
/// `[]` and `->{}` arms. All outcomes confirmed with `perl -c` (#15649
/// ripr discriminator).
#[test]
fn do_while_discriminates_closed_group_chain_op_arms() -> Result<(), String> {
    parse_clean("do { $s++ } while $a[0]{k};")?;
    parse_clean("do { $s++ } while $self->{a}{b};")?;
    for code in ["do { $s++; } while ($a[0]){k};", "do { $s++; } while ($self->{a}){b};"] {
        let mut parser = Parser::new(code);
        if parser.parse().is_ok() {
            return Err(format!("expected outright parse failure for `{code}`"));
        }
    }
    Ok(())
}

/// Discriminator 2 — parenthesization boundary for the same subscript
/// shape: an attached `{k}` continues a bare chain, but rejects once the
/// condition's own `)` has closed. Both outcomes confirmed with `perl -c`
/// (#15649 ripr discriminator).
#[test]
fn do_while_discriminates_bare_chain_from_closed_group_chain() -> Result<(), String> {
    parse_clean("do { $s++ } while $h{k}{j};")?;
    let code = "do { $s++; } while ($h{k}){k};";
    let mut parser = Parser::new(code);
    if parser.parse().is_ok() {
        return Err(format!("expected outright parse failure for `{code}`"));
    }
    Ok(())
}

/// Discriminator 3 — depth boundary: a subscript chain nested inside the
/// condition's own parentheses keeps consuming. Confirmed with `perl -c`
/// (#15649 ripr discriminator).
#[test]
fn do_while_chain_inside_nested_condition_group_stays_clean() -> Result<(), String> {
    parse_clean("do { $s++; } while (($h{k}{j}));")
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
    let anchor = error.location().ok_or_else(|| format!("error carries no location: {error:?}"))?;
    let brace = code.rfind('{').ok_or("trailing brace not found")?;
    if anchor != brace {
        return Err(format!("error anchors at {anchor}, trailing brace is at {brace}"));
    }
    Ok(())
}

/// Discriminator 4 — keep-consuming leaves for the closed-group gate
/// (`postfix.rs`: `inside_condition_group || (unparenthesized_condition &&
/// (bare_shape || subscript_chain))`). Each leaf gets an input that hits it:
/// inside-group consume, unparenthesized bare consume, unparenthesized chain
/// consume, and the all-false unparenthesized reject (#15649 ripr
/// discriminator).
#[test]
fn do_while_discriminates_keep_consuming_leaves() -> Result<(), String> {
    // `inside_condition_group`: `{` inside the condition's own `(...)`
    // keeps consuming.
    parse_clean("do { $s++ } while ($h{k});")?;
    // `unparenthesized_condition && bare_shape`: a bare `$h` keeps `{k}`.
    parse_clean("do { $s++ } while $h{k};")?;
    // `unparenthesized_condition && subscript_chain`: the chain keeps going.
    parse_clean("do { $s++ } while $h{k}{j};")?;
    // All false on a parenthesized condition: a brace after the group's own
    // `)` closed is the trailing block and rejects outright.
    let code = "do { $s++; } while ($flag) { $s++; }";
    let mut parser = Parser::new(code);
    if parser.parse().is_ok() {
        return Err(format!("expected outright parse failure for `{code}`"));
    }
    // All false on an unparenthesized condition with a grouped operand: the
    // guard consumes through the transparent group and the parse degrades
    // to recovery (ERROR nodes) rather than the hard `DoWhileTrailingBlock`
    // of the parenthesized close. Real `perl -c` rejects this outright; the
    // recovery leniency is residual scope, not #15649's parenthesized
    // trailing block.
    {
        let code = "do { $s++; } while $a eq ($b) { $s++; }";
        let mut parser = Parser::new(code);
        let ast = parser
            .parse()
            .map_err(|error| format!("expected recovery parse, got hard error: {error:?}"))?;
        if !ast.to_sexp().contains("ERROR") {
            return Err(format!("expected recovery ERROR nodes for `{code}`"));
        }
    }
    Ok(())
}

/// Discriminator 5 — exact return-value proof for the keep-consuming seam
/// (`postfix.rs`: `inside_condition_group || (unparenthesized_condition &&
/// (bare_shape || subscript_chain))`). Clean/reject outcomes alone only
/// weakly grip the seam: a mutant that breaks out early can still produce
/// a clean parse via the trailing-block path. Asserting the exact `{}`-op
/// subscript Binary count per arm proves each brace was consumed *as a
/// subscript through the seam*, which only the true arm combination
/// produces. Each arm gets its input; all outcomes confirmed with
/// `perl -c` (#15649 ripr discriminator).
#[test]
fn parse_postfix_chain_boundary_discriminator() -> Result<(), String> {
    // `inside_condition_group`: `{k}` inside the condition's own `(...)`
    // is consumed as a subscript Binary.
    assert_eq!(subscript_shape_count("do { $s++ } while ($h{k});")?, (1, 1));
    assert_eq!(subscript_shape_count("do { $s++ } while (($h{k}{j}));")?, (2, 2));
    // `unparenthesized_condition && bare_shape`: a bare `$h` keeps `{k}`.
    assert_eq!(subscript_shape_count("do { $s++ } while $h{k};")?, (1, 1));
    // `unparenthesized_condition && subscript_chain`: the chain keeps going.
    assert_eq!(subscript_shape_count("do { $s++ } while $h{k}{j};")?, (2, 2));
    assert_eq!(subscript_shape_count("do { $s++ } while $a[0]{k};")?, (2, 1));
    assert_eq!(subscript_shape_count("do { $s++ } while $self->{a}{b};")?, (1, 1));
    Ok(())
}

/// Discriminator 6 — call-observation proof for the three supporting calls
/// behind the keep-consuming seam: `consume_balanced_in_interpolated_string`
/// (balanced `${h{k}}` records no diagnosis while an unclosed `${` records
/// exactly one `Unclosed { delimiter` diagnosis), plus the paren-group
/// enter/leave balance (nested `(($flag))` must return the depth to zero so
/// the trailing block still rejects with the exact `DoWhileTrailingBlock`
/// variant — a missing `leave_paren_group` would leave the depth elevated
/// and consume the block instead). Ground truth confirmed with `perl -c`
/// (#15649 ripr discriminator).
#[test]
fn consume_balanced_and_paren_group_call_observation() -> Result<(), String> {
    let mut balanced = Parser::new("my $s = \"${h{k}}\";");
    balanced.parse().map_err(|error| format!("balanced interpolation must parse: {error:?}"))?;
    assert!(
        balanced.errors().is_empty(),
        "balanced `${{h{{k}}}}` must record no diagnosis, got: {:?}",
        balanced.errors()
    );

    let mut unclosed = Parser::new("my $s = \"${h{k}\";");
    unclosed.parse().map_err(|error| format!("unclosed interpolation must recover: {error:?}"))?;
    assert_eq!(unclosed.errors().len(), 1, "unclosed `${{` must record exactly one diagnosis");
    assert!(
        format!("{:?}", unclosed.errors()[0]).contains("Unclosed { delimiter"),
        "diagnosis must name the unclosed delimiter, got: {:?}",
        unclosed.errors()[0]
    );

    let err = Parser::new("do { $s++; } while (($flag)) { $s++; };")
        .parse()
        .expect_err("nested groups must still reject the trailing block");
    assert!(
        matches!(err, ParseError::DoWhileTrailingBlock { .. }),
        "nested-group trailing block must raise the exact variant, got: {err:?}"
    );
    Ok(())
}
