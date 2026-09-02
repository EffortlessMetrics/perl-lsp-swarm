//! Registry-backed Mojo::Base activation-site extraction (#9681).
//!
//! Extracts the exact `use Mojo::Base ...;` import sites from an AST and
//! turns their argument lists into [`MojoBaseImportEvidence`] for the
//! registry-backed adapter in `perl-semantic-facts`. This is the source side
//! of the #9681 activation contract:
//!
//! - only an exact `use Mojo::Base` (optionally `use Mojo::Base <version>`)
//!   is an activation site — `use Mojo::Base::_RoleBase`, `require
//!   Mojo::Base`, and `has` calls never activate;
//! - the caller package scopes each site (activation does not leak across
//!   packages);
//! - the site retains the import statement's source interval, the literal
//!   parent's source range when present, and the source generation it was
//!   extracted from, so a stale detection cannot be reused against newer
//!   source;
//! - computed parent expressions stay explicit dynamic boundaries instead of
//!   silently becoming literals.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::mojo_base::{
    MojoBaseImportEvidence, MojoBaseParentSelection, MojoBaseSiteAnchor,
    parse_mojo_base_import_args,
};
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};

/// One exact `use Mojo::Base ...;` activation site in a source file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseActivationSite {
    /// File the import appears in.
    pub file_id: FileId,
    /// Deterministic anchor for the activating import statement.
    pub anchor_id: AnchorId,
    /// Load-bearing site identity (package, interval, parent range,
    /// generation).
    pub anchor: MojoBaseSiteAnchor,
    /// Parsed import evidence (parent/signatures/options).
    pub evidence: MojoBaseImportEvidence,
}

/// Whether a `use` module string is an exact Mojo::Base import.
///
/// `use Mojo::Base 9.34;` carries the version appended by the parser; nested
/// `Mojo::Base::*` modules are not activation sites.
fn is_exact_mojo_base_import(module: &str) -> bool {
    if module == "Mojo::Base" {
        return true;
    }
    match module.strip_prefix("Mojo::Base ") {
        Some(rest) => {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
        }
        None => false,
    }
}

/// Extract every exact Mojo::Base activation site from `ast`, in source
/// order.
///
/// `source` is the text the AST was parsed from; it locates the literal
/// parent's byte range inside the import statement. `generation` is the
/// current source generation of that file and is retained on every site.
#[must_use]
pub fn extract_mojo_base_activation_sites(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MojoBaseActivationSite> {
    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default activation scope for script-style Mojo apps.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(ast, source, file_id, generation, &mut current_package, &mut sites);
    sites
}

fn walk_activation_sites(
    node: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
    current_package: &mut Option<String>,
    sites: &mut Vec<MojoBaseActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } if is_exact_mojo_base_import(module) => {
            let span_start = node.location.start().min(u32::MAX as usize) as u32;
            let span_end = node.location.end().min(u32::MAX as usize) as u32;
            let evidence = parse_mojo_base_import_args(args);
            sites.push(MojoBaseActivationSite {
                file_id,
                anchor_id: AnchorId(node.location.start() as u64),
                anchor: MojoBaseSiteAnchor::new(
                    current_package.clone(),
                    span_start,
                    span_end,
                    literal_parent_range(source, span_start, span_end, &evidence),
                    generation.clone(),
                ),
                evidence,
            });
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, source, file_id, generation, sites);
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
                    source,
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
                    source,
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
        walk_activation_sites(child, source, file_id, generation.clone(), current_package, sites);
    }
}

fn walk_package_block(
    block: &Node,
    name: &str,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
    sites: &mut Vec<MojoBaseActivationSite>,
) {
    if let NodeKind::Block { statements } = &block.kind {
        let mut package_scope = Some(name.to_string());
        for statement in statements {
            walk_activation_sites(
                statement,
                source,
                file_id,
                generation.clone(),
                &mut package_scope,
                sites,
            );
        }
    }
}

/// Locate the literal parent spelling's byte range inside the import
/// statement, when the parent selection is a literal.
///
/// The parser stores import arguments as plain strings without per-token
/// spans, so the range is located deterministically inside the statement's
/// own source interval: the search starts after the `Mojo::Base` module
/// prefix (plus any version token) so a bareword parent that also occurs
/// inside the module name cannot capture the module's range, then tries the
/// quoted spelling before the bareword spelling. Absence of a located range
/// never fabricates one.
fn literal_parent_range(
    source: &str,
    span_start: u32,
    span_end: u32,
    evidence: &MojoBaseImportEvidence,
) -> Option<(u32, u32)> {
    let MojoBaseParentSelection::Literal(parent) = &evidence.parent else {
        return None;
    };
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    if start >= end {
        return None;
    }
    let statement = source.get(start..end)?;
    let search_from = argument_search_offset(statement);
    let arguments = statement.get(search_from..)?;
    for spelling in [format!("'{parent}'"), format!("\"{parent}\""), parent.clone()] {
        if let Some(offset) = arguments.find(&spelling) {
            let range_start = start + search_from + offset;
            let range_end = range_start + spelling.len();
            return Some((range_start as u32, range_end as u32));
        }
    }
    None
}

