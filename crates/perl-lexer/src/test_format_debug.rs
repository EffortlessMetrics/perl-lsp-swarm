#[cfg(test)]
mod tests {
    use crate::{PerlLexer, TokenType};
    use std::sync::Arc;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn format_body_shares_payload_allocation() -> TestResult {
        for body in ["".to_owned(), "literal body\n".to_owned(), "é @<<<\n".repeat(8192)] {
            let input = format!("{body}.\n");
            let mut lexer = PerlLexer::new(&input);
            lexer.enter_format_mode();
            let token = lexer.next_token().ok_or("expected format body token")?;
            let TokenType::FormatBody(content) = &token.token_type else {
                return Err(format!("expected FormatBody, got {:?}", token.token_type).into());
            };
            assert_eq!(content.as_ref(), body);
            assert_eq!(token.text.as_ref(), body);
            assert_eq!((token.start, token.end), (0, input.len()));
            assert!(
                Arc::ptr_eq(content, &token.text),
                "FormatBody and Token.text must share one payload allocation ({} bytes)",
                body.len()
            );
        }
        Ok(())
    }

    #[test]
    fn format_body_preserves_dot_lines_and_following_code() -> TestResult {
        // Expected payloads are literal source, not output from a second lexer.
        for (input, expected, end) in [
            (".\n/ok/;", "", 2),
            ("é\n.\n/ok/;", "é\n", 5),
            ("é\r\n. \t\r\n/ok/;", "é\r\n", 9),
            (".text\nx.y\n .\n.\n/ok/;", ".text\nx.y\n .\n", 15),
        ] {
            let mut lexer = PerlLexer::new(input);
            lexer.enter_format_mode();
            let token = lexer.next_token().ok_or("expected format body token")?;
            let TokenType::FormatBody(content) = &token.token_type else {
                return Err(format!("expected FormatBody, got {:?}", token.token_type).into());
            };
            assert_eq!(content.as_ref(), expected);
            assert_eq!(token.text.as_ref(), expected);
            assert_eq!((token.start, token.end), (0, end));

            // A slash must resume as a regex term, not division or more format text.
            let following = lexer.next_token().ok_or("expected following regex token")?;
            assert_eq!(following.token_type, TokenType::RegexMatch);
            assert_eq!(following.text.as_ref(), "/ok/");
            assert_eq!((following.start, following.end), (end, end + 4));
            let semicolon = lexer.next_token().ok_or("expected following semicolon")?;
            assert_eq!(semicolon.text.as_ref(), ";");
            assert_eq!((semicolon.start, semicolon.end), (end + 4, end + 5));
            assert_eq!(lexer.next_token().map(|t| t.token_type), Some(TokenType::EOF));
            assert!(lexer.next_token().is_none());
        }
        Ok(())
    }

    #[test]
    fn format_body_accepts_dot_at_eof_with_byte_offsets() -> TestResult {
        for (suffix, expected, end) in
            [(".", "", 6), (". \t", "", 8), ("雪\n.", "雪\n", 10), ("雪\r\n. \t", "雪\r\n", 13)]
        {
            let input = format!("\"é\";{suffix}");
            let mut lexer = PerlLexer::new(&input);
            assert_eq!(lexer.next_token().ok_or("expected prefix string")?.text.as_ref(), "\"é\"");
            assert_eq!(lexer.next_token().ok_or("expected prefix semicolon")?.end, 5);
            lexer.enter_format_mode();
            let token = lexer.next_token().ok_or("expected format body token")?;
            let TokenType::FormatBody(content) = &token.token_type else {
                return Err(format!("expected FormatBody, got {:?}", token.token_type).into());
            };
            assert_eq!(content.as_ref(), expected);
            assert_eq!(token.text.as_ref(), expected);
            assert_eq!((token.start, token.end), (5, end));
            assert!(Arc::ptr_eq(content, &token.text));
            assert_eq!(lexer.next_token().map(|t| t.token_type), Some(TokenType::EOF));
            assert!(lexer.next_token().is_none());
        }
        Ok(())
    }

    #[test]
    fn format_body_missing_dot_preserves_error_payload_and_span() -> TestResult {
        for (suffix, end) in [("", 5), ("雪\n.text\nx.y", 18), ("body\r\n", 11)] {
            let input = format!("\"é\";{suffix}");
            let mut lexer = PerlLexer::new(&input);
            lexer.next_token().ok_or("expected prefix string")?;
            lexer.next_token().ok_or("expected prefix semicolon")?;
            lexer.enter_format_mode();
            let token = lexer.next_token().ok_or("expected unterminated format error")?;
            assert_eq!(token.token_type, TokenType::Error(Arc::from("Unterminated format body")));
            assert_eq!(token.text.as_ref(), suffix);
            assert_eq!((token.start, token.end), (5, end));
            assert_eq!(lexer.next_token().map(|t| t.token_type), Some(TokenType::EOF));
            assert!(lexer.next_token().is_none());
        }
        Ok(())
    }

    /// Test that format body parsing doesn't hang on unterminated input.
    ///
    /// This test verifies that the lexer returns an error token when given
    /// format content without a proper terminator (single `.` on a line).
    /// The test will hang if there's an infinite loop, which will be caught
    /// by the test framework's timeout or CI job timeout.
    #[test]
    fn test_format_body_infinite_loop() {
        let input = "format body without terminator\nthis goes on forever\n";
        let mut lexer = PerlLexer::new(input);

        // Manually enter format mode to test
        lexer.enter_format_mode();

        // This should complete quickly and return an error token
        // (not hang in an infinite loop)
        let token = lexer.next_token();

        // Should get an error token for unterminated format
        assert!(matches!(token, Some(t) if matches!(t.token_type, TokenType::Error(_))));
    }
}
