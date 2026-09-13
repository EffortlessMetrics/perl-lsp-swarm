//! Package graph edge extraction from Perl inheritance and role-composition patterns.
//!
//! Walks the AST to extract [`PackageEdge`] entries that describe inheritance,
//! role composition, and dependency relationships between Perl packages.
//!
//! # Supported Patterns
//!
//! | Perl source                              | `PackageEdgeKind`     |
//! |------------------------------------------|-----------------------|
//! | `use parent 'Base'`                      | `Inherits`            |
//! | `use parent qw(Base1 Base2)`             | `Inherits`            |
//! | `use base 'Base'`                        | `Inherits`            |
//! | `use base qw(Base1 Base2)`               | `Inherits`            |
//! | `@ISA = ('Base')`                        | `Inherits`            |
//! | `our @ISA = qw(Base1 Base2)`             | `Inherits`            |
//! | `push @ISA, 'Base'`                      | `Inherits`            |
//! | `extends 'Base'` (Moo/Moose)             | `Inherits`            |
//! | `with 'Role'` (Moo/Moose)               | `ComposesRole`        |
//!
//! # Framework DSL gating
//!
//! `extends` and `with` are ordinary Perl function calls, not keywords. They
//! carry inheritance and role-composition meaning only when an object
//! framework has imported them into the calling package. A package that
//! defines `sub extends { ... }` and calls `extends 'NotAParent'` is ordinary
//! Perl, and turning that into an `Inherits` edge is a false fact.
//!
//! Edges for these two spellings are therefore emitted only when, at the call
//! site, the current package has activated a framework that exports that exact
//! keyword ([`classify_framework_module`]), and the package does not define a
//! subroutine of the same name that would shadow the import. Activation is
//! source-ordered and package-local: `use Moose` in one package or later in
//! the file does not license an earlier call elsewhere.
//!
//! Native Perl forms (`use parent`, `use base`, `@ISA`) are unaffected — they
//! are not framework DSL and need no activation.

use crate::analysis::symbol::{FrameworkKind, classify_framework_module};
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::{AnchorId, Confidence, FileId, PackageEdge, PackageEdgeKind, Provenance};
use rustc_hash::{FxHashMap, FxHashSet};

/// The framework DSL keyword a call spelling would mean, if it were the DSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DslKeyword {
    /// `extends 'Base'` — inheritance.
    Extends,
    /// `with 'Role'` — role composition.
    With,
}

/// Which framework DSL keywords a `use` of a module imports into the package.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DslImports {
    /// The package may use `extends` to declare a superclass.
    extends: bool,
    /// The package may use `with` to compose a role.
    with: bool,
}

impl DslImports {
    /// The keywords a given framework activation exports.
    ///
    /// Role variants export `with` (a role may consume other roles) but not
    /// `extends`: `Moose::Role`, `Moo::Role`, and `Role::Tiny` do not give a
    /// package a superclass declaration.
    fn for_framework(kind: FrameworkKind) -> Self {
        match kind {
            FrameworkKind::Moo | FrameworkKind::Moose => Self { extends: true, with: true },
            FrameworkKind::MooRole
            | FrameworkKind::MooseRole
            | FrameworkKind::RoleTiny
            | FrameworkKind::RoleTinyWith => Self { extends: false, with: true },
            FrameworkKind::ClassTiny => Self::default(),
        }
    }

    /// Whether this activation set grants `keyword`.
    fn grants(self, keyword: DslKeyword) -> bool {
        match keyword {
            DslKeyword::Extends => self.extends,
            DslKeyword::With => self.with,
        }
    }

    /// Accumulate another activation in the same package.
    fn merge(&mut self, other: Self) {
        self.extends |= other.extends;
        self.with |= other.with;
    }
}

/// Extractor that walks an AST to produce [`PackageEdge`] entries for each
/// inheritance, role-composition, or dependency relationship found.
pub struct PackageGraphExtractor;

