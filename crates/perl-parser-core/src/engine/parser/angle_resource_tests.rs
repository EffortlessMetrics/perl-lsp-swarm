use super::*;

#[test]
fn angle_resource_refusal_survives_nested_recoverers() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "my %h = (key => <abc>);",
        "my $h = { key => <abc> };",
        "if (<abc>) { my $after = 7; }",
        "if (1) { my $value = <abc>; }",
        "my $value = do { <abc>; };",
    ] {
        let control = Parser::new(source).parse_with_recovery();
        if crate::RecoverySalvageProfile::from_parse(
            &control.ast,
            &control.diagnostics,
            control.terminated_early(),
        )
        .class
            != crate::RecoverySalvageClass::Clean
        {
            return Err(format!(
                "non-clean reachable control: {source}: {:?}",
                control.diagnostics
            )
            .into());
        }
        let tokens = TokenStream::with_lexer_config(
            source,
            perl_lexer::LexerConfig {
                max_angle_scan_bytes: 2,
                max_angle_scan_steps: 8,
                ..perl_lexer::LexerConfig::default()
            },
        );
        let mut parser =
            Parser::assemble(tokens, source, ParserConfigIdentity::production_default(), None);
        let output = parser.parse_with_recovery();
        let refused = source.find('<').ok_or("fixture opener missing")? + 3;
        if output.stop_cause() != Some(ParseStopCause::LexerBudgetExhausted)
            || !output.diagnostics.iter().any(|error| {
                matches!(error, ParseError::AngleScan {
                error: perl_lexer::LexerError::AngleBudgetExhausted {
                    dimension: perl_lexer::AngleScanDimension::Bytes, limit: 2, usage: 2, position
                }
            } if *position == refused)
            })
        {
            return Err(format!(
                "lost nested resource cause for {source}: {:?} {:?}",
                output.stop_cause(),
                output.diagnostics
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn angle_diagnostics_remap_source_offsets_without_changing_work()
-> Result<(), Box<dyn std::error::Error>> {
    let malformed = offset_parse_error(
        ParseError::AngleScan {
            error: perl_lexer::LexerError::UnterminatedAngle { position: 3, recovery: 11 },
        },
        17,
    );
    if !matches!(
        malformed,
        ParseError::AngleScan {
            error: perl_lexer::LexerError::UnterminatedAngle { position: 20, recovery: 28 }
        }
    ) {
        return Err("malformed angle endpoints not both remapped".into());
    }
    let budget = offset_parse_error(
        ParseError::AngleScan {
            error: perl_lexer::LexerError::AngleBudgetExhausted {
                dimension: perl_lexer::AngleScanDimension::Bytes,
                limit: 5,
                usage: 5,
                position: 3,
            },
        },
        17,
    );
    if !matches!(
        budget,
        ParseError::AngleScan {
            error: perl_lexer::LexerError::AngleBudgetExhausted {
                dimension: perl_lexer::AngleScanDimension::Bytes,
                limit: 5,
                usage: 5,
                position: 20
            }
        }
    ) {
        return Err("resource offset/work remap changed identity".into());
    }
    let fallback = offset_parse_error(
        ParseError::AngleContextFallback {
            reason: crate::tokens::token_stream::ContextualFallbackReason::NoCheckpointAuthority,
            location: 7,
        },
        17,
    );
    if !matches!(
        fallback,
        ParseError::AngleContextFallback {
            reason: crate::tokens::token_stream::ContextualFallbackReason::NoCheckpointAuthority,
            location: 24
        }
    ) {
        return Err("context fallback offset not remapped".into());
    }
    if malformed.location() != Some(20)
        || budget.location() != Some(20)
        || fallback.location() != Some(24)
    {
        return Err("legacy diagnostic location lost angle source position".into());
    }
    Ok(())
}

#[test]
fn angle_resource_refusal_survives_production_parser() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $value = <abc>; my $after = 7;";
    let tokens = TokenStream::with_lexer_config(
        source,
        perl_lexer::LexerConfig {
            max_angle_scan_bytes: 2,
            max_angle_scan_steps: 8,
            ..perl_lexer::LexerConfig::default()
        },
    );
    let mut parser =
        Parser::assemble(tokens, source, ParserConfigIdentity::production_default(), None);
    let output = parser.parse_with_recovery();
    if output.stop_cause() != Some(ParseStopCause::LexerBudgetExhausted) {
        return Err(
            format!("angle exhaustion lost terminal cause: {:?}", output.stop_cause()).into()
        );
    }
    if !output.diagnostics.iter().any(|error| {
        matches!(
            error,
            ParseError::AngleScan {
                error: perl_lexer::LexerError::AngleBudgetExhausted {
                    dimension: perl_lexer::AngleScanDimension::Bytes,
                    limit: 2,
                    usage: 2,
                    position: 15
                }
            }
        )
    }) {
        return Err(format!("lost exact resource diagnostic: {:?}", output.diagnostics).into());
    }
    Ok(())
}

#[test]
fn contextual_angle_discards_cached_suffix_after_resource_refusal()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tokens = TokenStream::with_lexer_config(
        "<a b>; my $after=7;",
        perl_lexer::LexerConfig { max_angle_scan_bytes: 1, ..perl_lexer::LexerConfig::default() },
    );
    tokens.peek_third()?;
    if tokens.next()?.kind() != TokenKind::Less {
        return Err("fixture opener missing".into());
    }
    if !matches!(
        tokens.apply_contextual(ContextualTokenOp::ScanAngleBody),
        ContextualOpResult::AngleScanned(Err(perl_lexer::LexerError::AngleBudgetExhausted { .. }))
    ) {
        return Err("missing contextual resource refusal".into());
    }
    if tokens.next()?.kind() != TokenKind::Eof {
        return Err("cached suffix survived terminal resource refusal".into());
    }
    Ok(())
}

#[test]
fn buffered_angle_refuses_without_mutating_tokens() -> Result<(), Box<dyn std::error::Error>> {
    let source = "<a>;";
    let mut live = TokenStream::new(source);
    let mut original = Vec::new();
    loop {
        let token = live.next()?;
        let eof = token.kind() == TokenKind::Eof;
        original.push(token);
        if eof {
            break;
        }
    }
    for retained in [false, true] {
        let mut stream = if retained {
            TokenStream::from_vec_with_source(original.clone(), source)
        } else {
            TokenStream::from_vec(original.clone())
        };
        stream.next()?;
        let before = stream.peek()?.clone();
        let result = stream.apply_contextual(ContextualTokenOp::ScanAngleBody);
        let expected = if retained {
            crate::tokens::token_stream::ContextualFallbackReason::NoCheckpointAuthority
        } else {
            crate::tokens::token_stream::ContextualFallbackReason::NoBufferedSource
        };
        if result != (ContextualOpResult::FallbackRequired { reason: expected })
            || stream.next()? != before
        {
            return Err("buffered fallback mutated/pretended angle recognition".into());
        }
    }
    Ok(())
}

#[test]
fn buffered_parser_reports_rebuild_instead_of_malformed_perl()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::error::{ErrorCategory, ErrorClass};
    let source = "my $value = <a.pm>;";
    let mut stream = TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next()?;
        let eof = token.kind() == TokenKind::Eof;
        tokens.push(token);
        if eof {
            break;
        }
    }
    let output = Parser::from_tokens(tokens, source).parse_with_recovery();
    let reason = crate::tokens::token_stream::ContextualFallbackReason::NoCheckpointAuthority;
    if output.stop_cause() != Some(ParseStopCause::AngleContextFallback { reason }) {
        return Err(format!("lost rebuild stop cause: {:?}", output.stop_cause()).into());
    }
    if !output.diagnostics.iter().any(|error| matches!(error, ParseError::AngleContextFallback { reason: observed, location: 12 } if *observed == reason)
        && error.error_class() == ErrorCategory::Transient) {
        return Err(format!("lost typed transient rebuild diagnostic: {:?}", output.diagnostics).into());
    }
    let live = Parser::new(source).parse_with_recovery();
    if crate::RecoverySalvageProfile::from_parse(
        &live.ast,
        &live.diagnostics,
        live.terminated_early(),
    )
    .class
        != crate::RecoverySalvageClass::Clean
    {
        return Err("live rebuild did not parse cleanly".into());
    }
    let NodeKind::Program { statements } = &live.ast.kind else {
        return Err("missing live program".into());
    };
    let NodeKind::VariableDeclaration { initializer: Some(value), .. } =
        &statements.first().ok_or("missing live declaration")?.kind
    else {
        return Err("missing live initializer".into());
    };
    if !matches!(&value.kind, NodeKind::Glob { pattern } if pattern == "a.pm")
        || value.location.start != 12
        || value.location.end != 18
    {
        return Err("live rebuild lost exact glob".into());
    }
    Ok(())
}

