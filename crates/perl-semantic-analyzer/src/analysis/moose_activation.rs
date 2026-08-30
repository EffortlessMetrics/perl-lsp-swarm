//! Source-backed Moose and Moose::Role activation-site extraction (#7788).
//!
//! Only exact `use Moose` and `use Moose::Role` imports enter the activation
//! surface. Same-named calls, package shape, nested `Moose::*` modules,
//! `require` wrappers, and unrelated DSL imports never activate this
//! detector. Each site retains package, statement interval, file, source
//! generation, requested version spelling, and any unmodeled import boundary.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::moose::{
    MooseActivationKind, MooseImportDisposition, MooseSiteAnchor,
};
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};

/// One source-backed Moose or Moose::Role activation observation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MooseActivationSite {
    /// File containing the import.
    pub file_id: FileId,
    /// Deterministic source anchor for the import statement.
    pub anchor_id: AnchorId,
    /// Class versus role activation, derived only from the exact module name.
    pub kind: MooseActivationKind,
    /// Static source version requirement, when present.
    pub requested_version: Option<String>,
    /// Reviewed or unmodeled import arguments.
    pub import_disposition: MooseImportDisposition,
    /// Load-bearing package, interval, and generation identity.
    pub anchor: MooseSiteAnchor,
}

impl MooseActivationSite {
    /// Whether this site can participate in exact activation.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        self.import_disposition.is_exact()
    }
}

/// Extract every Moose activation observation from `ast`, in source order.
#[must_use]
pub fn extract_moose_activation_sites(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MooseActivationSite> {
    let mut sites = Vec::new();
    let mut current_package = Some("main".to_string());
    walk_activation_sites(ast, source, file_id, generation, &mut current_package, &mut sites);
    sites
}

fn walk_activation_sites(
    node: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
    current_package: &mut Option<String>,
    sites: &mut Vec<MooseActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } => {
            let source_span = source.get(node.location.start..node.location.end);
            if let Some((kind, requested_version, import_disposition)) =
                classify_moose_import(module, args, source_span)
            {
                sites.push(MooseActivationSite {
                    file_id,
                    anchor_id: AnchorId(node.location.start as u64),
                    kind,
                    requested_version,
                    import_disposition,
                    anchor: MooseSiteAnchor::new(
                        current_package.clone(),
                        node.location.start.min(u32::MAX as usize) as u32,
                        node.location.end.min(u32::MAX as usize) as u32,
                        generation.clone(),
                    ),
                });
            }
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, source, file_id, generation, sites);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = Some(name.clone());
        }
        NodeKind::Program { statements } => {
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
    sites: &mut Vec<MooseActivationSite>,
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

fn classify_moose_import(
    module: &str,
    args: &[String],
    source_span: Option<&str>,
) -> Option<(MooseActivationKind, Option<String>, MooseImportDisposition)> {
    let (kind, mut requested_version) = classify_module(module)?;
    let Some(source_span) = source_span else {
        return Some((
            kind,
            requested_version,
            MooseImportDisposition::Unmodeled {
                arguments: vec!["<invalid-source-span>".to_string()],
            },
        ));
    };
    let normalized = normalize_import_args(args);
    let import_disposition = if normalized.is_empty() {
        if source_span.contains('(') {
            MooseImportDisposition::Unmodeled { arguments: vec!["(".to_string(), ")".to_string()] }
        } else {
            MooseImportDisposition::Exact
        }
    } else if requested_version.is_none()
        && normalized.len() == 1
        && is_version_spelling(&normalized[0])
    {
        requested_version = normalized.first().cloned();
        MooseImportDisposition::Exact
    } else {
        MooseImportDisposition::Unmodeled { arguments: normalized }
    };
    Some((kind, requested_version, import_disposition))
}

fn classify_module(module: &str) -> Option<(MooseActivationKind, Option<String>)> {
    exact_module_version(module, "Moose::Role")
        .map(|version| (MooseActivationKind::Role, version))
        .or_else(|| {
            exact_module_version(module, "Moose")
                .map(|version| (MooseActivationKind::Class, version))
        })
}

fn exact_module_version(module: &str, expected: &str) -> Option<Option<String>> {
    let suffix = module.strip_prefix(expected)?;
    if suffix.is_empty() {
        return Some(None);
    }
    let version = suffix.strip_prefix(' ')?;
    if is_version_spelling(version) { Some(Some(version.to_string())) } else { None }
}

fn is_version_spelling(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | '_'))
}

