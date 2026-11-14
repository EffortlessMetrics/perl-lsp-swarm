// Thematic integration tests: subst, qw, word operators, range, foreach (#5012 item 7).
// Consolidated from historical per-fix `fix_*.rs` files.

mod cpan_test_helpers;

mod qw_comment {
    // `#` inside qw is literal data, even when Perl warns about possible comments.
    // Oracle: Perl 5.40.1; see perldiag's "Possible attempt to put comments in qw() list".

    use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
    use perl_parser_core::{Node, NodeKind, Parser};

    fn parse(source: &str) -> Result<Node, String> {
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
        if !parser.errors().is_empty() {
            return Err(format!("unexpected diagnostics: {:?}", parser.errors()));
        }
        Ok(ast)
    }

    fn array(node: &Node) -> Result<&Node, String> {
        if matches!(node.kind, NodeKind::ArrayLiteral { .. }) {
            return Ok(node);
        }
        node.children()
            .into_iter()
            .find_map(|child| array(child).ok())
            .ok_or("missing qw array".into())
    }

    fn words(node: &Node) -> Result<Vec<&str>, String> {
        let NodeKind::ArrayLiteral { elements } = &array(node)?.kind else {
            return Err("expected qw array".into());
        };
        elements
            .iter()
            .map(|element| match &element.kind {
                NodeKind::String { value, interpolated: false } => Ok(value.as_str()),
                _ => Err(format!("expected literal word: {element:?}")),
            })
            .collect()
    }

    fn assert_spans(node: &Node, expected: &[(&str, usize, usize)]) -> Result<(), String> {
        let NodeKind::ArrayLiteral { elements } = &array(node)?.kind else {
            return Err("expected qw array".into());
        };
        let actual = elements
            .iter()
            .map(|element| match &element.kind {
                NodeKind::String { value, interpolated: false } => {
                    Ok((value.as_str(), element.location.start, element.location.end))
                }
                _ => Err(format!("expected literal word: {element:?}")),
            })
            .collect::<Result<Vec<_>, String>>()?;
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn qw_hash_words_are_literal_across_delimiters() -> Result<(), String> {
        for quote in [
            "qw(red # green blue)",
            "qw{red # green blue}",
            "qw[red # green blue]",
            "qw<red # green blue>",
            "qw/red # green blue/",
            "qw (red # green blue)",
            "qw(red # green\nblue)",
            "qw(red # green\r\nblue)",
        ] {
            let source = format!("my @x = {quote};");
            assert_eq!(words(&parse(&source)?)?, ["red", "#", "green", "blue"], "{source:?}");
        }
        assert_eq!(
            words(&parse("my @x = qw(red#green #tag trailing#);")?)?,
            ["red#green", "#tag", "trailing#"]
        );
        assert_eq!(
            words(&parse(
                "my @x = qw(https://example.test/docs#install https://example.test/help);"
            )?)?,
            ["https://example.test/docs#install", "https://example.test/help"]
        );
        Ok(())
    }

    #[test]
    fn qw_literal_hash_corpus_has_exact_words_and_spans() -> Result<(), String> {
        let source = include_str!("../../../test_corpus/qw_literal_hash_words.pl");
        let ast = parse(source)?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected Program".into());
        };
        assert_eq!(statements.len(), 2, "outside comment must not add AST words");
        assert_spans(
            &statements[0],
            &[
                ("#todo", 14, 19),
                ("red#green", 20, 29),
                ("#", 30, 31),
                ("blue", 32, 36),
                ("red", 37, 40),
            ],
        )?;
        assert_eq!(
            (array(&statements[0])?.location.start, array(&statements[0])?.location.end),
            (11, 41)
        );
        assert_eq!((statements[1].location.start, statements[1].location.end), (65, 78));
        assert!(
            matches!(&statements[1].kind, NodeKind::VariableDeclaration { variable, initializer: Some(value), .. }
            if matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
                && matches!(&value.kind, NodeKind::Number { value } if value == "1"))
        );
        Ok(())
    }

    #[test]
    fn qw_hash_words_keep_utf8_and_repeated_word_spans() -> Result<(), String> {
        assert_spans(
            &parse("my @x = qw(é#tag # é#tag); my $after = 1;")?,
            &[("é#tag", 11, 17), ("#", 18, 19), ("é#tag", 20, 26)],
        )
    }

    #[test]
    fn qw_prefix_comments_do_not_steal_hash_word_spans() -> Result<(), String> {
        assert_spans(&parse("my @x = qw # # fake\n(# #tag);")?, &[("#", 21, 22), ("#tag", 23, 27)])
    }

    #[test]
    fn qw_directives_preserve_hash_words() -> Result<(), String> {
        for quote in ["qw(red # green blue)", "qw (red # green blue)", "qw{red # green blue}"] {
            for directive in ["use", "no"] {
                let source = format!("{directive} Example {quote}; my $after = 1;");
                let ast = parse(&source)?;
                let NodeKind::Program { statements } = &ast.kind else {
                    return Err("expected Program".into());
                };
                assert_eq!(statements.len(), 2);
                let args = match &statements[0].kind {
                    NodeKind::Use { args, .. } | NodeKind::No { args, .. } => args,
                    _ => return Err(format!("expected directive: {:?}", statements[0])),
                };
                assert_eq!(args, &["qw(red # green blue)"], "{source:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn qw_outside_comments_and_empty_list_are_controls() -> Result<(), String> {
        assert_eq!(
            words(&parse("my @x = qw(red green); # blue qw(ignored)\nmy $after = 1;")?)?,
            ["red", "green"]
        );
        assert_eq!(words(&parse("my @x = qw();")?)?, Vec::<&str>::new());
        Ok(())
    }

    #[test]
    fn qw_unclosed_hash_words_recover_before_following_statement() -> Result<(), String> {
        let source = "my @w = qw(red # green\nprint 'tail';";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
        assert!(!parser.errors().is_empty(), "unclosed list must remain diagnosed");
        assert!(parser.errors().iter().any(|error| matches!(
            error,
            ParseError::Recovered {
                site: RecoverySite::ArgList,
                kind: RecoveryKind::InsertedCloser,
                location: 23
            }
        )));
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected Program".into());
        };
        assert_eq!(statements.len(), 2);
        assert_spans(&statements[0], &[("red", 11, 14), ("#", 15, 16), ("green", 17, 22)])?;
        assert_eq!(
            (array(&statements[0])?.location.start, array(&statements[0])?.location.end),
            (8, 23)
        );
        assert_eq!((statements[1].location.start, statements[1].location.end), (23, 35));
        assert!(statements[1].to_sexp().contains("tail"));
        assert_eq!(
            words(&parse("my @w = qw(red # green); print 'tail';")?)?,
            ["red", "#", "green"]
        );
        Ok(())
    }
}

mod subst_embedded_code_975 {
    /// Regression tests for #975: s///e and s///ee must set has_embedded_code = true.
    ///
    /// The `e` modifier evaluates the replacement as Perl code (equivalent to `eval`),
    /// so the substitution has embedded code regardless of what is in the pattern body.
    ///
    /// Run with: cargo test -p perl-parser-core -- subst_e_has_embedded_code
    use perl_parser_core::{NodeKind, Parser};
    use perl_tdd_support::must;

    /// Walk an AST and find the first Substitution node.
    fn find_first_substitution(node: &perl_parser_core::Node) -> Option<(bool, String)> {
        if let NodeKind::Substitution { has_embedded_code, modifiers, .. } = &node.kind {
            return Some((*has_embedded_code, modifiers.clone()));
        }
        for child in node.children() {
            if let Some(result) = find_first_substitution(child) {
                return Some(result);
            }
        }
        None
    }

    fn parse_subst(source: &str) -> perl_parser_core::Node {
        let mut parser = Parser::new(source);
        must(parser.parse())
    }

    // ── Site 1: primary.rs path (s/// as standalone expression with =~) ──────────

    /// `s/(\w+)/uc($1)/e` — single `e` modifier must set has_embedded_code=true.
    #[test]
    fn subst_e_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"$s =~ s/(\w+)/uc($1)/e;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(embedded, "s///e must set has_embedded_code=true (modifiers={:?})", mods);
        Ok(())
    }

    /// `s///ee` — double-eval form must also set has_embedded_code=true.
    #[test]
    fn subst_ee_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"$t =~ s/\$(\w+)/$$1/ee;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(embedded, "s///ee must set has_embedded_code=true (modifiers={:?})", mods);
        Ok(())
    }

