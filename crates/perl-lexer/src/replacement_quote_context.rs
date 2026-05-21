pub(super) fn can_start_replacement_expression_quote(input: &str, pos: usize) -> bool {
    input
        .get(..pos)
        .and_then(|text| text.chars().rev().find(|ch| !ch.is_whitespace()))
        .is_some_and(is_expression_quote_predecessor)
}

pub(super) fn is_word_apostrophe(input: &str, pos: usize, quote: char) -> bool {
    quote == '\''
        && input
            .get(..pos)
            .and_then(|text| text.chars().next_back())
            .is_some_and(is_word_character)
}

fn is_expression_quote_predecessor(ch: char) -> bool {
    matches!(
        ch,
        '(' | '['
            | '{'
            | ','
            | '='
            | ':'
            | '?'
            | '!'
            | '~'
            | '+'
            | '-'
            | '*'
            | '%'
            | '&'
            | '|'
            | '^'
            | '<'
            | '>'
    )
}

fn is_word_character(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}