impl PackageGraphExtractor {
    /// Walk the entire AST and return one [`PackageEdge`] per detected
    /// inheritance or role-composition relationship.
    ///
    /// Each edge carries an `anchor_id` derived from the statement's
    /// byte-offset span.
    ///
    /// Framework DSL edges (`extends`/`with`) require framework activation in
    /// the calling package; see the module documentation.
    pub fn extract(ast: &Node, _file_id: FileId) -> Vec<PackageEdge> {
        // Subroutines are installed at compile time, so a package-local
        // `sub extends` shadows the import regardless of where it appears
        // relative to the call. Collect those first.
        let mut shadows =
            ShadowScan { current_package: "main".to_string(), shadowed: FxHashSet::default() };
        shadows.walk(ast);

        let mut state = ExtractorState {
            current_package: "main".to_string(),
            activated: FxHashMap::default(),
            shadowed: shadows.shadowed,
            edges: Vec::new(),
        };
        state.walk(ast);
        state.edges
    }
}

/// Pre-pass that records packages defining a subroutine whose name collides
/// with a framework DSL keyword.
struct ShadowScan {
    /// Current package context.
    current_package: String,
    /// `(package, keyword)` pairs where a local subroutine shadows the DSL.
    shadowed: FxHashSet<(String, DslKeyword)>,
}

impl ShadowScan {
    fn walk(&mut self, node: &Node) {
        match &node.kind {
            NodeKind::Program { statements } => {
                for stmt in statements {
                    self.walk(stmt);
                }
                return;
            }
            NodeKind::Block { statements } => {
                let prev_package = self.current_package.clone();
                for stmt in statements {
                    self.walk(stmt);
                }
                self.current_package = prev_package;
                return;
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                let prev_package = self.current_package.clone();
                self.current_package = name.clone();
                self.walk(block);
                self.current_package = prev_package;
                return;
            }
            NodeKind::Package { name, block: None, .. } => {
                self.current_package = name.clone();
                return;
            }
            NodeKind::Subroutine { name: Some(sub_name), .. } => {
                if let Some(keyword) = dsl_keyword_for(sub_name) {
                    self.shadowed.insert((self.current_package.clone(), keyword));
                }
            }
            _ => {}
        }

        for child in node.children() {
            self.walk(child);
        }
    }
}

/// Map a call or subroutine spelling onto the DSL keyword it could mean.
fn dsl_keyword_for(name: &str) -> Option<DslKeyword> {
    match name {
        "extends" => Some(DslKeyword::Extends),
        "with" => Some(DslKeyword::With),
        _ => None,
    }
}

/// Internal state for the recursive AST walk.
struct ExtractorState {
    /// Current package context (updated when `package Foo;` is encountered).
    current_package: String,
    /// DSL keywords activated so far, per package, in source order.
    activated: FxHashMap<String, DslImports>,
    /// `(package, keyword)` pairs shadowed by a local subroutine.
    shadowed: FxHashSet<(String, DslKeyword)>,
    /// Accumulated edges.
    edges: Vec<PackageEdge>,
}

impl ExtractorState {
    /// Walk a statement list in source order using the current package context.
    fn walk_statements(&mut self, statements: &[Node]) {
        for stmt in statements {
            self.walk(stmt);
        }
    }

