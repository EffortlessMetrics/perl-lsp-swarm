//! Experimental document holder that fail-closes to a full fresh parse.
//!
//! `IncrementalDocument` is the #7292 experimental generation (`production_eligible:
//! false`). It is not a second production incremental engine. Retained edits apply
//! the source change, then rebuild the tree and range cache from `Parser::new`
//! with [`ParseSnapshotStrategy::IncrementalFullFallback`]. Invalid edits are
//! refused without advancing version, mutating the tree, or rewriting caches.

use super::ParseSnapshotStrategy;
use super::incremental_edit::{IncrementalEdit, IncrementalEditSet};
use perl_parser_core::{
    ast::{Node, NodeKind},
    error::{ParseError, ParseResult},
    parser::Parser,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tracing::debug;

/// Why an experimental `IncrementalDocument` edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum IncrementalEditRefusal {
    /// `start_byte` is greater than `old_end_byte`.
    #[error("backward range")]
    BackwardRange,
    /// The old range extends past the current source.
    #[error("out of range")]
    OutOfRange,
    /// The old range is not on UTF-8 character boundaries.
    #[error("not a UTF-8 character boundary")]
    NotCharBoundary,
}

/// Typed failure for experimental `IncrementalDocument` mutation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum IncrementalDocumentError {
    /// The edit was refused before any document generation was committed.
    #[error(
        "invalid incremental edit {start_byte}..{old_end_byte} for source length {source_len}: {reason}"
    )]
    InvalidEdit {
        /// Byte start of the refused edit.
        start_byte: usize,
        /// Byte end of the refused old range.
        old_end_byte: usize,
        /// Source length against which the edit was checked.
        source_len: usize,
        /// Why the edit was unmappable.
        reason: IncrementalEditRefusal,
        /// Index in a batch, when the refusal came from `apply_edits`.
        index: Option<usize>,
    },
    /// Fresh parse of the edited source failed.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

impl From<IncrementalDocumentError> for ParseError {
    fn from(error: IncrementalDocumentError) -> Self {
        match error {
            IncrementalDocumentError::Parse(error) => error,
            invalid @ IncrementalDocumentError::InvalidEdit { start_byte, .. } => {
                ParseError::SyntaxError { message: invalid.to_string(), location: start_byte }
            }
        }
    }
}

/// Experimental document with a rebuilt-on-edit subtree cache.
///
/// Edits do not patch leaves or shift cached subtrees. Successful mutation
/// always takes a full fresh parse and rebuilds range keys for the new source
/// generation.
#[derive(Debug, Clone)]
pub struct IncrementalDocument {
    /// Current parsed tree
    pub root: Arc<Node>,
    /// Source text
    pub source: String,
    /// Version number for tracking committed generations
    pub version: u64,
    /// Cache of subtrees for the current generation only
    pub subtree_cache: SubtreeCache,
    /// Accounting for the last construction or committed edit
    pub metrics: ParseMetrics,
    /// Strategy that produced the current generation
    pub last_strategy: ParseSnapshotStrategy,
}

/// Cache for current-generation subtrees.
///
/// Range keys are invalidated on every successful edit. Cache hits are not
/// parser work avoided and must not be reported as `nodes_reused`.
#[derive(Debug, Clone, Default)]
pub struct SubtreeCache {
    /// Maps content hash to subtree for content-based lookup
    pub by_content: HashMap<u64, Arc<Node>>,
    /// Maps byte range to subtree for the current source generation
    pub by_range: HashMap<(usize, usize), Arc<Node>>,
    /// LRU queue for cache eviction
    pub lru: VecDeque<u64>,
    /// Critical symbols that should be preserved longer
    pub critical_symbols: HashMap<u64, SymbolPriority>,
    /// Maximum cache size
    pub max_size: usize,
}

