//! Advanced tree reuse algorithms for incremental parsing
//!
//! This module implements sophisticated AST node reuse strategies that go beyond
//! simple value matching to achieve high node reuse rates even for complex edits.
//!
//! ## Key Features
//!
//! - **Structural similarity analysis** - Compare AST subtree patterns
//! - **Position-aware reuse** - Understand which nodes can be safely repositioned
//! - **Content-based hashing** - Fast comparison of subtree equivalence
//! - **Incremental node mapping** - Efficient lookup of reusable nodes
//! - **Advanced validation** - Ensure reused nodes maintain correctness
//!
//! ## Performance Targets
//!
//! - **≥85% node reuse** for simple value edits
//! - **≥70% node reuse** for structural modifications
//! - **≥50% node reuse** for complex multi-edit scenarios
//! - **<500µs processing** for reuse analysis on typical documents

use perl_parser_core::{
    ast::{NativeDebugSexpLimits, NativeDebugSexpResult, Node, NodeKind},
    edit::EditSet,
    position::{Position, Range},
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};

/// Advanced node reuse analyzer with sophisticated matching algorithms
#[derive(Debug)]
pub(super) struct AdvancedReuseAnalyzer {
    /// Cache of node structural hashes for fast comparison
    node_hashes: HashMap<usize, u64>,
    /// Mapping of positions to potentially reusable nodes
    position_map: HashMap<usize, Vec<NodeCandidate>>,
    /// Set of nodes that are known to be affected by edits
    affected_nodes: HashSet<usize>,
    /// Statistics for reuse analysis
    pub(super) analysis_stats: ReuseAnalysisStats,
}

/// Statistics tracking reuse analysis performance and effectiveness
#[derive(Debug, Default, Clone)]
pub struct ReuseAnalysisStats {
    pub nodes_analyzed: usize,
    pub structural_matches: usize,
    pub content_matches: usize,
    pub position_adjustments: usize,
    pub reuse_candidates_found: usize,
    pub validation_passes: usize,
    pub validation_failures: usize,
}

/// A candidate node for reuse with metadata about its reusability
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by future advanced matching strategies
struct NodeCandidate {
    node: Node,
    structural_hash: u64,
    confidence_score: f64,
    position_delta: isize,
    reuse_type: ReuseType,
}

/// Types of reuse strategies available for nodes
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Used by future advanced matching strategies
pub enum ReuseType {
    /// Direct reuse - node unchanged
    Direct,
    /// Position shift - same content, different position
    PositionShift,
    /// Content update - same structure, updated values
    ContentUpdate,
    /// Structural equivalent - same pattern with different details
    StructuralEquivalent,
}

/// Configuration for reuse analysis behavior
#[derive(Debug, Clone)]
pub struct ReuseConfig {
    /// Minimum confidence score for reuse (0.0-1.0)
    pub min_confidence: f64,
    /// Maximum position shift allowed for reuse
    pub max_position_shift: usize,
    /// Enable aggressive structural matching
    pub aggressive_structural_matching: bool,
    /// Enable content-based reuse for literals
    pub enable_content_reuse: bool,
    /// Maximum depth for recursive analysis
    pub max_analysis_depth: usize,
}

impl Default for ReuseConfig {
    fn default() -> Self {
        ReuseConfig {
            min_confidence: 0.75,
            max_position_shift: 1000,
            aggressive_structural_matching: true,
            enable_content_reuse: true,
            max_analysis_depth: 10,
        }
    }
}

impl Default for AdvancedReuseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AdvancedReuseAnalyzer {
    /// Create a new reuse analyzer with default configuration
    pub(super) fn new() -> Self {
        AdvancedReuseAnalyzer {
            node_hashes: HashMap::new(),
            position_map: HashMap::new(),
            affected_nodes: HashSet::new(),
            analysis_stats: ReuseAnalysisStats::default(),
        }
    }

    /// Create analyzer with custom configuration
    pub(super) fn with_config(_config: ReuseConfig) -> Self {
        // Store config for future use if needed
        Self::new()
    }

    /// Analyze potential node reuse between old and new trees
    ///
    /// Returns a mapping of old node positions to reuse strategies,
    /// enabling intelligent tree reconstruction with maximum node reuse.
    pub(super) fn analyze_reuse_opportunities(
        &mut self,
        old_tree: &Node,
        new_tree: &Node,
        edits: &EditSet,
        config: &ReuseConfig,
    ) -> ReuseAnalysisResult {
        self.analysis_stats = ReuseAnalysisStats::default();

        // Reset internal state
        self.node_hashes.clear();
        self.position_map.clear();
        self.affected_nodes.clear();

        // Build structural analysis of both trees
        let old_analysis = self.build_tree_analysis(old_tree, config);
        let new_analysis = self.build_tree_analysis(new_tree, config);

        // Identify affected regions from edits
        self.identify_affected_nodes(old_tree, edits);

        // Find reuse candidates using multiple strategies
        let mut reuse_map = HashMap::new();

        // Strategy 1: Direct structural matching
        self.find_direct_structural_matches(&old_analysis, &new_analysis, &mut reuse_map, config);

        // Strategy 2: Position-shifted matching
        self.find_position_shifted_matches(&old_analysis, &new_analysis, &mut reuse_map, config);

        // Strategy 3: Content-updated matching
        if config.enable_content_reuse {
            self.find_content_updated_matches(&old_analysis, &new_analysis, &mut reuse_map, config);
        }

        // Strategy 4: Aggressive structural matching
        if config.aggressive_structural_matching {
            self.find_aggressive_structural_matches(
                &old_analysis,
                &new_analysis,
                &mut reuse_map,
                config,
            );
        }

        // Validate reuse candidates and calculate confidence scores
        let validated_reuse_map =
            self.validate_reuse_candidates(reuse_map, old_tree, new_tree, config);

        // Calculate final statistics
        let total_old_nodes = self.count_nodes(old_tree);
        let total_new_nodes = self.count_nodes(new_tree);
        let reused_nodes = validated_reuse_map.len();
        let reuse_percentage = if total_old_nodes > 0 {
            (reused_nodes as f64 / total_old_nodes as f64) * 100.0
        } else {
            0.0
        };

        ReuseAnalysisResult {
            reuse_map: validated_reuse_map,
            total_old_nodes,
            total_new_nodes,
            reused_nodes,
            reuse_percentage,
            analysis_stats: self.analysis_stats.clone(),
        }
    }

    /// Build comprehensive analysis of tree structure
    fn build_tree_analysis(&mut self, tree: &Node, config: &ReuseConfig) -> TreeAnalysis {
        let mut analysis = TreeAnalysis::new();
        // One bottom-up pass over the whole tree, independent of how deep the
        // analysis walk below is allowed to go.
        let content_hashes = ContentHashes::compute(tree);
        self.analyze_node_recursive(tree, &mut analysis, 0, config, &content_hashes);
        analysis
    }

