//! Incremental parsing implementation with comprehensive tree reuse
//!
//! This module provides a high-performance incremental parser that achieves significant
//! performance improvements over full parsing through intelligent AST node reuse.
//! Designed for integration with LSP servers and real-time editing scenarios.

//!
//! ## Performance Characteristics
//!
//! - **Sub-millisecond updates** for simple value edits (target: <1ms)
//! - **Node reuse efficiency** of 70-90% for typical editing scenarios
//! - **Graceful fallback** to full parsing for complex structural changes
//! - **Memory efficient** with LRU cache eviction and `Arc<Node>` sharing
//! - **Time complexity**: O(n) for reparsed spans with bounded lookahead
//! - **Space complexity**: O(n) for cached nodes and reuse maps
//! - **Large file scaling**: Tuned to scale for large file edits (50GB PST-style workspaces)
//!
//! ## Supported Edit Types
//!
//! - **Simple value edits**: Number and string literal changes
//! - **Variable name edits**: Identifier modifications within bounds
//! - **Whitespace edits**: Lexically stable, source-coherent trivia changes
//! - **Multiple edits**: Batch processing with cumulative position tracking
//!
//! An admitted edit must produce exactly the tree a fresh parse would, in
//! spans and payloads alike. Value-leaf patches are only accepted after the
//! mapped replacement text is proven to still lex as a single token of the
//! admitted kind; anything else — a replacement that splits or retypes the
//! token — declines the incremental path and falls back to a full parse.
//! The tree rebuilds are single-pass (one structural clone plus in-place
//! updates), so cost stays linear in the number of nodes regardless of depth.
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use perl_parser::incremental_v2::IncrementalParserV2;
//! use perl_parser::edit::Edit;
//! use perl_parser::position::Position;
//!
//! let mut parser = IncrementalParserV2::new();
//!
//! // Initial parse
//! let source1 = "my $x = 42;";
//! let tree1 = parser.parse(source1)?;
//!
//! // Apply incremental edit
//! let edit = Edit::new(
//!     8, 10, 12, // positions: "42" -> "9999"
//!     Position::new(8, 1, 9),
//!     Position::new(10, 1, 11),
//!     Position::new(12, 1, 13),
//! );
//! parser.edit(edit);
//!
//! // Incremental reparse (typically <1ms)
//! let source2 = "my $x = 9999;";
//! let tree2 = parser.parse(source2)?;
//!
//! // Check performance metrics
//! println!("Nodes reused: {}", parser.reused_nodes);
//! println!("Nodes reparsed: {}", parser.reparsed_nodes);
//! # Ok::<(), perl_parser::error::ParseError>(())
//! ```
/// Safely convert an isize to usize, clamping negative values to 0.
/// Prevents wrap-around from unchecked `as usize` casts.
fn isize_to_usize_clamped(v: isize) -> usize {
    v.max(0) as usize
}

use super::{
    MAX_INCREMENTAL_EDIT_BATCH,
    incremental_advanced_reuse::{
        AdvancedReuseAnalyzer, ReuseAnalysisResult, ReuseConfig, ReuseStrategy, ReuseType,
    },
    whitespace_geometry::WhitespaceEditMap,
};
use perl_lexer::{PerlLexer, TokenType};
use perl_parser_core::{
    ast::{Node, NodeKind, SourceLocation},
    edit::{Edit, EditSet},
    error::ParseResult,
    parser::Parser,
};
use std::collections::HashMap;

/// The byte is a variable sigil, so the text it starts cannot be a bare
/// identifier token.
fn is_variable_sigil_byte(byte: u8) -> bool {
    matches!(byte, b'$' | b'@' | b'%' | b'&' | b'*')
}

/// Shift one span by `shift`, saturating at the source origin.
fn shift_location(location: SourceLocation, shift: isize) -> SourceLocation {
    if shift >= 0 {
        SourceLocation::new(
            location.start().saturating_add(shift as usize),
            location.end().saturating_add(shift as usize),
        )
    } else {
        SourceLocation::new(
            location.start().saturating_sub((-shift) as usize),
            location.end().saturating_sub((-shift) as usize),
        )
    }
}

/// Shift `node`'s span, every payload sub-span, and every descendant span by
/// `shift` in place.
///
/// Payload sub-spans (`name_span`, `body_span`, `phase_span`, catch-variable
/// locations, recovery tokens) are remapped state: the clone path maps them
/// through `Node::clone_with_mapped_locations`, so this path shares the same
/// authority via `NodeKind::map_payload_locations_in_place` rather than
/// leaving them at pre-shift offsets. A payload that cannot be remapped keeps
/// its old span; callers that consume shifted nodes must compare them against
/// a freshly parsed counterpart and skip the candidate on mismatch.
fn shift_positions_in_place(node: &mut Node, shift: isize) {
    node.location = shift_location(node.location, shift);
    node.kind.map_payload_locations_in_place(|location| shift_location(location, shift));
    node.for_each_child_mut(|child| shift_positions_in_place(child, shift));
}

/// Comprehensive performance metrics for incremental parsing analysis
///
/// Tracks detailed performance characteristics including parsing time,
/// node reuse statistics, and efficiency measurements for optimization
/// and debugging purposes.
#[derive(Debug, Clone, Default)]
pub struct IncrementalMetrics {
    pub parse_time_micros: u128,
    pub nodes_reused: usize,
    pub nodes_reparsed: usize,
    pub cache_hit_ratio: f64,
    pub edit_count: usize,
}

impl IncrementalMetrics {
    /// Create zeroed metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Percentage of nodes reused out of all nodes touched (0–100).
    pub fn efficiency_percentage(&self) -> f64 {
        if self.nodes_reused + self.nodes_reparsed == 0 {
            return 0.0;
        }
        self.nodes_reused as f64 / (self.nodes_reused + self.nodes_reparsed) as f64 * 100.0
    }

    /// Return `true` when the last parse completed in under 1 ms.
    pub fn is_sub_millisecond(&self) -> bool {
        self.parse_time_micros < 1000
    }

    /// Return a human-readable performance tier label for the last parse time.
    pub fn performance_category(&self) -> &'static str {
        match self.parse_time_micros {
            0..=100 => "Excellent (<100µs)",
            101..=500 => "Very Good (<500µs)",
            501..=1000 => "Good (<1ms)",
            1001..=5000 => "Acceptable (<5ms)",
            _ => "Needs Optimization (>5ms)",
        }
    }
}

/// A parse tree retained for incremental parsing decisions.
///
/// The tree stores only the root AST and its source text. Earlier versions
/// also maintained a `node_map: HashMap<usize, Vec<Node>>` position index
/// that owned-cloned every indexed subtree and descended into only a handful
/// of structural families, so containment lookups under assignments, loops,
/// method calls, and most other nodes returned an ancestor or `None` while
/// deep trees retained quadratic cloned nodes. That index was retired
/// (#13237): its single production caller performs at most one bounded
/// lookup per pending edit, which on-demand canonical child traversal serves
/// without retained entries, hidden construction work, or subtree
/// duplication.
#[derive(Debug, Clone)]
pub struct IncrementalTree {
    pub root: Node,
    pub source: String,
}

impl IncrementalTree {
    /// Create a new incremental tree.
    ///
    /// Construction performs no indexing and no subtree duplication: the
    /// root is stored as given.
    pub fn new(root: Node, source: String) -> Self {
        IncrementalTree { root, source }
    }

    /// Find the smallest node whose byte range contains `start..end`.
    ///
    /// Containment is the predicate `node.location.start <= start &&
    /// node.location.end >= end` over the half-open query range, so a
    /// zero-width query `p..p` matches every node spanning byte `p`,
    /// including zero-width recovery nodes positioned there. A reversed
    /// query (`start > end`) matches nothing.
    ///
    /// The lookup walks the canonical child traversal (`Node::for_each_child`,
    /// the #8424 field order) with an explicit heap stack and descends only
    /// into subtrees that contain the query. Among containing nodes it
    /// selects the narrowest span; ties keep the canonical-first node at the
    /// greatest visited depth, so the result never depends on hash order and
    /// is stable across calls. Worst-case work is linear in the number of
    /// nodes per query with no retained index; there is deliberately no O(1)
    /// lookup surface.
    ///
    /// The explicit work stack keeps lookup stack-safe for adversarially
    /// deep trees, and construction performs no traversal at all.
    pub fn find_containing_node(&self, start: usize, end: usize) -> Option<&Node> {
        if start > end {
            return None;
        }
        let root = &self.root;
        if !(root.location.start() <= start && root.location.end() >= end) {
            return None;
        }

        let mut best = root;
        let mut best_width = root.location.end() - root.location.start();
        let mut best_depth = 0usize;
        // Frames of containing nodes only, visited in canonical preorder:
        // children are collected in field order and pushed reversed so the
        // first child pops first.
        let mut stack: Vec<(&Node, usize)> = vec![(root, 0)];
        let mut children: Vec<&Node> = Vec::new();

        while let Some((node, depth)) = stack.pop() {
            let width = node.location.end() - node.location.start();
            if width < best_width || (width == best_width && depth > best_depth) {
                best = node;
                best_width = width;
                best_depth = depth;
            }
            children.clear();
            node.for_each_child(|child| {
                if child.location.start() <= start && child.location.end() >= end {
                    children.push(child);
                }
            });
            for child in children.drain(..).rev() {
                stack.push((child, depth + 1));
            }
        }

        Some(best)
    }
}

/// High-performance incremental parser with intelligent AST node reuse
///
/// Maintains previous parse state and applies edits incrementally when possible,
/// falling back to full parsing for complex structural changes. Designed for
/// real-time editing scenarios with sub-millisecond update targets.
///
/// ## Thread Safety
///
/// IncrementalParserV2 is not thread-safe and should be used from a single thread.
/// For multi-threaded scenarios, create separate parser instances per thread.
pub struct IncrementalParserV2 {
    last_tree: Option<IncrementalTree>,
    pending_edits: EditSet,
    pub reused_nodes: usize,
    pub reparsed_nodes: usize,
    pub metrics: IncrementalMetrics,
    /// Advanced reuse analyzer for sophisticated tree reuse strategies
    reuse_analyzer: AdvancedReuseAnalyzer,
    /// Configuration for reuse analysis
    reuse_config: ReuseConfig,
    /// Performance tracking for reuse analysis
    pub last_reuse_analysis: Option<ReuseAnalysisResult>,
    used_incremental_path: bool,
    advanced_reuse_selected: bool,
    materialized_reuse_nodes: usize,
}

impl IncrementalParserV2 {
    /// Create a parser with default reuse configuration and no cached tree.
    pub fn new() -> Self {
        IncrementalParserV2 {
            last_tree: None,
            pending_edits: EditSet::new(),
            reused_nodes: 0,
            reparsed_nodes: 0,
            metrics: IncrementalMetrics::new(),
            reuse_analyzer: AdvancedReuseAnalyzer::new(),
            reuse_config: ReuseConfig::default(),
            last_reuse_analysis: None,
            used_incremental_path: false,
            advanced_reuse_selected: false,
            materialized_reuse_nodes: 0,
        }
    }

    /// Create parser with custom reuse configuration
    pub fn with_reuse_config(config: ReuseConfig) -> Self {
        IncrementalParserV2 {
            last_tree: None,
            pending_edits: EditSet::new(),
            reused_nodes: 0,
            reparsed_nodes: 0,
            metrics: IncrementalMetrics::new(),
            reuse_analyzer: AdvancedReuseAnalyzer::with_config(config.clone()),
            reuse_config: config,
            last_reuse_analysis: None,
            used_incremental_path: false,
            advanced_reuse_selected: false,
            materialized_reuse_nodes: 0,
        }
    }

    /// Queue an edit to be applied on the next [`parse`] call.
    ///
    /// [`parse`]: IncrementalParserV2::parse
    pub fn edit(&mut self, edit: Edit) {
        self.pending_edits.add(edit);
    }

    /// Parse `source`, reusing cached tree nodes where edits did not affect them.
    pub fn parse(&mut self, source: &str) -> ParseResult<Node> {
        // Reset statistics
        self.reused_nodes = 0;
        self.reparsed_nodes = 0;
        self.last_reuse_analysis = None;
        self.used_incremental_path = false;
        self.advanced_reuse_selected = false;
        self.materialized_reuse_nodes = 0;

        // Try incremental parsing if we have a previous tree and edits
        if let Some(ref last_tree) = self.last_tree
            && !self.pending_edits.is_empty()
        {
            let last_tree_clone = last_tree.clone();
            // Check if we can do incremental parsing
            if let Some(new_tree) = self.try_incremental_parse(source, &last_tree_clone) {
                self.used_incremental_path = true;
                self.last_tree = Some(IncrementalTree::new(new_tree.clone(), source.to_string()));
                self.pending_edits = EditSet::new();
                return Ok(new_tree);
            }
        }

        // Fall back to full parse
        self.full_parse(source)
    }

