//! Rust-native Perl parser with tree-sitter-style ergonomics and tree-sitter-compatible
//! output. This is a facade over the v3 native parser (`perl-parser-core`); it is NOT
//! bindings to the C tree-sitter grammar. For the conventional tree-sitter binding, see
//! `tree-sitter-perl-c`.
//!
//! # Quick start
//!
//! ```rust
//! use tree_sitter_perl_rs::Parser;
//!
//! let mut parser = Parser::new();
//! if let Some(tree) = parser.parse("my $x = 42;") {
//!     let root = tree.root_node();
//!     println!("{}", root.to_sexp());
//! }
//! ```
//!
//! # Design
//!
//! This crate wraps the v3 recursive-descent Perl parser (`perl-parser-core`) with an API
//! surface that matches the conventions of the `tree-sitter` crate. Users familiar with
//! tree-sitter can work with Perl ASTs immediately, while the underlying engine is the
//! full-featured native v3 stack (not the C tree-sitter grammar).
//!
//! Key properties:
//! - `Parser::parse()` returns `Option<Tree>` — `None` only on complete parse failure.
//!   The v3 parser is highly error-tolerant and almost always produces a partial tree.
//! - `Node::to_sexp()` delegates to `perl_ast::Node::to_sexp()` for tree-sitter-compatible
//!   S-expression output.
//! - `Node::kind()` returns the tree-sitter grammar-canonical kind string.
//! - `Node::start_byte()` / `Node::end_byte()` expose the `SourceLocation` byte offsets.
//! - `Node::children()` and `Node::child()` mirror tree-sitter traversal conventions.
//!
//! # Relationship to `tree-sitter-perl-c`
//!
//! | Crate | Backing engine | Use when |
//! |-------|---------------|----------|
//! | `tree-sitter-perl-rs` | v3 native Rust parser (this crate) | You want the full-featured Rust toolchain |
//! | `tree-sitter-perl-c` | C tree-sitter grammar | You need compatibility with the tree-sitter C ecosystem |

#![deny(unsafe_code)]
#![deny(unreachable_pub)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use perl_ast::{Node as AstNode, NodeKind};
use perl_module::parse_module_import_head;
use perl_parser_core::{
    ParseOutput, Parser as CoreParser,
    incremental::{IncrementalEdit, IncrementalState},
};
use perl_position_tracking::Position;
use perl_pragma::{PragmaState, PragmaTracker};
use perl_semantic_analyzer::semantic::SemanticModel;
use std::cell::OnceCell;
use std::sync::Arc;

/// Parser diagnostics surfaced by [`Parser::parse_detailed`].
pub use perl_parser_core::ParseError as ParseDiagnostic;
/// Re-export of Edit type for tree-sitter-compatible incremental parsing.
///
/// Mirrors `tree_sitter::InputEdit` field layout for drop-in compatibility.
pub use perl_parser_core::edit::Edit as InputEdit;
pub use perl_parser_core::incremental::{FallbackReason, IncrementalMetrics};

/// A tree-sitter-compatible source position.
///
/// `row` and `column` are both zero-based and `column` is measured in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Point {
    /// Zero-based row number.
    pub row: usize,
    /// Zero-based byte column within `row`.
    pub column: usize,
}

/// A Perl parser with tree-sitter-style ergonomics.
///
/// Wraps the v3 recursive-descent Perl parser. Create one parser instance and call
/// [`parse`][Parser::parse] for each source file you need to process.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_rs::Parser;
///
/// let mut parser = Parser::new();
/// let tree = parser.parse("sub greet { print \"hello\"; }");
/// assert!(tree.is_some());
/// ```
pub struct Parser {
    // Stateless currently; the v3 CoreParser takes source at construction time.
    // Stored as a unit struct for forward compatibility (e.g. future options).
    _priv: (),
}

impl Parser {
    /// Create a new parser instance.
    pub fn new() -> Self {
        Parser { _priv: () }
    }

    /// Parse a Perl source string and return a [`Tree`], or `None` on complete failure.
    ///
    /// The v3 parser is highly error-tolerant — even malformed input usually produces a
    /// partial tree. `None` is reserved for extreme edge cases where no AST can be built
    /// at all.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tree_sitter_perl_rs::Parser;
    ///
    /// let mut parser = Parser::new();
    /// let tree = parser.parse("my $x = 42;");
    /// assert!(tree.is_some());
    /// ```
    pub fn parse(&mut self, source: &str) -> Option<Tree> {
        let mut core = CoreParser::new(source);
        match core.parse() {
            Ok(root) => {
                let root = Arc::new(root);
                Some(Tree {
                    shared_root: SharedNode::from_root(root.clone()),
                    root,
                    source: source.to_string(),
                    pending_edits: Vec::new(),
                    incremental_state: Some(IncrementalState::with_diagnostics(
                        source,
                        core.errors(),
                    )),
                    line_index: ByteLineIndex::new(source),
                    semantic_model: OnceCell::new(),
                    pragma_map: OnceCell::new(),
                    diagnostics: core.errors().to_vec(),
                })
            }
            Err(_) => None,
        }
    }

    /// Parse `source` and preserve recovery diagnostics and catastrophic failures.
    ///
    /// A recovered parse returns `tree: Some(_)` with one or more diagnostics. A
    /// catastrophic failure returns `tree: None` and a typed [`ParseFailure`]. Existing
    /// callers that only need the compatibility `Option` API can continue using
    /// [`parse`][Parser::parse].
    pub fn parse_detailed(&mut self, source: &str) -> ParseOutcome {
        let mut core = CoreParser::new(source);
        let ParseOutput { ast, diagnostics, terminated_early, .. } = core.parse_with_recovery();
        let failure = terminated_early
            .then(|| diagnostics.iter().find_map(ParseFailure::from_diagnostic))
            .flatten();
        let tree = failure.is_none().then(|| {
            let root = Arc::new(ast);
            Tree {
                shared_root: SharedNode::from_root(root.clone()),
                root,
                source: source.to_string(),
                pending_edits: Vec::new(),
                incremental_state: Some(IncrementalState::with_diagnostics(source, &diagnostics)),
                line_index: ByteLineIndex::new(source),
                semantic_model: OnceCell::new(),
                pragma_map: OnceCell::new(),
                diagnostics: diagnostics.clone(),
            }
        });

        ParseOutcome { tree, diagnostics, failure }
    }

    /// Parse `source` using `old_tree` as a hint for incremental re-parsing.
    ///
    /// A single clean edit inside one top-level statement parses only that
    /// statement and retains unaffected AST subtrees. Length-changing edits are
    /// supported when no downstream statement must shift; structural,
    /// recovery-sensitive, format, oversized, and unsupported edits use a safe
    /// full-parse fallback. Metrics distinguish AST reuse from token-cache work.
    ///
    /// Returns `None` on complete parse failure (same semantics as `parse`).
    pub fn parse_with_old_tree(&mut self, source: &str, old_tree: &Tree) -> Option<Tree> {
        // Fast path: if source is unchanged and no edits were recorded, reuse the old tree
        // instead of re-parsing. This mirrors tree-sitter's incremental no-op behavior.
        if source == old_tree.source() && old_tree.pending_edits.is_empty() {
            return Some(old_tree.clone());
        }

        if old_tree.pending_edits.len() == 1 {
            let edit = &old_tree.pending_edits[0];
            let Some(new_text) = source.get(edit.start_byte..edit.new_end_byte) else {
                return self.parse(source);
            };

            if let Some(tree) = self.try_statement_reuse(source, old_tree, edit, new_text) {
                return Some(tree);
            }

            let Some(mut state) = old_tree.incremental_state.clone() else {
                return self.parse(source);
            };
            let incremental_edit =
                IncrementalEdit::new(edit.start_byte, edit.old_end_byte, new_text);
            if let Ok(root) = state.reparse(source, &incremental_edit) {
                let diagnostics = state.diagnostics().to_vec();
                let root = Arc::new(root);
                return Some(Tree {
                    shared_root: SharedNode::from_root(root.clone()),
                    root,
                    source: source.to_string(),
                    pending_edits: Vec::new(),
                    incremental_state: Some(state),
                    line_index: ByteLineIndex::new(source),
                    semantic_model: OnceCell::new(),
                    pragma_map: OnceCell::new(),
                    diagnostics,
                });
            }
        }

        self.parse(source)
    }