    /// Recursively analyze nodes to build structural understanding
    fn analyze_node_recursive(
        &mut self,
        node: &Node,
        analysis: &mut TreeAnalysis,
        depth: usize,
        config: &ReuseConfig,
        content_hashes: &ContentHashes,
    ) {
        if depth > config.max_analysis_depth {
            return;
        }

        self.analysis_stats.nodes_analyzed += 1;

        // Calculate structural hash
        let structural_hash = self.calculate_structural_hash(node);
        self.node_hashes.insert(node.location.start, structural_hash);

        // Create node info for analysis
        let node_info = NodeAnalysisInfo {
            node: node.clone(),
            structural_hash,
            depth,
            children_count: self.get_children_count(node),
            content_hash: content_hashes.get(node),
        };

        analysis.add_node_info(node.location.start, node_info);

        // Add to position map
        let candidate = NodeCandidate {
            node: node.clone(),
            structural_hash,
            confidence_score: 1.0, // Will be refined during analysis
            position_delta: 0,
            reuse_type: ReuseType::Direct,
        };

        self.position_map.entry(node.location.start).or_default().push(candidate);

        // Recurse into children
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for stmt in statements {
                    self.analyze_node_recursive(stmt, analysis, depth + 1, config, content_hashes);
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                self.analyze_node_recursive(variable, analysis, depth + 1, config, content_hashes);
                if let Some(init) = initializer {
                    self.analyze_node_recursive(init, analysis, depth + 1, config, content_hashes);
                }
            }
            NodeKind::Binary { left, right, .. } => {
                self.analyze_node_recursive(left, analysis, depth + 1, config, content_hashes);
                self.analyze_node_recursive(right, analysis, depth + 1, config, content_hashes);
            }
            NodeKind::Unary { operand, .. } => {
                self.analyze_node_recursive(operand, analysis, depth + 1, config, content_hashes);
            }
            NodeKind::FunctionCall { args, .. } => {
                for arg in args {
                    self.analyze_node_recursive(arg, analysis, depth + 1, config, content_hashes);
                }
            }
            NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
                self.analyze_node_recursive(condition, analysis, depth + 1, config, content_hashes);
                self.analyze_node_recursive(
                    then_branch,
                    analysis,
                    depth + 1,
                    config,
                    content_hashes,
                );
                for (cond, branch) in elsif_branches {
                    self.analyze_node_recursive(cond, analysis, depth + 1, config, content_hashes);
                    self.analyze_node_recursive(
                        branch,
                        analysis,
                        depth + 1,
                        config,
                        content_hashes,
                    );
                }
                if let Some(branch) = else_branch {
                    self.analyze_node_recursive(
                        branch,
                        analysis,
                        depth + 1,
                        config,
                        content_hashes,
                    );
                }
            }
            _ => {} // Leaf nodes
        }
    }

    /// Identify nodes affected by edits
    fn identify_affected_nodes(&mut self, tree: &Node, edits: &EditSet) {
        for range in edits.coalesced_affected_ranges() {
            self.mark_affected_nodes_in_range(tree, range.start.byte, range.end.byte);
        }
    }

    /// Mark nodes as affected if they overlap with edit ranges
    fn mark_affected_nodes_in_range(&mut self, node: &Node, start: usize, end: usize) {
        let node_range = Range::from(node.location);
        let edit_range = Range::new(Position::new(start, 0, 0), Position::new(end, 0, 0));

        if !node_range.overlaps(&edit_range) {
            return;
        }
        self.affected_nodes.insert(node.location.start);

        // Recurse into children
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for stmt in statements {
                    self.mark_affected_nodes_in_range(stmt, start, end);
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                self.mark_affected_nodes_in_range(variable, start, end);
                if let Some(init) = initializer {
                    self.mark_affected_nodes_in_range(init, start, end);
                }
            }
            NodeKind::Binary { left, right, .. } => {
                self.mark_affected_nodes_in_range(left, start, end);
                self.mark_affected_nodes_in_range(right, start, end);
            }
            _ => {} // Handle other node types as needed
        }
    }

    /// Find direct structural matches between trees
    fn find_direct_structural_matches(
        &mut self,
        old_analysis: &TreeAnalysis,
        new_analysis: &TreeAnalysis,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        config: &ReuseConfig,
    ) {
        let mut used_target_positions: HashSet<usize> =
            reuse_map.values().map(|strategy| strategy.target_position).collect();

        for (old_pos, old_info) in &old_analysis.node_info {
            // Skip affected nodes for direct matching
            if self.affected_nodes.contains(old_pos) {
                continue;
            }

            // Look for exact structural matches in new tree
            for (new_pos, new_info) in &new_analysis.node_info {
                if used_target_positions.contains(new_pos) {
                    continue;
                }

                if old_info.structural_hash == new_info.structural_hash
                    && old_info.content_hash == new_info.content_hash
                    && old_info.children_count == new_info.children_count
                {
                    let confidence = self.calculate_match_confidence(old_info, new_info);
                    if confidence >= config.min_confidence {
                        reuse_map.insert(
                            *old_pos,
                            ReuseStrategy {
                                target_position: *new_pos,
                                reuse_type: ReuseType::Direct,
                                confidence_score: confidence,
                                position_adjustment: (*new_pos as isize) - (*old_pos as isize),
                            },
                        );
                        used_target_positions.insert(*new_pos);
                        self.analysis_stats.structural_matches += 1;
                        break; // Use first good match
                    }
                }
            }
        }
    }

    /// Find position-shifted matches (same content, different location)
    fn find_position_shifted_matches(
        &mut self,
        old_analysis: &TreeAnalysis,
        new_analysis: &TreeAnalysis,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        config: &ReuseConfig,
    ) {
        let mut used_target_positions: HashSet<usize> =
            reuse_map.values().map(|strategy| strategy.target_position).collect();

        for (old_pos, old_info) in &old_analysis.node_info {
            if reuse_map.contains_key(old_pos) {
                continue;
            }

            let mut best_match: Option<(usize, f64)> = None;
            for (new_pos, new_info) in &new_analysis.node_info {
                if used_target_positions.contains(new_pos) {
                    continue;
                }

                if old_info.content_hash == new_info.content_hash
                    && old_info.structural_hash == new_info.structural_hash
                {
                    let position_shift = (*new_pos as isize - *old_pos as isize).unsigned_abs();
                    if !self.is_position_shift_candidate_safe(
                        old_info,
                        new_info,
                        position_shift,
                        config,
                    ) {
                        continue;
                    }

                    let confidence = self.calculate_shifted_match_confidence(
                        old_info,
                        new_info,
                        position_shift,
                        config,
                    );
                    if confidence >= config.min_confidence
                        && best_match
                            .as_ref()
                            .is_none_or(|&(_, best_score)| confidence > best_score)
                    {
                        best_match = Some((*new_pos, confidence));
                    }
                }
            }

            if let Some((best_pos, confidence)) = best_match {
                reuse_map.insert(
                    *old_pos,
                    ReuseStrategy {
                        target_position: best_pos,
                        reuse_type: ReuseType::PositionShift,
                        confidence_score: confidence,
                        position_adjustment: (best_pos as isize) - (*old_pos as isize),
                    },
                );
                used_target_positions.insert(best_pos);
                self.analysis_stats.position_adjustments += 1;
            }
        }
    }

    /// Find content-updated matches (structure same, values changed)
    fn find_content_updated_matches(
        &mut self,
        old_analysis: &TreeAnalysis,
        new_analysis: &TreeAnalysis,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        config: &ReuseConfig,
    ) {
        let mut used_target_positions: HashSet<usize> =
            reuse_map.values().map(|strategy| strategy.target_position).collect();

        for (old_pos, old_info) in &old_analysis.node_info {
            if reuse_map.contains_key(old_pos) {
                continue;
            }

            // For leaf nodes, check if structure matches but content differs
            if old_info.children_count == 0 {
                for (new_pos, new_info) in &new_analysis.node_info {
                    if used_target_positions.contains(new_pos) {
                        continue;
                    }

                    if old_info.structural_hash == new_info.structural_hash
                        && old_info.content_hash != new_info.content_hash
                        && self.are_compatible_for_content_update(&old_info.node, &new_info.node)
                    {
                        let confidence = 0.8; // Content updates get medium confidence
                        if confidence >= config.min_confidence {
                            reuse_map.insert(
                                *old_pos,
                                ReuseStrategy {
                                    target_position: *new_pos,
                                    reuse_type: ReuseType::ContentUpdate,
                                    confidence_score: confidence,
                                    position_adjustment: (*new_pos as isize) - (*old_pos as isize),
                                },
                            );
                            used_target_positions.insert(*new_pos);
                            self.analysis_stats.content_matches += 1;
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Find aggressive structural matches using pattern analysis
    fn find_aggressive_structural_matches(
        &mut self,
        old_analysis: &TreeAnalysis,
        new_analysis: &TreeAnalysis,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        config: &ReuseConfig,
    ) {
        let mut used_target_positions: HashSet<usize> =
            reuse_map.values().map(|strategy| strategy.target_position).collect();

        // This is the most sophisticated matching - look for structural patterns
        // even when exact hashes don't match
        for (old_pos, old_info) in &old_analysis.node_info {
            if reuse_map.contains_key(old_pos) {
                continue;
            }

            let mut best_match: Option<(usize, f64)> = None;

            for (new_pos, new_info) in &new_analysis.node_info {
                if used_target_positions.contains(new_pos) {
                    continue;
                }

                if old_info.children_count > 0
                    && self.get_children_count(&old_info.node)
                        != self.get_children_count(&new_info.node)
                {
                    continue;
                }

                // Compare structural similarity
                let similarity =
                    self.calculate_structural_similarity(&old_info.node, &new_info.node);
                if similarity >= config.min_confidence * 0.8 {
                    // Slightly lower threshold for aggressive matching
                    if best_match.as_ref().is_none_or(|&(_, s)| similarity > s) {
                        best_match = Some((*new_pos, similarity));
                    }
                }
            }

            if let Some((best_pos, confidence)) = best_match
                && confidence >= config.min_confidence * 0.7
            {
                // Final threshold check
                reuse_map.insert(
                    *old_pos,
                    ReuseStrategy {
                        target_position: best_pos,
                        reuse_type: ReuseType::StructuralEquivalent,
                        confidence_score: confidence,
                        position_adjustment: (best_pos as isize) - (*old_pos as isize),
                    },
                );
                used_target_positions.insert(best_pos);
                self.analysis_stats.reuse_candidates_found += 1;
            }
        }
    }

    /// Validate reuse candidates to ensure correctness
    fn validate_reuse_candidates(
        &mut self,
        candidates: HashMap<usize, ReuseStrategy>,
        old_tree: &Node,
        new_tree: &Node,
        config: &ReuseConfig,
    ) -> HashMap<usize, ReuseStrategy> {
        let mut validated = HashMap::new();

        for (old_pos, strategy) in candidates {
            if self.validate_reuse_strategy(&strategy, old_tree, new_tree, config) {
                validated.insert(old_pos, strategy);
                self.analysis_stats.validation_passes += 1;
            } else {
                self.analysis_stats.validation_failures += 1;
            }
        }

        validated
    }

    /// Calculate structural hash for fast comparison
    fn calculate_structural_hash(&self, node: &Node) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash node kind discriminant
        std::mem::discriminant(&node.kind).hash(&mut hasher);

        // Hash structural properties based on node type
        match &node.kind {
            NodeKind::Program { statements } => {
                statements.len().hash(&mut hasher);
                "program".hash(&mut hasher);
            }
            NodeKind::Block { statements } => {
                statements.len().hash(&mut hasher);
                "block".hash(&mut hasher);
            }
            NodeKind::VariableDeclaration { declarator, .. } => {
                declarator.hash(&mut hasher);
                "vardecl".hash(&mut hasher);
            }
            NodeKind::Binary { op, .. } => {
                op.hash(&mut hasher);
                "binary".hash(&mut hasher);
            }
            NodeKind::Unary { op, .. } => {
                op.hash(&mut hasher);
                "unary".hash(&mut hasher);
            }
            NodeKind::FunctionCall { name, args } => {
                name.hash(&mut hasher);
                args.len().hash(&mut hasher);
                "funccall".hash(&mut hasher);
            }
            NodeKind::Number { .. } => "number".hash(&mut hasher),
            NodeKind::String { interpolated, .. } => {
                interpolated.hash(&mut hasher);
                "string".hash(&mut hasher);
            }
            NodeKind::VString { .. } => "vstring".hash(&mut hasher),
            NodeKind::Variable { sigil, .. } => {
                sigil.hash(&mut hasher);
                "variable".hash(&mut hasher);
            }
            NodeKind::Identifier { .. } => "identifier".hash(&mut hasher),
            _ => "other".hash(&mut hasher),
        }

        hasher.finish()
    }

    /// Get count of direct children for a node
    fn get_children_count(&self, node: &Node) -> usize {
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => statements.len(),
            NodeKind::VariableDeclaration { initializer, .. } => {
                if initializer.is_some() { 2 } else { 1 } // variable + optional initializer
            }
            NodeKind::Binary { .. } => 2, // left + right
            NodeKind::Unary { .. } => 1,  // operand
            NodeKind::FunctionCall { args, .. } => args.len(),
            NodeKind::If { elsif_branches, else_branch, .. } => {
                2 + elsif_branches.len() * 2 + if else_branch.is_some() { 1 } else { 0 }
            }
            _ => 0, // Leaf nodes
        }
    }

    /// Calculate confidence score for a potential match
    fn calculate_match_confidence(
        &self,
        old_info: &NodeAnalysisInfo,
        new_info: &NodeAnalysisInfo,
    ) -> f64 {
        let mut confidence = 0.0f64;

        // Structural match bonus
        if old_info.structural_hash == new_info.structural_hash {
            confidence += 0.4;
        }

        // Content match bonus
        if old_info.content_hash == new_info.content_hash {
            confidence += 0.3;
        }

        // Children count match bonus
        if old_info.children_count == new_info.children_count {
            confidence += 0.2;
        }

        // Depth similarity bonus
        let depth_diff = (old_info.depth as isize - new_info.depth as isize).abs();
        if depth_diff == 0 {
            confidence += 0.1;
        } else if depth_diff <= 2 {
            confidence += 0.05;
        }

        confidence.min(1.0)
    }

    /// Calculate structural similarity between two nodes
    fn calculate_structural_similarity(&self, old_node: &Node, new_node: &Node) -> f64 {
        // This is a more sophisticated comparison than hash equality
        let mut similarity = 0.0;

        // Base similarity from node type
        if std::mem::discriminant(&old_node.kind) == std::mem::discriminant(&new_node.kind) {
            similarity += 0.5;

            // Additional similarity based on node-specific properties
            match (&old_node.kind, &new_node.kind) {
                (NodeKind::Program { statements: s1 }, NodeKind::Program { statements: s2 }) => {
                    let len_similarity = 1.0
                        - ((s1.len() as f64 - s2.len() as f64).abs()
                            / (s1.len().max(s2.len()) as f64));
                    similarity += 0.3 * len_similarity;
                }
                (NodeKind::Binary { op: op1, .. }, NodeKind::Binary { op: op2, .. }) => {
                    if op1 == op2 {
                        similarity += 0.4;
                    }
                }
                (
                    NodeKind::FunctionCall { name: n1, args: a1 },
                    NodeKind::FunctionCall { name: n2, args: a2 },
                ) => {
                    if n1 == n2 {
                        similarity += 0.3;
                    }
                    let arg_similarity = 1.0
                        - ((a1.len() as f64 - a2.len() as f64).abs()
                            / (a1.len().max(a2.len()) as f64));
                    similarity += 0.2 * arg_similarity;
                }
                _ => {
                    similarity += 0.2; // Generic bonus for same type
                }
            }
        }

        similarity.min(1.0)
    }

    /// Check if two nodes are compatible for content updates
    fn are_compatible_for_content_update(&self, old_node: &Node, new_node: &Node) -> bool {
        match (&old_node.kind, &new_node.kind) {
            (NodeKind::Number { .. }, NodeKind::Number { .. }) => true,
            (
                NodeKind::String { interpolated: i1, .. },
                NodeKind::String { interpolated: i2, .. },
            ) => i1 == i2,
            (NodeKind::VString { .. }, NodeKind::VString { .. }) => true,
            (NodeKind::Variable { sigil: s1, .. }, NodeKind::Variable { sigil: s2, .. }) => {
                s1 == s2
            }
            (NodeKind::Identifier { .. }, NodeKind::Identifier { .. }) => true,
            _ => false,
        }
    }

    fn is_position_shift_candidate_safe(
        &self,
        old_info: &NodeAnalysisInfo,
        new_info: &NodeAnalysisInfo,
        position_shift: usize,
        config: &ReuseConfig,
    ) -> bool {
        if position_shift > config.max_position_shift {
            return false;
        }

        let old_is_container = self.is_container_node(&old_info.node);
        let new_is_container = self.is_container_node(&new_info.node);
        if old_is_container != new_is_container {
            return false;
        }

        if old_is_container {
            let max_container_shift = config.max_position_shift / 4;
            if position_shift > max_container_shift {
                return false;
            }

            let depth_diff = (old_info.depth as isize - new_info.depth as isize).unsigned_abs();
            if depth_diff > 1 || old_info.children_count != new_info.children_count {
                return false;
            }
        }

        true
    }

    fn calculate_shifted_match_confidence(
        &self,
        old_info: &NodeAnalysisInfo,
        new_info: &NodeAnalysisInfo,
        position_shift: usize,
        config: &ReuseConfig,
    ) -> f64 {
        let base_confidence = self.calculate_match_confidence(old_info, new_info);
        let shift_ratio = position_shift as f64 / config.max_position_shift.max(1) as f64;

        let shift_penalty = if self.is_container_node(&old_info.node) {
            0.45 * shift_ratio.min(1.0)
        } else if self.is_content_stable_leaf(&old_info.node) {
            0.12 * shift_ratio.min(1.0)
        } else {
            0.30 * shift_ratio.min(1.0)
        };

        (base_confidence - shift_penalty).clamp(0.0, 1.0)
    }

    fn is_content_stable_leaf(&self, node: &Node) -> bool {
        matches!(
            node.kind,
            NodeKind::Number { .. }
                | NodeKind::String { .. }
                | NodeKind::VString { .. }
                | NodeKind::Identifier { .. }
                | NodeKind::Variable { .. }
        )
    }

    fn is_container_node(&self, node: &Node) -> bool {
        matches!(node.kind, NodeKind::Program { .. } | NodeKind::Block { .. } | NodeKind::If { .. })
    }

    /// Validate a reuse strategy for correctness
    fn validate_reuse_strategy(
        &self,
        _strategy: &ReuseStrategy,
        _old_tree: &Node,
        _new_tree: &Node,
        _config: &ReuseConfig,
    ) -> bool {
        // Implement validation logic:
        // - Check that reused nodes maintain parent-child relationships
        // - Verify position adjustments are reasonable
        // - Ensure content updates are semantically valid
        // For now, accept all strategies (can be enhanced with specific validation)
        true
    }

    /// Count total nodes in a tree
    fn count_nodes(&self, node: &Node) -> usize {
        let mut count = 1;

        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for stmt in statements {
                    count += self.count_nodes(stmt);
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                count += self.count_nodes(variable);
                if let Some(init) = initializer {
                    count += self.count_nodes(init);
                }
            }
            NodeKind::Binary { left, right, .. } => {
                count += self.count_nodes(left);
                count += self.count_nodes(right);
            }
            NodeKind::Unary { operand, .. } => {
                count += self.count_nodes(operand);
            }
            NodeKind::FunctionCall { args, .. } => {
                for arg in args {
                    count += self.count_nodes(arg);
                }
            }
            NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
                count += self.count_nodes(condition);
                count += self.count_nodes(then_branch);
                for (cond, branch) in elsif_branches {
                    count += self.count_nodes(cond);
                    count += self.count_nodes(branch);
                }
                if let Some(branch) = else_branch {
                    count += self.count_nodes(branch);
                }
            }
            _ => {} // Leaf nodes
        }

        count
    }

    /// Map an old-tree byte position to its corresponding position in the new
    /// tree, using the supplied [`EditSet`] to apply byte shifts.
    ///
    /// Semantics:
    /// - If `old_pos` precedes the first edit's `start_byte`, returns `old_pos`
    ///   unchanged.
    /// - If `old_pos` falls inside an edit's old range
    ///   `[start_byte, old_end_byte)`, returns that edit's `new_end_byte`
    ///   (i.e. the position is consumed by the edit and snaps to the edit's
    ///   new boundary).
    /// - Otherwise the position is shifted by the cumulative byte shift of all
    ///   prior edits whose `old_end_byte <= old_pos`.
    ///
    /// Edits in [`EditSet`] are sorted by `start_byte`, so iteration short-
    /// circuits as soon as the next edit starts past `old_pos`.
    pub(super) fn map_old_position_to_new(&self, old_pos: usize, edits: &EditSet) -> usize {
        let mut shift: isize = 0;
        for edit in edits.edits() {
            if old_pos < edit.start_byte {
                break;
            }
            if old_pos < edit.old_end_byte {
                // Position is consumed by this edit; snap to its new end.
                return edit.new_end_byte;
            }
            shift += edit.byte_shift();
        }
        let signed = old_pos as isize + shift;
        if signed < 0 { 0 } else { signed as usize }
    }

    /// Attempt to register an `old_pos -> new_pos` reuse claim, enforcing the
    /// one-to-one invariant: at most one old position may map to any given
    /// new position.
    ///
    /// Returns `true` if the registration was inserted, `false` if some other
    /// `old_pos` has already claimed the same `new_pos` (in which case the
    /// existing claim is left unchanged).
    pub(super) fn try_register_match(
        &self,
        reuse_map: &mut HashMap<usize, ReuseStrategy>,
        old_pos: usize,
        new_pos: usize,
        reuse_type: ReuseType,
        confidence: f64,
    ) -> bool {
        if reuse_map.values().any(|s| s.target_position == new_pos) {
            return false;
        }
        reuse_map.insert(
            old_pos,
            ReuseStrategy {
                target_position: new_pos,
                reuse_type,
                confidence_score: confidence,
                position_adjustment: (new_pos as isize) - (old_pos as isize),
            },
        );
        true
    }
}

/// Comprehensive analysis of a tree structure
#[derive(Debug)]
struct TreeAnalysis {
    /// Ordered because greedy target reservation makes iteration order observable.
    node_info: BTreeMap<usize, NodeAnalysisInfo>,
}

impl TreeAnalysis {
    fn new() -> Self {
        TreeAnalysis { node_info: BTreeMap::new() }
    }

    fn add_node_info(&mut self, position: usize, info: NodeAnalysisInfo) {
        self.node_info.insert(position, info);
    }
}

/// Feeds rendered S-expression fragments straight into a hasher.
///
/// [`Node::render_debug_sexp`] writes through [`fmt::Write`], so no rendered
/// text is buffered on this side: fragments are hashed as they arrive and the
/// node's own payload string is never assembled here.
///
/// This is not an allocation-free path, and the sink cannot make it one. The
/// renderer still allocates internally per node — `grammar_kind_name` returns
/// an owned `String` even for a static name, `emit_atom` fills a scratch
/// `String`, and `load_children` builds a `Vec` — so the residue is O(1)
/// allocations per node. What is gone is the O(subtree) allocation the old
/// `node.to_sexp()` call made at every visited node. Removing the remainder
/// needs an allocation-free payload API on `perl-ast`, which is out of scope
/// here (#15037).
struct SexpHashSink<'a> {
    hasher: &'a mut DefaultHasher,
}

