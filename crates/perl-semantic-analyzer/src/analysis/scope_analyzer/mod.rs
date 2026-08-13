//! Scope analysis and variable tracking for Perl parsing workflows
//!
//! This module provides comprehensive scope analysis for Perl scripts, tracking
//! variable declarations, usage patterns, and potential issues across different
//! scopes within the LSP workflow stages.
//!
//! # LSP Workflow Integration
//!
//! Scope analysis supports semantic validation across LSP workflow stages:
//! - **Parse**: Identify declarations and scopes during syntax analysis
//! - **Index**: Provide scope metadata for symbol indexing
//! - **Navigate**: Resolve references with scope-aware lookups
//! - **Complete**: Filter completion items based on visible bindings
//! - **Analyze**: Report unused, shadowed, and undeclared variables
//!
//! # Performance
//!
//! - **Time complexity**: O(n) over AST nodes with scoped hash lookups
//! - **Space complexity**: O(n) for scope tables and variable maps (memory bounded)
//! - **Optimizations**: Fast sigil indexing to keep performance stable
//! - **Benchmarks**: Typically <5ms for mid-sized files, low ms for large files
//! - **Large file scaling**: Designed to scale across large file sets in workspaces
//!
//! # Usage Examples
//!
//! ```rust,ignore
//! use perl_parser::scope_analyzer::{ScopeAnalyzer, IssueKind};
//! use perl_parser::{Parser, ast::Node};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Analyze Perl script for scope issues
//! let script = "my $var = 42; sub hello { print $var; }";
//! let mut parser = Parser::new(script);
//! let ast = parser.parse()?;
//!
//! let analyzer = ScopeAnalyzer::new();
//! let pragma_map = vec![];
//! let issues = analyzer.analyze(&ast, script, &pragma_map);
//!
//! // Check for common scope issues in Perl parsing code
//! for issue in &issues {
//!     match issue.kind {
//!         IssueKind::UnusedVariable => println!("Unused variable: {}", issue.variable_name),
//!         IssueKind::VariableShadowing => println!("Variable shadowing: {}", issue.variable_name),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```

mod calls_and_exprs;
mod declarations;
mod interpolation;
mod scope_constructs;
mod uses;

use crate::ast::{Node, NodeKind};
use crate::pragma_tracker::{PragmaQueryCursor, PragmaState};
use perl_module::import::resolve_known_export_tag;
use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

/// Category of scope-related issue detected during analysis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum IssueKind {
    /// A variable declared in an inner scope shadows one in an outer scope.
    #[default]
    VariableShadowing,
    /// A declared variable is never read.
    UnusedVariable,
    /// A variable is used without a prior declaration (`my`/`our`/`local`).
    UndeclaredVariable,
    /// The same variable name is declared twice in the same scope.
    VariableRedeclaration,
    /// A subroutine parameter name appears more than once in the signature.
    DuplicateParameter,
    /// A parameter name shadows a package-level (`our`) variable.
    ParameterShadowsGlobal,
    /// A subroutine parameter is never used inside the body.
    UnusedParameter,
    /// A bareword was used where a string or identifier was expected.
    UnquotedBareword,
    /// A variable was accessed before any initializing assignment.
    UninitializedVariable,
    /// Capture variable (`$1`, `$2`, etc.) used with no preceding regex match in scope.
    CaptureVarWithoutRegexMatch,
    /// A feature-gated keyword (e.g. `say`) was used without the enabling
    /// `use feature '...'` / `use vX.Y` pragma active at that offset.
    FeatureNotEnabled,
    /// A package-qualified function call (`Foo::bar()`) under `use strict` names
    /// a sub that is not defined in the (in-file) target package (#3014).
    /// Only emitted when the target package is itself declared in this file;
    /// external modules are never flagged.
    UnresolvedQualifiedCall,
}

/// A single scope-analysis finding with location and human-readable description.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ScopeIssue {
    /// The category of scope problem detected.
    pub kind: IssueKind,
    /// The bare variable name (without sigil) involved in the issue.
    pub variable_name: String,
    /// Zero-based line number of the first token of the offending construct.
    pub line: usize,
    /// Byte offset range `(start, end)` of the offending construct.
    pub range: (usize, usize),
    /// Human-readable explanation of the issue.
    pub description: String,
}

#[derive(Debug)]
struct Variable {
    declaration_offset: usize,
    is_used: RefCell<bool>,
    is_our: bool,
    is_initialized: RefCell<bool>,
}

/// Convert a Perl sigil to an array index for fast variable lookup.
///
/// Sigil indices:
/// - `$` (scalar): 0
/// - `@` (array): 1
/// - `%` (hash): 2
/// - `&` (subroutine): 3
/// - `*` (glob): 4
/// - Other: 5 (fallback)
#[inline]
pub(super) fn sigil_to_index(sigil: &str) -> usize {
    // Use first byte for fast comparison - sigils are always single ASCII chars
    match sigil.as_bytes().first() {
        Some(b'$') => 0,
        Some(b'@') => 1,
        Some(b'%') => 2,
        Some(b'&') => 3,
        Some(b'*') => 4,
        _ => 5,
    }
}

/// Convert an array index back to a Perl sigil.
#[inline]
fn index_to_sigil(index: usize) -> &'static str {
    match index {
        0 => "$",
        1 => "@",
        2 => "%",
        3 => "&",
        4 => "*",
        _ => "",
    }
}

#[derive(Debug)]
pub(super) struct Scope {
    // Outer key: sigil index, Inner key: name
    variables: RefCell<[Option<FxHashMap<String, Rc<Variable>>>; 6]>,
    parent: Option<Rc<Scope>>,
    /// Whether a regex match operation (`=~`, `m//`, `s///`) has been seen in this scope.
    has_regex_match: Cell<bool>,
}

impl Scope {
    fn new() -> Self {
        let vars = std::array::from_fn(|_| None);
        Self { variables: RefCell::new(vars), parent: None, has_regex_match: Cell::new(false) }
    }

    fn with_parent(parent: Rc<Scope>) -> Self {
        let vars = std::array::from_fn(|_| None);
        Self {
            variables: RefCell::new(vars),
            parent: Some(parent),
            has_regex_match: Cell::new(false),
        }
    }

    /// Returns true if this scope or any ancestor scope has seen a regex match operation.
    fn regex_match_in_scope(&self) -> bool {
        if self.has_regex_match.get() {
            return true;
        }
        if let Some(ref parent) = self.parent { parent.regex_match_in_scope() } else { false }
    }

    fn declare_variable_parts(
        &self,
        sigil: &str,
        name: &str,
        offset: usize,
        is_our: bool,
        is_initialized: bool,
    ) -> Option<IssueKind> {
        let idx = sigil_to_index(sigil);

        // First check if already declared in this scope
        {
            let vars = self.variables.borrow();
            if let Some(map) = &vars[idx] {
                if map.contains_key(name) {
                    return Some(IssueKind::VariableRedeclaration);
                }
            }
        }

        // Check if it shadows a parent scope variable
        let shadows = if let Some(ref parent) = self.parent {
            parent.has_variable_parts(sigil, name)
        } else {
            false
        };

        // Now insert the variable
        let mut vars = self.variables.borrow_mut();
        let inner = vars[idx].get_or_insert_with(FxHashMap::default);

        inner.insert(
            name.to_string(),
            Rc::new(Variable {
                declaration_offset: offset,
                is_used: RefCell::new(is_our), // 'our' variables are considered used
                is_our,
                is_initialized: RefCell::new(is_initialized),
            }),
        );

        if shadows { Some(IssueKind::VariableShadowing) } else { None }
    }

    fn has_variable_parts(&self, sigil: &str, name: &str) -> bool {
        let idx = sigil_to_index(sigil);
        let mut current_scope = self;

        loop {
            {
                let vars = current_scope.variables.borrow();
                if let Some(map) = &vars[idx] {
                    if map.contains_key(name) {
                        return true;
                    }
                }
            }
            if let Some(ref parent) = current_scope.parent {
                current_scope = parent;
            } else {
                return false;
            }
        }
    }

    fn use_variable_parts(&self, sigil: &str, name: &str) -> (bool, bool) {
        let idx = sigil_to_index(sigil);
        let mut current_scope = self;

        loop {
            {
                let vars = current_scope.variables.borrow();
                if let Some(map) = &vars[idx] {
                    if let Some(var) = map.get(name) {
                        *var.is_used.borrow_mut() = true;
                        return (true, *var.is_initialized.borrow());
                    }
                }
            }

            if let Some(ref parent) = current_scope.parent {
                current_scope = parent;
            } else {
                return (false, false);
            }
        }
    }

    fn initialize_variable_parts(&self, sigil: &str, name: &str) {
        let idx = sigil_to_index(sigil);
        let mut current_scope = self;

        loop {
            {
                let vars = current_scope.variables.borrow();
                if let Some(map) = &vars[idx] {
                    if let Some(var) = map.get(name) {
                        *var.is_initialized.borrow_mut() = true;
                        return;
                    }
                }
            }

            if let Some(ref parent) = current_scope.parent {
                current_scope = parent;
            } else {
                return;
            }
        }
    }

    /// Optimized method to mark a variable as initialized AND used in one lookup.
    /// Returns true if the variable was found and updated.
    fn initialize_and_use_variable_parts(&self, sigil: &str, name: &str) -> bool {
        let idx = sigil_to_index(sigil);
        let mut current_scope = self;

        loop {
            {
                let vars = current_scope.variables.borrow();
                if let Some(map) = &vars[idx] {
                    if let Some(var) = map.get(name) {
                        *var.is_used.borrow_mut() = true;
                        *var.is_initialized.borrow_mut() = true;
                        return true;
                    }
                }
            }

            if let Some(ref parent) = current_scope.parent {
                current_scope = parent;
            } else {
                return false;
            }
        }
    }

    /// Iterate over unused variables that should be reported as diagnostics.
    /// Filters out underscore-prefixed variables (intentionally unused) before allocation.
    fn for_each_reportable_unused_variable<F>(&self, mut f: F)
    where
        F: FnMut(String, usize),
    {
        for (idx, inner_opt) in self.variables.borrow().iter().enumerate() {
            if let Some(inner) = inner_opt {
                for (name, var) in inner {
                    if !*var.is_used.borrow() && !var.is_our {
                        // Optimization: Check for underscore prefix before allocation
                        if name.starts_with('_') {
                            continue;
                        }
                        // Auto-suppress unused $self in plain subs — it's the
                        // dominant Moose/Moo invocant idiom and flagging it is
                        // more noisy than useful (#5060 item 3).
                        if name == "self" && idx == 0 {
                            // idx 0 = scalar sigil. Only skip if this scope's
                            // parent is a subroutine scope (not Method, which
                            // already pre-marks $self as used).
                            continue;
                        }
                        let full_name = format!("{}{}", index_to_sigil(idx), name);
                        f(full_name, var.declaration_offset);
                    }
                }
            }
        }
    }
}

