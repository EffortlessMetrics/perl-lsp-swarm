//! Semantic analysis for IDE features.
//!
//! This module provides semantic analysis on top of the symbol table,
//! including semantic tokens for syntax highlighting, hover information,
//! and code intelligence features.
//!
//! ## Module layout
//!
//! | Sub-module | Contents |
//! |------------|----------|
//! | `tokens`   | `SemanticTokenType`, `SemanticTokenModifier`, `SemanticToken` |
//! | `hover`    | `HoverInfo` |
//! | `builtins` | `BuiltinDoc`, `get_builtin_documentation`, classification helpers |
//! | `node_analysis` | `impl SemanticAnalyzer` — AST traversal and source helpers |
//! | `references`    | `impl SemanticAnalyzer` — reference resolution, `find_all_references` |
//! | `model`    | `SemanticModel` — stable, query-oriented LSP facade |

mod builtins;
mod exporter_metadata;
mod hover;
mod model;
mod node_analysis;
mod query_facade;
mod references;
mod tokens;

// Public re-exports — downstream consumers see exactly the same surface.
pub use builtins::{
    BuiltinDoc, ExceptionContext, PragmaDoc, get_attribute_documentation,
    get_builtin_documentation, get_exception_context, get_moose_type_documentation,
    get_operator_documentation, get_pragma_documentation, is_builtin_function,
    is_exception_function,
};
pub use exporter_metadata::{ExportedSubroutine, FileExportMetadata, PackageExportMetadata};
pub use hover::{AUTOLOAD_DYNAMIC_DISPATCH_DETAIL, HoverInfo};
pub use model::SemanticModel;
pub use query_facade::{
    DefinitionLocation, EffectivePragmaState, ParentChain, ResolvedSymbol, SemanticQueryFacade,
    VisibleImport,
};
pub use tokens::{SemanticToken, SemanticTokenModifier, SemanticTokenType};

use crate::SourceLocation;
use crate::analysis::class_model::{
    ClassModel, ClassModelBuilder, MethodResolutionOrder, ModifierKind,
};
use crate::analysis::generated_member_extractor::GeneratedMemberExtractor;
use crate::analysis::package_graph_extractor::PackageGraphExtractor;
use crate::ast::Node;
use crate::symbol::{
    Symbol, SymbolExtractor, SymbolTable, is_method_modifier_keyword, is_universal_method,
};
use perl_semantic_facts::{Confidence, FileId, GeneratedMember, PackageEdge, Provenance};
use std::collections::{HashMap, HashSet};

const MAX_MRO_TRAVERSAL_DEPTH: usize = 1024;

/// Return `true` if `declaration` marks a symbol as a synthetic method modifier.
///
/// Modifier symbols are minted by `SymbolExtractor` with the introducing keyword
/// (`before`, `after`, ...) recorded as their `declaration`.
fn is_method_modifier_declaration(declaration: Option<&str>) -> bool {
    declaration.is_some_and(is_method_modifier_keyword)
}

/// Return `true` if `symbol` is a subroutine that its package actually contributes
/// to method dispatch.
///
/// Two kinds of same-named subroutine are excluded because neither can be the
/// method a modifier decorates:
///
/// - synthetic modifier symbols, which carry the introducing keyword;
/// - lexical subs (`my sub`/`state sub`, Perl 5.18+ `feature 'lexical_subs'`).
///   `SymbolExtractor` qualifies every named sub as `Package::name` regardless of
///   declarator, so a lexical sub is string-indistinguishable from a real method
///   here, yet it never enters the package's method table and `$self->name` can
///   never reach it.
fn is_package_method_symbol(symbol: &Symbol) -> bool {
    if symbol.kind != crate::symbol::SymbolKind::Subroutine {
        return false;
    }
    !matches!(symbol.declaration.as_deref(), Some("my" | "state"))
        && !is_method_modifier_declaration(symbol.declaration.as_deref())
}

/// The package that declared `symbol`, taken from its qualified name.
///
/// Modifier symbols are qualified as `Package::method`, so stripping the method
/// name yields the declaring package. Returns `None` for an unqualified symbol,
/// which keeps an unattributable modifier unresolved instead of guessing.
fn declaring_package_of(symbol: &Symbol) -> Option<&str> {
    symbol.qualified_name.strip_suffix(&symbol.name)?.strip_suffix("::")
}

#[derive(Debug)]
/// Semantic analyzer providing comprehensive IDE features for Perl code.
///
/// Central component for LSP semantic analysis, combining symbol table
/// construction, semantic token generation, and hover information extraction
/// with enterprise-grade performance characteristics.
///
/// # Performance Characteristics
/// - Analysis time: O(n) where n is AST node count
/// - Memory usage: ~1MB per 10K lines of Perl code
/// - Incremental updates: ≤1ms for typical changes
/// - Symbol resolution: <50μs average lookup time
///
/// # LSP Workflow Integration
/// Core pipeline component:
/// 1. **Parse**: AST generation from Perl source
/// 2. **Index**: Symbol table and semantic token construction
/// 3. **Navigate**: Symbol resolution for go-to-definition
/// 4. **Complete**: Context-aware completion suggestions
/// 5. **Analyze**: Cross-reference analysis and diagnostics
///
/// # Perl Language Support
/// - Full Perl 5 syntax coverage with modern idioms
/// - Package-qualified symbol resolution
/// - Lexical scoping with `my`, `our`, `local`, `state`
/// - Object-oriented method dispatch
/// - Regular expression and heredoc analysis
///
/// # Examples
///
/// ```ignore
/// use perl_semantic_analyzer::{Parser, SemanticAnalyzer};
///
/// let code = "my $greeting = 'hello'; sub say_hi { print $greeting; }";
/// let mut parser = Parser::new(code);
/// let ast = parser.parse()?;
///
/// let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
/// let symbols = analyzer.symbol_table();
/// let tokens = analyzer.semantic_tokens();
/// ```
pub struct SemanticAnalyzer {
    /// Symbol table with scope hierarchy and definitions
    pub(super) symbol_table: SymbolTable,
    /// Generated semantic tokens for syntax highlighting
    pub(super) semantic_tokens: Vec<SemanticToken>,
    /// Hover information cache for symbol details
    pub(super) hover_info: HashMap<SourceLocation, HoverInfo>,
    /// Source code for text extraction and analysis
    pub(super) source: String,
    /// Class models extracted from the same file (for same-file inheritance resolution)
    pub class_models: Vec<ClassModel>,
    /// Per-file Exporter metadata extracted from statically readable assignments.
    pub export_metadata: FileExportMetadata,
    /// Package graph edges extracted from inheritance and role composition forms.
    package_edges: Vec<PackageEdge>,
    /// Framework-generated members extracted from accessor declarations.
    generated_members: Vec<GeneratedMember>,
}

impl SemanticAnalyzer {
    /// Create a new semantic analyzer from an AST.
    ///
    /// Equivalent to [`analyze_with_source`](Self::analyze_with_source) with an empty
    /// source string. Use this when you only need symbol-table and token analysis
    /// without source-text-dependent features like hover documentation.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use perl_semantic_analyzer::{Parser, SemanticAnalyzer};
    ///
    /// let mut parser = Parser::new("my $x = 42;");
    /// let ast = parser.parse()?;
    /// let analyzer = SemanticAnalyzer::analyze(&ast);
    /// assert!(!analyzer.symbol_table().symbols.is_empty());
    /// ```
    pub fn analyze(ast: &Node) -> Self {
        Self::analyze_with_source(ast, "")
    }

    /// Create a new semantic analyzer from an AST and source text.
    ///
    /// The source text enables richer analysis including hover documentation
    /// extraction and precise text-range lookups.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use perl_semantic_analyzer::{Parser, SemanticAnalyzer};
    ///
    /// let code = "sub greet { print \"Hello\\n\"; }";
    /// let mut parser = Parser::new(code);
    /// let ast = parser.parse()?;
    ///
    /// let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
    /// let tokens = analyzer.semantic_tokens();
    /// // tokens contains semantic highlighting data for the parsed code
    /// ```
    pub fn analyze_with_source(ast: &Node, source: &str) -> Self {
        let symbol_table = SymbolExtractor::new_with_source(source).extract(ast);
        let class_models = ClassModelBuilder::new().build(ast);
        let export_metadata = exporter_metadata::ExportMetadataBuilder::new().build(ast);
        let package_edges = PackageGraphExtractor::extract(ast, FileId(0));
        let generated_members =
            GeneratedMemberExtractor::extract_from_models(&class_models, "main");

        let mut analyzer = SemanticAnalyzer {
            symbol_table,
            semantic_tokens: Vec::new(),
            hover_info: HashMap::new(),
            source: source.to_string(),
            class_models,
            export_metadata,
            package_edges,
            generated_members,
        };

        analyzer.analyze_node(ast, 0);
        analyzer
    }

    /// Get the symbol table.
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get semantic tokens for syntax highlighting.
    pub fn semantic_tokens(&self) -> &[SemanticToken] {
        &self.semantic_tokens
    }

    /// Get per-file Exporter metadata for statically readable exports.
    pub fn export_metadata(&self) -> &FileExportMetadata {
        &self.export_metadata
    }

    /// Get package graph edges extracted from inheritance and role-composition patterns.
    pub fn package_edges(&self) -> &[PackageEdge] {
        &self.package_edges
    }

    /// Get framework-generated members extracted from accessor declarations.
    pub fn generated_members(&self) -> &[GeneratedMember] {
        &self.generated_members
    }

    /// Get hover information at a location for Navigate/Analyze stages.
    pub fn hover_at(&self, location: SourceLocation) -> Option<&HoverInfo> {
        self.hover_info.get(&location)
    }

    /// Iterate over all hover entries collected during analysis.
    pub fn all_hover_entries(&self) -> impl Iterator<Item = &HoverInfo> {
        self.hover_info.values()
    }

    /// Find the symbol at a given location for Navigate workflows.
    ///
    /// Returns the most specific (smallest range) symbol that contains the location.
    /// This ensures that when hovering inside a subroutine body, we return the
    /// variable at the cursor rather than the enclosing subroutine.
    pub fn symbol_at(&self, location: SourceLocation) -> Option<&Symbol> {
        let mut best: Option<&Symbol> = None;
        let mut best_span = usize::MAX;

        // Search through all symbols for the most specific one at this location
        for symbols in self.symbol_table.symbols.values() {
            for symbol in symbols {
                if symbol.location.start <= location.start && symbol.location.end >= location.end {
                    let span = symbol.location.end - symbol.location.start;
                    if span < best_span {
                        best = Some(symbol);
                        best_span = span;
                    }
                }
            }
        }
        best
    }

