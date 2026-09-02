//! Capturing a `{ ... }` argument in a `use` statement (#2517).
//!
//! `use Module <bare args>, { ... };` used to stop recording arguments at the
//! `{`, dropping the hash and everything after it. Recording it lets a reader
//! see configuration that changes what the `use` does — a Sub::Exporter
//! installation redirect, for one — but it also puts tokens in front of every
//! consumer of `UseDecl.args` that never saw them before. These contracts pin
//! the two boundaries that keeps honest: the hash body is not an import list,
//! and a hash that never closes does not consume the rest of the file.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::hir::{HirFile, HirKind, lower_ast};
use perl_parser_core::{NodeKind, Parser};
use perl_semantic_facts::{FileId, ImportKind, ImportSpec, ImportSymbols};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn use_arguments(file: &HirFile) -> Vec<Vec<String>> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::UseDecl(use_decl) => Some(use_decl.args.clone()),
            _ => None,
        })
        .collect()
}

fn import_spec_for<'a>(specs: &'a [ImportSpec], module: &str) -> Option<&'a ImportSpec> {
    specs.iter().find(|spec| spec.module == module)
}

#[test]
fn a_trailing_hashref_body_is_not_read_as_an_import_list() -> Result<(), String> {
    // The shape this repository already writes, from
    // `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs`.
    // The statement asks for `param1` and `param2`; `key` and `value` are a
    // configuration pair. Reading the hash body publishes them as requested
    // symbols at `ExactAst`/`High` — an over-claim in the import direction,
    // and exactly what recording the hash would cost if nothing guarded it.
    let source = "use Another::Module 'param1', 'param2', {key => 'value'};\n";
    assert_clean_parse(source);
    let file = lower(source);

    let specs = file.compile_environment.import_specs(FileId(0));
    let spec = import_spec_for(&specs, "Another::Module")
        .ok_or_else(|| format!("no import spec for Another::Module in {specs:?}"))?;

    assert_eq!(spec.kind, ImportKind::UseExplicitList);
    assert_eq!(
        spec.symbols,
        ImportSymbols::Explicit(vec!["param1".to_string(), "param2".to_string()]),
        "a configuration hash contributes no requested symbols"
    );
    Ok(())
}

#[test]
fn a_setup_hash_body_is_not_read_as_an_import_list_either() -> Result<(), String> {
    // The same rule at the seam that motivated recording these tokens: a module
    // configuring its own exports requests nothing from Sub::Exporter, so
    // neither the setup keys nor the names it exports are symbols this file
    // imports.
    let source = "use Sub::Exporter -setup => { exports => [qw(foo bar)] };\n";
    assert_clean_parse(source);
    let file = lower(source);

    let specs = file.compile_environment.import_specs(FileId(0));
    let spec = import_spec_for(&specs, "Sub::Exporter")
        .ok_or_else(|| format!("no import spec for Sub::Exporter in {specs:?}"))?;

    match &spec.symbols {
        ImportSymbols::Explicit(names) => assert!(
            !names.iter().any(|name| ["exports", "foo", "bar"].contains(&name.as_str())),
            "setup configuration leaked into the requested symbols: {names:?}"
        ),
        other => assert!(
            matches!(other, ImportSymbols::None | ImportSymbols::Default),
            "unexpected symbols for a pure -setup line: {other:?}"
        ),
    }
    Ok(())
}

#[test]
fn a_hash_that_never_closes_does_not_swallow_the_rest_of_the_file() {
    // Half-typed source is a normal editor state, not an edge case. Consuming
    // greedily to the matching brace has no matching brace to find here, so it
    // would pull `sub g` and the statement after it into this one `use` and
    // erase them from the tree. The statement terminator ends the argument
    // list instead.
    let file = lower(
        "use Foo -x => 1, { a => 1;\n\
         sub g { return 42; }\n\
         my $x = 1;\n",
    );

    let subs: Vec<&String> = file
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::SubDecl(sub) => sub.name.as_ref(),
            _ => None,
        })
        .collect();

    assert!(
        subs.iter().any(|name| name.as_str() == "g"),
        "the sub after an unterminated use argument must survive, saw items: {:?}",
        file.items
            .iter()
            .map(|item| format!("{:?}", std::mem::discriminant(&item.kind)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_hash_holding_a_block_with_statements_is_captured_whole() -> Result<(), String> {
    // The opposite direction, and the reason the terminator rule is scoped to
    // the hash's own level rather than every semicolon: a value may be a sub
    // whose body has statements, and truncating there would record a hash that
    // looks unbalanced to every later reader.
    let source = "use Foo -x => 1, { builder => sub { my $n = 1; return $n; } };\n";
    assert_clean_parse(source);
    let file = lower(source);

    let arguments = use_arguments(&file);
    let captured = arguments.first().ok_or_else(|| "no use declaration lowered".to_string())?;

    assert_eq!(
        captured.iter().filter(|token| token.as_str() == "{").count(),
        2,
        "both braces open in the recorded arguments: {captured:?}"
    );
    assert_eq!(
        captured.iter().filter(|token| token.as_str() == "}").count(),
        2,
        "both braces close in the recorded arguments: {captured:?}"
    );
    assert!(
        captured.iter().any(|token| token.as_str() == "return"),
        "the block body is recorded rather than truncated: {captured:?}"
    );
    Ok(())
}

#[test]
fn a_leading_hash_argument_is_unchanged() -> Result<(), String> {
    // `use constant { ... }` takes a different, older branch that this change
    // does not touch. The control that pins the two apart.
    let source = "use constant { PI => 3, E => 2 };\n";
    assert_clean_parse(source);
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;

    let args = match &ast.kind {
        NodeKind::Program { statements } => {
            statements.iter().find_map(|statement| match &statement.kind {
                NodeKind::Use { args, .. } => Some(args.clone()),
                _ => None,
            })
        }
        _ => None,
    };

    assert_eq!(
        args,
        Some(vec![
            "{".to_string(),
            "PI".to_string(),
            "=>".to_string(),
            "3".to_string(),
            ",".to_string(),
            "E".to_string(),
            "=>".to_string(),
            "2".to_string(),
            "}".to_string(),
        ])
    );
    Ok(())
}