    /// Recursive AST walker.
    fn walk(&mut self, node: &Node) {
        match &node.kind {
            // For the top-level program, walk statements in order so that
            // `package Foo;` (semicolon form) updates the current package for
            // subsequent sibling statements through the end of the file.
            NodeKind::Program { statements } => {
                self.walk_statements(statements);
                return;
            }

            // Bare blocks introduce a lexical package scope: `package Foo;` inside
            // the block applies to later statements in that block, but the outer
            // package resumes after the block.
            NodeKind::Block { statements } => {
                let prev_package = self.current_package.clone();
                self.walk_statements(statements);
                self.current_package = prev_package;
                return;
            }

            // `package Foo { ... }` (block form) — scoped package context.
            NodeKind::Package { name, block: Some(block), .. } => {
                let prev_package = self.current_package.clone();
                self.current_package = name.clone();
                self.walk(block);
                self.current_package = prev_package;
                return;
            }

            // `package Foo;` (semicolon form) — updates current package for
            // subsequent siblings. The actual statements follow as siblings
            // in the parent Program/Block.
            NodeKind::Package { name, block: None, .. } => {
                self.current_package = name.clone();
                return;
            }

            // `use parent 'Base'` / `use parent qw(Base1 Base2)`
            // `use base 'Base'` / `use base qw(Base1 Base2)`
            NodeKind::Use { module, args, .. } if module == "parent" || module == "base" => {
                let anchor_id = Self::anchor_from_node(node);
                let names = Self::extract_parent_names_from_args(args);
                for name in names {
                    self.emit_edge(name, PackageEdgeKind::Inherits, anchor_id, Confidence::High);
                }
            }

            // `use Moose;` / `use Moo::Role;` — records which DSL keywords are
            // imported into the current package from this point in the source.
            // Activation is package-local and source-ordered: it licenses only
            // later `extends`/`with` calls in this same package.
            NodeKind::Use { module, .. } => {
                if let Some(kind) = classify_framework_module(module) {
                    self.activated
                        .entry(self.current_package.clone())
                        .or_default()
                        .merge(DslImports::for_framework(kind));
                }
            }

            // `our @ISA = qw(Base1 Base2)` (VariableDeclaration form)
            NodeKind::VariableDeclaration { variable, initializer: Some(init), .. }
                if Self::is_isa_variable(variable) =>
            {
                let anchor_id = Self::anchor_from_node(node);
                let names = Self::collect_names_from_node(init);
                for name in names {
                    self.emit_edge(name, PackageEdgeKind::Inherits, anchor_id, Confidence::High);
                }
            }

            // `@ISA = qw(Base1 Base2)` (bare Assignment form)
            NodeKind::Assignment { lhs, rhs, .. } if Self::is_isa_variable(lhs) => {
                let anchor_id = Self::anchor_from_node(node);
                let names = Self::collect_names_from_node(rhs);
                for name in names {
                    self.emit_edge(name, PackageEdgeKind::Inherits, anchor_id, Confidence::High);
                }
            }

            // `push @ISA, 'Base'` and `extends 'Base'` / `with 'Role'`
            // Both appear as ExpressionStatement(FunctionCall { ... })
            NodeKind::ExpressionStatement { expression } => {
                self.handle_expression_statement(expression, node);
            }

            _ => {}
        }

        // Recurse into children for all other node types.
        for child in node.children() {
            self.walk(child);
        }
    }