/// Priority levels for symbols in cache eviction
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolPriority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// Accounting for the last construction or committed edit.
///
/// `nodes_reused` and `cache_hits` remain for historical field compatibility.
/// After a retained edit they are always zero: a full fresh parse is not
/// retained identity, and rebuilding the cache is not parser work avoided
/// (#7072 / #7081).
#[derive(Debug, Clone, Default)]
pub struct ParseMetrics {
    pub last_parse_time_ms: f64,
    pub nodes_reused: usize,
    pub nodes_reparsed: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

impl IncrementalDocument {
    /// Create a new incremental document from a full fresh parse.
    pub fn new(source: String) -> ParseResult<Self> {
        let start = Instant::now();
        let mut parser = Parser::new(&source);
        let root = parser.parse()?;

        let mut doc = IncrementalDocument {
            root: Arc::new(root),
            source,
            version: 0,
            subtree_cache: SubtreeCache::new(1000),
            metrics: ParseMetrics::default(),
            last_strategy: ParseSnapshotStrategy::Fresh,
        };

        doc.metrics.last_parse_time_ms = start.elapsed().as_secs_f64() * 1000.0;
        doc.metrics.nodes_reparsed = doc.count_nodes(&doc.root);
        doc.cache_subtrees();

        Ok(doc)
    }

    /// Apply one edit by rewriting source and taking a full fresh parse.
    ///
    /// Invalid edits are refused without advancing `version` or touching the
    /// tree, cache, metrics, or strategy.
    pub fn apply_edit(&mut self, edit: IncrementalEdit) -> Result<(), IncrementalDocumentError> {
        let start = Instant::now();
        let new_source = self.mapped_source(&edit, None)?;
        self.commit_fresh_parse(new_source, ParseSnapshotStrategy::IncrementalFullFallback, start)
    }

    /// Apply a batch by rewriting source and taking a full fresh parse.
    ///
    /// Any unmappable edit refuses the whole batch atomically. Overlapping but
    /// individually mappable edits take the explicit `apply_to_string` source
    /// fallback, then a full fresh parse. Empty batches are no-ops.
    pub fn apply_edits(
        &mut self,
        edits: &IncrementalEditSet,
    ) -> Result<(), IncrementalDocumentError> {
        if edits.edits.is_empty() {
            return Ok(());
        }

        for (index, edit) in edits.edits.iter().enumerate() {
            if let Some(reason) = Self::edit_refusal(&self.source, edit) {
                return Err(Self::invalid_edit(edit, self.source.len(), reason, Some(index)));
            }
        }

        let start = Instant::now();
        let new_source = if let Some(sorted_edits) = edits.normalize_for_source(&self.source) {
            let mut new_source = self.source.clone();
            for edit in &sorted_edits {
                if !self.apply_edit_in_place(&mut new_source, edit) {
                    return Err(Self::invalid_edit(
                        edit,
                        new_source.len(),
                        IncrementalEditRefusal::OutOfRange,
                        None,
                    ));
                }
            }
            new_source
        } else {
            edits.apply_to_string(&self.source)
        };

        self.commit_fresh_parse(new_source, ParseSnapshotStrategy::IncrementalFullFallback, start)
    }

    fn commit_fresh_parse(
        &mut self,
        new_source: String,
        strategy: ParseSnapshotStrategy,
        start: Instant,
    ) -> Result<(), IncrementalDocumentError> {
        let mut parser = Parser::new(&new_source);
        let new_root = parser.parse()?;
        let next_version =
            self.version.checked_add(1).ok_or(IncrementalDocumentError::InvalidEdit {
                start_byte: 0,
                old_end_byte: 0,
                source_len: new_source.len(),
                reason: IncrementalEditRefusal::OutOfRange,
                index: None,
            })?;

        self.source = new_source;
        self.root = Arc::new(new_root);
        self.version = next_version;
        self.last_strategy = strategy;
        self.metrics = ParseMetrics {
            last_parse_time_ms: start.elapsed().as_secs_f64() * 1000.0,
            nodes_reused: 0,
            nodes_reparsed: self.count_nodes(&self.root),
            cache_hits: 0,
            cache_misses: 0,
        };
        self.cache_subtrees();
        Ok(())
    }

