//! Registry-backed Dancer2 2.x activation-site extraction (#13616, L1).
//!
//! Extracts the exact `use Dancer2 ...;` import sites from an AST and turns
//! their argument lists into [`Dancer2TwoXImportEvidence`] for the 2.x
//! adapter in `perl-semantic-facts`. This is the source side of the bounded
//! Dancer2 2.x activation contract:
//!
//! - only an exact `use Dancer2` (optionally `use Dancer2 <version>`) is an
//!   activation site — `use Dancer2::Core`, `use Dancer2::Plugin`, Dancer v1
//!   `use Dancer`, and `use Dancer2::Plugin::Foo` never activate, preserving
//!   the #8910 containment;
//! - an explicit empty import list (`use Dancer2 ();`) calls no `import`, so
//!   the site is retained but its evidence marks the import suppressed (the
//!   parser reports it with the same empty argument vector as the bare
//!   import, so the distinction is recovered from the statement's own source
//!   interval, exactly as the Mojolicious Lite profile does);
//! - the caller package scopes each site, and same-package named package
//!   subroutines are collected so the adapter can apply the upstream
//!   un-overwrite rule: a keyword whose name is already owned by a named sub
//!   in the importing package mints no DSL binding;
//! - computed import options stay explicit dynamic boundaries instead of
//!   silently becoming defaults.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2_two_x::{
    Dancer2TwoXImportEvidence, parse_dancer2_two_x_import_args,
};
use perl_semantic_facts::{AnchorId, FileId};

/// One exact `use Dancer2 ...;` activation site in a source file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2TwoXActivationSite {
    /// Caller package at the activating import (application identity scope).
    pub package: Option<String>,
    /// File the import appears in.
    pub file_id: FileId,
    /// Deterministic anchor for the activating import statement.
    pub anchor_id: AnchorId,
    /// Start byte of the activating import statement.
    pub span_start_byte: u32,
    /// Parsed import evidence (appname/DSL/exclusions/import semantics).
    pub evidence: Dancer2TwoXImportEvidence,
    /// Registry keyword names already owned by same-package named package
    /// subroutines in this file; the upstream un-overwrite rule mints no DSL
    /// binding for these.
    pub shadowed_keywords: Vec<String>,
}

/// Whether a `use` module string is an exact Dancer2 2.x DSL import.
///
/// `use Dancer2 2.01;` carries the version appended by the parser; nested
/// `Dancer2::*` modules and Dancer v1 are not activation. Mirrors the #8914
/// extractor's predicate so both adapters select identically.
fn is_exact_dancer2_two_x_import(module: &str) -> bool {
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

/// Extract every exact Dancer2 2.x activation site from `ast`, in source
/// order, with per-package shadowed keyword names.
///
/// `source` is the text the AST was parsed from; it distinguishes
/// `use Dancer2;` from `use Dancer2 ();`, which the parser reports with the
/// same empty argument vector but which Perl treats differently.
#[must_use]
pub fn extract_dancer2_two_x_activation_sites(
    ast: &Node,
    source: &str,
    file_id: FileId,
) -> Vec<Dancer2TwoXActivationSite> {
    // Package-scoped named subs own their glob for the whole file at compile
    // time, so the shadow relation is order-independent within the file
    // (upstream checks the glob at import time; a later same-named sub
    // definition overwrites the installed binding in the same glob). Named
    // `my`/`state` subs bind lexically and never occupy the package glob the
    // import checks, so they are not shadowing here; lexical-sub precedence
    // inside their scope is a separate mechanism this profile does not model.
    let mut package_subs: Vec<(String, String)> = Vec::new();
    let mut root_package = Some("main".to_string());
    collect_package_subs(ast, &mut root_package, &mut package_subs);

    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default application identity scope for script-style Dancer2 apps.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(ast, source, file_id, &mut current_package, &package_subs, &mut sites);
    sites
}

/// Whether a named sub declaration occupies its package glob: no scope
/// declarator (plain `sub name`) or an explicit `our sub name`. Lexical
/// `my`/`state` subs do not.
fn occupies_package_glob(declarator: Option<&String>) -> bool {
    match declarator {
        None => true,
        Some(declarator) => declarator == "our",
    }
}

fn collect_package_subs(
    node: &Node,
    current_package: &mut Option<String>,
    package_subs: &mut Vec<(String, String)>,
) {
    match &node.kind {
        NodeKind::Subroutine { name: Some(name), declarator, .. }
            if occupies_package_glob(declarator.as_ref()) =>
        {
            if let Some(package) = current_package.as_deref() {
                package_subs.push((package.to_string(), name.clone()));
            }
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            // `package NAME { ... }` installs its declared package for the
            // block and restores the enclosing one afterwards.
            let mut package_scope = Some(name.clone());
            collect_package_subs(block, &mut package_scope, package_subs);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            // Bare `package X;` switches the package for following statements.
            *current_package = Some(name.clone());
        }
        NodeKind::Program { statements } => {
            // File scope: a bare `package X;` persists for the rest of the
            // file, so the running package is threaded through the loop.
            for statement in statements {
                collect_package_subs(statement, current_package, package_subs);
            }
            return;
        }
        NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations;
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards.
            let mut block_package = current_package.clone();
            for statement in statements {
                collect_package_subs(statement, &mut block_package, package_subs);
            }
            return;
        }
        _ => {}
    }
    for child in node.children() {
        collect_package_subs(child, current_package, package_subs);
    }
}