impl fmt::Write for SexpHashSink<'_> {
    fn write_str(&mut self, fragment: &str) -> fmt::Result {
        fragment.hash(self.hasher);
        Ok(())
    }
}

/// Content hashes for every node of one tree, computed bottom-up.
///
/// The hash of a node is its own grammar name and payloads folded together with
/// the already-computed hashes of its children, so each node's payload text is
/// rendered exactly once for the whole tree. The previous implementation hashed
/// `node.to_sexp()` per visited node, re-serializing overlapping subtrees at
/// every level of the walk.
///
/// Two subtrees that render to the same S-expression always receive the same
/// hash: the per-node component comes from the same renderer and the same
/// payload grammar, and children are folded in canonical visit-table order with
/// their field roles. Hash equality is a candidate filter only —
/// `collect_materializable_reuse` re-checks every candidate with full
/// structural equality before accepting it, so a collision costs a rejected
/// reuse opportunity and never a wrong tree.
#[derive(Debug, Default)]
struct ContentHashes {
    /// Keyed by node address rather than `location.start`: a parent and its
    /// first child can share a start offset, the same aliasing hazard already
    /// documented on `IncrementalParserV2::find_analyzed_node_at_start`.
    by_node: HashMap<usize, u64>,
    /// Total nodes the S-expression renderer visited across the whole pass.
    ///
    /// This is the measurement behind the depth-independence claim: it equals
    /// the node count exactly when each node's payload is rendered once. Whole
    /// subtree serialization drives it to the sum of all subtree sizes instead.
    #[cfg(test)]
    rendered_nodes: usize,
}

