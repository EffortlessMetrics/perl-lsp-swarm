//! Convert a native parse into a tree-sitter-compatible [`TsNode`] tree.

use perl_parser_core::{Node, ParseError, Parser};
use perl_workspace_core::Utf8LineIndex;

use crate::node::{TsNode, TsPoint, pascal_to_snake};

/// A failure to produce a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// The source could not be parsed as Perl.
    ///
    /// `offset` and `kind` are taken from the native parser diagnostic. Budget
    /// failures (`nesting_too_deep`, `recursion_depth_exhausted`, …) often have
    /// no byte offset; `kind` still distinguishes those variants.
    ParseFailed {
        /// Byte offset recorded by the native parser, when that failure carries one.
        offset: Option<usize>,
        /// Native parser error variant token (`syntax_error`, `nesting_too_deep`, …).
        ///
        /// Future `ParseError` variants that this adapter has not named yet are
        /// projected as `unknown:{native Debug}` so they do not collapse together.
        kind: String,
    },
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { offset: Some(offset), kind } => {
                write!(f, "could not parse source as Perl ({kind} at byte {offset})")
            }
            Self::ParseFailed { offset: None, kind } => {
                write!(f, "could not parse source as Perl ({kind})")
            }
        }
    }
}

impl std::error::Error for TreeError {}

fn tree_error_from_parse_error(error: ParseError) -> TreeError {
    TreeError::ParseFailed { offset: error.location(), kind: parse_error_kind(&error) }
}

fn parse_error_kind(error: &ParseError) -> String {
    match error {
        ParseError::UnexpectedEof => "unexpected_eof".to_owned(),
        ParseError::UnexpectedToken { .. } => "unexpected_token".to_owned(),
        ParseError::SyntaxError { .. } => "syntax_error".to_owned(),
        ParseError::Advisory { .. } => "advisory".to_owned(),
        ParseError::LexerError { .. } => "lexer_error".to_owned(),
        ParseError::RecursionLimit => "recursion_limit".to_owned(),
        ParseError::RecursionDepthExhausted { .. } => "recursion_depth_exhausted".to_owned(),
        ParseError::InvalidNumber { .. } => "invalid_number".to_owned(),
        ParseError::InvalidString => "invalid_string".to_owned(),
        ParseError::UnclosedDelimiter { .. } => "unclosed_delimiter".to_owned(),
        ParseError::InvalidRegex { .. } => "invalid_regex".to_owned(),
        ParseError::NestingTooDeep { .. } => "nesting_too_deep".to_owned(),
        ParseError::Cancelled => "cancelled".to_owned(),
        ParseError::Recovered { .. } => "recovered".to_owned(),
        other => format!("unknown:{other:?}"),
    }
}

/// Parse Perl `source` and return its tree-sitter-compatible tree.
///
/// # Errors
/// Returns [`TreeError::ParseFailed`] when the parser cannot produce a tree,
/// carrying the native diagnostic's offset (when present) and kind.
pub fn parse_to_tree(source: &str) -> Result<TsNode, TreeError> {
    let ast = {
        let mut parser = Parser::new(source);
        parser.parse().map_err(tree_error_from_parse_error)?
    };
    let line_index = Utf8LineIndex::new(source);
    Ok(to_ts_node(&ast, &line_index))
}

/// Convert one native [`Node`] (and its named children) into a [`TsNode`].
///
/// Children are emitted in the native AST's structural visit order (which
/// follows source order for each node's components); point columns are UTF-8
/// byte offsets within the row, matching tree-sitter's `Point`.
#[must_use]
pub fn to_ts_node(node: &Node, line_index: &Utf8LineIndex) -> TsNode {
    let start_byte = u32::try_from(node.location.start()).unwrap_or(u32::MAX);
    let end_byte = u32::try_from(node.location.end()).unwrap_or(u32::MAX);
    let (start_row, start_column) = line_index.line_col(start_byte);
    let (end_row, end_column) = line_index.line_col(end_byte);
    TsNode {
        kind: pascal_to_snake(node.kind.kind_name()),
        named: true,
        start_byte,
        end_byte,
        start_point: TsPoint { row: start_row, column: start_column },
        end_point: TsPoint { row: end_row, column: end_column },
        children: node.children().iter().map(|child| to_ts_node(child, line_index)).collect(),
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;

    #[test]
    fn parses_a_package_to_a_tree() {
        let tree = parse_to_tree("package App;\nsub run { 1 }\n1;\n").unwrap();
        assert_eq!(tree.kind, "program");
        assert!(tree.named);
        // The program spans the whole file.
        assert_eq!(tree.start_byte, 0);
        assert!(tree.descendant_count() > 1, "tree has nested nodes");
    }

    #[test]
    fn points_are_zero_based_and_track_lines() {
        let tree = parse_to_tree("package App;\nsub run { 1 }\n").unwrap();
        assert_eq!(tree.start_point, TsPoint { row: 0, column: 0 });
        // Find a node that starts on line 1 (the sub).
        assert!(
            has_node_starting_on_row(&tree, 1),
            "some node starts on row 1 (the sub on the second line)"
        );
    }

    #[test]
    fn parse_failure_is_an_error_not_a_panic() {
        let bad = "{".repeat(5000);
        // Either an error or a recovered tree — never a panic.
        let _ = parse_to_tree(&bad);
    }

    #[test]
    fn parse_failed_payload_distinguishes_located_native_errors() {
        // Mapper-level offset proof: `ParseError::syntax` carries a location, but
        // `Parser::parse()` currently returns `Err` only for budget failures whose
        // `location()` is `None`. Live `parse_to_tree` discrimination is the
        // adapter integration test (kind, and offset when the native error has one).
        let early = tree_error_from_parse_error(ParseError::syntax("early failure", 3));
        let late = tree_error_from_parse_error(ParseError::syntax("late failure", 19));
        assert_ne!(early, late, "same kind at different offsets must remain distinct");
        assert_eq!(
            early,
            TreeError::ParseFailed { offset: Some(3), kind: "syntax_error".to_owned() }
        );
        assert_eq!(
            late,
            TreeError::ParseFailed { offset: Some(19), kind: "syntax_error".to_owned() }
        );
    }

    #[test]
    fn tree_error_displays_readably() {
        assert_eq!(
            TreeError::ParseFailed { offset: None, kind: "nesting_too_deep".to_owned() }
                .to_string(),
            "could not parse source as Perl (nesting_too_deep)"
        );
        assert_eq!(
            TreeError::ParseFailed { offset: Some(12), kind: "syntax_error".to_owned() }
                .to_string(),
            "could not parse source as Perl (syntax_error at byte 12)"
        );
    }

    fn has_node_starting_on_row(node: &TsNode, row: u32) -> bool {
        node.start_point.row == row
            || node.children.iter().any(|c| has_node_starting_on_row(c, row))
    }
}
