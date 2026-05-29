use crate::PerlLexer;

impl PerlLexer<'_> {
    #[inline]
    pub(crate) fn parse_quoted_heredoc_delimiter(
        &mut self,
        quote: char,
        text: &mut String,
    ) -> Option<String> {
        text.push(quote);
        self.advance();

        let mut delim = String::new();
        while self.position < self.input.len() {
            let Some(ch) = self.current_char() else {
                break;
            };

            if ch == quote {
                text.push(ch);
                self.advance();
                return Some(delim);
            }

            if ch == '\n' || ch == '\r' {
                return None;
            }

            delim.push(ch);
            text.push(ch);
            self.advance();
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quoted_heredoc_delimiter_collects_text_and_delimiter()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("'TAG'\nbody");
        let mut text = String::from("<<");

        let delimiter = lexer
            .parse_quoted_heredoc_delimiter('\'', &mut text)
            .ok_or("Expected quoted heredoc delimiter")?;

        assert_eq!(delimiter, "TAG");
        assert_eq!(text, "<<'TAG'");
        assert_eq!(lexer.current_char(), Some('\n'));
        Ok(())
    }

    #[test]
    fn parse_quoted_heredoc_delimiter_rejects_newline_before_closing_quote()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("'TAG\n");
        let mut text = String::from("<<");

        let delimiter = lexer.parse_quoted_heredoc_delimiter('\'', &mut text);

        assert_eq!(delimiter, None);
        assert_eq!(text, "<<'TAG");
        assert_eq!(lexer.current_char(), Some('\n'));
        Ok(())
    }

    #[test]
    fn parse_quoted_heredoc_delimiter_rejects_eof_before_closing_quote()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("\"TAG");
        let mut text = String::from("<<");

        let delimiter = lexer.parse_quoted_heredoc_delimiter('"', &mut text);

        assert_eq!(delimiter, None);
        assert_eq!(text, "<<\"TAG");
        assert_eq!(lexer.current_char(), None);
        Ok(())
    }
}
