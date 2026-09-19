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
use perl_semantic_facts::framework_adapters::mojolicious::MOJOLICIOUS_LITE_MODULE;
use perl_semantic_facts::framework_adapters::mojolicious::{
    MojoliciousLiteImportEvidence, MojoliciousLiteVersionRequirement, MojoliciousSiteAnchor,
    mojolicious_lite_import_evidence,
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
/// `source` is the text the AST was parsed from; it distinguishes
/// `use Mojolicious::Lite;` from `use Mojolicious::Lite ();`, which the parser
/// reports with the same empty argument vector but which Perl treats
/// differently. `generation` is the current source generation of that file and
/// is retained on every site.
#[must_use]
pub fn extract_mojolicious_lite_activation_sites(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MojoliciousLiteActivationSite> {
    let mut sites = Vec::new();
    // An unqualified file's caller package is `main` in Perl; it is the
    // default activation scope for script-style Lite apps, which are the
    // overwhelmingly common shape.
    let mut current_package: Option<String> = Some("main".to_string());
    walk_activation_sites(ast, source, file_id, generation, &mut current_package, &mut sites);
    sites
}

/// Whether one `use` statement carries an explicit empty import list.
///
/// `use Mojolicious::Lite ();` calls no `import`, so the Lite DSL is never
/// installed. The parser reports it with the same empty argument vector as the
/// bare import, so the distinction is recovered from the statement's own source
/// interval. An unlocatable interval never fabricates suppression.
fn has_explicit_empty_import(source: &str, span_start: u32, span_end: u32) -> bool {
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    if start >= end {
        return false;
    }
    let Some(statement) = source.get(start..end) else {
        return false;
    };
    let Some(module_at) = statement.find(MOJOLICIOUS_LITE_MODULE) else {
        return false;
    };
    let rest = skip_layout(skip_version_token(skip_layout(
        &statement[module_at + MOJOLICIOUS_LITE_MODULE.len()..],
    )));
    // Perl's explicit empty import list may carry whitespace, newlines, or
    // comments between the parentheses; `( )`, a multiline `(\n)`, and
    // `( # why\n)` all suppress `import` exactly as `()` does.
    let Some(after_open) = rest.strip_prefix('(') else {
        return false;
    };
    skip_layout(after_open).starts_with(')')
}

/// Skip Perl layout — whitespace and line comments — which may appear
/// anywhere an empty import list is spelled out.
///
/// Only layout is skipped. A `#` that opens a comment is indistinguishable
/// here from one inside a quoted argument, but a quoted argument is not an
/// empty list: the scan then fails to find the closing parenthesis and
/// reports no suppression, which is the fail-closed direction.
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
/// v-string spelling, so `use Mojolicious::Lite 9.34 ();` is recognised as a
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

/// The version requirement a `use Mojolicious::Lite ...;` module string
/// carries, when the parser folded a numeric one into the module name.
fn module_version_requirement(module: &str) -> Option<&str> {
    module
        .strip_prefix(MOJOLICIOUS_LITE_MODULE)?
        .strip_prefix(' ')
        .map(str::trim)
        .filter(|rest| !rest.is_empty())
}

/// The complete contiguous version requirement the statement actually spells,
/// read from its own source interval.
///
/// The parser folds only the requirement's first numeric component into the
/// module name, so `use Mojolicious::Lite 9.34.1;` and
/// `use Mojolicious::Lite 9.34 .1;` are indistinguishable by token. Reading
/// the contiguous run of version characters from the source separates them:
/// the first spells `9.34.1`, the second only `9.34`, leaving `.1` an ordinary
/// import argument. An unlocatable interval yields `None`, which consumes no
/// continuation at all rather than fabricating one.
fn spelled_version_requirement(source: &str, span_start: u32, span_end: u32) -> Option<String> {
    let start = span_start as usize;
    let end = (span_end as usize).min(source.len());
    if start >= end {
        return None;
    }
    let statement = source.get(start..end)?;
    let module_at = statement.find(MOJOLICIOUS_LITE_MODULE)?;
    let rest = statement[module_at + MOJOLICIOUS_LITE_MODULE.len()..].trim_start();
    let body = rest.strip_prefix('v').unwrap_or(rest);
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let taken = body.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_').count();
    Some(rest[..(rest.len() - body.len()) + taken].to_string())
}

/// The version requirement one activation site carries, joining the prefix the
/// parser folded into the module name with the statement as actually spelled.
fn site_version_requirement(
    module: &str,
    source: &str,
    span_start: u32,
    span_end: u32,
) -> MojoliciousLiteVersionRequirement {
    MojoliciousLiteVersionRequirement::new(
        module_version_requirement(module).map(str::to_string),
        spelled_version_requirement(source, span_start, span_end),
    )
}

fn walk_activation_sites(
    node: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
    current_package: &mut Option<String>,
    sites: &mut Vec<MojoliciousLiteActivationSite>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } if is_exact_mojolicious_lite_import(module) => {
            let span_start = node.location.start().min(u32::MAX as usize) as u32;
            let span_end = node.location.end().min(u32::MAX as usize) as u32;
            sites.push(MojoliciousLiteActivationSite {
                file_id,
                anchor_id: AnchorId(node.location.start() as u64),
                anchor: MojoliciousSiteAnchor::new(
                    current_package.clone(),
                    span_start,
                    span_end,
                    generation.clone(),
                ),
                evidence: mojolicious_lite_import_evidence(
                    args,
                    has_explicit_empty_import(source, span_start, span_end),
                    &site_version_requirement(module, source, span_start, span_end),
                ),
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
    sites: &mut Vec<MojoliciousLiteActivationSite>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_semantic_facts::framework_adapters::mojolicious::MojoliciousLiteImportSelection;
    use perl_tdd_support::must;

    fn sites(code: &str) -> Vec<MojoliciousLiteActivationSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_mojolicious_lite_activation_sites(
            &ast,
            code,
            FileId(1),
            SourceGeneration::known("gen-1"),
        )
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
    }

    #[test]
    fn an_explicit_empty_import_list_is_recorded_as_suppressed() {
        // `use Mojolicious::Lite ();` loads the module but calls no `import`,
        // so the Lite DSL is never installed. The parser reports the same
        // empty argument vector as the bare import, so the distinction comes
        // from the statement's own source interval.
        let found = sites("use Mojolicious::Lite ();\n");
        assert_eq!(found.len(), 1, "the site is still observed; only the role is refused");
        assert_eq!(found[0].evidence.selection, MojoliciousLiteImportSelection::ImportSuppressed);
    }

    #[test]
    fn a_bare_import_beside_the_suppressed_form_stays_default() {
        // Negative control: the ordinary import must not be swept up by the
        // suppression check.
        let found = sites("use Mojolicious::Lite;\n");
        assert_eq!(found[0].evidence.selection, MojoliciousLiteImportSelection::Default);
    }

    #[test]
    fn a_vstring_version_import_still_activates() {
        // The parser puts a v-string in the argument list rather than folding
        // it into the module name, unlike the numeric spelling.
        let found = sites("use Mojolicious::Lite v9.34;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].evidence.selection, MojoliciousLiteImportSelection::Default);
        assert!(found[0].evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn a_hash_import_argument_is_a_dynamic_boundary() {
        let found = sites("use Mojolicious::Lite %options;\n");
        assert_eq!(found.len(), 1);
        assert!(matches!(
            found[0].evidence.selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
    }

    #[test]
    fn a_versioned_empty_import_is_still_suppressed() {
        // `use Mojolicious::Lite 9.34 ();` — the version must not hide the
        // explicit empty import list.
        for code in ["use Mojolicious::Lite 9.34 ();\n", "use Mojolicious::Lite v9.34 ();\n"] {
            let found = sites(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(
                found[0].evidence.selection,
                MojoliciousLiteImportSelection::ImportSuppressed,
                "{code} suppresses import"
            );
        }
    }

    #[test]
    fn a_versioned_import_records_its_source_requirement() {
        // Numeric requirements are folded into the module name; v-strings
        // arrive in the argument list. Both must be captured.
        assert_eq!(
            sites("use Mojolicious::Lite 9.34;\n")[0].evidence.source_version_requirement,
            Some("9.34".to_string())
        );
        assert_eq!(
            sites("use Mojolicious::Lite v9.34;\n")[0].evidence.source_version_requirement,
            Some("9.34".to_string())
        );
        assert_eq!(sites("use Mojolicious::Lite;\n")[0].evidence.source_version_requirement, None);
    }

    #[test]
    fn a_spaced_or_multiline_empty_import_list_is_still_suppressed() {
        // Perl's explicit empty import list may carry whitespace between the
        // parentheses; all of these suppress `import`.
        for code in [
            "use Mojolicious::Lite ();\n",
            "use Mojolicious::Lite ( );\n",
            "use Mojolicious::Lite (\n);\n",
            "use Mojolicious::Lite 9.34 ( );\n",
        ] {
            let found = sites(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(
                found[0].evidence.selection,
                MojoliciousLiteImportSelection::ImportSuppressed,
                "{code} suppresses import"
            );
        }
    }

    #[test]
    fn a_non_empty_parenthesized_import_list_is_not_suppressed() {
        // Negative control for the whitespace relaxation: a real import list
        // in parentheses must not read as an empty one.
        let found = sites("use Mojolicious::Lite (-signatures);\n");
        assert_eq!(found.len(), 1);
        assert_ne!(
            found[0].evidence.selection,
            MojoliciousLiteImportSelection::ImportSuppressed,
            "a populated list does not suppress import"
        );
    }

    #[test]
    fn a_three_part_version_requirement_is_recorded_whole() {
        let found = sites("use Mojolicious::Lite 9.34.1;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].evidence.source_version_requirement,
            Some("9.34.1".to_string()),
            "the requirement must not be truncated to its first component"
        );
        assert!(found[0].evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn a_block_form_package_scopes_its_own_activation() {
        // `package NAME { ... }` installs its declared package for the block.
        let found = sites("package MyApp {\n    use Mojolicious::Lite;\n}\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].anchor.package.as_deref(), Some("MyApp"));
    }

    #[test]
    fn a_lexical_block_restores_the_enclosing_package_afterwards() {
        // A statement-form `package X;` inside a bare block must not leak past
        // the block's end.
        let found = sites("package Outer;\n{\n    package Inner;\n}\nuse Mojolicious::Lite;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].anchor.package.as_deref(),
            Some("Outer"),
            "the block-local package must not survive the block"
        );
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