    fn mapped_source(
        &self,
        edit: &IncrementalEdit,
        index: Option<usize>,
    ) -> Result<String, IncrementalDocumentError> {
        let (start, end) = self.map_edit_range(&self.source, edit, index)?;
        let mut result = String::with_capacity(self.source.len() + edit.new_text.len());
        result.push_str(&self.source[..start]);
        result.push_str(&edit.new_text);
        result.push_str(&self.source[end..]);
        Ok(result)
    }

    fn apply_edit_in_place(&self, source: &mut String, edit: &IncrementalEdit) -> bool {
        let Some((start, end)) = Self::try_map_edit_range(source, edit) else {
            return false;
        };
        source.replace_range(start..end, &edit.new_text);
        true
    }

    fn map_edit_range(
        &self,
        source: &str,
        edit: &IncrementalEdit,
        index: Option<usize>,
    ) -> Result<(usize, usize), IncrementalDocumentError> {
        match Self::try_map_edit_range(source, edit) {
            Some(range) => Ok(range),
            None => {
                let reason =
                    Self::edit_refusal(source, edit).unwrap_or(IncrementalEditRefusal::OutOfRange);
                Err(Self::invalid_edit(edit, source.len(), reason, index))
            }
        }
    }

    fn try_map_edit_range(source: &str, edit: &IncrementalEdit) -> Option<(usize, usize)> {
        if Self::edit_refusal(source, edit).is_some() {
            None
        } else {
            Some((edit.start_byte, edit.old_end_byte))
        }
    }

    fn edit_refusal(source: &str, edit: &IncrementalEdit) -> Option<IncrementalEditRefusal> {
        if edit.start_byte > edit.old_end_byte {
            Some(IncrementalEditRefusal::BackwardRange)
        } else if edit.old_end_byte > source.len() {
            Some(IncrementalEditRefusal::OutOfRange)
        } else if !source.is_char_boundary(edit.start_byte)
            || !source.is_char_boundary(edit.old_end_byte)
        {
            Some(IncrementalEditRefusal::NotCharBoundary)
        } else {
            None
        }
    }

    fn invalid_edit(
        edit: &IncrementalEdit,
        source_len: usize,
        reason: IncrementalEditRefusal,
        index: Option<usize>,
    ) -> IncrementalDocumentError {
        IncrementalDocumentError::InvalidEdit {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            source_len,
            reason,
            index,
        }
    }

    /// Cache subtrees for the current source generation only.
    fn cache_subtrees(&mut self) {
        self.subtree_cache.clear();
        let root = self.root.clone();
        self.cache_node(&root);
    }

    fn cache_node(&mut self, node: &Node) {
        let range = (node.location.start, node.location.end);
        self.subtree_cache.by_range.insert(range, Arc::new(node.clone()));

        let hash = self.hash_node(node);
        let priority = self.get_symbol_priority(node);

        self.subtree_cache.by_content.insert(hash, Arc::new(node.clone()));
        self.subtree_cache.critical_symbols.insert(hash, priority);
        self.subtree_cache.lru.push_back(hash);
        self.subtree_cache.evict_if_needed();

        node.for_each_child(|child| self.cache_node(child));
    }

    /// Generate hash for a node (for content-based caching)
    fn hash_node(&self, node: &Node) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        std::mem::discriminant(&node.kind).hash(&mut hasher);

        match &node.kind {
            NodeKind::Number { value } => value.hash(&mut hasher),
            NodeKind::String { value, .. } => value.hash(&mut hasher),
            NodeKind::VString { value } => value.hash(&mut hasher),
            NodeKind::Identifier { name } => name.hash(&mut hasher),
            _ => {}
        }

        hasher.finish()
    }