impl ContentHashes {
    /// Compute the content hash of every node in `root`'s subtree.
    ///
    /// The walk is iterative so that deeply nested input cannot overflow the
    /// stack, matching the renderer it draws its payloads from.
    fn compute(root: &Node) -> Self {
        let mut by_node: HashMap<usize, u64> = HashMap::new();
        #[cfg(test)]
        let mut rendered_nodes = 0usize;
        let mut pending: Vec<(&Node, bool)> = vec![(root, false)];

        while let Some((node, children_done)) = pending.pop() {
            if !children_done {
                pending.push((node, true));
                node.for_each_child_with_field(|_, child| pending.push((child, false)));
                continue;
            }

            let mut hasher = DefaultHasher::new();
            let rendered = Self::hash_own_payload(node, &mut hasher);
            #[cfg(test)]
            {
                rendered_nodes = rendered_nodes.saturating_add(rendered);
            }
            let _ = rendered;

            // Fold children in canonical visit-table order, each under the same
            // field name the renderer labels it with. The role is folded so the
            // hash mirrors the rendered form by construction: no current
            // `NodeKind` yields two identical child sequences under different
            // roles, so this guards a future AST shape rather than a case the
            // suite can falsify today. Child arity needs no separate term —
            // a shorter or longer child sequence is already a different hash
            // input (`child_arity_is_load_bearing`).
            node.for_each_child_with_field(|field, child| {
                match field {
                    Some(field) => field.name().hash(&mut hasher),
                    None => "child".hash(&mut hasher),
                }
                by_node.get(&Self::key(child)).copied().unwrap_or(0).hash(&mut hasher);
            });

            by_node.insert(Self::key(node), hasher.finish());
        }

        Self {
            by_node,
            #[cfg(test)]
            rendered_nodes,
        }
    }