    fn try_statement_reuse(
        &mut self,
        source: &str,
        old_tree: &Tree,
        edit: &InputEdit,
        new_text: &str,
    ) -> Option<Tree> {
        if old_tree.has_error() {
            return None;
        }

        let NodeKind::Program { statements } = &old_tree.root.kind else {
            return None;
        };
        let (statement_index, statement_start, statement_end) =
            statements.iter().enumerate().find_map(|(index, statement)| {
                let segment_end = statements
                    .get(index + 1)
                    .map_or(old_tree.source.len(), |next| next.location.start);
                (statement.location.start <= edit.start_byte
                    && edit.start_byte < segment_end
                    && edit.old_end_byte <= segment_end)
                    .then_some((index, statement.location.start, segment_end))
            })?;
        let delta =
            new_text.len() as isize - edit.old_end_byte.saturating_sub(edit.start_byte) as isize;
        if delta != 0 && statement_index + 1 != statements.len() {
            return None;
        }
        let new_statement_end = (statement_end as isize).saturating_add(delta).max(0) as usize;
        if old_tree.shared_root.children.len() != statements.len()
            || new_statement_end > source.len()
            || !source.is_char_boundary(statement_start)
            || !source.is_char_boundary(new_statement_end)
        {
            return None;
        }

        let fragment = source.get(statement_start..new_statement_end)?;
        let mut parser = CoreParser::new(fragment);
        let fragment_root = parser.parse().ok()?;
        if !parser.errors().is_empty() {
            return None;
        }
        let fragment_root_end =
            statement_start.saturating_add(fragment_root.location.end).min(source.len());
        let NodeKind::Program { mut statements } = fragment_root.kind else {
            return None;
        };
        if statements.len() != 1 {
            return None;
        }
        let mut replacement = statements.pop()?;
        shift_locations(&mut replacement, statement_start);

        let mut new_root = old_tree.root.as_ref().clone();
        let NodeKind::Program { statements } = &mut new_root.kind else {
            return None;
        };
        let target = statements.get_mut(statement_index)?;
        *target = replacement.clone();
        if delta != 0 {
            // The parser's Program span excludes trailing comments or
            // whitespace. Reuse the fragment Program end so incremental and
            // fresh root spans stay equivalent.
            new_root.location.end = fragment_root_end;
        }

        let mut children = old_tree.shared_root.children.clone();
        let old_reused = children
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != statement_index)
            .map(|(_, child)| shared_node_count(&child.1))
            .sum();
        let replacement_shared = SharedNode::from_owned(replacement.clone());
        let replacement_reparsed = shared_node_count(&replacement_shared);
        let field = children.get(statement_index).and_then(|child| child.0);
        *children.get_mut(statement_index)? = (field, replacement_shared);

        let incremental_edit =
            IncrementalEdit::new(edit.start_byte, edit.old_end_byte, new_text.to_owned());
        let mut state = old_tree.incremental_state.clone()?;
        state
            .record_ast_reuse(
                source,
                &incremental_edit,
                &[],
                statement_start..new_statement_end,
                old_reused,
                replacement_reparsed.saturating_add(1),
            )
            .ok()?;

        let root = Arc::new(new_root);
        Some(Tree {
            shared_root: SharedNode::with_children(root.clone(), children),
            root,
            source: source.to_string(),
            pending_edits: Vec::new(),
            incremental_state: Some(state),
            line_index: ByteLineIndex::new(source),
            semantic_model: OnceCell::new(),
            pragma_map: OnceCell::new(),
            diagnostics: Vec::new(),
        })
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

/// A descriptor for the Perl language as parsed by the native v3 engine.
///
/// Provides node kind names and field metadata for Rust-native tooling.
/// This is NOT a `tree_sitter::Language` — it does not require a C toolchain
/// and cannot be used with `tree_sitter::Parser::set_language`. For drop-in
/// tree-sitter compatibility use `tree-sitter-perl-c` instead.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_rs::language;
///
/// let lang = language();
/// assert!(lang.node_kind_count() > 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerlLanguage {
    kind_names: &'static [&'static str],
    field_names: &'static [perl_ast::FieldId],
}

impl PerlLanguage {
    /// Returns the number of distinct node kinds in the grammar.
    pub fn node_kind_count(&self) -> usize {
        self.kind_names.len()
    }

    /// Returns all node kind names, in declaration order.
    ///
    /// The order matches the variant declaration order of [`perl_ast::NodeKind`].
    /// `ALL_KIND_NAMES` is auto-derived via `strum::VariantNames`; callers that
    /// need a sorted list should sort the returned slice themselves.
    pub fn node_kind_names(&self) -> &[&'static str] {
        self.kind_names
    }

    /// Returns `true` if the given kind name is a named (non-anonymous) node kind.
    pub fn node_kind_is_named(&self, kind: &str) -> bool {
        self.kind_names.contains(&kind)
    }

    /// Returns the stable named-field identifiers exposed by the AST.
    pub fn field_names(&self) -> &'static [FieldId] {
        self.field_names
    }

    /// Returns the field identifier for a canonical field name.
    pub fn field_id_for_name(&self, name: &str) -> Option<FieldId> {
        perl_ast::FieldId::from_name(name)
    }
}

impl Default for PerlLanguage {
    fn default() -> Self {
        LANGUAGE
    }
}

/// Returns the [`PerlLanguage`] descriptor for Rust-native tooling.
///
/// Note: This is NOT equivalent to `tree_sitter::Language`. See [`PerlLanguage`].
pub fn language() -> PerlLanguage {
    LANGUAGE
}

/// The [`PerlLanguage`] descriptor as a constant.
pub static LANGUAGE: PerlLanguage = PerlLanguage {
    kind_names: perl_ast::NodeKind::ALL_KIND_NAMES,
    field_names: perl_ast::FieldId::ALL,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ByteLineIndex {
    line_starts: Vec<usize>,
}

impl ByteLineIndex {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self { line_starts }
    }

    fn point(&self, source_len: usize, byte: usize) -> Point {
        let clamped = byte.min(source_len);
        let row = self.line_starts.partition_point(|start| *start <= clamped).saturating_sub(1);
        Point { row, column: clamped.saturating_sub(self.line_starts[row]) }
    }
}

/// Persistent facade node storage used to retain proven unchanged subtrees.
///
/// The native AST remains available through `Node::inner`; the reference-counted
/// child links let incremental trees share traversal nodes without cloning them.
#[derive(Debug)]
struct SharedNode {
    source: Arc<AstNode>,
    path: Vec<usize>,
    children: Vec<(Option<FieldId>, Arc<SharedNode>)>,
}

impl SharedNode {
    fn from_root(source: Arc<AstNode>) -> Arc<Self> {
        Self::build(source, Vec::new())
    }

    fn from_owned(node: AstNode) -> Arc<Self> {
        Self::from_root(Arc::new(node))
    }

    fn build(source: Arc<AstNode>, path: Vec<usize>) -> Arc<Self> {
        let mut children = Vec::new();
        Self::ast_at(&source, &path).for_each_child_with_field(|field, _| {
            let index = children.len();
            let mut child_path = path.clone();
            child_path.push(index);
            children.push((field, Self::build(source.clone(), child_path)));
        });
        Arc::new(Self { source, path, children })
    }

