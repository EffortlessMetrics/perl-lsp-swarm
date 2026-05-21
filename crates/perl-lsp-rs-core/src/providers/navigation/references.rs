//! Find-all-references functionality for symbol usage analysis in Perl scripts
//!
//! This module provides comprehensive reference finding capabilities for Perl script
//! development within the LSP workflow. Enables developers to quickly locate all
//! usage sites of variables, functions, and packages across Perl code.
//!
//! # LSP Workflow Integration
//!
//! - **Parse**: Identifies symbol definitions during Perl script parsing
//! - **Index**: Supports refactoring and symbol standardization
//! - **Navigate**: Analyzes variable flow and dependencies in Perl code
//! - **Complete**: Enables reference highlighting and navigation in editors
//! - **Analyze**: Powers workspace-wide symbol usage tracking
//!
//! # Usage Examples
//!
//! ```rust,ignore
//! use perl_parser_core::{Parser, Node};
//! use perl_lsp_providers::ide::lsp_compat::references::find_references_single_file;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let script = "my $count = 0; $count++; print $count;";
//! let mut parser = Parser::new(script);
//! let ast = parser.parse()?;
//!
//! // Find all references to $count
//! if let Some(refs) = find_references_single_file(&ast, 3) { // Position of first $count
//!     println!("Found {} references to $count", refs.len());
//!     for (start, end) in refs {
//!         println!("Reference at {}-{}: {}", start, end, &script[start..end]);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::qualified_name::split_qualified_name;

fn callable_identity(name: &str) -> (&str, &str) {
    let (pkg, bare) = split_qualified_name(name);
    (pkg.unwrap_or("main"), bare)
}

/// Return (start_offset, end_offset) for same-file references
pub fn find_references_single_file(ast: &Node, offset: usize) -> Option<Vec<(usize, usize)>> {
    let needle = find_node_at_offset(ast, offset)?;

    // Determine target "identity"
    let (want_kind, want_pkg, want_name, want_sigil) = match &needle.kind {
        NodeKind::Variable { sigil, name } => {
            let sigil_char = sigil.chars().next();
            ("var", "main".to_string(), name.clone(), sigil_char)
        }
        NodeKind::FunctionCall { name, .. } | NodeKind::Subroutine { name: Some(name), .. } => {
            let (pkg, bare) = callable_identity(name);
            ("sub", pkg.to_string(), bare.to_string(), None)
        }
        _ => return None,
    };

    let mut out = Vec::new();

    fn walk(
        node: &Node,
        out: &mut Vec<(usize, usize)>,
        want_kind: &str,
        want_pkg: &str,
        want_name: &str,
        want_sigil: Option<char>,
    ) {
        let location = &node.location;
        match &node.kind {
            NodeKind::Variable { sigil, name } if want_kind == "var" => {
                let sig_char = sigil.chars().next();
                if sig_char == want_sigil && name == want_name {
                    out.push((location.start, location.end));
                }
            }
            NodeKind::FunctionCall { name, .. } if want_kind == "sub" => {
                let (pkg, bare) = callable_identity(name);
                if bare == want_name && pkg == want_pkg {
                    out.push((location.start, location.end));
                }
            }
            NodeKind::Subroutine { name: Some(name), .. } if want_kind == "sub" => {
                let (pkg, bare) = callable_identity(name);
                if bare == want_name && pkg == want_pkg {
                    out.push((location.start, location.end));
                }
            }
            _ => {}
        }

        // Walk children
        for ch in get_node_children(node) {
            walk(ch, out, want_kind, want_pkg, want_name, want_sigil);
        }
    }

    walk(ast, &mut out, want_kind, &want_pkg, &want_name, want_sigil);
    Some(out)
}

fn find_node_at_offset(node: &Node, offset: usize) -> Option<&Node> {
    if offset < node.location.start || offset > node.location.end {
        return None;
    }

    // Check children first for more specific match
    let children = get_node_children(node);
    for child in children {
        if let Some(found) = find_node_at_offset(child, offset) {
            return Some(found);
        }
    }

    // If no child contains the offset, return this node
    Some(node)
}

fn get_node_children(node: &Node) -> Vec<&Node> {
    node.children()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;

    fn parse(source: &str) -> anyhow::Result<Node> {
        let mut parser = Parser::new(source);
        parser.parse().map_err(Into::into)
    }

    #[test]
    fn qualified_sub_declaration_found_in_references() -> anyhow::Result<()> {
        // `sub Foo::bar` stores name as "Foo::bar"; the walk must split it to
        // match against the bare "bar" and package "Foo" extracted at lookup
        // time.  Before the fix, name == want_name compared "Foo::bar" to "bar"
        // and the subroutine declaration was silently dropped from results.
        let source = "sub Foo::bar { 1 } Foo::bar();";
        let ast = parse(source)?;
        // Cursor at position 4 sits on the 'F' in `sub Foo::bar { ... }`,
        // which resolves to the Subroutine node whose name is "Foo::bar".
        let refs = find_references_single_file(&ast, 4);
        let refs =
            refs.ok_or_else(|| anyhow::anyhow!("should return Some for a known sub name"))?;
        // Must include both the declaration and the call site
        assert!(refs.len() >= 2, "expected at least 2 references, got {}", refs.len());
        Ok(())
    }

    #[test]
    fn bare_sub_declaration_still_found() -> anyhow::Result<()> {
        let source = "sub greet { } greet();";
        let ast = parse(source)?;
        let refs = find_references_single_file(&ast, 4);
        let refs = refs.ok_or_else(|| anyhow::anyhow!("expected subroutine references"))?;
        assert!(refs.len() >= 2, "expected declaration + call, got {}", refs.len());
        Ok(())
    }

    #[test]
    fn sub_in_different_package_not_confused() -> anyhow::Result<()> {
        // A subroutine named `bar` in package `Other` must NOT appear when
        // searching for `Foo::bar`.
        let source = "sub Foo::bar { 1 } sub Other::bar { 2 } Foo::bar();";
        let ast = parse(source)?;
        let refs = find_references_single_file(&ast, 4);
        let refs = refs.ok_or_else(|| anyhow::anyhow!("expected references for Foo::bar"))?;
        // Should find `sub Foo::bar` and the call `Foo::bar()`, but NOT `sub Other::bar`
        for &(start, end) in &refs {
            let slice = &source[start..end];
            assert!(
                !slice.contains("Other"),
                "Other::bar must not appear in results for Foo::bar, but got: {slice:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn default_main_package_applies_to_bare_and_qualified_main_calls() -> anyhow::Result<()> {
        let source = "sub greet { } main::greet(); greet();";
        let ast = parse(source)?;
        let refs = find_references_single_file(&ast, 4);
        let refs = refs.ok_or_else(|| anyhow::anyhow!("expected references for greet"))?;

        assert_eq!(refs.len(), 3, "expected declaration and both call forms");
        Ok(())
    }
}