#[test]
fn buffered_angle_fallback_propagates_through_nested_block()
-> Result<(), Box<dyn std::error::Error>> {
    // FC-ANGLE-FALLBACK-ORDINARY-BLOCK: the top-level control covers
    // parse_program; an angle inside an ordinary nested block must still
    // surface the typed rebuild request instead of a recovered Ok(block).
    use crate::error::{ErrorCategory, ErrorClass};
    let source = "if (1) { my $value = <a.pm>; }";
    let mut stream = TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next()?;
        let eof = token.kind() == TokenKind::Eof;
        tokens.push(token);
        if eof {
            break;
        }
    }
    let output = Parser::from_tokens(tokens, source).parse_with_recovery();
    let reason = crate::tokens::token_stream::ContextualFallbackReason::NoCheckpointAuthority;
    if output.stop_cause() != Some(ParseStopCause::AngleContextFallback { reason }) {
        return Err(format!("nested block swallowed fallback: {:?}", output.stop_cause()).into());
    }
    if !output.diagnostics.iter().any(|error| {
        matches!(
            error,
            ParseError::AngleContextFallback { reason: observed, .. } if *observed == reason
        ) && error.error_class() == ErrorCategory::Transient
    }) {
        return Err("nested block lost typed transient fallback diagnostic".into());
    }
    Ok(())
}

#[test]
#[test]
fn malformed_angle_error_node_respects_node_budget() -> Result<(), Box<dyn std::error::Error>> {
    let source = "<oops;";
    let mut budget = crate::ParseBudget::unlimited();
    budget.max_nodes_constructed = 0;
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    let output = Parser::with_production_config(source, config).parse_with_recovery();
    if output.stop_cause()
        != Some(ParseStopCause::CoreBudgetExhausted {
            dimension: crate::ParseCoreDimension::NodesConstructed,
            limit: 0,
            usage: 0,
        })
        || output.budget_usage.nodes_constructed != 0
    {
        return Err(
            format!("malformed angle bypassed node charge: {:?}", output.stop_cause()).into()
        );
    }
    if !output.diagnostics.iter().any(|error| {
        matches!(
            error,
            ParseError::AngleScan {
                error: perl_lexer::LexerError::UnterminatedAngle { position: 0, recovery: 5 }
            }
        )
    }) {
        return Err("node refusal did not reach malformed angle recovery".into());
    }
    Ok(())
}
