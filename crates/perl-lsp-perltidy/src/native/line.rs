use super::block::{format_simple_control_block_tokens, format_simple_subroutine_tokens};
use super::statement::{
    format_simple_lexical_tokens, format_simple_module_tokens, format_simple_statement_tokens,
};
use super::{FormatConfig, TextRange};

pub(super) fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

pub(super) fn range_includes_line(range: TextRange, line: u32) -> bool {
    line >= range.start.line
        && (line < range.end.line || line == range.end.line && range.end.character > 0)
}

pub(super) fn format_simple_line(line: &str, config: &FormatConfig) -> Option<String> {
    format_simple_control_block_line(line, config)
        .or_else(|| format_simple_subroutine_line(line, config))
        .or_else(|| format_simple_module_line(line, config))
        .or_else(|| format_simple_statement_line(line, config))
        .or_else(|| format_simple_lexical_line(line, config))
}

fn split_code_line(line: &str) -> Option<(&str, &str, Option<&str>)> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (indent, body) = line.split_at(indent_len);
    if body.is_empty() {
        return None;
    }

    let (body, trailing_comment) = split_trailing_comment(body);
    Some((indent, body, trailing_comment))
}

fn tokenize_body(body: &str) -> Option<Vec<perl_parser_core::Token>> {
    let mut stream = perl_parser_core::TokenStream::new(body);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next().ok()?;
        if token.kind == perl_parser_core::TokenKind::Eof {
            break;
        }
        tokens.push(token);
    }
    Some(tokens)
}

pub(super) fn format_simple_module_line(line: &str, config: &FormatConfig) -> Option<String> {
    let (indent, body, trailing_comment) = split_code_line(line)?;
    let tokens = tokenize_body(body)?;

    let formatted = format_simple_module_tokens(&tokens, config)?;
    Some(format!("{indent}{}", append_trailing_comment(formatted, trailing_comment)))
}

pub(super) fn format_simple_lexical_line(line: &str, config: &FormatConfig) -> Option<String> {
    let (indent, body, trailing_comment) = split_code_line(line)?;
    let tokens = tokenize_body(body)?;

    let formatted = format_simple_lexical_tokens(&tokens, config)?;
    Some(format!("{indent}{}", append_trailing_comment(formatted, trailing_comment)))
}

pub(super) fn format_simple_subroutine_line(line: &str, config: &FormatConfig) -> Option<String> {
    let (indent, body, trailing_comment) = split_code_line(line)?;
    let tokens = tokenize_body(body)?;

    let formatted = format_simple_subroutine_tokens(&tokens, indent, config)?;
    Some(append_trailing_comment(formatted, trailing_comment))
}

pub(super) fn format_simple_control_block_line(
    line: &str,
    config: &FormatConfig,
) -> Option<String> {
    let (indent, body, trailing_comment) = split_code_line(line)?;
    let tokens = tokenize_body(body)?;

    let formatted = format_simple_control_block_tokens(&tokens, indent, config)?;
    Some(append_trailing_comment(formatted, trailing_comment))
}

pub(super) fn format_simple_statement_line(line: &str, config: &FormatConfig) -> Option<String> {
    let (indent, body, trailing_comment) = split_code_line(line)?;
    let tokens = tokenize_body(body)?;

    let formatted = format_simple_statement_tokens(&tokens, config)?;
    Some(format!("{indent}{}", append_trailing_comment(formatted, trailing_comment)))
}

pub(super) fn split_trailing_comment(body: &str) -> (&str, Option<&str>) {
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escaped = false;

    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' && (in_single || in_double || in_backtick) {
            escaped = true;
            continue;
        }

        match ch {
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            '#' if !in_single && !in_double && !in_backtick => {
                let code = body[..index].trim_end();
                if code.trim().is_empty() {
                    return (body, None);
                }
                return (code, Some(&body[index..]));
            }
            _ => {}
        }
    }

    (body, None)
}

pub(super) fn append_trailing_comment(
    mut formatted: String,
    trailing_comment: Option<&str>,
) -> String {
    if let Some(comment) = trailing_comment {
        formatted.push(' ');
        formatted.push_str(comment);
    }
    formatted
}