/// Helper to split a full variable name into sigil and name parts.
pub(super) fn split_variable_name(full_name: &str) -> (&str, &str) {
    if !full_name.is_empty() {
        let c = full_name.as_bytes()[0];
        if c == b'$' || c == b'@' || c == b'%' || c == b'&' || c == b'*' {
            return (&full_name[0..1], &full_name[1..]);
        }
    }
    ("", full_name)
}

fn is_interpolated_var_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_interpolated_var_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':'
}

fn has_escaped_interpolation_marker(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return false;
    }

    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }

    backslashes % 2 == 1
}

pub(super) enum ExtractedName<'a> {
    Parts(&'a str, &'a str),
    Full(String),
}

pub(super) struct AnalysisContext<'a> {
    code: &'a str,
    pragma_map: &'a [(Range<usize>, PragmaState)],
    pragma_cursor: RefCell<PragmaQueryCursor>,
    imported_barewords: HashSet<String>,
    /// Names of subroutines defined anywhere in this file. Used to suppress the
    /// feature-gate diagnostic when a user has shadowed a feature-gated keyword
    /// with their own `sub` (e.g. `sub say { ... } say(...)`).
    defined_subs: HashSet<String>,
    /// Package names declared anywhere in this file via `package Foo;` or
    /// `package Foo { ... }` (excluding the implicit `main`).  Used by the
    /// strict-subs check for package-qualified calls (#3014): a call to
    /// `Foo::bar()` only produces a diagnostic when `Foo` is a package defined
    /// in this file and `bar` is not among its defined subs. External packages
    /// (loaded via `use`/`require`) are never flagged because we cannot know
    /// which subs they export.
    defined_packages: HashSet<String>,
    /// Whether a top-level `use vX.Y` / `use N.NNN` version pragma is declared.
    /// When one is, the feature-gate diagnostic (e.g. for `say`) defers to the
    /// version-compatibility lint (`PL900`), which owns the version-declared case
    /// with a more specific message — avoiding a duplicate diagnostic (#2584).
    has_declared_version: bool,
    line_starts: RefCell<Option<Vec<usize>>>,
    /// Current package name, updated as `package` statements are traversed.
    current_package: RefCell<String>,
    /// Monotonic counter incremented on every `package X` declaration.
    ///
    /// Used together with `our_decl_generations` to distinguish a true same-package
    /// redeclaration (`our $x; our $x;` without switching packages) from a legitimate
    /// re-import after a package switch (`package A; our $x; package B; our $x; package A; our $x;`).
    pub(super) package_change_generation: Cell<u64>,
    /// Maps the qualified name of each `our`-declared variable (e.g., `"Foo::x"`) to the
    /// `package_change_generation` value at the time of its most recent declaration.
    ///
    /// On first declaration the entry is inserted.  On a subsequent declaration:
    /// - same generation → same package visit → `VariableRedeclaration` is emitted.
    /// - different generation → package switched since last declaration → silently accepted
    ///   as a re-import; the entry is updated to the new generation.
    pub(super) our_decl_generations: RefCell<HashMap<String, u64>>,
}

impl<'a> AnalysisContext<'a> {
    fn new(ast: &Node, code: &'a str, pragma_map: &'a [(Range<usize>, PragmaState)]) -> Self {
        Self {
            code,
            pragma_map,
            pragma_cursor: RefCell::new(PragmaQueryCursor::new()),
            imported_barewords: collect_imported_barewords(ast),
            defined_subs: collect_defined_subs(ast),
            defined_packages: collect_defined_packages(ast),
            has_declared_version: has_declared_perl_version(ast),
            line_starts: RefCell::new(None),
            current_package: RefCell::new("main".to_string()),
            package_change_generation: Cell::new(0),
            our_decl_generations: RefCell::new(HashMap::new()),
        }
    }

    fn pragma_state_for_offset(&self, offset: usize) -> PragmaState {
        self.pragma_cursor.borrow_mut().state_for_offset(self.pragma_map, offset)
    }

    fn has_imported_bareword(&self, name: &str) -> bool {
        self.imported_barewords.contains(name)
    }

    /// Whether a `sub` named `name` is defined in the package that an unqualified
    /// call at the current position would resolve to. An unqualified name resolves
    /// against the active package; an explicitly-qualified name (`Foo::bar`) is
    /// matched directly. Keeps a `sub say` in one package from suppressing the
    /// feature gate for `say` in a different package of the same file (#4892).
    fn has_defined_sub(&self, name: &str) -> bool {
        if name.contains("::") {
            self.defined_subs.contains(name)
        } else {
            let pkg = self.current_package.borrow();
            self.defined_subs.contains(&format!("{}::{}", pkg.as_str(), name))
        }
    }

    fn has_declared_version(&self) -> bool {
        self.has_declared_version
    }

    /// Whether a package named `pkg` (e.g. `"Foo"`) is declared in this file.
    /// `main` is always considered defined (implicit). Used by the strict-subs
    /// check for qualified calls (#3014) to avoid false positives on external
    /// modules we cannot introspect.
    pub(super) fn has_defined_package(&self, pkg: &str) -> bool {
        pkg == "main" || self.defined_packages.contains(pkg)
    }

    fn get_line(&self, offset: usize) -> usize {
        let mut line_starts_guard = self.line_starts.borrow_mut();
        let starts = line_starts_guard.get_or_insert_with(|| {
            let mut indices = Vec::with_capacity(self.code.len() / 40); // Estimate
            indices.push(0);
            for (i, b) in self.code.bytes().enumerate() {
                if b == b'\n' {
                    indices.push(i + 1);
                }
            }
            indices
        });

        // Find the line that contains the offset
        match starts.binary_search(&offset) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        }
    }
}

impl<'a> ExtractedName<'a> {
    fn as_string(&self) -> String {
        match self {
            ExtractedName::Parts(sigil, name) => format!("{}{}", sigil, name),
            ExtractedName::Full(s) => s.clone(),
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            ExtractedName::Parts(sigil, name) => (sigil, name),
            ExtractedName::Full(s) => split_variable_name(s),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            ExtractedName::Parts(sigil, name) => sigil.is_empty() && name.is_empty(),
            ExtractedName::Full(s) => s.is_empty(),
        }
    }
}

/// Analyzes an AST for scope-related issues such as unused variables and shadowing.
///
/// Produces a list of [`ScopeIssue`]s that can be surfaced as LSP diagnostics
/// or used by the refactoring engine.  The analyzer is stateless and may be
/// reused across multiple invocations.
pub struct ScopeAnalyzer;

impl Default for ScopeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopeAnalyzer {
    /// Create a new scope analyzer instance.
    pub fn new() -> Self {
        Self
    }

    pub(super) fn package_variable_name(
        &self,
        name: &str,
        context: &AnalysisContext<'_>,
    ) -> Option<String> {
        if name.is_empty() || name.contains("::") {
            return None;
        }

        let current_package = context.current_package.borrow();
        Some(format!("{}::{}", current_package.as_str(), name))
    }

    pub(super) fn declare_variable_parts_in_context(
        &self,
        scope: &Rc<Scope>,
        sigil: &str,
        name: &str,
        offset: usize,
        is_our: bool,
        is_initialized: bool,
        context: &AnalysisContext<'_>,
    ) -> Option<IssueKind> {
        if is_our && let Some(qualified_name) = self.package_variable_name(name, context) {
            return scope.declare_variable_parts(
                sigil,
                &qualified_name,
                offset,
                is_our,
                is_initialized,
            );
        }

        scope.declare_variable_parts(sigil, name, offset, is_our, is_initialized)
    }

    pub(super) fn has_variable_parts_in_context(
        &self,
        scope: &Rc<Scope>,
        sigil: &str,
        name: &str,
        context: &AnalysisContext<'_>,
    ) -> bool {
        if scope.has_variable_parts(sigil, name) {
            return true;
        }

        self.package_variable_name(name, context)
            .is_some_and(|qualified_name| scope.has_variable_parts(sigil, &qualified_name))
    }

    pub(super) fn use_variable_parts_in_context(
        &self,
        scope: &Rc<Scope>,
        sigil: &str,
        name: &str,
        context: &AnalysisContext<'_>,
    ) -> (bool, bool) {
        let (found, initialized) = scope.use_variable_parts(sigil, name);
        if found {
            return (found, initialized);
        }

        self.package_variable_name(name, context).map_or((false, false), |qualified_name| {
            scope.use_variable_parts(sigil, &qualified_name)
        })
    }

    pub(super) fn initialize_variable_parts_in_context(
        &self,
        scope: &Rc<Scope>,
        sigil: &str,
        name: &str,
        context: &AnalysisContext<'_>,
    ) {
        if scope.has_variable_parts(sigil, name) {
            scope.initialize_variable_parts(sigil, name);
            return;
        }

        if let Some(qualified_name) = self.package_variable_name(name, context) {
            scope.initialize_variable_parts(sigil, &qualified_name);
        }
    }

    pub(super) fn initialize_and_use_variable_parts_in_context(
        &self,
        scope: &Rc<Scope>,
        sigil: &str,
        name: &str,
        context: &AnalysisContext<'_>,
    ) -> bool {
        if scope.initialize_and_use_variable_parts(sigil, name) {
            return true;
        }

        self.package_variable_name(name, context).is_some_and(|qualified_name| {
            scope.initialize_and_use_variable_parts(sigil, &qualified_name)
        })
    }

    /// Analyze `ast` for scope issues, using `pragma_map` to honour `use strict` regions.
    ///
    /// Returns all detected issues sorted by byte offset.
    pub fn analyze(
        &self,
        ast: &Node,
        code: &str,
        pragma_map: &[(Range<usize>, PragmaState)],
    ) -> Vec<ScopeIssue> {
        let mut issues = Vec::new();
        let root_scope = Rc::new(Scope::new());

        // Use a vector as a stack for ancestors to avoid O(N) HashMap allocation
        let mut ancestors: Vec<&Node> = Vec::new();

        let context = AnalysisContext::new(ast, code, pragma_map);

        self.analyze_node(ast, &root_scope, &mut ancestors, &mut issues, &context);

        // Collect all unused variables from all scopes
        self.collect_unused_variables(&root_scope, &mut issues, &context);

        issues
    }