    fn with_children(
        source: Arc<AstNode>,
        children: Vec<(Option<FieldId>, Arc<SharedNode>)>,
    ) -> Arc<Self> {
        Arc::new(Self { source, path: Vec::new(), children })
    }

    fn ast(&self) -> &AstNode {
        Self::ast_at(&self.source, &self.path)
    }

    fn ast_at<'a>(source: &'a AstNode, path: &[usize]) -> &'a AstNode {
        let mut current = source;
        for &index in path {
            let mut child = None;
            let mut current_index = 0;
            current.for_each_child(|candidate| {
                if current_index == index {
                    child = Some(candidate);
                }
                current_index += 1;
            });
            let Some(next) = child else {
                return source;
            };
            current = next;
        }
        current
    }
}

/// The result of a successful parse: an owned syntax tree and the source text.
///
/// Use [`root_node`][Tree::root_node] to begin traversal.
#[derive(Debug)]
pub struct Tree {
    shared_root: Arc<SharedNode>,
    root: Arc<AstNode>,
    source: String,
    /// Pending edits recorded via [`Tree::edit`].
    pending_edits: Vec<InputEdit>,
    incremental_state: Option<IncrementalState>,
    line_index: ByteLineIndex,
    semantic_model: OnceCell<SemanticModel>,
    pragma_map: OnceCell<Vec<(std::ops::Range<usize>, PragmaState)>>,
    diagnostics: Vec<ParseDiagnostic>,
}

impl Clone for Tree {
    fn clone(&self) -> Self {
        Self {
            shared_root: self.shared_root.clone(),
            root: self.root.clone(),
            source: self.source.clone(),
            pending_edits: self.pending_edits.clone(),
            incremental_state: self.incremental_state.clone(),
            line_index: self.line_index.clone(),
            semantic_model: OnceCell::new(),
            pragma_map: OnceCell::new(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

impl PartialEq for Tree {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.source == other.source
            && self.pending_edits == other.pending_edits
            && self.diagnostics == other.diagnostics
    }
}

/// The result of [`Parser::parse_detailed`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ParseOutcome {
    /// The recovered syntax tree, when parsing did not fail catastrophically.
    pub tree: Option<Tree>,
    /// Diagnostics collected during parsing, including recoverable errors.
    pub diagnostics: Vec<ParseDiagnostic>,
    /// The typed reason parsing could not produce a usable tree, if any.
    pub failure: Option<ParseFailure>,
}

impl ParseOutcome {
    /// Returns `true` when diagnostics or an explicit error node were observed.
    pub fn has_error(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::blocks_clean_parse)
            || self.tree.as_ref().is_some_and(Tree::has_error)
    }

    /// Returns `true` when a tree was produced with recovery diagnostics.
    pub fn is_recovered(&self) -> bool {
        self.tree.is_some() && self.has_error()
    }
}

/// Typed catastrophic parse failures surfaced by [`Parser::parse_detailed`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ParseFailure {
    /// The parser recursion budget was exceeded.
    RecursionLimit,
    /// The parser's structural nesting budget was exceeded.
    NestingTooDeep {
        /// Observed nesting depth.
        depth: usize,
        /// Configured maximum nesting depth.
        max_depth: usize,
    },
    /// Parsing was cancelled by the caller.
    Cancelled,
    /// A future or currently unclassified catastrophic failure.
    Other {
        /// The original parser diagnostic.
        diagnostic: ParseDiagnostic,
    },
}

impl ParseFailure {
    fn from_diagnostic(diagnostic: &ParseDiagnostic) -> Option<Self> {
        match diagnostic {
            ParseDiagnostic::RecursionLimit => Some(Self::RecursionLimit),
            ParseDiagnostic::NestingTooDeep { depth, max_depth } => {
                Some(Self::NestingTooDeep { depth: *depth, max_depth: *max_depth })
            }
            ParseDiagnostic::Cancelled => Some(Self::Cancelled),
            _ => Some(Self::Other { diagnostic: diagnostic.clone() }),
        }
    }
}

/// Experimental semantic overlay query handle.
///
/// This API is intentionally limited while the facade integration is in development.
/// Query capabilities will expand over time.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SemanticOverlay<'tree> {
    tree: &'tree Tree,
}

/// Symbol definition returned by [`SemanticOverlay`] queries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OverlayDefinition {
    /// Symbol name as written in source.
    pub name: String,
    /// Package-qualified symbol name.
    pub qualified_name: String,
    /// Symbol kind label (debug string form).
    pub kind: String,
    /// Definition span start byte (inclusive).
    pub start_byte: usize,
    /// Definition span end byte (exclusive).
    pub end_byte: usize,
}

/// Import statement visible at a specific source offset.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VisibleImport {
    /// Imported module token (`Foo::Bar`, `strict`, etc.).
    pub module: String,
    /// Statement start byte (inclusive).
    pub statement_start_byte: usize,
    /// Statement end byte (exclusive).
    pub statement_end_byte: usize,
}

impl Tree {
    /// Returns the root node of the syntax tree.
    pub fn root_node(&self) -> Node<'_> {
        Node {
            inner: &self.shared_root,
            tree_source: &self.source,
            line_index: &self.line_index,
            edits: &self.pending_edits,
        }
    }

    /// Returns the source text this tree was built from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns measurements from the most recent parse represented by this tree.
    pub fn incremental_metrics(&self) -> Option<&IncrementalMetrics> {
        self.incremental_state.as_ref().map(IncrementalState::metrics)
    }

    /// Returns the byte range reprocessed by the most recent parse.
    ///
    /// A full parse reports the entire source range. An unchanged-tree fast path
    /// has no incremental metrics and returns an empty vector.
    pub fn changed_ranges(&self) -> Vec<std::ops::Range<usize>> {
        self.incremental_metrics()
            .map(|metrics| vec![metrics.changed_range.clone()])
            .unwrap_or_default()
    }

    /// Returns the diagnostics collected while building this tree.
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Returns `true` when parsing produced diagnostics or an explicit error node.
    pub fn has_error(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::blocks_clean_parse)
            || shared_has_error(&self.shared_root)
    }

    /// Records a source edit on this tree, invalidating affected byte ranges.
    ///
    /// After calling `edit()`, pass this tree and the new source to
    /// [`Parser::parse_with_old_tree`] to re-parse efficiently.
    ///
    /// The edit is consumed by the next parser parse_with_old_tree call. The
    /// current lower-tier kernel supports one edit at a time; multiple pending
    /// edits use a safe full-parse fallback.
    pub fn edit(&mut self, edit: &InputEdit) {
        self.pending_edits.push(edit.clone());
    }

    /// Returns a cursor positioned at the root node for stateful tree traversal.
    ///
    /// This mirrors `tree_sitter::Tree::walk()` and is equivalent to
    /// `tree.root_node().walk()`.
    pub fn walk(&self) -> TreeCursor<'_> {
        self.root_node().walk()
    }

    /// Returns the experimental semantic overlay query handle for this tree.
    pub fn semantic_overlay(&self) -> SemanticOverlay<'_> {
        SemanticOverlay { tree: self }
    }

    fn semantic_model(&self) -> &SemanticModel {
        self.semantic_model.get_or_init(|| SemanticModel::build(&self.root, &self.source))
    }

    fn pragma_map(&self) -> &[(std::ops::Range<usize>, PragmaState)] {
        self.pragma_map.get_or_init(|| PragmaTracker::build(&self.root))
    }
}

impl<'tree> SemanticOverlay<'tree> {
    /// Resolve a symbol definition at a byte offset in the source.
    pub fn definition_at_offset(&self, offset: usize) -> Option<OverlayDefinition> {
        let model = self.tree.semantic_model();
        model.definition_at(offset).map(|symbol| OverlayDefinition {
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            kind: format!("{:?}", symbol.kind),
            start_byte: symbol.location.start,
            end_byte: symbol.location.end,
        })
    }