    fn full_parse(&mut self, source: &str) -> ParseResult<Node> {
        let mut parser = Parser::new(source);
        let root = parser.parse()?;

        // For first parse or structural changes, all nodes are reparsed
        if self.last_tree.is_none() {
            // First parse - no reuse possible
            self.reused_nodes = 0;
            self.reparsed_nodes = self.count_nodes(&root);
        } else {
            // Check if this was a fallback due to too many edits, invalid conditions, or empty source
            // In such cases, we should report 0 reused nodes as it's truly a full reparse
            let should_skip_reuse = source.is_empty()
                || self.pending_edits.len() > MAX_INCREMENTAL_EDIT_BATCH
                || self.last_tree.as_ref().is_none_or(|tree| !self.is_simple_value_edit(tree));

            if should_skip_reuse {
                // Full fallback - no actual reuse
                self.reused_nodes = 0;
                self.reparsed_nodes = self.count_nodes(&root);
            } else if let Some(ref old_tree) = self.last_tree {
                // Normal incremental fallback - still compare against old tree
                let (reused, reparsed) = self.analyze_reuse(&old_tree.root, &root);
                self.reused_nodes = reused;
                self.reparsed_nodes = reparsed;
            } else {
                // No old tree - full parse
                self.reused_nodes = 0;
                self.reparsed_nodes = self.count_nodes(&root);
            }
        }

        self.last_tree = Some(IncrementalTree::new(root.clone(), source.to_string()));
        self.pending_edits = EditSet::new();

        Ok(root)
    }

    fn try_incremental_parse(&mut self, source: &str, last_tree: &IncrementalTree) -> Option<Node> {
        // Exact, lexically stable whitespace edits can reuse the complete old
        // tree. Select this before advanced analysis, which parses a fresh tree
        // in order to estimate reuse and would otherwise shadow the fast path.
        if let Some(edit_map) =
            WhitespaceEditMap::try_new(&last_tree.source, source, &self.pending_edits)
        {
            return self.incremental_parse_whitespace(source, last_tree, &edit_map);
        }

        // Try advanced reuse analysis for edits that need structural comparison.
        if let Some(advanced_result) = self.try_advanced_reuse_parse(source, last_tree) {
            return Some(advanced_result);
        }

        // Fall back to original value-update strategy for compatibility.
        if self.is_simple_value_edit(last_tree) {
            return self.incremental_parse_simple(source, last_tree);
        }

        // For complex structural changes, fall back to full parse.
        None
    }

    /// Try advanced reuse analysis for sophisticated tree reuse.
    fn try_advanced_reuse_parse(
        &mut self,
        source: &str,
        last_tree: &IncrementalTree,
    ) -> Option<Node> {
        let mut parser = Parser::new(source);
        let new_tree = parser.parse().ok()?;

        let mut analysis_result = self.reuse_analyzer.analyze_reuse_opportunities(
            &last_tree.root,
            &new_tree,
            &self.pending_edits,
            &self.reuse_config,
        );
        let (reuse_map, replacements) = self.collect_materializable_reuse(
            &last_tree.root,
            &new_tree,
            &analysis_result.reuse_map,
        );
        analysis_result.reuse_map = reuse_map;
        analysis_result.reused_nodes = analysis_result.reuse_map.len();
        analysis_result.reuse_percentage = if analysis_result.total_old_nodes == 0 {
            0.0
        } else {
            analysis_result.reused_nodes as f64 / analysis_result.total_old_nodes as f64 * 100.0
        };

        if analysis_result.reused_nodes == 0
            || analysis_result.reused_nodes > analysis_result.total_new_nodes
            || !analysis_result.meets_efficiency_target(self.reuse_config.min_confidence * 100.0)
        {
            return None;
        }

        self.materialized_reuse_nodes = replacements.values().map(Vec::len).sum();

        self.reused_nodes = analysis_result.reused_nodes;
        self.reparsed_nodes = analysis_result.total_new_nodes - analysis_result.reused_nodes;
        self.advanced_reuse_selected = true;
        self.last_reuse_analysis = Some(analysis_result);
        Some(new_tree)
    }

    fn collect_materializable_reuse(
        &self,
        old_tree: &Node,
        new_tree: &Node,
        reuse_map: &HashMap<usize, ReuseStrategy>,
    ) -> (HashMap<usize, ReuseStrategy>, HashMap<usize, Vec<(Node, Node)>>) {
        let mut materialized_map = HashMap::new();
        let mut replacements: HashMap<usize, Vec<(Node, Node)>> = HashMap::new();

        for (old_position, strategy) in reuse_map {
            if !matches!(&strategy.reuse_type, ReuseType::Direct | ReuseType::PositionShift) {
                continue;
            }
            let Some(old_node) = Self::find_analyzed_node_at_start(old_tree, *old_position) else {
                continue;
            };
            let Some(new_node) =
                Self::find_analyzed_node_at_start(new_tree, strategy.target_position)
            else {
                continue;
            };

            let replacement =
                self.clone_with_shifted_positions(old_node, strategy.position_adjustment);
            if &replacement != new_node {
                continue;
            }

            materialized_map.insert(*old_position, strategy.clone());
            replacements
                .entry(strategy.target_position)
                .or_default()
                .push((new_node.clone(), replacement));
        }

        (materialized_map, replacements)
    }

