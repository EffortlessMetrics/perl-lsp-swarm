//! Registry-backed Mojolicious::Lite activation-site extraction (#9688).
//!
//! Extracts the exact `use Mojolicious::Lite ...;` import sites from an AST
//! and turns their argument lists into [`MojoliciousLiteImportEvidence`] for
//! the registry-backed adapter in `perl-semantic-facts`. This is the source
//! side of the #9688 Lite profile:
//!
//! - only an exact `use Mojolicious::Lite` (optionally `use
//!   Mojolicious::Lite <version>`) is an activation site — `use
//!   Mojolicious`, `use Mojolicious::Lite::Something`, `require
//!   Mojolicious::Lite`, and same-named local calls never activate;
//! - the caller package scopes each site, so a Lite activation cannot leak
//!   across packages;
//! - the site retains the import statement's source interval and the source
//!   generation it was extracted from, so a stale detection cannot be reused
//!   against newer source.
//!
//! The full-application and controller profiles have no extractor here on
//! purpose: they are ordinary `use Mojo::Base '<parent>';` imports already
//! extracted by
//! [`crate::analysis::mojo_base_activation::extract_mojo_base_activation_sites`],
//! and classified by
//! [`perl_semantic_facts::framework_adapters::mojolicious::mojolicious_role_facts_from_mojo_base`].
//! Adding a second recognizer for them would duplicate the Mojo::Base
//! authority this profile is required to consume.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::mojolicious::{
    MojoliciousLiteImportEvidence, MojoliciousSiteAnchor, parse_mojolicious_lite_import_args,
};
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};

/// One exact `use Mojolicious::Lite ...;` activation site in a source file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoliciousLiteActivationSite {
    /// File the import appears in.
    pub file_id: FileId,
    /// Deterministic anchor for the activating import statement.
    pub anchor_id: AnchorId,
    /// Load-bearing site identity (package, interval, generation).
    pub anchor: MojoliciousSiteAnchor,
    /// Parsed import evidence (selection/signatures/options).
    pub evidence: MojoliciousLiteImportEvidence,
}

/// Whether a `use` module string is an exact `Mojolicious::Lite` import.
///
/// `use Mojolicious::Lite 9.34;` carries the version appended by the parser.
/// Nested `Mojolicious::Lite::*` modules and the plain `Mojolicious` module
/// are not Lite activation sites.
fn is_exact_mojolicious_lite_import(module: &str) -> bool {
    if module == "Mojolicious::Lite" {
        return true;
    }
    match module.strip_prefix("Mojolicious::Lite ") {
        Some(rest) => {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
        }
        None => false,
    }
}

/// Extract every exact Mojolicious::Lite activation site from `ast`, in
/// source order.
///
/// `generation` is the current source generation of that file and is
/// retained on every site.
#[must_use]
pub fn extract_mojolicious_lite_activation_sites(
    ast: &Node,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MojoliciousLiteActivationSite> {
    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default activation scope for script-style Lite apps, which are the
    // overwhelmingly common shape.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(ast, file_id, generation, &mut current_package, &mut sites);
    sites
}

fn walk_activation_sites(
    node: &Node,
    file_id: FileId,
    generation: SourceGeneration,
    current_package: &mut Option<String>,
    sites: &mut Vec<MojoliciousLiteActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } if is_exact_mojolicious_lite_import(module) => {
            let span_start = node.location.start.min(u32::MAX as usize) as u32;
            let span_end = node.location.end.min(u32::MAX as usize) as u32;
            sites.push(MojoliciousLiteActivationSite {
                file_id,
                anchor_id: AnchorId(node.location.start as u64),
                anchor: MojoliciousSiteAnchor::new(
                    current_package.clone(),
                    span_start,
                    span_end,
                    // A Lite import has no literal parent spelling; the role
                    // comes from the module itself.
                    None,
                    generation.clone(),
                ),
                evidence: parse_mojolicious_lite_import_args(args),
            });
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, file_id, generation, sites);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            // Bare `package X;` switches the package for following statements.
            *current_package = Some(name.clone());
        }
        NodeKind::Program { statements } => {
            // File scope: a bare `package X;` persists for the rest of the file.
            for statement in statements {
                walk_activation_sites(
                    statement,
                    file_id,
                    generation.clone(),
                    current_package,
                    sites,
                );
            }
            return;
        }
        NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations:
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards.
            let mut block_package = current_package.clone();
            for statement in statements {
                walk_activation_sites(
                    statement,
                    file_id,
                    generation.clone(),
                    &mut block_package,
                    sites,
                );
            }
            return;
        }
        _ => {}
    }
    for child in node.children() {
        walk_activation_sites(child, file_id, generation.clone(), current_package, sites);
    }
}