    fn count_nodes(&self, node: &Node) -> usize {
        let mut count = 1;
        node.for_each_child(|child| count += self.count_nodes(child));
        count
    }

    /// Determine the priority of a symbol for cache eviction
    fn get_symbol_priority(&self, node: &Node) -> SymbolPriority {
        match &node.kind {
            NodeKind::Package { .. } => SymbolPriority::Critical,
            NodeKind::Use { .. } | NodeKind::No { .. } => SymbolPriority::Critical,
            NodeKind::Subroutine { .. } => SymbolPriority::Critical,
            NodeKind::FunctionCall { .. } => SymbolPriority::High,
            NodeKind::Variable { .. } => SymbolPriority::High,
            NodeKind::VariableDeclaration { .. } => SymbolPriority::High,
            NodeKind::Block { .. } => SymbolPriority::Medium,
            NodeKind::If { .. } | NodeKind::While { .. } | NodeKind::For { .. } => {
                SymbolPriority::Medium
            }
            NodeKind::Assignment { .. } => SymbolPriority::Medium,
            NodeKind::Number { .. } | NodeKind::String { .. } | NodeKind::VString { .. } => {
                SymbolPriority::Low
            }
            NodeKind::Binary { .. } | NodeKind::Unary { .. } => SymbolPriority::Low,
            _ => SymbolPriority::Medium,
        }
    }

    /// Get current parse tree
    pub fn tree(&self) -> &Node {
        &self.root
    }

    /// Get current source text
    pub fn text(&self) -> &str {
        &self.source
    }

    /// Get accounting for the last construction or committed edit
    pub fn metrics(&self) -> &ParseMetrics {
        &self.metrics
    }

    /// Strategy that produced the current generation.
    pub fn last_strategy(&self) -> ParseSnapshotStrategy {
        self.last_strategy
    }

    /// Set maximum cache size
    pub fn set_cache_max_size(&mut self, max_size: usize) {
        self.subtree_cache.set_max_size(max_size);
    }
}

impl SubtreeCache {
    fn new(max_size: usize) -> Self {
        SubtreeCache {
            by_content: HashMap::new(),
            by_range: HashMap::new(),
            lru: VecDeque::new(),
            critical_symbols: HashMap::new(),
            max_size,
        }
    }

    fn clear(&mut self) {
        self.by_content.clear();
        self.by_range.clear();
        self.lru.clear();
        self.critical_symbols.clear();
    }

    fn evict_if_needed(&mut self) {
        while self.by_content.len() > self.max_size {
            if let Some(hash) = self.find_least_important_entry() {
                debug!(
                    "Evicting cache entry with hash {} (priority: {:?})",
                    hash,
                    self.critical_symbols.get(&hash).copied().unwrap_or(SymbolPriority::Low)
                );
                self.by_content.remove(&hash);
                self.critical_symbols.remove(&hash);
                self.lru.retain(|&h| h != hash);
            } else if let Some(hash) = self.lru.pop_front() {
                debug!("Fallback eviction for hash {}", hash);
                self.by_content.remove(&hash);
                self.critical_symbols.remove(&hash);
            } else {
                break;
            }
        }
    }

    /// Find the least important cache entry for eviction.
    fn find_least_important_entry(&self) -> Option<u64> {
        let mut best: Option<(u64, SymbolPriority)> = None;

        for &hash in &self.lru {
            let priority = self.critical_symbols.get(&hash).copied().unwrap_or(SymbolPriority::Low);

            match best {
                None => best = Some((hash, priority)),
                Some((_, best_priority)) => {
                    if priority < best_priority {
                        best = Some((hash, priority));
                    }
                }
            }
        }

        best.map(|(hash, _)| hash)
    }

    fn set_max_size(&mut self, max_size: usize) {
        self.max_size = max_size;
        self.evict_if_needed();
    }
}

#[cfg(test)]
mod tests {
    use super::super::incremental_edit::IncrementalEdit;
    use super::*;