    /// Content hash of a node belonging to the tree this map was computed over.
    fn get(&self, node: &Node) -> u64 {
        self.by_node.get(&Self::key(node)).copied().unwrap_or(0)
    }

    /// Number of nodes hashed.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.by_node.len()
    }

    fn key(node: &Node) -> usize {
        std::ptr::from_ref(node) as usize
    }

    /// Hash a node's own grammar name and payloads, excluding its children.
    ///
    /// Returns the number of nodes the renderer visited, which is `1` whenever
    /// the depth limit is doing its job.
    fn hash_own_payload(node: &Node, hasher: &mut DefaultHasher) -> usize {
        let limits =
            NativeDebugSexpLimits { max_depth: Some(0), ..NativeDebugSexpLimits::unbounded() };
        // A depth limit of zero renders this node's grammar name and payloads
        // and then refuses the first descent, so `Truncated` is the expected
        // result for any node that has children.
        let result = node.render_debug_sexp(&mut SexpHashSink { hasher }, limits);

        let rendered = match result {
            NativeDebugSexpResult::Complete { work }
            | NativeDebugSexpResult::Truncated { work, .. } => work.nodes_visited,
            NativeDebugSexpResult::InstrumentFailure { work, .. } => {
                // Unreachable for a sink that cannot fail, but a silently empty
                // payload would let two different nodes hash alike. Fall back to
                // a marker plus the kind discriminant; reuse still has to clear
                // full structural equality either way.
                "native-debug-sexp-instrument-failure".hash(hasher);
                std::mem::discriminant(&node.kind).hash(hasher);
                work.nodes_visited
            }
        };

        // A `try` block's catch binders are parent-owned payloads that the
        // renderer nests under each catch child rather than under the `try`
        // node itself (see `load_children` in `perl-ast`), so they are not
        // covered by the depth-limited render above.
        if let NodeKind::Try { catch_blocks, .. } = &node.kind {
            for (binder, _) in catch_blocks {
                binder.as_ref().map(|(name, _)| name).hash(hasher);
            }
        }

        rendered
    }
}

/// Detailed information about a node for reuse analysis
#[derive(Debug, Clone)]
struct NodeAnalysisInfo {
    node: Node,
    structural_hash: u64,
    content_hash: u64,
    depth: usize,
    children_count: usize,
}

/// Strategy for reusing a node from old tree to new tree
#[derive(Debug, Clone)]
pub struct ReuseStrategy {
    pub target_position: usize,
    pub reuse_type: ReuseType,
    pub confidence_score: f64,
    pub position_adjustment: isize,
}

/// Result of reuse analysis with comprehensive metrics
#[derive(Debug)]
pub struct ReuseAnalysisResult {
    pub reuse_map: HashMap<usize, ReuseStrategy>,
    pub total_old_nodes: usize,
    pub total_new_nodes: usize,
    pub reused_nodes: usize,
    pub reuse_percentage: f64,
    pub analysis_stats: ReuseAnalysisStats,
}

impl ReuseAnalysisResult {
    /// Check if reuse analysis achieved target efficiency
    pub fn meets_efficiency_target(&self, target_percentage: f64) -> bool {
        self.reuse_percentage >= target_percentage
    }

    /// Get a summary of the analysis performance
    pub fn performance_summary(&self) -> String {
        format!(
            "Reuse Analysis: {:.1}% efficiency ({}/{} nodes), {} structural matches, {} position adjustments",
            self.reuse_percentage,
            self.reused_nodes,
            self.total_old_nodes,
            self.analysis_stats.structural_matches,
            self.analysis_stats.position_adjustments
        )
    }
}

#[cfg(test)]
mod tests {
    use super::hash_fixtures::content_hash;
    use super::*;
    use perl_parser_core::{SourceLocation, ast::Node};

    #[test]
    fn test_advanced_reuse_analyzer_creation() {
        let analyzer = AdvancedReuseAnalyzer::new();
        assert_eq!(analyzer.analysis_stats.nodes_analyzed, 0);
    }

