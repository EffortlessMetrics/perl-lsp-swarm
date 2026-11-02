use super::ImportMap;
use perl_module::resolve_known_export_tag;
use perl_parser_core::ast::Node;
use perl_parser_core::hir::lower_ast;
use perl_semantic_analyzer::analysis::import_extractor::ImportExtractor;
use perl_semantic_facts::{FileId, ImportKind, ImportSpec, ImportSymbols};
use perl_workspace::semantic::workspace_import_extractor::extract_import_specs_from_hir;
use std::collections::{HashMap, HashSet};

mod used_modules;

use used_modules::is_importable_module;

/// Walk the document and build an `ImportMap` from canonical HIR import facts.
///
/// Only uppercase-starting module names are included (skips pragmas like
/// `strict`, `warnings`, `feature`, `constant`, `utf8`, `lib`, `parent`, `base`).
pub(super) fn extract_import_map(ast: &Node) -> ImportMap {
    extract_import_map_with_source(ast, None)
}

/// Same as [`extract_import_map`], but passes buffer source for framework import
/// normalization that requires source text.
pub(super) fn extract_import_map_with_source(ast: &Node, source: Option<&str>) -> ImportMap {
    let hir = lower_ast(ast);
    let specs = extract_import_specs_from_hir(&hir, ast, FileId(0), source);
    import_map_from_specs(&specs)
}

fn import_map_from_specs(specs: &[ImportSpec]) -> ImportMap {
    let mut map: ImportMap = HashMap::new();
    for spec in specs {
        if !is_importable_module(&spec.module) {
            continue;
        }
        match spec.kind {
            ImportKind::Use => {}
            ImportKind::UseEmpty => {
                map.entry(spec.module.clone()).or_default();
            }
            ImportKind::UseExplicitList | ImportKind::UseTag | ImportKind::UseConstant => {
                merge_spec_symbols(&spec.module, &spec.symbols, &mut map);
            }
            ImportKind::Require
            | ImportKind::RequireThenImport
            | ImportKind::DynamicRequire
            | ImportKind::ManualImport => {}
        }
    }
    map
}

fn merge_spec_symbols(module: &str, symbols: &ImportSymbols, map: &mut ImportMap) {
    match symbols {
        ImportSymbols::None => {
            map.entry(module.to_string()).or_default();
            return;
        }
        ImportSymbols::Default | ImportSymbols::Dynamic => return,
        ImportSymbols::Explicit(explicit) => {
            if explicit.is_empty() {
                map.entry(module.to_string()).or_default();
            } else {
                map.entry(module.to_string()).or_default().extend(explicit.iter().cloned());
            }
        }
        ImportSymbols::Tags(tags) => {
            apply_tag_and_name_symbols(module, tags, &[], map);
        }
        ImportSymbols::Mixed { tags, names: explicit } => {
            apply_tag_and_name_symbols(module, tags, explicit, map);
        }
    }
}

fn apply_tag_and_name_symbols(
    module: &str,
    tags: &[String],
    explicit: &[String],
    map: &mut ImportMap,
) {
    let mut names: HashSet<String> = explicit.iter().cloned().collect();
    let mut has_unresolved_tag = false;
    extend_resolved_tag_symbols(module, tags, &mut names, &mut has_unresolved_tag);
    let has_symbol_args = !tags.is_empty() || !explicit.is_empty();
    if has_symbol_args && has_unresolved_tag && names.is_empty() {
        return;
    }
    if has_symbol_args {
        map.entry(module.to_string()).or_default().extend(names);
    } else {
        map.entry(module.to_string()).or_default();
    }
}