    fn require_find(source: &str, needle: &str) -> Result<usize, IncrementalDocumentError> {
        source.find(needle).ok_or_else(|| {
            ParseError::SyntaxError {
                message: format!("test source should contain {needle:?}"),
                location: 0,
            }
            .into()
        })
    }

    #[test]
    fn test_incremental_single_token_edit() -> Result<(), IncrementalDocumentError> {
        let source = r#"
            my $x = 42;
            my $y = 100;
            print $x + $y;
        "#;

        let mut doc = IncrementalDocument::new(source.to_string())?;
        let pos = require_find(source, "42")?;
        doc.apply_edit(IncrementalEdit::new(pos + 1, pos + 2, "3".to_string()))?;

        let mut parser = Parser::new(&doc.source);
        let fresh = parser.parse()?;
        assert_eq!(*doc.root, fresh);
        assert_eq!(doc.metrics.nodes_reused, 0);
        assert_eq!(doc.last_strategy, ParseSnapshotStrategy::IncrementalFullFallback);
        assert!(doc.source.contains("43"));

        Ok(())
    }

    #[test]
    fn test_incremental_multiple_edits() -> Result<(), IncrementalDocumentError> {
        let source = r#"
            sub calculate {
                my $a = 10;
                my $b = 20;
                return $a + $b;
            }
        "#;

        let mut doc = IncrementalDocument::new(source.to_string())?;
        let mut edits = IncrementalEditSet::new();
        let pos_10 = require_find(source, "10")?;
        edits.add(IncrementalEdit::new(pos_10, pos_10 + 2, "15".to_string()));
        let pos_20 = require_find(source, "20")?;
        edits.add(IncrementalEdit::new(pos_20, pos_20 + 2, "25".to_string()));
        doc.apply_edits(&edits)?;

        let critical_count = doc
            .subtree_cache
            .critical_symbols
            .values()
            .filter(|&p| *p == SymbolPriority::Critical)
            .count();
        assert!(critical_count > 0, "Should preserve critical symbols during batch edits");

        let mut parser = Parser::new(&doc.source);
        let fresh = parser.parse()?;
        assert_eq!(*doc.root, fresh);
        assert_eq!(doc.metrics.nodes_reused, 0);
        assert!(doc.source.contains("15"));
        assert!(doc.source.contains("25"));

        Ok(())
    }

    #[test]
    fn test_cache_eviction() -> ParseResult<()> {
        let source = "my $x = 1;";
        let doc = IncrementalDocument::new(source.to_string())?;
        assert!(!doc.subtree_cache.by_range.is_empty());
        assert!(!doc.subtree_cache.by_content.is_empty());
        Ok(())
    }

    #[test]
    fn test_symbol_priority_classification() -> ParseResult<()> {
        let source = r#"
            package TestPkg;
            use strict;

            sub test_func {
                my $var = 42;
                if ($var > 0) {
                    return $var + 1;
                }
            }
        "#;
        let doc = IncrementalDocument::new(source.to_string())?;
        let priorities: std::collections::HashSet<_> =
            doc.subtree_cache.critical_symbols.values().cloned().collect();
        assert!(
            priorities.contains(&SymbolPriority::Critical),
            "Should classify package/use/sub as critical"
        );
        assert!(
            priorities.contains(&SymbolPriority::High),
            "Should classify variables as high priority"
        );
        assert!(
            priorities.contains(&SymbolPriority::Low)
                || priorities.contains(&SymbolPriority::Medium),
            "Should have lower priority symbols"
        );
        Ok(())
    }

    #[test]
    fn test_cache_respects_max_size() -> Result<(), IncrementalDocumentError> {
        let source = "my $x = 1; my $y = 2; my $z = 3;";
        let mut doc = IncrementalDocument::new(source.to_string())?;
        assert!(doc.subtree_cache.by_content.len() > 1);
        doc.set_cache_max_size(1);
        assert!(doc.subtree_cache.by_content.len() <= 1);

        let pos = require_find(source, "1")?;
        doc.apply_edit(IncrementalEdit::new(pos, pos + 1, "10".to_string()))?;
        assert!(doc.subtree_cache.by_content.len() <= 1);
        Ok(())
    }