    #[test]
    fn test_structural_hash_calculation() {
        let analyzer = AdvancedReuseAnalyzer::new();

        // Create sample nodes
        let node1 = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let node2 = Node::new(
            NodeKind::Number { value: "99".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let hash1 = analyzer.calculate_structural_hash(&node1);
        let hash2 = analyzer.calculate_structural_hash(&node2);

        // Same structure should have same hash
        assert_eq!(hash1, hash2, "Numbers should have same structural hash regardless of value");
    }

    #[test]
    fn test_content_hash_differs_for_different_values() {
        let node1 = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let node2 = Node::new(
            NodeKind::Number { value: "99".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let hash1 = content_hash(&node1);
        let hash2 = content_hash(&node2);

        assert_ne!(hash1, hash2, "Different values should have different content hashes");
    }

    #[test]
    fn vstring_hashes_preserve_kind_and_value_boundaries() {
        let analyzer = AdvancedReuseAnalyzer::new();

        let node1 = Node::new(
            NodeKind::VString { value: "v1.2.3".to_string() },
            SourceLocation { start: 0, end: 6 },
        );
        let node2 = Node::new(
            NodeKind::VString { value: "v2.0.0".to_string() },
            SourceLocation { start: 0, end: 6 },
        );

        let structural_hash1 = analyzer.calculate_structural_hash(&node1);
        let structural_hash2 = analyzer.calculate_structural_hash(&node2);
        assert_eq!(
            structural_hash1, structural_hash2,
            "v-string structural hash should depend on kind, not version text"
        );

        let content_hash1 = content_hash(&node1);
        let content_hash2 = content_hash(&node2);
        assert_ne!(
            content_hash1, content_hash2,
            "v-string content hash must distinguish version text"
        );
    }

    #[test]
    fn test_children_count_calculation() {
        let analyzer = AdvancedReuseAnalyzer::new();

        // Leaf node
        let leaf = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 0, end: 2 },
        );
        assert_eq!(analyzer.get_children_count(&leaf), 0);

        // Binary node
        let binary = Node::new(
            NodeKind::Binary {
                op: "+".to_string(),
                left: Box::new(leaf.clone()),
                right: Box::new(leaf.clone()),
            },
            SourceLocation { start: 0, end: 5 },
        );
        assert_eq!(analyzer.get_children_count(&binary), 2);

        // Program node
        let program = Node::new(
            NodeKind::Program { statements: vec![binary] },
            SourceLocation { start: 0, end: 5 },
        );
        assert_eq!(analyzer.get_children_count(&program), 1);
    }

    #[test]
    fn test_reuse_config_defaults() {
        let config = ReuseConfig::default();
        assert_eq!(config.min_confidence, 0.75);
        assert_eq!(config.max_position_shift, 1000);
        assert!(config.aggressive_structural_matching);
        assert!(config.enable_content_reuse);
        assert_eq!(config.max_analysis_depth, 10);
    }

    #[test]
    fn test_node_compatibility_for_content_update() {
        let analyzer = AdvancedReuseAnalyzer::new();

        let num1 = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let num2 = Node::new(
            NodeKind::Number { value: "99".to_string() },
            SourceLocation { start: 0, end: 2 },
        );

        let str1 = Node::new(
            NodeKind::String { value: "hello".to_string(), interpolated: false },
            SourceLocation { start: 0, end: 7 },
        );

        let vstring1 = Node::new(
            NodeKind::VString { value: "v1.2.3".to_string() },
            SourceLocation { start: 0, end: 6 },
        );

        let vstring2 = Node::new(
            NodeKind::VString { value: "v1.2.4".to_string() },
            SourceLocation { start: 0, end: 6 },
        );

        // Same type nodes should be compatible
        assert!(analyzer.are_compatible_for_content_update(&num1, &num2));
        assert!(analyzer.are_compatible_for_content_update(&vstring1, &vstring2));

        // Different type nodes should not be compatible
        assert!(!analyzer.are_compatible_for_content_update(&num1, &str1));
        assert!(!analyzer.are_compatible_for_content_update(&vstring1, &str1));
    }

    #[test]
    fn vstring_is_content_stable_leaf_for_position_shift_scoring() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let vstring = Node::new(
            NodeKind::VString { value: "v1.2.3".to_string() },
            SourceLocation { start: 0, end: 6 },
        );

        assert!(
            analyzer.is_content_stable_leaf(&vstring),
            "v-string literals should use the same stable-leaf shift penalty as strings"
        );
    }

    #[test]
    fn test_identify_affected_nodes_marks_only_overlapping_regions() {
        let mut analyzer = AdvancedReuseAnalyzer::new();
        let tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number { value: "1".to_string() },
                        SourceLocation { start: 0, end: 1 },
                    ),
                    Node::new(
                        NodeKind::Number { value: "2".to_string() },
                        SourceLocation { start: 20, end: 21 },
                    ),
                ],
            },
            SourceLocation { start: 0, end: 21 },
        );

        let mut edits = EditSet::new();
        edits.add(perl_parser_core::edit::Edit::new(
            0,
            2,
            2,
            Position::new(0, 0, 0),
            Position::new(2, 0, 2),
            Position::new(2, 0, 2),
        ));

        analyzer.identify_affected_nodes(&tree, &edits);

        assert!(analyzer.affected_nodes.contains(&0));
        assert!(!analyzer.affected_nodes.contains(&20));
    }

    // --- map_old_position_to_new unit tests ---

    fn make_edit(start: usize, old_end: usize, new_end: usize) -> perl_parser_core::edit::Edit {
        perl_parser_core::edit::Edit::new(
            start,
            old_end,
            new_end,
            Position::new(start, 0, start as u32),
            Position::new(old_end, 0, old_end as u32),
            Position::new(new_end, 0, new_end as u32),
        )
    }

    #[test]
    fn test_map_position_before_edit_unchanged() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(10, 12, 14));
        // old_pos=5 is before edit start (10) — no shift applied
        assert_eq!(analyzer.map_old_position_to_new(5, &edits), 5);
    }

    #[test]
    fn test_map_position_after_single_edit_shifted() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(8, 10, 11)); // "10"→"100": shift +1
        // old_pos=13 should map to 14 (shifted by +1)
        assert_eq!(analyzer.map_old_position_to_new(13, &edits), 14);
    }

    #[test]
    fn test_map_position_accumulates_two_shifts() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(8, 10, 11)); // +1
        edits.add(make_edit(24, 26, 27)); // +1
        // old_pos=30 (past both edits) should shift by +2
        assert_eq!(analyzer.map_old_position_to_new(30, &edits), 32);
    }

    /// old_pos at edit.start_byte is inside the edit region — returns new_end_byte.
    ///
    /// The previous `consumed_shift` formulation could incorrectly apply a shift
    /// for this case instead of detecting the position as inside the edit.
    #[test]
    fn test_map_position_at_edit_start_returns_new_end() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        // Edit covers [10, 20) → new_end_byte = 15 (shrink)
        edits.add(make_edit(10, 20, 15));
        // old_pos=10 satisfies start_byte(10) <= old_pos(10) < old_end_byte(20)
        assert_eq!(analyzer.map_old_position_to_new(10, &edits), 15);
    }

    #[test]
    fn test_map_position_at_edit_old_end_is_shifted() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(10, 20, 25)); // +5 shift
        // old_pos=20 is at old_end_byte (exclusive boundary) — shifted by +5
        assert_eq!(analyzer.map_old_position_to_new(20, &edits), 25);
    }

    #[test]
    fn test_map_position_inside_edit_returns_new_end() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(10, 30, 20)); // big deletion
        assert_eq!(analyzer.map_old_position_to_new(15, &edits), 20);
    }

    #[test]
    fn test_map_position_between_two_edits_shifted_by_first_only() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut edits = EditSet::new();
        edits.add(make_edit(5, 7, 8)); // shift +1 at [5,7)
        edits.add(make_edit(20, 22, 23)); // shift +1 at [20,22)
        // old_pos=10 is between the two edits — shifted only by first (+1)
        assert_eq!(analyzer.map_old_position_to_new(10, &edits), 11);
    }

    // --- try_register_match unit tests ---

    #[test]
    fn test_try_register_match_rejects_duplicate_new_pos() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut reuse_map = HashMap::new();

        let first = analyzer.try_register_match(&mut reuse_map, 0, 10, ReuseType::Direct, 0.9);
        assert!(first, "first registration should succeed");

        let dup = analyzer.try_register_match(&mut reuse_map, 5, 10, ReuseType::Direct, 0.95);
        assert!(!dup, "second registration for same new_pos must be rejected");
        // The map still has exactly one entry — old_pos=0 holds the claim
        assert_eq!(reuse_map.len(), 1);
        assert_eq!(reuse_map[&0].target_position, 10);
    }

    #[test]
    fn test_try_register_match_distinct_new_positions_both_succeed() {
        let analyzer = AdvancedReuseAnalyzer::new();
        let mut reuse_map = HashMap::new();

        assert!(analyzer.try_register_match(&mut reuse_map, 0, 10, ReuseType::Direct, 0.9));
        assert!(
            analyzer.try_register_match(&mut reuse_map, 5, 20, ReuseType::PositionShift, 0.85,)
        );
        assert_eq!(reuse_map.len(), 2);
    }
    #[test]
    fn tree_analysis_exposes_sorted_match_order() {
        let mut analysis = TreeAnalysis::new();
        let node = |start| {
            Node::new(
                NodeKind::Number { value: "10".to_string() },
                SourceLocation { start, end: start + 2 },
            )
        };

        for start in [30, 10, 20] {
            analysis.add_node_info(
                start,
                NodeAnalysisInfo {
                    node: node(start),
                    structural_hash: 0,
                    content_hash: 0,
                    depth: 0,
                    children_count: 0,
                },
            );
        }

        assert_eq!(
            analysis.node_info.keys().copied().collect::<Vec<_>>(),
            vec![10, 20, 30],
            "greedy matching must traverse byte positions in stable order"
        );
    }

    #[test]
    fn matching_reservation_is_stable_for_duplicate_nodes() {
        let old_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number { value: "10".to_string() },
                        SourceLocation { start: 10, end: 12 },
                    ),
                    Node::new(
                        NodeKind::Number { value: "10".to_string() },
                        SourceLocation { start: 20, end: 22 },
                    ),
                ],
            },
            SourceLocation { start: 0, end: 22 },
        );
        let new_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number { value: "10".to_string() },
                        SourceLocation { start: 100, end: 102 },
                    ),
                    Node::new(
                        NodeKind::Number { value: "10".to_string() },
                        SourceLocation { start: 110, end: 112 },
                    ),
                ],
            },
            SourceLocation { start: 0, end: 112 },
        );

        let mut analyzer = AdvancedReuseAnalyzer::new();
        let result = analyzer.analyze_reuse_opportunities(
            &old_tree,
            &new_tree,
            &EditSet::new(),
            &ReuseConfig::default(),
        );

        assert_eq!(result.reuse_map.get(&10).map(|s| s.target_position), Some(100));
        assert_eq!(result.reuse_map.get(&20).map(|s| s.target_position), Some(110));
    }
}