    /// Handle expression statements that may contain `push @ISA`, `extends`, or `with`.
    fn handle_expression_statement(&mut self, expression: &Node, stmt_node: &Node) {
        if let NodeKind::FunctionCall { name, args } = &expression.kind {
            match name.as_str() {
                // `push @ISA, 'Base1', 'Base2'`
                "push" => {
                    if let Some(first_arg) = args.first() {
                        if Self::is_isa_variable(first_arg) {
                            let anchor_id = Self::anchor_from_node(stmt_node);
                            for arg in args.iter().skip(1) {
                                let names = Self::collect_names_from_node(arg);
                                for name in names {
                                    self.emit_edge(
                                        name,
                                        PackageEdgeKind::Inherits,
                                        anchor_id,
                                        Confidence::High,
                                    );
                                }
                            }
                        }
                    }
                }
                // `extends 'Base'` (Moo/Moose) — framework DSL. A call that is
                // not the activated DSL falls through and emits nothing.
                "extends" if self.dsl_call_is_framework(DslKeyword::Extends) => {
                    let anchor_id = Self::anchor_from_node(stmt_node);
                    let names = Self::collect_names_from_args(args);
                    for name in names {
                        self.emit_edge(
                            name,
                            PackageEdgeKind::Inherits,
                            anchor_id,
                            Confidence::High,
                        );
                    }
                }
                // `with 'Role'` (Moo/Moose) — framework DSL, same gating.
                "with" if self.dsl_call_is_framework(DslKeyword::With) => {
                    let anchor_id = Self::anchor_from_node(stmt_node);
                    let names = Self::collect_names_from_args(args);
                    for name in names {
                        self.emit_edge(
                            name,
                            PackageEdgeKind::ComposesRole,
                            anchor_id,
                            Confidence::High,
                        );
                    }
                }
                _ => {}
            }
        }

        // Also handle the two-statement form where `extends`/`with` is parsed
        // as a bare Identifier followed by a String in the next statement.
        // This is handled by the parent walk since we process siblings.
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Whether a call spelled as a framework DSL keyword actually resolves to
    /// that keyword in the current package.
    ///
    /// Requires that the package activated a framework exporting `keyword`
    /// at or before this point in the source, and that the package does not
    /// define a subroutine of the same name that would shadow the import.
    fn dsl_call_is_framework(&self, keyword: DslKeyword) -> bool {
        if self.shadowed.contains(&(self.current_package.clone(), keyword)) {
            return false;
        }
        self.activated.get(&self.current_package).is_some_and(|imports| imports.grants(keyword))
    }

    /// Emit a [`PackageEdge`] from the current package to the given target.
    fn emit_edge(
        &mut self,
        to_package: String,
        kind: PackageEdgeKind,
        anchor_id: AnchorId,
        confidence: Confidence,
    ) {
        self.edges.push(PackageEdge::new(
            self.current_package.clone(),
            to_package,
            kind,
            Some(anchor_id),
            Provenance::ExactAst,
            confidence,
        ));
    }

    /// Derive an [`AnchorId`] from a node's byte-offset span.
    fn anchor_from_node(node: &Node) -> AnchorId {
        AnchorId(node.location.start as u64)
    }

    /// Check whether a node is the `@ISA` variable.
    fn is_isa_variable(node: &Node) -> bool {
        matches!(&node.kind, NodeKind::Variable { sigil, name } if sigil == "@" && name == "ISA")
    }

    /// Extract parent class names from `use parent`/`use base` args.
    ///
    /// The parser stores args as strings. Handles:
    /// - Quoted strings: `"'Parent'"` → `"Parent"`
    /// - qw-lists: `"qw(Base1 Base2)"` → `["Base1", "Base2"]`
    /// - Flags like `-norequire` are skipped.
    fn extract_parent_names_from_args(args: &[String]) -> Vec<String> {
        let mut names = Vec::new();
        for arg in args {
            let trimmed = arg.trim();
            // Skip flags like -norequire
            if trimmed.starts_with('-') || trimmed.is_empty() {
                continue;
            }
            names.extend(Self::expand_arg_to_names(trimmed));
        }
        names
    }

    /// Expand a single arg string into individual class/role names.
    ///
    /// Handles qw(...) lists and quoted strings.
    fn expand_arg_to_names(arg: &str) -> Vec<String> {
        let arg = arg.trim();
        // qw(...) form
        if arg.starts_with("qw(") {
            if let Some(content) = arg.strip_prefix("qw(").and_then(|s| s.strip_suffix(')')) {
                return content
                    .split_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
            }
        }
        // Other qw variants: qw{...}, qw[...], qw/.../ etc.
        if arg.starts_with("qw") && arg.len() > 3 {
            let bytes = arg.as_bytes();
            let open = bytes[2] as char;
            let close = match open {
                '(' => ')',
                '{' => '}',
                '[' => ']',
                '<' => '>',
                c => c,
            };
            if let Some(end) = arg.rfind(close) {
                if end > 3 {
                    let content = &arg[3..end];
                    return content
                        .split_whitespace()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                }
            }
        }
        // Quoted string: strip quotes
        let unquoted = arg.trim_matches('\'').trim_matches('"').trim();
        if unquoted.is_empty() {
            return Vec::new();
        }
        vec![unquoted.to_string()]
    }

    /// Collect package/class/role names from an AST node (RHS of @ISA assignment
    /// or argument to push/extends/with).
    fn collect_names_from_node(node: &Node) -> Vec<String> {
        match &node.kind {
            NodeKind::String { value, .. } => {
                let trimmed = value.trim_matches('\'').trim_matches('"').trim();
                if trimmed.is_empty() { Vec::new() } else { vec![trimmed.to_string()] }
            }
            NodeKind::Identifier { name } => {
                // Handle qw(...) stored as identifier
                if name.starts_with("qw") {
                    Self::expand_arg_to_names(name)
                } else if name.is_empty() {
                    Vec::new()
                } else {
                    vec![name.clone()]
                }
            }
            NodeKind::ArrayLiteral { elements } => {
                elements.iter().flat_map(Self::collect_names_from_node).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Collect names from function call arguments (Vec<Node>).
    fn collect_names_from_args(args: &[Node]) -> Vec<String> {
        args.iter().flat_map(Self::collect_names_from_node).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    /// Parse Perl source and extract package graph edges.
    fn parse_and_extract(code: &str) -> Vec<PackageEdge> {
        let mut parser = Parser::new(code);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(_) => return Vec::new(),
        };
        PackageGraphExtractor::extract(&ast, FileId(1))
    }

    // ── use parent 'Base' → Inherits ────────────────────────────────────

    #[test]
    fn test_use_parent_single() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nuse parent 'Base';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Base");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        assert_eq!(edge.provenance, Provenance::ExactAst);
        assert_eq!(edge.confidence, Confidence::High);
        assert!(edge.anchor_id.is_some());
        Ok(())
    }

    #[test]
    fn test_use_parent_qw_multiple() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nuse parent qw(Base1 Base2);\n1;");
        assert_eq!(edges.len(), 2, "expected two edges, got {}", edges.len());

        assert_eq!(edges[0].from_package, "Child");
        assert_eq!(edges[0].to_package, "Base1");
        assert_eq!(edges[0].kind, PackageEdgeKind::Inherits);

        assert_eq!(edges[1].from_package, "Child");
        assert_eq!(edges[1].to_package, "Base2");
        assert_eq!(edges[1].kind, PackageEdgeKind::Inherits);
        Ok(())
    }

    #[test]
    fn test_use_parent_with_norequire() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nuse parent -norequire, 'Base';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Base");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        Ok(())
    }

    // ── use base 'Base' → Inherits ──────────────────────────────────────

    #[test]
    fn test_use_base_single() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nuse base 'Base';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Base");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        assert_eq!(edge.confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn test_use_base_qw_multiple() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nuse base qw(Base1 Base2);\n1;");
        assert_eq!(edges.len(), 2, "expected two edges, got {}", edges.len());