    #[test]
    fn test_cache_priority_preservation() -> Result<(), IncrementalDocumentError> {
        let source = r#"
            package MyPackage;
            use strict;
            use warnings;

            sub process {
                my $x = 42;
                my $y = "hello";
                return $x + 1;
            }
        "#;
        let mut doc = IncrementalDocument::new(source.to_string())?;
        let initial_cache_size = doc.subtree_cache.by_content.len();
        assert!(initial_cache_size > 3, "Should have multiple cached nodes");
        doc.set_cache_max_size(3);
        assert!(doc.subtree_cache.by_content.len() <= 3);

        let has_critical_symbols = doc
            .subtree_cache
            .critical_symbols
            .values()
            .cloned()
            .any(|p| p == SymbolPriority::Critical);
        assert!(has_critical_symbols, "Should preserve critical symbols like package/use/sub");

        let pos = require_find(source, "42")?;
        doc.apply_edit(IncrementalEdit::new(pos, pos + 2, "100".to_string()))?;
        assert!(doc.subtree_cache.by_content.len() <= 3);
        let has_critical_after_edit = doc
            .subtree_cache
            .critical_symbols
            .values()
            .cloned()
            .any(|p| p == SymbolPriority::Critical);
        assert!(has_critical_after_edit, "Should preserve critical symbols after edit");
        Ok(())
    }

    #[test]
    fn test_workspace_symbol_cache_preservation() -> ParseResult<()> {
        let source = r#"
            package TestModule;

            sub exported_function { }
            sub internal_helper { }

            my $global_var = "test";
        "#;
        let mut doc = IncrementalDocument::new(source.to_string())?;
        doc.set_cache_max_size(2);
        let package_preserved = doc
            .subtree_cache
            .by_content
            .values()
            .any(|node| matches!(node.kind, NodeKind::Package { .. }));
        assert!(package_preserved, "Package declaration should be preserved for workspace symbols");
        Ok(())
    }

    #[test]
    fn test_completion_metadata_preservation() -> ParseResult<()> {
        let source = r#"
            use Data::Dumper;
            use List::Util qw(first max);

            sub calculate {
                my ($input, $multiplier) = @_;
                return $input * $multiplier;
            }
        "#;
        let mut doc = IncrementalDocument::new(source.to_string())?;
        doc.set_cache_max_size(4);
        let use_statements_count = doc
            .subtree_cache
            .by_content
            .values()
            .filter(|node| matches!(node.kind, NodeKind::Use { .. }))
            .count();
        assert!(
            use_statements_count >= 1,
            "Use statements should be preserved for completion metadata"
        );
        let function_preserved = doc
            .subtree_cache
            .by_content
            .values()
            .any(|node| matches!(node.kind, NodeKind::Subroutine { .. }));
        assert!(function_preserved, "Function definitions should be preserved for completion");
        Ok(())
    }

    #[test]
    fn test_code_lens_reference_preservation() -> ParseResult<()> {
        let source = r#"
            package MyClass;

            sub new {
                my $class = shift;
                return bless {}, $class;
            }

            sub process_data {
                my ($self, $data) = @_;
                return $self->transform($data);
            }
        "#;
        let mut doc = IncrementalDocument::new(source.to_string())?;
        doc.set_cache_max_size(3);
        let critical_nodes = doc
            .subtree_cache
            .by_content
            .values()
            .filter(|node| {
                matches!(node.kind, NodeKind::Package { .. } | NodeKind::Subroutine { .. })
            })
            .count();
        assert!(critical_nodes >= 2, "Should preserve package and key subroutines for code lens");
        Ok(())
    }
}