    pub(super) fn analyze_node<'a>(
        &self,
        node: &'a Node,
        scope: &Rc<Scope>,
        ancestors: &mut Vec<&'a Node>,
        issues: &mut Vec<ScopeIssue>,
        context: &AnalysisContext<'a>,
    ) {
        // Get effective pragma state at this node's location
        let pragma_state = context.pragma_state_for_offset(node.location.start);
        let strict_vars_mode = pragma_state.strict_vars || pragma_state.signatures_strict;
        let strict_subs_mode = pragma_state.strict_subs || pragma_state.signatures_strict;
        match &node.kind {
            NodeKind::VariableDeclaration { declarator, variable, initializer, .. } => {
                let _ = declarations::handle_variable_declaration(
                    self,
                    node,
                    declarator,
                    variable,
                    initializer.as_deref(),
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::VariableListDeclaration { declarator, variables, initializer, .. } => {
                declarations::handle_variable_list_declaration(
                    self,
                    initializer.as_deref(),
                    declarator,
                    variables,
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::Use { module, args, .. } => {
                declarations::handle_use(self, node, module, args, scope, context);
            }
            NodeKind::Variable { sigil, name } => {
                let _ = uses::handle_variable(
                    self,
                    node,
                    sigil,
                    name,
                    scope,
                    ancestors,
                    issues,
                    context,
                    strict_vars_mode,
                );
            }
            NodeKind::Typeglob { name } => {
                uses::handle_typeglob(self, node, name, scope, issues, context, strict_vars_mode);
            }
            NodeKind::Readline { filehandle: Some(filehandle) } => {
                uses::handle_readline(
                    self,
                    node,
                    filehandle,
                    scope,
                    issues,
                    context,
                    strict_vars_mode,
                );
            }
            NodeKind::FunctionCall { name, args } => {
                calls_and_exprs::handle_function_call(
                    self,
                    node,
                    name,
                    args,
                    scope,
                    ancestors,
                    issues,
                    context,
                    &pragma_state,
                    strict_vars_mode,
                    strict_subs_mode,
                );
            }
            NodeKind::AmperCall { name, args } => {
                calls_and_exprs::handle_amper_call(
                    self,
                    node,
                    name,
                    args,
                    scope,
                    ancestors,
                    issues,
                    context,
                    strict_vars_mode,
                );
            }
            NodeKind::MethodCall { object, method, args } => {
                calls_and_exprs::handle_method_call(
                    self,
                    node,
                    object,
                    method,
                    args,
                    scope,
                    ancestors,
                    issues,
                    context,
                    strict_vars_mode,
                );
            }
            NodeKind::Unary { op: _, operand } => {
                calls_and_exprs::handle_unary(
                    self, node, operand, scope, ancestors, issues, context,
                );
            }
            NodeKind::String { value, interpolated } => {
                interpolation::handle_string(self, value, *interpolated, scope, context);
            }
            NodeKind::Heredoc { content, interpolated, .. } => {
                interpolation::handle_heredoc(self, content, *interpolated, scope, context);
            }
            NodeKind::Assignment { lhs, rhs, op: _ } => {
                let _ = uses::handle_assignment(
                    self, node, lhs, rhs, scope, ancestors, issues, context,
                );
            }

            NodeKind::Tie { variable, package, args } => {
                uses::handle_tie(
                    self, node, variable, package, args, scope, ancestors, issues, context,
                );
            }

            NodeKind::Untie { variable } => {
                uses::handle_untie(self, node, variable, scope, ancestors, issues, context);
            }

            NodeKind::Identifier { name } => {
                uses::handle_identifier(
                    self,
                    node,
                    name,
                    issues,
                    context,
                    ancestors,
                    &pragma_state,
                    strict_subs_mode,
                );
            }

            NodeKind::Binary { op: _, left, right } => {
                // All binary operations (including {} and [])
                // We don't need special handling for {} and [] here because NodeKind::Variable
                // will handle the context-sensitive lookup (checking ancestors).
                calls_and_exprs::handle_binary(
                    self, node, left, right, scope, ancestors, issues, context,
                );
            }

            NodeKind::ArrayLiteral { elements } => {
                calls_and_exprs::handle_array_literal(
                    self, node, elements, scope, ancestors, issues, context,
                );
            }

            NodeKind::Block { statements } => {
                scope_constructs::handle_block(
                    self, node, statements, scope, ancestors, issues, context,
                );
            }

            NodeKind::PhaseBlock { block, .. } => {
                scope_constructs::handle_phase_block(
                    self, node, block, scope, ancestors, issues, context,
                );
            }

            NodeKind::For { init, condition, update, body, .. } => {
                scope_constructs::handle_for(
                    self,
                    node,
                    init.as_deref(),
                    condition.as_deref(),
                    update.as_deref(),
                    body,
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::Foreach { variable, list, body, continue_block } => {
                scope_constructs::handle_foreach(
                    self,
                    node,
                    variable,
                    list,
                    body,
                    continue_block.as_deref(),
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::Subroutine { signature, body, .. } => {
                scope_constructs::handle_subroutine(
                    self,
                    node,
                    signature.as_deref(),
                    body,
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            // Perl 5.38+ `use feature 'class'` method declarations.
            // Like a subroutine but `$self` is an implicit invocant that must be
            // pre-declared in the method scope to avoid false UndeclaredVariable
            // diagnostics when authors write `$self->foo` inside the body.
            NodeKind::Method { signature, body, .. } => {
                scope_constructs::handle_method(
                    self,
                    node,
                    signature.as_deref(),
                    body,
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            // Perl 5.38+ class block.  Introduces a new package-level scope so
            // that field declarations and method definitions do not leak into the
            // enclosing lexical scope.
            NodeKind::Class { name, body, .. } => {
                scope_constructs::handle_package(
                    self,
                    node,
                    name,
                    Some(body),
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::Try { body, catch_blocks, finally_block } => {
                scope_constructs::handle_try(
                    self,
                    node,
                    body,
                    catch_blocks,
                    finally_block.as_deref(),
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            NodeKind::Package { name, block, .. } => {
                scope_constructs::handle_package(
                    self,
                    node,
                    name,
                    block.as_deref(),
                    scope,
                    ancestors,
                    issues,
                    context,
                );
            }

            // Regex match operations set capture variables ($1, $2, ...) in the current scope.
            NodeKind::Match { expr, .. } => {
                interpolation::handle_match(self, node, expr, scope, ancestors, issues, context);
            }

            NodeKind::Substitution { expr, .. } => {
                interpolation::handle_substitution(
                    self, node, expr, scope, ancestors, issues, context,
                );
            }

            // Standalone regex (m// matching against $_) also sets capture variables.
            NodeKind::Regex { .. } => {
                interpolation::handle_regex(scope);
            }

            NodeKind::StatementModifier { statement, condition, .. } => {
                // Perl hoists a `my` declaration in the modifier condition to the
                // enclosing block, so the condition must be analyzed BEFORE the
                // statement.  The default children() order is statement-first,
                // which causes a false-positive UndeclaredVariable for idioms like
                //   `print $x if my $x = 1;`
                // Analyze the condition first so any `my` it introduces is visible
                // to the statement.
                ancestors.push(node);
                self.analyze_node(condition, scope, ancestors, issues, context);
                self.analyze_node(statement, scope, ancestors, issues, context);
                ancestors.pop();
            }

            _ => {
                // Recursively analyze children
                ancestors.push(node);
                for child in node.children() {
                    self.analyze_node(child, scope, ancestors, issues, context);
                }
                ancestors.pop();
            }
        }
    }

    /// Resolve the variable symbol that a syntax form should count as a use.
    ///
    /// This keeps explicit dereference syntax precise:
    /// - `@$ref` and `%$ref` count as uses of `$ref`
    /// - `$arr[0]` counts as a use of `@arr`
    /// - `$hash{k}` counts as a use of `%hash`
    /// - Arrow dereference forms stay on the scalar reference itself
    pub(super) fn resolve_variable_use_target<'a>(
        &self,
        node: &'a Node,
        ancestors: &[&'a Node],
        context: &AnalysisContext<'_>,
    ) -> Option<(&'a str, &'a str)> {
        let NodeKind::Variable { sigil, name } = &node.kind else {
            return None;
        };

        // Explicit scalar-reference dereference forms should count as uses of the
        // underlying scalar lexical (`$ref`) rather than a container lexical of the
        // same bare name. This covers compact and braced syntaxes such as:
        // - `@$ref`, `%$ref`, `$$ref`
        // - `@{$ref}`, `%{$ref}`, `${$ref}`
        if (sigil == "@" || sigil == "%" || sigil == "$")
            && context
                .code
                .get(node.location.start..node.location.end)
                .is_some_and(is_explicit_scalar_reference_deref)
        {
            return Some(("$", normalize_scalar_deref_base_name(name)));
        }

        if (sigil == "@" || sigil == "%" || sigil == "$") && name.starts_with('$') && name.len() > 1
        {
            return Some(("$", &name[1..]));
        }

        if sigil == "$"
            && let Some(parent) = ancestors.last()
            && let NodeKind::Binary { op, left, right } = &parent.kind
            && std::ptr::eq(left.as_ref(), node)
        {
            match op.as_str() {
                "[]" => return Some(("@", name)),
                "->[]" | "->{}" => return Some(("$", name)),
                "{}" if self.is_dynamic_method_deref_rhs(right)
                    || self.is_dynamic_method_deref_context(parent, ancestors)
                    || self.is_braced_dynamic_method_call(parent, context) =>
                {
                    return Some(("$", name));
                }
                "{}" => return Some(("%", name)),
                _ => {}
            }
        }

        // Hash slice syntax (`@hash{...}`) reads from `%hash`, not a lexical `@hash`.
        // Bridge this so strict-vars and usage tracking resolve against the declared hash.
        if sigil == "@"
            && let Some(parent) = ancestors.last()
            && let NodeKind::HashSlice { target, .. } = &parent.kind
            && std::ptr::eq(target.as_ref(), node)
        {
            return Some(("%", name));
        }

        // When the parser interprets `print $arr[0]` as indirect-object syntax, it produces
        // `IndirectCall { object: Variable($, "arr"), args: [ArrayLiteral([0])] }`.
        // Similarly, `print $hash{a}` produces
        // `IndirectCall { object: Variable($, "hash"), args: [Block([a])] }`.
        // Bridge the sigil so that `@arr` / `%hash` are marked as used, not `$arr` / `$hash`.
        if sigil == "$"
            && let Some(parent) = ancestors.last()
            && let NodeKind::IndirectCall { object, args, .. } = &parent.kind
            && std::ptr::eq(object.as_ref(), node)
        {
            if let Some(first_arg) = args.first() {
                match &first_arg.kind {
                    NodeKind::ArrayLiteral { .. } => return Some(("@", name)),
                    NodeKind::Block { .. } => return Some(("%", name)),
                    _ => {}
                }
            }
        }

        Some((sigil, name))
    }

    pub(super) fn extract_name_like_variable<'a>(
        &self,
        name: &'a str,
    ) -> Option<(&'a str, &'a str)> {
        let (sigil, var_name) = split_variable_name(name);
        if sigil.is_empty()
            || var_name.is_empty()
            || var_name.contains("::")
            || !self.looks_like_variable_name(var_name)
        {
            return None;
        }
        Some((sigil, var_name))
    }

    pub(super) fn extract_method_name_variable<'a>(
        &self,
        method: &'a str,
    ) -> Option<(&'a str, &'a str)> {
        self.extract_name_like_variable(method).or_else(|| {
            let inner = method.strip_prefix("${")?.strip_suffix('}')?;
            if inner.contains("::") || !self.looks_like_variable_name(inner) {
                return None;
            }
            Some(("$", inner))
        })
    }

    pub(super) fn looks_like_variable_name(&self, name: &str) -> bool {
        matches!(
            name.chars().next(),
            Some('A'..='Z' | 'a'..='z' | '_' | '$' | '@' | '%' | '&' | '*' | '^' | '#' | '!' | '?')
        )
    }

    pub(super) fn is_dynamic_method_deref_rhs(&self, node: &Node) -> bool {
        matches!(
            &node.kind,
            NodeKind::Unary { op, operand }
                if op == "\\"
                    && matches!(
                        &operand.kind,
                        NodeKind::String { .. } | NodeKind::Identifier { .. }
                    )
        )
    }

    pub(super) fn is_dynamic_method_deref_context<'a>(
        &self,
        node: &'a Node,
        ancestors: &[&'a Node],
    ) -> bool {
        let Some(grandparent) = ancestors.iter().rev().nth(1).copied() else {
            return false;
        };

        match &grandparent.kind {
            NodeKind::MethodCall { object, .. } => std::ptr::eq(object.as_ref(), node),
            NodeKind::FunctionCall { name, args } if name == "->()" => {
                args.first().is_some_and(|arg| std::ptr::eq(arg, node))
            }
            _ => false,
        }
    }

    pub(super) fn is_braced_dynamic_method_call(
        &self,
        node: &Node,
        context: &AnalysisContext<'_>,
    ) -> bool {
        let Some(selector_text) = context.code.get(node.location.start..node.location.end) else {
            return false;
        };
        if !selector_text.contains("->${") {
            return false;
        }

        let Some(suffix) = context.code.get(node.location.end..) else {
            return false;
        };
        suffix.trim_start().starts_with("()")
    }

    pub(super) fn record_variable_use(
        &self,
        scope: &Rc<Scope>,
        strict_vars_mode: bool,
        context: &AnalysisContext<'_>,
        issues: &mut Vec<ScopeIssue>,
        node: &Node,
        sigil: &str,
        name: &str,
    ) {
        let (variable_used, is_initialized) =
            self.use_variable_parts_in_context(scope, sigil, name, context);
        if !variable_used {
            if strict_vars_mode {
                self.push_undeclared_variable_issue(issues, context, node, sigil, name);
            }
        } else if !is_initialized {
            self.push_uninitialized_variable_issue(issues, context, node, sigil, name);
        }
    }

    pub(super) fn push_undeclared_variable_issue(
        &self,
        issues: &mut Vec<ScopeIssue>,
        context: &AnalysisContext<'_>,
        node: &Node,
        sigil: &str,
        name: &str,
    ) {
        let full_name = format!("{}{}", sigil, name);
        issues.push(ScopeIssue {
            kind: IssueKind::UndeclaredVariable,
            variable_name: full_name.clone(),
            line: context.get_line(node.location.start),
            range: (node.location.start, node.location.end),
            description: format!("Variable '{}' is used but not declared", full_name),
        });
    }

    pub(super) fn push_uninitialized_variable_issue(
        &self,
        issues: &mut Vec<ScopeIssue>,
        context: &AnalysisContext<'_>,
        node: &Node,
        sigil: &str,
        name: &str,
    ) {
        // Honour `no warnings 'uninitialized'` (#2584). The pragma model already
        // computed for this file records disabled warning categories in
        // source-ordered lexical ranges, so querying the effective state at this
        // use site gives lexically-correct suppression: `no warnings 'uninitialized'`
        // silences the diagnostic within its scope, and a later
        // `use warnings 'uninitialized'` re-enables it — all in source order via the
        // range map. See `uninitialized_warning_suppressed` for why this gates on
        // the specific category rather than the global `warnings` bit or a blanket
        // `all`.
        if uninitialized_warning_suppressed(&context.pragma_state_for_offset(node.location.start)) {
            return;
        }
        let full_name = format!("{}{}", sigil, name);
        issues.push(ScopeIssue {
            kind: IssueKind::UninitializedVariable,
            variable_name: full_name.clone(),
            line: context.get_line(node.location.start),
            range: (node.location.start, node.location.end),
            description: format!("Variable '{}' is used before being initialized", full_name),
        });
    }

    /// Marks variables as initialized when they appear on the left-hand side of an assignment.
    /// Handles scalar variables, list assignments like `($x, $y) = ...`, and nested structures.
    pub(super) fn mark_initialized(
        &self,
        node: &Node,
        scope: &Rc<Scope>,
        context: &AnalysisContext<'_>,
    ) {
        match &node.kind {
            NodeKind::Variable { sigil, name } => {
                if !name.contains("::") {
                    self.initialize_variable_parts_in_context(scope, sigil, name, context);
                }
            }
            // For all other node types (parens, lists, etc.), recurse into children
            // to find any nested variables that should be marked as initialized
            _ => {
                for child in node.children() {
                    self.mark_initialized(child, scope, context);
                }
            }
        }
    }

    pub(super) fn analyze_block_with_scope<'a>(
        &self,
        node: &'a Node,
        scope: &Rc<Scope>,
        ancestors: &mut Vec<&'a Node>,
        issues: &mut Vec<ScopeIssue>,
        context: &AnalysisContext<'a>,
    ) {
        if let NodeKind::Block { statements } = &node.kind {
            ancestors.push(node);
            for stmt in statements {
                self.analyze_node(stmt, scope, ancestors, issues, context);
            }
            ancestors.pop();
        } else {
            self.analyze_node(node, scope, ancestors, issues, context);
        }
    }

    pub(super) fn mark_builtin_declaration_arg_consumed(
        &self,
        node: &Node,
        scope: &Rc<Scope>,
        context: &AnalysisContext<'_>,
    ) {
        match &node.kind {
            NodeKind::VariableDeclaration { variable, .. } => {
                let extracted = self.extract_variable_name(variable);
                let (sigil, name) = extracted.parts();
                if !sigil.is_empty() && !name.is_empty() && !name.contains("::") {
                    let _ = self
                        .initialize_and_use_variable_parts_in_context(scope, sigil, name, context);
                }
            }
            NodeKind::VariableListDeclaration { variables, .. } => {
                for variable in variables {
                    self.mark_builtin_declaration_arg_consumed(variable, scope, context);
                }
            }
            NodeKind::VariableWithAttributes { variable, .. } => {
                self.mark_builtin_declaration_arg_consumed(variable, scope, context);
            }
            _ => {}
        }
    }

    pub(super) fn mark_interpolated_variables_used(
        &self,
        content: &str,
        scope: &Rc<Scope>,
        context: &AnalysisContext<'_>,
    ) {
        let bytes = content.as_bytes();
        let mut index = 0;

        while index < bytes.len() {
            let sigil = match bytes[index] {
                b'$' => "$",
                b'@' => "@",
                _ => {
                    index += 1;
                    continue;
                }
            };

            if has_escaped_interpolation_marker(bytes, index) {
                index += 1;
                continue;
            }

            if index + 1 >= bytes.len() {
                break;
            }

            let (start, requires_closing_brace) =
                if bytes[index + 1] == b'{' { (index + 2, true) } else { (index + 1, false) };

            if start >= bytes.len() || !is_interpolated_var_start(bytes[start]) {
                index += 1;
                continue;
            }

            let mut end = start + 1;
            while end < bytes.len() && is_interpolated_var_continue(bytes[end]) {
                end += 1;
            }

            if requires_closing_brace && (end >= bytes.len() || bytes[end] != b'}') {
                index += 1;
                continue;
            }

            if let Some(name) = content.get(start..end) {
                if !name.contains("::") {
                    let _ = self.use_variable_parts_in_context(scope, sigil, name, context);
                }
            }

            index = if requires_closing_brace { end + 1 } else { end };
        }
    }

    pub(super) fn collect_unused_variables(
        &self,
        scope: &Rc<Scope>,
        issues: &mut Vec<ScopeIssue>,
        context: &AnalysisContext<'_>,
    ) {
        scope.for_each_reportable_unused_variable(|var_name, offset| {
            let start = offset.min(context.code.len());
            let end = (start + var_name.len()).min(context.code.len());

            // Optimization: Generate description using the string reference before moving it
            let description = format!("Variable '{}' is declared but never used", var_name);

            issues.push(ScopeIssue {
                kind: IssueKind::UnusedVariable,
                variable_name: var_name, // Move: Avoids cloning the string
                line: context.get_line(offset),
                range: (start, end),
                description,
            });
        });
    }

    pub(super) fn extract_variable_name<'a>(&self, node: &'a Node) -> ExtractedName<'a> {
        match &node.kind {
            NodeKind::Variable { sigil, name } => ExtractedName::Parts(sigil, name),
            NodeKind::MandatoryParameter { variable }
            | NodeKind::OptionalParameter { variable, .. }
            | NodeKind::SlurpyParameter { variable }
            | NodeKind::NamedParameter { variable, .. } => self.extract_variable_name(variable),
            NodeKind::ArrayLiteral { elements } => {
                // Handle array reference patterns like @{$ref}
                if elements.len() == 1 {
                    if let Some(first) = elements.first() {
                        return self.extract_variable_name(first);
                    }
                }
                ExtractedName::Full(String::new())
            }
            NodeKind::Binary { op, left, .. } if op == "->" => {
                // Handle method call patterns on variables
                self.extract_variable_name(left)
            }
            _ => {
                if let Some(child) = node.first_child() {
                    self.extract_variable_name(child)
                } else {
                    ExtractedName::Full(String::new())
                }
            }
        }
    }

    /// Determines if a node is in a hash key context, where barewords are legitimate.
    ///
    /// This method efficiently detects various hash key contexts to avoid false positives
    /// in strict mode bareword detection. It handles:
    ///
    /// # Hash Key Contexts Detected:
    /// - **Hash subscripts**: `$hash{bareword_key}` or `%hash{bareword_key}`
    /// - **Hash literals**: `{ key => value, another_key => value2 }`
    /// - **Hash slices**: `@hash{key1, key2, key3}` where keys are in an array
    /// - **Postfix hash slices**: `$ref->%{key}` where the key is auto-quoted
    /// - **Nested hash structures**: Complex nested hash access patterns
    ///
    /// # Performance Characteristics:
    /// - Early termination on first positive match
    /// - Efficient pointer-based parent traversal
    /// - O(depth) complexity where depth is AST nesting level
    /// - Typical case: 1-3 parent checks for hash contexts
    ///
    /// # Examples:
    /// ```perl
    /// use strict;
    /// my %hash = (key1 => 'value1');        # key1 is in hash key context
    /// my $val = $hash{bareword_key};         # bareword_key is in hash key context  
    /// my @vals = @hash{key1, key2};          # key1, key2 are in hash key context
    /// print INVALID_BAREWORD;                # NOT in hash key context - should warn
    /// ```
    pub(super) fn is_in_hash_key_context(
        &self,
        node: &Node,
        ancestors: &[&Node],
        max_depth: usize,
    ) -> bool {
        let mut current = node;

        // Traverse up the AST to find hash key contexts
        // Limit traversal depth to prevent excessive searching
        // Iterate ancestors in reverse (from immediate parent up)
        let len = ancestors.len();

        for i in (0..len).rev() {
            if len - i > max_depth {
                break;
            }

            let parent = ancestors[i];

            match &parent.kind {
                // Method call: Class->method (Class is bareword)
                NodeKind::Binary { op, left, right: _ } if op == "->" => {
                    // Check if current node is the class name (left side of the -> operation)
                    if std::ptr::eq(left.as_ref(), current) {
                        return true;
                    }
                }
                NodeKind::MethodCall { object, .. } => {
                    // Check if current node is the class name (object)
                    if std::ptr::eq(object.as_ref(), current) {
                        return true;
                    }
                }
                // Hash subscript: $hash{key} or %hash{key}
                NodeKind::Binary { op, left: _, right } if op == "{}" => {
                    // Check if current node is the key (right side of the {} operation)
                    if std::ptr::eq(right.as_ref(), current) {
                        return true;
                    }
                }
                // Hash/key-value slice keys: @hash{...} or %hash{...}
                NodeKind::HashSlice { keys, .. } | NodeKind::KeyValueSlice { keys, .. } => {
                    if std::ptr::eq(keys.as_ref(), current) {
                        return true;
                    }
                }
                // Arrow-deref hash subscript/slice: $ref->{key}, $obj->method()->{key},
                // $a->{b}{c}, $ref->%{key}
                // Anchor on `node`, not `current`: only direct simple keys are auto-quoted,
                // so composite or qualified keys like `$ref->{FOO + 1}` and `$ref->{FOO::BAR}`
                // must still flag their barewords.
                NodeKind::Binary { op, left: _, right } if op == "->{}" || op == "->%{}" => {
                    if std::ptr::eq(right.as_ref(), node)
                        && Self::is_simple_autoquoted_hash_key(node)
                    {
                        return true;
                    }
                }
                NodeKind::HashLiteral { pairs } => {
                    // Check if current node is a key in any of the pairs
                    for (key, _value) in pairs {
                        if std::ptr::eq(key, current) {
                            return true;
                        }
                    }
                }
                NodeKind::ArrayLiteral { .. } => {
                    // Check grandparent
                    if i > 0 {
                        let grandparent = ancestors[i - 1];
                        if let NodeKind::Binary { op, right, .. } = &grandparent.kind {
                            if op == "{}" && std::ptr::eq(right.as_ref(), parent) {
                                return true;
                            }
                        }
                        // ArrayLiteral used as keys in a slice: @hash{@keys} or %hash{@keys}
                        if matches!(&grandparent.kind,
                            NodeKind::HashSlice { keys, .. } | NodeKind::KeyValueSlice { keys, .. }
                            if std::ptr::eq(keys.as_ref(), parent))
                        {
                            return true;
                        }
                    }
                }
                // Handle IndirectCall which parser sometimes produces for $hash{key} in print statements
                NodeKind::IndirectCall { object, args, .. } => {
                    // Check if current is one of the arguments
                    for arg in args {
                        if std::ptr::eq(arg, current) {
                            // Check if object is a variable that looks like a hash
                            if let NodeKind::Variable { sigil, .. } = &object.kind {
                                if sigil == "$" {
                                    return true;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            current = parent;
        }

        false
    }

    fn is_simple_autoquoted_hash_key(node: &Node) -> bool {
        matches!(&node.kind, NodeKind::Identifier { name } if !name.contains("::"))
    }

    /// Return one human-readable fix suggestion per issue.
    pub fn get_suggestions(&self, issues: &[ScopeIssue]) -> Vec<String> {
        issues
            .iter()
            .map(|issue| match issue.kind {
                IssueKind::VariableShadowing => {
                    format!("Consider rename '{}' to avoid shadowing", issue.variable_name)
                }
                IssueKind::UnusedVariable => {
                    format!(
                        "Remove unused variable '{}' or prefix with underscore",
                        issue.variable_name
                    )
                }
                IssueKind::UndeclaredVariable => {
                    format!("Declare '{}' with 'my', 'our', or 'local'", issue.variable_name)
                }
                IssueKind::VariableRedeclaration => {
                    format!("Remove duplicate declaration of '{}'", issue.variable_name)
                }
                IssueKind::DuplicateParameter => {
                    format!("Remove or rename duplicate parameter '{}'", issue.variable_name)
                }
                IssueKind::ParameterShadowsGlobal => {
                    format!("Rename parameter '{}' to avoid shadowing", issue.variable_name)
                }
                IssueKind::UnusedParameter => {
                    format!("Rename '{}' with underscore or add comment", issue.variable_name)
                }
                IssueKind::UnquotedBareword => {
                    format!("Quote bareword '{}' or declare as filehandle", issue.variable_name)
                }
                IssueKind::UninitializedVariable => {
                    format!("Initialize '{}' before use", issue.variable_name)
                }
                IssueKind::CaptureVarWithoutRegexMatch => {
                    format!(
                        "Perform a regex match (=~ /.../) before using capture variable '{}'",
                        issue.variable_name
                    )
                }
                IssueKind::FeatureNotEnabled => {
                    // Resolve the enabling `feature` name from the keyword rather
                    // than assuming they match — they coincide for `say` but not
                    // for e.g. `given`/`when` (feature `switch`).
                    let feature =
                        feature_for_keyword(&issue.variable_name).unwrap_or(&issue.variable_name);
                    format!(
                        "Enable '{}' with `use feature '{}'` or a `use vX.Y` bundle",
                        issue.variable_name, feature
                    )
                }
                IssueKind::UnresolvedQualifiedCall => {
                    format!(
                        "Define sub '{}' or correct the call — strict mode cannot resolve it in the target package",
                        issue.variable_name
                    )
                }
            })
            .collect()
    }
}

fn collect_imported_barewords(ast: &Node) -> HashSet<String> {
    fn push_symbol(imported: &mut HashSet<String>, module: &str, token: &str) {
        let symbol = token.trim().trim_matches('\'').trim_matches('"').trim();
        if symbol.is_empty() || symbol == "," {
            return;
        }

        if symbol.starts_with(':') {
            if let Some(expanded) = resolve_known_export_tag(module, symbol) {
                imported.extend(expanded.iter().map(|name| (*name).to_string()));
            }
            return;
        }

        let is_bareword = symbol.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && symbol
                .as_bytes()
                .first()
                .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_');
        if is_bareword {
            imported.insert(symbol.to_string());
        }
    }

    fn require_module_name(node: &Node) -> Option<String> {
        let (NodeKind::FunctionCall { name, args } | NodeKind::AmperCall { name, args }) =
            &node.kind
        else {
            return None;
        };
        if name != "require" {
            return None;
        }
        let first = args.first()?;
        match &first.kind {
            NodeKind::Identifier { name } => Some(name.clone()),
            NodeKind::String { value, .. } => {
                let cleaned = value.trim_matches('\'').trim_matches('"').trim();
                if cleaned.is_empty() {
                    return None;
                }
                Some(cleaned.trim_end_matches(".pm").replace('/', "::"))
            }
            _ => None,
        }
    }

    fn require_variable_name(node: &Node) -> Option<String> {
        let (NodeKind::FunctionCall { name, args } | NodeKind::AmperCall { name, args }) =
            &node.kind
        else {
            return None;
        };
        if name != "require" {
            return None;
        }
        let first = args.first()?;
        let NodeKind::Variable { sigil, name } = &first.kind else {
            return None;
        };
        (sigil == "$" && !name.contains("::")).then(|| name.clone())
    }

    fn maybe_record_manual_imports(
        node: &Node,
        required_modules: &HashSet<String>,
        imported: &mut HashSet<String>,
    ) {
        let NodeKind::MethodCall { object, method, args } = &node.kind else {
            return;
        };
        if method != "import" {
            return;
        }
        let NodeKind::Identifier { name: module } = &object.kind else {
            return;
        };
        if !required_modules.contains(module) {
            return;
        }
        for arg in args {
            match &arg.kind {
                NodeKind::String { value, .. } => push_symbol(imported, module, value),
                NodeKind::Identifier { name } => {
                    if name.starts_with("qw") {
                        let content = name
                            .trim_start_matches("qw")
                            .trim_start_matches(|c: char| "([{/<|!".contains(c))
                            .trim_end_matches(|c: char| ")]}/|!>".contains(c));
                        for token in content.split_whitespace() {
                            push_symbol(imported, module, token);
                        }
                    } else {
                        push_symbol(imported, module, name);
                    }
                }
                NodeKind::ArrayLiteral { elements } => {
                    for el in elements {
                        if let NodeKind::String { value, .. } = &el.kind {
                            push_symbol(imported, module, value);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn maybe_record_dynamic_manual_imports(
        node: &Node,
        dynamic_require_vars: &HashSet<String>,
        imported: &mut HashSet<String>,
    ) {
        let NodeKind::MethodCall { object, method, args } = &node.kind else {
            return;
        };
        if method != "import" {
            return;
        }
        let NodeKind::Variable { sigil, name } = &object.kind else {
            return;
        };
        if sigil != "$" || !dynamic_require_vars.contains(name) {
            return;
        }

        for arg in args {
            match &arg.kind {
                NodeKind::String { value, .. } => push_symbol(imported, "", value),
                NodeKind::Identifier { name } => {
                    if name.starts_with("qw") {
                        let content = name
                            .trim_start_matches("qw")
                            .trim_start_matches(|c: char| "([{/<|!".contains(c))
                            .trim_end_matches(|c: char| ")]}/|!>".contains(c));
                        for token in content.split_whitespace() {
                            push_symbol(imported, "", token);
                        }
                    } else {
                        push_symbol(imported, "", name);
                    }
                }
                NodeKind::ArrayLiteral { elements } => {
                    for el in elements {
                        if let NodeKind::String { value, .. } = &el.kind {
                            push_symbol(imported, "", value);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Unwrap an `ExpressionStatement` node to its inner expression, or return
    /// the node itself if it is not an expression statement.
    fn inner_node(stmt: &Node) -> &Node {
        if let NodeKind::ExpressionStatement { expression } = &stmt.kind {
            expression.as_ref()
        } else {
            stmt
        }
    }

    // `in_eval` — when true we are inside a runtime `eval { }` block and
    // `require` statements are no longer static; skip the require+import
    // suppression analysis for the current block.
    fn visit(node: &Node, imported: &mut HashSet<String>, in_eval: bool) {
        if let NodeKind::Use { module, args, .. } = &node.kind {
            for arg in args {
                if arg.starts_with("qw") {
                    let content = arg
                        .trim_start_matches("qw")
                        .trim_start_matches(|c: char| "([{/<|!".contains(c))
                        .trim_end_matches(|c: char| ")]}/|!>".contains(c));
                    for token in content.split_whitespace() {
                        push_symbol(imported, module, token);
                    }
                } else {
                    push_symbol(imported, module, arg);
                }
            }
        } else if !in_eval {
            if let NodeKind::Program { statements } | NodeKind::Block { statements } = &node.kind {
                let required_modules: HashSet<String> = statements
                    .iter()
                    .filter_map(|stmt| require_module_name(inner_node(stmt)))
                    .collect();
                let dynamic_require_vars: HashSet<String> = statements
                    .iter()
                    .filter_map(|stmt| require_variable_name(inner_node(stmt)))
                    .collect();
                if !required_modules.is_empty() || !dynamic_require_vars.is_empty() {
                    for stmt in statements {
                        let inner = inner_node(stmt);
                        maybe_record_manual_imports(inner, &required_modules, imported);
                        maybe_record_dynamic_manual_imports(inner, &dynamic_require_vars, imported);
                    }
                }
            }
        }

        // Propagate eval context: children of an Eval block are runtime.
        let child_in_eval = in_eval || matches!(&node.kind, NodeKind::Eval { .. });
        for child in node.children() {
            visit(child, imported, child_in_eval);
        }
    }

    let mut imported = HashSet::new();
    visit(ast, &mut imported, false);
    imported
}

/// Collect the names of all subroutines defined anywhere in the file.
///
/// Used to suppress the feature-gate diagnostic when a user has defined their
/// own sub with the same name as a feature-gated keyword (e.g. `sub say { ... }`),
/// in which case `say(...)` is a call to the user sub, not the builtin.
fn collect_defined_subs(ast: &Node) -> HashSet<String> {
    // Store each sub as a **package-qualified** name (`main::foo`, `Foo::bar`) so
    // an unqualified call in package P is only suppressed by P's own `sub`, not by
    // a same-named sub in a different package of the same file (review #4892): e.g.
    // `package Other; sub say {} package main; say "x";` must still flag `main::say`.
    fn qualify(current_pkg: &str, name: &str) -> String {
        if name.contains("::") {
            // Already explicitly qualified (`sub Foo::bar {}`) — package-independent.
            name.to_string()
        } else {
            format!("{current_pkg}::{name}")
        }
    }
    fn inner(stmt: &Node) -> &Node {
        if let NodeKind::ExpressionStatement { expression } = &stmt.kind {
            expression.as_ref()
        } else {
            stmt
        }
    }
    fn visit(node: &Node, current_pkg: &str, subs: &mut HashSet<String>) {
        match &node.kind {
            // A statement list threads the active package across siblings: a
            // statement-form `package Foo;` rebinds it for everything that follows,
            // matching Perl's file-scoped package semantics.
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                let mut pkg = current_pkg.to_string();
                for stmt in statements {
                    if let NodeKind::Package { name, block: None, .. } = &inner(stmt).kind {
                        pkg = name.clone();
                    }
                    visit(stmt, &pkg, subs);
                }
            }
            // Block-form `package Foo { ... }` scopes the package to its block only.
            NodeKind::Package { name, block: Some(block), .. } => {
                visit(block, name, subs);
            }
            NodeKind::Subroutine { name: Some(name), .. } => {
                subs.insert(qualify(current_pkg, name));
                for child in node.children() {
                    visit(child, current_pkg, subs);
                }
            }
            _ => {
                for child in node.children() {
                    visit(child, current_pkg, subs);
                }
            }
        }
    }
    let mut subs = HashSet::new();
    // A file with no `package` statement is in `main`.
    visit(ast, "main", &mut subs);
    subs
}

/// Collect every package name declared in the file via `package Foo;` or
/// `package Foo { ... }` (excluding the implicit `main`).  Used by the
/// strict-subs qualified-call check (#3014) to distinguish in-file packages
/// (whose sub visibility we can prove) from external modules (which we
/// cannot, and therefore never flag).
fn collect_defined_packages(ast: &Node) -> HashSet<String> {
    fn inner(stmt: &Node) -> &Node {
        if let NodeKind::ExpressionStatement { expression } = &stmt.kind {
            expression.as_ref()
        } else {
            stmt
        }
    }
    let mut packages = HashSet::new();
    fn visit(node: &Node, packages: &mut HashSet<String>) {
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for stmt in statements {
                    if let NodeKind::Package { name, block: None, .. } = &inner(stmt).kind {
                        if name != "main" {
                            packages.insert(name.clone());
                        }
                    }
                    visit(stmt, packages);
                }
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                if name != "main" {
                    packages.insert(name.clone());
                }
                visit(block, packages);
            }
            _ => {
                for child in node.children() {
                    visit(child, packages);
                }
            }
        }
    }
    visit(ast, &mut packages);
    packages
}

/// Map a feature-gated keyword to the `feature` pragma name that enables it.
///
/// Currently only `say` (issue #2584 criterion 2). `state` is a distinct
/// declaration-node path and is intentionally not gated here — tracked as a
/// follow-up. Version bundles (`use v5.10`/`use v5.36`) enable the underlying
/// feature and are resolved by `PragmaState::has_feature`, so they need no
/// entry here.
pub fn feature_for_keyword(name: &str) -> Option<&'static str> {
    match name {
        "say" => Some("say"),
        _ => None,
    }
}

/// Whether the file declares a top-level Perl version pragma (`use vX.Y` /
/// `use N.NNN`).
///
/// When one is present, the `version_compat` lint (`PL900`) owns feature-gate
/// diagnostics for that file with a version-specific message (e.g. "`say`
/// requires Perl v5.10+; declared version is v5.8"), so the scope analyzer's
/// feature gate must stand down to avoid emitting a second, redundant warning
/// on the same construct (#2584 review). The bare-`say`-with-no-version case —
/// which `version_compat` deliberately skips as ambiguous — remains the scope
/// analyzer's to flag. Mirrors `version_compat`'s top-level `Program`-statement
/// scan so the two lints partition the space exactly.
fn has_declared_perl_version(ast: &Node) -> bool {
    let NodeKind::Program { statements } = &ast.kind else {
        return false;
    };
    statements.iter().any(|stmt| {
        matches!(&stmt.kind, NodeKind::Use { module, .. }
            if crate::pragma_tracker::parse_perl_version(module).is_some())
    })
}

/// Returns true if `name` (without sigil) is a numbered capture variable.
///
/// Capture variables are `$1`, `$2`, ..., `$9`, `$10`, `$11`, etc.
/// `$0` is the program name and is NOT a capture variable.
#[inline]
pub(super) fn is_capture_variable(name: &str) -> bool {
    // Must be non-empty, all digits, and not "0" (which is $0 = program name)
    !name.is_empty() && name != "0" && name.as_bytes().iter().all(|c| c.is_ascii_digit())
}

/// Check if a variable is a built-in Perl global variable
pub(super) fn is_builtin_global(sigil: &str, name: &str) -> bool {
    // Fast path: most user variables start with lowercase and are not built-ins
    // Exception: $a and $b are built-in sort variables
    if !name.is_empty() {
        let first = name.as_bytes()[0];
        if first.is_ascii_lowercase() {
            // Optimization: Combine length and byte check to avoid multiple comparisons
            if name.len() > 1 || (first != b'a' && first != b'b') {
                return false;
            }
        }
    }

    let sigil_byte = match sigil.as_bytes().first() {
        Some(b) => *b,
        None => {
            return match name {
                // Filehandles (no sigil)
                "STDIN" | "STDOUT" | "STDERR" | "DATA" | "ARGVOUT" => true,
                _ => false,
            };
        }
    };

    match sigil_byte {
        b'$' => match name {
            // Special variables
            "_" | "!" | "@" | "?" | "^" | "$" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"
            | "9" | "." | "," | "/" | "\\" | "\"" | ";" | "%" | "=" | "-" | "~" | "|" | "&"
            | "`" | "'" | "+" | "[" | "]" | ":" | "^A" | "^C" | "^D" | "^E" | "^F" | "^H" | "^I" | "^L"
            | "^M" | "^N" | "^O" | "^P" | "^R" | "^S" | "^T" | "^V" | "^W" | "^X" |
            // Common globals
            "ARGV" | "VERSION" | "AUTOLOAD" |
            // Sort variables
            "a" | "b" |
            // Error variables
            "EVAL_ERROR" | "ERRNO" | "EXTENDED_OS_ERROR" | "CHILD_ERROR" |
            "PROCESS_ID" | "PROGRAM_NAME" |
            // Perl version variables
            "PERL_VERSION" | "OLD_PERL_VERSION" |
            // Perl internal special values (perlguts/perlapi) — used in XS and introspection code
            "PL_sv_yes" | "PL_sv_no" | "PL_sv_undef" => true,
            _ => {
                // Check patterns
                // $^X (single-char) control variables — lexer produces name `^X`.
                // ${^NAME} (multi-char) control variables — lexer produces name `{^NAME}`.
                // Both should be treated as built-ins.
                //
                // Form 1: `^` followed by one or more ASCII uppercase letters or underscores.
                //   Examples: `^A`, `^W`, `^MATCH`, `^PREMATCH`, `^POSTMATCH`.
                // Form 2: `{^NAME}` — same but wrapped in braces by the lexer.
                //   Examples: `{^MATCH}`, `{^PREMATCH}`, `{^POSTMATCH}`.
                let caret_name = if let Some(inner) = name
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                {
                    inner
                } else {
                    name
                };
                if let Some(rest) = caret_name.strip_prefix('^') {
                    if !rest.is_empty()
                        && rest
                            .as_bytes()
                            .iter()
                            .all(|c| c.is_ascii_uppercase() || *c == b'_')
                    {
                        return true;
                    }
                }

                // Numbered capture variables ($1, $2, etc.)
                // Note: $0-$9 are already handled in the match above, but this covers $10+
                // Optimization: use byte check to avoid utf-8 decoding
                if !name.is_empty() && name.as_bytes().iter().all(|c| c.is_ascii_digit()) {
                    return true;
                }

                false
            }
        },
        b'@' => matches!(name, "_" | "+" | "-" | "INC" | "ARGV" | "EXPORT" | "EXPORT_OK" | "ISA"),
        b'%' => matches!(name, "_" | "+" | "-" | "!" | "ENV" | "INC" | "SIG" | "EXPORT_TAGS"),
        _ => false,
    }
}

/// Check if an identifier is a known Perl built-in function
pub(super) fn is_known_function(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if matches!(name, "PL_sv_yes" | "PL_sv_no" | "PL_sv_undef") {
        return true;
    }
    // Optimization: All known functions are lowercase or start with non-uppercase chars
    if name.as_bytes()[0].is_ascii_uppercase() {
        return false;
    }

    match name {
        // I/O functions
        "print" | "printf" | "say" | "open" | "close" | "read" | "write" | "seek" | "tell"
        | "eof" | "fileno" | "binmode" | "sysopen" | "sysread" | "syswrite" | "sysclose"
        | "select" |
        // String functions
        "chomp" | "chop" | "chr" | "crypt" | "fc" | "hex" | "index" | "lc" | "lcfirst" | "length"
        | "oct" | "ord" | "pack" | "q" | "qq" | "qr" | "quotemeta" | "qw" | "qx" | "reverse"
        | "rindex" | "sprintf" | "substr" | "tr" | "uc" | "ucfirst" | "unpack" |
        // Array/List functions
        "pop" | "push" | "shift" | "unshift" | "splice" | "split" | "join" | "grep" | "map"
        | "sort" |
        // Hash functions
        "delete" | "each" | "exists" | "keys" | "values" |
        // Control flow
        "die" | "exit" | "return" | "goto" | "last" | "next" | "redo" | "continue" | "break"
        | "given" | "when" | "default" |
        // File test operators
        "stat" | "lstat" | "-r" | "-w" | "-x" | "-o" | "-R" | "-W" | "-X" | "-O" | "-e" | "-z"
        | "-s" | "-f" | "-d" | "-l" | "-p" | "-S" | "-b" | "-c" | "-t" | "-u" | "-g" | "-k"
        | "-T" | "-B" | "-M" | "-A" | "-C" |
        // System functions
        "system" | "exec" | "fork" | "wait" | "waitpid" | "kill" | "sleep" | "alarm"
        | "getpgrp" | "getppid" | "getpriority" | "setpgrp" | "setpriority" | "time" | "times"
        | "localtime" | "gmtime" |
        // Math functions
        "abs" | "atan2" | "cos" | "exp" | "int" | "log" | "rand" | "sin" | "sqrt" | "srand" |
        // Misc functions
        "defined" | "undef" | "ref" | "bless" | "tie" | "tied" | "untie" | "eval" | "caller"
        | "import" | "require" | "use" | "do" | "package" | "sub" | "my" | "our" | "local"
        | "state" | "scalar" | "wantarray" | "warn" => true,
        _ => false,
    }
}

/// Builtins whose declaration-capable arguments are all consumed by the builtin itself.
///
/// Keep this list explicit and conservative. Only include builtins where the parser already
/// emits declaration nodes for the relevant argument, and where treating that declaration as
/// used avoids false diagnostics after the call.
///
/// Position semantics:
/// - Position 0: `open`, `opendir`, `sysopen`, `socket`, `accept`, `dbmopen`
/// - Position 1: `read`, `sysread`, `recv`, `shmread`
/// - Positions 0 and 1: `pipe`, `socketpair`
pub(super) fn builtin_declaration_arg_positions(name: &str) -> &'static [usize] {
    match name {
        // Position 0: the first argument is the new handle/socket
        "open" | "opendir" | "sysopen" | "socket" | "accept" | "dbmopen" => &[0],
        // Position 1: the second argument is the buffer (first is an existing handle)
        "read" | "sysread" | "recv" | "shmread" => &[1],
        // pipe: both first arguments are new handles
        "pipe" => &[0, 1],
        // socketpair: both first arguments are new sockets
        "socketpair" => &[0, 1],
        _ => &[],
    }
}

/// Whether the specific `uninitialized` warning category is disabled in `state`.
///
/// Returns `true` when the effective pragma state at a use site lists the
/// `uninitialized` category in its `disabled_warning_categories` (populated by
/// `no warnings 'uninitialized'`). Consumers use this to suppress the
/// [`IssueKind::UninitializedVariable`] diagnostic within the pragma's lexical
/// scope; a later `use warnings 'uninitialized'` removes the entry and re-enables
/// the diagnostic in source order.
///
/// Deliberately scoped to the *specific* category rather than the blanket `all`
/// marker. Two considerations drive this:
/// - Gating on explicit category membership — not the global `warnings` flag —
///   keeps the analyzer's default behaviour (emit for files with no
///   `use warnings`) unchanged; only an explicit per-category opt-out silences it.
/// - Treating a bare `all` entry as suppression would misfire on re-enable
///   transitions the flattened `disabled_warning_categories` list cannot express:
///   after `no warnings 'all'; use warnings 'uninitialized'` the list still holds
///   `all` even though `uninitialized` is active, so matching `all` would wrongly
///   suppress a diagnostic Perl emits. The trade-off is that a blanket
///   `no warnings 'all'` does not suppress this static lint — a conservative
///   (never a false-negative) choice; use `no warnings 'uninitialized'` to
///   suppress it explicitly.
fn uninitialized_warning_suppressed(state: &PragmaState) -> bool {
    state.disabled_warning_categories.iter().any(|category| category == "uninitialized")
}

/// Builtins that operate on `$_` by default when called with zero arguments.
///
/// When any of these is invoked as a bare call (no args), Perl implicitly reads
/// (and in some cases modifies) `$_`. Marking `$_` as used at call sites prevents
/// false "unused" or "uninitialized" diagnostics for lexically-scoped `my $_`.
pub(super) fn is_topic_defaulting_builtin(name: &str) -> bool {
    matches!(
        name,
        "chomp"
            | "chop"
            | "chr"
            | "hex"
            | "lc"
            | "lcfirst"
            | "length"
            | "oct"
            | "ord"
            | "uc"
            | "ucfirst"
            | "abs"
            | "int"
            | "log"
            | "sqrt"
            | "cos"
            | "sin"
            | "exp"
            | "print"
            | "say"
    )
}

/// Topic-defaulting builtins that also modify `$_` when called without args.
pub(super) fn is_topic_modifying_builtin(name: &str) -> bool {
    matches!(name, "chomp" | "chop")
}

fn is_explicit_scalar_reference_deref(source: &str) -> bool {
    source.starts_with("@$")
        || source.starts_with("%$")
        || source.starts_with("$$")
        || source.starts_with("@{$")
        || source.starts_with("%{$")
        || source.starts_with("${$")
}

fn normalize_scalar_deref_base_name(name: &str) -> &str {
    let unwrapped =
        name.strip_prefix('{').and_then(|inner| inner.strip_suffix('}')).unwrap_or(name);

    unwrapped.strip_prefix('$').unwrap_or(unwrapped)
}

/// Check if an identifier is a known filehandle
#[allow(dead_code)]
fn is_filehandle(name: &str) -> bool {
    match name {
        "STDIN" | "STDOUT" | "STDERR" | "ARGV" | "ARGVOUT" | "DATA" | "STDHANDLE"
        | "__PACKAGE__" | "__FILE__" | "__LINE__" | "__SUB__" | "__END__" | "__DATA__" => true,
        _ => {
            // Check if it's all uppercase (common convention for filehandles)
            name.chars().all(|c| c.is_ascii_uppercase() || c == '_') && !name.is_empty()
        }
    }
}

// ============================================================================
// Inline lib tests — required for Codecov Patch 95 (`--lib` coverage gate).
// These exercise the package-change-generation paths added for #1661.
// ============================================================================
#[cfg(test)]
mod tests_our_redecl {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::{IssueKind, ScopeAnalyzer, ScopeIssue};
    use crate::Parser;
    use crate::pragma_tracker::PragmaTracker;

    fn analyze(code: &str) -> Vec<ScopeIssue> {
        let mut parser = Parser::new(code);
        let ast = parser.parse().unwrap();
        let pragma_map = PragmaTracker::build(&ast);
        ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
    }

    fn redecls_for_var<'a>(issues: &'a [ScopeIssue], name: &str) -> Vec<&'a ScopeIssue> {
        issues
            .iter()
            .filter(|i| i.kind == IssueKind::VariableRedeclaration && i.variable_name == name)
            .collect()
    }

    /// Same-package `our` redeclaration must emit `VariableRedeclaration` (#1661).
    #[test]
    fn our_same_package_redecl_is_error() {
        let issues = analyze("use strict;\npackage Foo;\nour $x = 1;\nour $x = 2;\nprint $x;\n");
        assert!(
            !redecls_for_var(&issues, "$x").is_empty(),
            "input that hits the boundary: is_our; got: {:?}",
            issues
        );
    }

    /// Lexical `my` redeclaration still uses the non-`our` path (#1661 control).
    #[test]
    fn my_same_scope_redecl_exercises_non_our_branch() {
        let issues = analyze("use strict;\nmy $x = 1;\nmy $x = 2;\nprint $x;\n");
        assert!(
            !redecls_for_var(&issues, "$x").is_empty(),
            "input that hits the boundary: !is_our; got: {:?}",
            issues
        );
    }

    /// Redeclaration assertions must stay bound to the exact variable name.
    #[test]
    fn redecls_for_var_filters_exact_variable_name() {
        let issues = analyze(
            "use strict;\npackage Foo;\nour $x = 1;\nour $x = 2;\nour $y = 3;\nprint $x + $y;\n",
        );

        let x_redecls = redecls_for_var(&issues, "$x");
        assert_eq!(
            x_redecls.len(),
            1,
            "expected exactly one VariableRedeclaration for $x; got: {:?}",
            x_redecls
        );
        assert!(
            x_redecls.iter().all(|issue| issue.kind == IssueKind::VariableRedeclaration
                && issue.variable_name == "$x"),
            "expected only $x VariableRedeclaration issues; got: {:?}",
            x_redecls
        );

        assert!(
            redecls_for_var(&issues, "$y").is_empty(),
            "non-redeclared $y must not match $x redeclaration issues; got: {:?}",
            issues
        );
    }

    /// Cross-package `our` re-import after a package switch must NOT emit an error (#1661).
    #[test]
    fn our_package_switch_reimport_allowed() {
        let issues = analyze(
            "use strict;\npackage Foo;\nour $x = 1;\npackage Bar;\nour $x = 2;\npackage Foo;\nour $x = 3;\nprint $x;\n",
        );
        assert!(
            redecls_for_var(&issues, "$x").is_empty(),
            "expected no VariableRedeclaration across package switches; got: {:?}",
            redecls_for_var(&issues, "$x")
        );
    }

    /// Uninitialized same-package `our` redeclaration is also an error (#1661).
    #[test]
    fn our_uninit_same_package_redecl_is_error() {
        let issues = analyze("use strict;\npackage Foo;\nour $x;\nour $x;\nprint $x;\n");
        assert!(
            !redecls_for_var(&issues, "$x").is_empty(),
            "expected VariableRedeclaration for uninitialized same-package our $x"
        );
    }

    /// Different-package `our` declarations must not error (#1661 positive control).
    #[test]
    fn our_different_packages_no_error() {
        let issues = analyze(
            "use strict;\npackage Foo;\nour $x = 1;\npackage Bar;\nour $x = 2;\nprint $x;\n",
        );
        assert!(
            redecls_for_var(&issues, "$x").is_empty(),
            "expected no VariableRedeclaration across packages; got: {:?}",
            redecls_for_var(&issues, "$x")
        );
    }
}

// ============================================================================
// #2584 — `no warnings 'uninitialized'` gates the UninitializedVariable
// diagnostic. The pragma model records disabled warning categories in
// source-ordered lexical ranges; the scope analyzer must consume that fact so an
// explicit `no warnings 'uninitialized'` suppresses the diagnostic within its
// lexical scope, and a later `use warnings 'uninitialized'` re-enables it in
// source order. Suppression is scoped to the specific `uninitialized` category
// only, never the blanket `all` marker — see `uninitialized_warning_suppressed`
// for why (matching `all` would produce false-negatives on re-enable sequences).
// ============================================================================
// ============================================================================
// Feature-gated keyword diagnostics (issue #2584, criterion 2).
//
// A feature-gated bareword such as `say` is only valid when the enabling
// `feature` is active at that offset — `use feature 'say'`, or a version bundle
// like `use v5.10`/`use v5.36` that includes it. The pragma model already
// resolves bundle→feature membership via `PragmaState::has_feature`, so the
// scope analyzer just gates the `say` FunctionCall on it. Method calls
// (`$o->say`) and autoquoted hash keys (`say => 1`) are structurally different
// nodes and are never gated; an explicit import or a user-defined `sub say`
// suppresses the gate. `state` (a declaration node) is deliberately out of
// scope for this slice.
// ============================================================================
#[cfg(test)]
mod tests_feature_keyword_gate {
    use super::{IssueKind, ScopeAnalyzer, ScopeIssue};
    use crate::Parser;
    use crate::pragma_tracker::PragmaTracker;
    use perl_tdd_support::must;

    fn analyze(code: &str) -> Vec<ScopeIssue> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let pragma_map = PragmaTracker::build(&ast);
        ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
    }

    fn feature_gate_issues<'a>(issues: &'a [ScopeIssue], name: &str) -> Vec<&'a ScopeIssue> {
        issues
            .iter()
            .filter(|i| i.kind == IssueKind::FeatureNotEnabled && i.variable_name == name)
            .collect()
    }

    /// Control + acceptance: `say` with no enabling pragma is flagged.
    #[test]
    fn say_without_feature_is_flagged() {
        let issues = analyze("say \"hello\";\n");
        assert!(
            !feature_gate_issues(&issues, "say").is_empty(),
            "say without `use feature 'say'` must be flagged; got: {issues:?}"
        );
    }

    /// Acceptance (2): `use feature 'say'` enables `say`.
    #[test]
    fn say_with_use_feature_say_is_clean() {
        let issues = analyze("use feature 'say';\nsay \"hello\";\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "`use feature 'say'` must enable say; got: {issues:?}"
        );
    }

    /// Version bundles enable the feature: `use v5.36` includes `say`.
    #[test]
    fn say_with_use_v5_36_bundle_is_clean() {
        let issues = analyze("use v5.36;\nsay \"hello\";\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "`use v5.36` bundle must enable say; got: {issues:?}"
        );
    }

    /// `use v5.10` is the classic say-enabling bundle.
    #[test]
    fn say_with_use_v5_10_bundle_is_clean() {
        let issues = analyze("use v5.10;\nsay \"hello\";\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "`use v5.10` bundle must enable say; got: {issues:?}"
        );
    }

    /// False-positive guard: a method call `$o->say(...)` is not the builtin.
    #[test]
    fn method_call_say_not_flagged() {
        let issues = analyze("my $o = shift;\n$o->say(\"hi\");\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "method-call say must not be flagged; got: {issues:?}"
        );
    }

    /// False-positive guard: `say => 1` autoquotes to a hash key, not a call.
    #[test]
    fn say_hash_key_not_flagged() {
        let issues = analyze("my %h = (say => 1);\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "autoquoted `say =>` key must not be flagged; got: {issues:?}"
        );
    }

    /// False-positive guard: a user-defined `sub say` shadows the builtin.
    #[test]
    fn user_defined_sub_say_not_flagged() {
        let issues = analyze("sub say { return 1; }\nsay();\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "call to user-defined `sub say` must not be flagged; got: {issues:?}"
        );
    }

    /// De-duplication guard (#2584 review): when a version pragma is declared but
    /// too low to enable `say` (`use v5.8;`), the scope-analyzer gate stands down
    /// so the `version_compat` lint (`PL900`) owns the diagnostic — otherwise the
    /// same `say` would carry two warnings. `version_compat` lives in
    /// `perl-lsp-rs-core`, so here we only assert our gate emits nothing.
    #[test]
    fn say_with_low_version_declared_defers_to_version_compat() {
        let issues = analyze("use v5.8;\nsay \"hi\";\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "with a version declared, the scope-analyzer say gate must defer to version_compat; got: {issues:?}"
        );
    }

    /// The bare case — no version *and* no feature — is the gap `version_compat`
    /// skips (undeclared version is ambiguous), so it stays the scope analyzer's
    /// to flag even after the version-deferral guard above.
    #[test]
    fn say_with_no_version_still_flagged() {
        let issues = analyze("use strict;\nsay \"hi\";\n");
        assert!(
            !feature_gate_issues(&issues, "say").is_empty(),
            "bare say with no version pragma must still be flagged; got: {issues:?}"
        );
    }

    /// Package-aware shadowing (#4892 review): a `sub say` in one package must NOT
    /// suppress the feature gate for `say` in a different package of the same file —
    /// the unqualified call resolves against the active package (`main`), which has
    /// no `say`, so the diagnostic still fires.
    #[test]
    fn say_not_suppressed_by_other_package_sub() {
        let issues = analyze("package Other;\nsub say { return 1; }\npackage main;\nsay \"x\";\n");
        assert!(
            !feature_gate_issues(&issues, "say").is_empty(),
            "Other::say must not suppress the main-package say gate; got: {issues:?}"
        );
    }

    /// Control for the above: a `sub say` in the *same* package as the call still
    /// suppresses the gate.
    #[test]
    fn say_suppressed_by_same_package_sub() {
        let issues = analyze("package Foo;\nsub say { return 1; }\nsay \"x\";\n");
        assert!(
            feature_gate_issues(&issues, "say").is_empty(),
            "same-package sub say must suppress the gate; got: {issues:?}"
        );
    }
}

// ============================================================================
#[cfg(test)]
mod tests_uninitialized_warning_gate {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::{IssueKind, ScopeAnalyzer, ScopeIssue};
    use crate::Parser;
    use crate::pragma_tracker::PragmaTracker;

    fn analyze(code: &str) -> Vec<ScopeIssue> {
        let mut parser = Parser::new(code);
        let ast = parser.parse().unwrap();
        let pragma_map = PragmaTracker::build(&ast);
        ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
    }

    fn uninit_for_var<'a>(issues: &'a [ScopeIssue], name: &str) -> Vec<&'a ScopeIssue> {
        issues
            .iter()
            .filter(|i| i.kind == IssueKind::UninitializedVariable && i.variable_name == name)
            .collect()
    }

    /// Control: a declared-but-uninitialized variable that is then read produces
    /// an `UninitializedVariable` diagnostic when no suppressing pragma is present.
    /// Proves the trigger is real so the suppression tests below are meaningful.
    #[test]
    fn uninitialized_use_reported_without_pragma() {
        let issues = analyze("my $x;\nprint $x;\n");
        assert!(
            !uninit_for_var(&issues, "$x").is_empty(),
            "expected UninitializedVariable for read of uninitialized $x; got: {:?}",
            issues
        );
    }

    /// `no warnings 'uninitialized'` suppresses the diagnostic (acceptance (1)).
    #[test]
    fn no_warnings_uninitialized_suppresses_diagnostic() {
        let issues = analyze("no warnings 'uninitialized';\nmy $x;\nprint $x;\n");
        assert!(
            uninit_for_var(&issues, "$x").is_empty(),
            "no warnings 'uninitialized' must suppress the diagnostic; got: {:?}",
            issues
        );
    }

    /// Suppression is scoped to the specific `uninitialized` category, not the
    /// blanket `all` marker. The flattened `disabled_warning_categories` list
    /// cannot express `all`-minus-a-re-enabled-category transitions, so treating
    /// a bare `all` entry as suppression would produce false-negatives (see the
    /// re-enable regression tests below). The deliberate trade-off: a blanket
    /// `no warnings 'all'` does not silence this static lint — a conservative,
    /// never-false-negative choice.
    #[test]
    fn no_warnings_all_does_not_suppress_specific_lint() {
        let issues = analyze("no warnings 'all';\nmy $x;\nprint $x;\n");
        assert!(
            !uninit_for_var(&issues, "$x").is_empty(),
            "blanket no warnings 'all' is intentionally not treated as uninitialized suppression; got: {:?}",
            issues
        );
    }

    /// Suppression is category-specific: disabling an unrelated category leaves
    /// the uninitialized diagnostic active.
    #[test]
    fn no_warnings_unrelated_category_still_reports() {
        let issues = analyze("no warnings 'once';\nmy $x;\nprint $x;\n");
        assert!(
            !uninit_for_var(&issues, "$x").is_empty(),
            "unrelated no warnings 'once' must not suppress uninitialized; got: {:?}",
            issues
        );
    }

    /// Regression (#4803 review, P1): `no warnings 'all'` followed by a specific
    /// `use warnings 'uninitialized'` re-enables the category — Perl emits, so we
    /// must too. The pragma list still holds `all` here, which is exactly why the
    /// predicate matches the specific category only, never `all`.
    #[test]
    fn all_disable_then_specific_reenable_reports() {
        let issues =
            analyze("no warnings 'all';\nuse warnings 'uninitialized';\nmy $x;\nprint $x;\n");
        assert!(
            !uninit_for_var(&issues, "$x").is_empty(),
            "use warnings 'uninitialized' after no warnings 'all' must re-enable the diagnostic; got: {:?}",
            issues
        );
    }

    /// Regression (#4803 review, P1): a specific `no warnings 'uninitialized'`
    /// followed by a blanket `use warnings 'all'` re-enables everything — Perl
    /// emits, so we must too. Relies on `use warnings 'all'` clearing the disabled
    /// set in perl-pragma.
    #[test]
    fn specific_disable_then_all_reenable_reports() {
        let issues =
            analyze("no warnings 'uninitialized';\nuse warnings 'all';\nmy $x;\nprint $x;\n");
        assert!(
            !uninit_for_var(&issues, "$x").is_empty(),
            "use warnings 'all' after no warnings 'uninitialized' must re-enable the diagnostic; got: {:?}",
            issues
        );
    }

    /// A later `use warnings 'uninitialized'` re-enables the diagnostic in source
    /// order (acceptance: lexical re-enable). The suppressed read stays silent;
    /// the read after re-enabling is reported.
    #[test]
    fn use_warnings_uninitialized_reenables_in_source_order() {
        let code = concat!(
            "no warnings 'uninitialized';\n",  // line 1: disable
            "my $x;\n",                        // line 2
            "print $x;\n",                     // line 3: suppressed
            "use warnings 'uninitialized';\n", // line 4: re-enable
            "my $y;\n",                        // line 5
            "print $y;\n",                     // line 6: reported
        );
        let issues = analyze(code);
        assert!(
            uninit_for_var(&issues, "$x").is_empty(),
            "read under active no warnings 'uninitialized' must stay suppressed; got: {:?}",
            issues
        );
        assert!(
            !uninit_for_var(&issues, "$y").is_empty(),
            "read after use warnings 'uninitialized' must be reported; got: {:?}",
            issues
        );
    }
}
