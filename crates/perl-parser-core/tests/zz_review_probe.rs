//! Throwaway adversarial probe for PR #14530 (Sub::Exporter -setup lowering).
//! Not part of the permanent suite; delete before finishing review.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn dump(label: &str, source: &str) {
    println!("\n=== {label} ===");
    println!("--- source ---\n{source}");
    let file = lower(source);
    println!("--- export_declarations ---");
    for d in &file.stash_graph.export_declarations {
        println!(
            "  package={:?} kind={:?} tag={:?} symbols={:?}",
            d.package, d.kind, d.tag_name, d.symbols
        );
    }
    println!("--- dynamic_boundaries (export-related) ---");
    for b in &file.stash_graph.dynamic_boundaries {
        println!("  package={:?} symbol={:?} kind={:?} reason={:?}", b.package, b.symbol, b.kind, b.reason);
    }
}

#[test]
fn probe_real_group_rename_idiom() {
    // Documented Sub::Exporter::Tutorial idiom:
    // groups => { fauna => [ qw(beef lox), rabbit => { -as => 'coney' } ] }
    dump(
        "real group rename idiom (beef, lox, rabbit->coney)",
        "package Food;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(beef lox rabbit)],\n\
             groups  => { fauna => [ qw(beef lox), rabbit => { -as => 'coney' } ] },\n\
         };\n",
    );
}

#[test]
fn probe_nested_setup_key_before_real_flag() {
    // A hash value containing a key literally named "-setup" BEFORE the real
    // top-level -setup flag appears in the token stream.
    dump(
        "nested -setup key inside another hash, appearing before the real flag",
        "package My::Utils;\n\
         use Sub::Exporter -collector => { -setup => 1 }, -setup => { exports => [qw(a)] };\n",
    );
}

#[test]
fn probe_two_setup_statements_same_package() {
    dump(
        "two separate use Sub::Exporter -setup statements in one package",
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a)], groups => { default => [qw(a)] } };\n\
         use Sub::Exporter -setup => { exports => [qw(b)], groups => { default => [qw(b)] } };\n",
    );
}

#[test]
fn probe_before_any_package_statement() {
    dump(
        "use Sub::Exporter before any package statement",
        "use Sub::Exporter -setup => { exports => [qw(a)] };\n",
    );
}

#[test]
fn probe_exports_nested_inside_groups_same_name() {
    dump(
        "a group literally named exports, nested inside groups",
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             groups  => { exports => [qw(nested)] },\n\
             exports => [qw(top)],\n\
         };\n",
    );
}

#[test]
fn probe_groups_key_nested_inside_exports_hashref_form() {
    dump(
        "a key literally named groups, nested inside the exports hashref form",
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => { groups => undef, real => undef },\n\
         };\n",
    );
}

#[test]
fn probe_empty_exports_and_groups() {
    dump(
        "empty exports list and empty groups hash",
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw()], groups => {} };\n",
    );
}

#[test]
fn probe_unbalanced_source_no_panic() {
    dump(
        "unclosed setup hash (parse-error recovery)",
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a)]\n\
         sub foo { 1 }\n",
    );
    dump(
        "unclosed exports arrayref only",
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a), groups => { default => [qw(a)] } };\n",
    );
    dump(
        "stray closing brace before -setup",
        "package My::Utils;\n\
         use Sub::Exporter -setup => } exports => [qw(a)] };\n",
    );
}

#[test]
fn probe_qw_unusual_delimiters_and_string_with_brackets() {
    dump(
        "qw with ! delimiter and a string containing brace/bracket/comma chars",
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [ qw!a b!, 'c{d[e,f]}' ],\n\
         };\n",
    );
}

#[test]
fn probe_multiline_trailing_comma_and_comment() {
    dump(
        "multiline, trailing comma, comment interleaved",
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [\n\
                 qw(a b), # comment about b\n\
                 c => \\&gen,\n\
             ],\n\
             groups => {\n\
                 default => [qw(a)],\n\
             },\n\
         };\n",
    );
}

#[test]
fn probe_setup_not_first_flag() {
    dump(
        "-setup not first: -collector then -setup",
        "package My::Utils;\n\
         use Sub::Exporter -collector => { -init => 1 }, -setup => { exports => [qw(z)] };\n",
    );
}

#[test]
fn probe_qualified_name_in_exports() {
    dump(
        "a qualified name (::) inside exports list",
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(Other::Package::helper foo)] };\n",
    );
}

#[test]
fn probe_deep_nesting() {
    dump(
        "very deep nesting inside exports value",
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [ qw(a), b => { x => { y => { z => [1,2,3] } } } ],\n\
         };\n",
    );
}