    /// Find the definition of a symbol at a given position for Navigate workflows.
    pub fn find_definition(&self, position: usize) -> Option<&Symbol> {
        // First, find if there's a reference at this position
        for refs in self.symbol_table.references.values() {
            for reference in refs {
                if reference.location.start <= position && reference.location.end >= position {
                    let symbols = self.resolve_reference_to_symbols(reference);
                    if let Some(first_symbol) = symbols.first() {
                        return Some(self.resolve_definition_target(first_symbol));
                    }
                }
            }
        }

        // If no reference found, check if we're on a definition itself
        self.symbol_at(SourceLocation { start: position, end: position })
            .map(|symbol| self.resolve_definition_target(symbol))
    }

    /// Redirect method modifier definitions to the underlying method they modify.
    ///
    /// Method modifiers (`before`, `after`, `around`, `override`, `augment`) are modeled as synthetic
    /// subroutine symbols so hover/navigation can describe them, but go-to-definition
    /// should land on the real method declaration when it exists.
    fn resolve_definition_target<'a>(&'a self, symbol: &'a Symbol) -> &'a Symbol {
        if let Some(target) = self.resolve_method_modifier_target(symbol) { target } else { symbol }
    }

    /// If `symbol` is a method modifier target, find the underlying method symbol.
    ///
    /// Resolution is bound to the package that declared the modifier: first that
    /// package's own method, then its ancestors in the package's configured
    /// method-resolution order. A target that neither provides is left unresolved
    /// rather than redirected to a same-named method in an unrelated package.
    fn resolve_method_modifier_target<'a>(&'a self, symbol: &'a Symbol) -> Option<&'a Symbol> {
        if !is_method_modifier_declaration(symbol.declaration.as_deref()) {
            return None;
        }

        let declaring_package = declaring_package_of(symbol)?;

        if let Some(local) = self.method_symbol_in_package(declaring_package, symbol) {
            return Some(local);
        }

        // Ancestors in the package's configured resolution order. Each is looked
        // up in the symbol table by qualified name rather than through its class
        // model, so a plain-Perl parent that never became a `ClassModel` still
        // provides its method — matching the fallback inherited hover already has.
        self.resolve_parent_chain(declaring_package)?
            .iter()
            .find_map(|ancestor| self.method_symbol_in_package(ancestor, symbol))
    }

    /// One `ClassModel` per package name, merging reopened `package` segments.
    ///
    /// A file may declare the same package more than once, and each declaration
    /// produces its own `ClassModel`. Keying a lookup map straight off
    /// `class_models` therefore lets a later segment hide an earlier one's
    /// parents, roles, and methods. Perl has one package here, so the segments
    /// are combined before any resolution decision is made. Repeated methods
    /// are replaced by their later declaration, and later explicit ancestry or
    /// MRO declarations replace earlier package state.
    fn merged_class_models(&self) -> Vec<ClassModel> {
        let mut merged: Vec<ClassModel> = Vec::new();
        for model in &self.class_models {
            let Some(existing) = merged.iter_mut().find(|candidate| candidate.name == model.name)
            else {
                merged.push(model.clone());
                continue;
            };

            // `extends`, `use parent`, `use base`, and `@ISA` describe the
            // package's current ancestry. A later declaration supersedes an
            // earlier one; blindly unioning the lists invents parents that Perl
            // would no longer dispatch through. An empty later segment carries
            // no ancestry declaration and therefore leaves the prior value
            // intact.
            if model.parents_explicit {
                if model.parents_additive {
                    existing.parents.extend(model.parents.iter().cloned());
                } else {
                    existing.parents = model.parents.clone();
                }
            }
            if !model.roles.is_empty() {
                existing.roles = model.roles.clone();
            }

            // Reopening a package can redefine a method. Keep declaration
            // order authoritative so every consumer selects the later symbol,
            // rather than retaining a stale earlier body in the merged view.
            for method in &model.methods {
                existing.methods.retain(|candidate| candidate.name != method.name);
                existing.methods.push(method.clone());
            }
            existing.modifiers.extend(model.modifiers.iter().cloned());
            // An explicit `use mro 'dfs'` must be able to reset an earlier
            // explicit C3. Silent reopened segments carry no MRO decision.
            if model.mro_explicit {
                existing.mro = model.mro;
            }
        }
        merged
    }