fn extend_resolved_tag_symbols(
    module: &str,
    tags: &[String],
    names: &mut HashSet<String>,
    has_unresolved_tag: &mut bool,
) {
    for tag in tags {
        let tag_token = if tag.starts_with(':') { tag.clone() } else { format!(":{tag}") };
        if let Some(expanded) = resolve_known_export_tag(module, &tag_token) {
            names.extend(expanded.iter().map(|name| (*name).to_string()));
        } else {
            *has_unresolved_tag = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeImportAuthority {
    pub end: usize,
    pub module: String,
    pub symbols: HashSet<String>,
}

/// Recover exact runtime import facts without making them file-wide authority.
pub(super) fn extract_runtime_import_authority(ast: &Node) -> Vec<RuntimeImportAuthority> {
    let specs = ImportExtractor::extract(ast, FileId(0));
    let mut authorities = Vec::new();
    collect_runtime_import_authority(ast, &specs, &mut authorities);
    authorities
}

fn collect_runtime_import_authority(
    node: &Node,
    specs: &[ImportSpec],
    authorities: &mut Vec<RuntimeImportAuthority>,
) {
    use perl_parser_core::ast::NodeKind;
    use perl_semantic_facts::ImportKind;

    let statements: &[Node] = match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => statements,
        NodeKind::Package { block: Some(block), .. } => match &block.kind {
            NodeKind::Block { statements } => statements,
            _ => &[],
        },
        _ => &[],
    };

    for (index, statement) in statements.iter().enumerate() {
        let Some(next) = statements.get(index + 1) else { continue };
        let Some(spec) = specs.iter().find(|spec| {
            spec.kind == ImportKind::RequireThenImport
                && spec.span_start_byte == Some(statement.location.start as u32)
        }) else {
            continue;
        };
        let expression = unwrap_expression_statement(next);
        let NodeKind::MethodCall { object, method, .. } = &expression.kind else { continue };
        if method != "import"
            || !matches!(&object.kind, NodeKind::Identifier { name } if name == &spec.module)
        {
            continue;
        }
        let ImportSymbols::Explicit(symbols) = &spec.symbols else { continue };
        if !authorities
            .iter()
            .any(|authority| authority.end == next.location.end && authority.module == spec.module)
        {
            authorities.push(RuntimeImportAuthority {
                end: next.location.end,
                module: spec.module.clone(),
                symbols: symbols.iter().cloned().collect(),
            });
        }
    }

    for child in node.children() {
        collect_runtime_import_authority(child, specs, authorities);
    }
}

fn unwrap_expression_statement(node: &Node) -> &Node {
    use perl_parser_core::ast::NodeKind;
    match &node.kind {
        NodeKind::ExpressionStatement { expression } => expression,
        _ => node,
    }
}

pub(super) use used_modules::collect_used_module_names;

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_test_must::{must, must_some};

    /// Regression test for #1700: an unresolvable export tag must not silently
    /// discard the explicit symbols collected alongside it.
    #[test]
    fn unresolved_export_tag_keeps_explicit_symbols() {
        let code = "use Module::Thing qw(:unknown_tag known_sub);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Module::Thing"));
        assert!(
            symbols.contains("known_sub"),
            "explicit symbol `known_sub` must survive an unresolved export tag; got: {symbols:?}"
        );
    }

    /// Verify that a purely resolved import still works correctly after the change.
    #[test]
    fn resolved_only_import_unaffected() {
        let code = "use Foo::Bar qw(alpha beta);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Foo::Bar"));
        assert!(symbols.contains("alpha"), "alpha must be present; got: {symbols:?}");
        assert!(symbols.contains("beta"), "beta must be present; got: {symbols:?}");
    }

    /// A configuration hash is not an import list. These symbols become the
    /// filter deciding which completions a module may offer, so reading the
    /// hash body would hide every real export behind its keys and values.
    #[test]
    fn trailing_configuration_hash_contributes_no_import_filter_symbols() {
        let code = "use Another::Module 'param1', 'param2', {key => 'value'};\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Another::Module"));
        assert!(symbols.contains("param1"), "param1 is requested; got: {symbols:?}");
        assert!(symbols.contains("param2"), "param2 is requested; got: {symbols:?}");
        assert!(!symbols.contains("key"), "a hash key is not imported; got: {symbols:?}");
        assert!(!symbols.contains("value"), "a hash value is not imported; got: {symbols:?}");
    }

    /// `foo => { -as => 'bar' }` installs `bar`. Skipping the option hash would
    /// leave the filter allowing `foo` — which is not installed — and blocking
    /// `bar`, which is, hiding a real symbol from completion.
    #[test]
    fn a_per_symbol_option_hash_keeps_the_installed_name_in_the_filter() {
        let code = "use Module foo => { -as => 'bar' };\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Module"));
        assert!(symbols.contains("bar"), "the installed name must survive; got: {symbols:?}");
        assert!(!symbols.contains("as"), "an option keyword is not a symbol; got: {symbols:?}");
        assert!(!symbols.contains("foo"), "a renamed name is not installed; got: {symbols:?}");
    }

    /// An affix option carries a fragment, not a name: `ok => { -postfix => '_ok' }`
    /// installs `ok_ok`, which the Test2 provider composes. Publishing `_ok` into
    /// the completion filter would name nothing, so affix hashes are skipped.
    #[test]
    fn an_affix_rename_hash_publishes_no_fragment_into_the_filter() {
        let code = "use Test2::V0 ok => { -postfix => '_ok' };\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Test2::V0"));
        assert!(!symbols.contains("_ok"), "a fragment is not a symbol; got: {symbols:?}");
        assert!(!symbols.contains("postfix"), "an option key is not a symbol; got: {symbols:?}");
    }

    /// A dashed first key alone does not mark a hash as per-symbol options; an
    /// ordinary module configuration may open with one. Only the documented
    /// option names retain the body.
    #[test]
    fn a_configuration_hash_opening_with_a_dashed_key_is_still_skipped() {
        let code = "use Module 'foo', { -config => 'value' };\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        let symbols = must_some(map.get("Module"));
        assert!(symbols.contains("foo"), "the requested name survives; got: {symbols:?}");
        assert!(!symbols.contains("config"), "a config key is not imported; got: {symbols:?}");
        assert!(!symbols.contains("value"), "a config value is not imported; got: {symbols:?}");
    }

    #[test]
    fn a_setup_hash_contributes_no_import_filter_symbols() {
        let code = "use Sub::Exporter -setup => { exports => [qw(foo bar)] };\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map_with_source(&ast, Some(code));

        if let Some(symbols) = map.get("Sub::Exporter") {
            for leaked in ["exports", "foo", "bar"] {
                assert!(
                    !symbols.contains(leaked),
                    "setup configuration leaked into the import filter: {symbols:?}"
                );
            }
        }
    }

    #[test]
    fn non_importable_module_with_args_is_skipped() {
        let code = "use strict qw(vars refs);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        assert!(
            !map.contains_key("strict"),
            "pragma imports should stay out of ImportMap even with args; got: {map:?}"
        );
    }

    #[test]
    fn importable_module_without_args_is_skipped() {
        let code = "use Module::Thing;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        assert!(
            !map.contains_key("Module::Thing"),
            "use statements without import args should not create ImportMap entries; got: {map:?}"
        );
    }

    #[test]
    fn unresolved_export_tag_without_explicit_symbols_remains_unknown() {
        let code = "use Module::Thing qw(:unknown_tag);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        assert!(
            !map.contains_key("Module::Thing"),
            "unresolved-only tag imports should remain unknown instead of becoming an empty-set import; got: {map:?}"
        );
    }

    #[test]
    fn empty_qw_import_records_empty_entry() {
        let code = "use Module::Thing qw();\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        let symbols = must_some(map.get("Module::Thing"));
        assert!(
            symbols.is_empty(),
            "actually empty import lists should still record an empty entry; got: {symbols:?}"
        );
    }

    #[test]
    fn explicit_use_import_records_explicit_symbols() {
        let code = "use Foo qw(bar);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        let symbols = must_some(map.get("Foo"));
        assert_eq!(symbols, &HashSet::from(["bar".to_string()]));
    }

    #[test]
    fn runtime_import_is_not_file_wide_import_map_authority() {
        let code = "require Foo; Foo->import(qw(bar));\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        assert!(
            !map.contains_key("Foo"),
            "runtime imports must not enter the file-wide map: {map:?}"
        );
        assert!(!collect_used_module_names(&ast).contains("Foo"));
    }

    #[test]
    fn runtime_import_authority_starts_after_the_import_call() {
        let code = "require Foo; Foo->import(qw(bar));\nbar";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let authorities = extract_runtime_import_authority(&ast);

        assert_eq!(authorities.len(), 1);
        assert_eq!(authorities[0].module, "Foo");
        assert_eq!(authorities[0].symbols, HashSet::from(["bar".to_string()]));
        assert!(authorities[0].end <= must_some(code.find("\nbar")));
    }

    #[test]
    fn require_only_has_no_runtime_import_authority() {
        let code = "require Foo;\nbar";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        assert!(extract_runtime_import_authority(&ast).is_empty());
    }

    /// Multiple explicit symbols alongside an unresolved tag — all explicit symbols
    /// must survive; unresolvable tag symbols are silently omitted (acceptable partial miss).
    #[test]
    fn multiple_explicit_symbols_with_unresolved_tag() {
        let code = "use My::Util qw(:nonexistent_tag foo bar baz);\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let map = extract_import_map(&ast);

        let symbols = must_some(map.get("My::Util"));
        assert!(symbols.contains("foo"), "foo must survive; got: {symbols:?}");
        assert!(symbols.contains("bar"), "bar must survive; got: {symbols:?}");
        assert!(symbols.contains("baz"), "baz must survive; got: {symbols:?}");
    }

    #[test]
    fn recurrence_guard_rejects_completion_local_use_arg_classifier() {
        let src = include_str!("import_map.rs");
        let production = src.split("#[cfg(test)]").next().expect("production import_map source");
        assert!(
            !production.contains("fn collect_use_import("),
            "completion must not keep a provider-local use argument classifier"
        );
        assert!(
            !production.contains("arguments_outside_configuration_hashes"),
            "completion must not reparse flattened directive arguments for import semantics"
        );
        assert!(
            production.contains("extract_import_specs_from_hir"),
            "completion import map must consume canonical HIR import facts"
        );
        assert!(
            production.contains("lower_ast"),
            "import map must route through the HIR compile-environment projection"
        );
    }
}