fn shadowed_for_package(package: Option<&str>, package_subs: &[(String, String)]) -> Vec<String> {
    let Some(package) = package else { return Vec::new() };
    let mut shadowed: Vec<String> = package_subs
        .iter()
        .filter(|(sub_package, _)| sub_package == package)
        .map(|(_, name)| name.clone())
        .collect();
    shadowed.sort_unstable();
    shadowed.dedup();
    shadowed
}

/// Whether one `use` statement carries an explicit empty import list.
///
/// `use Dancer2 ();` calls no `import`, so the DSL is never installed and no
/// application is created. The parser reports it with the same empty argument
/// vector as the bare import, so the distinction is recovered from the
/// statement's own source interval. An unlocatable interval never fabricates
/// suppression.
fn has_explicit_empty_import(source: &str, span_start: u32, span_end: u32) -> bool {
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    if start >= end {
        return false;
    }
    let Some(statement) = source.get(start..end) else {
        return false;
    };
    let Some(module_at) = statement.find("Dancer2") else {
        return false;
    };
    let rest =
        skip_layout(skip_version_token(skip_layout(&statement[module_at + "Dancer2".len()..])));
    // Perl's explicit empty import list may carry whitespace or comments
    // between the parentheses; `( )` and a multiline list suppress `import`
    // exactly as `()` does.
    let Some(after_open) = rest.strip_prefix('(') else {
        return false;
    };
    skip_layout(after_open).starts_with(')')
}

/// Skip Perl layout — whitespace and line comments — which may appear
/// anywhere an empty import list is spelled out.
fn skip_layout(rest: &str) -> &str {
    let mut rest = rest;
    loop {
        rest = rest.trim_start();
        let Some(after_hash) = rest.strip_prefix('#') else {
            return rest;
        };
        // A comment running to the end of the statement encloses no `)`.
        rest = match after_hash.find('\n') {
            Some(newline) => &after_hash[newline + 1..],
            None => "",
        };
    }
}

/// Skip a leading `VERSION` requirement token, in either the numeric or
/// v-string spelling, so `use Dancer2 2.01 ();` is recognised as a
/// suppressed import rather than an ordinary versioned one.
fn skip_version_token(rest: &str) -> &str {
    let body = rest.strip_prefix('v').unwrap_or(rest);
    let taken = body.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_').count();
    if taken == 0 {
        return rest;
    }
    let consumed = (rest.len() - body.len()) + taken;
    &rest[consumed..]
}

#[allow(clippy::too_many_arguments)]
fn walk_activation_sites(
    node: &Node,
    source: &str,
    file_id: FileId,
    current_package: &mut Option<String>,
    package_subs: &[(String, String)],
    sites: &mut Vec<Dancer2TwoXActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, .. } if is_exact_dancer2_two_x_import(module) => {
            let span_start = node.location.start.min(u32::MAX as usize) as u32;
            let span_end = node.location.end.min(u32::MAX as usize) as u32;
            let mut evidence = parse_dancer2_two_x_import_args(node_args(node));
            evidence.import_suppressed = has_explicit_empty_import(source, span_start, span_end);
            sites.push(Dancer2TwoXActivationSite {
                package: current_package.clone(),
                file_id,
                anchor_id: AnchorId(node.location.start as u64),
                span_start_byte: span_start,
                shadowed_keywords: shadowed_for_package(current_package.as_deref(), package_subs),
                evidence,
            });
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, source, file_id, package_subs, sites);
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
                    current_package,
                    package_subs,
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
                    &mut block_package,
                    package_subs,
                    sites,
                );
            }
            return;
        }
        _ => {}
    }
    for child in node.children() {
        walk_activation_sites(child, source, file_id, current_package, package_subs, sites);
    }
}

/// The argument token strings of a `use` node.
fn node_args(node: &Node) -> &[String] {
    match &node.kind {
        NodeKind::Use { args, .. } => args,
        _ => &[],
    }
}