/// Byte offset where the import argument list starts inside one `use` statement:
/// after the module name and any version token that follows it.
fn argument_search_offset(statement: &str) -> usize {
    let Some(module_at) = statement.find("Mojo::Base") else {
        return 0;
    };
    let mut offset = module_at + "Mojo::Base".len();
    let rest = &statement[offset..];
    let version_len = rest.find(|c: char| !c.is_ascii_whitespace()).map_or(0, |lead| {
        let after_space = &rest[lead..];
        let version = after_space
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_')
            .count();
        if version > 0 { lead + version } else { 0 }
    });
    offset += version_len;
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn sites(code: &str) -> Vec<MojoBaseActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_mojo_base_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"))
    }

    #[test]
    fn exact_use_mojo_base_is_one_activation_site() {
        let found = sites("package App;\nuse Mojo::Base -base;\nhas attr => 1;\n");
        assert_eq!(found.len(), 1, "exactly one activation site per import");
        assert_eq!(found[0].anchor.package.as_deref(), Some("App"));
    }

    #[test]
    fn literal_parent_form_retains_spelling_and_range() {
        let code = "package Log;\nuse Mojo::Base 'Mojo::EventEmitter', -signatures;\n";
        let found = sites(code);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].evidence.parent,
            MojoBaseParentSelection::Literal("Mojo::EventEmitter".to_string())
        );
        assert!(found[0].evidence.signatures);
        let (start, end) = must_some(found[0].anchor.parent_range);
        assert_eq!(
            &code[(start as usize)..(end as usize)],
            "'Mojo::EventEmitter'",
            "the parent range must cover the literal spelling"
        );
    }

    #[test]
    fn nested_role_base_and_require_do_not_activate() {
        assert!(
            sites("use Mojo::Base::_RoleBase;\nrequire Mojo::Base;\nuse Mojolicious;\n").is_empty()
        );
    }

    #[test]
    fn has_calls_alone_are_not_activation_proof() {
        // A raw `has` call without a Mojo::Base import is not an activation
        // site (#9681 negative control).
        assert!(sites("package App;\nhas attr => (is => 'ro');\n").is_empty());
    }

    #[test]
    fn unqualified_file_defaults_to_main_package() {
        let found = sites("use Mojo::Base -base;\n1;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].anchor.package.as_deref(), Some("main"));
    }

    #[test]
    fn lexical_block_package_state_is_restored() {
        let found = sites(
            "package Outer; { package Inner; use Mojo::Base -base; } use Mojo::Base -base;
",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].anchor.package.as_deref(), Some("Inner"));
        assert_eq!(
            found[1].anchor.package.as_deref(),
            Some("Outer"),
            "inner block-scoped package must not leak past the closing brace"
        );
    }

    #[test]
    fn versioned_use_mojo_base_activates() {
        assert_eq!(sites("use Mojo::Base 9.34;\n").len(), 1);
    }

    #[test]
    fn package_scoping_is_tracked_per_site() {
        let found = sites(
            "package App;\nuse Mojo::Base -base;\npackage Other;\nuse Mojo::Base 'Parent';\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].anchor.package.as_deref(), Some("App"));
        assert_eq!(found[1].anchor.package.as_deref(), Some("Other"));
        assert_eq!(
            found[1].evidence.parent,
            MojoBaseParentSelection::Literal("Parent".to_string())
        );
    }

    #[test]
    fn bareword_parent_range_avoids_module_name_capture() {
        // A bareword parent that also occurs inside the module name must not
        // capture the module's own range.
        let code = "package App;\nuse Mojo::Base Base;\n";
        let found = sites(code);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.parent, MojoBaseParentSelection::Literal("Base".to_string()));
        let (start, end) = must_some(found[0].anchor.parent_range);
        assert_eq!(
            &code[(start as usize)..(end as usize)],
            "Base",
            "range must cover the argument, not the module name"
        );
        let module_prefix_end = found[0].anchor.span_start_byte + "use Mojo::Base".len() as u32;
        assert!(start >= module_prefix_end, "range must start after the module name prefix");
    }

    #[test]
    fn site_retains_statement_interval_and_generation() {
        let code = "package App;\nuse Mojo::Base -base;\n";
        let found = sites(code);
        assert_eq!(found.len(), 1);
        let (start, end) = (found[0].anchor.span_start_byte, found[0].anchor.span_end_byte);
        assert!(end > start, "interval must be non-empty");
        assert!(
            code[(start as usize)..(end as usize)].contains("use Mojo::Base -base"),
            "interval must cover the activating import"
        );
        assert_eq!(found[0].anchor.source_generation, SourceGeneration::known("gen-1"));
    }
}