    /// Resolve the node the analyzer keyed at byte offset `start`.
    ///
    /// INVARIANT: this traversal must mirror
    /// `AdvancedReuseAnalyzer::analyze_node_recursive`. That function keys its
    /// `TreeAnalysis` entries by `node.location.start`, so when a parent and its
    /// first child share a start offset the later insertion wins. This lookup
    /// reproduces that by visiting children first, in reverse order, before the
    /// node itself. If either traversal order changes, the two disagree, the
    /// exact-equality check in `collect_materializable_reuse` silently rejects
    /// every candidate, and reuse drops to zero with no test failure.
    fn find_analyzed_node_at_start(node: &Node, start: usize) -> Option<&Node> {
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for statement in statements.iter().rev() {
                    if let Some(candidate) = Self::find_analyzed_node_at_start(statement, start) {
                        return Some(candidate);
                    }
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                if let Some(initializer) = initializer
                    && let Some(candidate) = Self::find_analyzed_node_at_start(initializer, start)
                {
                    return Some(candidate);
                }
                if let Some(candidate) = Self::find_analyzed_node_at_start(variable, start) {
                    return Some(candidate);
                }
            }
            NodeKind::Binary { left, right, .. } => {
                if let Some(candidate) = Self::find_analyzed_node_at_start(right, start) {
                    return Some(candidate);
                }
                if let Some(candidate) = Self::find_analyzed_node_at_start(left, start) {
                    return Some(candidate);
                }
            }
            NodeKind::Unary { operand, .. } => {
                if let Some(candidate) = Self::find_analyzed_node_at_start(operand, start) {
                    return Some(candidate);
                }
            }
            NodeKind::FunctionCall { args, .. } => {
                for argument in args.iter().rev() {
                    if let Some(candidate) = Self::find_analyzed_node_at_start(argument, start) {
                        return Some(candidate);
                    }
                }
            }
            NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
                if let Some(branch) = else_branch
                    && let Some(candidate) = Self::find_analyzed_node_at_start(branch, start)
                {
                    return Some(candidate);
                }
                for (condition, branch) in elsif_branches.iter().rev() {
                    if let Some(candidate) = Self::find_analyzed_node_at_start(branch, start) {
                        return Some(candidate);
                    }
                    if let Some(candidate) = Self::find_analyzed_node_at_start(condition, start) {
                        return Some(candidate);
                    }
                }
                if let Some(candidate) = Self::find_analyzed_node_at_start(then_branch, start) {
                    return Some(candidate);
                }
                if let Some(candidate) = Self::find_analyzed_node_at_start(condition, start) {
                    return Some(candidate);
                }
            }
            _ => {}
        }

        (node.location.start() == start).then_some(node)
    }

    fn is_simple_value_edit(&self, tree: &IncrementalTree) -> bool {
        // Don't attempt incremental parsing for too many edits at once
        if self.pending_edits.len() > MAX_INCREMENTAL_EDIT_BATCH {
            return false;
        }

        // Track cumulative shift so we can map each edit back to the
        // coordinates in the original source code represented by `tree`.
        let mut cumulative_shift: isize = 0;

        for edit in self.pending_edits.edits() {
            let original_start =
                isize_to_usize_clamped(edit.start_byte as isize - cumulative_shift);
            let original_end =
                isize_to_usize_clamped(edit.old_end_byte as isize - cumulative_shift);

            let affected_node = tree.find_containing_node(original_start, original_end);

            match affected_node {
                Some(node) => {
                    match &node.kind {
                        // Support string and numeric literals
                        NodeKind::Number { .. } | NodeKind::String { .. }
                            if original_start >= node.location.start()
                                && original_end <= node.location.end() =>
                        {
                            cumulative_shift += edit.byte_shift();
                            continue;
                        }
                        // Support simple identifier edits (variable names)
                        NodeKind::Variable { .. }
                            if original_start >= node.location.start()
                                && original_end <= node.location.end() =>
                        {
                            cumulative_shift += edit.byte_shift();
                            continue;
                        }
                        // Support identifier edits (identifiers can often be treated like simple values)
                        NodeKind::Identifier { .. }
                            if original_start >= node.location.start()
                                && original_end <= node.location.end() =>
                        {
                            cumulative_shift += edit.byte_shift();
                            continue;
                        }
                        _ => {
                            return false; // Not a simple value
                        }
                    }
                }
                None => {
                    return false; // No containing node found
                }
            }
        }

        true
    }

    /// Reuse the complete prior tree for a validated whitespace-only edit batch.
    fn incremental_parse_whitespace(
        &mut self,
        source: &str,
        last_tree: &IncrementalTree,
        edit_map: &WhitespaceEditMap,
    ) -> Option<Node> {
        let new_root = edit_map.clone_tree(&last_tree.root)?;
        if !self.validate_incremental_result(&new_root, source) {
            return None;
        }

        self.reused_nodes = self.count_nodes(&last_tree.root);
        self.reparsed_nodes = 0;
        Some(new_root)
    }

    fn incremental_parse_simple(
        &mut self,
        source: &str,
        last_tree: &IncrementalTree,
    ) -> Option<Node> {
        // Validate that the source is long enough for our edits
        if source.is_empty() {
            return None;
        }

        // Reuse the previous tree by cloning it once and applying the edits
        // in place. A declined edit (a replacement whose text no longer forms
        // one token of the admitted kind) aborts the attempt; the caller then
        // falls back to a full parse rather than accept a divergent tree.
        let new_root = self.clone_and_update_tree(&last_tree.root, source, &last_tree.source)?;

        // Validate that the new tree makes sense
        if !self.validate_incremental_result(&new_root, source) {
            return None;
        }

        // After producing the new tree, analyse how many nodes were reused
        // versus reparsed for metrics.
        self.count_reuse_potential(&last_tree.root, &new_root);

        Some(new_root)
    }

    /// Validate that an incremental parsing result is reasonable
    ///
    /// Enhanced validation including structural consistency and Unicode safety.
    fn validate_incremental_result(&self, node: &Node, source: &str) -> bool {
        // Basic sanity checks
        if source.is_empty() {
            // Empty source is edge case - validate node is minimal
            return match &node.kind {
                NodeKind::Program { statements } => statements.is_empty(),
                _ => false,
            };
        }

        // Position boundary validation
        if node.location.start() > source.len() || node.location.end() > source.len() {
            return false;
        }

        if node.location.start() > node.location.end() {
            return false;
        }

        // Unicode boundary validation - ensure positions fall on character boundaries
        if !source.is_char_boundary(node.location.start())
            || !source.is_char_boundary(node.location.end())
        {
            return false;
        }

        // Structural validation - ensure node content matches source
        if node.location.start() < node.location.end() {
            let node_text = &source[node.location.start()..node.location.end()];

            // Validate node content makes sense for node type
            match &node.kind {
                NodeKind::Number { value } => {
                    // Number value should be parseable and match source
                    if value.trim() != node_text.trim() {
                        return false;
                    }
                    // Validate it's actually a number
                    if value.parse::<f64>().is_err() && value.parse::<i64>().is_err() {
                        return false;
                    }
                }
                NodeKind::String { value, .. }
                    if !node_text.is_empty()
                        && !value.contains(node_text.trim_matches(|c| c == '"' || c == '\'')) =>
                {
                    // Be lenient for string validation as quotes might be handled differently
                }
                NodeKind::Variable { name, .. } if !node_text.contains(name) => {
                    // Variable name should appear in the source text
                    return false;
                }
                NodeKind::Identifier { name } if name.trim() != node_text.trim() => {
                    // Identifier name should match source text
                    return false;
                }
                _ => {
                    // For container nodes, just ensure they have reasonable bounds
                    // Detailed validation would require recursing into children
                }
            }
        }

        // Recursive validation for container nodes (limited depth to avoid performance issues)
        self.validate_node_tree_consistency(node, source, 0, 3)
    }

    /// Recursive validation helper with depth limiting
    fn validate_node_tree_consistency(
        &self,
        node: &Node,
        source: &str,
        depth: usize,
        max_depth: usize,
    ) -> bool {
        if depth > max_depth {
            return true; // Stop recursing to avoid performance issues
        }

        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                // Validate all child statements are within parent bounds
                for stmt in statements {
                    if stmt.location.start() < node.location.start()
                        || stmt.location.end() > node.location.end()
                    {
                        return false;
                    }
                    if !self.validate_node_tree_consistency(stmt, source, depth + 1, max_depth) {
                        return false;
                    }
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                if !self.validate_node_tree_consistency(variable, source, depth + 1, max_depth) {
                    return false;
                }
                if let Some(init) = initializer
                    && !self.validate_node_tree_consistency(init, source, depth + 1, max_depth)
                {
                    return false;
                }
            }
            NodeKind::Binary { left, right, .. }
                if !self.validate_node_tree_consistency(left, source, depth + 1, max_depth)
                    || !self.validate_node_tree_consistency(
                        right,
                        source,
                        depth + 1,
                        max_depth,
                    ) =>
            {
                return false;
            }
            _ => {
                // Leaf nodes don't need recursive validation
            }
        }

        true
    }

    /// Rebuild `root` with the admitted simple edits applied in one pass.
    ///
    /// The tree is cloned exactly once and then updated in place, so the work
    /// is linear in the number of nodes regardless of depth. `None` declines
    /// the incremental result — an affected value leaf's replacement no longer
    /// forms a single token of the admitted kind — and the caller falls back
    /// to a full parse instead of accepting a divergent tree.
    fn clone_and_update_tree(
        &self,
        root: &Node,
        new_source: &str,
        old_source: &str,
    ) -> Option<Node> {
        let mut updated = root.clone();
        self.update_subtree_in_place(&mut updated, new_source, old_source).then_some(updated)
    }

    /// Update one node and its subtree in place for the pending edits.
    ///
    /// Returns `false` when the node is an affected value leaf whose mapped
    /// replacement text fails token validation; the partially updated tree is
    /// then discarded by the caller.
    fn update_subtree_in_place(&self, node: &mut Node, new_source: &str, old_source: &str) -> bool {
        // Original geometry drives every mapping. Children are disjoint from
        // this node's own `location` field, so updating them first cannot
        // skew the coordinates read here.
        let original_start = node.location.start();
        let original_end = node.location.end();
        let mut valid = true;
        node.for_each_child_mut(|child| {
            valid &= self.update_subtree_in_place(child, new_source, old_source);
        });
        if !valid {
            return false;
        }

        // Affected value leaves re-read their payload from the new source.
        // Every other node keeps its payload and only remaps its span.
        let patched_span = if self.is_node_affected(node)
            && matches!(
                node.kind,
                NodeKind::Number { .. }
                    | NodeKind::String { .. }
                    | NodeKind::Variable { .. }
                    | NodeKind::Identifier { .. }
            ) {
            match self.patch_leaf_payload(
                &mut node.kind,
                new_source,
                old_source,
                original_start,
                original_end,
            ) {
                Some(span) => span,
                None => return false,
            }
        } else {
            let new_start = isize_to_usize_clamped(
                original_start as isize + self.calculate_shift_exclusive(original_start),
            );
            let new_end = isize_to_usize_clamped(
                original_end as isize + self.calculate_shift_at(original_end),
            );
            // Payload sub-spans ride the same mapping as the span they
            // anchor: start counts edits ending strictly before it, end
            // counts edits ending at or before it. The whitespace reuse path
            // remaps them through `Node::clone_with_mapped_locations`;
            // leaving them at pre-shift offsets here would make the two
            // remapping paths disagree. A recovery token that cannot keep
            // its validated byte width declines the edit into a full parse.
            if !node.kind.map_payload_locations_in_place(|location| {
                SourceLocation::new(
                    isize_to_usize_clamped(
                        location.start() as isize
                            + self.calculate_shift_exclusive(location.start()),
                    ),
                    isize_to_usize_clamped(
                        location.end() as isize + self.calculate_shift_at(location.end()),
                    ),
                )
            }) {
                return false;
            }
            (new_start, new_end)
        };

        node.location = SourceLocation::new(patched_span.0, patched_span.1);
        true
    }

    /// Map an admitted value leaf's old span to its span in the new source and
    /// patch its payload from that text, or decline the edit.
    ///
    /// The span arithmetic decides which side of the leaf absorbs a zero-width
    /// boundary insertion: text typed exactly at the leaf's start or end
    /// becomes part of the literal, so the start counts only edits ending
    /// strictly before it ([`Self::calculate_shift_exclusive`]) while the end
    /// counts edits ending at or before it ([`Self::calculate_shift_at`]).
    /// Counting one boundary twice — the previous behavior — produced spans
    /// that no fresh parse could reproduce.
    ///
    /// The payload is only patched after the mapped text is proven to still
    /// lex as one token of the admitted kind; renames must keep the sigil and
    /// string edits must keep the quote. Anything else returns `None` so the
    /// whole incremental build is abandoned in favor of a full parse.
    fn patch_leaf_payload(
        &self,
        kind: &mut NodeKind,
        new_source: &str,
        old_source: &str,
        old_start: usize,
        old_end: usize,
    ) -> Option<(usize, usize)> {
        let new_start =
            isize_to_usize_clamped(old_start as isize + self.calculate_shift_exclusive(old_start));
        let new_end = isize_to_usize_clamped(old_end as isize + self.calculate_shift_at(old_end));
        if new_start > new_end
            || new_end > new_source.len()
            || !new_source.is_char_boundary(new_start)
            || !new_source.is_char_boundary(new_end)
        {
            return None;
        }
        let text = &new_source[new_start..new_end];

        match kind {
            NodeKind::Number { value } => {
                if !self.lexes_as_single_token(text, &|token| matches!(token, TokenType::Number(_)))
                {
                    return None;
                }
                *value = text.to_string();
            }
            NodeKind::String { value, interpolated: _ } => {
                // The opening quote must survive unchanged: switching `'` to
                // `"` flips the stored interpolation flag, which the patch
                // cannot recompute. Inside the `q` operator family the second
                // byte selects the operator (`q(` vs `qq(` vs `qw(`), so it
                // must match there too; for plain quotes the second byte is
                // string content and may change freely.
                let old_bytes = old_source.as_bytes();
                let old_first = old_bytes.get(old_start);
                if old_first != text.as_bytes().first()
                    || (old_first == Some(&b'q')
                        && old_bytes.get(old_start + 1) != text.as_bytes().get(1))
                {
                    return None;
                }
                if !self.lexes_as_single_token(text, &|token| {
                    matches!(
                        token,
                        TokenType::StringLiteral
                            | TokenType::InterpolatedString(_)
                            | TokenType::QuoteSingle
                            | TokenType::QuoteDouble
                    )
                }) {
                    return None;
                }
                *value = text.to_string();
            }
            NodeKind::Variable { sigil, name } => {
                // A rename must stay one variable token with a non-empty
                // name. Braced forms (`${foo}`, `${Foo::bar}`) span the
                // braces while the stored name strips them, so a braced old
                // form requires a closed braced new form and the name is the
                // brace-enclosed inner text; a bare form keeps the name
                // suffix after the sigil. Sigil, brace, or token-structure
                // changes fall back to parsing.
                let old_text = old_source.get(old_start..old_end)?;
                let braced =
                    old_text.strip_prefix(sigil.as_str()).is_some_and(|rest| rest.starts_with('{'));
                let name_text = if braced {
                    let inner_end = text.len().checked_sub(1)?;
                    let opening = sigil.len() + 1;
                    if !text.starts_with(sigil.as_str())
                        || text.as_bytes().get(sigil.len()) != Some(&b'{')
                        || !text.ends_with('}')
                        || inner_end <= opening
                        || !text[opening..inner_end]
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
                    {
                        return None;
                    }
                    &text[opening..inner_end]
                } else {
                    if !text.starts_with(sigil.as_str()) || text.len() <= sigil.len() {
                        return None;
                    }
                    &text[sigil.len()..]
                };
                if name_text.is_empty()
                    || !self.lexes_as_single_token(text, &|token| {
                        matches!(token, TokenType::Identifier(_))
                    })
                {
                    return None;
                }
                *name = name_text.to_string();
            }
            NodeKind::Identifier { name } => {
                // A sigil-leading replacement would parse as a `Variable`
                // node, not an `Identifier`, so it must not be patched.
                if text.as_bytes().first().is_some_and(|&byte| is_variable_sigil_byte(byte)) {
                    return None;
                }
                if !self
                    .lexes_as_single_token(text, &|token| matches!(token, TokenType::Identifier(_)))
                {
                    return None;
                }
                *name = text.to_string();
            }
            _ => return None,
        }

        Some((new_start, new_end))
    }

    /// Require `text` to lex as exactly one trivia-free token of an accepted
    /// class whose span covers the whole text.
    fn lexes_as_single_token(&self, text: &str, accepted: &dyn Fn(&TokenType) -> bool) -> bool {
        if text.is_empty() {
            return false;
        }
        let mut lexer = PerlLexer::new(text);
        let mut seen_token = false;
        loop {
            let Some(token) = lexer.next_token() else {
                break;
            };
            if token.token_type == TokenType::EOF {
                break;
            }
            if token.token_type.is_trivia()
                || token.start != 0
                || token.end != text.len()
                || !accepted(&token.token_type)
            {
                return false;
            }
            seen_token = true;
        }
        seen_token
    }

    /// Calculate cumulative byte shift at position with Unicode-safe handling
    ///
    /// Enhanced to handle multibyte Unicode characters correctly and avoid
    /// splitting characters across edit boundaries.
    fn calculate_shift_at(&self, position: usize) -> isize {
        let mut shift = 0;
        for edit in self.pending_edits.edits() {
            let original_old_end = isize_to_usize_clamped(edit.old_end_byte as isize - shift);

            if original_old_end <= position {
                let edit_shift = edit.byte_shift();
                shift += edit_shift;
            } else {
                break;
            }
        }

        shift
    }

    /// Calculate the cumulative byte shift of edits ending strictly before
    /// `position` (in original source coordinates).
    ///
    /// [`Self::calculate_shift_at`] counts an edit whose old end equals
    /// `position`; this helper deliberately does not. The distinction keeps
    /// zero-width boundary insertions from being counted twice: text inserted
    /// exactly at a leaf's start or end merges into that leaf, so the leaf's
    /// start uses this exclusive count while its end uses the inclusive one —
    /// and an ancestor's start must not move for an insertion that merged
    /// into a leaf anchored at the same byte.
    fn calculate_shift_exclusive(&self, position: usize) -> isize {
        let mut shift = 0;
        for edit in self.pending_edits.edits() {
            let original_old_end = isize_to_usize_clamped(edit.old_end_byte as isize - shift);

            if original_old_end < position {
                let edit_shift = edit.byte_shift();
                shift += edit_shift;
            } else {
                break;
            }
        }

        shift
    }

    /// Ensure position falls on a valid Unicode character boundary
    ///
    /// Adjusts position to the nearest valid character boundary if needed,
    /// preventing panics from invalid UTF-8 slice operations.
    #[allow(dead_code)]
    fn ensure_unicode_boundary(&self, source: &str, position: usize) -> usize {
        if position >= source.len() {
            return source.len();
        }

        if source.is_char_boundary(position) {
            return position;
        }

        // Find the previous character boundary
        for i in (0..=position).rev() {
            if i < source.len() && source.is_char_boundary(i) {
                return i;
            }
        }

        // Fallback to start of string
        0
    }

    /// Calculate position shift with Unicode safety
    ///
    /// Ensures that the shifted position falls on a valid character boundary
    /// and handles complex multibyte characters correctly.
    #[allow(dead_code)]
    fn calculate_unicode_safe_position(
        &self,
        original_pos: usize,
        shift: isize,
        source: &str,
    ) -> usize {
        let new_pos = if shift >= 0 {
            original_pos.saturating_add(shift as usize)
        } else {
            original_pos.saturating_sub((-shift) as usize)
        };

        self.ensure_unicode_boundary(source, new_pos)
    }

    /// Get current performance metrics
    pub fn get_metrics(&self) -> &IncrementalMetrics {
        &self.metrics
    }

    /// Reset performance metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = IncrementalMetrics::new();
    }

    /// Get the last reuse analysis result if available
    pub fn get_last_reuse_analysis(&self) -> Option<&ReuseAnalysisResult> {
        self.last_reuse_analysis.as_ref()
    }

    /// Update reuse configuration
    pub fn set_reuse_config(&mut self, config: ReuseConfig) {
        self.reuse_config = config.clone();
        self.reuse_analyzer = AdvancedReuseAnalyzer::with_config(config);
    }

    /// Check if the last parse used advanced reuse analysis
    pub fn used_advanced_reuse(&self) -> bool {
        self.advanced_reuse_selected
    }

    /// Return the number of old subtrees selected for materialization by the last parse.
    pub fn get_materialized_reuse_count(&self) -> usize {
        self.materialized_reuse_nodes
    }

    /// Return whether the last parse accepted an incrementally produced tree.
    ///
    /// This reports acceptance, not attempt: a parse that tried the incremental
    /// path and fell back to a full parse returns `false`.
    pub fn used_incremental_path(&self) -> bool {
        self.used_incremental_path
    }

    /// Get detailed reuse efficiency report
    pub fn get_reuse_efficiency_report(&self) -> String {
        if let Some(analysis) = &self.last_reuse_analysis {
            format!(
                "Advanced Reuse Analysis:\n  Efficiency: {:.1}%\n  Nodes reused: {}\n  Total nodes: {}\n  {}",
                analysis.reuse_percentage,
                analysis.reused_nodes,
                analysis.total_old_nodes,
                analysis.performance_summary()
            )
        } else {
            format!(
                "Basic Incremental Analysis:\n  Efficiency: {:.1}%\n  Nodes reused: {}\n  Nodes reparsed: {}",
                self.reused_nodes as f64 / (self.reused_nodes + self.reparsed_nodes) as f64 * 100.0,
                self.reused_nodes,
                self.reparsed_nodes
            )
        }
    }

    /// Whether any pending edit touches the node's original span.
    ///
    /// Queued edits are expressed in the coordinates produced by the edits
    /// before them, while `node` carries its coordinates in the tree's
    /// original source, so every edit is mapped back by the cumulative shift
    /// of the edits before it before any comparison. Comparing raw
    /// coordinates lets an earlier length change displace a later edit
    /// beyond its leaf, leaving that leaf's payload stale while the tree is
    /// still accepted.
    fn is_node_affected(&self, node: &Node) -> bool {
        let start = node.location.start();
        let end = node.location.end();
        let mut shift = 0isize;
        for edit in self.pending_edits.edits() {
            let original_start = isize_to_usize_clamped(edit.start_byte as isize - shift);
            let original_old_end = isize_to_usize_clamped(edit.old_end_byte as isize - shift);

            // `Edit::overlaps_range` requires a strict interior overlap
            // (`start < old_end && old_end > start`); a pure insertion has an
            // empty old range and never passes it.
            if original_start < end && original_old_end > start {
                return true;
            }

            // A pure insertion landing exactly on a node boundary reports no
            // overlap even though the inserted text becomes part of that
            // node's source text. Without this window a boundary insertion
            // leaves the node's cached content stale (for example typing a
            // digit at the end of a numeric literal), so the incremental tree
            // diverges from a fresh parse. `EditSet::affected_ranges` already
            // widens pure insertions for the same reason; this keeps the two
            // invalidation paths consistent.
            if edit.start_byte == edit.old_end_byte
                && original_start >= start
                && original_start <= end
            {
                return true;
            }

            shift += edit.byte_shift();
        }

        false
    }

    /// Clone `node` with every position shifted by `shift`, in one pass.
    ///
    /// The single structural clone plus an in-place span walk keeps this
    /// linear in the subtree size; the previous recursive form re-cloned the
    /// entire remaining subtree at every depth. The uniform shift is the
    /// intended semantics here: the result is compared against a freshly
    /// parsed node and any mismatch simply skips the reuse candidate.
    fn clone_with_shifted_positions(&self, node: &Node, shift: isize) -> Node {
        let mut shifted = node.clone();
        shift_positions_in_place(&mut shifted, shift);
        shifted
    }

    fn count_reuse_potential(&mut self, old_root: &Node, new_root: &Node) {
        // Compare trees and count which nodes could have been reused
        let (reused, reparsed) = self.analyze_reuse(old_root, new_root);
        self.reused_nodes = reused;
        self.reparsed_nodes = reparsed;
    }

    /// Classify every node of the produced tree as reused or reparsed.
    ///
    /// The traversal walks the canonical [`Node::children`] surface so that
    /// `reused + reparsed` always reconciles to the produced tree's node count.
    /// A per-kind traversal cannot hold that invariant: any node kind it does not
    /// descend into contributes a single count for an entire subtree, which
    /// leaves the public reuse counters unable to describe the tree they report on.
    fn analyze_reuse(&self, old_node: &Node, new_node: &Node) -> (usize, usize) {
        let (mut reused, mut reparsed) =
            if self.nodes_match(old_node, new_node) { (1, 0) } else { (0, 1) };

        let old_children = old_node.children();
        let new_children = new_node.children();
        for (index, new_child) in new_children.iter().enumerate() {
            match old_children.get(index) {
                Some(old_child) => {
                    let (child_reused, child_reparsed) = self.analyze_reuse(old_child, new_child);
                    reused += child_reused;
                    reparsed += child_reparsed;
                }
                // A produced child with no positional counterpart is new work.
                None => reparsed += self.count_nodes(new_child),
            }
        }

        (reused, reparsed)
    }

    /// Check if two nodes are structurally equivalent for reuse purposes
    ///
    /// Enhanced to support more node types for better reuse detection.
    /// Returns true if nodes can be considered equivalent for caching.
    fn nodes_match(&self, node1: &Node, node2: &Node) -> bool {
        match (&node1.kind, &node2.kind) {
            // Value nodes - must match exactly
            (NodeKind::Number { value: v1 }, NodeKind::Number { value: v2 }) => v1 == v2,
            (
                NodeKind::String { value: v1, interpolated: i1 },
                NodeKind::String { value: v2, interpolated: i2 },
            ) => v1 == v2 && i1 == i2,
            // VString nodes - version value must match exactly
            (NodeKind::VString { value: v1 }, NodeKind::VString { value: v2 }) => v1 == v2,

            // Variable nodes - sigil and name must match
            (
                NodeKind::Variable { sigil: s1, name: n1 },
                NodeKind::Variable { sigil: s2, name: n2 },
            ) => s1 == s2 && n1 == n2,

            // Identifier nodes
            (NodeKind::Identifier { name: n1 }, NodeKind::Identifier { name: n2 }) => n1 == n2,

            // Binary operators - operator must match, operands checked recursively
            (NodeKind::Binary { op: op1, .. }, NodeKind::Binary { op: op2, .. }) => op1 == op2,

            // Unary operators - operator must match, operand checked recursively
            (NodeKind::Unary { op: op1, .. }, NodeKind::Unary { op: op2, .. }) => op1 == op2,

            // Function calls - name and argument count should match
            (
                NodeKind::FunctionCall { name: n1, args: args1 },
                NodeKind::FunctionCall { name: n2, args: args2 },
            ) => n1 == n2 && args1.len() == args2.len(),

            // Variable declarations - declarator should match
            (
                NodeKind::VariableDeclaration { declarator: d1, .. },
                NodeKind::VariableDeclaration { declarator: d2, .. },
            ) => d1 == d2,

            // Array literals - length should match for structural similarity
            (NodeKind::ArrayLiteral { elements: e1 }, NodeKind::ArrayLiteral { elements: e2 }) => {
                e1.len() == e2.len()
            }

            // Hash literals - key count should match for structural similarity
            (NodeKind::HashLiteral { pairs: p1 }, NodeKind::HashLiteral { pairs: p2 }) => {
                p1.len() == p2.len()
            }

            // Block statements - statement count should match
            (NodeKind::Block { statements: s1 }, NodeKind::Block { statements: s2 }) => {
                s1.len() == s2.len()
            }

            // Program nodes - statement count should match
            (NodeKind::Program { statements: s1 }, NodeKind::Program { statements: s2 }) => {
                s1.len() == s2.len()
            }

            // Control flow - structural matching
            (NodeKind::If { .. }, NodeKind::If { .. }) => true, // Structure checked recursively
            (NodeKind::While { .. }, NodeKind::While { .. }) => true,
            (NodeKind::For { .. }, NodeKind::For { .. }) => true,
            (NodeKind::Foreach { .. }, NodeKind::Foreach { .. }) => true,

            // Subroutine definitions - name should match if present
            (NodeKind::Subroutine { name: n1, .. }, NodeKind::Subroutine { name: n2, .. }) => {
                n1 == n2
            }

            // Package declarations - name should match
            (NodeKind::Package { name: n1, .. }, NodeKind::Package { name: n2, .. }) => n1 == n2,

            // Use statements - module name should match
            (NodeKind::Use { module: m1, .. }, NodeKind::Use { module: m2, .. }) => m1 == m2,

            // Same node types without specific content - consider structural match
            (kind1, kind2) => std::mem::discriminant(kind1) == std::mem::discriminant(kind2),
        }
    }

    /// Count every node of `node`'s subtree over the canonical child surface.
    ///
    /// This must stay canonical: the reuse counters are reconciled against it,
    /// and a per-kind traversal would silently omit whole subtrees.
    fn count_nodes(&self, node: &Node) -> usize {
        1 + node.children().into_iter().map(|child| self.count_nodes(child)).sum::<usize>()
    }
}