        assert_eq!(edges[0].to_package, "Base1");
        assert_eq!(edges[1].to_package, "Base2");
        Ok(())
    }

    // ── @ISA = ('Base') → Inherits ──────────────────────────────────────

    #[test]
    fn test_isa_assignment_bare() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\n@ISA = ('Base');\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Base");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        Ok(())
    }

    #[test]
    fn test_isa_assignment_our() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\nour @ISA = qw(Base1 Base2);\n1;");
        assert_eq!(edges.len(), 2, "expected two edges, got {}", edges.len());

        assert_eq!(edges[0].to_package, "Base1");
        assert_eq!(edges[1].to_package, "Base2");
        Ok(())
    }

    // ── push @ISA, 'Base' → Inherits ────────────────────────────────────

    #[test]
    fn test_push_isa_single() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\npush @ISA, 'Base';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Base");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        Ok(())
    }

    #[test]
    fn test_push_isa_multiple() -> Result<(), String> {
        let edges = parse_and_extract("package Child;\npush @ISA, 'Base1', 'Base2';\n1;");
        assert_eq!(edges.len(), 2, "expected two edges, got {}", edges.len());

        assert_eq!(edges[0].to_package, "Base1");
        assert_eq!(edges[1].to_package, "Base2");
        Ok(())
    }

    // ── extends 'Base' (Moo/Moose) → Inherits ──────────────────────────

    #[test]
    fn test_extends_single() -> Result<(), String> {
        let edges =
            parse_and_extract("package MyApp::Admin;\nuse Moose;\nextends 'MyApp::User';\n1;");
        // May also get a DependsOn for `use Moose` — filter to Inherits.
        let inherits: Vec<_> =
            edges.iter().filter(|e| e.kind == PackageEdgeKind::Inherits).collect();
        let edge = inherits.first().ok_or("expected at least one Inherits edge")?;

        assert_eq!(edge.from_package, "MyApp::Admin");
        assert_eq!(edge.to_package, "MyApp::User");
        assert_eq!(edge.kind, PackageEdgeKind::Inherits);
        Ok(())
    }

    // ── with 'Role' (Moo/Moose) → ComposesRole ─────────────────────────

    #[test]
    fn test_with_single_role() -> Result<(), String> {
        let edges =
            parse_and_extract("package MyApp::User;\nuse Moose;\nwith 'MyApp::Printable';\n1;");
        let roles: Vec<_> =
            edges.iter().filter(|e| e.kind == PackageEdgeKind::ComposesRole).collect();
        let edge = roles.first().ok_or("expected at least one ComposesRole edge")?;

        assert_eq!(edge.from_package, "MyApp::User");
        assert_eq!(edge.to_package, "MyApp::Printable");
        assert_eq!(edge.kind, PackageEdgeKind::ComposesRole);
        Ok(())
    }

    #[test]
    fn test_with_multiple_roles() -> Result<(), String> {
        let edges =
            parse_and_extract("package MyApp::User;\nuse Moose;\nwith 'Role1', 'Role2';\n1;");
        let roles: Vec<_> =
            edges.iter().filter(|e| e.kind == PackageEdgeKind::ComposesRole).collect();
        assert_eq!(roles.len(), 2, "expected two ComposesRole edges, got {}", roles.len());

        assert_eq!(roles[0].to_package, "Role1");
        assert_eq!(roles[1].to_package, "Role2");
        Ok(())
    }

    // ── Package context tracking ────────────────────────────────────────

    #[test]
    fn test_multiple_packages() -> Result<(), String> {
        let code = r#"
package Parent;
1;

package Child;
use parent 'Parent';
1;
"#;
        let edges = parse_and_extract(code);
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "Child");
        assert_eq!(edge.to_package, "Parent");
        Ok(())
    }

    #[test]
    fn test_default_main_package() -> Result<(), String> {
        let edges = parse_and_extract("use parent 'Base';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "main");
        assert_eq!(edge.to_package, "Base");
        Ok(())
    }

    #[test]
    fn test_package_declaration_inside_block_restores_outer_package() -> Result<(), String> {
        let code = r#"
package Outer;
{
    package Inner;
    use parent 'InnerBase';
}
use parent 'OuterBase';
1;
"#;
        let edges = parse_and_extract(code);
        assert_eq!(edges.len(), 2, "expected two inheritance edges, got {}", edges.len());

        assert_eq!(edges[0].from_package, "Inner");
        assert_eq!(edges[0].to_package, "InnerBase");
        assert_eq!(edges[1].from_package, "Outer");
        assert_eq!(edges[1].to_package, "OuterBase");
        Ok(())
    }

    // ── Combined patterns ───────────────────────────────────────────────

    #[test]
    fn test_extends_and_with_combined() -> Result<(), String> {
        let code = r#"
package MyApp::Admin;
use Moose;
extends 'MyApp::User';
with 'MyApp::Printable', 'MyApp::Serializable';
1;
"#;
        let edges = parse_and_extract(code);
        let inherits: Vec<_> =
            edges.iter().filter(|e| e.kind == PackageEdgeKind::Inherits).collect();
        let roles: Vec<_> =
            edges.iter().filter(|e| e.kind == PackageEdgeKind::ComposesRole).collect();

        assert_eq!(inherits.len(), 1, "expected one Inherits edge");
        assert_eq!(inherits[0].to_package, "MyApp::User");

        assert_eq!(roles.len(), 2, "expected two ComposesRole edges");
        assert_eq!(roles[0].to_package, "MyApp::Printable");
        assert_eq!(roles[1].to_package, "MyApp::Serializable");
        Ok(())
    }

    // ── No edges for unrelated code ─────────────────────────────────────

    #[test]
    fn test_no_edges_for_plain_use() -> Result<(), String> {
        let edges = parse_and_extract("package Foo;\nuse strict;\nuse warnings;\n1;");
        // No inheritance or role edges expected.
        let inheritance_edges: Vec<_> = edges
            .iter()
            .filter(|e| {
                e.kind == PackageEdgeKind::Inherits || e.kind == PackageEdgeKind::ComposesRole
            })
            .collect();
        assert!(
            inheritance_edges.is_empty(),
            "expected no inheritance/role edges, got {inheritance_edges:?}"
        );
        Ok(())
    }

    // ── Framework activation gating (negative controls) ─────────────────

    /// Collect only the framework-DSL-derived edge kinds.
    fn dsl_edges(code: &str) -> Vec<PackageEdge> {
        parse_and_extract(code)
            .into_iter()
            .filter(|e| {
                e.kind == PackageEdgeKind::Inherits || e.kind == PackageEdgeKind::ComposesRole
            })
            .collect()
    }

    #[test]
    fn bare_extends_without_framework_activation_emits_no_edge() {
        // Ordinary Perl: a user sub named `extends` is not Moo/Moose inheritance.
        let edges = dsl_edges("package Plain;\nsub extends { 1 }\nextends 'NotAParent';\n1;");
        assert!(edges.is_empty(), "expected no edges without activation, got {edges:?}");
    }

    #[test]
    fn bare_with_without_framework_activation_emits_no_edge() {
        let edges = dsl_edges("package Plain;\nsub with { 1 }\nwith 'NotARole';\n1;");
        assert!(edges.is_empty(), "expected no edges without activation, got {edges:?}");
    }

    #[test]
    fn dsl_spelling_alone_emits_no_edge() {
        // No framework anywhere and no local sub: the spelling alone must not
        // manufacture inheritance or role facts. This pins the activation
        // gate independently of the shadowing gate.
        let edges = dsl_edges("package Plain;\nextends 'NotAParent';\nwith 'NotARole';\n1;");
        assert!(edges.is_empty(), "spelling alone must not emit edges, got {edges:?}");
    }

    #[test]
    fn extends_before_activation_emits_no_edge() {
        // Source order matters: the DSL is not imported yet at the call site.
        let edges = dsl_edges("package Early;\nextends 'NotAParent';\nuse Moose;\n1;");
        assert!(edges.is_empty(), "expected no edges before activation, got {edges:?}");
    }

    #[test]
    fn activation_in_another_package_does_not_activate_this_one() {
        let code = "package Framed;\nuse Moose;\n1;\n\npackage Plain;\nextends 'NotAParent';\n1;\n";
        let edges = dsl_edges(code);
        assert!(
            edges.iter().all(|e| e.from_package != "Plain"),
            "activation must be package-local, got {edges:?}"
        );
    }

    #[test]
    fn local_sub_shadowing_the_dsl_suppresses_the_edge() {
        // A package-local `sub extends` replaces the imported DSL keyword.
        let edges =
            dsl_edges("package Shadow;\nuse Moose;\nsub extends { 1 }\nextends 'NotAParent';\n1;");
        assert!(
            edges.iter().all(|e| e.kind != PackageEdgeKind::Inherits),
            "a shadowing sub must prevent an exact Inherits edge, got {edges:?}"
        );
    }

    #[test]
    fn role_activation_grants_with_but_not_extends() {
        // `Moose::Role` exports `with`, but does not export `extends`.
        let edges = dsl_edges("package R;\nuse Moose::Role;\nwith 'Other::Role';\n1;");
        assert_eq!(edges.len(), 1, "expected exactly the role edge, got {edges:?}");
        assert_eq!(edges[0].kind, PackageEdgeKind::ComposesRole);
        assert_eq!(edges[0].to_package, "Other::Role");

        let bad = dsl_edges("package R;\nuse Moose::Role;\nextends 'NotAParent';\n1;");
        assert!(bad.is_empty(), "Moose::Role must not grant `extends`, got {bad:?}");
    }

    #[test]
    fn moo_and_mouse_activate_both_keywords() {
        for module in ["Moo", "Mouse"] {
            let code = format!("package P;\nuse {module};\nextends 'B';\nwith 'R';\n1;\n");
            let edges = dsl_edges(&code);
            assert_eq!(edges.len(), 2, "{module}: expected two edges, got {edges:?}");
            assert_eq!(edges[0].kind, PackageEdgeKind::Inherits);
            assert_eq!(edges[1].kind, PackageEdgeKind::ComposesRole);
        }
    }

    #[test]
    fn native_isa_relations_do_not_require_framework_activation() {
        // Gating applies only to the framework DSL, never to native Perl forms.
        for code in [
            "package C;\nuse parent 'B';\n1;",
            "package C;\nuse base 'B';\n1;",
            "package C;\nour @ISA = ('B');\n1;",
            "package C;\npush @ISA, 'B';\n1;",
        ] {
            let edges = dsl_edges(code);
            assert_eq!(edges.len(), 1, "native form must still emit: {code:?} -> {edges:?}");
            assert_eq!(edges[0].to_package, "B");
        }
    }

    // ── Qualified package names ─────────────────────────────────────────

    #[test]
    fn test_qualified_parent_names() -> Result<(), String> {
        let edges = parse_and_extract("package My::Child;\nuse parent 'My::Base::Class';\n1;");
        let edge = edges.first().ok_or("expected at least one PackageEdge")?;

        assert_eq!(edge.from_package, "My::Child");
        assert_eq!(edge.to_package, "My::Base::Class");
        Ok(())
    }
}