/// Shared fixtures for the content-hash proof.
#[cfg(test)]
mod hash_fixtures {
    use super::*;
    use perl_parser_core::{SourceLocation, ast::Node};

    /// Content hash of one subtree, through the same bottom-up pass production
    /// uses.
    pub(super) fn content_hash(node: &Node) -> u64 {
        ContentHashes::compute(node).get(node)
    }

    pub(super) fn loc() -> SourceLocation {
        SourceLocation { start: 0, end: 1 }
    }

    pub(super) fn total_nodes(node: &Node) -> usize {
        let mut count = 1usize;
        node.for_each_child(|child| count = count.saturating_add(total_nodes(child)));
        count
    }

    pub(super) fn number(value: &str) -> Node {
        Node::new(NodeKind::Number { value: value.to_string() }, loc())
    }

    pub(super) fn string(value: &str, interpolated: bool) -> Node {
        Node::new(NodeKind::String { value: value.to_string(), interpolated }, loc())
    }

    pub(super) fn variable(sigil: &str, name: &str) -> Node {
        Node::new(NodeKind::Variable { sigil: sigil.to_string(), name: name.to_string() }, loc())
    }

    pub(super) fn program(statements: Vec<Node>) -> Node {
        Node::new(NodeKind::Program { statements }, loc())
    }

    pub(super) fn block(statements: Vec<Node>) -> Node {
        Node::new(NodeKind::Block { statements }, loc())
    }

    pub(super) fn binary(op: &str, left: Node, right: Node) -> Node {
        Node::new(
            NodeKind::Binary { op: op.to_string(), left: Box::new(left), right: Box::new(right) },
            loc(),
        )
    }

    pub(super) fn unary(op: &str, operand: Node) -> Node {
        Node::new(NodeKind::Unary { op: op.to_string(), operand: Box::new(operand) }, loc())
    }

    pub(super) fn call(name: &str, args: Vec<Node>) -> Node {
        Node::new(NodeKind::FunctionCall { name: name.to_string(), args }, loc())
    }

    pub(super) fn if_node(condition: Node, then_branch: Node, else_branch: Option<Node>) -> Node {
        Node::new(
            NodeKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                elsif_branches: Vec::new(),
                else_branch: else_branch.map(Box::new),
                keyword: None,
            },
            loc(),
        )
    }

    /// A `try` block with one catch handler, optionally binding an exception
    /// variable.
    pub(super) fn try_node(binder: Option<&str>) -> Node {
        Node::new(
            NodeKind::Try {
                body: Box::new(block(vec![number("1")])),
                catch_blocks: vec![(
                    binder.map(|name| (name.to_string(), loc())),
                    Box::new(block(vec![number("2")])),
                )],
                finally_block: None,
            },
            loc(),
        )
    }

    /// An `if` carrying `elsif` arms.
    ///
    /// `elsif` is the one place the visit table reuses a `FieldId` within a
    /// single variant: `FieldId::CONDITION` labels both the leading condition
    /// and every `elsif` condition (`kind_schema/visit.rs`). Field names alone
    /// therefore cannot disambiguate position here, which makes these shapes
    /// the sharpest test of the ordered child fold.
    pub(super) fn if_with_elsifs(
        condition: Node,
        then_branch: Node,
        elsif_branches: Vec<(Node, Node)>,
        else_branch: Option<Node>,
    ) -> Node {
        Node::new(
            NodeKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                elsif_branches: elsif_branches
                    .into_iter()
                    .map(|(cond, body)| (Box::new(cond), Box::new(body)))
                    .collect(),
                else_branch: else_branch.map(Box::new),
                keyword: None,
            },
            loc(),
        )
    }

    /// A `try` whose second child is a catch handler, versus one whose second
    /// child is the identical block in the `finally` role.
    ///
    /// Same child count, same order, same child content — only the field role
    /// differs, which is the case that makes folding the role load-bearing.
    pub(super) fn try_with_second_child_as(role: SecondChildRole) -> Node {
        let handler = || Box::new(block(vec![number("7")]));
        let (catch_blocks, finally_block) = match role {
            SecondChildRole::Catch => (vec![(None, handler())], None),
            SecondChildRole::Finally => (Vec::new(), Some(handler())),
        };
        Node::new(
            NodeKind::Try { body: Box::new(block(vec![number("1")])), catch_blocks, finally_block },
            loc(),
        )
    }

    #[derive(Clone, Copy)]
    pub(super) enum SecondChildRole {
        Catch,
        Finally,
    }

    /// Wrap `leaf` in `depth` nested unary nodes.
    pub(super) fn nest(depth: usize, leaf: Node) -> Node {
        let mut node = leaf;
        for _ in 0..depth {
            node = unary("!", node);
        }
        node
    }

    /// A deliberately varied set of subtrees, including near-miss pairs that
    /// differ only in one payload, one role, or one arity.
    pub(super) fn corpus_nodes() -> Vec<Node> {
        vec![
            number("1"),
            number("2"),
            string("x", false),
            string("x", true),
            string("y", false),
            variable("$", "x"),
            variable("@", "x"),
            variable("$", "y"),
            program(Vec::new()),
            program(vec![number("1")]),
            program(vec![number("2")]),
            program(vec![number("1"), number("1")]),
            program(vec![number("1"), number("2")]),
            program(vec![number("2"), number("1")]),
            block(Vec::new()),
            block(vec![number("1")]),
            binary("+", number("1"), number("2")),
            binary("+", number("2"), number("1")),
            binary("-", number("1"), number("2")),
            unary("!", number("1")),
            unary("-", number("1")),
            call("foo", Vec::new()),
            call("bar", Vec::new()),
            call("foo", vec![number("1")]),
            call("foo", vec![number("1"), number("2")]),
            call("foo", vec![call("foo", vec![number("1")])]),
            if_node(number("1"), block(vec![number("2")]), None),
            if_node(number("1"), block(vec![number("2")]), Some(block(vec![number("3")]))),
            if_node(number("1"), block(vec![number("3")]), Some(block(vec![number("2")]))),
            try_node(None),
            try_node(Some("$err")),
            try_node(Some("$other")),
            try_with_second_child_as(SecondChildRole::Catch),
            try_with_second_child_as(SecondChildRole::Finally),
            // `elsif` reuses FieldId::CONDITION, so only the ordered fold
            // separates these.
            if_with_elsifs(number("1"), block(vec![number("2")]), Vec::new(), None),
            if_with_elsifs(
                number("1"),
                block(vec![number("2")]),
                vec![(number("3"), block(vec![number("4")]))],
                None,
            ),
            if_with_elsifs(
                number("1"),
                block(vec![number("2")]),
                vec![(number("4"), block(vec![number("3")]))],
                None,
            ),
            if_with_elsifs(
                number("1"),
                block(vec![number("2")]),
                vec![(number("3"), block(vec![number("4")]))],
                Some(block(vec![number("5")])),
            ),
            if_with_elsifs(
                number("1"),
                block(vec![number("2")]),
                vec![
                    (number("3"), block(vec![number("4")])),
                    (number("5"), block(vec![number("6")])),
                ],
                None,
            ),
            if_with_elsifs(
                number("1"),
                block(vec![number("2")]),
                vec![
                    (number("5"), block(vec![number("6")])),
                    (number("3"), block(vec![number("4")])),
                ],
                None,
            ),
            nest(3, number("1")),
            nest(3, number("2")),
            nest(4, number("1")),
        ]
    }
}

#[cfg(test)]
mod content_hash_tests {
    use super::hash_fixtures::*;
    use super::*;
    use perl_parser_core::{SourceLocation, ast::Node};

    /// The property the previous `to_sexp` hash provided, stated directly: two
    /// subtrees hash equal exactly when they render to the same S-expression.
    ///
    /// Soundness (same render => same hash) is what reuse selection depends on.
    /// Completeness (different render => different hash) is what keeps the new
    /// hash from proposing candidates the old one would not have.
    #[test]
    fn content_hash_agrees_with_sexp_equality_across_a_corpus() {
        let corpus = corpus_nodes();
        assert!(corpus.len() > 20, "corpus too small to discriminate");

        for (i, a) in corpus.iter().enumerate() {
            for (j, b) in corpus.iter().enumerate() {
                let same_render = a.to_sexp() == b.to_sexp();
                let same_hash = content_hash(a) == content_hash(b);
                assert_eq!(
                    same_render,
                    same_hash,
                    "corpus[{i}] vs corpus[{j}]: to_sexp equality {same_render} but hash equality \
                     {same_hash}\n  a = {}\n  b = {}",
                    a.to_sexp(),
                    b.to_sexp()
                );
            }
        }
    }