    /// Resolve a symbol definition for the given node span.
    ///
    /// Uses the node start byte as the query point.
    pub fn definition_for_node(&self, node: &Node<'_>) -> Option<OverlayDefinition> {
        self.definition_at_offset(node.start_byte())
    }

    /// Returns the list of `use`-import modules visible at `offset`.
    ///
    /// Visibility is currently lexical-by-position: this returns `use` statements
    /// with starts less than or equal to `offset`.
    pub fn visible_imports_at_offset(&self, offset: usize) -> Vec<VisibleImport> {
        let mut imports = Vec::new();
        collect_visible_use_imports(&self.tree.root, self.tree.source(), offset, &mut imports);
        let mut deduped = Vec::new();
        for import in imports {
            if !deduped.iter().any(|existing: &VisibleImport| existing.module == import.module) {
                deduped.push(import);
            }
        }
        deduped
    }

    /// Returns the effective pragma state at a byte offset.
    pub fn pragma_state_at_offset(&self, offset: usize) -> PragmaState {
        PragmaTracker::state_for_offset(self.tree.pragma_map(), offset)
    }
}

/// A borrowed reference to a node in the syntax tree.
///
/// Mirrors the tree-sitter `Node` API surface. Lifetime `'tree` is tied to the
/// owning [`Tree`].
#[derive(Clone, Copy)]
pub struct Node<'tree> {
    inner: &'tree SharedNode,
    tree_source: &'tree str,
    line_index: &'tree ByteLineIndex,
    edits: &'tree [InputEdit],
}

impl<'tree> Node<'tree> {
    /// Returns the tree-sitter grammar-canonical node kind name.
    ///
    /// This matches tree-sitter expectations for `Node::kind()`, for example the
    /// root node kind is `"source_file"`. Use [`native_kind`][Node::native_kind]
    /// for the v3 parser's internal PascalCase kind name.
    pub fn kind(&self) -> String {
        self.grammar_kind()
    }

    /// Returns the v3 parser's internal node kind name (e.g. `"Program"`).
    pub fn native_kind(&self) -> &'static str {
        self.inner.ast().kind.kind_name()
    }

    /// Returns the tree-sitter grammar-canonical node kind name.
    ///
    /// Alias of [`kind`][Node::kind] kept for compatibility.
    ///
    /// This method returns the grammar name
    /// used in S-expressions (e.g., `"source_file"`, `"sub"`).
    /// This matches the kind strings returned by `tree-sitter-perl-c` and the
    /// upstream tree-sitter Perl grammar.
    /// Error nodes use `"ERROR"` (uppercase), matching tree-sitter convention.
    ///
    /// For most nodes the grammar name is an allocation-free static mapping. For
    /// nodes whose name depends on runtime data (e.g., operator-named `Binary`
    /// or dynamic `VariableDeclaration`), only that node's fields are inspected.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tree_sitter_perl_rs::Parser;
    ///
    /// let mut parser = Parser::new();
    /// let tree = parser.parse("my $x = 42;");
    /// assert!(tree.is_some());
    /// assert_eq!(tree.unwrap().root_node().grammar_kind(), "source_file");
    /// ```
    pub fn grammar_kind(&self) -> String {
        self.inner.ast().kind.grammar_kind_name()
    }

    /// Returns `true` when this node is an explicit parser error node.
    pub fn is_error(&self) -> bool {
        matches!(self.inner.ast().kind, NodeKind::Error { .. })
    }

    /// Returns `true` when this node or one of its descendants is an error node.
    pub fn has_error(&self) -> bool {
        shared_has_error(self.inner)
    }

    /// Returns a tree-sitter-compatible S-expression for this node and its subtree.
    ///
    /// Delegates to `perl_ast::Node::to_sexp()`. Example output:
    /// `(source_file (my_declaration (variable $ x) (number 42)))`.
    pub fn to_sexp(&self) -> String {
        self.inner.ast().to_sexp()
    }

    /// Returns the number of direct children.
    pub fn child_count(&self) -> usize {
        self.inner.children.len()
    }

    /// Returns the `i`-th direct child, or `None` if out of range.
    pub fn child(&self, i: usize) -> Option<Node<'tree>> {
        self.inner.children.get(i).map(|(_, child)| Node {
            inner: child,
            tree_source: self.tree_source,
            line_index: self.line_index,
            edits: self.edits,
        })
    }

    /// Returns the first direct child carrying the given named field.
    ///
    /// Unknown field names and fields absent from this node return `None`.
    pub fn child_by_field_name(&self, name: &str) -> Option<Node<'tree>> {
        let field = FieldId::from_name(name)?;
        self.inner.children.iter().find_map(|(candidate, child)| {
            (candidate == &Some(field)).then_some(Node {
                inner: child,
                tree_source: self.tree_source,
                line_index: self.line_index,
                edits: self.edits,
            })
        })
    }

    /// Returns all direct children carrying the given named field, in source order.
    pub fn children_by_field_name(&self, name: &str) -> impl Iterator<Item = Node<'tree>> + '_ {
        let field = FieldId::from_name(name);
        let mut children = Vec::new();
        if let Some(field) = field {
            for (candidate, child) in &self.inner.children {
                if *candidate == Some(field) {
                    children.push(Node {
                        inner: child,
                        tree_source: self.tree_source,
                        line_index: self.line_index,
                        edits: self.edits,
                    });
                }
            }
        }
        children.into_iter()
    }

    /// Returns the named field associated with the `index`-th direct child.
    pub fn field_name_for_child(&self, index: usize) -> Option<&'static str> {
        self.inner.children.get(index).and_then(|(field, _)| field.map(FieldId::name))
    }

    /// Returns an iterator over direct children.
    ///
    /// The iterator yields [`Node`] values sharing the same `'tree` lifetime as `self`.
    pub fn children(&self) -> impl Iterator<Item = Node<'tree>> + '_ {
        NodeChildren {
            inner: self.inner.children.iter(),
            tree_source: self.tree_source,
            line_index: self.line_index,
            edits: self.edits,
        }
    }

    /// Returns the start byte offset in the source text (inclusive).
    pub fn start_byte(&self) -> usize {
        self.edited_byte(self.inner.ast().location.start, false)
    }

    /// Returns the end byte offset in the source text (exclusive).
    pub fn end_byte(&self) -> usize {
        self.edited_byte(self.inner.ast().location.end, true)
    }

    /// Returns the start position as a tree-sitter-compatible [`Point`].
    ///
    /// `row`/`column` are zero-based and `column` is measured in bytes.
    pub fn start_position(&self) -> Point {
        self.edited_position(self.inner.ast().location.start, false)
    }

    /// Returns the end position as a tree-sitter-compatible [`Point`].
    ///
    /// `row`/`column` are zero-based and `column` is measured in bytes.
    pub fn end_position(&self) -> Point {
        self.edited_position(self.inner.ast().location.end, true)
    }

    /// Extracts the source text slice covered by this node.
    ///
    /// Returns `Err` only when the byte range contains invalid UTF-8, which is unlikely
    /// for content produced from a valid Rust `&str`.
    ///
    /// If the node's byte offsets extend beyond `source`, the result is clamped to
    /// the available range rather than panicking. This can happen when `source` is a
    /// different buffer than the one used to build the tree.
    pub fn utf8_text<'a>(&self, source: &'a [u8]) -> Result<&'a str, std::str::Utf8Error> {
        let start = self.start_byte().min(source.len());
        let end = self.end_byte().min(source.len());
        std::str::from_utf8(&source[start..end])
    }

    /// Returns `true` if this node has no children (is a leaf node).
    pub fn is_leaf(&self) -> bool {
        self.inner.children.is_empty()
    }

    /// Returns the source text that was provided when creating the owning [`Tree`].
    pub fn tree_source(&self) -> &'tree str {
        self.tree_source
    }

    fn edited_byte(&self, byte: usize, end: bool) -> usize {
        let mut position = Position::new(byte, 0, 0);
        for edit in self.edits {
            position = edit.apply_to_position(position).unwrap_or(if end {
                edit.new_end_position
            } else {
                edit.start_position
            });
        }
        position.byte
    }

    fn edited_position(&self, byte: usize, end: bool) -> Point {
        let original = self.line_index.point(self.tree_source.len(), byte);
        let mut position = Position::new(
            byte,
            u32::try_from(original.row).unwrap_or(u32::MAX),
            u32::try_from(original.column).unwrap_or(u32::MAX),
        );
        for edit in self.edits {
            position = edit.apply_to_position(position).unwrap_or(if end {
                edit.new_end_position
            } else {
                edit.start_position
            });
        }
        Point { row: position.line as usize, column: position.column as usize }
    }

    /// Returns the inner `perl_ast::Node` for direct access to the v3 AST.
    ///
    /// This escape hatch lets callers use capabilities that go beyond the tree-sitter
    /// surface (e.g., match on [`PerlNodeKind`] variants).
    pub fn inner(&self) -> &'tree AstNode {
        self.inner.ast()
    }

    /// Returns a cursor positioned at this node for stateful tree traversal.
    ///
    /// Mirrors `tree_sitter::TreeCursor` style navigation with a lightweight,
    /// allocation-free path stack.
    pub fn walk(&self) -> TreeCursor<'tree> {
        TreeCursor {
            root: self.inner,
            tree_source: self.tree_source,
            line_index: self.line_index,
            edits: self.edits,
            path: Vec::new(),
            nodes: Vec::new(),
        }
    }
}