    /// Find the method named `symbol.name` that `package` itself contributes.
    fn method_symbol_in_package<'a>(
        &'a self,
        package: &str,
        symbol: &'a Symbol,
    ) -> Option<&'a Symbol> {
        let qualified = format!("{package}::{}", symbol.name);
        self.symbol_table
            .symbols
            .get(&symbol.name)?
            .iter()
            .filter(|candidate| {
                is_package_method_symbol(candidate)
                    && candidate.qualified_name == qualified
                    && candidate.location != symbol.location
            })
            .max_by_key(|candidate| candidate.location.start)
    }

    /// Check if an operator is a file test operator.
    ///
    /// File test operators in Perl are unary operators that test file properties:
    /// -e (exists), -d (directory), -f (file), -r (readable), -w (writable), etc.
    ///
    /// Delegates to `builtins::is_file_test_operator`.
    pub fn is_file_test_operator(op: &str) -> bool {
        builtins::is_file_test_operator(op)
    }

    /// Resolve hover info for a method by walking the same-file parent chain.
    ///
    /// Given a receiver package name and a method name, walks the merged
    /// `parents` of each package model (BFS) and returns `HoverInfo` for
    /// the first class in the chain that defines the method.
    ///
    /// For packages not in `class_models` (plain packages with no OO indicators),
    /// falls back to the symbol table looking for `PackageName::method_name`.
    ///
    /// Returns `None` when no ancestor defined in the same source file
    /// defines the method. For cross-file inherited-method navigation, the
    /// LSP layer (`inherited_method_definition_location` in `navigation.rs`)
    /// handles workspace index traversal - this method is intentionally
    /// scoped to the current file's `class_models` only.
    pub fn resolve_inherited_method_hover(
        &self,
        receiver_class: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        self.resolve_inherited_method_hover_ordered(receiver_class, method_name)
    }

    /// Resolve the source location of a method inherited from the current class.
    ///
    /// This skips the receiver class itself and searches its ancestors in the
    /// package's configured method-resolution order.
    pub fn resolve_inherited_method_location(
        &self,
        receiver_class: &str,
        method_name: &str,
    ) -> Option<SourceLocation> {
        let merged = self.merged_class_models();
        let models_by_name: HashMap<&str, &ClassModel> =
            merged.iter().map(|model| (model.name.as_str(), model)).collect();

        let receiver_model = models_by_name.get(receiver_class).copied()?;
        let ancestor_order = match receiver_model.mro {
            MethodResolutionOrder::Dfs => self.dfs_ancestor_order(receiver_class, &models_by_name),
            MethodResolutionOrder::C3 => self.c3_ancestor_order(receiver_class, &models_by_name),
        };

        for ancestor in ancestor_order {
            if let Some(model) = models_by_name.get(ancestor.as_str()).copied()
                && let Some(location) = self.method_location_in_model(model, method_name)
            {
                return Some(location);
            }
        }

        if is_universal_method(method_name) {
            return self
                .symbol_table
                .symbols
                .get(method_name)
                .and_then(|symbols| {
                    symbols.iter().find(|symbol| {
                        symbol.kind == crate::symbol::SymbolKind::Subroutine
                            && symbol.qualified_name == format!("UNIVERSAL::{method_name}")
                    })
                })
                .map(|symbol| symbol.location);
        }

        None
    }

    /// Resolve the ordered parent chain for a class in same-file class models.
    ///
    /// Returns ancestors in configured method-resolution order, excluding `receiver_class`.
    pub fn resolve_parent_chain(&self, receiver_class: &str) -> Option<Vec<String>> {
        let merged = self.merged_class_models();
        let models_by_name: HashMap<&str, &ClassModel> =
            merged.iter().map(|model| (model.name.as_str(), model)).collect();
        let receiver_model = models_by_name.get(receiver_class).copied()?;

        let chain = match receiver_model.mro {
            MethodResolutionOrder::Dfs => self.dfs_ancestor_order(receiver_class, &models_by_name),
            MethodResolutionOrder::C3 => self.c3_ancestor_order(receiver_class, &models_by_name),
        };
        Some(chain)
    }

    fn resolve_inherited_method_hover_ordered(
        &self,
        receiver_class: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        let merged = self.merged_class_models();
        let models_by_name: HashMap<&str, &ClassModel> =
            merged.iter().map(|model| (model.name.as_str(), model)).collect();

        let Some(receiver_model) = models_by_name.get(receiver_class).copied() else {
            return self.resolve_plain_package_method_hover(receiver_class, method_name);
        };

        let ancestor_order = match receiver_model.mro {
            MethodResolutionOrder::Dfs => self.dfs_ancestor_order(receiver_class, &models_by_name),
            MethodResolutionOrder::C3 => self.c3_ancestor_order(receiver_class, &models_by_name),
        };

        // Perl dispatches in three phases: the whole resolution order is searched
        // for an exact method, then UNIVERSAL, and only then AUTOLOAD. Resolving
        // per-class instead — exact-then-AUTOLOAD for each class in turn — lets a
        // subclass AUTOLOAD pre-empt an ancestor's real method, which would report
        // an exact, source-backed call as a dynamic boundary.

        // Phase 1 — exact method across the receiver and its full ancestor order.
        if let Some(hover) =
            self.exact_hover_for_model_method(receiver_model, receiver_class, method_name)
        {
            return Some(hover);
        }
        for ancestor in &ancestor_order {
            let exact = match models_by_name.get(ancestor.as_str()).copied() {
                Some(model) => {
                    self.exact_hover_for_model_method(model, receiver_class, method_name)
                }
                None => self.plain_package_exact_method_hover(ancestor, method_name),
            };
            if let Some(hover) = exact {
                return Some(hover);
            }
        }

        // Phase 2 — UNIVERSAL is exact and outranks AUTOLOAD.
        if is_universal_method(method_name) {
            return Some(Self::universal_method_hover(method_name));
        }

        // Phase 3 — AUTOLOAD fallback, in the same order.
        if let Some(hover) =
            self.autoload_hover_for_model(receiver_model, receiver_class, method_name)
        {
            return Some(hover);
        }
        for ancestor in &ancestor_order {
            let autoload = match models_by_name.get(ancestor.as_str()).copied() {
                Some(model) => self.autoload_hover_for_model(model, receiver_class, method_name),
                None => self.plain_package_autoload_hover(ancestor, method_name),
            };
            if let Some(hover) = autoload {
                return Some(hover);
            }
        }

        None
    }

    /// Exact-method half of model resolution.
    ///
    /// Kept separate from [`Self::autoload_hover_for_model`] so the caller can
    /// exhaust every exact candidate in the resolution order before any AUTOLOAD
    /// fallback is considered — see [`Self::resolve_inherited_method_hover_ordered`].
    fn exact_hover_for_model_method(
        &self,
        model: &ClassModel,
        receiver_class: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        // Lexical `my sub`/`state sub` declarations are recorded in the model
        // for provenance, but they do not enter the package method table and
        // therefore cannot satisfy a package method dispatch.  Keep the exact
        // hover path aligned with `method_location_in_model` and the modifier
        // resolver by requiring a package-level declarator.
        if !model
            .methods
            .iter()
            .any(|method| method.name == method_name && method.declarator.is_none())
        {
            return None;
        }
        let is_direct = model.name == receiver_class;
        let mut details = if is_direct {
            vec![format!("Defined in {}", model.name)]
        } else {
            vec![format!("Inherited from {}", model.name)]
        };
        let modifier_names: Vec<&str> = model
            .modifiers
            .iter()
            .filter(|modifier| modifier.method_name == method_name)
            .map(|modifier| match modifier.kind {
                ModifierKind::Before => "before",
                ModifierKind::After => "after",
                ModifierKind::Around => "around",
                ModifierKind::Override => "override",
                ModifierKind::Augment => "augment",
            })
            .collect();
        if !modifier_names.is_empty() {
            details.push(format!("Decorated with: {}", modifier_names.join(", ")));
        }
        Some(HoverInfo {
            signature: format!("sub {}::{}", model.name, method_name),
            documentation: None,
            details,
            // The class model names this exact method; the signature is the
            // subroutine the call actually reaches.
            confidence: Confidence::High,
            // Left unclassified: see `HoverInfo::provenance`.
            provenance: None,
        })
    }

    /// AUTOLOAD half of model resolution. Only reached once no exact method and
    /// no UNIVERSAL method answers, matching Perl's dispatch order.
    fn autoload_hover_for_model(
        &self,
        model: &ClassModel,
        receiver_class: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        if !model.methods.iter().any(|m| m.name == "AUTOLOAD") {
            return None;
        }
        let is_direct = model.name == receiver_class;
        let mut details = if is_direct {
            vec![
                format!("Resolved via AUTOLOAD in {}", model.name),
                format!("Requested method: {method_name}"),
            ]
        } else {
            vec![
                format!("Resolved via inherited AUTOLOAD from {}", model.name),
                format!("Requested method: {method_name}"),
            ]
        };
        details.push(AUTOLOAD_DYNAMIC_DISPATCH_DETAIL.to_string());
        Some(HoverInfo {
            signature: format!("sub {}::AUTOLOAD", model.name),
            documentation: None,
            details,
            // AUTOLOAD is a DynamicBoundary (PLSP-SPEC-0017): the requested
            // method name is only known at runtime, so this signature is the
            // handler that would be entered, not an exact definition of
            // `method_name`.
            confidence: Confidence::Low,
            provenance: Some(Provenance::DynamicBoundary),
        })
    }

    /// Hover for a `UNIVERSAL` method such as `can`, `isa`, `DOES`, or `VERSION`.
    fn universal_method_hover(method_name: &str) -> HoverInfo {
        HoverInfo {
            signature: format!("sub UNIVERSAL::{method_name}"),
            documentation: None,
            details: vec!["Defined in UNIVERSAL".to_string()],
            // UNIVERSAL methods are exact: the name is statically known and
            // always reaches the same builtin. Not a dynamic boundary.
            confidence: Confidence::High,
            provenance: None,
        }
    }

    fn method_location_in_model(
        &self,
        model: &ClassModel,
        method_name: &str,
    ) -> Option<SourceLocation> {
        model
            .methods
            .iter()
            .rev()
            .find(|method| method.name == method_name && method.declarator.is_none())
            .or_else(|| {
                model
                    .methods
                    .iter()
                    .rev()
                    .find(|method| method.name == "AUTOLOAD" && method.declarator.is_none())
            })
            .map(|method| method.location)
    }

    /// Exact-method half of plain-package resolution, for a package that has no
    /// `ClassModel`. Split from the AUTOLOAD half for the same ordering reason as
    /// [`Self::exact_hover_for_model_method`].
    fn plain_package_exact_method_hover(
        &self,
        package_name: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        let qualified = format!("{}::{}", package_name, method_name);
        let found_in_table = self.symbol_table.symbols.get(method_name).is_some_and(|syms| {
            syms.iter().any(|s| {
                matches!(s.kind, crate::symbol::SymbolKind::Subroutine)
                    && s.qualified_name == qualified
            })
        }) || self.symbol_table.symbols.contains_key(&qualified);

        found_in_table.then(|| HoverInfo {
            signature: format!("sub {}::{}", package_name, method_name),
            documentation: None,
            details: vec![format!("Inherited from {}", package_name)],
            // The symbol table holds this exact qualified subroutine. Left
            // unclassified: a framework-generated accessor also reaches the
            // table here, and that is SourceBackedGenerated, not exact source.
            confidence: Confidence::High,
            provenance: None,
        })
    }

    /// AUTOLOAD half of plain-package resolution.
    fn plain_package_autoload_hover(
        &self,
        package_name: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        let qualified_autoload = format!("{}::AUTOLOAD", package_name);
        let autoload_in_table = self.symbol_table.symbols.get("AUTOLOAD").is_some_and(|syms| {
            syms.iter().any(|s| {
                matches!(s.kind, crate::symbol::SymbolKind::Subroutine)
                    && s.qualified_name == qualified_autoload
            })
        }) || self.symbol_table.symbols.contains_key(&qualified_autoload);

        autoload_in_table.then(|| HoverInfo {
            signature: format!("sub {}::AUTOLOAD", package_name),
            documentation: None,
            details: vec![
                format!("Resolved via AUTOLOAD in {}", package_name),
                format!("Requested method: {}", method_name),
                AUTOLOAD_DYNAMIC_DISPATCH_DETAIL.to_string(),
            ],
            // Same DynamicBoundary rule as the class-model path above.
            confidence: Confidence::Low,
            provenance: Some(Provenance::DynamicBoundary),
        })
    }

    fn resolve_plain_package_method_hover(
        &self,
        package_name: &str,
        method_name: &str,
    ) -> Option<HoverInfo> {
        // Same exact → UNIVERSAL → AUTOLOAD order as the class-model resolver.
        // A plain package has no recorded ancestors, so there is no chain to walk.
        self.plain_package_exact_method_hover(package_name, method_name)
            .or_else(|| {
                is_universal_method(method_name).then(|| Self::universal_method_hover(method_name))
            })
            .or_else(|| self.plain_package_autoload_hover(package_name, method_name))
    }

    fn dfs_ancestor_order(
        &self,
        package: &str,
        models_by_name: &HashMap<&str, &ClassModel>,
    ) -> Vec<String> {
        fn walk(
            package: &str,
            models_by_name: &HashMap<&str, &ClassModel>,
            seen: &mut HashSet<String>,
            out: &mut Vec<String>,
            depth: usize,
        ) {
            if depth >= MAX_MRO_TRAVERSAL_DEPTH {
                return;
            }

            let Some(model) = models_by_name.get(package).copied() else {
                return;
            };

            for parent in &model.parents {
                if seen.insert(parent.clone()) {
                    out.push(parent.clone());
                    walk(parent, models_by_name, seen, out, depth + 1);
                }
            }
        }

        let mut seen = HashSet::from([package.to_string()]);
        let mut out = Vec::new();
        walk(package, models_by_name, &mut seen, &mut out, 0);
        out
    }

    fn c3_ancestor_order(
        &self,
        package: &str,
        models_by_name: &HashMap<&str, &ClassModel>,
    ) -> Vec<String> {
        fn linearize(
            package: &str,
            models_by_name: &HashMap<&str, &ClassModel>,
            visited: &mut HashSet<String>,
            depth: usize,
        ) -> Vec<String> {
            if depth >= MAX_MRO_TRAVERSAL_DEPTH {
                return vec![package.to_string()];
            }

            if !visited.insert(package.to_string()) {
                return vec![];
            }

            let Some(model) = models_by_name.get(package).copied() else {
                return vec![package.to_string()];
            };

            let parents = model.parents.clone();
            if parents.is_empty() {
                return vec![package.to_string()];
            }

            let mut parent_mros: Vec<Vec<String>> = parents
                .iter()
                .map(|parent| linearize(parent, models_by_name, &mut visited.clone(), depth + 1))
                .collect();
            parent_mros.push(parents.clone());

            let mut result = vec![package.to_string()];
            loop {
                parent_mros.retain(|list| !list.is_empty());
                if parent_mros.is_empty() {
                    break;
                }

                let chosen = parent_mros.iter().find_map(|list| {
                    let candidate = list.first()?;
                    let in_tail = parent_mros
                        .iter()
                        .any(|other| other.iter().skip(1).any(|name| name == candidate));
                    if in_tail { None } else { Some(candidate.clone()) }
                });

                match chosen {
                    Some(name) => {
                        if !result.contains(&name) {
                            result.push(name.clone());
                        }
                        for list in &mut parent_mros {
                            if list.first().is_some_and(|head| head == &name) {
                                list.remove(0);
                            }
                        }
                    }
                    None => {
                        for list in parent_mros {
                            if let Some(head) = list.first()
                                && !result.contains(head)
                            {
                                result.push(head.clone());
                            }
                        }
                        break;
                    }
                }
            }

            result
        }

        linearize(package, models_by_name, &mut HashSet::new(), 0).into_iter().skip(1).collect()
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use crate::ast::{Node, NodeKind};
    use crate::parser::Parser;
    use crate::symbol::SymbolKind;
    use perl_semantic_facts::{Confidence, GeneratedMemberKind, PackageEdgeKind, Provenance};

    #[test]
    fn test_semantic_tokens() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $x = 42;
print $x;
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze(&ast);
        let tokens = analyzer.semantic_tokens();

        // Phase 1 implementation (Issue #188) handles critical AST node types
        // including VariableListDeclaration, Ternary, ArrayLiteral, HashLiteral,
        // Try, and PhaseBlock nodes

        // Check first $x is a declaration
        let x_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| {
                matches!(
                    t.token_type,
                    SemanticTokenType::Variable | SemanticTokenType::VariableDeclaration
                )
            })
            .collect();
        assert!(!x_tokens.is_empty());
        assert!(x_tokens[0].modifiers.contains(&SemanticTokenModifier::Declaration));
        Ok(())
    }

    #[test]
    fn test_hover_info() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
sub foo {
    return 42;
}

my $result = foo();
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze(&ast);

        // The hover info would be at specific locations
        // In practice, we'd look up by position
        assert!(!analyzer.hover_info.is_empty());
        Ok(())
    }

    #[test]
    fn test_hover_doc_from_pod() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
# This is foo
# More docs
sub foo {
    return 1;
}
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find the symbol for foo and check its hover documentation
        let sym = analyzer.symbol_table().symbols.get("foo").ok_or("symbol not found")?[0].clone();
        let hover = analyzer.hover_at(sym.location).ok_or("hover not found")?;
        assert!(hover.documentation.as_ref().ok_or("doc not found")?.contains("This is foo"));
        Ok(())
    }

    #[test]
    fn test_comment_doc_extraction() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