fn walk_package_block(
    block: &Node,
    name: &str,
    source: &str,
    file_id: FileId,
    package_subs: &[(String, String)],
    sites: &mut Vec<Dancer2TwoXActivationSite>,
) {
    if let NodeKind::Block { statements } = &block.kind {
        let mut package_scope = Some(name.to_string());
        for statement in statements {
            walk_activation_sites(
                statement,
                source,
                file_id,
                &mut package_scope,
                package_subs,
                sites,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_semantic_facts::framework_adapters::dancer2::{AppNameSelection, DslSelection};
    use perl_tdd_support::must;

    fn sites(code: &str) -> Vec<Dancer2TwoXActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_two_x_activation_sites(&ast, code, FileId(1))
    }

    #[test]
    fn exact_use_dancer2_is_one_activation_site() {
        let found = sites("package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n");
        assert_eq!(found.len(), 1, "exactly one activation site per import");
        assert_eq!(found[0].package.as_deref(), Some("App"));
        assert!(found[0].shadowed_keywords.is_empty());
    }

    #[test]
    fn dancer2_core_and_plugin_do_not_activate() {
        assert!(
            sites("use Dancer2::Core;\nuse Dancer2::Core::App;\nuse Dancer2::Plugin;\n").is_empty()
        );
    }

    #[test]
    fn dancer_v1_and_plugins_do_not_activate() {
        assert!(sites("use Dancer;\nuse Dancer2::Plugin qw(hook);\n").is_empty());
    }

    #[test]
    fn unqualified_file_defaults_to_main_package() {
        let found = sites("use Dancer2;\nget '/x' => sub { 1 };\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package.as_deref(), Some("main"));
    }

    #[test]
    fn versioned_import_activates() {
        let found = sites("use Dancer2 2.01;\n");
        assert_eq!(found.len(), 1);
        assert!(!found[0].evidence.import_suppressed);
    }

    #[test]
    fn vstring_version_import_activates_without_import_arguments() {
        // The parser reports the v-string in the argument list; perl consumes
        // the VERSION slot before import runs.
        let found = sites("use Dancer2 v2.01;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.version_slot_spellings, vec!["v2.01".to_string()]);
        assert!(!found[0].evidence.odd_argument_count);
    }

    #[test]
    fn explicit_empty_import_is_marked_suppressed() {
        let found = sites("use Dancer2 ();\n");
        assert_eq!(found.len(), 1, "the site is observed; the import is suppressed");
        assert!(found[0].evidence.import_suppressed);
    }

    #[test]
    fn spaced_and_versioned_empty_imports_are_suppressed() {
        for code in [
            "use Dancer2 ( );\n",
            "use Dancer2 (\n);\n",
            "use Dancer2 2.01 ();\n",
            "use Dancer2 v2.01 ();\n",
        ] {
            let found = sites(code);
            assert_eq!(found.len(), 1, "{code}");
            assert!(found[0].evidence.import_suppressed, "{code} suppresses import");
        }
    }

    #[test]
    fn a_populated_import_list_is_not_suppressed() {
        let found = sites("use Dancer2 ('!params');\n");
        assert_eq!(found.len(), 1);
        assert!(!found[0].evidence.import_suppressed);
        assert_eq!(found[0].evidence.excluded_keywords, vec!["params".to_string()]);
    }

    #[test]
    fn package_scoping_is_tracked() {
        let found =
            sites("package App;\nuse Dancer2;\npackage Other;\nuse Dancer2 appname => 'X';\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("App"));
        assert_eq!(found[1].package.as_deref(), Some("Other"));
        assert_eq!(found[1].evidence.appname, Some(AppNameSelection::Literal("X".to_string())));
    }

    #[test]
    fn lexical_block_package_state_is_restored() {
        let found = sites("package Outer; { package Inner; use Dancer2; } use Dancer2;\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("Inner"));
        assert_eq!(found[1].package.as_deref(), Some("Outer"));
    }

    #[test]
    fn block_form_package_scopes_its_own_activation() {
        let found = sites("package MyApp {\n    use Dancer2;\n}\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package.as_deref(), Some("MyApp"));
    }

    #[test]
    fn same_package_named_subs_shadow_their_names() {
        let found = sites("package App;\nsub template { 1 }\nsub helper { 2 }\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].shadowed_keywords, vec!["helper".to_string(), "template".to_string()]);
    }

    #[test]
    fn our_subs_shadow_but_lexical_subs_do_not() {
        let found = sites("package App;\nour sub set { 1 }\nmy sub get { 2 }\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].shadowed_keywords, vec!["set".to_string()]);
    }

    #[test]
    fn shadowing_is_order_independent_and_package_scoped() {
        // A sub defined after the import replaces the installed binding in
        // the same glob, so both orders shadow.
        let found = sites(
            "package App;\nuse Dancer2;\nsub params { 1 }\npackage Other;\nsub engine { 2 }\nuse Dancer2;\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].shadowed_keywords, vec!["params".to_string()]);
        assert_eq!(found[1].shadowed_keywords, vec!["engine".to_string()]);
    }

    #[test]
    fn other_packages_subs_do_not_shadow() {
        let found = sites("package Lib;\nsub get { 1 }\npackage App;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].shadowed_keywords.is_empty());
    }

    #[test]
    fn exclusions_and_dsl_selection_reach_the_evidence() {
        let found = sites("package App;\nuse Dancer2 '!params';\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.excluded_keywords, vec!["params".to_string()]);

        let found = sites("package App;\nuse Dancer2 dsl => 'My::DSL';\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.dsl, Some(DslSelection::CustomLiteral("My::DSL".to_string())));
    }

    #[test]
    fn noop_tags_and_nopragmas_reach_the_evidence() {
        let found = sites("use Dancer2 ':script' ':nopragmas';\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.no_op_tags, vec![":script".to_string()]);
        assert!(found[0].evidence.nopragmas);
    }
}