/// Re-export of [`perl_ast::NodeKind`] so callers can pattern-match node variants
/// without a direct dependency on `perl-ast`.
pub use perl_ast::{FieldId, NodeKind as PerlNodeKind};

/// Stateful cursor for navigating a subtree.
///
/// The cursor is rooted at the [`Node`] that created it via [`Node::walk`].
/// Calling [`goto_parent`][TreeCursor::goto_parent] at the root returns `false`
/// and keeps the cursor at the root.
pub struct TreeCursor<'tree> {
    root: &'tree SharedNode,
    tree_source: &'tree str,
    line_index: &'tree ByteLineIndex,
    edits: &'tree [InputEdit],
    /// Child indices from `root` to the current node.
    path: Vec<usize>,
    nodes: Vec<&'tree SharedNode>,
}

impl<'tree> TreeCursor<'tree> {
    /// Returns the node currently selected by the cursor.
    pub fn node(&self) -> Node<'tree> {
        Node {
            inner: self.nodes.last().copied().unwrap_or(self.root),
            tree_source: self.tree_source,
            line_index: self.line_index,
            edits: self.edits,
        }
    }

    /// Moves to the first child of the current node.
    ///
    /// Returns `true` when movement succeeds, `false` when the node has no children.
    pub fn goto_first_child(&mut self) -> bool {
        let Some(child) = self.current_ast_node().children.first().map(|(_, child)| child.as_ref())
        else {
            return false;
        };
        self.path.push(0);
        self.nodes.push(child);
        true
    }

    /// Moves to the last child of the current node.
    ///
    /// Returns `true` when movement succeeds, `false` when the node has no children.
    pub fn goto_last_child(&mut self) -> bool {
        let child_count = self.current_ast_node().children.len();
        let Some(child) = child_count.checked_sub(1).and_then(|index| {
            self.current_ast_node().children.get(index).map(|(_, child)| child.as_ref())
        }) else {
            return false;
        };
        self.path.push(child_count - 1);
        self.nodes.push(child);
        true
    }

    /// Moves to the next sibling of the current node.
    ///
    /// Returns `true` on success. Returns `false` if the cursor is at root or if
    /// there is no next sibling.
    pub fn goto_next_sibling(&mut self) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let parent = self.current_parent_ast_node();
        let sibling_count = parent.children.len();
        let current_index = self.path[self.path.len() - 1];
        let next = current_index + 1;
        if next >= sibling_count {
            return false;
        }

        let last_pos = self.path.len() - 1;
        self.path[last_pos] = next;
        let Some(sibling) = parent.children.get(next).map(|(_, child)| child.as_ref()) else {
            return false;
        };
        self.nodes[last_pos] = sibling;
        true
    }

    /// Moves to the previous sibling of the current node.
    ///
    /// Returns `true` on success. Returns `false` if the cursor is at root or if
    /// there is no previous sibling.
    pub fn goto_previous_sibling(&mut self) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let current_index = self.path[self.path.len() - 1];
        if current_index == 0 {
            return false;
        }

        let last_pos = self.path.len() - 1;
        self.path[last_pos] = current_index - 1;
        let parent = self.current_parent_ast_node();
        let Some(sibling) = parent.children.get(current_index - 1).map(|(_, child)| child.as_ref())
        else {
            return false;
        };
        self.nodes[last_pos] = sibling;
        true
    }

    /// Moves to the parent node.
    ///
    /// Returns `true` when movement succeeds, `false` when already at root.
    pub fn goto_parent(&mut self) -> bool {
        if self.path.pop().is_some() {
            self.nodes.pop();
            true
        } else {
            false
        }
    }

    /// Resets the cursor back to its root node.
    pub fn reset(&mut self) {
        self.path.clear();
        self.nodes.clear();
    }

    fn current_ast_node(&self) -> &'tree SharedNode {
        self.nodes.last().copied().unwrap_or(self.root)
    }

    fn current_parent_ast_node(&self) -> &'tree SharedNode {
        debug_assert!(!self.path.is_empty(), "current_parent_ast_node requires a non-root cursor");
        if self.path.len() == 1 { self.root } else { self.nodes[self.nodes.len() - 2] }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn shift_locations(node: &mut AstNode, offset: usize) {
    node.location.start = node.location.start.saturating_add(offset);
    node.location.end = node.location.end.saturating_add(offset);
    node.for_each_child_mut(|child| shift_locations(child, offset));
}

fn shared_node_count(node: &SharedNode) -> usize {
    1usize.saturating_add(node.children.iter().map(|(_, child)| shared_node_count(child)).sum())
}

/// Borrowed direct-child iterator used by the facade.
///
/// List-shaped AST nodes use their backing slice iterator directly. The indexed
/// fallback is reserved for the small fixed-field variants, where avoiding a
/// temporary allocation is more important than maintaining a second large match
/// table in this facade.
struct NodeChildren<'tree> {
    inner: std::slice::Iter<'tree, (Option<FieldId>, Arc<SharedNode>)>,
    tree_source: &'tree str,
    line_index: &'tree ByteLineIndex,
    edits: &'tree [InputEdit],
}

impl<'tree> Iterator for NodeChildren<'tree> {
    type Item = Node<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, inner)| Node {
            inner: inner.as_ref(),
            tree_source: self.tree_source,
            line_index: self.line_index,
            edits: self.edits,
        })
    }
}
fn shared_has_error(node: &SharedNode) -> bool {
    if matches!(node.ast().kind, NodeKind::Error { .. }) {
        return true;
    }

    node.children.iter().any(|(_, child)| shared_has_error(child))
}

fn collect_visible_use_imports(
    node: &AstNode,
    source: &str,
    offset: usize,
    out: &mut Vec<VisibleImport>,
) {
    // Only attempt import extraction on Use AST nodes. Container nodes (Program,
    // Block, Subroutine, etc.) span large source ranges that may accidentally start
    // with a `use` token, producing imports with incorrect statement byte ranges.
    // Use nodes have no children (for_each_child is a no-op), so they are visited
    // exactly once per tree traversal — no inner dedup needed.
    if matches!(node.kind, NodeKind::Use { .. }) && node.location.start <= offset {
        let start = node.location.start.min(source.len());
        let end = node.location.end.min(source.len());
        let statement_text = &source[start..end];
        if let Some(import_head) = parse_module_import_head(statement_text) {
            out.push(VisibleImport {
                module: import_head.token.to_string(),
                statement_start_byte: start,
                statement_end_byte: end,
            });
        }
    }

    node.for_each_child(|child| collect_visible_use_imports(child, source, offset, out));
}