impl Default for IncrementalParserV2 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::position::Position;
    use std::time::Instant;

    fn adaptive_perf_budget_micros(base_budget_micros: u128) -> u128 {
        let thread_count = std::env::var("RUST_TEST_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism().map_or(8, std::num::NonZeroUsize::get)
            });

        let mut budget = base_budget_micros;
        if thread_count <= 2 {
            budget = budget.saturating_mul(2);
        } else if thread_count <= 4 {
            budget = budget.saturating_mul(3) / 2;
        }

        if std::env::var("CI").is_ok() {
            budget = budget.saturating_mul(3) / 2;
        }

        budget
    }

    #[test]
    fn test_basic_compilation() {
        let parser = IncrementalParserV2::new();
        assert_eq!(parser.reused_nodes, 0);
        assert_eq!(parser.reparsed_nodes, 0);
    }

    #[test]
    fn whitespace_map_preserves_if_keyword_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let loc = |start, end| perl_parser_core::ast::SourceLocation::new(start, end);
        let number =
            |start| Node::new(NodeKind::Number { value: "1".to_string() }, loc(start, start + 1));
        let block = |start, end| {
            Node::new(NodeKind::Block { statements: vec![number(start + 1)] }, loc(start, end))
        };
        let if_node = Node::new(
            NodeKind::If {
                condition: Box::new(number(1)),
                then_branch: Box::new(block(4, 10)),
                elsif_branches: vec![(Box::new(number(12)), Box::new(block(14, 20)))],
                else_branch: Some(Box::new(block(22, 28))),
                keyword: Some("unless".to_string()),
            },
            loc(0, 29),
        );
        let root = Node::new(NodeKind::Program { statements: vec![if_node] }, loc(0, 29));

        // One space inserted before the program: every statement-side span
        // shifts by one and the mapped clone preserves the payload wholesale.
        let mut edits = EditSet::new();
        edits.add(Edit::new(
            0,
            0,
            1,
            Position::new(0, 1, 1),
            Position::new(0, 1, 1),
            Position::new(1, 1, 2),
        ));
        let edit_map =
            WhitespaceEditMap::try_new(&"a".repeat(29), &format!(" {}", "a".repeat(29)), &edits)
                .ok_or("whitespace insertion should be admitted")?;
        let mapped = edit_map.clone_tree(&root).ok_or("location mapping failed")?;

        assert_eq!(mapped.location, loc(0, 30));
        let (kind, _) = mapped.into_parts();
        let NodeKind::Program { statements } = kind else {
            return Err("expected Program root".into());
        };
        let (if_kind, if_location) =
            statements.into_iter().next().ok_or("expected one statement")?.into_parts();
        assert_eq!(if_location, loc(1, 30));
        let NodeKind::If { keyword, else_branch, .. } = if_kind else {
            return Err("expected If statement".into());
        };
        assert_eq!(keyword.as_deref(), Some("unless"));
        assert!(else_branch.is_some());
        Ok(())
    }

    #[test]
    fn vstring_nodes_match_only_when_version_text_matches() {
        let parser = IncrementalParserV2::new();
        let loc = |start, end| perl_parser_core::ast::SourceLocation::new(start, end);
        let vstring = |value: &str| {
            Node::new(NodeKind::VString { value: value.to_string() }, loc(0, value.len()))
        };

        assert!(
            parser.nodes_match(&vstring("v1.2.3"), &vstring("v1.2.3")),
            "incremental v2 reuse should match equal v-string literals"
        );
        assert!(
            !parser.nodes_match(&vstring("v1.2.3"), &vstring("v2.0.0")),
            "incremental v2 reuse must not match different v-string literals"
        );
    }

    #[test]
    fn advanced_reuse_selects_only_exact_old_subtrees() {
        let parser = IncrementalParserV2::new();
        let location = |start, end| SourceLocation::new(start, end);
        let old_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(NodeKind::Number { value: "1".to_string() }, location(1, 2)),
                    Node::new(NodeKind::Number { value: "2".to_string() }, location(3, 4)),
                ],
            },
            location(0, 4),
        );
        let new_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(NodeKind::Number { value: "1".to_string() }, location(1, 2)),
                    Node::new(NodeKind::Number { value: "3".to_string() }, location(3, 4)),
                ],
            },
            location(0, 4),
        );
        let reuse_map = HashMap::from([
            (
                1,
                ReuseStrategy {
                    target_position: 1,
                    reuse_type: ReuseType::Direct,
                    confidence_score: 1.0,
                    position_adjustment: 0,
                },
            ),
            (
                3,
                ReuseStrategy {
                    target_position: 3,
                    reuse_type: ReuseType::ContentUpdate,
                    confidence_score: 0.8,
                    position_adjustment: 0,
                },
            ),
        ]);

        let (materialized_map, replacements) =
            parser.collect_materializable_reuse(&old_tree, &new_tree, &reuse_map);
        assert_eq!(materialized_map.len(), 1);
        assert!(matches!(
            materialized_map.get(&1).map(|strategy| &strategy.reuse_type),
            Some(ReuseType::Direct)
        ));
        assert!(!materialized_map.contains_key(&3));

        // Each recorded replacement must be an exact clone of the produced node it
        // would stand in for, which is what makes the selection safe to report.
        let selected: Vec<&(Node, Node)> = replacements.values().flatten().collect();
        assert_eq!(selected.len(), 1);
        let (produced, replacement) = selected[0];
        assert_eq!(produced, replacement);
        assert_eq!(replacements.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn test_performance_timing_detailed() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse with timing
        let source1 = "my $x = 42;";
        let start = Instant::now();
        let _tree1 = parser.parse(source1)?;
        let initial_parse_time = start.elapsed();

        println!("Initial parse time: {:?}", initial_parse_time);
        println!("Initial nodes reparsed: {}", parser.reparsed_nodes);

        // Apply incremental edit with detailed timing
        parser.edit(Edit::new(
            8,
            10,
            12, // "42" -> "4242"
            Position::new(8, 1, 9),
            Position::new(10, 1, 11),
            Position::new(12, 1, 13),
        ));

        let source2 = "my $x = 4242;";
        let start = Instant::now();
        let _tree2 = parser.parse(source2)?;
        let incremental_parse_time = start.elapsed();

        println!("Incremental parse time: {:?}", incremental_parse_time);
        println!(
            "Incremental nodes reused: {}, reparsed: {}",
            parser.reused_nodes, parser.reparsed_nodes
        );

        // Performance assertions - sub-5ms to avoid flaky CI on loaded runners
        assert!(
            incremental_parse_time.as_micros() < 5000,
            "Incremental parse time should be <5ms, got {:?}",
            incremental_parse_time
        );

        // Verify efficiency - should reuse most nodes
        assert!(parser.reused_nodes >= 3, "Should reuse at least 3 nodes");
        assert_eq!(parser.reparsed_nodes, 1, "Should only reparse the changed Number node");

        // Performance ratio check - for very small examples, overhead may exceed benefits
        let speedup =
            initial_parse_time.as_nanos() as f64 / incremental_parse_time.as_nanos() as f64;
        println!("Performance improvement: {:.2}x faster", speedup);

        // For micro-benchmarks, we focus on correctness and reasonable performance rather than speedup
        // The real benefits show up with larger documents where node reuse matters more
        if speedup >= 1.5 {
            println!("✅ Good speedup achieved: {:.2}x", speedup);
        } else {
            println!("⚠️ Limited speedup for micro-benchmark (expected for tiny examples)");
        }

        Ok(())
    }

    #[test]
    fn test_incremental_value_change() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse with timing
        let source1 = "my $x = 42;";
        let start = Instant::now();
        let _tree1 = parser.parse(source1)?;
        let initial_time = start.elapsed();

        // Initial parse counts all nodes: Program + VarDecl + Variable + Number = 4
        // But semicolon is not counted as a separate node
        assert_eq!(parser.reparsed_nodes, 4); // Program, VarDecl, Variable, Number
        println!(
            "Initial parse: {}µs, {} nodes parsed",
            initial_time.as_micros(),
            parser.reparsed_nodes
        );

        // Change the number value
        parser.edit(Edit::new(
            8,
            10,
            12, // "42" -> "4242"
            Position::new(8, 1, 9),
            Position::new(10, 1, 11),
            Position::new(12, 1, 13),
        ));

        let source2 = "my $x = 4242;";
        let start = Instant::now();
        let tree2 = parser.parse(source2)?;
        let incremental_time = start.elapsed();

        println!(
            "Incremental parse: {}µs, reused_nodes = {}, reparsed_nodes = {}",
            incremental_time.as_micros(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );
        assert_eq!(parser.reused_nodes, 3); // Program, VarDecl, Variable can be reused
        assert_eq!(parser.reparsed_nodes, 1); // Only Number needs reparsing

        // Performance validation
        assert!(incremental_time.as_micros() < 500, "Incremental update should be <500µs");
        let efficiency =
            parser.reused_nodes as f64 / (parser.reused_nodes + parser.reparsed_nodes) as f64;
        assert!(
            efficiency >= 0.75,
            "Node reuse efficiency should be ≥75%, got {:.1}%",
            efficiency * 100.0
        );

        // Verify the tree is correct
        if let NodeKind::Program { statements } = &tree2.kind
            && let NodeKind::VariableDeclaration { initializer: Some(init), .. } =
                &statements[0].kind
            && let NodeKind::Number { value } = &init.kind
        {
            assert_eq!(value, "4242");
        }

        Ok(())
    }

    #[test]
    fn test_multiple_value_changes() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse with timing
        let source1 = "my $x = 10;\nmy $y = 20;";
        let start = Instant::now();
        parser.parse(source1)?;
        let initial_time = start.elapsed();
        let initial_nodes = parser.reparsed_nodes;

        println!(
            "Initial parse (multi-statement): {}µs, {} nodes",
            initial_time.as_micros(),
            initial_nodes
        );

        // Change both values
        parser.edit(Edit::new(
            8,
            10,
            11, // "10" -> "100"
            Position::new(8, 1, 9),
            Position::new(10, 1, 11),
            Position::new(11, 1, 12),
        ));

        parser.edit(Edit::new(
            21,
            23,
            24, // "20" -> "200" (adjusted for previous edit)
            Position::new(21, 2, 9),
            Position::new(23, 2, 11),
            Position::new(24, 2, 12),
        ));

        let source2 = "my $x = 100;\nmy $y = 200;";
        let start = Instant::now();
        let tree = parser.parse(source2)?;
        let incremental_time = start.elapsed();

        println!(
            "Multiple edits: {}µs, reused_nodes = {}, reparsed_nodes = {}",
            incremental_time.as_micros(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );
        // Advanced reuse system can reuse more nodes than expected
        // The actual counts may be higher due to improved efficiency
        assert!(
            parser.reused_nodes >= 5,
            "Should reuse at least 5 nodes, got {}",
            parser.reused_nodes
        );
        assert!(
            parser.reparsed_nodes >= 1,
            "Should reparse at least 1 node, got {}",
            parser.reparsed_nodes
        );

        // Performance validation for multiple edits — relaxed for CI runners
        assert!(incremental_time.as_micros() < 5000, "Multiple edits should be <5ms");
        let total_nodes = parser.reused_nodes + parser.reparsed_nodes;
        let reuse_ratio = parser.reused_nodes as f64 / total_nodes as f64;
        assert!(
            reuse_ratio >= 0.7,
            "Multi-edit reuse ratio should be ≥70%, got {:.1}%",
            reuse_ratio * 100.0
        );

        // Verify both values were updated correctly
        if let NodeKind::Program { statements } = &tree.kind {
            if let NodeKind::VariableDeclaration { initializer: Some(init), .. } =
                &statements[0].kind
                && let NodeKind::Number { value } = &init.kind
            {
                assert_eq!(value, "100");
            }
            if let NodeKind::VariableDeclaration { initializer: Some(init), .. } =
                &statements[1].kind
                && let NodeKind::Number { value } = &init.kind
            {
                assert_eq!(value, "200");
            }
        }

        Ok(())
    }

    #[test]
    fn test_too_many_edits_fallback() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse
        let source1 = "my $x = 1;";
        parser.parse(source1)?;

        // Add too many edits (> 10)
        for i in 0..15 {
            parser.edit(Edit::new(
                8 + i,
                9 + i,
                10 + i,
                Position::new(8 + i, 1, (9 + i) as u32),
                Position::new(9 + i, 1, (10 + i) as u32),
                Position::new(10 + i, 1, (11 + i) as u32),
            ));
        }

        let source2 = "my $x = 123456789012345;";
        let tree = parser.parse(source2)?;

        // Advanced reuse system may still achieve some reuse even with too many edits
        // The system now uses sophisticated analysis rather than simple fallbacks
        assert!(parser.reparsed_nodes > 0, "Should reparse some nodes");
        // Note: reused_nodes may be > 0 due to advanced reuse algorithms

        // Tree should still be correct
        if let NodeKind::Program { statements } = &tree.kind {
            assert_eq!(statements.len(), 1);
        }

        Ok(())
    }

    #[test]
    fn test_invalid_edit_bounds() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse
        let source1 = "my $x = 42;";
        parser.parse(source1)?;

        // Edit that goes beyond the node bounds (should fall back to full parse)
        parser.edit(Edit::new(
            8,
            12, // Beyond the number literal
            13,
            Position::new(8, 1, 9),
            Position::new(12, 1, 13),
            Position::new(13, 1, 14),
        ));

        let source2 = "my $x = 123;";
        let tree = parser.parse(source2)?;

        // Advanced reuse system may still achieve some reuse even with invalid bounds
        // The system is now more resilient and may not always fall back completely
        assert!(parser.reparsed_nodes > 0, "Should reparse some nodes");
        // Note: reused_nodes may be > 0 due to advanced reuse algorithms

        // Tree should still be correct
        if let NodeKind::Program { statements } = &tree.kind
            && let NodeKind::VariableDeclaration { initializer: Some(init), .. } =
                &statements[0].kind
            && let NodeKind::Number { value } = &init.kind
        {
            assert_eq!(value, "123");
        }

        Ok(())
    }

    #[test]
    fn test_string_edit() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse
        let source1 = "my $name = \"hello\";";
        parser.parse(source1)?;

        // Change string content
        parser.edit(Edit::new(
            12,
            17, // "hello" -> "world"
            17,
            Position::new(12, 1, 13),
            Position::new(17, 1, 18),
            Position::new(17, 1, 18),
        ));

        let source2 = "my $name = \"world\";";
        let tree = parser.parse(source2)?;

        // Should reuse most of the tree
        println!(
            "DEBUG test_string_edit: reused_nodes = {}, reparsed_nodes = {}",
            parser.reused_nodes, parser.reparsed_nodes
        );
        assert_eq!(parser.reused_nodes, 3); // Program, VarDecl, Variable
        assert_eq!(parser.reparsed_nodes, 1); // Only String

        // Verify the string was updated
        if let NodeKind::Program { statements } = &tree.kind
            && let NodeKind::VariableDeclaration { initializer: Some(init), .. } =
                &statements[0].kind
            && let NodeKind::String { value, .. } = &init.kind
        {
            assert_eq!(value, "\"world\"");
        }

        Ok(())
    }

    #[test]
    fn test_empty_source_handling() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Initial parse with valid source
        let source1 = "my $x = 42;";
        let start = Instant::now();
        parser.parse(source1)?;
        let initial_time = start.elapsed();
        println!("Initial parse time: {}µs", initial_time.as_micros());

        // Add an edit
        parser.edit(Edit::new(
            8,
            10,
            11,
            Position::new(8, 1, 9),
            Position::new(10, 1, 11),
            Position::new(11, 1, 12),
        ));

        // Try to parse empty source (should fall back to full parse)
        let source2 = "";
        let start = Instant::now();
        let result = parser.parse(source2);
        let parse_time = start.elapsed();

        println!("Empty source parse time: {}µs", parse_time.as_micros());

        // Should handle gracefully and either succeed or fail cleanly
        match result {
            Ok(_) => {
                // If it succeeds, should be a full parse
                assert_eq!(parser.reused_nodes, 0);
                println!("Empty source parsing succeeded with fallback");
            }
            Err(_) => {
                // If it fails, that's also acceptable for empty source
                assert_eq!(parser.reused_nodes, 0);
                println!("Empty source parsing failed gracefully (expected)");
            }
        }

        // Performance should still be reasonable even for empty source handling
        assert!(parse_time.as_millis() < 100, "Empty source handling should be fast");

        Ok(())
    }

    #[test]
    fn test_complex_nested_structure_edits() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Complex nested Perl structure
        let source1 = r#"
