use crate::PerlLexer;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

impl PerlLexer<'_> {
    /// Normalize file start by skipping BOM if present
    pub(crate) fn normalize_file_start(&mut self) {
        if self.position == 0 && self.matches_bytes(UTF8_BOM) {
            self.position = UTF8_BOM.len();
            self.line_start_offset = UTF8_BOM.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_file_start_skips_bom_at_start_only() -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("\u{feff}my $x");
        lexer.normalize_file_start();

        assert_eq!(lexer.position, 3);
        assert_eq!(lexer.line_start_offset, 3);
        assert_eq!(lexer.current_char(), Some('m'));

        let mut not_at_start = PerlLexer::new("x\u{feff}y");
        not_at_start.position = 1;
        not_at_start.normalize_file_start();
        assert_eq!(not_at_start.position, 1);
        Ok(())
    }

    #[test]
    fn normalize_file_start_leaves_plain_input_unchanged() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut lexer = PerlLexer::new("my $x");
        lexer.normalize_file_start();

        assert_eq!(lexer.position, 0);
        assert_eq!(lexer.line_start_offset, 0);
        Ok(())
    }
}