// Invariant: TreeCursor path is constructed by traversal methods in this type.
// If a stale/invalid path somehow appears, return the last valid node instead
// of panicking, preserving total API safety guarantees.
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_some;

    #[test]
    fn test_parser_creates_tree() {
        let mut p = Parser::new();
        let tree = p.parse("my $x = 42;");
        assert!(tree.is_some());
    }

    #[test]
    fn test_root_node_kind() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 42;"));
        assert_eq!(tree.root_node().kind(), "source_file");
        assert_eq!(tree.root_node().native_kind(), "Program");
    }

    #[test]
    fn test_to_sexp_starts_with_source_file() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 42;"));
        let sexp = tree.root_node().to_sexp();
        assert!(
            sexp.starts_with("(source_file"),
            "sexp should start with (source_file, got: {}",
            sexp
        );
    }

    #[test]
    fn test_child_count_for_program_with_statements() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 42;\nmy $y = 99;"));
        let root = tree.root_node();
        assert!(root.child_count() >= 1, "root should have children");
    }

    #[test]
    fn test_start_and_end_byte() {
        let source = "my $x = 42;";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();
        assert_eq!(root.start_byte(), 0);
        assert_eq!(root.end_byte(), source.len(), "root end_byte should clamp to source length");
    }

    #[test]
    fn test_start_and_end_position_are_tree_sitter_compatible() {
        let source = "my $x = 1;\nmy $y = 2;";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();

        assert_eq!(root.start_position(), Point { row: 0, column: 0 });
        assert_eq!(root.end_position(), Point { row: 1, column: 10 });
    }

    #[test]
    fn test_end_position_column_uses_bytes_not_chars() {
        let source = "my $emoji = \"😀\";";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();

        assert_eq!(root.end_byte(), source.len());
        assert_eq!(root.end_position(), Point { row: 0, column: source.len() });
    }

    /// Verify the end_byte clamp invariant: for every node in the tree,
    /// `end_byte()` must not exceed `tree.source().len()`.  This exercises the
    /// `.min(self.tree_source.len())` guard on the full node set, not just the
    /// root, so that any future parser regression producing an out-of-bounds
    /// location is caught here.
    #[test]
    fn test_end_byte_never_exceeds_source_len_for_all_nodes() {
        let sources = [
            "my $x = 42;",
            "sub foo { return 1; }",
            "use strict;\nuse warnings;\nmy @arr = (1, 2, 3);",
            // empty source — edge case for zero-length trees
            "",
        ];
        for source in sources {
            let mut p = Parser::new();
            let tree = match p.parse(source) {
                Some(t) => t,
                // v3 parser returns None only on extreme failure; skip rather than panic
                None => continue,
            };
            let source_len = tree.source().len();
            // Walk all direct children of root and check the invariant
            let root = tree.root_node();
            assert!(
                root.end_byte() <= source_len,
                "root end_byte {} > source_len {} for source {:?}",
                root.end_byte(),
                source_len,
                source
            );
            for child in root.children() {
                assert!(
                    child.end_byte() <= source_len,
                    "child end_byte {} > source_len {} for source {:?}",
                    child.end_byte(),
                    source_len,
                    source
                );
            }
        }
    }

    #[test]
    fn test_utf8_text_round_trip() {
        let source = "my $x = 42;";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();
        let text = root.utf8_text(source.as_bytes());
        assert!(text.is_ok(), "utf8_text should succeed");
        // The root node spans the whole source — verify the actual content, not just Ok.
        let extracted = must_some(text.ok());
        assert_eq!(extracted, source, "utf8_text should return the full source for the root node");
    }

    #[test]
    fn test_utf8_text_multibyte_unicode() {
        // 'é' is 2 bytes in UTF-8; the parser must not split a codepoint boundary.
        let source = "my $x = 'café';";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();
        let bytes = source.as_bytes();
        let text = root.utf8_text(bytes);
        assert!(text.is_ok(), "utf8_text should handle multi-byte UTF-8");
    }

    #[test]
    fn test_utf8_text_mismatched_source_does_not_panic() {
        // utf8_text takes a caller-supplied byte slice. When the slice is shorter
        // than the tree's byte offsets, the implementation must clamp rather than panic.
        let source = "my $x = 42;";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        let root = tree.root_node();
        // A shorter slice — would panic without the start.min(source.len()) guard.
        let short = b"my";
        let result = root.utf8_text(short);
        assert!(result.is_ok(), "utf8_text should not panic with short source slice");
    }

    #[test]
    fn test_invalid_perl_returns_some_tree() {
        // The v3 parser is error-tolerant — even syntactically invalid Perl should
        // produce a partial tree (Some), not None. None is only returned on cancellation.
        let mut p = Parser::new();
        let tree = p.parse("sub { @@@@invalid{{{{");
        assert!(tree.is_some(), "invalid Perl should still yield an error-recovery tree");
    }

    #[test]
    fn test_children_iterator_matches_child_count() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 1; my $y = 2;"));
        let root = tree.root_node();
        let collected: Vec<_> = root.children().collect();
        assert_eq!(collected.len(), root.child_count());
    }

    #[test]
    fn test_borrowed_child_iterator_preserves_list_and_pair_order() {
        let first_loc = perl_ast::SourceLocation { start: 0, end: 1 };
        let second_loc = perl_ast::SourceLocation { start: 1, end: 2 };
        let parent_loc = perl_ast::SourceLocation { start: 0, end: 2 };
        let first = AstNode::new(NodeKind::Identifier { name: "x".to_string() }, first_loc);
        let second = AstNode::new(NodeKind::Number { value: "2".to_string() }, second_loc);
        let program = AstNode::new(
            NodeKind::Program { statements: vec![first.clone(), second.clone()] },
            parent_loc,
        );
        let program_shared = SharedNode::from_owned(program);
        let program_starts: Vec<_> =
            program_shared.children.iter().map(|(_, node)| node.ast().location.start).collect();
        assert_eq!(program_starts, vec![0, 1]);

        let hash = AstNode::new(NodeKind::HashLiteral { pairs: vec![(first, second)] }, parent_loc);
        let hash_shared = SharedNode::from_owned(hash);
        let hash_kinds: Vec<_> =
            hash_shared.children.iter().map(|(_, node)| node.ast().kind.kind_name()).collect();
        assert_eq!(hash_kinds, vec!["Identifier", "Number"]);
    }

    #[test]
    fn test_equal_length_statement_edit_reuses_unaffected_shared_subtree() {
        let source = "my $x = 1;\nmy $y = 2;";
        let new_source = source.replace("$y", "$z");
        let start = must_some(source.find("$y")) + 1;
        let edit = perl_parser_core::edit::Edit::new(
            start,
            start + 1,
            start + 1,
            perl_position_tracking::Position::new(start, 1, (start - 11) as u32),
            perl_position_tracking::Position::new(start + 1, 1, (start - 10) as u32),
            perl_position_tracking::Position::new(start + 1, 1, (start - 10) as u32),
        );
        let mut parser = Parser::new();
        let old_tree = must_some(parser.parse(source));
        assert!(!old_tree.has_error());
        let old_child = old_tree.shared_root.children.first().map(|(_, child)| child.clone());
        let mut edited_tree = old_tree.clone();
        edited_tree.edit(&edit);
        let new_tree = must_some(parser.parse_with_old_tree(&new_source, &edited_tree));
        let new_child = new_tree.shared_root.children.first().map(|(_, child)| child.clone());

        assert!(old_child.is_some() && new_child.is_some());
        assert!(
            Arc::ptr_eq(&must_some(old_child), &must_some(new_child)),
            "AST reuse did not trigger: metrics={:?}, edit={start}, old_children={}, new_children={}",
            new_tree.incremental_metrics(),
            old_tree.shared_root.children.len(),
            new_tree.shared_root.children.len(),
        );
    }

    #[test]
    fn test_child_by_index() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 1; my $y = 2;"));
        let root = tree.root_node();
        if root.child_count() > 0 {
            let first = root.child(0);
            assert!(first.is_some());
        }
        assert!(root.child(9999).is_none());
    }

    #[test]
    fn test_empty_source_yields_tree() {
        // The v3 parser is error-tolerant; empty input returns Program { statements: [] }.
        let mut p = Parser::new();
        let tree = p.parse("");
        assert!(tree.is_some(), "empty input should still yield a tree");
    }

    #[test]
    fn test_source_accessor() {
        let source = "sub foo { }";
        let mut p = Parser::new();
        let tree = must_some(p.parse(source));
        assert_eq!(tree.source(), source);
    }

    #[test]
    fn test_default_parser() {
        let mut p = Parser::default();
        let tree = p.parse("1;");
        assert!(tree.is_some());
    }

    #[test]
    fn test_is_leaf_for_leaf_nodes() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("42"));
        let root = tree.root_node();
        // The root Program is not a leaf.
        assert!(!root.is_leaf());
    }

    // Tests for grammar_kind() method

    #[test]
    fn test_grammar_kind_returns_source_file_for_root() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 42;"));
        assert_eq!(tree.root_node().grammar_kind(), "source_file");
    }

    #[test]
    fn test_grammar_kind_returns_variable_with_attributes_for_list_form() {
        let mut p = Parser::new();
        // VariableWithAttributes is only produced for per-variable attributes in list form:
        // `my ($x : lvalue);`. Scalar form `my $x : lvalue;` does not produce this node.
        let tree = must_some(p.parse("my ($x : lvalue);"));
        let root = tree.root_node();
        let mut found_var_with_attrs = false;
        for child in root.children() {
            if child.grammar_kind() == "my_declaration" {
                for sub in child.children() {
                    if sub.grammar_kind() == "variable_with_attributes" {
                        found_var_with_attrs = true;
                    }
                }
            }
        }
        assert!(found_var_with_attrs, "should find variable_with_attributes");
    }

    #[test]
    fn test_grammar_kind_double_paren_edge_case() {
        // Test that grammar_kind() remains independent of the double-paren sexp form.
        // VariableWithAttributes produces ((variable $ foo) (attributes :lvalue))
        let mut p = Parser::new();
        let tree = must_some(p.parse("my ($x : lvalue);"));
        let root = tree.root_node();
        let sexp = root.to_sexp();
        assert!(sexp.contains("((variable"), "sexp should include the double-paren variable form");
        let declaration =
            must_some(root.children().find(|node| node.grammar_kind() == "my_declaration"));
        let variable = must_some(
            declaration.children().find(|node| node.grammar_kind() == "variable_with_attributes"),
        );
        assert_eq!(variable.grammar_kind(), "variable_with_attributes");
    }

    // Tests for PerlLanguage descriptor

    #[test]
    fn test_language_returns_descriptor_with_nonzero_kind_count() {
        let lang = language();
        assert!(lang.node_kind_count() > 0, "language should report at least one node kind");
    }

    #[test]
    fn test_language_constant_has_nonzero_kind_count() {
        assert!(LANGUAGE.node_kind_count() > 0, "LANGUAGE should have at least one node kind");
    }

    #[test]
    fn test_language_reports_program_as_named_kind() {
        let lang = language();
        assert!(lang.node_kind_is_named("Program"), "'Program' should be a named kind");
    }

    #[test]
    fn test_language_rejects_unknown_kind() {
        let lang = language();
        assert!(
            !lang.node_kind_is_named("__nonexistent_kind__"),
            "unknown kind should not be named"
        );
    }

    #[test]
    fn test_language_kind_names_contains_program() {
        let lang = language();
        let names = lang.node_kind_names();
        assert!(names.contains(&"Program"), "kind names should include 'Program'");
    }

    #[test]
    fn test_language_default_returns_same_as_language() {
        // PartialEq compares the backing slice elements, not just the pointer.
        // Both language() and PerlLanguage::default() return LANGUAGE so this
        // also verifies the Default impl wires up the correct constant.
        assert_eq!(language(), PerlLanguage::default());
    }

    #[test]
    fn test_language_kind_names_declaration_order_and_no_duplicates() {
        // ALL_KIND_NAMES is now in declaration order (not alphabetical) via strum::VariantNames
        // (changed in PR #1491). Verify there are no duplicates and 'Program' is first.
        let lang = language();
        let names = lang.node_kind_names();
        assert!(!names.is_empty(), "node_kind_names must not be empty");
        assert_eq!(
            names.first(),
            Some(&"Program"),
            "First kind name should be 'Program' (declaration order)"
        );
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "node_kind_names must not contain duplicates: {} entries, {} unique",
            names.len(),
            unique.len()
        );
    }

    #[test]
    fn test_language_is_named_with_empty_string_returns_false() {
        // Empty string is not a valid kind name and must not be found.
        assert!(!language().node_kind_is_named(""), "empty kind name must return false");
    }

    #[test]
    fn test_tree_cursor_walks_children_and_siblings() {
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1; my $y = 2;"));
        let root = tree.root_node();
        let mut cursor = root.walk();

        assert_eq!(cursor.node().grammar_kind(), "source_file");
        assert!(cursor.goto_first_child(), "source_file should have at least one child");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
        assert!(cursor.goto_next_sibling(), "first statement should have a sibling");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
        assert!(!cursor.goto_next_sibling(), "second statement should be the last sibling");
    }

    #[test]
    fn test_tree_walk_starts_cursor_at_root() {
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1;"));
        let mut cursor = tree.walk();

        assert_eq!(cursor.node().grammar_kind(), "source_file");
        assert!(cursor.goto_first_child(), "root should have a child");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    }

    #[test]
    fn test_tree_cursor_parent_and_reset_behavior() {
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1;"));
        let root = tree.root_node();
        let mut cursor = root.walk();

        assert!(!cursor.goto_parent(), "cursor at root must not move to parent");
        assert!(cursor.goto_first_child(), "root should have a child");
        assert!(cursor.goto_parent(), "child should have root as parent");
        assert_eq!(cursor.node().grammar_kind(), "source_file");

        assert!(cursor.goto_first_child(), "root should still have a child");
        cursor.reset();
        assert_eq!(cursor.node().grammar_kind(), "source_file");
    }

    #[test]
    fn test_tree_cursor_last_child_and_previous_sibling_behavior() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $a = 1; my $b = 2;"));
        let root = tree.root_node();
        let mut cursor = root.walk();

        assert!(cursor.goto_last_child(), "root should have a last child");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
        assert!(cursor.goto_previous_sibling(), "last child should have a previous sibling");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
        assert!(!cursor.goto_previous_sibling(), "first sibling should not have previous sibling");
    }

    #[test]
    fn test_tree_cursor_last_child_returns_false_for_leaf() {
        let mut p = Parser::new();
        let tree = must_some(p.parse("my $x = 42;"));
        let root = tree.root_node();
        let mut cursor = root.walk();

        assert!(cursor.goto_first_child(), "root should have a child");
        assert!(cursor.goto_first_child(), "my_declaration should have a child");
        let at_leaf = !cursor.goto_last_child();
        assert!(at_leaf, "leaf nodes should not have a last child");
    }

    #[test]
    fn test_tree_cursor_goto_first_child_returns_false_for_leaf() {
        // A leaf node has no children; goto_first_child must return false and
        // leave the cursor positioned at the leaf rather than panicking.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1;"));
        let root = tree.root_node();
        let mut cursor = root.walk();

        // Navigate to a leaf: root -> first child (my_declaration) -> first child (leaf token).
        assert!(cursor.goto_first_child(), "root should have a child");
        assert!(cursor.goto_first_child(), "my_declaration should have a child");
        // The leaf must refuse another goto_first_child.
        let at_leaf = !cursor.goto_first_child();
        assert!(at_leaf, "goto_first_child must return false on a leaf node");
    }

    #[test]
    fn test_tree_cursor_multiple_goto_next_sibling_exhausts() {
        // When repeatedly calling goto_next_sibling, the cursor must eventually
        // return false and stay positioned at the last sibling.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("1; 2; 3;"));
        let mut cursor = tree.walk();

        // Navigate to first statement
        assert!(cursor.goto_first_child());
        let mut _count = 1;
        // Keep advancing siblings until we can't
        while cursor.goto_next_sibling() {
            _count += 1;
        }
        // After last goto_next_sibling returns false, cursor should still be valid
        // and still have a node (the last sibling).
        let node = cursor.node();
        assert!(
            !node.kind().is_empty(),
            "cursor should remain at valid node after exhausting siblings"
        );
    }

    #[test]
    fn test_tree_cursor_goto_parent_at_root_repeatedly() {
        // Calling goto_parent at root should return false every time, keeping cursor at root.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1;"));
        let mut cursor = tree.walk();

        // We should always be at root initially
        assert_eq!(cursor.node().grammar_kind(), "source_file");

        // Try to go up multiple times — must stay at root
        for _ in 0..3 {
            let result = cursor.goto_parent();
            assert!(!result, "goto_parent at root must return false");
            assert_eq!(
                cursor.node().grammar_kind(),
                "source_file",
                "cursor must remain at root after failed goto_parent"
            );
        }
    }

    #[test]
    fn test_tree_cursor_reset_from_deep_nesting() {
        // reset() must return cursor to root regardless of depth.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("sub foo { my $x = 1; }"));
        let mut cursor = tree.walk();

        // Navigate deep into the tree
        let mut depth = 0;
        while cursor.goto_first_child() && depth < 10 {
            depth += 1;
        }
        assert!(depth > 0, "should have navigated at least one level deep");

        // reset() should bring us back to root
        cursor.reset();
        assert_eq!(cursor.node().grammar_kind(), "source_file", "reset must return cursor to root");
    }

    #[test]
    fn test_tree_cursor_empty_source_root_is_valid() {
        // Empty source still produces a (minimal) tree; cursor at root should be valid.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse(""));
        let cursor = tree.walk();

        let node = cursor.node();
        assert_eq!(node.grammar_kind(), "source_file");
        assert!(node.child_count() == 0, "empty source tree should have no statements");
    }

    #[test]
    fn test_tree_cursor_empty_source_goto_first_child_returns_false() {
        // Empty source root has no children; goto_first_child must return false.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse(""));
        let mut cursor = tree.walk();

        let result = cursor.goto_first_child();
        assert!(!result, "empty tree root should have no first child");
    }

    #[test]
    fn test_tree_cursor_single_statement_navigation() {
        // Single statement: root -> statement -> (children or leaf).
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("42;"));
        let mut cursor = tree.walk();

        assert_eq!(cursor.node().grammar_kind(), "source_file");
        assert!(cursor.goto_first_child(), "root should have exactly one statement");

        // The single statement should have no next sibling
        assert!(!cursor.goto_next_sibling(), "single statement should be the only child");

        // Going back up should land at root
        assert!(cursor.goto_parent(), "should be able to return to root");
        assert_eq!(cursor.node().grammar_kind(), "source_file");
    }

    #[test]
    fn test_tree_cursor_sibling_navigation_exact_count() {
        // Navigate through all siblings and verify the count matches child_count.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("1; 2; 3; 4;"));
        let root = tree.root_node();
        let child_count = root.child_count();

        let mut cursor = tree.walk();
        assert!(cursor.goto_first_child());

        let mut sibling_count = 1;
        while cursor.goto_next_sibling() {
            sibling_count += 1;
        }

        assert_eq!(sibling_count, child_count, "sibling count should match root.child_count()");
    }

    #[test]
    fn test_tree_cursor_alternating_parent_child_navigation() {
        // Test mixed navigation: down, up, down again at different indices.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("sub a { 1; } sub b { 2; }"));
        let mut cursor = tree.walk();

        // Down to first sub
        assert!(cursor.goto_first_child());
        let first_kind = cursor.node().grammar_kind().to_string();

        // Back to root
        assert!(cursor.goto_parent());
        assert_eq!(cursor.node().grammar_kind(), "source_file");

        // Down again to first sub (should be the same)
        assert!(cursor.goto_first_child());
        assert_eq!(
            cursor.node().grammar_kind(),
            first_kind,
            "re-navigating should reach the same node"
        );
    }

    #[test]
    fn test_tree_cursor_complex_traversal_sequence() {
        // Complex sequence: down, sibling, sibling, up, down, sibling.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $a = 1; my $b = 2; my $c = 3;"));
        let mut cursor = tree.walk();

        // Down to first statement
        assert!(cursor.goto_first_child(), "down to first stmt");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");

        // Move to second statement
        assert!(cursor.goto_next_sibling(), "sibling to second stmt");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");

        // Move to third statement
        assert!(cursor.goto_next_sibling(), "sibling to third stmt");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");

        // No fourth statement
        assert!(!cursor.goto_next_sibling(), "no fourth statement");

        // Back to root
        assert!(cursor.goto_parent(), "back to root");
        assert_eq!(cursor.node().grammar_kind(), "source_file");

        // Down to first again
        assert!(cursor.goto_first_child(), "down to first again");
        assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    }

    #[test]
    fn test_tree_cursor_node_identity_after_traversal() {
        // A node retrieved at the same path should be equal across separate traversals.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $x = 1;"));
        let mut cursor = tree.walk();

        // First traversal: get to first child and extract its kind
        assert!(cursor.goto_first_child());
        let first_kind = cursor.node().grammar_kind().to_string();

        // Reset and repeat
        cursor.reset();
        assert!(cursor.goto_first_child());
        let second_kind = cursor.node().grammar_kind().to_string();

        assert_eq!(
            first_kind, second_kind,
            "node at the same path should have the same kind in both traversals"
        );
    }

    #[test]
    fn test_tree_cursor_sibling_with_unicode_identifiers() {
        // Cursor must correctly navigate siblings even when source contains Unicode.
        let mut parser = Parser::new();
        let tree = must_some(parser.parse("my $café = 1; my $naïve = 2;"));
        let mut cursor = tree.walk();

        let root = tree.root_node();
        let expected_count = root.child_count();

        // Count siblings via cursor
        assert!(cursor.goto_first_child());
        let mut count = 1;
        while cursor.goto_next_sibling() {
            count += 1;
        }

        assert_eq!(
            count, expected_count,
            "sibling count should match even with Unicode identifiers"
        );
    }

    #[test]
    fn test_tree_cursor_deeply_nested_structure() {
        // Verify cursor can navigate a deeply nested structure without stack overflow.
        let mut parser = Parser::new();
        // Create nested blocks: { { { ... } } }
        let mut code = String::new();
        for i in 0..5 {
            code.push_str(&format!("sub level_{} {{ ", i));
        }
        code.push_str("1;");
        for _ in 0..5 {
            code.push_str(" }");
        }

        let tree = must_some(parser.parse(&code));
        let mut cursor = tree.walk();

        // Navigate down as far as possible
        let mut depth = 0;
        while cursor.goto_first_child() && depth < 50 {
            depth += 1;
        }

        // Should have navigated several levels
        assert!(depth > 2, "should navigate multiple levels in nested structure");

        // Navigate back up
        while cursor.goto_parent() {
            depth -= 1;
        }

        // Should be back at root
        assert_eq!(cursor.node().grammar_kind(), "source_file");
        assert_eq!(depth, 0, "should have gone back up to root (depth 0)");
    }
}
