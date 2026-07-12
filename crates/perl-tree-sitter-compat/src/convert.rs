//! Convert a native parse into a tree-sitter-compatible [`TsNode`] tree.

use perl_parser_core::{Node, Parser as NativeParser};
use perl_workspace_core::Utf8LineIndex;
use tree_sitter_perl_rs::{Node as FacadeNode, Parser as FacadeParser};

use crate::dogfood;
use crate::node::{TsNode, TsPoint, pascal_to_snake};
use crate::shadow;

/// A failure to produce a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// The source could not be parsed as Perl.
    ParseFailed,
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed => write!(f, "could not parse source as Perl"),
        }
    }
}

impl std::error::Error for TreeError {}

/// Parse Perl `source` and return its tree-sitter-compatible tree.
///
/// # Errors
/// Returns [`TreeError::ParseFailed`] when the parser cannot produce a tree.
pub fn parse_to_tree(source: &str) -> Result<TsNode, TreeError> {
    let mut facade_parser = FacadeParser::new();
    let outcome = facade_parser.parse_detailed(source);
    let recovered = outcome.is_recovered();
    if let Some(tree) = outcome.tree {
        dogfood::record_facade_tree(recovered);
        return Ok(to_ts_node_facade(tree.root_node()));
    }

    dogfood::record_native_fallback();
    let ast = {
        let mut parser = NativeParser::new(source);
        parser.parse().map_err(|_| TreeError::ParseFailed)?
    };
    let _shadow = shadow::compare(source, &ast);
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
    let start_byte = u32::try_from(node.location.start).unwrap_or(u32::MAX);
    let end_byte = u32::try_from(node.location.end).unwrap_or(u32::MAX);
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

/// Convert a node from the Rust-native tree-sitter facade.
///
/// The facade is authoritative for normal and recovered parses. The native
/// AST conversion above remains available as a narrow catastrophic-failure
/// fallback, keeping this adapter's public output stable during adoption.
#[must_use]
pub fn to_ts_node_facade(node: FacadeNode<'_>) -> TsNode {
    TsNode {
        kind: pascal_to_snake(node.native_kind()),
        named: true,
        start_byte: u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
        end_byte: u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
        start_point: facade_point(node.start_position()),
        end_point: facade_point(node.end_position()),
        children: node.children().map(to_ts_node_facade).collect(),
    }
}

fn facade_point(point: tree_sitter_perl_rs::Point) -> TsPoint {
    TsPoint {
        row: u32::try_from(point.row).unwrap_or(u32::MAX),
        column: u32::try_from(point.column).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_package_to_a_tree() -> Result<(), TreeError> {
        let tree = parse_to_tree("package App;\nsub run { 1 }\n1;\n")?;
        assert_eq!(tree.kind, "program");
        assert!(tree.named);
        // The program spans the whole file.
        assert_eq!(tree.start_byte, 0);
        assert!(tree.descendant_count() > 1, "tree has nested nodes");
        Ok(())
    }

    #[test]
    fn points_are_zero_based_and_track_lines() -> Result<(), TreeError> {
        let tree = parse_to_tree("package App;\nsub run { 1 }\n")?;
        assert_eq!(tree.start_point, TsPoint { row: 0, column: 0 });
        // Find a node that starts on line 1 (the sub).
        assert!(
            has_node_starting_on_row(&tree, 1),
            "some node starts on row 1 (the sub on the second line)"
        );
        Ok(())
    }

    #[test]
    fn parse_failure_is_an_error_not_a_panic() {
        let bad = "{".repeat(5000);
        // Either an error or a recovered tree — never a panic.
        let _ = parse_to_tree(&bad);
    }

    #[test]
    fn tree_error_displays_readably() {
        assert_eq!(TreeError::ParseFailed.to_string(), "could not parse source as Perl");
    }

    #[test]
    fn normal_parse_uses_the_facade_as_primary_source() -> Result<(), TreeError> {
        let before = crate::adoption_stats();
        let tree = parse_to_tree("my $value = 42;\n")?;
        let after = crate::adoption_stats();
        assert_eq!(tree.kind, "program");
        assert!(after.facade_trees > before.facade_trees);
        assert_eq!(after.native_fallbacks, before.native_fallbacks);
        Ok(())
    }

    #[test]
    fn facade_conversion_preserves_the_established_native_shape() -> Result<(), TreeError> {
        let source = "package App;\nsub run { 1 }\nmy $value = 42;\n";
        let native = {
            let mut parser = NativeParser::new(source);
            parser.parse().map_err(|_| TreeError::ParseFailed)?
        };
        let mut facade_parser = FacadeParser::new();
        let facade_tree = facade_parser.parse(source).ok_or(TreeError::ParseFailed)?;
        let facade = facade_tree.root_node();
        assert_eq!(to_ts_node(&native, &Utf8LineIndex::new(source)), to_ts_node_facade(facade));
        Ok(())
    }

    #[test]
    fn recovered_facade_trees_are_counted_without_fallback() -> Result<(), TreeError> {
        let before = crate::adoption_stats();
        let tree = parse_to_tree("if (")?;
        let after = crate::adoption_stats();
        assert_eq!(tree.kind, "program");
        assert!(after.recovered_trees > before.recovered_trees);
        assert_eq!(after.native_fallbacks, before.native_fallbacks);
        Ok(())
    }

    fn has_node_starting_on_row(node: &TsNode, row: u32) -> bool {
        node.start_point.row == row
            || node.children.iter().any(|c| has_node_starting_on_row(c, row))
    }
}