    /// `s/a/b/ge` — combined modifiers: has_embedded_code must be true.
    #[test]
    fn subst_ge_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"$s =~ s/a/b/ge;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(embedded, "s///ge must set has_embedded_code=true (modifiers={:?})", mods);
        Ok(())
    }

    /// `s/a/b/gr` — no `e` modifier: has_embedded_code must remain false (regression guard).
    #[test]
    fn subst_no_e_modifier_does_not_set_has_embedded_code() -> Result<(), Box<dyn std::error::Error>>
    {
        let ast = parse_subst(r#"$s =~ s/a/b/gr;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(!embedded, "s///gr must NOT set has_embedded_code (modifiers={:?})", mods);
        Ok(())
    }

    /// `s/(?{1+1})/b/g` — `(?{...})` in pattern with no `e`: has_embedded_code=true
    /// via the existing pattern-body path (unchanged behavior, regression guard).
    #[test]
    fn subst_embedded_code_in_pattern_stays_true() -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"$s =~ s/(?{1+1})/b/g;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(
            embedded,
            "s///g with (?{{...}}) in pattern must keep has_embedded_code=true (modifiers={:?})",
            mods
        );
        Ok(())
    }

    // ── Site 2: quotes.rs path (s{}{} form — no =~) ──────────────────────────────

    /// `s{(\w+)}{uc($1)}e` — brace-delimited form exercises the quotes.rs code path.
    #[test]
    fn subst_quote_operator_form_e_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>>
    {
        let ast = parse_subst(r#"s{(\w+)}{uc($1)}e;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(
            embedded,
            "s{{}}{{}}e (quote-operator form) must set has_embedded_code=true (modifiers={:?})",
            mods
        );
        Ok(())
    }

    // ── Sexp marker test ─────────────────────────────────────────────────────────

    /// The `(has_embedded_code true)` marker must appear in the sexp for `s///e`.
    #[test]
    fn subst_e_sexp_contains_risk_code_marker() {
        let mut parser = Parser::new(r#"$s =~ s/(\w+)/uc($1)/e;"#);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(has_embedded_code true)"),
            "sexp for s///e must contain '(has_embedded_code true)' marker; got:\n{}",
            sexp
        );
    }

    // ── Site 2 supplemental: quotes.rs false branch and both-true path ───────────

    /// `s{a}{b}g` — brace-delimited form with NO `e` modifier: has_embedded_code must be false.
    /// This covers the `modifiers.contains('e')` → false branch in quotes.rs.
    #[test]
    fn subst_quote_operator_form_no_e_does_not_set_has_embedded_code()
    -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"s{a}{b}g;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(
            !embedded,
            "s{{}}{{}}g (quote-operator, no e modifier) must NOT set has_embedded_code (modifiers={:?})",
            mods
        );
        Ok(())
    }

    /// `s{a}{b}ee` — brace-delimited form with `ee` double-eval modifier: has_embedded_code must be true.
    /// This covers the `ee` path in quotes.rs (modifiers.contains('e') is true for 'ee').
    #[test]
    fn subst_quote_operator_form_ee_sets_has_embedded_code()
    -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"s{a}{b}ee;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(
            embedded,
            "s{{}}{{}}ee (quote-operator, double-eval) must set has_embedded_code=true (modifiers={:?})",
            mods
        );
        Ok(())
    }

    /// `s{(?{1+1})}{b}e` — both `(?{...})` in pattern body AND `e` modifier active simultaneously.
    /// Verifies the `||` does not short-circuit incorrectly when both sides are true (quotes.rs path).
    #[test]
    fn subst_quote_operator_form_both_embedded_code_and_e_modifier()
    -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_subst(r#"s{(?{1+1})}{b}e;"#);
        let (embedded, mods) =
            find_first_substitution(&ast).ok_or("should find a Substitution node")?;
        assert!(
            embedded,
            "s{{(?{{...}})}}{{}}e (both pattern body and e modifier) must set has_embedded_code=true (modifiers={:?})",
            mods
        );
        Ok(())
    }

    /// `find_first_substitution` returns `None` when the AST has no Substitution node.
    /// Pins the `None` branch of the helper: a non-substitution expression must yield None.
    /// RIPR seam: `find_first_substitution` return_value None at line 20.
    #[test]
    fn find_first_substitution_returns_none_for_non_subst_ast() {
        // Parse a plain assignment — no s/// anywhere in the AST.
        let ast = parse_subst(r#"my $x = 42;"#);
        assert_eq!(
            find_first_substitution(&ast),
            None,
            "find_first_substitution must return None for an AST with no Substitution node"
        );
    }
}

mod range_operators {
    // Focused regression proof for #13903: the two-dot range operator is
    // non-associative. One unparenthesized range parses cleanly, an adjacent
    // second `..` at the same precedence level is a syntax error, and explicit
    // parentheses remain the only way to nest ranges.
    //
    // Precedence-boundary controls keep #13128's symbolic-operator ordering
    // intact.

    use super::cpan_test_helpers::{assert_clean_parse, assert_has_blocking_error, parse};
    use perl_parser_core::{Node, NodeKind};
    use perl_tdd_support::must_some;

    #[derive(Clone, Copy)]
    enum NestedSide {
        Left,
        Right,
    }

    fn first_binary(node: &Node) -> Option<&Node> {
        if matches!(&node.kind, NodeKind::Binary { .. }) {
            return Some(node);
        }

        for child in node.children() {
            if let Some(binary) = first_binary(child) {
                return Some(binary);
            }
        }

        None
    }

    fn binary_parts(node: &Node) -> Option<(&str, &Node, &Node)> {
        match &node.kind {
            NodeKind::Binary { op, left, right } => {
                Some((op.as_str(), left.as_ref(), right.as_ref()))
            }
            _ => None,
        }
    }

    fn first_ternary(node: &Node) -> Option<&Node> {
        if matches!(&node.kind, NodeKind::Ternary { .. }) {
            return Some(node);
        }

        for child in node.children() {
            if let Some(ternary) = first_ternary(child) {
                return Some(ternary);
            }
        }

        None
    }

    fn ternary_condition(node: &Node) -> Option<&Node> {
        match &node.kind {
            NodeKind::Ternary { condition, .. } => Some(condition.as_ref()),
            _ => None,
        }
    }

    fn assert_nested_binary(
        source: &str,
        outer_op: &str,
        nested_side: NestedSide,
        nested_op: &str,
    ) {
        assert_clean_parse(source);
        let ast = parse(source);
        let outer = must_some(first_binary(&ast));
        let (actual_outer, left, right) = must_some(binary_parts(outer));
        assert_eq!(
            actual_outer,
            outer_op,
            "unexpected outer operator for `{source}`:\n{}",
            ast.to_sexp()
        );

        let nested = match nested_side {
            NestedSide::Left => left,
            NestedSide::Right => right,
        };
        let (actual_nested, _, _) = must_some(binary_parts(nested));
        assert_eq!(
            actual_nested,
            nested_op,
            "unexpected nested operator for `{source}`:\n{}",
            ast.to_sexp()
        );
    }

    #[test]
    fn single_range_still_parses_cleanly() {
        assert_clean_parse("my @r = (1 .. 3);");
        assert_clean_parse("my $x = $a .. $b;");
        assert_clean_parse("for my $i (1 .. 10) { }");
    }

