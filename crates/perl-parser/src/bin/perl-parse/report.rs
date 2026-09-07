use std::io::{self, Write};

use perl_parser::ParseError;

pub(crate) fn write_error(
    error: &ParseError,
    source: &str,
    stderr: &mut impl Write,
) -> io::Result<()> {
    match error {
        ParseError::UnexpectedToken { expected, found, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(stderr, "Parse error: Unexpected token at line {line}, column {col}")?;
            writeln!(stderr, "  Expected: {expected}")?;
            writeln!(stderr, "  Found: {found}")?;
            write_error_context(source, *location, stderr)?;
        }
        ParseError::UnexpectedEof => {
            writeln!(stderr, "Parse error: Unexpected end of input")?;
            if !source.is_empty() {
                write_error_context(source, source.len() - 1, stderr)?;
            }
        }
        ParseError::SyntaxError { message, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(stderr, "Parse error: {message} at line {line}, column {col}")?;
            write_error_context(source, *location, stderr)?;
        }
        ParseError::Advisory { message, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(stderr, "Parse advisory: {message} at line {line}, column {col}")?;
            write_error_context(source, *location, stderr)?;
        }
        ParseError::InvalidNumber { literal } => {
            writeln!(stderr, "Parse error: Invalid number literal: {literal}")?;
        }
        ParseError::InvalidString => {
            writeln!(stderr, "Parse error: Invalid string literal")?;
        }
        ParseError::UnclosedDelimiter { delimiter } => {
            writeln!(stderr, "Parse error: Unclosed delimiter: {delimiter}")?;
        }
        ParseError::InvalidRegex { message } => {
            writeln!(stderr, "Parse error: Invalid regex: {message}")?;
        }
        ParseError::LexerError { message } => {
            writeln!(stderr, "Parse error: Lexer error: {message}")?;
        }
        ParseError::RecursionLimit => {
            writeln!(stderr, "Parse error: Maximum recursion depth exceeded")?;
        }
        ParseError::NestingTooDeep { depth, max_depth } => {
            writeln!(stderr, "Parse error: Nesting too deep ({depth} > {max_depth})")?;
        }
        ParseError::Cancelled => {
            writeln!(stderr, "Parse error: Parsing cancelled")?;
        }
        ParseError::Recovered { site, kind, location } => {
            let (line, col) = position_to_line_col(source, *location);
            writeln!(stderr, "Parse recovery: {kind:?} at {site:?} (line {line}, column {col})")?;
            write_error_context(source, *location, stderr)?;
        }
        // Forward-compatible fallback for future variants (#2898)
        _ => {
            writeln!(stderr, "Parse error: {error}")?;
        }
    }
    Ok(())
}

pub(crate) fn position_to_line_col(source: &str, position: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;

    for (i, ch) in source.chars().enumerate() {
        if i >= position {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}

pub(crate) fn write_error_context(
    source: &str,
    position: usize,
    stderr: &mut impl Write,
) -> io::Result<()> {
    let lines: Vec<&str> = source.lines().collect();
    let (line_num, col_num) = position_to_line_col(source, position);

    if line_num > 0 && line_num <= lines.len() {
        writeln!(stderr)?;

        if line_num > 1 {
            writeln!(stderr, "  {} | {}", line_num - 1, lines[line_num - 2])?;
        }

        writeln!(stderr, "  {} | {}", line_num, lines[line_num - 1])?;

        write!(stderr, "  {} | ", " ".repeat(line_num.to_string().len()))?;
        writeln!(stderr, "{}^", " ".repeat(col_num - 1))?;

        if line_num < lines.len() {
            writeln!(stderr, "  {} | {}", line_num + 1, lines[line_num])?;
        }
    }
    Ok(())
}