fn normalize_import_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|argument| argument.trim())
        .filter(|argument| !argument.is_empty() && !matches!(*argument, "," | "=>" | ";"))
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn sites(code: &str) -> Vec<MooseActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_moose_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"))
    }

    #[test]
    fn exact_class_and_role_imports_keep_distinct_identity() {
        let found = sites("package Classish;\nuse Moose;\npackage Roleish;\nuse Moose::Role;\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, MooseActivationKind::Class);
        assert_eq!(found[0].anchor.package.as_deref(), Some("Classish"));
        assert_eq!(found[1].kind, MooseActivationKind::Role);
        assert_eq!(found[1].anchor.package.as_deref(), Some("Roleish"));
        assert!(found.iter().all(MooseActivationSite::is_exact));
    }

    #[test]
    fn nested_modules_require_wrappers_and_dsl_names_do_not_activate() {
        let found = sites(
            "package App;\n\
             use Moose::Util;\n\
             require Moose;\n\
             sub has { 1 }\n\
             sub extends { 1 }\n\
             sub with { 1 }\n\
             has thing => 1;\n",
        );
        assert!(found.is_empty());
    }

    #[test]
    fn package_shape_never_reclassifies_the_exact_import() {
        let found = sites(
            "package RoleShaped;\n\
             use Moose;\n\
             requires 'work';\n\
             package ClassShaped;\n\
             use Moose::Role;\n\
             sub new { bless {}, shift }\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, MooseActivationKind::Class);
        assert_eq!(found[1].kind, MooseActivationKind::Role);
    }

    #[test]
    fn versioned_import_retains_requested_version() {
        let found = sites("package App;\nuse Moose 2.4000;\n");
        let site = must_some(found.first());
        assert_eq!(site.requested_version.as_deref(), Some("2.4000"));
        assert!(site.is_exact());
    }

    #[test]
    fn unmodeled_import_arguments_are_an_explicit_boundary() {
        let found = sites("package App;\nuse Moose -traits => 'My::Trait';\n");
        let site = must_some(found.first());
        assert!(!site.is_exact());
        assert!(matches!(
            &site.import_disposition,
            MooseImportDisposition::Unmodeled { arguments } if !arguments.is_empty()
        ));
    }

    #[test]
    fn empty_import_list_does_not_activate_moose() {
        let found = sites("package App;\nuse Moose ();\n");
        let site = must_some(found.first());
        assert!(!site.is_exact());
        assert!(matches!(
            &site.import_disposition,
            MooseImportDisposition::Unmodeled { arguments }
                if arguments == &["(".to_string(), ")".to_string()]
        ));
    }

    #[test]
    fn lexical_block_package_state_is_restored() {
        let found = sites("package Outer; { package Inner; use Moose; } use Moose::Role;\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].anchor.package.as_deref(), Some("Inner"));
        assert_eq!(found[1].anchor.package.as_deref(), Some("Outer"));
    }

    #[test]
    fn site_retains_statement_interval_and_generation() {
        let code = "package App;\nuse Moose;\n";
        let found = sites(code);
        let site = must_some(found.first());
        let start = site.anchor.span_start_byte as usize;
        let end = site.anchor.span_end_byte as usize;
        assert!(end > start);
        assert!(code[start..end].contains("use Moose"));
        assert_eq!(site.anchor.source_generation, SourceGeneration::known("gen-1"));
    }
}
