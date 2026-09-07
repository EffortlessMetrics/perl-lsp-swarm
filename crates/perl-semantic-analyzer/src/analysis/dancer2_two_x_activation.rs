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
    parse_parenthesized_dancer2_two_x_import_args,
};
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};

/// One exact `use Dancer2 ...;` activation site in a source file.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2TwoXActivationSite {
    /// Source generation the site was extracted from; exact activation
    /// requires the detection generation to match it.
    pub source_generation: SourceGeneration,
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
    generation: SourceGeneration,
) -> Vec<Dancer2TwoXActivationSite> {
    // Package-scoped named subs own their glob for the whole file at compile
    // time, so the shadow relation is order-independent within the file
    // (upstream checks the glob at import time; a later same-named sub
    // definition overwrites the installed binding in the same glob). Named
    // `my`/`state` subs bind lexically and never occupy the package glob the
    // import checks, so they are not shadowing here; lexical-sub precedence
    // inside their scope is a separate mechanism this profile does not model.
    let mut package_subs: Vec<PackageSub> = Vec::new();
    let mut root_package = Some("main".to_string());
    collect_package_subs(ast, &mut root_package, &mut package_subs);

    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default application identity scope for script-style Dancer2 apps.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(
        ast,
        source,
        file_id,
        generation.clone(),
        &mut current_package,
        &package_subs,
        &mut sites,
    );
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

/// Core modules that alter compilation or the environment without
/// installing functions into the caller's package, so their arguments never
/// shadow a same-named import (`use lib qw(get)` takes a library path, not
/// a function list; #14408 review).
const NON_IMPORTING_CORE_MODULES: [&str; 7] =
    ["lib", "strict", "warnings", "utf8", "feature", "experimental", "mro"];

/// One package-glob entry that can shadow a DSL keyword: a sub definition
/// (`declaration_offset: None` — it owns the glob regardless of position,
/// because it replaces whatever the import installed) or a forward
/// declaration (`Some(offset)` — its stub only blocks the import when it
/// compiled before the `use`; against an already-installed CV it is a
/// no-op).
type PackageSub = (String, String, Option<u32>);

fn collect_package_subs(
    node: &Node,
    current_package: &mut Option<String>,
    package_subs: &mut Vec<PackageSub>,
) {
    match &node.kind {
        NodeKind::Subroutine { name: Some(name), declarator, body, .. }
            if occupies_package_glob(declarator.as_ref()) =>
        {
            // A forward declaration (`sub get;`) installs a stub CV into the
            // package glob at compile time. Upstream fetches
            // `*{"${caller}::${export}"}{CODE}` and `next if defined
            // $existing` — `defined` on the fetched CV REFERENCE is true for
            // a stub — so a declaration compiled BEFORE the `use` blocks the
            // keyword exactly like a definition (#14408 review; pinned
            // upstream Role/DSL.pm @ 674837c). A declaration AFTER the `use`
            // is a no-op against the already-installed CV, so only
            // definitions shadow position-independently. Lexical subs stay
            // excluded via the declarator gate above.
            let body_span_width = body.location.end.saturating_sub(body.location.start);
            let is_forward_declaration =
                body.location.start == body.location.end || body_span_width < 2;
            let declaration_offset =
                if is_forward_declaration { Some(body.location.start as u32) } else { None };
            if name.contains("::") {
                // A qualified name belongs to its declared package, however
                // the running package scope is spelled: `sub App::get` inside
                // package Main still defines App::get, and a leading `::`
                // addresses main's namespace.
                if let Some((owner, leaf)) = split_qualified_sub(name) {
                    package_subs.push((owner, leaf, declaration_offset));
                }
            } else if let Some(package) = current_package.as_deref() {
                package_subs.push((package.to_string(), name.clone(), declaration_offset));
            }
        }
        NodeKind::Use { module, args, .. }
            if !is_exact_dancer2_two_x_import(module)
                && !NON_IMPORTING_CORE_MODULES.contains(&module.as_str()) =>
        {
            // Functions imported from other modules occupy this package's
            // globs before Dancer2 runs: its un-overwrite rule preserves
            // them, so the same-named DSL keyword is never installed
            // (#14408 review). Only plain words count; tags and `!`
            // exclusions are this module's own import vocabulary.
            for arg in args {
                let words = perl_semantic_facts::framework_adapters::dancer2_two_x::qw_words(arg)
                    .unwrap_or_else(|| {
                        arg.split_whitespace()
                            .map(|word| word.trim_matches('\'').trim_matches('"').to_string())
                            .collect()
                    });
                for text in words {
                    if text.is_empty()
                        || text.starts_with('!')
                        || text.starts_with(':')
                        || text.contains('(')
                    {
                        continue;
                    }
                    if let Some(package) = current_package.as_deref() {
                        package_subs.push((package.to_string(), text, None));
                    }
                }
            }
        }
        NodeKind::Assignment { lhs, .. } => {
            // Only a typeglob ASSIGNMENT installs a glob entry: a bare
            // `*get{CODE}` read or `\*get` reference defines no function and
            // must not shadow a same-named import (#14408 review).
            if let NodeKind::Typeglob { name } = &lhs.kind {
                record_glob_shadow(name, current_package, package_subs);
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

/// Record a typeglob assignment (`*get = ...`) as occupying a glob: a bare
/// leaf lands in the running package; a qualified name (`*App::get`,
/// `*::get`) belongs to its declared package, however the running scope is
/// spelled (#14408 review).
fn record_glob_shadow(
    name: &str,
    current_package: &Option<String>,
    package_subs: &mut Vec<PackageSub>,
) {
    if name.contains("::") {
        if let Some((owner, leaf)) = split_qualified_sub(name) {
            package_subs.push((owner, leaf, None));
        }
    } else if !name.is_empty() {
        if let Some(package) = current_package.as_deref() {
            package_subs.push((package.to_string(), name.to_string(), None));
        }
    }
}

/// True when the import arguments are spelled as a parenthesized list
/// (`use Dancer2 (...)`). Perl consumes a bare `use MODULE VERSION`
/// requirement only outside parentheses: a v-string inside the list is an
/// import argument and must not claim the version slot (#14408 review).
fn has_parenthesized_import_list(source: &str, span_start: u32, span_end: u32) -> bool {
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    let Some(statement) = source.get(start..end) else {
        return false;
    };
    let Some(after_use) = skip_layout(statement).strip_prefix("use") else {
        return false;
    };
    let module = skip_layout(after_use);
    // Scan past the module spelling (letters, `::`, underscores, and any
    // version components the parser folded into the module name).
    let after_module = match module.find(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_')) {
        Some(index) => &module[index..],
        None => return false,
    };
    // A spelled bare version (`use Dancer2 2.0 (...)`) occupies the
    // requirement slot; the parenthesized list that follows is still an
    // import list whose contents never claim the version slot (#14408
    // review).
    let after_version = skip_version_token(skip_layout(after_module));
    skip_layout(after_version).starts_with('(')
}

/// Split a fully-qualified subroutine name into its owning package and the
/// final keyword. `App::get` declares `App`'s `get`; `::App::get` and
/// `main::App::get` both address `App` inside main's root namespace; a bare
/// `::get` is `main::get`.
fn split_qualified_sub(name: &str) -> Option<(String, String)> {
    let without_leading = name.strip_prefix("::").unwrap_or(name);
    match without_leading.rsplit_once("::") {
        Some((package, leaf)) if !package.is_empty() => {
            Some((package.to_string(), leaf.to_string()))
        }
        Some(_) => Some(("main".to_string(), without_leading.to_string())),
        None => Some(("main".to_string(), without_leading.to_string())),
    }
}

fn shadowed_for_package(
    package: Option<&str>,
    package_subs: &[PackageSub],
    site_offset: u32,
) -> Vec<String> {
    let Some(package) = package else { return Vec::new() };
    let mut shadowed: Vec<String> = package_subs
        .iter()
        .filter(|(sub_package, _, _)| sub_package == package)
        .filter(|(_, _, declaration_offset)| match declaration_offset {
            None => true,
            Some(offset) => *offset < site_offset,
        })
        .map(|(_, name, _)| name.clone())
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
    // The module name follows the `use` keyword directly (comments and
    // whitespace between them are layout). Searching the raw statement for
    // "Dancer2" could match the name inside a comment that precedes the
    // module — e.g. `use # Dancer2 comment\n Dancer2 ();` — so the keyword is
    // consumed first and the module located in what remains.
    let Some(after_use_keyword) = skip_layout(statement).strip_prefix("use") else {
        return false;
    };
    let Some(rest) = skip_layout(after_use_keyword).strip_prefix("Dancer2") else {
        return false;
    };
    let rest = skip_layout(skip_version_token(skip_layout(rest)));
    // Perl's explicit empty import list may carry whitespace or comments
    // between the parentheses; `( )` and a multiline list suppress `import`
    // exactly as `()` does.
    let Some(after_open) = rest.strip_prefix('(') else {
        return false;
    };
    skip_layout(after_open).starts_with(')')
}

/// The complete contiguous version requirement the statement actually
/// spells, read from its own source interval.
///
/// The parser folds only the requirement's leading numeric components into
/// the module name, so `use Dancer2 2.0.1;` arrives as module `Dancer2 2.0`
/// plus the separate tokens `.` and `1`. Reading the contiguous run of
/// version characters from the source recovers the whole requirement; an
/// unlocatable interval yields `None`, which consumes no continuation.
fn spelled_version_requirement(source: &str, span_start: u32, span_end: u32) -> Option<String> {
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    if start >= end {
        return None;
    }
    let statement = source.get(start..end)?;
    let after_use_keyword = skip_layout(statement).strip_prefix("use")?;
    let rest = skip_layout(after_use_keyword).strip_prefix("Dancer2")?;
    let rest = skip_layout(rest);
    let body = rest.strip_prefix('v').unwrap_or(rest);
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let taken = body.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_').count();
    Some(rest[..(rest.len() - body.len()) + taken].to_string())
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
    generation: SourceGeneration,
    current_package: &mut Option<String>,
    package_subs: &[PackageSub],
    sites: &mut Vec<Dancer2TwoXActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, .. } if is_exact_dancer2_two_x_import(module) => {
            let span_start = node.location.start.min(u32::MAX as usize) as u32;
            let span_end = node.location.end.min(u32::MAX as usize) as u32;
            // The parser folds the version's leading components into the
            // module name; the spelled requirement's remaining components
            // arrive as separate leading tokens and are consumed here, so a
            // contiguous three-part version does not degrade into unmodeled
            // options. A whitespace-separated component is a genuine import
            // argument and stays.
            let spelled = spelled_version_requirement(source, span_start, span_end);
            let mut node_args: Vec<String> = node_args(node).to_vec();
            if let Some(requirement) = &spelled {
                let folded = module.split_once(' ').map(|(_, version)| version).unwrap_or("");
                if let Some(continuation) = requirement.strip_prefix(folded) {
                    let mut joined = String::new();
                    let mut keep = 0usize;
                    for arg in node_args.iter() {
                        let candidate = format!("{joined}{arg}");
                        if candidate.len() <= continuation.len()
                            && continuation.starts_with(candidate.as_str())
                        {
                            joined = candidate;
                            keep += 1;
                        } else {
                            break;
                        }
                    }
                    if joined == continuation {
                        node_args = node_args[keep..].to_vec();
                    }
                }
            }
            // A parenthesized argument list delivers its contents as plain
            // import arguments: a leading v-string there never claims the
            // `use MODULE VERSION` slot (#14408 review).
            let parenthesized = has_parenthesized_import_list(source, span_start, span_end);
            let mut evidence = if parenthesized {
                parse_parenthesized_dancer2_two_x_import_args(&node_args)
            } else {
                parse_dancer2_two_x_import_args(&node_args)
            };
            evidence.import_suppressed = has_explicit_empty_import(source, span_start, span_end);
            if let Some(requirement) = spelled {
                evidence.version_slot_spellings = vec![requirement];
            }
            sites.push(Dancer2TwoXActivationSite {
                package: current_package.clone(),
                source_generation: generation.clone(),
                file_id,
                anchor_id: AnchorId(node.location.start as u64),
                span_start_byte: span_start,
                shadowed_keywords: shadowed_for_package(
                    current_package.as_deref(),
                    package_subs,
                    span_start,
                ),
                evidence,
            });
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            walk_package_block(block, name, source, file_id, generation, package_subs, sites);
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
                    generation.clone(),
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
        walk_activation_sites(
            child,
            source,
            file_id,
            generation.clone(),
            current_package,
            package_subs,
            sites,
        );
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
    generation: SourceGeneration,
    package_subs: &[PackageSub],
    sites: &mut Vec<Dancer2TwoXActivationSite>,
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
        sites_with_generation(code, SourceGeneration::known("gen-1"))
    }

    fn sites_with_generation(
        code: &str,
        generation: SourceGeneration,
    ) -> Vec<Dancer2TwoXActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_two_x_activation_sites(&ast, code, FileId(1), generation)
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
            assert_eq!(found.len(), 1, "{code}: len={}", found.len());
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

    #[test]
    fn comment_between_use_and_module_does_not_hide_the_empty_import() {
        // The word Dancer2 inside the comment must not be taken for the
        // module name: the `use` keyword is consumed first, layout (comments
        // and whitespace) skipped, and only then is the module located.
        let found = sites("use # Dancer2 comment\n Dancer2 ();\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].evidence.import_suppressed,
            "the explicit empty import must be recognized past the comment"
        );
    }

    #[test]
    fn qualified_sub_outside_the_package_shadows_the_import() {
        // `sub App::get` at file scope declares App's get before the package
        // exists: the qualified name owns the shadow, so the later import
        // must not report `get` as freshly installed.
        let found = sites("sub App::get { }\npackage App;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "the qualified App::get definition must shadow the import, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn qualified_sub_inside_another_package_shadows_across_packages() {
        let found = sites("package Other;\nsub App::get { }\npackage App;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a qualified definition inside another package must still shadow App's import, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn imported_functions_from_other_modules_shadow_the_import() {
        // Functions imported from another module occupy this package's
        // globs before Dancer2 runs; its un-overwrite rule preserves them.
        let found = sites(
            "use Helpers qw(get post);
use Dancer2;
get '/x' => sub { 1 };
",
        );
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "an earlier imported get must shadow the import, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn typeglob_assignment_shadows_the_import() {
        let found = sites(
            r"*get = \&other;
use Dancer2;
",
        );
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a typeglob assignment must shadow the import, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn forward_declaration_after_import_does_not_shadow() {
        // A stub declaration compiled AFTER the `use` is a no-op against the
        // already-installed CV (Perl never replaces a real CV with a stub),
        // so the keyword stays imported. The position matters: see
        // forward_declaration_before_import_shadows.
        let found = sites(
            "use Dancer2;
sub get;
get '/x' => sub { 1 };
",
        );
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a post-import stub declaration never replaces the installed CV, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn forward_declaration_before_import_shadows() {
        // `sub get;` before `use Dancer2` installs a stub CV at compile
        // time. Upstream fetches `*{"${caller}::${export}"}{CODE}` and `next
        // if defined $existing` — `defined` on the CV reference is true for
        // a stub — so the keyword is skipped (pinned upstream Role/DSL.pm @
        // 674837c, #14408 review).
        let found = sites(
            "sub get;
use Dancer2;
get '/x' => sub { 1 };
",
        );
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a pre-import stub occupies the glob and blocks the keyword: {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn versioned_parenthesized_list_stays_an_import_list() {
        // `use Dancer2 2.0 (v3.0)`: the bare `2.0` claims the VERSION slot
        // and `(v3.0)` is the import list, so its lone v-string keeps the
        // pinned odd-arity die instead of hiding as a second version
        // (#14408 review).
        let found = sites("package App;\nuse Dancer2 2.0 (v3.0);\n");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].evidence.version_slot_spellings,
            vec!["2.0".to_string()],
            "the bare spelled requirement claims the VERSION slot"
        );
        assert!(
            found[0].evidence.odd_argument_count,
            "the parenthesized v-string stays an import argument: {:?}",
            found[0].evidence
        );
    }

    #[test]
    fn empty_body_definition_after_import_still_shadows() {
        // `sub get { }` DOES install the glob (empty body is a real
        // definition), so the import must be shadowed.
        let found = sites(
            "use Dancer2;
sub get { }
get '/x' => sub { 1 };
",
        );
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "an empty-body definition must shadow the import, got {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn contiguous_three_part_version_is_consumed_whole() {
        // The parser folds `2.0` into the module and leaves `.` and `1` as
        // separate tokens; the spelled-requirement consumption must join
        // them instead of letting them degrade into unmodeled options.
        let found = sites(
            "use Dancer2 2.0.1;
",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].evidence.version_slot_spellings,
            vec!["2.0.1".to_string()],
            "the contiguous version must be recorded whole"
        );
        assert!(
            found[0].evidence.unmodeled_options.is_empty(),
            "no part of a contiguous version may leak into unmodeled options: {:?}",
            found[0].evidence.unmodeled_options
        );
    }

    #[test]
    fn whitespace_separated_component_is_a_genuine_import_argument() {
        // `use Dancer2 2.0 .1;` spells only `2.0` contiguously: the `.1`
        // is a genuine import argument (odd-arity per the pinned contract),
        // not part of the version.
        let found = sites(
            "use Dancer2 2.0 .1;
",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.version_slot_spellings, vec!["2.0".to_string()]);
        // The version did not swallow the trailing component: `.1` survives
        // as an unmodeled import entry (key `.` with `1` consumed as its
        // paired value per the %pairing model — recording pair VALUES in
        // unmodeled_options is a v2 evidence-model item).
        assert!(
            found[0].evidence.unmodeled_options.contains(&".".to_string()),
            "the whitespace-separated .1 must survive as an unmodeled import entry: {:?}",
            found[0].evidence.unmodeled_options
        );
    }

    #[test]
    fn leading_double_colon_addresses_main_root() {
        // `sub ::get` is main::get: the leading `::` addresses main's root,
        // so the leaf lands on main's shadow list.
        let found = sites("sub ::get { }\npackage App;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "::get belongs to main, not to App's shadow list"
        );
    }

    #[test]
    fn non_importing_core_module_arguments_do_not_shadow() {
        // `use lib qw(get)` takes a library path, not a function list: its
        // argument defines no function and must not hide the `get` keyword
        // (#14408 review).
        let found = sites("package App;\nuse lib qw(get);\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a `use lib` path never occupies a glob: {:?}",
            found[0].shadowed_keywords
        );
        // A genuine function-importing module keeps the shadow.
        let found = sites("package App;\nuse Scalar::Util qw(get);\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a same-named import from another module still shadows"
        );
    }

    #[test]
    fn typeglob_read_does_not_shadow() {
        // Reading a glob (`*get{CODE}`) defines no function; only a typeglob
        // assignment installs a glob entry (#14408 review).
        let found = sites("package App;\nmy $old = *get{CODE};\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a glob read defines no function: {:?}",
            found[0].shadowed_keywords
        );
        // The assignment form keeps the shadow.
        let found = sites("package App;\n*get = \\&impl;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "a typeglob assignment installs the glob and shadows the import"
        );
    }

    #[test]
    fn qualified_typeglob_assignment_lands_on_its_declared_package() {
        // `*App::get = ...` inside package Main installs App::get: Main's
        // own keyword set is untouched (#14408 review).
        let found = sites("package Main;\n*App::get = \\&impl;\nuse Dancer2;\n");
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].shadowed_keywords.iter().any(|k| k == "get"),
            "the qualified glob belongs to App, not Main: {:?}",
            found[0].shadowed_keywords
        );
    }

    #[test]
    fn parenthesized_v_string_is_an_import_argument() {
        // `use Dancer2 (v2.01)` passes the v-string to `import`, where the
        // odd arity dies: the parenthesized list never claims the bare
        // VERSION slot (#14408 review).
        let found = sites("package App;\nuse Dancer2 (v2.01);\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].evidence.version_slot_spellings.is_empty());
        assert!(
            found[0].evidence.odd_argument_count,
            "the unmodeled lone argument keeps the pinned odd-arity die"
        );
        // The bare form keeps the version slot and stays clean.
        let found = sites("package App;\nuse Dancer2 v2.01;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].evidence.version_slot_spellings,
            vec!["v2.01".to_string()],
            "the bare requirement claims the VERSION slot"
        );
        assert!(!found[0].evidence.odd_argument_count);
    }
}
