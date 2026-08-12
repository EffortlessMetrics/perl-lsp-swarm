//! Public advanced-reuse surface with complete-tree accounting.
//!
//! The reuse engine may inspect a bounded subset of node kinds while selecting
//! candidates. Public totals describe the produced AST, so they are normalized
//! through the canonical [`Node::children`] traversal before being exposed.

use super::incremental_advanced_reuse_engine as engine;
use perl_parser_core::{ast::Node, edit::EditSet};
use std::collections::HashMap;

pub use engine::{
    ReuseAnalysisResult, ReuseAnalysisStats, ReuseConfig, ReuseStrategy, ReuseType,
};

/// Advanced reuse analyzer with canonical whole-tree metrics.
#[derive(Debug)]
pub struct AdvancedReuseAnalyzer {
    inner: engine::AdvancedReuseAnalyzer,
    /// Statistics from the most recent reuse analysis.
    pub analysis_stats: ReuseAnalysisStats,
}

impl AdvancedReuseAnalyzer {
    /// Create an analyzer with the default reuse configuration.
    pub fn new() -> Self {
        let inner = engine::AdvancedReuseAnalyzer::new();
        let analysis_stats = inner.analysis_stats.clone();
        Self {
            inner,
            analysis_stats,
        }
    }

    /// Create an analyzer with a caller-supplied reuse configuration.
    pub fn with_config(config: ReuseConfig) -> Self {
        let inner = engine::AdvancedReuseAnalyzer::with_config(config);
        let analysis_stats = inner.analysis_stats.clone();
        Self {
            inner,
            analysis_stats,
        }
    }

    /// Analyze reuse opportunities and report totals for the complete ASTs.
    pub fn analyze_reuse_opportunities(
        &mut self,
        old_tree: &Node,
        new_tree: &Node,
        edits: &EditSet,
        config: &ReuseConfig,
    ) -> ReuseAnalysisResult {
        let mut result = self
            .inner
            .analyze_reuse_opportunities(old_tree, new_tree, edits, config);
        self.analysis_stats = result.analysis_stats.clone();

        result.total_old_nodes = canonical_node_count(old_tree);
        result.total_new_nodes = canonical_node_count(new_tree);
        result.reuse_percentage = if result.total_old_nodes == 0 {
            0.0
        } else {
            result.reused_nodes as f64 / result.total_old_nodes as f64 * 100.0
        };
        result
    }

    /// Map an old-tree byte position through an edit set.
    pub fn map_old_position_to_new(&self, old_pos: usize, edits: &EditSet) -> usize {
        self.inner.map_old_position_to_new(old_pos, edits)
    }

    /// Register a one-to-one reuse match.
    pub fn try_register_match(
        &self,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        old_pos: usize,
        new_pos: usize,
        reuse_type: ReuseType,
        confidence: f64,
    ) -> bool {
        self.inner.try_register_match(
            reuse_map,
            old_pos,
            new_pos,
            reuse_type,
            confidence,
        )
    }
}

impl Default for AdvancedReuseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn canonical_node_count(node: &Node) -> usize {
    1 + node
        .children()
        .into_iter()
        .map(canonical_node_count)
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::{SourceLocation, ast::NodeKind, edit::Edit, position::Position};

    #[test]
    fn public_totals_include_children_of_every_node_kind() {
        let value = Node::new(
            NodeKind::Number {
                value: "1".to_string(),
            },
            SourceLocation { start: 7, end: 8 },
        );
        let return_statement = Node::new(
            NodeKind::Return {
                value: Some(Box::new(value)),
            },
            SourceLocation { start: 0, end: 8 },
        );
        let tree = Node::new(
            NodeKind::Program {
                statements: vec![return_statement],
            },
            SourceLocation { start: 0, end: 8 },
        );

        let mut analyzer = AdvancedReuseAnalyzer::new();
        let result = analyzer.analyze_reuse_opportunities(
            &tree,
            &tree,
            &EditSet::new(),
            &ReuseConfig::default(),
        );

        assert_eq!(result.total_old_nodes, 3);
        assert_eq!(result.total_new_nodes, 3);
    }

    #[test]
    fn public_reuse_map_assigns_each_new_position_once() {
        let old_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number {
                            value: "1".to_string(),
                        },
                        SourceLocation { start: 1, end: 2 },
                    ),
                    Node::new(
                        NodeKind::Number {
                            value: "2".to_string(),
                        },
                        SourceLocation { start: 10, end: 11 },
                    ),
                ],
            },
            SourceLocation { start: 0, end: 11 },
        );
        let new_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number {
                            value: "3".to_string(),
                        },
                        SourceLocation { start: 1, end: 2 },
                    ),
                    Node::new(
                        NodeKind::Number {
                            value: "4".to_string(),
                        },
                        SourceLocation { start: 10, end: 11 },
                    ),
                ],
            },
            SourceLocation { start: 0, end: 11 },
        );
        let mut edits = EditSet::new();
        edits.add(Edit::new(
            0,
            11,
            11,
            Position::new(0, 0, 0),
            Position::new(11, 0, 11),
            Position::new(11, 0, 11),
        ));

        let mut analyzer = AdvancedReuseAnalyzer::new();
        let result = analyzer.analyze_reuse_opportunities(
            &old_tree,
            &new_tree,
            &edits,
            &ReuseConfig::default(),
        );
        let unique_targets = result
            .reuse_map
            .values()
            .map(|strategy| strategy.target_position)
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(unique_targets.len(), result.reuse_map.len());
        assert_eq!(result.reused_nodes, result.reuse_map.len());
    }
}