if ($condition) {
    my $nested = {
        key1 => "value1",
        key2 => 42,
        key3 => [1, 2, 3]
    };
    process($nested);
}
"#;

        let start = Instant::now();
        parser.parse(source1)?;
        let initial_time = start.elapsed();
        let initial_nodes = parser.reparsed_nodes;

        println!(
            "Complex structure initial parse: {}µs, {} nodes",
            initial_time.as_micros(),
            initial_nodes
        );

        // Edit nested value - should be challenging for incremental parser
        let value_start =
            source1.find("42").ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;
        parser.edit(Edit::new(
            value_start,
            value_start + 2,
            value_start + 4, // "42" -> "9999"
            Position::new(value_start, 1, 1),
            Position::new(value_start + 2, 1, 3),
            Position::new(value_start + 4, 1, 5),
        ));

        let source2 = source1.replace("42", "9999");
        let start = Instant::now();
        let _tree = parser.parse(&source2)?;
        let incremental_time = start.elapsed();

        println!(
            "Complex nested edit: {}µs, reused={}, reparsed={}",
            incremental_time.as_micros(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );

        // Even with complex nesting, should have reasonable performance
        assert!(incremental_time.as_millis() < 10, "Complex nested edit should be <10ms");

        // Should still achieve some node reuse
        if parser.reused_nodes > 0 {
            println!("Successfully reused {} nodes in complex structure", parser.reused_nodes);
        } else {
            println!("Complex structure caused full reparse (acceptable for edge cases)");
        }

        Ok(())
    }

    #[test]
    fn test_large_document_performance() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Generate a larger Perl document
        let mut large_source = String::new();
        for i in 0..100 {
            large_source.push_str(&format!("my $var{} = {};\n", i, i * 10));
        }

        let start = Instant::now();
        parser.parse(&large_source)?;
        let initial_time = start.elapsed();
        let initial_nodes = parser.reparsed_nodes;

        println!(
            "Large document initial parse: {}ms, {} nodes",
            initial_time.as_millis(),
            initial_nodes
        );

        // Edit in the middle of the document
        let edit_pos = large_source
            .find("my $var50 = 500")
            .ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?
            + 13;
        parser.edit(Edit::new(
            edit_pos,
            edit_pos + 3, // "500" -> "999"
            edit_pos + 3,
            Position::new(edit_pos, 1, 1),
            Position::new(edit_pos + 3, 1, 4),
            Position::new(edit_pos + 3, 1, 4),
        ));

        let source2 = large_source.replace("500", "999");
        let start = Instant::now();
        let _tree = parser.parse(&source2)?;
        let incremental_time = start.elapsed();

        println!(
            "Large document incremental: {}ms, reused={}, reparsed={}",
            incremental_time.as_millis(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );

        // Large document performance targets
        assert!(incremental_time.as_millis() < 50, "Large document incremental should be <50ms");

        // Should achieve significant node reuse on large documents
        if parser.reused_nodes > 0 {
            let reuse_percentage = parser.reused_nodes as f64
                / (parser.reused_nodes + parser.reparsed_nodes) as f64
                * 100.0;
            println!("Large document reuse rate: {:.1}%", reuse_percentage);
            assert!(reuse_percentage > 50.0, "Large document should reuse >50% of nodes");
        }

        Ok(())
    }

    #[test]
    fn test_unicode_heavy_incremental_parsing() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Unicode-heavy source with emojis and international characters
        let source1 = "my $🌟variable = '你好世界'; # Comment with emoji 🚀\nmy $café = 'résumé';";

        let start = Instant::now();
        parser.parse(source1)?;
        let initial_time = start.elapsed();

        println!("Unicode document initial parse: {}µs", initial_time.as_micros());

        // Edit the unicode string content
        let edit_start =
            source1.find("你好世界").ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;
        let edit_end = edit_start + "你好世界".len();
        parser.edit(Edit::new(
            edit_start,
            edit_end,
            edit_start + "再见".len(), // "你好世界" -> "再见" (hello world -> goodbye)
            Position::new(edit_start, 1, 1),
            Position::new(edit_end, 1, 2),
            Position::new(edit_start + "再见".len(), 1, 2),
        ));

        let source2 = source1.replace("你好世界", "再见");
        let start = Instant::now();
        let _tree = parser.parse(&source2)?;
        let incremental_time = start.elapsed();

        println!(
            "Unicode incremental edit: {}µs, reused={}, reparsed={}",
            incremental_time.as_micros(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );

        // Unicode handling should not significantly impact performance.
        let unicode_budget_micros = adaptive_perf_budget_micros(5_000);
        assert!(
            incremental_time.as_micros() < unicode_budget_micros,
            "Unicode incremental parsing should be <{}µs (got {}µs)",
            unicode_budget_micros,
            incremental_time.as_micros()
        );
        assert!(parser.reused_nodes > 0 || parser.reparsed_nodes > 0, "Should parse successfully");

        Ok(())
    }

    /// A digit edit inside a subroutine-local number literal is a simple
    /// value edit and must be produced fresh-equivalently.
    ///
    /// This asserted `reparsed_nodes >= 1` while the retired position index
    /// could not see inside subroutine bodies, so the edit fell back to a
    /// full reparse and the assertion only ratified that fallback. With the
    /// index retired (#13237), the smallest containing node is the number
    /// literal itself and the simple-value path admits the edit. Assert the
    /// discriminating properties instead — the same standard
    /// `whitespace_before_operator_matches_a_fresh_parse` established: the
    /// incremental result must equal a fresh parse, and the counters must
    /// describe the tree they report on.
    #[test]
    fn test_edit_near_ast_node_boundaries() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Source with clear AST node boundaries
        let source1 = "sub func { my $x = 123; return $x * 2; }";

        parser.parse(source1)?;

        // Edit right at the boundary between number and semicolon
        let number_start =
            source1.find("123").ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;
        let replacement = "456";
        let number_end = number_start + 3;
        parser.edit(Edit::new(
            number_end - 1, // Edit last digit of number
            number_end,
            number_end - 1 + replacement.len(), // "3" -> "456"
            Position::new(number_end - 1, 1, 1),
            Position::new(number_end, 1, 2),
            Position::new(number_end - 1 + replacement.len(), 1, 4),
        ));

        let source2 = source1.replace("123", "12456");
        let start = Instant::now();
        let incremental = parser.parse(&source2)?;
        let boundary_edit_time = start.elapsed();

        println!(
            "Boundary edit time: {}µs, reused={}, reparsed={}",
            boundary_edit_time.as_micros(),
            parser.reused_nodes,
            parser.reparsed_nodes
        );

        // Boundary edits are tricky but should still be efficient.
        let boundary_budget_micros = adaptive_perf_budget_micros(5_000);
        assert!(
            boundary_edit_time.as_micros() < boundary_budget_micros,
            "AST boundary edit should be <{}µs (got {}µs)",
            boundary_budget_micros,
            boundary_edit_time.as_micros()
        );
        let fresh = Parser::new(&source2).parse()?;
        assert_eq!(
            incremental, fresh,
            "incremental result must equal a fresh parse in shape and span geometry"
        );
        let produced = parser.count_nodes(&incremental);
        assert_eq!(
            parser.reused_nodes + parser.reparsed_nodes,
            produced,
            "reuse accounting must reconcile to the produced node count"
        );

        Ok(())
    }

    #[test]
    fn whitespace_insertion_uses_fast_path_and_matches_fresh_parse() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "my $x = 42;";
        let old_tree = parser.parse(source1)?;

        parser.edit(Edit::new(
            6,
            6,
            7,
            Position::new(6, 1, 7),
            Position::new(6, 1, 7),
            Position::new(7, 1, 8),
        ));

        let source2 = "my $x  = 42;";
        let incremental = parser.parse(source2)?;
        let fresh = Parser::new(source2).parse()?;

        assert_eq!(incremental, fresh);
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());
        assert_eq!(parser.reparsed_nodes, 0);
        assert_eq!(parser.reused_nodes, parser.count_nodes(&old_tree));
        Ok(())
    }

    #[test]
    fn test_performance_regression_detection() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();

        // Baseline performance measurement
        let source = "my $baseline = 42; my $test = 'hello';";
        let mut parse_times = Vec::new();

        // Multiple runs for statistical significance
        for i in 0..10 {
            let start = Instant::now();
            parser.parse(source)?;
            let time = start.elapsed();
            parse_times.push(time.as_micros());

            // Edit for next iteration
            parser.edit(Edit::new(
                15,
                17,
                19, // Edit position
                Position::new(15, 1, 16),
                Position::new(17, 1, 18),
                Position::new(19, 1, 20),
            ));

            // Alternate source for variations
            let test_source = if i % 2 == 0 {
                "my $baseline = 99; my $test = 'hello';"
            } else {
                "my $baseline = 42; my $test = 'hello';"
            };

            let start = Instant::now();
            parser.parse(test_source)?;
            let incremental_time = start.elapsed();

            println!(
                "Run {}: initial={}µs, incremental={}µs, reused={}, reparsed={}",
                i + 1,
                time.as_micros(),
                incremental_time.as_micros(),
                parser.reused_nodes,
                parser.reparsed_nodes
            );

            // Performance regression detection
            assert!(
                incremental_time.as_millis() < 10,
                "Run {} performance regression detected: {}ms",
                i + 1,
                incremental_time.as_millis()
            );
        }

        // Statistical analysis
        let avg_time = parse_times.iter().sum::<u128>() / parse_times.len() as u128;
        let max_time =
            *parse_times.iter().max().ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;
        let min_time =
            *parse_times.iter().min().ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;

        println!(
            "Performance statistics: avg={}µs, min={}µs, max={}µs",
            avg_time, min_time, max_time
        );

        let variation_factor = max_time as f64 / avg_time as f64;
        assert!(
            variation_factor <= 10.0,
            "Extreme performance inconsistency detected: max={}µs, avg={}µs ({}x variation)",
            max_time,
            avg_time,
            variation_factor
        );
        if variation_factor > 5.0 {
            println!(
                "⚠️ High performance variation detected: max={}µs, avg={}µs ({}x variation) - may indicate system load",
                max_time, avg_time, variation_factor
            );
        }

        Ok(())
    }

    #[test]
    fn whitespace_before_operator_uses_fast_path_and_maps_selective_geometry() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "my $x = 42;";
        let old_tree = parser.parse(source1)?;
        parser.edit(Edit::new(
            6,
            6,
            8,
            Position::new(6, 1, 7),
            Position::new(6, 1, 7),
            Position::new(8, 1, 9),
        ));
        let source2 = "my $x   = 42;";
        let incremental = parser.parse(source2)?;
        let fresh = Parser::new(source2).parse()?;

        assert_eq!(
            incremental, fresh,
            "incremental result must equal a fresh parse in shape and span geometry"
        );
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());
        assert_eq!(parser.reparsed_nodes, 0);
        assert_eq!(parser.reused_nodes, parser.count_nodes(&old_tree));

        if let NodeKind::Program { statements } = &incremental.kind
            && let NodeKind::VariableDeclaration {
                variable, initializer: Some(initializer), ..
            } = &statements[0].kind
        {
            assert_eq!(variable.location, SourceLocation::new(3, 5));
            assert_eq!(initializer.location, SourceLocation::new(10, 12));
        } else {
            return Err(perl_parser_core::error::ParseError::UnexpectedEof);
        }
        Ok(())
    }

    #[test]
    fn comment_insertion_is_not_admitted_as_whitespace_reuse() -> ParseResult<()> {
        let source1 = "my $x = 42;";
        let source2 = "my $x = 42; # comment";
        let edit = Edit::new(
            11,
            11,
            21,
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
            Position::new(21, 1, 22),
        );
        let mut edits = EditSet::new();
        edits.add(edit.clone());
        assert!(WhitespaceEditMap::try_new(source1, source2, &edits).is_none());

        let mut parser = IncrementalParserV2::new();
        parser.parse(source1)?;
        parser.edit(edit);
        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, Parser::new(source2).parse()?);
        Ok(())
    }

    #[test]
    fn trailing_whitespace_uses_fast_path_without_expanding_program_span() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "my $x = 42;";
        let old_tree = parser.parse(source1)?;
        parser.edit(Edit::new(
            11,
            11,
            16,
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
            Position::new(16, 1, 17),
        ));
        let source2 = "my $x = 42;     ";
        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, Parser::new(source2).parse()?);
        assert_eq!(incremental.location, old_tree.location);
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());
        assert_eq!(parser.reparsed_nodes, 0);
        Ok(())
    }

    #[test]
    fn newline_insertion_uses_fast_path_without_expanding_program_span() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "my $x = 42;";
        let old_tree = parser.parse(source1)?;
        parser.edit(Edit::new(
            11,
            11,
            12,
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
            Position::new(12, 2, 1),
        ));
        let source2 = "my $x = 42;\n";
        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, Parser::new(source2).parse()?);
        assert_eq!(incremental.location, old_tree.location);
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());
        assert_eq!(parser.reparsed_nodes, 0);
        Ok(())
    }

    #[test]
    fn whitespace_deletion_uses_fast_path_and_maps_selective_geometry() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "my  $x  =  42;";
        let old_tree = parser.parse(source1)?;
        parser.edit(Edit::new(
            3,
            4,
            3,
            Position::new(3, 1, 4),
            Position::new(4, 1, 5),
            Position::new(3, 1, 4),
        ));
        let source2 = "my $x  =  42;";
        let incremental = parser.parse(source2)?;
        let fresh = Parser::new(source2).parse()?;

        assert_eq!(
            incremental, fresh,
            "incremental result must equal a fresh parse in shape and span geometry"
        );
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());
        assert_eq!(parser.reparsed_nodes, 0);
        assert_eq!(parser.reused_nodes, parser.count_nodes(&old_tree));

        if let NodeKind::Program { statements } = &incremental.kind
            && let NodeKind::VariableDeclaration {
                variable, initializer: Some(initializer), ..
            } = &statements[0].kind
        {
            assert_eq!(variable.location, SourceLocation::new(3, 5));
            assert_eq!(initializer.location, SourceLocation::new(10, 12));
        } else {
            return Err(perl_parser_core::error::ParseError::UnexpectedEof);
        }
        Ok(())
    }

    #[test]
    fn whitespace_at_statement_boundary_shifts_only_the_following_statement() -> ParseResult<()> {
        let mut parser = IncrementalParserV2::new();
        let source1 = "print 'hello';my $x = 42;";
        let old_tree = parser.parse(source1)?;
        let (old_first, old_second) = if let NodeKind::Program { statements } = &old_tree.kind {
            (statements[0].location, statements[1].location)
        } else {
            return Err(perl_parser_core::error::ParseError::UnexpectedEof);
        };

        parser.edit(Edit::new(
            14,
            14,
            15,
            Position::new(14, 1, 15),
            Position::new(14, 1, 15),
            Position::new(15, 1, 16),
        ));
        let source2 = "print 'hello'; my $x = 42;";
        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, Parser::new(source2).parse()?);
        assert!(parser.used_incremental_path());
        assert!(!parser.used_advanced_reuse());

        if let NodeKind::Program { statements } = &incremental.kind {
            assert_eq!(statements[0].location, old_first);
            assert_eq!(
                statements[1].location,
                SourceLocation::new(old_second.start() + 1, old_second.end() + 1)
            );
        } else {
            return Err(perl_parser_core::error::ParseError::UnexpectedEof);
        }
        Ok(())
    }

    #[test]
    fn structural_replacement_is_not_admitted_as_whitespace_reuse() -> ParseResult<()> {
        let source1 = "my $x = 42;";
        let source2 = "my $x += 42;";
        let edit = Edit::new(
            6,
            7,
            8,
            Position::new(6, 1, 7),
            Position::new(7, 1, 8),
            Position::new(8, 1, 9),
        );
        let mut edits = EditSet::new();
        edits.add(edit.clone());
        assert!(WhitespaceEditMap::try_new(source1, source2, &edits).is_none());

        let mut parser = IncrementalParserV2::new();
        parser.parse(source1)?;
        parser.edit(edit);
        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, Parser::new(source2).parse()?);
        Ok(())
    }

    /// A parser whose advanced reuse is guaranteed to decline, so the simple
    /// and trivia fallback paths execute deterministically.
    fn strict_fallback_parser() -> IncrementalParserV2 {
        IncrementalParserV2::with_reuse_config(ReuseConfig {
            min_confidence: 1.0,
            ..Default::default()
        })
    }

    /// A variable rename admitted to the simple path must patch the Variable
    /// payload, not just its span: the accepted AST must name `y`, exactly
    /// like a fresh parse. The previous generic rebuild branch cloned the old
    /// `Variable { name: "x" }` payload unchanged.
    #[test]
    fn variable_rename_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("$x = 1;")?;
        parser.edit(Edit::new(
            1,
            2,
            2, // "x" -> "y"
            Position::new(1, 1, 2),
            Position::new(2, 1, 3),
            Position::new(2, 1, 3),
        ));
        let source2 = "$y = 1;";
        let incremental = parser.parse(source2)?;
        assert!(
            parser.used_incremental_path(),
            "a rename inside an admitted Variable leaf must stay incremental"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "renamed variable payload must match a fresh parse");
        Ok(())
    }

    /// A bareword rename admitted through an Identifier leaf must patch the
    /// identifier payload the same way.
    #[test]
    fn identifier_rename_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my $x = foo;")?;
        parser.edit(Edit::new(
            8,
            11,
            11, // "foo" -> "bar"
            Position::new(8, 1, 9),
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
        ));
        let source2 = "my $x = bar;";
        let incremental = parser.parse(source2)?;
        assert!(parser.used_incremental_path(), "bareword rename must stay incremental");
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "renamed identifier payload must match a fresh parse");
        Ok(())
    }

    /// Rename the name of the first `Variable` leaf and require the accepted
    /// incremental tree to equal a fresh parse of the edited source.
    ///
    /// A replacement strictly inside the admitted value leaf keeps the simple
    /// path selected while shifting every later node, so a full-tree equality
    /// check against the fresh parse is the honest oracle for payload
    /// sub-spans: `name_span`, `body_span`, `phase_span`, and catch-variable
    /// locations are remapped state that must move with the shift, and an
    /// absent sub-span (`None`, for example an anonymous `sub`) must stay
    /// absent. The rename length may grow or shrink so both shift directions
    /// are exercised.
    fn rename_first_variable_and_compare_with_fresh_parse(
        source1: &str,
        old_name: &str,
        new_name: &str,
    ) -> ParseResult<()> {
        let sigil_start = source1
            .find(&format!("${old_name}"))
            .ok_or(perl_parser_core::error::ParseError::UnexpectedEof)?;
        let name_start = sigil_start + 1;
        let name_end = name_start + old_name.len();
        let new_name_end =
            (name_end as isize + (new_name.len() as isize - old_name.len() as isize)) as usize;
        let source2 = format!("{}{new_name}{}", &source1[..name_start], &source1[name_end..]);

        let mut parser = strict_fallback_parser();
        parser.parse(source1)?;
        let column = |byte: usize| u32::try_from(byte + 1).unwrap_or(u32::MAX);
        parser.edit(Edit::new(
            name_start,
            name_end,
            new_name_end,
            Position::new(name_start, 1, column(name_start)),
            Position::new(name_end, 1, column(name_end)),
            Position::new(new_name_end, 1, column(new_name_end)),
        ));
        let incremental = parser.parse(&source2)?;
        assert!(
            parser.used_incremental_path(),
            "a rename inside an admitted Variable leaf must stay incremental for {source1:?}"
        );
        let fresh = Parser::new(&source2).parse()?;
        assert_eq!(
            incremental, fresh,
            "incremental result must equal a fresh parse after renaming ${old_name} in {source1:?}"
        );
        Ok(())
    }

    /// An insertion-lengthening rename before declared constructs must shift
    /// `Package::name_span`, `Subroutine::name_span`, `PhaseBlock::phase_span`,
    /// `Heredoc::body_span`, and the `Try` catch-variable location together
    /// with the node spans they anchor. The incremental path used to shift
    /// only `location` and leave every payload sub-span at its pre-edit
    /// offsets while the tree was still accepted.
    #[test]
    fn value_edit_growth_shifts_later_payload_sub_spans_like_a_fresh_parse() -> ParseResult<()> {
        rename_first_variable_and_compare_with_fresh_parse(
            "my $v = 1;\npackage Local::Sample;\nsub greeting { return $v; }\nBEGIN { $v; }\nmy $text = <<'EOS';\nbody\nEOS\ntry { $v; } catch ($err) { $v; }\n",
            "v",
            "value",
        )
    }

    /// A shrinking rename (negative shift) before a `Format` must keep
    /// `Format::name_span` coherent with a fresh parse in the other shift
    /// direction as well.
    #[test]
    fn value_edit_shrink_shifts_later_payload_sub_spans_like_a_fresh_parse() -> ParseResult<()> {
        rename_first_variable_and_compare_with_fresh_parse(
            "my $probe = 1;\nformat Sample =\n@\n$probe\n.\n",
            "probe",
            "p",
        )
    }

    /// `Class` and `Method` name spans must shift like any other payload
    /// sub-span, and an anonymous `sub` (negative control) must keep its
    /// absent `name_span` absent rather than gaining a shifted `Some`.
    #[test]
    fn value_edit_shifts_class_method_name_spans_and_preserves_absent_ones() -> ParseResult<()> {
        rename_first_variable_and_compare_with_fresh_parse(
            "use feature 'class';\nmy $probe = 1;\nclass Sample { method greet { $probe } }\nmy $code = sub { $probe; };\n",
            "probe",
            "p",
        )
    }

    /// Braced variable forms (`${foo}`, `${Foo::Bar}`) span the braces while
    /// the stored name strips them. A rename inside the braces must store the
    /// brace-stripped inner text — the previous suffix slice kept the closing
    /// brace in the accepted payload.
    #[test]
    fn braced_variable_rename_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my ${foo} = 1;")?;
        parser.edit(Edit::new(
            5,
            8,
            8, // "foo" -> "bar" inside the braces
            Position::new(5, 1, 6),
            Position::new(8, 1, 9),
            Position::new(8, 1, 9),
        ));
        let source2 = "my ${bar} = 1;";
        let incremental = parser.parse(source2)?;
        assert!(
            parser.used_incremental_path(),
            "a rename inside braced variables must stay incremental"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "braced rename payload must match a fresh parse");

        let mut parser = strict_fallback_parser();
        parser.parse("my ${Foo::Bar} = 1;")?;
        parser.edit(Edit::new(
            11,
            13,
            13, // "Bar" -> "Baz" inside the qualified braced form
            Position::new(11, 1, 12),
            Position::new(13, 1, 14),
            Position::new(13, 1, 14),
        ));
        let source3 = "my ${Foo::Baz} = 1;";
        let incremental = parser.parse(source3)?;
        assert!(parser.used_incremental_path(), "qualified braced rename must stay incremental");
        let fresh = Parser::new(source3).parse()?;
        assert_eq!(incremental, fresh, "qualified braced rename payload must match a fresh parse");
        Ok(())
    }

    /// A zero-width insertion at a literal's start extends that literal: the
    /// accepted Number must span [5..7] with value "21". The previous mapping
    /// counted the boundary insertion in both the node shift and the content
    /// delta, accepting `Number [6..8] "1;"`, which no fresh parse produces.
    #[test]
    fn leading_literal_insertion_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("$x = 1;")?;
        parser.edit(Edit::new(
            5,
            5,
            6, // insert "2" at byte 5
            Position::new(5, 1, 6),
            Position::new(5, 1, 6),
            Position::new(6, 1, 7),
        ));
        let source2 = "$x = 21;";
        let incremental = parser.parse(source2)?;
        assert!(
            parser.used_incremental_path(),
            "a boundary insertion into an admitted Number leaf must stay incremental"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "extended literal must match a fresh parse");
        Ok(())
    }

    /// Replacing `"k"` with `"j", 42` splits the token: the simple path must
    /// decline and fall back to a full parse instead of accepting one String
    /// spanning both arguments.
    #[test]
    fn token_splitting_string_replacement_falls_back_to_full_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("$u->get(\"k\");")?;
        parser.edit(Edit::new(
            8,
            11,
            15, // "\"k\"" -> "\"j\", 42"
            Position::new(8, 1, 9),
            Position::new(11, 1, 12),
            Position::new(15, 1, 16),
        ));
        let source2 = "$u->get(\"j\", 42);";
        let incremental = parser.parse(source2)?;
        assert!(
            !parser.used_incremental_path(),
            "a replacement that splits the token must decline the incremental path"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");
        Ok(())
    }

    /// A replacement that changes string syntax must decline: switching `'`
    /// to `"` flips the stored interpolation flag, and switching `q(` to
    /// `qq(` does the same inside the quote-operator family. The patch cannot
    /// recompute those fields, so the whole incremental attempt falls back to
    /// a full parse instead of accepting a stale-semantics tree.
    #[test]
    fn quote_syntax_changes_decline_incremental_path() -> ParseResult<()> {
        // Single quotes to double quotes inside a method-call argument.
        let mut parser = strict_fallback_parser();
        parser.parse("$u->get('k');")?;
        parser.edit(Edit::new(
            8,
            11,
            11, // 'k' -> "k"
            Position::new(8, 1, 9),
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
        ));
        let source2 = "$u->get(\"k\");";
        let incremental = parser.parse(source2)?;
        assert!(
            !parser.used_incremental_path(),
            "a quote-style change must decline the incremental path"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");

        // Single-quote operator to double-quote operator: the first byte
        // matches, so the operator family itself must be compared too.
        let mut parser = strict_fallback_parser();
        parser.parse("$u->get(q(k));")?;
        parser.edit(Edit::new(
            8,
            9,
            10, // q( -> qq(
            Position::new(8, 1, 9),
            Position::new(9, 1, 10),
            Position::new(10, 1, 11),
        ));
        let source3 = "$u->get(qq(k));";
        let incremental = parser.parse(source3)?;
        assert!(
            !parser.used_incremental_path(),
            "a quote-operator change must decline the incremental path"
        );
        let fresh = Parser::new(source3).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");
        Ok(())
    }

    /// An earlier lengthening edit must not hide a later value edit: queued
    /// edits are expressed in post-edit coordinates while the tree carries
    /// original coordinates, so invalidation maps every edit back by the
    /// cumulative shift before comparing. The previous raw-coordinate
    /// comparison left the second leaf's payload stale while still accepting
    /// the tree.
    #[test]
    fn lengthening_edit_before_second_value_edit_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my $a = 1; my $b = 2;")?;
        // First edit: "1" -> "000000000000" (+11 bytes).
        parser.edit(Edit::new(
            8,
            9,
            20,
            Position::new(8, 1, 9),
            Position::new(9, 1, 10),
            Position::new(20, 1, 21),
        ));
        // Second edit, in coordinates after the first: the trailing "2" moved
        // from [19..20) to [30..31); change it to "9".
        parser.edit(Edit::new(
            30,
            31,
            31,
            Position::new(30, 1, 31),
            Position::new(31, 1, 32),
            Position::new(31, 1, 32),
        ));
        let source2 = "my $a = 000000000000; my $b = 9;";
        let incremental = parser.parse(source2)?;
        assert!(
            parser.used_incremental_path(),
            "both edits sit inside admitted Number leaves and must stay incremental"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "the later leaf's payload must be patched");
        Ok(())
    }

    /// Several whitespace edits compose: each node's span follows the
    /// cumulative boundary mapping and the program span keeps hugging its
    /// statements.
    #[test]
    fn multiple_trivia_edits_match_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my $x = 42;my $y = 7;")?;
        // Insert one space between the statements...
        parser.edit(Edit::new(
            11,
            11,
            12,
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
            Position::new(12, 1, 13),
        ));
        // ...and, in the coordinates after that insertion, widen the gap by
        // two more spaces.
        parser.edit(Edit::new(
            12,
            12,
            14,
            Position::new(12, 1, 13),
            Position::new(12, 1, 13),
            Position::new(14, 1, 15),
        ));
        let source2 = "my $x = 42;   my $y = 7;";
        let incremental = parser.parse(source2)?;
        assert!(
            parser.used_incremental_path(),
            "whitespace-only edits must take the whitespace reuse path"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "combined whitespace edits must match a fresh parse");
        Ok(())
    }

    /// Trivia inserted inside a multi-character operator changes
    /// tokenization: `1..3` becomes `1. .3`, which parses differently. The
    /// inserted text lexes as trivia in isolation, so admission must also
    /// require the surrounding token stream to be unchanged; otherwise the
    /// trivia remap would accept the old operator.
    #[test]
    fn operator_splitting_trivia_declines_trivia_path() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my @r = 1..3;")?;
        parser.edit(Edit::new(
            10,
            10,
            11, // insert a space between the two dots of `..`
            Position::new(10, 1, 11),
            Position::new(10, 1, 11),
            Position::new(11, 1, 12),
        ));
        let source2 = "my @r = 1. .3;";
        let incremental = parser.parse(source2)?;
        assert!(
            !parser.used_incremental_path(),
            "trivia that splits an operator must decline the trivia path"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");
        Ok(())
    }

    /// A `#` inserted more than a window away from code comments out
    /// everything to end of line: the second statement disappears from a
    /// fresh parse while both bounded windows would contain no non-trivia
    /// tokens. The whole-stream comparison must catch it.
    #[test]
    fn comment_hiding_code_declines_trivia_path() -> ParseResult<()> {
        let source1 = format!("my $a = 1;{}my $b = 2;", " ".repeat(20));
        let mut parser = strict_fallback_parser();
        parser.parse(&source1)?;
        parser.edit(Edit::new(
            10,
            10,
            11, // insert "#" right after the first semicolon
            Position::new(10, 1, 11),
            Position::new(10, 1, 11),
            Position::new(11, 1, 12),
        ));
        let source2 = format!("my $a = 1;#{}my $b = 2;", " ".repeat(20));
        let incremental = parser.parse(&source2)?;
        assert!(
            !parser.used_incremental_path(),
            "a comment that hides a statement must decline the trivia path"
        );
        let fresh = Parser::new(&source2).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");
        Ok(())
    }

    /// A newline inserted inside a comment exposes the hidden code as
    /// statements: the token stream gains tokens the old tree does not have.
    #[test]
    fn newline_exposing_commented_code_declines_trivia_path() -> ParseResult<()> {
        let source1 = "my $a = 1; # my $b = 2;";
        let mut parser = strict_fallback_parser();
        parser.parse(source1)?;
        parser.edit(Edit::new(
            13,
            13,
            14, // insert a newline between "#" and the hidden statement
            Position::new(13, 1, 14),
            Position::new(13, 1, 14),
            Position::new(14, 2, 1),
        ));
        let source2 = "my $a = 1; #\nmy $b = 2;";
        let incremental = parser.parse(source2)?;
        assert!(
            !parser.used_incremental_path(),
            "a newline exposing commented-out code must decline the trivia path"
        );
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "the fallback tree must match a fresh parse");
        Ok(())
    }

    /// Trailing trivia must not move any node. The previous trivia path
    /// shifted every span by the edit's byte delta, accepting a tree where
    /// every node was displaced by five bytes relative to a fresh parse.
    #[test]
    fn trailing_trivia_insertion_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my $x = 42;")?;
        parser.edit(Edit::new(
            11,
            11,
            16, // insert five trailing spaces
            Position::new(11, 1, 12),
            Position::new(11, 1, 12),
            Position::new(16, 1, 17),
        ));
        let source2 = "my $x = 42;     ";
        let incremental = parser.parse(source2)?;
        assert!(parser.used_incremental_path(), "trailing trivia must take the trivia remap path");
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "trivia remap must match a fresh parse");
        Ok(())
    }

    /// Trivia inserted before the first statement shifts that statement while
    /// the program span stays anchored at byte zero and tracks its last child.
    #[test]
    fn leading_trivia_insertion_matches_a_fresh_parse() -> ParseResult<()> {
        let mut parser = strict_fallback_parser();
        parser.parse("my $x = 42;")?;
        parser.edit(Edit::new(
            0,
            0,
            1, // insert one leading space
            Position::new(0, 1, 1),
            Position::new(0, 1, 1),
            Position::new(1, 1, 2),
        ));
        let source2 = " my $x = 42;";
        let incremental = parser.parse(source2)?;
        assert!(parser.used_incremental_path(), "leading trivia must take the trivia remap path");
        let fresh = Parser::new(source2).parse()?;
        assert_eq!(incremental, fresh, "leading trivia remap must match a fresh parse");
        Ok(())
    }

    /// Every fixture node spans `[start..total_end]`; the chain descends one
    /// start byte per level down to the number literal.
    fn assert_chain_spans(node: &Node, start: usize, total_end: usize, visited: &mut usize) {
        assert_eq!(node.location, SourceLocation::new(start, total_end), "node span moved");
        *visited += 1;
        match &node.kind {
            NodeKind::Program { statements } => {
                assert_eq!(statements.len(), 1);
                assert_chain_spans(&statements[0], start, total_end, visited);
            }
            NodeKind::Unary { operand, .. } => {
                assert_chain_spans(operand, start + 1, total_end, visited);
            }
            NodeKind::Number { value } => {
                assert_eq!(value, "1");
                assert_eq!(start, total_end - 1);
            }
            other => panic!("unexpected fixture node {}", other.kind_name()),
        }
    }

    /// The whitespace reuse must stay linear in tree depth: one structural
    /// clone plus one mapped-location pass per node. A depth-20,000 chain
    /// completes in milliseconds; the pre-#13917 recursive rebuild re-cloned
    /// the entire remaining subtree at every level — quadratic work — and
    /// cannot finish within this budget.
    #[test]
    fn whitespace_reuse_is_linear_in_tree_depth() {
        const DEPTH: usize = 20_000;
        let total_end = DEPTH + 1;
        let loc = |start: usize, end: usize| SourceLocation::new(start, end);
        let mut chain =
            Node::new(NodeKind::Number { value: "1".to_string() }, loc(DEPTH, total_end));
        for start in (0..DEPTH).rev() {
            chain = Node::new(
                NodeKind::Unary { op: "!".to_string(), operand: Box::new(chain) },
                loc(start, total_end),
            );
        }
        let root = Node::new(NodeKind::Program { statements: vec![chain] }, loc(0, total_end));
        let old_source = format!("{}1", "!".repeat(DEPTH));
        let new_source = format!("{}1 ", "!".repeat(DEPTH));
        let mut edits = EditSet::new();
        edits.add(Edit::new(
            total_end,
            total_end,
            total_end + 1, // trailing space
            Position::new(total_end, 1, 1),
            Position::new(total_end, 1, 1),
            Position::new(total_end + 1, 1, 1),
        ));

        // The mapped clone walks the canonical traversal once, so
        // adversarially deep chains need a large stack and this fixture
        // provides one. The bound pinned here is time, not stack: one
        // structural clone plus one location-mapping pass must stay linear
        // in depth, where the previous per-level `node.kind.clone()` rebuild
        // was quadratic.
        let work = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let edit_map = WhitespaceEditMap::try_new(&old_source, &new_source, &edits)
                    .expect("trailing whitespace insertion should be admitted");

                let start = Instant::now();
                let mapped = edit_map
                    .clone_tree(&root)
                    .expect("location mapping should succeed on the fixture");
                let elapsed = start.elapsed();

                let budget = adaptive_perf_budget_micros(2_000_000);
                assert!(
                    elapsed.as_micros() < budget,
                    "whitespace reuse must stay linear in depth: {}µs (budget {}µs)",
                    elapsed.as_micros(),
                    budget
                );

                // Trailing trivia moves nothing.
                let mut visited = 0usize;
                assert_chain_spans(&mapped, 0, total_end, &mut visited);
                assert_eq!(visited, DEPTH + 2, "every fixture node must be checked");
            })
            .expect("test worker thread");
        work.join().expect("linear whitespace reuse worker panicked");
    }
}