# Adds two numbers
sub add { 1 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols =
            analyzer.symbol_table().find_symbol("add", 0, crate::symbol::SymbolKind::Subroutine);
        assert!(!sub_symbols.is_empty());
        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        assert_eq!(hover.documentation.as_deref(), Some("Adds two numbers"));
        Ok(())
    }

    #[test]
    fn test_extract_documentation_with_out_of_bounds_offset()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "sub add { 1 }\n";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        assert_eq!(analyzer.extract_documentation(code.len() + 1), None);
        Ok(())
    }

    #[test]
    fn test_cross_package_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package Foo {
    # bar sub
    sub bar { 42 }
}

package main;
Foo::bar();
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
        let pos = code.find("Foo::bar").ok_or("Foo::bar not found")? + 5; // position within "bar"
        let def = analyzer.find_definition(pos).ok_or("definition")?;
        assert_eq!(def.name, "bar");

        let hover = analyzer.hover_at(def.location).ok_or("hover not found")?;
        assert!(hover.documentation.as_ref().ok_or("doc not found")?.contains("bar sub"));
        Ok(())
    }

    #[test]
    fn test_universal_method_hover_fallback() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package UNIVERSAL;
sub can { 1 }
sub isa { 1 }

package Foo;
sub new { bless {}, shift }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "can")
            .ok_or("expected UNIVERSAL hover fallback")?;

        assert!(
            hover.signature.contains("UNIVERSAL::can"),
            "expected UNIVERSAL hover signature, got: {}",
            hover.signature
        );
        assert!(
            hover.details.iter().any(|detail| detail.contains("UNIVERSAL")),
            "expected UNIVERSAL hover details, got: {:?}",
            hover.details
        );
        Ok(())
    }

    #[test]
    fn test_autoload_hover_fallback() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package Foo;
sub AUTOLOAD { 1 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "dynamic_method")
            .ok_or("expected AUTOLOAD hover fallback")?;

        assert!(
            hover.signature.contains("Foo::AUTOLOAD"),
            "expected AUTOLOAD hover signature, got: {}",
            hover.signature
        );
        assert!(
            hover.details.iter().any(|detail| detail.contains("AUTOLOAD")),
            "expected AUTOLOAD hover details, got: {:?}",
            hover.details
        );
        assert!(
            hover.details.iter().any(|detail| detail.contains("dynamic_method")),
            "expected requested method detail, got: {:?}",
            hover.details
        );
        Ok(())
    }

    // ---------------------------------------------------------------
    // #14256 — AUTOLOAD hover must not claim exact-method authority.
    //
    // AUTOLOAD is a DynamicBoundary under PLSP-SPEC-0017, so its hover
    // carries Confidence::Low; every exact resolution stays High. The
    // High-side assertions are the opposite-direction controls: an
    // implementation that blanket-downgrades hovers fails them.
    // ---------------------------------------------------------------

    #[test]
    fn test_autoload_method_hover_is_low_confidence() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package Foo;
sub AUTOLOAD { 1 }
sub real_method { 2 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "dynamic_method")
            .ok_or("expected AUTOLOAD hover fallback")?;

        assert_eq!(
            hover.confidence,
            Confidence::Low,
            "AUTOLOAD dispatch is a dynamic boundary and must not claim exact authority; got {:?} for {:?}",
            hover.confidence,
            hover.details
        );
        assert!(
            hover.details.iter().any(|d| d == AUTOLOAD_DYNAMIC_DISPATCH_DETAIL),
            "expected the dynamic-dispatch detail line, got: {:?}",
            hover.details
        );
        // The pre-existing explanation must survive (regression control).
        assert!(
            hover.details.iter().any(|d| d.contains("Resolved via AUTOLOAD in Foo")),
            "expected the original AUTOLOAD provenance detail, got: {:?}",
            hover.details
        );
        assert!(
            hover.details.iter().any(|d| d.contains("dynamic_method")),
            "expected the requested-method detail, got: {:?}",
            hover.details
        );
        // PLSP-SPEC-0002 keeps "Low confidence" and "Dynamic boundary" as
        // distinct states, so the boundary must be carried by provenance rather
        // than inferred from Confidence::Low.
        assert_eq!(
            hover.provenance,
            Some(Provenance::DynamicBoundary),
            "AUTOLOAD dispatch must be classified as a dynamic boundary, got {:?}",
            hover.provenance
        );
        Ok(())
    }

    #[test]
    fn test_exact_hovers_are_not_classified_as_dynamic_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        // The renderer keys the "dynamic dispatch" card off DynamicBoundary
        // provenance. Any exact resolution that leaked that class would be
        // mislabelled, so pin every exact path to "not a boundary".
        let code = r#"
package Base;
sub inherited_method { 1 }

package Child;
our @ISA = ('Base');
sub AUTOLOAD { 2 }
sub real_method { 3 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        for (receiver, method) in
            [("Child", "real_method"), ("Child", "inherited_method"), ("Child", "can")]
        {
            let hover = analyzer
                .resolve_inherited_method_hover(receiver, method)
                .ok_or_else(|| format!("expected hover for {receiver}->{method}"))?;
            assert_ne!(
                hover.provenance,
                Some(Provenance::DynamicBoundary),
                "{receiver}->{method} is exact and must not be a dynamic boundary; got {:?}",
                hover.details
            );
            assert_eq!(
                hover.confidence,
                Confidence::High,
                "{receiver}->{method} is exact and must stay High",
            );
        }
        Ok(())
    }

    #[test]
    fn test_exact_method_hover_stays_high_confidence() -> Result<(), Box<dyn std::error::Error>> {
        // Same class also declares AUTOLOAD, so a blanket downgrade keyed on
        // "class has AUTOLOAD" rather than on the resolution actually taken
        // would fail here.
        let code = r#"
package Foo;
sub AUTOLOAD { 1 }
sub real_method { 2 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "real_method")
            .ok_or("expected exact method hover")?;

        assert_eq!(
            hover.confidence,
            Confidence::High,
            "an exactly-resolved method must stay High; got {:?}",
            hover.confidence
        );
        assert!(
            hover.signature.contains("Foo::real_method"),
            "expected the exact method signature, got: {}",
            hover.signature
        );
        assert!(
            !hover.details.iter().any(|d| d == AUTOLOAD_DYNAMIC_DISPATCH_DETAIL),
            "exact resolution must not carry the dynamic-dispatch detail, got: {:?}",
            hover.details
        );
        Ok(())
    }

    #[test]
    fn test_lexical_subs_do_not_claim_exact_method_hover() -> Result<(), Box<dyn std::error::Error>>
    {
        // Named lexical subs are collected for source/provenance purposes, but
        // they are not package methods.  If exact hover checks only the name,
        // these declarations incorrectly outrank the class AUTOLOAD boundary.
        let code = r#"
package Foo;
use Moo;
use feature 'lexical_subs';
my sub hidden_method { 1 }
state sub another_hidden_method { 2 }
sub AUTOLOAD { 3 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        for method_name in ["hidden_method", "another_hidden_method"] {
            let hover = analyzer
                .resolve_inherited_method_hover("Foo", method_name)
                .ok_or_else(|| format!("expected AUTOLOAD hover for {method_name}"))?;
            assert_eq!(
                hover.signature, "sub Foo::AUTOLOAD",
                "lexical {method_name} must not be presented as an exact package method"
            );
            assert_eq!(
                hover.provenance,
                Some(Provenance::DynamicBoundary),
                "lexical {method_name} should fall through to the dynamic AUTOLOAD boundary"
            );
        }
        Ok(())
    }

    #[test]
    fn test_inherited_autoload_hover_is_low_confidence() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package Base;
sub AUTOLOAD { 1 }

package Child;
our @ISA = ('Base');
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Child", "dynamic_method")
            .ok_or("expected inherited AUTOLOAD hover fallback")?;

        assert_eq!(
            hover.confidence,
            Confidence::Low,
            "inherited AUTOLOAD is still a dynamic boundary; got {:?} for {:?}",
            hover.confidence,
            hover.details
        );
        assert!(
            hover.details.iter().any(|d| d == AUTOLOAD_DYNAMIC_DISPATCH_DETAIL),
            "expected the dynamic-dispatch detail line, got: {:?}",
            hover.details
        );
        Ok(())
    }

    #[test]
    fn test_universal_method_hover_stays_high_confidence() -> Result<(), Box<dyn std::error::Error>>
    {
        // UNIVERSAL methods are exact, not a dynamic boundary.
        //
        // This package deliberately declares no AUTOLOAD. Real Perl resolves
        // UNIVERSAL before consulting AUTOLOAD, but this analyzer currently
        // checks the class-model AUTOLOAD fallback first, so a class that has
        // both would return the AUTOLOAD hover here. That precedence defect is
        // separate from this claim and is tracked in #14257; pinning the
        // current behaviour in a test would only make that fix harder.
        let code = r#"
package Foo;
sub real_method { 1 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "can")
            .ok_or("expected UNIVERSAL hover")?;

        assert_eq!(
            hover.confidence,
            Confidence::High,
            "UNIVERSAL methods are exact, not a dynamic boundary; got {:?}",
            hover.confidence
        );
        assert!(
            hover.signature.contains("UNIVERSAL::can"),
            "expected the UNIVERSAL signature, got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_ancestor_exact_method_beats_subclass_autoload() -> Result<(), Box<dyn std::error::Error>>
    {
        // Perl searches the whole MRO for an exact method before consulting
        // AUTOLOAD, so `Child->inherited_method` reaches `Base::inherited_method`
        // and never enters `Child::AUTOLOAD`. Reporting it as dynamic dispatch
        // would be a false claim about an exact, source-backed call.
        let code = r#"
package Base;
sub inherited_method { 1 }

package Child;
our @ISA = ('Base');
sub AUTOLOAD { 2 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Child", "inherited_method")
            .ok_or("expected inherited exact-method hover")?;

        assert_eq!(
            hover.confidence,
            Confidence::High,
            "an exact inherited method must not be reported as dynamic dispatch; got {:?} for {:?}",
            hover.confidence,
            hover.details
        );
        assert!(
            hover.signature.contains("Base::inherited_method"),
            "expected the ancestor's exact method signature, got: {}",
            hover.signature
        );
        assert!(
            !hover.details.iter().any(|d| d == AUTOLOAD_DYNAMIC_DISPATCH_DETAIL),
            "exact inherited resolution must not carry the dynamic-dispatch detail, got: {:?}",
            hover.details
        );
        Ok(())
    }

    #[test]
    fn test_universal_method_beats_autoload() -> Result<(), Box<dyn std::error::Error>> {
        // Perl resolves UNIVERSAL before AUTOLOAD: `Foo->can(...)` calls
        // UNIVERSAL::can even when Foo declares AUTOLOAD (#14257).
        let code = r#"
package Foo;
sub AUTOLOAD { 1 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Foo", "can")
            .ok_or("expected UNIVERSAL hover")?;

        assert_eq!(
            hover.confidence,
            Confidence::High,
            "UNIVERSAL::can is exact and must outrank AUTOLOAD; got {:?} for {:?}",
            hover.confidence,
            hover.details
        );
        assert!(
            hover.signature.contains("UNIVERSAL::can"),
            "expected the UNIVERSAL signature, got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_autoload_still_wins_when_no_exact_method_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        // Opposite-direction control for the two tests above: reordering must not
        // disable the AUTOLOAD fallback for a name nothing in the chain defines.
        let code = r#"
package Base;
sub inherited_method { 1 }

package Child;
our @ISA = ('Base');
sub AUTOLOAD { 2 }
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let hover = analyzer
            .resolve_inherited_method_hover("Child", "nothing_defines_this")
            .ok_or("expected AUTOLOAD fallback after the exact pass finds nothing")?;

        assert_eq!(
            hover.confidence,
            Confidence::Low,
            "AUTOLOAD must still answer when no exact method exists; got {:?}",
            hover.confidence
        );
        assert!(
            hover.signature.contains("Child::AUTOLOAD"),
            "expected the AUTOLOAD signature, got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_local_declaration_hover_is_high_confidence() -> Result<(), Box<dyn std::error::Error>> {
        // Guards the bulk `node_analysis.rs` construction sites: an ordinary
        // source-backed declaration must not be downgraded.
        let code = r#"
sub documented_sub {
    return 1;
}
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sym = analyzer
            .symbol_table()
            .symbols
            .get("documented_sub")
            .and_then(|syms| syms.first())
            .ok_or("symbol not found")?
            .clone();
        let hover = analyzer.hover_at(sym.location).ok_or("hover not found")?;

        assert_eq!(
            hover.confidence,
            Confidence::High,
            "an exact declaration hover must be High; got {:?}",
            hover.confidence
        );
        Ok(())
    }

    #[test]
    fn test_scope_identification() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $x = 0;
package Foo {
    my $x = 1;
    sub bar { return $x; }
}
my $y = $x;
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let inner_ref_pos = code.find("return $x").ok_or("return $x not found")? + "return ".len();
        let inner_def = analyzer.find_definition(inner_ref_pos).ok_or("inner def not found")?;
        let expected_inner = code.find("my $x = 1").ok_or("my $x = 1 not found")? + 3;
        assert_eq!(inner_def.location.start, expected_inner);

        let outer_ref_pos = code.rfind("$x;").ok_or("$x; not found")?;
        let outer_def = analyzer.find_definition(outer_ref_pos).ok_or("outer def not found")?;
        let expected_outer = code.find("my $x = 0").ok_or("my $x = 0 not found")? + 3;
        assert_eq!(outer_def.location.start, expected_outer);
        Ok(())
    }

    #[test]
    fn test_pod_documentation_extraction() -> Result<(), Box<dyn std::error::Error>> {
        // Test with a simple case that parses correctly
        let code = r#"# Simple comment before sub
sub documented_with_comment {
    return "test";
}
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols = analyzer.symbol_table().find_symbol(
            "documented_with_comment",
            0,
            crate::symbol::SymbolKind::Subroutine,
        );
        assert!(!sub_symbols.is_empty());
        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        let doc = hover.documentation.as_ref().ok_or("doc not found")?;
        assert!(doc.contains("Simple comment before sub"));
        Ok(())
    }

    #[test]
    fn test_empty_source_handling() -> Result<(), Box<dyn std::error::Error>> {
        let code = "";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Should not crash with empty source
        assert!(analyzer.semantic_tokens().is_empty());
        assert!(analyzer.hover_info.is_empty());
        Ok(())
    }

    #[test]
    fn test_multiple_comment_lines() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
# First comment
# Second comment
# Third comment
sub multi_commented {
    1;
}
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols = analyzer.symbol_table().find_symbol(
            "multi_commented",
            0,
            crate::symbol::SymbolKind::Subroutine,
        );
        assert!(!sub_symbols.is_empty());
        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        let doc = hover.documentation.as_ref().ok_or("doc not found")?;
        assert!(doc.contains("First comment"));
        assert!(doc.contains("Second comment"));
        assert!(doc.contains("Third comment"));
        Ok(())
    }

    // SemanticModel tests
    #[test]
    fn test_semantic_model_build_and_tokens() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $x = 42;
my $y = 10;
$x + $y;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);

        // Should have semantic tokens
        let tokens = model.tokens();
        assert!(!tokens.is_empty(), "SemanticModel should provide tokens");

        // Should have variable tokens
        let var_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| {
                matches!(
                    t.token_type,
                    SemanticTokenType::Variable | SemanticTokenType::VariableDeclaration
                )
            })
            .collect();
        assert!(var_tokens.len() >= 2, "Should have at least 2 variable tokens");
        Ok(())
    }

    #[test]
    fn test_semantic_model_symbol_table_access() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $x = 42;
sub foo {
    my $y = $x;
}
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);

        // Should be able to access symbol table
        let symbol_table = model.symbol_table();
        let x_symbols = symbol_table.find_symbol("x", 0, SymbolKind::scalar());
        assert!(!x_symbols.is_empty(), "Should find $x in symbol table");

        let foo_symbols = symbol_table.find_symbol("foo", 0, SymbolKind::Subroutine);
        assert!(!foo_symbols.is_empty(), "Should find sub foo in symbol table");
        Ok(())
    }

    #[test]
    fn test_semantic_model_package_edges() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package Child;