    #[test]
    fn identical_subtrees_built_independently_hash_equal() {
        assert_eq!(
            content_hash(&program(vec![binary("+", number("1"), number("2"))])),
            content_hash(&program(vec![binary("+", number("1"), number("2"))])),
            "structurally identical trees must hash equal regardless of allocation"
        );
    }

    #[test]
    fn source_location_alone_does_not_change_the_hash() {
        let here = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 0, end: 2 },
        );
        let moved = Node::new(
            NodeKind::Number { value: "42".to_string() },
            SourceLocation { start: 900, end: 902 },
        );
        assert_eq!(here.to_sexp(), moved.to_sexp(), "guard: the renderer omits spans");
        assert_eq!(
            content_hash(&here),
            content_hash(&moved),
            "content hash must stay position-independent, as position-shifted reuse relies on it"
        );
    }

    #[test]
    fn a_leaf_change_deep_in_the_tree_changes_the_root_hash() {
        // Proves child hashes are actually folded into the parent rather than
        // the parent hashing only its own payload.
        let deep_a = nest(12, number("1"));
        let deep_b = nest(12, number("2"));
        assert_ne!(
            content_hash(&deep_a),
            content_hash(&deep_b),
            "a change 12 levels down must reach the root hash"
        );
    }

    #[test]
    fn child_field_roles_are_load_bearing() {
        // Same children, different roles: `if (1) {2} else {3}` must not hash
        // like `if (1) {3} else {2}`.
        let then_first = if_node(number("1"), number("2"), Some(number("3")));
        let else_first = if_node(number("1"), number("3"), Some(number("2")));
        assert_ne!(
            content_hash(&then_first),
            content_hash(&else_first),
            "swapping then/else arms must change the hash"
        );
    }

    #[test]
    fn child_field_roles_are_load_bearing_when_order_and_content_match() {
        // A `try` with one catch handler and no finally, versus one with no
        // catch and an identical finally block: two children, same order, same
        // content. Only the field role tells them apart.
        let as_catch = try_with_second_child_as(SecondChildRole::Catch);
        let as_finally = try_with_second_child_as(SecondChildRole::Finally);

        assert_ne!(
            as_catch.to_sexp(),
            as_finally.to_sexp(),
            "guard: the renderer distinguishes the catch and finally roles"
        );
        assert_ne!(
            content_hash(&as_catch),
            content_hash(&as_finally),
            "identical children in identical order must not hash alike across field roles"
        );
    }

    #[test]
    fn elsif_arms_are_separated_by_fold_order_not_field_names() {
        // The visit table labels the leading condition and every `elsif`
        // condition with the same `FieldId::CONDITION`, so field names alone
        // cannot tell these apart — only the ordered child fold can.
        let a = if_with_elsifs(
            number("1"),
            block(vec![number("2")]),
            vec![(number("3"), block(vec![number("4")]))],
            None,
        );
        let swapped_within_arm = if_with_elsifs(
            number("1"),
            block(vec![number("2")]),
            vec![(number("4"), block(vec![number("3")]))],
            None,
        );
        let two_arms_reordered = if_with_elsifs(
            number("1"),
            block(vec![number("2")]),
            vec![(number("5"), block(vec![number("6")])), (number("3"), block(vec![number("4")]))],
            None,
        );
        let two_arms = if_with_elsifs(
            number("1"),
            block(vec![number("2")]),
            vec![(number("3"), block(vec![number("4")])), (number("5"), block(vec![number("6")]))],
            None,
        );

        assert_ne!(a.to_sexp(), swapped_within_arm.to_sexp(), "guard: renders differ");
        assert_ne!(
            content_hash(&a),
            content_hash(&swapped_within_arm),
            "swapping an elsif condition with its body must change the hash"
        );
        assert_ne!(
            content_hash(&two_arms),
            content_hash(&two_arms_reordered),
            "reordering two elsif arms must change the hash"
        );
    }

    #[test]
    fn child_arity_is_load_bearing() {
        let one = program(vec![number("1")]);
        let two = program(vec![number("1"), number("1")]);
        assert_ne!(
            content_hash(&one),
            content_hash(&two),
            "a repeated child must not fold into the same hash as a single child"
        );
    }

    #[test]
    fn catch_binder_names_are_load_bearing() {
        // The renderer nests a `try` block's catch binder under the catch child
        // rather than under the `try` node, so a depth-limited render of the
        // `try` node alone does not see it.
        let with_err = try_node(Some("$err"));
        let with_other = try_node(Some("$other"));
        let without = try_node(None);

        assert_ne!(
            with_err.to_sexp(),
            with_other.to_sexp(),
            "guard: the renderer distinguishes catch binders"
        );
        assert_ne!(
            content_hash(&with_err),
            content_hash(&with_other),
            "catch binder name must reach the hash"
        );
        assert_ne!(
            content_hash(&with_err),
            content_hash(&without),
            "a present binder must not hash like an absent one"
        );
    }

    #[test]
    fn every_node_is_hashed_exactly_once_per_tree() {
        let tree = nest(40, number("1"));
        let hashes = ContentHashes::compute(&tree);
        assert_eq!(hashes.len(), total_nodes(&tree), "one hash entry per node");
    }

    /// The falsifier for the defect itself: no whole-subtree serialization.
    ///
    /// Each node's payload render must visit exactly that one node, so the
    /// renderer's total node visits equal the tree's node count. Hashing
    /// `node.to_sexp()` per node instead makes each render walk a whole
    /// subtree, so the total becomes the sum of all subtree sizes — quadratic
    /// in the depth of a nested chain.
    #[test]
    fn payload_rendering_never_walks_a_whole_subtree() {
        for depth in [1usize, 8, 40] {
            let tree = nest(depth, number("1"));
            let nodes = total_nodes(&tree);
            let hashes = ContentHashes::compute(&tree);

            assert_eq!(
                hashes.rendered_nodes, nodes,
                "depth {depth}: the renderer visited {} nodes for a {nodes}-node tree; payload \
                 rendering must not descend into children",
                hashes.rendered_nodes
            );

            // Guard that the measurement is live: whole-subtree rendering of
            // this same chain would have cost quadratically more.
            let whole_subtree_cost = nodes * (nodes + 1) / 2;
            assert!(
                depth == 0 || hashes.rendered_nodes < whole_subtree_cost || nodes == 1,
                "depth {depth}: cost is indistinguishable from whole-subtree rendering"
            );
        }
    }

    /// The depth-independence measurement #9608 asks for, taken as work rather
    /// than wall clock so it is deterministic in CI.
    ///
    /// `nodes_analyzed` still grows with `max_analysis_depth` — that is the
    /// live control proving the measurement would notice if content hashing
    /// became depth-dependent again.
    #[test]
    fn content_hashing_cost_does_not_grow_with_analysis_depth() {
        let tree = nest(30, number("1"));
        let expected_hashed = total_nodes(&tree);

        let mut analyzed_by_depth = Vec::new();
        for depth in [1usize, 5, 10, 30] {
            let config = ReuseConfig { max_analysis_depth: depth, ..ReuseConfig::default() };

            // Hashing work for a fixed tree is the node count at every depth.
            assert_eq!(
                ContentHashes::compute(&tree).len(),
                expected_hashed,
                "content hashing work must not depend on max_analysis_depth"
            );

            let mut analyzer = AdvancedReuseAnalyzer::new();
            analyzer.analyze_reuse_opportunities(&tree, &tree, &EditSet::new(), &config);
            analyzed_by_depth.push(analyzer.analysis_stats.nodes_analyzed);
        }

        assert!(
            analyzed_by_depth.windows(2).any(|w| w[1] > w[0]),
            "control failed: the analysis walk should still deepen with max_analysis_depth, \
             otherwise this test cannot detect depth-dependent hashing ({analyzed_by_depth:?})"
        );
    }
}
