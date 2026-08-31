use perl_lexer::{PerlLexer, TokenType};
use std::time::{Duration, Instant};

type TestResult = Result<(), String>;

#[test]
fn test_heredoc_depth_limit_preserves_error_payload_and_span() -> TestResult {
    // A real list assignment queues more than MAX_HEREDOC_DEPTH pending
    // heredocs before the statement closes. The 101st header is therefore
    // rejected by the production `try_heredoc` path.
    let mut code = String::from("my @h = (");
    for i in 0..110 {
        code.push_str(&format!("<<EOF{i}, "));
    }
    code.push_str(");");

    let mut lexer = PerlLexer::new(&code);
    let tokens = lexer.collect_tokens();

    let error = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::Error(ref message) if &**message == "Heredoc nesting too deep"))
        .ok_or_else(|| "production heredoc path should emit a depth error".to_string())?;
    let expected_start =
        code.find("<<EOF100").ok_or_else(|| "depth-limit header should be present".to_string())?;
    let expected_end = expected_start + "<<EOF100".len();

    assert_eq!(error.start, expected_start, "depth error must start at the rejected header");
    assert_eq!(error.end, expected_end, "depth error span must stop at the header boundary");
    assert_eq!(&*error.text, &code[expected_start..expected_end]);
    assert!(error.end < code.len(), "depth recovery must not consume the remaining statement");
    Ok(())
}

#[test]
fn test_heredoc_timeout() {
    // This is hard to test deterministically without mocking time,
    // but we can try with a very large input and see if it triggers.
    // Actually, we can just check if the code compiles and runs.

    let mut code = String::from("my $x = <<EOF;\n");
    for _ in 0..100000 {
        code.push_str("some content line\n");
    }
    // No EOF terminator

    let start = Instant::now();
    let mut lexer = PerlLexer::new(&code);
    let _tokens = lexer.collect_tokens();
    let duration = start.elapsed();

    assert!(duration < Duration::from_secs(10), "Lexer should not hang for more than 10 seconds");
}

#[test]
fn test_heredoc_large_crlf_body_respects_budget() {
    let mut code = String::from("my $x = <<EOF;\r\n");
    // Bigger than MAX_HEREDOC_BYTES in lexer implementation.
    for _ in 0..40_000 {
        code.push_str("0123456789\r\n");
    }

    let start = Instant::now();
    let mut lexer = PerlLexer::new(&code);
    let tokens = lexer.collect_tokens();
    let duration = start.elapsed();

    assert!(duration < Duration::from_secs(10), "Lexer should remain bounded");
    assert!(
        tokens.iter().any(|t| matches!(t.token_type, TokenType::UnknownRest)),
        "expected UnknownRest when heredoc body exceeds budget"
    );
}