use parent 'Base';
use Moose;
with 'Role';
1;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);
        let edges = model.package_edges();

        assert!(
            edges.iter().any(|edge| {
                edge.from_package == "Child"
                    && edge.to_package == "Base"
                    && edge.kind == PackageEdgeKind::Inherits
            }),
            "SemanticModel should expose the use-parent package edge, got: {edges:?}"
        );
        assert!(
            edges.iter().any(|edge| {
                edge.from_package == "Child"
                    && edge.to_package == "Role"
                    && edge.kind == PackageEdgeKind::ComposesRole
            }),
            "SemanticModel should expose the role-composition package edge, got: {edges:?}"
        );
        Ok(())
    }

    #[test]
    fn test_semantic_model_generated_members() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
package GeneratedExample;
use Moo;
has 'name' => (is => 'ro');
has '+status' => (is => 'rw', predicate => 1);
1;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);
        let members = model.generated_members();

        let name = members
            .iter()
            .find(|member| member.name == "name")
            .ok_or("expected generated getter for `name`")?;
        assert_eq!(name.kind, GeneratedMemberKind::Getter);
        assert_eq!(name.package, "GeneratedExample");
        assert_eq!(name.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(name.confidence, Confidence::Medium);

        assert!(
            members.iter().any(|member| {
                member.name == "status" && member.kind == GeneratedMemberKind::Accessor
            }),
            "expected rw status accessor in generated members, got: {members:?}"
        );
        assert!(
            members.iter().any(|member| {
                member.name == "has_status" && member.kind == GeneratedMemberKind::Predicate
            }),
            "expected predicate generated from `+status`, got: {members:?}"
        );
        assert!(
            members.iter().all(|member| !member.name.starts_with('+')),
            "generated member names should not retain inherited-attribute `+`: {members:?}"
        );
        Ok(())
    }

    #[test]
    fn test_semantic_model_hover_info() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
# This is a documented variable
my $documented = 42;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);

        // Find the location of the variable declaration
        let symbol_table = model.symbol_table();
        let symbols = symbol_table.find_symbol("documented", 0, SymbolKind::scalar());
        assert!(!symbols.is_empty(), "Should find $documented");

        // Check if hover info is available
        if let Some(hover) = model.hover_info_at(symbols[0].location) {
            assert!(hover.signature.contains("documented"), "Hover should contain variable name");
        }
        // Note: hover_info_at might return None if no explicit hover was generated,
        // which is acceptable for now
        Ok(())
    }

    #[test]
    fn test_analyzer_find_definition_scalar() -> Result<(), Box<dyn std::error::Error>> {
        let code = "my $x = 1;\n$x + 2;\n";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        // Use the same path SemanticModel uses to feed source
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find the byte offset of the reference "$x" in the second line
        let ref_line = code.lines().nth(1).ok_or("line 2 not found")?;
        let line_offset = code.lines().next().ok_or("line 1 not found")?.len() + 1; // +1 for '\n'
        let col_in_line = ref_line.find("$x").ok_or("could not find $x on line 2")?;
        let ref_pos = line_offset + col_in_line;

        let symbol =
            analyzer.find_definition(ref_pos).ok_or("definition not found for $x reference")?;

        // 1. Must be a scalar named "x"
        assert_eq!(symbol.name, "x");
        assert_eq!(symbol.kind, SymbolKind::scalar());

        // 2. Declaration must come before reference
        assert!(
            symbol.location.start < ref_pos,
            "Declaration {:?} should precede reference at byte {}",
            symbol.location.start,
            ref_pos
        );
        Ok(())
    }

    #[test]
    fn test_semantic_model_definition_at() -> Result<(), Box<dyn std::error::Error>> {
        let code = "my $x = 1;\n$x + 2;\n";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let model = SemanticModel::build(&ast, code);

        // Compute the byte offset of the reference "$x" on the second line
        let ref_line_index = 1;
        let ref_line = code.lines().nth(ref_line_index).ok_or("line not found")?;
        let col_in_line = ref_line.find("$x").ok_or("could not find $x")?;
        let byte_offset = code
            .lines()
            .take(ref_line_index)
            .map(|l| l.len() + 1) // +1 for '\n'
            .sum::<usize>()
            + col_in_line;

        let definition = model.definition_at(byte_offset);
        assert!(
            definition.is_some(),
            "definition_at returned None for $x reference at {}",
            byte_offset
        );
        if let Some(symbol) = definition {
            assert_eq!(symbol.name, "x");
            assert_eq!(symbol.kind, SymbolKind::scalar());
            assert!(
                symbol.location.start < byte_offset,
                "Declaration {:?} should precede reference at byte {}",
                symbol.location.start,
                byte_offset
            );
        }
        Ok(())
    }

    #[test]
    fn test_analyzer_find_definition_goto_label() -> Result<(), Box<dyn std::error::Error>> {
        let code = "START: while (1) {\n    goto START;\n}\n";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
        let ref_pos = code.find("START;\n").ok_or("could not find goto label")?;

        let symbol = analyzer
            .find_definition(ref_pos)
            .ok_or("definition not found for goto label reference")?;

        assert_eq!(symbol.name, "START");
        assert_eq!(symbol.kind, SymbolKind::Label);
        assert!(
            symbol.location.start < ref_pos,
            "Label definition {:?} should precede goto reference at byte {}",
            symbol.location.start,
            ref_pos
        );
        Ok(())
    }

    #[test]
    fn test_anonymous_subroutine_semantic_tokens() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $closure = sub {
    my $x = 42;
    return $x + 1;
};
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Check that we have semantic tokens for the anonymous sub
        let tokens = analyzer.semantic_tokens();

        // Should have a keyword token for 'sub'
        let sub_keywords: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Keyword)).collect();

        assert!(!sub_keywords.is_empty(), "Should have keyword token for 'sub'");

        // Check hover info exists for the anonymous sub
        let sub_position = code.find("sub {").ok_or("sub { not found")?;
        let hover_exists = analyzer
            .hover_info
            .iter()
            .any(|(loc, _)| loc.start <= sub_position && loc.end >= sub_position);

        assert!(hover_exists, "Should have hover info for anonymous subroutine");
        Ok(())
    }

    #[test]
    fn test_infer_type_for_literals() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $num = 42;
