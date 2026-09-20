//! Registry-backed Dancer2 activation-site extraction (#8914).
//!
//! Extracts the exact `use Dancer2 ...;` import sites from an AST and turns
//! their argument lists into [`Dancer2ImportEvidence`] for the registry-backed
//! adapter in `perl-semantic-facts`. This is the source side of the #8914
//! activation contract:
//!
//! - only an exact `use Dancer2` (optionally `use Dancer2 <version>`) is an
//!   activation site — `use Dancer2::Core`, `use Dancer2::Core::App`,
//!   `use Dancer2::Plugin`, and Dancer v1 `use Dancer` never activate,
//!   preserving the containment landed for #8910;
//! - the caller package scopes each site (activation does not leak across
//!   packages);
//! - computed import options stay explicit dynamic boundaries instead of
//!   silently becoming defaults.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2::Dancer2ImportEvidence;
use perl_semantic_facts::framework_adapters::dancer2::parse_dancer2_import_args;
use perl_semantic_facts::{AnchorId, FileId};

/// One exact `use Dancer2 ...;` activation site in a source file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2ActivationSite {
    /// Caller package at the activating import (application identity scope).
    pub package: Option<String>,
    /// File the import appears in.
    pub file_id: FileId,
    /// Deterministic anchor for the activating import statement.
    pub anchor_id: AnchorId,
    /// Start byte of the activating import statement.
    pub span_start_byte: u32,
    /// Parsed import evidence (appname/DSL/exclusions).
    pub evidence: Dancer2ImportEvidence,
}

/// Whether a `use` module string is an exact Dancer2 DSL import.
///
/// `use Dancer2 1.123;` carries the version appended by the parser; nested
/// `Dancer2::*` modules and Dancer v1 are not activation.
fn is_exact_dancer2_import(module: &str) -> bool {
    if module == "Dancer2" {
        return true;
    }
    match module.strip_prefix("Dancer2 ") {
        Some(rest) => {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
        }
        None => false,
    }
}

/// Extract every exact Dancer2 activation site from `ast`, in source order.
#[must_use]
pub fn extract_dancer2_activation_sites(ast: &Node, file_id: FileId) -> Vec<Dancer2ActivationSite> {
    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default application identity scope for script-style Dancer2 apps.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(ast, file_id, &mut current_package, &mut sites);
    sites
}

fn walk_activation_sites(
    node: &Node,
    file_id: FileId,
    current_package: &mut Option<String>,
    sites: &mut Vec<Dancer2ActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } if is_exact_dancer2_import(module) => {
            sites.push(Dancer2ActivationSite {
                package: current_package.clone(),
                file_id,
                anchor_id: AnchorId(node.location.start as u64),
                span_start_byte: node.location.start.min(u32::MAX as usize) as u32,
                evidence: parse_dancer2_import_args(args),
            });
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, file_id, sites);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            // Bare `package X;` switches the package for following statements.
            *current_package = Some(name.clone());
        }
        NodeKind::Program { statements } => {
            // File scope: a bare `package X;` persists for the rest of the file.
            for statement in statements {
                walk_activation_sites(statement, file_id, current_package, sites);
            }
            return;
        }
        NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations:
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards.
            let mut block_package = current_package.clone();
            for statement in statements {
                walk_activation_sites(statement, file_id, &mut block_package, sites);
            }
            return;
        }
        _ => {}
    }
    for child in node.children() {
        walk_activation_sites(child, file_id, current_package, sites);
    }
}

fn walk_package_block(
    block: &Node,
    name: &str,
    file_id: FileId,
    sites: &mut Vec<Dancer2ActivationSite>,
) {
    if let NodeKind::Block { statements } = &block.kind {
        let mut package_scope = Some(name.to_string());
        for statement in statements {
            walk_activation_sites(statement, file_id, &mut package_scope, sites);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn sites(code: &str) -> Vec<Dancer2ActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_activation_sites(&ast, FileId(1))
    }

    #[test]
    fn exact_use_dancer2_is_one_activation_site() {
        let found = sites("package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n");
        assert_eq!(found.len(), 1, "exactly one activation site per import");
        assert_eq!(found[0].package.as_deref(), Some("App"));
    }

    #[test]
    fn dancer2_core_and_plugin_do_not_activate() {
        assert!(
            sites("use Dancer2::Core;\nuse Dancer2::Core::App;\nuse Dancer2::Plugin;\n").is_empty()
        );
    }

    #[test]
    fn dancer_v1_does_not_activate() {
        assert!(sites("use Dancer;\n").is_empty());
    }

    #[test]
    fn unqualified_file_defaults_to_main_package() {
        let found = sites(
            "use Dancer2;
get '/x' => sub { 1 };
",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package.as_deref(), Some("main"));
    }

    #[test]
    fn lexical_block_package_state_is_restored() {
        let found = sites(
            "package Outer; { package Inner; use Dancer2; } use Dancer2;
",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("Inner"));
        assert_eq!(
            found[1].package.as_deref(),
            Some("Outer"),
            "inner block-scoped package must not leak past the closing brace"
        );
    }

    #[test]
    fn versioned_use_dancer2_activates() {
        let found = sites("use Dancer2 1.123;\n");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn package_scoping_is_tracked() {
        let found =
            sites("package App;\nuse Dancer2;\npackage Other;\nuse Dancer2 appname => 'X';\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("App"));
        assert_eq!(found[1].package.as_deref(), Some("Other"));
        assert_eq!(
            must_some(found[1].evidence.appname.as_ref()).clone(),
            perl_semantic_facts::framework_adapters::dancer2::AppNameSelection::Literal(
                "X".to_string()
            )
        );
    }
}
