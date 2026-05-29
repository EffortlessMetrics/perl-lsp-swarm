use crate::keywords::is_lexer_keyword;

#[inline(always)]
pub(crate) fn is_keyword_fast(word: &str) -> bool {
    // Fast length-based rejection for most cases.
    // Lexer keywords are currently bounded to 1..=9 characters.
    matches!(word.len(), 1..=9) && is_lexer_keyword(word)
}

#[inline]
pub(crate) fn is_builtin_function(word: &str) -> bool {
    BARE_TERM_BUILTINS.binary_search(&word).is_ok()
}

#[inline(always)]
pub(crate) fn is_quote_op_word_prefix(word: &[u8]) -> bool {
    matches!(word, b"m" | b"q" | b"qq" | b"qw" | b"qx" | b"qr")
}

const BARE_TERM_BUILTINS: &[&str] = &[
    "abs", "chomp", "chop", "chr", "close", "defined", "delete", "each", "exists", "hex", "int",
    "join", "keys", "lc", "lcfirst", "length", "oct", "open", "ord", "pack", "print", "push",
    "read", "ref", "reverse", "rindex", "say", "scalar", "splice", "sprintf", "sqrt", "substr",
    "tie", "uc", "ucfirst", "unpack", "unshift", "untie", "values", "write",
];

#[cfg(test)]
mod tests {
    use super::{is_builtin_function, is_keyword_fast, is_quote_op_word_prefix};

    #[test]
    fn keyword_fast_accepts_lexer_keywords_and_rejects_out_of_range_words()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(is_keyword_fast("my"));
        assert!(is_keyword_fast("continue"));
        assert!(!is_keyword_fast("not_a_keyword"));
        assert!(!is_keyword_fast("abcdefghijklmnop"));
        Ok(())
    }

    #[test]
    fn builtin_lookup_covers_bare_term_builtins() -> Result<(), Box<dyn std::error::Error>> {
        for word in ["print", "say", "defined", "substr", "ucfirst"] {
            assert!(is_builtin_function(word), "{word}");
        }
        for word in ["my", "sub", "unknown_builtin"] {
            assert!(!is_builtin_function(word), "{word}");
        }
        Ok(())
    }

    #[test]
    fn quote_operator_prefix_lookup_is_exact() -> Result<(), Box<dyn std::error::Error>> {
        for word in [b"m".as_slice(), b"q", b"qq", b"qw", b"qx", b"qr"] {
            assert!(is_quote_op_word_prefix(word));
        }
        for word in [b"s".as_slice(), b"tr", b"y", b"qqx", b""] {
            assert!(!is_quote_op_word_prefix(word));
        }
        Ok(())
    }
}