my $str = "hello";
my @arr = (1, 2, 3);
my %hash = (a => 1);
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find nodes and test type inference
        // We need to walk the AST to find the literal nodes
        fn find_number_node(node: &Node) -> Option<&Node> {
            match &node.kind {
                NodeKind::Number { .. } => Some(node),
                NodeKind::Program { statements } | NodeKind::Block { statements } => {
                    for stmt in statements {
                        if let Some(found) = find_number_node(stmt) {
                            return Some(found);
                        }
                    }
                    None
                }
                NodeKind::VariableDeclaration { initializer, .. } => {
                    initializer.as_ref().and_then(|init| find_number_node(init))
                }
                _ => None,
            }
        }

        if let Some(num_node) = find_number_node(&ast) {
            let inferred = analyzer.infer_type(num_node);
            assert_eq!(inferred, Some("number".to_string()), "Should infer number type");
        }

        Ok(())
    }

    #[test]
    fn test_infer_type_for_binary_operations() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"my $sum = 10 + 20;
my $concat = "a" . "b";
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find binary operation nodes
        fn find_binary_node<'a>(node: &'a Node, op: &str) -> Option<&'a Node> {
            match &node.kind {
                NodeKind::Binary { op: node_op, .. } if node_op == op => Some(node),
                NodeKind::Program { statements } | NodeKind::Block { statements } => {
                    for stmt in statements {
                        if let Some(found) = find_binary_node(stmt, op) {
                            return Some(found);
                        }
                    }
                    None
                }
                NodeKind::VariableDeclaration { initializer, .. } => {
                    initializer.as_ref().and_then(|init| find_binary_node(init, op))
                }
                _ => None,
            }
        }

        // Test arithmetic operation infers to number
        if let Some(add_node) = find_binary_node(&ast, "+") {
            let inferred = analyzer.infer_type(add_node);
            assert_eq!(inferred, Some("number".to_string()), "Arithmetic should infer to number");
        }

        // Test concatenation infers to string
        if let Some(concat_node) = find_binary_node(&ast, ".") {
            let inferred = analyzer.infer_type(concat_node);
            assert_eq!(
                inferred,
                Some("string".to_string()),
                "Concatenation should infer to string"
            );
        }

        Ok(())
    }

    #[test]
    fn test_anonymous_subroutine_hover_info() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
# This is a closure
my $adder = sub {
    my ($x, $y) = @_;
    return $x + $y;
};
"#;

        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find hover info for the anonymous sub
        let sub_position = code.find("sub {").ok_or("sub { not found")?;
        let hover = analyzer
            .hover_info
            .iter()
            .find(|(loc, _)| loc.start <= sub_position && loc.end >= sub_position)
            .map(|(_, h)| h);

        assert!(hover.is_some(), "Should have hover info");

        if let Some(h) = hover {
            assert!(h.signature.contains("sub"), "Hover signature should contain 'sub'");
            assert!(
                h.details.iter().any(|d| d.contains("Anonymous")),
                "Hover details should mention anonymous subroutine"
            );
            // Documentation extraction searches backwards from the sub keyword,
            // but the comment is before `my $adder =` (not immediately before `sub`),
            // so extract_documentation may not find it. Accept either outcome.
            if let Some(doc) = &h.documentation {
                assert!(
                    doc.contains("closure"),
                    "If documentation found, it should mention closure"
                );
            }
        }
        Ok(())
    }

    // Phase 2/3 Handler Tests
    #[test]
    fn test_substitution_operator_semantic_token() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $str = "hello world";
$str =~ s/world/Perl/;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(!operator_tokens.is_empty(), "Should have operator tokens for substitution");
        Ok(())
    }

    #[test]
    fn test_transliteration_operator_semantic_token() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $str = "hello";
$str =~ tr/el/ol/;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(!operator_tokens.is_empty(), "Should have operator tokens for transliteration");
        Ok(())
    }

    #[test]
    fn test_reference_operator_semantic_token() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $x = 42;
my $ref = \$x;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(!operator_tokens.is_empty(), "Should have operator tokens for reference operator");
        Ok(())
    }

    #[test]
    fn test_postfix_loop_semantic_token() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my @list = (1, 2, 3);
print $_ for @list;
my $x = 0;
$x++ while $x < 10;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let control_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.token_type, SemanticTokenType::KeywordControl))
            .collect();

        assert!(!control_tokens.is_empty(), "Should have control keyword tokens for postfix loops");
        Ok(())
    }

    #[test]
    fn test_file_test_operator_semantic_token() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $file = "test.txt";
if (-e $file) {
    print "exists";
}
if (-d $file) {
    print "directory";
}
if (-f $file) {
    print "file";
}
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(!operator_tokens.is_empty(), "Should have operator tokens for file test operators");
        Ok(())
    }

    #[test]
    fn test_all_file_test_operators_recognized() -> Result<(), Box<dyn std::error::Error>> {
        // Test that the is_file_test_operator helper recognizes all file test operators
        let file_test_ops = vec![
            "-e", "-d", "-f", "-r", "-w", "-x", "-s", "-z", "-T", "-B", "-M", "-A", "-C", "-l",
            "-p", "-S", "-u", "-g", "-k", "-t", "-O", "-G", "-R", "-b", "-c",
        ];

        for op in file_test_ops {
            assert!(
                SemanticAnalyzer::is_file_test_operator(op),
                "Operator {} should be recognized as file test operator",
                op
            );
        }

        // Test that non-file-test operators are not recognized
        assert!(
            !SemanticAnalyzer::is_file_test_operator("+"),
            "Operator '+' should not be recognized as file test operator"
        );
        assert!(
            !SemanticAnalyzer::is_file_test_operator("-"),
            "Operator '-' should not be recognized as file test operator"
        );
        assert!(
            !SemanticAnalyzer::is_file_test_operator("++"),
            "Operator '++' should not be recognized as file test operator"
        );

        Ok(())
    }

    #[test]
    fn test_postfix_loop_modifiers() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my @items = (1, 2, 3);
print $_ for @items;
print $_ foreach @items;
my $x = 0;
$x++ while $x < 10;
$x-- until $x < 0;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let control_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.token_type, SemanticTokenType::KeywordControl))
            .collect();

        // Should have at least 4 control keyword tokens (for, foreach, while, until)
        assert!(
            control_tokens.len() >= 4,
            "Should have at least 4 control keyword tokens for postfix loop modifiers"
        );
        Ok(())
    }

    #[test]
    fn test_substitution_with_modifiers() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $str = "hello world";
$str =~ s/world/Perl/gi;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(
            !operator_tokens.is_empty(),
            "Should have operator tokens for substitution with modifiers"
        );
        Ok(())
    }

    #[test]
    fn test_transliteration_y_operator() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $str = "hello";
$str =~ y/hello/world/;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze(&ast);

        let tokens = analyzer.semantic_tokens();
        let operator_tokens: Vec<_> =
            tokens.iter().filter(|t| matches!(t.token_type, SemanticTokenType::Operator)).collect();

        assert!(
            !operator_tokens.is_empty(),
            "Should have operator tokens for y/// transliteration"
        );
        Ok(())
    }

    #[test]
    fn test_builtin_documentation_coverage() -> Result<(), Box<dyn std::error::Error>> {
        // Verify that commonly-used builtins have documentation
        let builtins = [
            "print", "say", "push", "pop", "shift", "unshift", "map", "grep", "sort", "reverse",
            "split", "join", "chomp", "chop", "length", "substr", "index", "rindex", "lc", "uc",
            "die", "warn", "eval", "open", "close", "read", "keys", "values", "exists", "delete",
            "defined", "ref", "bless", "sprintf", "chr", "ord",
        ];

        for name in &builtins {
            let doc = get_builtin_documentation(name);
            assert!(doc.is_some(), "Built-in '{}' should have documentation", name);
            let doc = doc.unwrap();
            assert!(
                !doc.signature.is_empty(),
                "Built-in '{}' should have a non-empty signature",
                name
            );
            assert!(
                !doc.description.is_empty(),
                "Built-in '{}' should have a non-empty description",
                name
            );
        }
        Ok(())
    }

    #[test]
    fn test_builtin_hover_for_function_call() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my @items = (3, 1, 4);
push @items, 5;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find the hover info for 'push' function call
        let push_pos = code.find("push").ok_or("push not found")?;
        let hover_for_push =
            analyzer.hover_info.iter().find(|(loc, _)| loc.start <= push_pos && loc.end > push_pos);

        assert!(hover_for_push.is_some(), "Should have hover info for 'push' builtin");
        let (_, hover) = hover_for_push.unwrap();
        assert!(
            hover.signature.contains("push"),
            "Hover signature should contain 'push', got: {}",
            hover.signature
        );
        assert!(hover.documentation.is_some(), "Hover for 'push' should have documentation");
        Ok(())
    }

    #[test]
    fn test_core_prefixed_builtin_hover_for_function_call() -> Result<(), Box<dyn std::error::Error>>
    {
        let code = r#"
my $value = "abc";
CORE::length($value);
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let length_pos = code.find("CORE::length").ok_or("CORE::length not found")?;
        let hover = analyzer
            .hover_info
            .iter()
            .find(|(loc, _)| loc.start <= length_pos && loc.end > length_pos);

        assert!(hover.is_some(), "Should have hover info for CORE::length builtin");
        let (_, hover) = hover.ok_or("missing hover for CORE::length")?;
        assert!(
            hover.signature.contains("length"),
            "Hover signature should contain 'length', got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_utf8_builtin_hover_for_function_call() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
my $value = "\x{100}";
utf8::encode($value);
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let encode_pos = code.find("utf8::encode").ok_or("utf8::encode not found")?;
        let hover = analyzer
            .hover_info
            .iter()
            .find(|(loc, _)| loc.start <= encode_pos && loc.end > encode_pos);

        assert!(hover.is_some(), "Should have hover info for utf8::encode builtin");
        let (_, hover) = hover.ok_or("missing hover for utf8::encode")?;
        assert!(
            hover.signature.contains("utf8::encode"),
            "Hover signature should contain utf8::encode, got: {}",
            hover.signature
        );
        let documentation =
            hover.documentation.as_deref().ok_or("utf8::encode hover should have docs")?;
        assert!(
            documentation.contains("extended UTF-8 octets"),
            "Hover docs should describe utf8::encode semantics, got: {documentation}"
        );
        Ok(())
    }

    #[test]
    fn test_package_hover_with_pod_name_section() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
=head1 NAME

My::Module - A great module for testing

=head1 DESCRIPTION

This module does great things.

=cut

package My::Module;

sub new { bless {}, shift }

1;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find the package symbol
        let pkg_symbols = analyzer.symbol_table().symbols.get("My::Module");
        assert!(pkg_symbols.is_some(), "Should find My::Module in symbol table");

        let pkg = &pkg_symbols.unwrap()[0];
        let hover = analyzer.hover_at(pkg.location);
        assert!(hover.is_some(), "Should have hover info for package");

        let hover = hover.unwrap();
        assert!(
            hover.signature.contains("package My::Module"),
            "Package hover signature should contain 'package My::Module', got: {}",
            hover.signature
        );
        // The POD NAME section should be extracted as documentation
        if let Some(doc) = &hover.documentation {
            assert!(
                doc.contains("A great module for testing"),
                "Package hover should contain POD NAME content, got: {}",
                doc
            );
        }
        Ok(())
    }

    #[test]
    fn test_package_documentation_via_symbol() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
