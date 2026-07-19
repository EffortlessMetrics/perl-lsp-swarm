//! #4504 — dynamic braced glob dereference assignment must stay a dynamic boundary.
//!
//! `*{$name} = \&foo;` captures the destination glob's symbol from a *runtime*
//! expression, so the HIR stash graph cannot know which symbol is being aliased at
//! parse time. It must therefore record a dynamic-stash boundary (the same way
//! `*dynamic = $target;` already does for a dynamic RHS) rather than promoting the
//! assignment to an `ExactAst` `TypeglobAlias` slot named after the literal capture
//! text (e.g. the symbol `"$name"`). Acceptance criterion #7: "keep unresolved
//! runtime behavior an explicit dynamic boundary."

use perl_parser_core::Parser;
use perl_parser_core::hir::{GlobSlotSource, HirFile, StashGraph, lower_ast};

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// `package::name target=...` for every `TypeglobAlias`-sourced stash slot.
fn typeglob_alias_slots(graph: &StashGraph) -> Vec<String> {
    let mut slots = Vec::new();
    for package in &graph.packages {
        for slot in &package.slots {
            if matches!(slot.source, GlobSlotSource::TypeglobAlias) {
                slots.push(format!(
                    "{}::{} target={}",
                    package.package,
                    slot.name,
                    slot.alias_target.as_deref().unwrap_or("<none>")
                ));
            }
        }
    }
    slots
}

fn has_typeglob_boundary(graph: &StashGraph) -> bool {
    graph.dynamic_boundaries.iter().any(|boundary| boundary.reason.contains("typeglob"))
}

#[test]
fn dynamic_typeglob_assignment_records_boundary_not_static_alias() {
    // Dynamic LHS + static-looking RHS: the RHS is a resolvable code ref, but the
    // destination symbol is only known at runtime, so no ExactAst alias may be minted.
    let file = lower_source("package Child;\n*{$name} = \\&foo;\n");

    let aliases = typeglob_alias_slots(&file.stash_graph);
    assert!(
        aliases.is_empty(),
        "dynamic-LHS typeglob assignment must not create a static TypeglobAlias slot, got: {aliases:?}"
    );
    assert!(
        has_typeglob_boundary(&file.stash_graph),
        "dynamic-LHS typeglob assignment must record a typeglob dynamic-stash boundary"
    );
}

#[test]
fn split_token_dynamic_typeglob_assignment_records_boundary() {
    // Whitespace-form parity: `* { $name } = ...` must behave like the fused form.
    let file = lower_source("package Child;\n* { $name } = \\&foo;\n");

    let aliases = typeglob_alias_slots(&file.stash_graph);
    assert!(
        aliases.is_empty(),
        "split-token dynamic typeglob assignment must not create a static TypeglobAlias slot, got: {aliases:?}"
    );
    assert!(
        has_typeglob_boundary(&file.stash_graph),
        "split-token dynamic typeglob assignment must record a typeglob dynamic-stash boundary"
    );
}

#[test]
fn static_typeglob_assignment_still_records_exact_alias() {
    // Negative control: a real bareword typeglob alias must keep its ExactAst slot.
    let file = lower_source("package Child;\n*alias = \\&foo;\n");

    let aliases = typeglob_alias_slots(&file.stash_graph);
    assert!(
        aliases.iter().any(|slot| slot == "Child::alias target=foo"),
        "static bareword typeglob alias must stay an ExactAst TypeglobAlias slot, got: {aliases:?}"
    );
}

#[test]
fn qualified_static_typeglob_assignment_still_records_exact_alias() {
    // Negative control: a `::`-qualified bareword name is still static.
    let file = lower_source("*Other::alias = \\&foo;\n");

    let aliases = typeglob_alias_slots(&file.stash_graph);
    assert!(
        aliases.iter().any(|slot| slot == "Other::alias target=foo"),
        "qualified bareword typeglob alias must stay an ExactAst TypeglobAlias slot, got: {aliases:?}"
    );
}

#[test]
fn braced_bareword_typeglob_assignment_still_records_exact_alias() {
    // Negative control: a braced *bareword* target (`*{name}`, `*{ name }`) resolves
    // to a static symbol, so it must stay an ExactAst alias — the dynamic-boundary
    // routing only applies to names that are not bareword-like.
    for source in ["package Child;\n*{name} = \\&foo;\n", "package Child;\n*{ name } = \\&foo;\n"] {
        let file = lower_source(source);
        let aliases = typeglob_alias_slots(&file.stash_graph);
        assert!(
            aliases.iter().any(|slot| slot == "Child::name target=foo"),
            "braced bareword typeglob alias must stay an ExactAst TypeglobAlias slot for {source:?}, got: {aliases:?}"
        );
    }
}

#[test]
fn symbolic_string_typeglob_assignment_records_boundary() {
    // A quoted symbolic name (`*{'STDOUT'}`) is not a resolvable bareword: the target
    // is a symbolic (string) glob, so it stays a dynamic boundary rather than a
    // claimed ExactAst alias — consistent with symbolic deref handling elsewhere.
    let file = lower_source("package Child;\n*{'STDOUT'} = \\&foo;\n");

    let aliases = typeglob_alias_slots(&file.stash_graph);
    assert!(
        aliases.is_empty(),
        "symbolic-string typeglob assignment must not create a static TypeglobAlias slot, got: {aliases:?}"
    );
    assert!(
        has_typeglob_boundary(&file.stash_graph),
        "symbolic-string typeglob assignment must record a typeglob dynamic-stash boundary"
    );
}