    #[test]
    fn parenthesized_range_nesting_is_accepted_on_both_sides() {
        // `(1 .. 2) .. 3` — outer range whose left operand is a range.
        assert_nested_binary("my $x = (1 .. 2) .. 3;", "..", NestedSide::Left, "..");
        // `1 .. (2 .. 3)` — outer range whose right operand is a range.
        assert_nested_binary("my $x = 1 .. (2 .. 3);", "..", NestedSide::Right, "..");
    }

    #[test]
    fn unparenthesized_chained_range_is_not_a_clean_parse() {
        // Perl 5.40.1: `1 .. 2 .. 3;` is a syntax error near `2 ..`. The chained
        // form must surface a blocking diagnostic and/or Error/Missing node — it
        // must not silently publish the pre-#13903 clean left-folded AST.
        assert_has_blocking_error("my $x = $a .. $b .. $c;", "range");
        assert_has_blocking_error("1 .. 2 .. 3;", "range");
    }

    #[test]
    fn symbolic_operators_remain_inside_range_operands() {
        // #13128 precedence boundary: symbolic bitwise/logical operators stay
        // inside range operands after the non-associativity change.
        assert_nested_binary("$a || $b .. $c;", "..", NestedSide::Left, "||");
        assert_nested_binary("$a .. $b || $c;", "..", NestedSide::Right, "||");
        assert_nested_binary("$a & $b .. $c;", "..", NestedSide::Left, "&");
        assert_nested_binary("$a .. $b & $c;", "..", NestedSide::Right, "&");
    }

    #[test]
    fn range_remains_inside_ternary_condition() {
        let source = "$a .. $b ? $c : $d;";
        assert_clean_parse(source);
        let ast = parse(source);
        let ternary = must_some(first_ternary(&ast));
        let condition = must_some(ternary_condition(ternary));
        let (condition_op, _, _) = must_some(binary_parts(condition));
        assert_eq!(
            condition_op,
            "..",
            "range must remain inside the ternary condition:\n{}",
            ast.to_sexp()
        );
    }

    #[test]
    fn chained_range_after_symbolic_operator_is_still_rejected() {
        // The second `..` must not sneak back in through a symbolic-operator
        // operand at the same precedence level.
        assert_has_blocking_error("$a || $b .. $c .. $d;", "range");
    }

    #[test]
    fn symbolic_logical_and_bitwise_ops_bind_inside_left_range_operand() {
        for (source, nested_op) in [
            ("$a & $b .. $c;", "&"),
            ("$a ^ $b .. $c;", "^"),
            ("$a | $b .. $c;", "|"),
            ("$a && $b .. $c;", "&&"),
            ("$a || $b .. $c;", "||"),
            ("$a // $b .. $c;", "//"),
        ] {
            assert_nested_binary(source, "..", NestedSide::Left, nested_op);
        }
    }

    #[test]
    fn symbolic_logical_and_bitwise_ops_bind_inside_right_range_operand() {
        for (source, nested_op) in [
            ("$a .. $b & $c;", "&"),
            ("$a .. $b ^ $c;", "^"),
            ("$a .. $b | $c;", "|"),
            ("$a .. $b && $c;", "&&"),
            ("$a .. $b || $c;", "||"),
            ("$a .. $b // $c;", "//"),
        ] {
            assert_nested_binary(source, "..", NestedSide::Right, nested_op);
        }
    }

    #[test]
    fn declaration_tail_uses_the_same_range_precedence() {
        assert_nested_binary("(my $x || $fallback .. $end);", "..", NestedSide::Left, "||");
    }

    #[test]
    fn range_precedence_13128_remains_inside_ternary_condition() {
        let source = "$a .. $b ? $c : $d;";
        assert_clean_parse(source);
        let ast = parse(source);
        let ternary = must_some(first_ternary(&ast));
        let condition = must_some(ternary_condition(ternary));
        let (condition_op, _, _) = must_some(binary_parts(condition));
        assert_eq!(
            condition_op,
            "..",
            "range must bind above ternary for `{source}`:\n{}",
            ast.to_sexp()
        );
    }
}

mod foreach_bare_variable {
    use super::cpan_test_helpers::*;

    // foreach (LIST) — implicit $_ topic variable
    #[test]
    fn foreach_implicit_topic_no_variable() {
        assert_clean_parse("foreach (@INC) { print; }");
    }

    #[test]
    fn foreach_implicit_topic_keys() {
        assert_clean_parse("foreach (keys %ENV) { print \"$_\\n\"; }");
    }

    #[test]
    fn foreach_implicit_topic_array_ref() {
        assert_clean_parse("foreach (@{$list}) { process($_); }");
    }

    // foreach $var (LIST) — bare scalar without my
    #[test]
    fn foreach_bare_scalar_no_my() {
        assert_clean_parse("foreach $mod (@ISA) { eval \"require $mod\"; }");
    }

    #[test]
    fn foreach_bare_scalar_dirs() {
        assert_clean_parse("foreach $dir (@dirs) { push @found, $dir if -d $dir; }");
    }

    #[test]
    fn for_bare_scalar_no_my() {
        assert_clean_parse("for $mod (@ISA) { print $mod; }");
    }

    // Regression: existing working cases still work
    #[test]
    fn foreach_with_my_still_works() {
        assert_clean_parse("foreach my $item (@list) { print $item; }");
    }

    #[test]
    fn for_with_my_still_works() {
        assert_clean_parse("for my $i (1..10) { print $i; }");
    }

    // Labeled foreach with implicit $_
    #[test]
    fn labeled_foreach_implicit_topic() {
        assert_clean_parse("LOOP: foreach (@args) { next LOOP if $_ eq 'skip'; }");
    }
}

mod foreach_c_style_loop {
    use super::cpan_test_helpers::*;

    // Pattern A from #2149: C-style foreach loop
    //
    // In Perl, `for` and `foreach` are fully interchangeable.
    // `foreach (my $i=0; $i<10; $i++) { ... }` is valid C-style syntax.
    // The parser's parse_foreach_statement() only handled list-style foreach,
    // not C-style. This fix delegates to C-style for loop parsing when
    // semicolons are detected inside the parens.