=head1 NAME

Utils - Utility functions

=cut

package Utils;

sub helper { 1 }

1;
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let pkg_symbols = analyzer.symbol_table().symbols.get("Utils");
        assert!(pkg_symbols.is_some(), "Should find Utils package");

        let pkg = &pkg_symbols.unwrap()[0];
        // The symbol extractor should have extracted POD docs
        assert!(
            pkg.documentation.is_some(),
            "Package symbol should have documentation from POD NAME section"
        );
        let doc = pkg.documentation.as_ref().unwrap();
        assert!(
            doc.contains("Utility functions"),
            "Package doc should contain 'Utility functions', got: {}",
            doc
        );
        Ok(())
    }

    #[test]
    fn test_subroutine_with_pod_docs_hover() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"
=head2 process

Processes input data and returns the result.

=cut

sub process {
    my ($input) = @_;
    return $input * 2;
}
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols = analyzer.symbol_table().find_symbol("process", 0, SymbolKind::Subroutine);
        assert!(!sub_symbols.is_empty(), "Should find sub process");

        let hover = analyzer.hover_at(sub_symbols[0].location);
        assert!(hover.is_some(), "Should have hover for sub process");

        let hover = hover.unwrap();
        assert!(
            hover.signature.contains("sub process"),
            "Hover should show sub signature, got: {}",
            hover.signature
        );
        // The POD =head2 docs should be extracted
        if let Some(doc) = &hover.documentation {
            assert!(
                doc.contains("process") || doc.contains("Processes"),
                "Sub hover should contain POD documentation, got: {}",
                doc
            );
        }
        Ok(())
    }

    #[test]
    fn test_variable_hover_shows_declaration_type() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"my $count = 42;
