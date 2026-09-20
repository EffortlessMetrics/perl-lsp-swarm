//! Public token and native AST proof for #13930. Perl 5.44 is the external oracle.
use perl_parser_core::{
    Node, NodeKind, ParseBudget, ParseCoreDimension, ParseError, ParseStopCause, Parser,
    ParserConfigIdentity, RecoverySalvageClass, RecoverySalvageProfile, TokenKind, TokenStream,
};

type R = Result<(), Box<dyn std::error::Error>>;

fn check(condition: bool, message: &str) -> R {
    if condition { Ok(()) } else { Err(message.into()) }
}
fn nodes<'a>(node: &'a Node, matches: &impl Fn(&NodeKind) -> bool, found: &mut Vec<&'a Node>) {
    if matches(&node.kind) {
        found.push(node);
    }
    for child in node.children() {
        nodes(child, matches, found);
    }
}
fn repetitions(node: &Node) -> Vec<&Node> {
    let mut found = Vec::new();
    nodes(node, &|kind| matches!(kind, NodeKind::Binary { op, .. } if op == "x"), &mut found);
    found
}
fn slice<'a>(source: &'a str, node: &Node) -> Result<&'a str, Box<dyn std::error::Error>> {
    source.get(node.location.start..node.location.end).ok_or_else(|| "invalid node geometry".into())
}
fn clean(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let output = Parser::new(source).parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        != RecoverySalvageClass::Clean
    {
        return Err(format!("non-clean {source}: {:?}", output.diagnostics).into());
    }
    Ok(output.ast)
}
fn after(ast: &Node, source: &str) -> R {
    let mut found = Vec::new();
    nodes(
        ast,
        &|kind| matches!(kind, NodeKind::VariableDeclaration { variable, .. } if matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")),
        &mut found,
    );
    check(found.len() == 1, "lost/duplicated following declaration")?;
    let declaration = found.first().ok_or("missing after")?;
    let NodeKind::VariableDeclaration { variable, initializer: Some(value), .. } =
        &declaration.kind
    else {
        return Err("lost after initializer".into());
    };
    check(
        slice(source, variable)? == "$after"
            && matches!(&value.kind, NodeKind::Number { value } if value == "7"),
        "changed after declaration",
    )
}
fn closed(term: &str, family: &str) -> R {
    for expression in [format!("\"x\" x {term}"), format!("f(\"x\" x {term}, 7)")] {
        let source = format!("# λ\nmy $value = {expression}; my $after = 7;");
        let ast = clean(&source)?;
        let found = repetitions(&ast);
        check(found.len() == 1, "expected exactly one binary x")?;
        let binary = found.first().ok_or("missing repetition")?;
        let NodeKind::Binary { left, right, .. } = &binary.kind else {
            return Err("wrong repetition kind".into());
        };
        check(slice(&source, binary)? == format!("\"x\" x {term}"), "wrong binary span")?;
        check(
            slice(&source, left)? == "\"x\"" && matches!(&left.kind, NodeKind::String { .. }),
            "wrong lhs",
        )?;
        check(slice(&source, right)? == term, "angle RHS truncated or consumed suffix")?;
        check(
            match (&right.kind, family) {
                (NodeKind::Diamond, "diamond") => true,
                (NodeKind::Readline { filehandle: Some(handle) }, "readline") => handle == "STDIN",
                (NodeKind::Readline { filehandle: Some(handle) }, "scalar") => handle == "$fh",
                (NodeKind::Glob { pattern }, "glob") => pattern == "*.pm",
                _ => false,
            },
            "wrong angle RHS topology",
        )?;
        if expression.starts_with("f(") {
            let mut calls = Vec::new();
            nodes(
                &ast,
                &|kind| matches!(kind, NodeKind::FunctionCall { name, .. } if name == "f"),
                &mut calls,
            );
            let NodeKind::FunctionCall { args, .. } = &calls.first().ok_or("lost call")?.kind
            else {
                return Err("wrong call".into());
            };
            check(
                args.len() == 2
                    && matches!(args.get(1).map(|n| &n.kind), Some(NodeKind::Number { value }) if value == "7"),
                "angle swallowed call/comma continuation",
            )?;
        }
        after(&ast, &source)?;
    }
    Ok(())
}
#[test]
fn diamond_rhs_is_complete_repetition() -> R {
    closed("<>", "diamond")
}
#[test]
fn readline_rhs_is_complete_repetition() -> R {
    closed("<STDIN>", "readline")
}
#[test]
fn glob_rhs_is_complete_repetition() -> R {
    closed("<*.pm>", "glob")
}
#[test]
fn scalar_handle_rhs_is_complete_repetition() -> R {
    closed("<$fh>", "scalar")
}

#[test]
fn comparison_and_shift_remain_outside_repetition() -> R {
    for (expression, outer, repetition_left) in
        [("$a x $b < $c", "<", true), ("$a < $b x $c", "<", false), ("$a x $b << $c", "<<", true)]
    {
        let source = format!("my $value = {expression};");
        let ast = clean(&source)?;
        let mut found = Vec::new();
        nodes(&ast, &|kind| matches!(kind, NodeKind::Binary { op, .. } if op == outer), &mut found);
        check(found.len() == 1 && repetitions(&ast).len() == 1, "operator count drift")?;
        let node = found.first().ok_or("lost comparison/shift")?;
        let NodeKind::Binary { left, right, .. } = &node.kind else {
            return Err("wrong outer kind".into());
        };
        let nested = if repetition_left { left } else { right };
        check(
            matches!(&nested.kind, NodeKind::Binary { op, .. } if op == "x")
                && slice(&source, node)? == expression,
            "wrong precedence or outer geometry",
        )?;
        check(
            slice(&source, nested)? == if repetition_left { "$a x $b" } else { "$b x $c" },
            "wrong repetition operand boundary",
        )?;
    }
    Ok(())
}

#[test]
fn missing_closer_preserves_declaration_and_remains_nonclean() -> R {
    for separator in ["; ", ";\n"] {
        let source = format!("my $value = \"x\" x <STDIN{separator}my $after = 7;");
        let output = Parser::new(&source).parse_with_recovery();
        check(output.stop_cause().is_none(), "malformed angle terminated parser")?;
        check(
            RecoverySalvageProfile::from_parse(&output.ast, &output.diagnostics, false).class
                != RecoverySalvageClass::Clean,
            "malformed angle silently clean",
        )?;
        after(&output.ast, &source)?;
    }
    Ok(())
}

#[test]
fn angle_repetition_respects_token_budget() -> R {
    // String, contextual x, and the real '<' are the three consumed tokens.
    // The lexer-owned malformed scan then records its exact boundary before
    // the parser refuses the still-unconsumed semicolon. This cannot pass by
    // stopping before the angle route, unlike the former declaration limit 2.
    let source = "\"x\" x <STDIN; my $after = 7;";
    let mut budget = ParseBudget::unlimited();
    budget.max_tokens_consumed = 3;
    let output = Parser::with_production_config(
        source,
        ParserConfigIdentity::production_default().with_budget(budget),
    )
    .parse_with_recovery();
    check(
        output.stop_cause()
            == Some(ParseStopCause::CoreBudgetExhausted {
                dimension: ParseCoreDimension::TokensConsumed,
                limit: 3,
                usage: 3,
            })
            && output.budget_usage.tokens_consumed == 3,
        "wrong typed token refusal",
    )?;
    check(
        output.diagnostics.iter().any(|error| {
            matches!(
                error,
                ParseError::AngleScan {
                    error: perl_lexer::LexerError::UnterminatedAngle { position: 6, recovery: 12 }
                }
            )
        }),
        "budget control never reached malformed angle scan",
    )?;
    let mut suffix = Vec::new();
    nodes(
        &output.ast,
        &|kind| matches!(kind, NodeKind::Variable { name, .. } if name == "after"),
        &mut suffix,
    );
    check(suffix.is_empty(), "terminal budget consumed following declaration")
}

#[test]
fn non_repetition_x_contexts_and_existing_recovery_remain_distinct() -> R {
    for source in ["my %h = (x => 1);", "$obj->x(1);", "Pkg::x(1);", "my $v = $h{x};", "x(1);"] {
        let ast = clean(source)?;
        check(repetitions(&ast).is_empty(), "nonoperator x became repetition")?;
    }
    for source in ["\"x\" x; my $after = 7;", "$value x = 3; my $after = 7;"] {
        let output = Parser::new(source).parse_with_recovery();
        check(!output.diagnostics.is_empty(), "existing invalid repetition silently clean")?;
        after(&output.ast, source)?;
    }
    Ok(())
}

#[test]
fn public_lexer_and_parser_token_observations() -> R {
    for source in [
        "\"x\" x <>;",
        "\"x\" x <STDIN>;",
        "\"x\" x <*.pm>;",
        "\"x\" x <$fh>;",
        "$a x $b < $c;",
        "$a < $b x $c;",
        "$a x $b << $c;",
        "\"x\" x <STDIN; my $after = 7;",
        "\"x\" x <<EOF;\n3\nEOF\n",
    ] {
        let angle_boundary = source.starts_with("\"x\" x <");
        let mut raw_boundary_seen = false;
        let mut parser_boundary_seen = false;
        let mut shift_seen = false;
        let mut lexer = perl_lexer::PerlLexer::new(source);
        let mut raw_count = 0;
        while let Some(token) = lexer.next_token() {
            raw_count += 1;
            check(raw_count <= source.len() + 1, "raw lexer failed to progress")?;

            if angle_boundary && token.start == 6 {
                raw_boundary_seen = true;
                if source.starts_with("\"x\" x <<") {
                    check(
                        token.text.as_ref() == "<<EOF" && token.end == 11,
                        "heredoc raw boundary changed",
                    )?;
                } else {
                    check(
                        token.text.as_ref() == "<" && token.end == 7,
                        "angle raw boundary changed",
                    )?;
                    check(
                        matches!(&token.token_type, perl_lexer::TokenType::Operator(op) if op.as_ref() == "<"),
                        "angle raw kind changed",
                    )?;
                }
            }
        }
        let mut stream = TokenStream::new(source);
        for index in 0..=source.len() + 1 {
            let token = stream.next()?;
            if source == "$a x $b << $c;" && token.start() == 8 {
                check(
                    token.kind() == TokenKind::LeftShift && token.end() == 10,
                    "shift identity changed",
                )?;
                shift_seen = true;
            }
            if angle_boundary && token.start() == 6 {
                parser_boundary_seen = true;
                let (kind, end) = if source.starts_with("\"x\" x <<") {
                    (TokenKind::HeredocStart, 11)
                } else {
                    (TokenKind::Less, 7)
                };
                check(
                    token.kind() == kind && token.end() == end,
                    "angle/heredoc token identity changed",
                )?;
            }
            if token.kind() == TokenKind::Eof {
                break;
            }
            check(index <= source.len(), "parser token stream failed to progress")?;
        }
        check(raw_count != 0, "vacuous raw lexer observation")?;
        check(
            !angle_boundary || (raw_boundary_seen && parser_boundary_seen),
            "missing required angle/heredoc boundary witness",
        )?;
        check(source != "$a x $b << $c;" || shift_seen, "missing shift boundary witness")?;
    }
    Ok(())
}