fn walk_package_block(
    block: &Node,
    name: &str,
    file_id: FileId,
    generation: SourceGeneration,
    sites: &mut Vec<MojoliciousLiteActivationSite>,
) {
    if let NodeKind::Block { statements } = &block.kind {
        let mut package_scope = Some(name.to_string());
        for statement in statements {
            walk_activation_sites(
                statement,
                file_id,
                generation.clone(),
                &mut package_scope,
                sites,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_semantic_facts::framework_adapters::mojolicious::MojoliciousLiteImportSelection;
    use perl_tdd_support::must;

    fn sites(code: &str) -> Vec<MojoliciousLiteActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_mojolicious_lite_activation_sites(&ast, FileId(1), SourceGeneration::known("gen-1"))
    }

    #[test]
    fn exact_lite_import_is_one_activation_site_scoped_to_main() {
        let found = sites("use Mojolicious::Lite;\nget '/' => sub { 1 };\n");
        assert_eq!(found.len(), 1, "exactly one activation site per import");
        assert_eq!(found[0].anchor.package.as_deref(), Some("main"));
        assert_eq!(found[0].evidence.selection, MojoliciousLiteImportSelection::Default);
    }

    #[test]
    fn signatures_import_option_is_retained() {
        let found = sites("use Mojolicious::Lite -signatures;\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].evidence.signatures);
        assert_eq!(found[0].evidence.selection, MojoliciousLiteImportSelection::Signatures);
    }

    #[test]
    fn versioned_lite_import_is_still_an_activation_site() {
        let found = sites("use Mojolicious::Lite 9.34;\n");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn the_plain_mojolicious_module_is_not_a_lite_activation_site() {
        // Negative control: `use Mojolicious;` loads the framework class; it
        // is not the Lite DSL import.
        assert!(sites("use Mojolicious;\n").is_empty());
    }

    #[test]
    fn nested_lite_modules_are_not_activation_sites() {
        // Negative control, mirroring the Mojo::Base adapter's containment.
        assert!(sites("use Mojolicious::Lite::Plugin;\n").is_empty());
        assert!(sites("use Mojolicious::Commands;\n").is_empty());
    }

    #[test]
    fn a_same_named_local_call_is_not_an_activation() {
        // Negative control: only an import activates.
        assert!(sites("Mojolicious::Lite();\n").is_empty());
    }

    #[test]
    fn the_activating_package_scopes_the_site() {
        let found = sites("package MyApp;\nuse Mojolicious::Lite;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].anchor.package.as_deref(), Some("MyApp"));
    }

    #[test]
    fn lite_activation_does_not_leak_across_packages() {
        let found =
            sites("package First;\nuse Mojolicious::Lite;\npackage Second;\nsub helper { 1 }\n");
        assert_eq!(found.len(), 1, "the second package has no import of its own");
        assert_eq!(found[0].anchor.package.as_deref(), Some("First"));
    }

    #[test]
    fn each_package_owns_its_own_activation_site() {
        let found = sites(
            "package First;\nuse Mojolicious::Lite;\npackage Second;\nuse Mojolicious::Lite;\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].anchor.package.as_deref(), Some("First"));
        assert_eq!(found[1].anchor.package.as_deref(), Some("Second"));
    }

    #[test]
    fn the_site_retains_its_source_interval_and_generation() {
        let code = "use Mojolicious::Lite;\n";
        let found = sites(code);
        assert_eq!(found.len(), 1);
        let anchor = &found[0].anchor;
        assert!(
            anchor.span_end_byte > anchor.span_start_byte,
            "the import interval must be non-empty"
        );
        assert!(anchor.span_end_byte as usize <= code.len());
        assert_eq!(anchor.source_generation, SourceGeneration::known("gen-1"));
        assert_eq!(anchor.parent_range, None, "a Lite import has no literal parent spelling");
    }

    #[test]
    fn a_dynamic_import_argument_is_recorded_as_a_boundary() {
        let found = sites("use Mojolicious::Lite $flag;\n");
        assert_eq!(found.len(), 1);
        assert!(matches!(
            found[0].evidence.selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
    }
}
