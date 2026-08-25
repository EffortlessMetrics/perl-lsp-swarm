impl<'a> Parser<'a> {
    /// Parse an expression
    fn parse_expression(&mut self) -> ParseResult<Node> {
        self.with_recursion_guard(|s| s.parse_comma())
    }

    /// Validate parser-owned regex bodies while preserving Perl syntax that is
    /// risky but valid. Canonical retained-analysis entry points record the
    /// whole operator here and defer the one body analysis until the completed
    /// AST supplies lexical language-profile state.
    fn analyze_regex_body_for_ast(&mut self, pattern: &str, start: usize) -> ParseResult<bool> {
        // The session check comes first deliberately. `from_utf8` validates the whole
        // source, so testing it before the session would charge every ordinary parse an
        // O(source) scan per regex body for a hook that is about to decline anyway.
        if crate::engine::regex_retention::has_active_session()
            && let Ok(source) = std::str::from_utf8(self.src_bytes)
            && crate::engine::regex_retention::record_operator_geometry(source, start)
        {
            return Ok(false);
        }

        let validator = crate::engine::regex_validator::RegexValidator::new();
        let source_end = self
            .tokens
            .peek()
            .map(|token| token.start)
            .unwrap_or(self.src_bytes.len())
            .min(self.src_bytes.len());
        let geometry = self
            .src_bytes
            .get(start..source_end)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|source| quote_parser::extract_regex_family_geometry(source, start));

        let Some(geometry) = geometry else {
            // Without exact source geometry, retain the AST compatibility fact
            // but do not publish a position-bearing diagnostic from a guessed
            // token start.
            return Ok(validator.detects_code_execution(pattern));
        };
        let pattern = geometry.pattern.text.as_str();
        let pattern_start = geometry.pattern.range.start;
        let has_embedded_code = validator.find_code_execution(pattern, pattern_start).is_some();
        let nested_quantifier = validator.find_nested_quantifier(pattern, pattern_start);

        if !has_embedded_code && nested_quantifier.is_none() {
            validator.validate(pattern, pattern_start).map_err(|error| match error {
                crate::engine::regex_validator::RegexError::Syntax { message, offset } => {
                    ParseError::syntax(message, offset)
                }
            })?;
        }

        if let Some(finding) = nested_quantifier {
            self.record_error(ParseError::nested_quantifier_advisory(finding.offset));
        }

        Ok(has_embedded_code)
    }
}
