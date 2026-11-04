use perl_lexer::{
    AngleScanDimension, Checkpointable, LexerConfig, LexerError, PerlLexer, TokenType,
};

type R = Result<(), Box<dyn std::error::Error>>;
fn opener(lexer: &mut PerlLexer<'_>) -> R {
    let token = lexer.next_token().ok_or("missing opener")?;
    if token.text.as_ref() != "<" {
        return Err(format!("wrong opener: {token:?}").into());
    }
    Ok(())
}
fn config(bytes: usize, steps: usize) -> LexerConfig {
    LexerConfig {
        max_angle_scan_bytes: bytes,
        max_angle_scan_steps: steps,
        ..LexerConfig::default()
    }
}

#[test]
fn exact_source_and_scalar_work() -> R {
    let source = "<λ\\> b>;";
    let mut lexer = PerlLexer::new(source);
    opener(&mut lexer)?;
    let span = lexer.scan_angle_body()?;
    if source.get(span.body_start..span.body_end) != Some("λ\\> b")
        || span.end != 8
        || lexer.angle_scan_usage() != (7, 6)
    {
        return Err(format!("wrong span/work: {span:?} {:?}", lexer.angle_scan_usage()).into());
    }
    if lexer.next_token().ok_or("missing suffix")?.text.as_ref() != ";" {
        return Err("lost suffix".into());
    }
    Ok(())
}

#[test]
fn byte_and_step_limits_are_independent() -> R {
    for (bytes, steps, failure) in [
        (1, 9, Some(AngleScanDimension::Bytes)),
        (2, 9, Some(AngleScanDimension::Bytes)),
        (3, 1, Some(AngleScanDimension::Steps)),
        (3, 2, None),
        (4, 3, None),
    ] {
        let mut lexer = PerlLexer::with_config("<λ>;", config(bytes, steps));
        opener(&mut lexer)?;
        match (lexer.scan_angle_body(), failure) {
            (Ok(span), None) if span.end == 4 && lexer.angle_scan_usage() == (3, 2) => {}
            (
                Err(LexerError::AngleBudgetExhausted { dimension, limit, usage, position }),
                Some(expected),
            ) if dimension == expected => {
                let expected_limit =
                    if dimension == AngleScanDimension::Bytes { bytes } else { steps };
                let expected_position = if bytes == 1 { 1 } else { 3 };
                if limit != expected_limit || usage != limit || position != expected_position {
                    return Err("wrong refusal quantities".into());
                }
                if lexer.next_token().ok_or("missing terminal EOF")?.token_type != TokenType::EOF {
                    return Err("resource failure leaked suffix".into());
                }
            }
            other => return Err(format!("wrong resource outcome: {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn closed_punctuation_wins_and_missing_closure_preserves_boundary() -> R {
    for body in ["a;b", "a,b", "a)b", "a b", "a#b", "a\rb", r"a\>b"] {
        let source = format!("<{body}>;");
        let mut lexer = PerlLexer::new(&source);
        opener(&mut lexer)?;
        let span = lexer.scan_angle_body()?;
        if source.get(span.body_start..span.body_end) != Some(body) {
            return Err("body changed".into());
        }
    }
    for source in ["<a;my $after=7;", "<a,my $after=7;\n", "<a)my $after=7;\r\n"] {
        let mut lexer = PerlLexer::new(source);
        opener(&mut lexer)?;
        if !matches!(
            lexer.scan_angle_body(),
            Err(LexerError::UnterminatedAngle { position: 0, recovery: 2 })
        ) {
            return Err("wrong malformed range".into());
        }
        let token = lexer.next_token().ok_or("lost recovery delimiter")?;
        if token.start != 2 || token.end != 3 {
            return Err("recovery delimiter consumed".into());
        }
    }
    Ok(())
}

#[test]
fn restore_inherits_spent_work_and_never_refunds() -> R {
    let source = "<a>;<b>;";
    let mut lexer = PerlLexer::with_config(source, config(5, 5));
    opener(&mut lexer)?;
    let before = lexer.checkpoint();
    lexer.scan_angle_body()?;
    let spent = lexer.checkpoint();
    let mut fresh = PerlLexer::with_config(source, config(5, 5));
    fresh.restore(&spent)?;
    if fresh.angle_scan_usage() != (2, 2) {
        return Err("fresh restore refunded budget".into());
    }
    lexer.restore(&before)?;
    lexer.scan_angle_body()?;
    if lexer.angle_scan_usage() != (4, 4) {
        return Err("same-instance restore refunded budget".into());
    }
    lexer.restore(&before)?;
    if !matches!(lexer.scan_angle_body(), Err(LexerError::AngleBudgetExhausted { usage: 5, .. })) {
        return Err("repeated scan escaped cumulative limit".into());
    }
    lexer.reset();
    opener(&mut lexer)?;
    if !matches!(lexer.scan_angle_body(), Err(LexerError::AngleBudgetExhausted { usage: 5, .. })) {
        return Err("reset refunded same-source operation work".into());
    }
    Ok(())
}

#[test]
fn each_policy_dimension_and_old_schema_refuse_without_mutation() -> R {
    let source = "<a>;";
    let mut producer = PerlLexer::with_config(source, config(4, 4));
    opener(&mut producer)?;
    let checkpoint = producer.checkpoint();
    for policy in [config(3, 4), config(4, 3)] {
        let mut target = PerlLexer::with_config(source, policy);
        let before = target.checkpoint();
        if target.restore(&checkpoint).is_ok() || target.checkpoint() != before {
            return Err("policy mismatch accepted or mutated state".into());
        }
    }
    let mut old = checkpoint;
    old.__test_stamp_schema(1);
    let before = producer.checkpoint();
    if producer.restore(&old).is_ok() || producer.checkpoint() != before {
        return Err("old schema accepted or mutated state".into());
    }
    Ok(())
}

#[test]
fn repeated_unterminated_attempts_share_one_work_envelope() -> R {
    let source = "<a;<b;<c;<d;";
    let mut lexer = PerlLexer::with_config(source, config(17, 17));
    let mut attempts = 0;
    loop {
        let token = lexer.next_token().ok_or("missing terminal token")?;
        if token.token_type == TokenType::EOF {
            return Err("missing resource refusal".into());
        }
        if token.text.as_ref() != "<" {
            continue;
        }
        attempts += 1;
        match lexer.scan_angle_body() {
            Err(LexerError::UnterminatedAngle { .. }) => {}
            Err(LexerError::AngleBudgetExhausted { usage: 17, .. }) => break,
            other => return Err(format!("wrong repeated-malformed outcome: {other:?}").into()),
        }
    }
    if attempts != 2 || lexer.angle_scan_usage() != (17, 17) {
        return Err("cumulative scans reset or stopped at wrong work".into());
    }
    Ok(())
}

#[test]
fn lf_is_a_boundary_even_after_escape_but_bare_cr_is_content() -> R {
    for source in ["<a\nb>", "<a\r\nb>", "<a\\\nb>"] {
        let mut lexer = PerlLexer::new(source);
        opener(&mut lexer)?;
        let boundary = source.find('\n').ok_or("fixture LF missing")?;
        if !matches!(lexer.scan_angle_body(), Err(LexerError::UnterminatedAngle { position: 0, recovery }) if recovery == boundary)
        {
            return Err(format!("LF boundary ignored: {source:?}").into());
        }
    }
    let mut lexer = PerlLexer::new("<a\rb>");
    opener(&mut lexer)?;
    if lexer.scan_angle_body()?.end != 5 {
        return Err("bare CR treated as LF".into());
    }
    Ok(())
}