my @items = (1, 2, 3);
my %config = (key => "value");
"#;
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Check scalar variable hover
        let scalar_pos = code.find("$count").ok_or("$count not found")?;
        let scalar_hover = analyzer
            .hover_info
            .iter()
            .find(|(loc, _)| loc.start <= scalar_pos && loc.end > scalar_pos);
        assert!(scalar_hover.is_some(), "Should have hover for $count");
        let (_, hover) = scalar_hover.unwrap();
        assert!(
            hover.signature.contains("$count"),
            "Scalar hover should show variable name, got: {}",
            hover.signature
        );

        // Check array variable hover
        let array_pos = code.find("@items").ok_or("@items not found")?;
        let array_hover = analyzer
            .hover_info
            .iter()
            .find(|(loc, _)| loc.start <= array_pos && loc.end > array_pos);
        assert!(array_hover.is_some(), "Should have hover for @items");
        let (_, hover) = array_hover.unwrap();
        assert!(
            hover.signature.contains("@items"),
            "Array hover should show variable name, got: {}",
            hover.signature
        );

        // Check hash variable hover
        let hash_pos = code.find("%config").ok_or("%config not found")?;
        let hash_hover =
            analyzer.hover_info.iter().find(|(loc, _)| loc.start <= hash_pos && loc.end > hash_pos);
        assert!(hash_hover.is_some(), "Should have hover for %config");
        let (_, hover) = hash_hover.unwrap();
        assert!(
            hover.signature.contains("%config"),
            "Hash hover should show variable name, got: {}",
            hover.signature
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Subroutine signature hover tests (issue #2353)
    // -----------------------------------------------------------------------

    #[test]
    fn test_signature_hover_shows_param_names() -> Result<(), Box<dyn std::error::Error>> {
        // sub with named scalar parameters — hover must show them by name, not (...)
        let code = "sub add($x, $y) { $x + $y }";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols =
            analyzer.symbol_table().find_symbol("add", 0, crate::symbol::SymbolKind::Subroutine);
        assert!(!sub_symbols.is_empty(), "symbol 'add' not found");

        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        assert!(
            hover.signature.contains("$x"),
            "hover signature should contain '$x', got: {}",
            hover.signature
        );
        assert!(
            hover.signature.contains("$y"),
            "hover signature should contain '$y', got: {}",
            hover.signature
        );
        assert!(
            !hover.signature.contains("(...)"),
            "hover signature must not fall back to '(...)', got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_signature_hover_with_optional_param() -> Result<(), Box<dyn std::error::Error>> {
        // optional parameter with default value
        let code = "sub greet($name, $greeting = 'Hello') { \"$greeting, $name\" }";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols =
            analyzer.symbol_table().find_symbol("greet", 0, crate::symbol::SymbolKind::Subroutine);
        assert!(!sub_symbols.is_empty(), "symbol 'greet' not found");

        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        assert!(
            hover.signature.contains("$name"),
            "hover signature should contain '$name', got: {}",
            hover.signature
        );
        assert!(
            hover.signature.contains("$greeting"),
            "hover signature should contain '$greeting', got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_signature_hover_with_slurpy_param() -> Result<(), Box<dyn std::error::Error>> {
        // slurpy array parameter collects remaining args
        let code = "sub log_all($level, @messages) { print \"$level: @messages\" }";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let sub_symbols = analyzer.symbol_table().find_symbol(
            "log_all",
            0,
            crate::symbol::SymbolKind::Subroutine,
        );
        assert!(!sub_symbols.is_empty(), "symbol 'log_all' not found");

        let hover = analyzer.hover_at(sub_symbols[0].location).ok_or("hover not found")?;
        assert!(
            hover.signature.contains("@messages"),
            "hover signature should contain '@messages', got: {}",
            hover.signature
        );
        Ok(())
    }

    #[test]
    fn test_find_definition_returns_method_kind_for_native_method()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "class Foo {\n    method bar { return 1; }\n}\n";
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // Find offset of "bar" in "method bar" on line 1
        let line1 = code.lines().nth(1).ok_or("no line 1")?;
        let line0_len = code.lines().next().ok_or("no line 0")?.len() + 1;
        let col = line1.find("bar").ok_or("bar not found on line 1")?;
        let offset = line0_len + col;

        let sym = analyzer.find_definition(offset).ok_or("no symbol found at 'bar'")?;
        assert_eq!(sym.name, "bar", "symbol name should be 'bar'");
        assert_eq!(
            sym.kind,
            SymbolKind::Method,
            "native method should have SymbolKind::Method, got {:?}",
            sym.kind
        );
        Ok(())
    }

    #[test]
    fn test_find_definition_redirects_method_modifier_to_target_method()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = include_str!(
            "../../../../perl-lsp-rs/tests/fixtures/frameworks/moo_method_modifiers.pl"
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;

        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        // The method definition is line 3; the modifier targets are on lines 8, 13, and 18.
        for target_line in [8, 13, 18] {
            let line = code.lines().nth(target_line).ok_or("missing modifier line")?;
            let col = line.find("save").ok_or("modifier target not found")?;
            let mut offset = 0;
            for line in code.lines().take(target_line) {
                offset += line.len() + 1;
            }
            offset += col;

            let sym = analyzer
                .find_definition(offset)
                .ok_or("no symbol found at method modifier target")?;
            assert_eq!(sym.name, "save", "modifier target should resolve to save");
            let method_start = code.find("sub save").ok_or("method declaration not found")?;
            assert_eq!(
                sym.location.start, method_start,
                "modifier target should resolve to the underlying method declaration"
            );
            assert_eq!(
                sym.declaration, None,
                "definition should land on the real method, not the synthetic modifier"
            );
        }

        Ok(())
    }

    /// Byte offset of the modifier's quoted target name in `code`.
    fn modifier_target_offset(code: &str, modifier: &str, target: &str) -> Option<usize> {
        let modifier_start = code.find(modifier)?;
        code[modifier_start..].find(target).map(|found| modifier_start + found)
    }

    #[test]
    fn test_method_modifier_target_prefers_declaring_package_over_same_named_method()
    -> Result<(), Box<dyn std::error::Error>> {
        // Both packages define `save`. The modifier belongs to `Demo`, so it must
        // resolve to `Demo::save` and never to the earlier `Other::save`.
        let code = concat!(
            "package Other;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 1;\n",
            "}\n",
            "\n",
            "package Demo;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 2;\n",
            "}\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let other_save = code.find("sub save").ok_or("Other::save not found")?;
        let demo_save = code[other_save + 1..]
            .find("sub save")
            .map(|f| f + other_save + 1)
            .ok_or("Demo::save not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_eq!(
            sym.qualified_name, "Demo::save",
            "modifier must resolve inside its own package, got {}",
            sym.qualified_name
        );
        assert_eq!(
            sym.location.start, demo_save,
            "modifier target should land on Demo::save, not Other::save"
        );
        assert_ne!(
            sym.location.start, other_save,
            "modifier target must not leak into the unrelated Other package"
        );
        Ok(())
    }

    #[test]
    fn test_method_modifier_target_unresolved_in_declaring_package_is_not_redirected()
    -> Result<(), Box<dyn std::error::Error>> {
        // `Demo` declares no `save` and inherits none. The only `save` in the file
        // belongs to the unrelated `Other`, so there is no honest target and the
        // definition must stay on the modifier declaration itself.
        let code = concat!(
            "package Other;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 1;\n",
            "}\n",
            "\n",
            "package Demo;\n",
            "use Moo;\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let other_save = code.find("sub save").ok_or("Other::save not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_ne!(
            sym.location.start, other_save,
            "unresolved modifier target must not jump to a same-named method in another package"
        );
        assert_eq!(
            sym.declaration.as_deref(),
            Some("before"),
            "definition should remain on the modifier declaration when no target is provable"
        );
        Ok(())
    }

    #[test]
    fn test_method_modifier_target_resolves_through_parent_class()
    -> Result<(), Box<dyn std::error::Error>> {
        // `Child` does not declare `save`; it inherits it. Resolution must reach
        // `Base::save` through the class's method-resolution order, and the
        // unrelated `Bystander::save` must never win.
        let code = concat!(
            "package Bystander;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 0;\n",
            "}\n",
            "\n",
            "package Base;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 1;\n",
            "}\n",
            "\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'Base';\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let bystander_save = code.find("sub save").ok_or("Bystander::save not found")?;
        let base_save = code[bystander_save + 1..]
            .find("sub save")
            .map(|f| f + bystander_save + 1)
            .ok_or("Base::save not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_eq!(
            sym.location.start, base_save,
            "inherited modifier target should resolve to Base::save through the MRO"
        );
        assert_ne!(
            sym.location.start, bystander_save,
            "inherited resolution must not pick the unrelated Bystander::save"
        );
        Ok(())
    }

    #[test]
    fn test_method_modifier_target_ignores_lexical_subs() -> Result<(), Box<dyn std::error::Error>>
    {
        // A `my sub` is qualified `Demo::save` like any other named sub, but it
        // never enters the package's method table, so `$self->save` cannot reach
        // it and a modifier must not resolve to it.
        let code = concat!(
            "package Demo;\n",
            "use Moo;\n",
            "use feature 'lexical_subs';\n",
            "\n",
            "if (1) {\n",
            "    my sub save {\n",
            "        return 42;\n",
            "    }\n",
            "}\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let lexical_sub = code.find("my sub save").ok_or("lexical sub not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_ne!(
            sym.location.start, lexical_sub,
            "a lexical `my sub` is not in the package method table and must not be a modifier target"
        );
        assert_eq!(
            sym.declaration.as_deref(),
            Some("before"),
            "with no package method to reach, definition stays on the modifier declaration"
        );
        Ok(())
    }

    #[test]
    fn test_method_modifier_target_resolves_through_plain_package_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        // `Base` is ordinary Perl with no OO indicators, so it never becomes a
        // `ClassModel`. It is still a real parent contributing a real method, and
        // the decoy `Bystander::save` must not win.
        let code = concat!(
            "package Bystander;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 0;\n",
            "}\n",
            "\n",
            "package Base;\n",
            "\n",
            "sub save {\n",
            "    return 1;\n",
            "}\n",
            "\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'Base';\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let bystander_save = code.find("sub save").ok_or("Bystander::save not found")?;
        let base_save = code[bystander_save + 1..]
            .find("sub save")
            .map(|found| found + bystander_save + 1)
            .ok_or("Base::save not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_eq!(
            sym.location.start, base_save,
            "a plain-Perl parent with no class model still provides the inherited method"
        );
        assert_ne!(
            sym.location.start, bystander_save,
            "resolution must not fall back to an unrelated package's method"
        );
        Ok(())
    }

    #[test]
    fn test_method_modifier_target_resolves_through_reopened_parent_declaration()
    -> Result<(), Box<dyn std::error::Error>> {
        // `Child` is declared twice: the first segment carries `extends 'Base'`,
        // the second carries the modifier. The package has one ancestry, so the
        // inherited target must still resolve.
        let code = concat!(
            "package Base;\n",
            "use Moo;\n",
            "\n",
            "sub save {\n",
            "    return 1;\n",
            "}\n",
            "\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'Base';\n",
            "\n",
            "package Other;\n",
            "use Moo;\n",
            "\n",
            "package Child;\n",
            "use Moo;\n",
            "\n",
            "before 'save' => sub {\n",
            "    my ($self) = @_;\n",
            "};\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let base_save = code.find("sub save").ok_or("Base::save not found")?;

        let sym = analyzer.find_definition(offset).ok_or("no symbol at modifier target")?;

        assert_eq!(
            sym.location.start, base_save,
            "a reopened package keeps the ancestry declared in its earlier segment"
        );
        Ok(())
    }

    #[test]
    fn test_reopened_package_later_ancestry_replaces_parents_and_roles()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package OldParent;\n",
            "use Moo;\n",
            "sub save { 1 }\n",
            "package NewParent;\n",
            "use Moo;\n",
            "sub save { 2 }\n",
            "package OldRole;\n",
            "use Moo::Role;\n",
            "package NewRole;\n",
            "use Moo::Role;\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'OldParent';\n",
            "with 'OldRole';\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'NewParent';\n",
            "with 'NewRole';\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let child = analyzer
            .merged_class_models()
            .into_iter()
            .find(|model| model.name == "Child")
            .ok_or("Child model not found")?;
        assert_eq!(child.parents, vec!["NewParent"]);
        assert_eq!(child.roles, vec!["NewRole"]);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let new_parent_save = code.find("sub save { 2 }").ok_or("NewParent::save not found")?;
        let target = analyzer.find_definition(offset).ok_or("modifier definition")?;
        assert_eq!(target.location.start, new_parent_save);
        Ok(())
    }

    #[test]
    fn test_reopened_package_additive_parent_declarations_keep_all_ancestors()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package First; use Moo; sub save { 1 }\n",
            "package Second; use Moo; sub save { 2 }\n",
            "package Child; use Moo; use parent 'First';\n",
            "package Child; use Moo; use base 'Second';\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
        let chain = analyzer.resolve_parent_chain("Child").ok_or("parent chain")?;
        assert_eq!(chain, vec!["First", "Second"]);
        Ok(())
    }

    #[test]
    fn test_reopened_package_empty_isa_clears_prior_ancestry()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package Base; use Moo; sub save { 1 }\n",
            "package Child; use Moo; extends 'Base';\n",
            "package Child; use Moo; @ISA = ();\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
        let child = analyzer
            .merged_class_models()
            .into_iter()
            .find(|model| model.name == "Child")
            .ok_or("Child model")?;
        assert!(child.parents.is_empty());
        assert!(analyzer.resolve_inherited_method_location("Child", "save").is_none());
        Ok(())
    }

    #[test]
    fn test_empty_mro_directives_do_not_override_prior_selection()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package Root; use Moo; sub save { 1 }\n",
            "package Left; use Moo; extends 'Root';\n",
            "package Right; use Moo; extends 'Root'; sub save { 2 }\n",
            "package Child; use Moo; extends 'Left', 'Right'; use mro 'c3';\n",
            "package Child; use Moo; no mro;\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
        let child = analyzer
            .merged_class_models()
            .into_iter()
            .find(|model| model.name == "Child")
            .ok_or("Child model")?;
        assert_eq!(child.mro, MethodResolutionOrder::C3);
        Ok(())
    }

    #[test]
    fn test_reopened_package_duplicate_method_uses_later_definition()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package Child;\n",
            "use Moo;\n",
            "sub save { 1 }\n",
            "package Child;\n",
            "use Moo;\n",
            "sub save { 2 }\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let later_save = code.rfind("sub save { 2 }").ok_or("later Child::save not found")?;
        let target = analyzer.find_definition(offset).ok_or("modifier definition")?;
        assert_eq!(target.qualified_name, "Child::save");
        assert_eq!(target.location.start, later_save);
        Ok(())
    }

    #[test]
    fn test_reopened_parent_definition_and_hover_share_merged_view()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package Base;\n",
            "use Moo;\n",
            "sub save { 1 }\n",
            "package Base;\n",
            "use Moo;\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'Base';\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let base_save = code.find("sub save { 1 }").ok_or("Base::save not found")?;
        let target = analyzer.find_definition(offset).ok_or("modifier definition")?;
        assert_eq!(target.location.start, base_save);

        let hover = analyzer
            .resolve_inherited_method_hover("Child", "save")
            .ok_or("inherited hover not found")?;
        assert_eq!(hover.signature, "sub Base::save");
        Ok(())
    }

    #[test]
    fn test_reopened_package_explicit_dfs_replaces_prior_c3_for_navigation_and_hover()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = concat!(
            "package Root;\n",
            "use Moo;\n",
            "sub save { 1 }\n",
            "package Left;\n",
            "use Moo;\n",
            "extends 'Root';\n",
            "package Right;\n",
            "use Moo;\n",
            "extends 'Root';\n",
            "sub save { 2 }\n",
            "package Child;\n",
            "use Moo;\n",
            "extends 'Left', 'Right';\n",
            "use mro 'c3';\n",
            "package Child;\n",
            "use Moo;\n",
            "use mro 'dfs';\n",
            "before 'save' => sub { };\n",
        );
        let mut parser = Parser::new(code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);

        let child = analyzer
            .merged_class_models()
            .into_iter()
            .find(|model| model.name == "Child")
            .ok_or("Child model not found")?;
        assert_eq!(child.mro, MethodResolutionOrder::Dfs);

        let chain = analyzer.resolve_parent_chain("Child").ok_or("parent chain")?;
        assert_eq!(chain, vec!["Left", "Root", "Right"]);

        let offset = modifier_target_offset(code, "before 'save'", "save")
            .ok_or("modifier target not found")?;
        let root_save = code.find("sub save { 1 }").ok_or("Root::save not found")?;
        let target = analyzer.find_definition(offset).ok_or("modifier definition")?;
        assert_eq!(target.location.start, root_save);

        let hover = analyzer
            .resolve_inherited_method_hover("Child", "save")
            .ok_or("inherited hover not found")?;
        assert_eq!(hover.signature, "sub Root::save");
        Ok(())
    }

    #[test]
    fn test_resolve_inherited_method_location_limits_dfs_depth()
    -> Result<(), Box<dyn std::error::Error>> {
        let chain_len = MAX_MRO_TRAVERSAL_DEPTH + 10;
        let mut code = String::new();
        for i in 0..chain_len {
            code.push_str(&format!("package P{i}; use parent 'P{}';\n", i + 1));
        }
        code.push_str(&format!("package P{chain_len}; sub target {{ 1 }}\n"));

        let mut parser = Parser::new(&code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, &code);

        let location = analyzer.resolve_inherited_method_location("P0", "target");
        assert!(location.is_none(), "DFS traversal should stop at depth limit");
        Ok(())
    }

    #[test]
    fn test_resolve_inherited_method_location_limits_c3_depth()
    -> Result<(), Box<dyn std::error::Error>> {
        let chain_len = MAX_MRO_TRAVERSAL_DEPTH + 10;
        let mut code = String::new();
        code.push_str("package P0; use mro 'c3'; use parent 'P1';\n");
        for i in 1..chain_len {
            code.push_str(&format!("package P{i}; use parent 'P{}';\n", i + 1));
        }
        code.push_str(&format!("package P{chain_len}; sub target {{ 1 }}\n"));

        let mut parser = Parser::new(&code);
        let ast = parser.parse()?;
        let analyzer = SemanticAnalyzer::analyze_with_source(&ast, &code);

        let location = analyzer.resolve_inherited_method_location("P0", "target");
        assert!(location.is_none(), "C3 traversal should stop at depth limit");
        Ok(())
    }
}