    #[test]
    fn test_foreach_c_style_basic() {
        // Basic C-style foreach loop
        assert_clean_parse(r#"foreach (my $i=0; $i<10; $i++) { print $i; }"#);
    }

    #[test]
    fn test_foreach_c_style_array_length() {
        // C-style foreach with array-length condition
        assert_clean_parse(r#"foreach (my $i=0; $i<@arr; $i+=2) { push @out, $arr[$i]; }"#);
    }

    #[test]
    fn test_foreach_c_style_decrement() {
        // C-style foreach decrementing
        assert_clean_parse(r#"foreach (my $i = $n-1; $i >= 0; $i--) { next; }"#);
    }

    #[test]
    fn test_foreach_c_style_complex_condition() {
        // C-style foreach with complex condition (from CPAN: Biber/Entry/Names.pm)
        assert_clean_parse(
            r#"foreach (my $x=0; $x <= $#choices; $x++) { push @ret, $choices[$x]; }"#,
        );
    }

    #[test]
    fn test_foreach_c_style_expression_init() {
        // C-style foreach where init is a plain expression (no my)
        assert_clean_parse(r#"foreach ($i = 0; $i < 10; $i++) { print $i; }"#);
    }

    #[test]
    fn test_foreach_c_style_empty_parts() {
        // C-style foreach with empty init
        assert_clean_parse(r#"foreach (; $i < 10; $i++) { print $i; }"#);
    }

    #[test]
    fn test_foreach_c_style_all_empty() {
        // C-style foreach with all parts empty (infinite loop)
        assert_clean_parse(r#"foreach (;;) { last; }"#);
    }

    #[test]
    fn test_foreach_c_style_nested() {
        // Nested C-style foreach loops
        assert_clean_parse(
            r#"foreach (my $i = 0; $i < 10; $i++) {
            foreach (my $j = 0; $j < 10; $j++) {
                print "$i $j\n";
            }
        }"#,
        );
    }

    // Regression tests — list-style foreach must still work

    #[test]
    fn test_foreach_list_style_regression() {
        // Standard list-style foreach
        assert_clean_parse(r#"foreach my $x (@list) { print $x; }"#);
    }

    #[test]
    fn test_foreach_list_style_bare_var_regression() {
        // Bare variable in list-style foreach
        assert_clean_parse(r#"foreach $item (@items) { print $item; }"#);
    }

    #[test]
    fn test_foreach_list_style_implicit_topic_regression() {
        // Implicit $_ topic variable
        assert_clean_parse(r#"foreach (@list) { print; }"#);
    }

    #[test]
    fn test_for_c_style_still_works_regression() {
        // C-style `for` must still work (not just `foreach`)
        assert_clean_parse(r#"for (my $i=0; $i<10; $i++) { print $i; }"#);
    }

    #[test]
    fn test_for_list_style_still_works_regression() {
        // List-style `for` must still work
        assert_clean_parse(r#"for my $x (@list) { print $x; }"#);
    }
}

mod foreach_no_variable_tests {
    use super::cpan_test_helpers::*;

    /// Strict clean parse check that also catches `(ERROR ...)` nodes.
    fn assert_strict_clean_parse(source: &str) {
        let ast = parse(source);
        let sexp = ast.to_sexp();
        let markers = [
            "(error ",
            "(Error ",
            "(ERROR ",
            "(missing_expression",
            "(missing_statement",
            "(missing_identifier",
            "(missing_block",
            "MissingExpression",
            "MissingStatement",
            "MissingIdentifier",
            "MissingBlock",
        ];
        for marker in &markers {
            assert!(
                !sexp.contains(marker),
                "Clean-parse assertion failed: found '{}' in sexp for source:\n{}\n\nsexp:\n{}",
                marker,
                source,
                sexp,
            );
        }
    }

    // --- foreach with variable declarators ---

    #[test]
    fn test_foreach_our_variable() {
        assert_strict_clean_parse("foreach our $item (@list) { print $item; }");
    }

    #[test]
    fn test_for_our_variable() {
        assert_strict_clean_parse("for our $item (@list) { print $item; }");
    }

    // --- foreach/for with implicit $_ ---

    #[test]
    fn test_foreach_implicit_topic() {
        assert_strict_clean_parse("foreach (@list) { print; }");
    }

    #[test]
    fn test_for_implicit_topic() {
        assert_strict_clean_parse("for (@array) { print; }");
    }

    // --- foreach/for with bare scalar ---

    #[test]
    fn test_foreach_bare_scalar() {
        assert_strict_clean_parse("foreach $item (@list) { print $item; }");
    }

    #[test]
    fn test_for_bare_scalar() {
        assert_strict_clean_parse("for $item (@list) { print $item; }");
    }

    // --- foreach/for my (standard) ---

    #[test]
    fn test_foreach_my_variable() {
        assert_strict_clean_parse("foreach my $item (@list) { print $item; }");
    }

    #[test]
    fn test_for_my_variable() {
        assert_strict_clean_parse("for my $item (@list) { print $item; }");
    }

    // --- nested, continue, complex lists ---

    #[test]
    fn test_foreach_nested_implicit() {
        assert_strict_clean_parse("for (@outer) { for (@inner) { print; } }");
    }

    #[test]
    fn test_foreach_with_continue() {
        assert_strict_clean_parse(
            r#"foreach my $item (@list) { print $item; } continue { print "next\n"; }"#,
        );
    }
}

mod qw_space_delimiter {
    use super::cpan_test_helpers::*;

    // Tests for issue #2895: qw with space before single-quote (or double-quote)
    // delimiter fails inside function calls.
    //
    // Root Cause: Commit 7741fcd8 (#2815) introduced a restriction that only
    // paired delimiters ({, [, (, <) are accepted after whitespace. This was
    // over-broad — it also blocked ' and " which are unambiguous for all
    // quote operators except `s` (where `-s 'filename'` is a filetest).
    //
    // Fix: `crates/perl-lexer/src/lib.rs` — add `is_quote_char` predicate in
    // `try_identifier_or_keyword` Path 2, guarded with `op != "s"`.

    // ── Primary failing case: qw with space before single-quote in a call ────────

    /// foo(qw 'A B') — the primary regression: qw + space + single-quote inside
    /// a function call argument list. Without the fix, the lexer treats `qw` as
    /// an identifier and `'A B')` as a single-quoted string that consumes the `)`.
    #[test]
    fn qw_space_squote_in_call() {
        assert_clean_parse(r#"foo(qw 'A B');"#);
    }

    /// $_[0]->_cached_tmpdir(qw 'TMPDIR TEMP TMP') — the exact OS2.pm pattern
    /// that generates unexpected_rparen_expr cascade failures.
    #[test]
    fn qw_space_squote_method_call() {
        assert_clean_parse(r#"$_[0]->_cached_tmpdir(qw 'TMPDIR TEMP TMP');"#);
    }

    /// Two qw with space+single-quote in the same call — ensures both are
    /// lexed correctly, not just the first one.
    #[test]
    fn qw_space_squote_double_in_call() {
        assert_clean_parse(r#"f(qw 'A B', qw 'C D');"#);
    }

    /// my $x = [qw /A B/]; my $y = 1 — slash delimiters after whitespace must not
    /// swallow the closing bracket or following statement. This is the
    /// Regexp::Common::zip corpus shape behind the current unclosed_bracket bucket.
    #[test]
    fn qw_space_slash_inside_array_preserves_following_tokens() {
        let ast = parse(
            r#"my $x = ['zip', 'Australia' => qw /-prefix= -country= -lax=/];
    my $y = 1;"#,
        );
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable (sigil $) (name y))"),
            "expected parser to continue after slash-delimited qw list, got: {sexp}"
        );
        assert_clean_parse(
            r#"my $x = ['zip', 'Australia' => qw /-prefix= -country= -lax=/];
    my $y = 1;"#,
        );
    }

    // ── Statement-level (already works — non-regression baseline) ────────────────

    /// my @x = qw 'A B' — at statement level. This worked before; must remain
    /// working after the fix.
    #[test]
    fn qw_space_squote_statement() {
        assert_clean_parse(r#"my @x = qw 'A B';"#);
    }

    // ── Double-quote delimiter variant ───────────────────────────────────────────

    /// my @x = qw "A B" — double-quote delimiter with space. The fix must cover
    /// " in addition to '.
    #[test]
    fn qw_space_dquote_statement() {
        assert_clean_parse("my @x = qw \"A B\";");
    }

    // ── Newline separator ────────────────────────────────────────────────────────

    /// my @x = qw\n'A B' — newline between qw and delimiter. peek_nonspace_and_following
    /// already skips newlines; this confirms the fix handles that path too.
    #[test]
    fn qw_newline_squote_statement() {
        assert_clean_parse("my @x = qw\n'A B';");
    }

    // ── Other q-family operators with space + single-quote ───────────────────────

    /// my $x = q 'hello world' — single-char q operator, not s. Must be allowed.
    #[test]
    fn q_space_squote_statement() {
        assert_clean_parse(r#"my $x = q 'hello world';"#);
    }

    /// my $x = qq 'hello $name' — qq operator with space + single-quote delimiter.
    #[test]
    fn qq_space_squote_statement() {
        assert_clean_parse(r#"my $x = qq 'hello world';"#);
    }

    /// if ($x =~ m 'foo') { 1 } — m operator with space + single-quote delimiter.
    #[test]
    fn m_space_squote_in_condition() {
        assert_clean_parse(r#"if ($x =~ m 'foo') { 1 }"#);
    }

    // ── No-space baseline: must be unchanged by the fix ──────────────────────────

    /// foo(qw'A B') — adjacent single-quote, no space. Must continue to work.
    #[test]
    fn qw_nospace_squote_unchanged() {
        assert_clean_parse(r#"foo(qw'A B');"#);
    }

    /// foo(qw(A B)) — parenthesis delimiter, no space. Must continue to work.
    #[test]
    fn qw_parens_unchanged() {
        assert_clean_parse(r#"foo(qw(A B));"#);
    }

    // ── Other operators: qr and qx with space + single-quote ─────────────────────

    /// my $re = qr 'foo' — qr operator with space + single-quote delimiter.
    /// is_quote_char covers qr (op != "s" is true), so this must be accepted.
    #[test]
    fn qr_space_squote_statement() {
        assert_clean_parse(r#"my $re = qr 'foo';"#);
    }

    /// my $out = qx 'date' — qx operator with space + single-quote delimiter.
    /// is_quote_char covers qx (op != "s" is true), so this must be accepted.
    #[test]
    fn qx_space_squote_statement() {
        assert_clean_parse(r#"my $out = qx 'date';"#);
    }

    // ── Critical regression guard: -s 'filename' must stay a filetest ────────────

    /// if (-s 'tmpfile') { ... } — file-size filetest with string literal.
    /// The op != "s" guard in is_quote_char prevents -s 'filename' from being
    /// misinterpreted as a substitution operator start.
    #[test]
    fn s_filetest_with_string_literal_not_subst() {
        assert_clean_parse(r#"if (-s 'tmpfile') { print "has content"; }"#);
    }
}

mod subst_squote_delimiter {
    use super::cpan_test_helpers::*;

    // Tests for issue #2896: s/''/'/g single-quote in substitution replacement
    // with slash delimiter — the string-skip arm incorrectly treats `'` as a
    // string opener, consuming past the closing `/`.

    /// Core failing case: empty pattern, single-quote replacement, slash delimiter.
    /// From TAP/Parser/YAMLish/Reader.pm and Log/Log4perl/DateFormat.pm.
    #[test]
    fn subst_slash_delim_squote_replacement() {
        // $literal =~ s/''/'/g
        assert_clean_parse(r#"$literal =~ s/''/'/g;"#);
    }

    /// Variant: single-quote pattern with double-single-quote replacement.
    /// From PgCommon.pm — this may already pass; kept as a regression guard.
    #[test]
    fn subst_slash_delim_squote_pattern_squote_replacement() {
        // $value =~ s/'/''/g
        assert_clean_parse(r#"$value =~ s/'/''/g;"#);
    }

    /// Double-quote variant: same structure with `"` as the replacement content.
    #[test]
    fn subst_slash_delim_dquote_replacement() {
        assert_clean_parse(r#"$x =~ s/""/"/g;"#);
    }

    /// Single literal quote as the full replacement with /e modifier.
    #[test]
    fn subst_slash_delim_squote_replacement_e_modifier() {
        assert_clean_parse(r#"$x =~ s/pat/'/e;"#);
    }

    /// Regression: s///e with string containing closing delimiter must still work.
    /// This exercises the string-skip logic that was added for issue #2395.
    #[test]
    fn subst_e_string_containing_closing_delim_regression() {
        assert_clean_parse(r#"$str =~ s/([A-Za-z]+)/join('/', @parts)/ge;"#);
    }

    /// Regression: s///e with double-quoted string containing /.
    #[test]
    fn subst_e_dquoted_string_containing_slash_regression() {
        assert_clean_parse(r#"$str =~ s/foo/sprintf("%s/%s", $a, $b)/e;"#);
    }

    /// Regression: plain s/// with slash delimiter still works.
    #[test]
    fn subst_slash_delim_basic_regression() {
        assert_clean_parse(r#"$x =~ s/foo/bar/g;"#);
    }
}

mod subst_unusual_delimiters_2732 {
    use super::cpan_test_helpers::*;

    // Tests for issue #2732: substitution/transliteration modifier parsing fixes
    //
    // Root Cause 1A: Path 2 in lexer uses current_char() instead of peek_nonspace()
    //   for s/tr/y/m detection — fails when whitespace precedes the delimiter.
    //
    // Root Cause 1B: after_arrow flag persists across statement boundaries (;, ), })
    //   causing s/// on the next statement to be treated as an identifier.
    //
    // Root Cause 2: is_quote_delim() rejects control characters via .is_control(),
    //   but Perl allows any non-alphanumeric, non-whitespace delimiter (e.g. BEL \x07).
    //
    // Root Cause 3: parse_hash_subscript_key() doesn't handle quote-operator
    //   identifiers (s, m, tr, y, q, qq, qw, qr, qx) as bareword hash keys.

    // ── Root Cause 1A: whitespace before delimiter ──────────────────────────────

    /// s { pattern } { replacement }g — space before opening brace
    #[test]
    fn test_subst_space_before_brace() {
        assert_clean_parse("s { foo } { bar }g;");
    }

    /// s [pattern] [replacement]ex — space before bracket (MakeMaker.pm pattern)
    #[test]
    fn test_subst_space_before_bracket() {
        let source = r#"$value =~ s [^~(\w*)] [$1]ex;"#;
        assert_clean_parse(source);
    }

    /// s < pattern > <replacement> — space before angle bracket (Data/Dumper.pm pattern)
    #[test]
    fn test_subst_space_before_angle() {
        let source = r#"$x =~ s <foo> <bar>;"#;
        assert_clean_parse(source);
    }

    /// s\n{pattern}{replacement}g — newline before delimiter (diagnostics.pm pattern)
    #[test]
    fn test_subst_newline_before_delimiter() {
        assert_clean_parse("s\n{foo}\n{bar}g;");
    }

    // ── Root Cause 1B: after_arrow persists across statement boundary ────────────

    /// s/// on next statement after ->() call — after_arrow must be cleared by ;
    /// Reproduces Filter/Simple.pm: $transform->(@_); s/$extractor/.../g;
    #[test]
    fn test_subst_after_arrow_call_statement() {
        let source = r#"$transform->(@_); s/foo/bar/g;"#;
        assert_clean_parse(source);
    }

    /// after_arrow cleared by ) — method call inside expression
    #[test]
    fn test_subst_after_method_call_paren() {
        let source = r#"my $x = $obj->method(); s/foo/bar/;"#;
        assert_clean_parse(source);
    }

    /// after_arrow cleared by } — block following method call
    #[test]
    fn test_subst_after_arrow_in_block() {
        let source = r#"if ($obj->thing) { s/foo/bar/; }"#;
        assert_clean_parse(source);
    }

    // ── Root Cause 2: control character delimiter ────────────────────────────────

    /// s\x07pattern\x07replacement\x07 — BEL as delimiter (perl5db.pl pattern)
    #[test]
    fn test_subst_bel_delimiter() {
        // BEL character (\x07) as substitution delimiter
        assert_clean_parse("s\x07foo\x07bar\x07;");
    }

    // ── Root Cause 3: hash key 's' and other quote-op identifiers ───────────────

    /// $_->{s} — 's' as bare hash subscript key (Biber/Config.pm pattern)
    #[test]
    fn test_hash_key_s() {
        let source = r#"my $x = $_->{s};"#;
        assert_clean_parse(source);
    }

    /// $_->{m} — 'm' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_m() {
        let source = r#"my $x = $_->{m};"#;
        assert_clean_parse(source);
    }

    /// $_->{tr} — 'tr' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_tr() {
        let source = r#"my $x = $_->{tr};"#;
        assert_clean_parse(source);
    }

    /// $_->{y} — 'y' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_y() {
        let source = r#"my $x = $_->{y};"#;
        assert_clean_parse(source);
    }

    /// Hash slice with quote-op keys in fat-arrow list context — s/m/tr/y before => must autoquote.
    /// Regression: with peek_nonspace(), s before => was being treated as substitution operator.
    #[test]
    fn test_hash_quote_op_fat_arrow() {
        let source = r#"my %h = (s => 1, m => 2, tr => 3, y => 4);"#;
        assert_clean_parse(source);
    }

    // ── Regression guards: existing adjacent-delimiter forms must still work ─────

    /// s/foo/bar/g — adjacent slash delimiter unaffected
    #[test]
    fn test_subst_adjacent_slash_regression() {
        assert_clean_parse("s/foo/bar/g;");
    }

    /// s{foo}{bar}g — adjacent brace delimiter unaffected
    #[test]
    fn test_subst_adjacent_brace_regression() {
        assert_clean_parse("s{foo}{bar}g;");
    }

    /// $obj->s — method named 's' must NOT be treated as substitution operator
    #[test]
    fn test_arrow_method_named_s_regression() {
        let source = r#"$obj->s("arg");"#;
        assert_clean_parse(source);
    }

    // ── Root Cause 3 extended: q-family hash keys ────────────────────────────────

    /// $_->{q} — 'q' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_q() {
        let source = r#"my $x = $_->{q};"#;
        assert_clean_parse(source);
    }

    /// $_->{qq} — 'qq' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_qq() {
        let source = r#"my $x = $_->{qq};"#;
        assert_clean_parse(source);
    }

    /// $_->{qw} — 'qw' as bare hash subscript key via arrow
    #[test]
    fn test_hash_key_qw() {
        let source = r#"my $x = $_->{qw};"#;
        assert_clean_parse(source);
    }

    // ── Regression guard: chained hash access with arrow ────────────────────────

    /// $h->{outer}->{inner} — chained hash access must not break after_arrow clearing on }
    /// If clearing after_arrow on } breaks chained access, this will parse with errors.
    #[test]
    fn test_chained_hash_access_regression() {
        let source = r#"my $x = $h->{outer}->{inner};"#;
        assert_clean_parse(source);
    }

    /// $h->{s}->{m} — chained access with quote-op keys at each level
    #[test]
    fn test_chained_hash_access_quote_op_keys() {
        let source = r#"my $x = $h->{s}->{m};"#;
        assert_clean_parse(source);
    }

    // ── Non-arrow hash subscript with quote-op keys ──────────────────────────────
    //
    // $h{s} (no arrow) was previously broken — the lexer treated `s` as a quote
    // operator because after_arrow was false. Fixed by subsequent lexer work
    // (after_var_subscript tracking in PR #2833/#2844), which correctly suppresses
    // quote-operator recognition inside hash subscript braces even without `->`.

    /// $h{s} — bare hash subscript (no arrow) with quote-op key.
    /// Previously broken: lexer treated `s` as substitution operator.
    /// Fixed by after_var_subscript lexer tracking.
    #[test]
    fn test_non_arrow_hash_key_s() {
        let source = r#"my $x = $h{s};"#;
        assert_clean_parse(source);
    }

    /// $h{q} — bare hash subscript with 'q' key.
    /// Previously broken: lexer treated `q` as quote operator.
    /// Fixed by after_var_subscript lexer tracking.
    #[test]
    fn test_non_arrow_hash_key_q() {
        let source = r#"my $x = $h{q};"#;
        assert_clean_parse(source);
    }

    // ── Regression: space before paren delimiter (all four paired chars covered) ─

    /// s (pattern) (replacement) — space before paren delimiter.
    /// Parens are one of the four paired delimiters ({, [, (, <) that are allowed
    /// after whitespace, but this case was not previously tested.
    #[test]
    fn test_subst_space_before_paren() {
        let source = r#"$x =~ s (foo) (bar);"#;
        assert_clean_parse(source);
    }

    /// Upstream Perl `t/base/lex.t` uses a legal substitution where `s` is followed
    /// by a line comment before the paired pattern and replacement delimiters.
    #[test]
    fn test_subst_comment_gap_before_paired_pattern_delimiter() {
        let source = "$_ = \"a\";\ns # comment\n [a] #\n [b] #\n ;";
        assert_clean_parse(source);
    }

    /// Module::Build::Platform::Unix::_detildefy uses a comment and newline gap
    /// between the paired pattern and replacement delimiters. Keep the extracted
    /// CPAN shape here so the corpus bucket remains tied to the real source rather
    /// than only to a smaller synthetic delimiter example.
    #[test]
    fn test_module_build_platform_unix_detildefy_substitution() {
        let source = r#"$value =~ s[^~([^/]+)?(?=/|$)]   # tilde with optional username
      [$1 ?
       (eval{(getpwnam $1)[7]} || "~$1") :
       ($ENV{HOME} || eval{(getpwuid $>)[7]} || glob("~"))
      ]ex;"#;
        assert_clean_parse(source);
    }

    /// Upstream Perl `t/base/lex.t` also exercises quote-like operators where a
    /// line comment separates the operator from the first delimiter.
    #[test]
    fn test_quote_ops_comment_gap_before_delimiter() {
        assert_clean_parse("$x = q # comment\n \"b\"#;");
        assert_clean_parse("$x = qq # comment\n \"b\"#;");
        assert_clean_parse("@x = qw # comment\n \"b\"#;");
        assert_clean_parse("\"b\" =~ m # comment\n \"b\"#;");
        assert_clean_parse("$x = qr # comment\n \"b\"#;");
    }

    #[test]
    fn test_base_lex_comment_gap_quote_ops_in_comparisons() {
        let source = r##"
    q # comment
     "b"#
      eq 'b' or print "not ";
    qq # comment
     "b"#
      eq 'b' or print "not ";
    qw # comment
     "b"#
      [0] eq 'b' or print "not ";
    "b" =~ m # comment
     "b"#
      or print "not ";
    qr # comment
     "b"#
      eq qr/b/ or print "not ";
    "##;
        assert_clean_parse(source);
    }

    #[test]
    fn test_base_lex_fat_arrow_quotes_keyword_across_lines() {
        let source = "print \"not \" unless (time\n                     =>) eq time=>;";
        assert_clean_parse(source);
    }

    /// Mixed delimiter substitution: paired pattern + slash replacement.
    /// Perl accepts `s{foo}/bar/`, and this must not be misclassified as
    /// missing replacement.
    #[test]
    fn test_subst_mixed_paired_and_slash_delims() {
        assert_clean_parse(r#"$x =~ s{foo}/bar/;"#);
    }

    /// Mixed delimiter substitution with all other paired-open delimiter variants.
    /// The fix must work for `[`, `(`, and `<` paired openers, not just `{`.
    #[test]
    fn test_subst_mixed_bracket_and_slash_delims() {
        assert_clean_parse(r#"$x =~ s[foo]/bar/;"#);
    }

    #[test]
    fn test_subst_mixed_paren_and_slash_delims() {
        assert_clean_parse(r#"$x =~ s(foo)/bar/;"#);
    }

    #[test]
    fn test_subst_mixed_angle_and_slash_delims() {
        assert_clean_parse(r#"$x =~ s<foo>/bar/;"#);
    }

    /// Mixed delimiter substitution with a non-slash non-paired replacement.
    /// Perl also accepts `s{foo}!bar!` (any non-alphanumeric, non-whitespace char).
    #[test]
    fn test_subst_mixed_paired_and_pipe_delims() {
        assert_clean_parse(r#"$x =~ s{foo}|bar|;"#);
    }

    /// Missing replacement after paired pattern delimiter must still error.
    /// `s{foo}` with no second part is malformed.
    #[test]
    fn test_subst_paired_pattern_missing_replacement_errors() {
        assert_has_error(r#"$x =~ s{foo};"#, "Missing");
    }

    /// Paired-pattern substitutions may use a closed alphanumeric replacement
    /// delimiter. Perl deparses this as `s/foo/y/`.
    #[test]
    fn test_subst_paired_pattern_alphanumeric_replacement_delimiter() {
        assert_clean_parse(r#"$x =~ s{foo}xyx;"#);
    }

    /// Malformed paired substitution with an unclosed alphanumeric replacement
    /// delimiter should report the missing close, not pretend the replacement is
    /// absent.
    #[test]
    fn test_subst_paired_pattern_unclosed_alphanumeric_replacement_delimiter_errors() {
        assert_has_error(r#"$x =~ s{foo}xyz;"#, "Missing closing delimiter");
    }

    /// Mixed delimiter transliteration: paired search + slash replacement.
    /// Perl accepts `tr{abc}/xyz/`.
    #[test]
    fn test_transliteration_mixed_paired_and_slash_delims() {
        assert_clean_parse(r#"$x =~ tr{abc}/xyz/;"#);
    }

    /// Mixed delimiter transliteration for bracket and paren paired openers.
    #[test]
    fn test_transliteration_mixed_bracket_and_slash_delims() {
        assert_clean_parse(r#"$x =~ tr[abc]/xyz/;"#);
    }

    // ── Regression: non-paired delimiters after whitespace must be rejected ───────

    /// -s $bs — file-size test, not substitution. The original XSLoader.pm regression.
    /// `s` here is a bareword file-test operator; `$bs` is its argument.
    /// With whitespace before `$`, the lexer must NOT treat `$` as a delimiter.
    #[test]
    fn test_file_size_test_not_subst() {
        let source = r#"goto \&XSLoader::bootstrap_inherit if not -f $file or -s $bs;"#;
        assert_clean_parse(source);
    }

    /// -s $bs in assignment context — variant of the core regression
    #[test]
    fn test_file_size_test_in_condition() {
        let source = r#"if (-s $file) { print "has content"; }"#;
        assert_clean_parse(source);
    }

    // ── Regression: comma-delimited substitution (XSLoader.pm) ──────────────────

    /// XSLoader.pm line 43: s,[\\/][^\\/]+$,, — comma as substitution delimiter.
    /// This is a non-standard but valid Perl delimiter. Must parse cleanly.
    #[test]
    fn test_comma_delimited_subst_xsloader() {
        let source = r#"$modlibname =~ s,[\\/][^\\/]+$,, while $c--;"#;
        assert_clean_parse(source);
    }

    /// Diagnostic preservation: invalid substitution modifiers should remain
    /// reported as errors, even after delimiter-path fixes.
    #[test]
    fn test_invalid_substitution_modifier_still_errors() {
        assert_has_error(r#"$x =~ s{foo}{bar}z;"#, "Invalid substitution modifier");
    }

    /// Diagnostic preservation: unterminated replacement still reports the
    /// missing-closing-delimiter error.
    #[test]
    fn test_missing_substitution_closing_delimiter_still_errors() {
        assert_has_error(r#"$x =~ s/foo/;"#, "Missing closing delimiter in substitution");
    }

    /// XSLoader.pm full content — must parse cleanly (corpus gate regression test).
    #[test]
    fn test_xsloader_pm_full() {
        let source = r#"use strict;
    no strict 'refs';

    package XSLoader;

    our $VERSION = "0.32";

    package DynaLoader;

    boot_DynaLoader('DynaLoader') if defined(&boot_DynaLoader) &&
                                    !defined(&dl_error);
    package XSLoader;

    sub load {
        package DynaLoader;

        my ($caller, $modlibname) = caller();
        my $module = $caller;

        if (@_) {
            $module = $_[0];
        } else {
            $_[0] = $module;
        }

        my $boots = "$module\::bootstrap";
        goto &$boots if defined &$boots;

        goto \&XSLoader::bootstrap_inherit unless $module and defined &dl_load_file;

        my @modparts = split(/::/,$module);
        my $modfname = $modparts[-1];
        my $modfname_orig = $modfname;

        my $modpname = join('/',@modparts);
        my $c = () = split(/::/,$caller,-1);
        $modlibname =~ s,[\\/][^\\/]+$,, while $c--;
        if ($modlibname !~ m{^/}) {
            FOUND: {
                for (@INC) {
                    if ($_ eq $modlibname) {
                        last FOUND;
                    }
                }
                goto \&XSLoader::bootstrap_inherit;
            }
        }
        my $file = "$modlibname/auto/$modpname/$modfname.so";

        my $bs = "$modlibname/auto/$modpname/$modfname_orig.bs";

        goto \&XSLoader::bootstrap_inherit if not -f $file or -s $bs;

        my $bootname = "boot_$module";
        $bootname =~ s/\W/_/g;
        @DynaLoader::dl_require_symbols = ($bootname);

        my $boot_symbol_ref;

        my $libref = dl_load_file($file, 0) or do {
            require Carp;
            Carp::croak("Can't load '$file' for module $module: " . dl_error());
        };
        push(@DynaLoader::dl_librefs,$libref);

        $boot_symbol_ref = dl_find_symbol($libref, $bootname) or do {
            require Carp;
            Carp::croak("Can't find '$bootname' symbol in $file\n");
        };

        push(@DynaLoader::dl_modules, $module);

      boot:
        my $xs = dl_install_xsub($boots, $boot_symbol_ref, $file);

        push(@DynaLoader::dl_shared_objects, $file);
        return &$xs(@_);
    }

    sub bootstrap_inherit {
        require DynaLoader;
        goto \&DynaLoader::bootstrap_inherit;
    }

    1;
    "#;
        assert_clean_parse(source);
    }

    // ── Regression guard for #2895 fix: -s 'filename' must remain a filetest ─────

    /// -s 'config.txt' — file-size filetest with a string literal argument.
    /// After the #2895 fix (which allows ' and " after whitespace for non-s operators),
    /// the `op != "s"` guard must keep -s 'filename' as a filetest, NOT a substitution.
    #[test]
    fn test_file_size_test_with_string_literal() {
        assert_clean_parse(r#"if (-s 'config.txt') { 1 }"#);
    }
}

mod word_and_comma_rhs_2856 {
    use super::cpan_test_helpers::*;

    // Regression tests for issue #2856: `and` operator doesn't consume
    // comma-separated RHS expressions.
    //
    // Pattern: `/test/ and $x = 1, last;`
    // Root cause: `parse_word_and_expr_with()` called `parse_word_not_expr()` which
    // went through `parse_assignment()` without comma handling. Fixed by applying
    // the same comma-collection logic that `parse_word_or_expr()` uses.

    // === Core regression: and + comma-separated RHS ===

    #[test]
    fn test_and_comma_rhs_basic() {
        // Basic pattern: regex and assignment, last
        assert_clean_parse("/test/ and $x = 1, last;");
    }

    #[test]
    fn test_and_comma_rhs_two_assignments() {
        // Two assignments on the RHS of `and`
        assert_clean_parse("/test/ and $x = 1, $y = 2;");
    }

    #[test]
    fn test_and_comma_rhs_print_close() {
        // Real-world pattern from IPC/Cmd.pm
        assert_clean_parse(r#"open(F, $f) and print F "data\n", close F;"#);
    }

    #[test]
    fn test_and_comma_rhs_last() {
        // Pattern: condition and assignment, control flow verb
        assert_clean_parse("$ok and $n = 1, last;");
    }

    #[test]
    fn test_and_comma_rhs_next() {
        assert_clean_parse("$ok and $n++, next;");
    }

    #[test]
    fn test_and_comma_rhs_three_elements() {
        // Three comma-separated elements on `and` RHS
        assert_clean_parse("$ok and $a = 1, $b = 2, $c = 3;");
    }

    // === Real-world patterns from affected CPAN files ===

    #[test]
    fn test_and_comma_file_copy_pattern() {
        // Pattern from File/Copy.pm
        assert_clean_parse(r#"$ok and $copied++, last;"#);
    }

    #[test]
    fn test_and_comma_text_balanced_pattern() {
        // Pattern from Text/Balanced.pm: track state and advance
        assert_clean_parse("$match and $pos = $next, $depth++, last;");
    }

    #[test]
    fn test_and_comma_ipc_cmd_pattern() {
        // Pattern from IPC/Cmd.pm: write data and track
        assert_clean_parse(r#"$ok and syswrite($fh, $data), $written += length($data);"#);
    }

    #[test]
    fn test_and_comma_unicode_ucd_pattern() {
        // Pattern from Unicode/UCD.pm: condition and list of updates
        assert_clean_parse("defined $val and $result = $val, $found++, last;");
    }

    // === and + comma with string on RHS ===

    #[test]
    fn test_and_comma_rhs_with_string() {
        assert_clean_parse(r#"/error/ and $msg = "found", last;"#);
    }

    // === Ensure `or` with commas still works (no regression) ===

    #[test]
    fn test_or_comma_rhs_still_works() {
        assert_clean_parse("/test/ or $x = 1, last;");
    }

    #[test]
    fn test_or_comma_rhs_two_elements() {
        assert_clean_parse("/test/ or $x = 1, $y = 2;");
    }

    // === and without comma still works (no regression) ===

    #[test]
    fn test_and_no_comma_still_works() {
        assert_clean_parse("$a and $b;");
    }

    #[test]
    fn test_and_assignment_no_comma() {
        assert_clean_parse("$a and $b = 1;");
    }

    #[test]
    fn test_and_die_no_comma() {
        assert_clean_parse(r#"$ok and die "failed";"#);
    }

    // === and chains with commas ===

    #[test]
    fn test_and_chain_with_comma_on_last() {
        // `and` chain where the rightmost has commas
        assert_clean_parse("$a and $b and $x = 1, last;");
    }

    // === not on RHS with comma ===

    #[test]
    fn test_and_not_rhs_no_comma() {
        // not applies only to its operand; comma is still collected
        assert_clean_parse("$a and not $b;");
    }

    #[test]
    fn test_and_not_rhs_with_comma() {
        // `$a and not $b, $c` — comma follows `not $b`; parser collects both elements
        // after `and` into a list. The `not` applies only to `$b` (same as `or` branch).
        assert_clean_parse("$a and not $b, $c;");
    }

    // === Precedence interaction: and+comma then or ===

    #[test]
    fn test_and_comma_then_or() {
        // `$a and $b, $c or $d` — or at outer level stops comma collection in and branch
        assert_clean_parse("$a and $b, $c or $d;");
    }

    // === Trailing comma before semicolon ===

    #[test]
    fn test_and_rhs_trailing_comma() {
        // `$ok and $n = 1,` — trailing comma before semicolon; silently consumed
        assert_clean_parse("$ok and $n = 1, ;");
    }

    // === and with comma inside parentheses ===

    #[test]
    fn test_and_comma_inside_parens() {
        // `($a and $b, $c)` — comma-collection inside paren context stops at `)`
        assert_clean_parse("my $x = ($a and $b, $c);");
    }

    // === and+comma inside if condition ===

    #[test]
    fn test_and_comma_in_if_block() {
        // Real-world: if block body containing and + comma
        assert_clean_parse("if ($ok) { $a and $b++, last; }");
    }

    // === Chained and with comma on each RHS ===

    #[test]
    fn test_and_chain_comma_on_each() {
        // Each `and` arm independently collects its comma list
        assert_clean_parse("$a and $b, $c and $d, $e;");
    }
}

mod word_op_after_return_control_2396 {
    use super::cpan_test_helpers::*;

    // Tests for issue #2396: word operators (or/and) after return, loop control,
    // indirect calls, and inside paren lists.

    // === return + word operator ===

    #[test]
    fn test_return_or_die() {
        // `return or die` means `(return) or (die)` — no return value
        assert_clean_parse("return or die;");
    }

    #[test]
    fn test_return_and_die() {
        assert_clean_parse("return and die;");
    }

    #[test]
    fn test_return_xor_die() {
        assert_clean_parse("return xor die;");
    }

    #[test]
    fn test_return_value_or_die() {
        // return with a value, then or die — value parsed, or at stmt level
        assert_clean_parse(r#"return $x or die "no value";"#);
    }

    #[test]
    fn test_return_or_in_sub() {
        assert_clean_parse("sub f { return or die; }");
    }

    // === loop control + word operator ===

    #[test]
    fn test_last_and_die() {
        // `last and die` — last terminates the loop, then die executes
        assert_clean_parse("last and die;");
    }

    #[test]
    fn test_next_or_die() {
        assert_clean_parse("next or die;");
    }

    #[test]
    fn test_last_or_warn() {
        assert_clean_parse(r#"last or warn "loop ended";"#);
    }

    #[test]
    fn test_next_and_log() {
        assert_clean_parse(r#"next and print "skipped\n";"#);
    }

    // === indirect call (print/say/warn) + word operator ===

    #[test]
    fn test_print_fh_or_die() {
        // print with filehandle followed by or die
        assert_clean_parse(r#"print $fh "data" or die "write failed";"#);
    }

    #[test]
    fn test_print_stderr_or_die() {
        assert_clean_parse(r#"print STDERR "error\n" or die "write failed";"#);
    }

    #[test]
    fn test_say_fh_or_die() {
        assert_clean_parse(r#"say $fh "data" or die "write failed";"#);
    }

    #[test]
    fn test_warn_or_die() {
        // warn is also indirect; it stops at word op
        assert_clean_parse(r#"warn "something" or die;"#);
    }

    // === word op inside paren list (hash value) ===

    #[test]
    fn test_hash_value_or_default() {
        // The `or` applies to the value in the pair
        assert_clean_parse(r#"my %h = (key => $val or "default");"#);
    }

    #[test]
    fn test_hash_multiple_pairs_with_or() {
        assert_clean_parse(r#"my %h = (a => $x or 1, b => $y or 2);"#);
    }

    // === CPAN patterns from the issue ===

    #[test]
    fn test_hash_element_or_return() {
        // $hash{key} or return — already works, regression guard
        assert_clean_parse(r#"$hash{key} or return;"#);
    }

    #[test]
    fn test_method_call_or_croak() {
        // $obj->method() or croak — already works, regression guard
        assert_clean_parse(r#"$obj->method() or croak "method failed";"#);
    }

    #[test]
    fn test_return_value_or_croak() {
        assert_clean_parse(r#"return $self->{attr} or croak "no attr";"#);
    }
}

mod word_op_and_after_open_2409 {
    use super::cpan_test_helpers::*;

    // Regression tests for issue #2409: `and` word operator after open(...) or
    // similar 3-argument list-operator calls with explicit parentheses.
    //
    // Pattern: `open(my $fh, '<', $file) and do { ... }`
    // Fixed as part of #2396 (word-op dispatch in parse_expression_statement).

    // === open() + and ===

    #[test]
    fn test_open_3arg_and_binmode() {
        // Exact pattern from the issue description
        assert_clean_parse("open(my $fh, '<', $f) and binmode($fh);");
    }

    #[test]
    fn test_open_3arg_and_do_block() {
        // Pattern from Catalyst.pm: open(...) and do { ... }
        assert_clean_parse(r#"open(my $fh, '<', $file) and do { print "ok\n" };"#);
    }

    #[test]
    fn test_open_2arg_and_die() {
        // 2-argument open followed by and die
        assert_clean_parse(r#"open(my $fh, $file) and die "unexpected";"#);
    }

    #[test]
    fn test_open_bareword_and_die() {
        // open with bareword filehandle
        assert_clean_parse(r#"open(FH, '<', $file) and print "opened";"#);
    }

    // === other list-ops + and ===

    #[test]
    fn test_close_fh_and_next() {
        assert_clean_parse("close($fh) and next;");
    }

    #[test]
    fn test_function_call_and_expr() {
        // Generic 3-arg function call followed by `and`
        assert_clean_parse("foo($a, $b, $c) and bar();");
    }

    #[test]
    fn test_function_call_and_die() {
        assert_clean_parse(r#"do_thing($x, $y) and die "unexpected success";"#);
    }

    // === and with complex RHS ===

    #[test]
    fn test_open_and_do_complex() {
        assert_clean_parse(r#"open(my $fh, '<', $file) and do { local $/; <$fh> };"#);
    }
}
