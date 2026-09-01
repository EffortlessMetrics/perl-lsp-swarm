//! Sub::Exporter `-setup` export declarations (#2517).
//!
//! Sub::Exporter replaces `@EXPORT`/`@EXPORT_OK`/`%EXPORT_TAGS` with a
//! configuration hash, so the classic stash-variable path never observes these
//! packages. These contracts pin the HIR export declarations lowered from the
//! `-setup` hash, and pin a dynamic export boundary — never a partial export
//! list — for every configuration that cannot be enumerated statically.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    ExportDeclaration, ExportDeclarationKind, HirFile, StashConfidence, StashDynamicBoundaryKind,
    StashProvenance, lower_ast,
};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// `kind/tag -> symbols` for one package, in declaration order.
fn declarations(
    file: &HirFile,
    package: &str,
) -> Vec<(ExportDeclarationKind, Option<String>, Vec<String>)> {
    file.stash_graph
        .export_declarations
        .iter()
        .filter(|declaration| declaration.package == package)
        .map(|declaration| {
            (declaration.kind, declaration.tag_name.clone(), declaration.symbols.clone())
        })
        .collect()
}

/// `(symbol, reason)` for each dynamic export boundary on one package.
fn export_boundaries(file: &HirFile, package: &str) -> Vec<(Option<String>, String)> {
    file.stash_graph
        .dynamic_boundaries
        .iter()
        .filter(|boundary| {
            boundary.package.as_deref() == Some(package)
                && boundary.kind == StashDynamicBoundaryKind::DynamicExportDeclaration
        })
        .map(|boundary| (boundary.symbol.clone(), boundary.reason.clone()))
        .collect()
}

fn first_declaration<'a>(file: &'a HirFile, package: &str) -> Option<&'a ExportDeclaration> {
    file.stash_graph.export_declarations.iter().find(|d| d.package == package)
}

#[test]
fn setup_hash_declares_exports_default_group_and_tags() -> Result<(), String> {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [ qw(foo bar), baz => \\&build_baz ],\n\
             groups  => {\n\
                 default => [qw(foo)],\n\
                 heavy   => [qw(bar baz)],\n\
             },\n\
         };\n\
         sub foo { 1 }\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils"),
        vec![
            // Every `exports` name is importable on request, including the
            // name whose value is a generator coderef.
            (
                ExportDeclarationKind::Optional,
                None,
                vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]
            ),
            // A bare `use My::Utils;` installs the `default` group.
            (ExportDeclarationKind::Default, None, vec!["foo".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("heavy".to_string()),
                vec!["bar".to_string(), "baz".to_string()]
            ),
            // Sub::Exporter's implicit `all` group covers every export.
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]
            ),
        ]
    );
    assert!(
        export_boundaries(&file, "My::Utils").is_empty(),
        "a fully static -setup hash must not record a dynamic export boundary"
    );

    let declaration =
        first_declaration(&file, "My::Utils").ok_or("expected a My::Utils export declaration")?;
    assert_eq!(declaration.provenance, StashProvenance::DesugaredAst);
    assert_eq!(declaration.confidence, StashConfidence::High);
    assert!(
        declaration.declaration_item.is_some(),
        "export declarations must anchor back to the use statement"
    );
    Ok(())
}

#[test]
fn hashref_exports_form_declares_the_same_names() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => { foo => undef, bar => \\&gen } };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils"),
        vec![
            (ExportDeclarationKind::Optional, None, vec!["foo".to_string(), "bar".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["foo".to_string(), "bar".to_string()]
            ),
        ]
    );
}

#[test]
fn quoted_export_names_are_declared() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => ['foo', \"bar\"] };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils").first().map(|(kind, _, symbols)| (*kind, symbols.clone())),
        Some((ExportDeclarationKind::Optional, vec!["foo".to_string(), "bar".to_string()]))
    );
}

#[test]
fn no_default_group_leaves_the_default_export_list_empty() {
    // Sub::Exporter: "If a module that uses Sub::Exporter is used with no
    // arguments, it will try to export the group named default. If that group
    // has not been specifically configured, it will be empty."
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a b)] };\n",
    );

    assert!(
        !declarations(&file, "My::Utils")
            .iter()
            .any(|(kind, _, _)| *kind == ExportDeclarationKind::Default),
        "an unconfigured default group must not become a default export list"
    );
}

#[test]
fn explicit_all_group_is_not_replaced_by_the_implicit_one() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(a b)],\n\
             groups  => { all => [qw(a)] },\n\
         };\n",
    );

    let tags: Vec<_> = declarations(&file, "My::Utils")
        .into_iter()
        .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Tag)
        .collect();
    assert_eq!(
        tags,
        vec![(ExportDeclarationKind::Tag, Some("all".to_string()), vec!["a".to_string()])],
        "an explicitly configured all group must be the only all tag"
    );
}

#[test]
fn sub_exporter_progressive_uses_the_same_setup_contract() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter::Progressive -setup => {\n\
             exports => [qw(a b)],\n\
             groups  => { default => [qw(a)] },\n\
         };\n",
    );

    assert!(
        declarations(&file, "My::Utils")
            .iter()
            .any(|(kind, _, symbols)| *kind == ExportDeclarationKind::Default
                && symbols == &vec!["a".to_string()])
    );
}

#[test]
fn unrelated_setup_keys_do_not_become_exports() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports    => [qw(a)],\n\
             collectors => [qw(c)],\n\
             into_level => 1,\n\
         };\n",
    );

    for (_, _, symbols) in declarations(&file, "My::Utils") {
        assert!(
            !symbols.contains(&"c".to_string()) && !symbols.contains(&"into_level".to_string()),
            "collectors and installer options are not exports: {symbols:?}"
        );
    }
}

// --- negative controls: nothing enumerable, nothing claimed ----------------

#[test]
fn computed_exports_record_a_boundary_instead_of_an_export_list() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => $list };\n",
    );

    assert!(
        declarations(&file, "My::Utils").is_empty(),
        "a computed exports value must not produce export declarations"
    );
    assert_eq!(
        export_boundaries(&file, "My::Utils"),
        vec![(
            Some("exports".to_string()),
            "Sub::Exporter exports list is not statically enumerable".to_string()
        )]
    );
}

#[test]
fn a_group_naming_another_group_records_a_boundary_for_that_group() {
    // `default => [qw(-all)]` and its `:all` spelling name a group, not a
    // symbol. The exports list is still enumerable, so it stays; the group is
    // reported as a boundary rather than as an empty default list.
    for reference in ["-all", ":all"] {
        let file = lower(&format!(
            "package My::Utils;\n\
             use Sub::Exporter -setup => {{\n\
                 exports => [qw(a b)],\n\
                 groups  => {{ default => [qw({reference})] }},\n\
             }};\n",
        ));

        assert!(
            !declarations(&file, "My::Utils")
                .iter()
                .any(|(kind, _, _)| *kind == ExportDeclarationKind::Default),
            "{reference}: an unresolved group must not become a default export list"
        );
        assert_eq!(
            export_boundaries(&file, "My::Utils"),
            vec![(
                Some("default".to_string()),
                "Sub::Exporter group is not statically enumerable".to_string()
            )],
            "{reference}: expected one dynamic group boundary"
        );
    }
}

#[test]
fn sub_exporter_without_a_setup_hash_records_a_boundary() {
    for source in [
        "package My::Utils;\nuse Sub::Exporter;\n",
        "package My::Utils;\nuse Sub::Exporter -setup => $config;\n",
    ] {
        let file = lower(source);
        assert!(declarations(&file, "My::Utils").is_empty());
        assert_eq!(
            export_boundaries(&file, "My::Utils"),
            vec![(
                None,
                "Sub::Exporter export configuration is not a static -setup hash".to_string()
            )],
            "expected a dynamic export boundary for: {source}"
        );
    }
}

#[test]
fn computed_groups_value_records_a_boundary_but_keeps_the_exports_list() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a b)], groups => $groups };\n",
    );

    assert!(
        declarations(&file, "My::Utils")
            .iter()
            .any(|(kind, _, _)| *kind == ExportDeclarationKind::Optional),
        "an enumerable exports list survives an unenumerable groups value"
    );
    assert_eq!(
        export_boundaries(&file, "My::Utils"),
        vec![(
            Some("groups".to_string()),
            "Sub::Exporter groups value is not a static hash".to_string()
        )]
    );
}

#[test]
fn classic_exporter_declarations_are_unchanged() {
    let file = lower(
        "package Classic;\n\
         use Exporter 'import';\n\
         our @EXPORT = qw(x);\n\
         our @EXPORT_OK = qw(y);\n\
         our %EXPORT_TAGS = (all => [qw(x y)]);\n",
    );

    assert_eq!(
        declarations(&file, "Classic"),
        vec![
            (ExportDeclarationKind::Default, None, vec!["x".to_string()]),
            (ExportDeclarationKind::Optional, None, vec!["y".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["x".to_string(), "y".to_string()]
            ),
        ]
    );
    assert!(export_boundaries(&file, "Classic").is_empty());
}

#[test]
fn importing_sub_exporter_does_not_export_from_the_importing_package() {
    // A consumer of a Sub::Exporter-based module must not be treated as
    // configuring its own exports.
    let file = lower("package Consumer;\nuse My::Utils qw(foo);\n");

    assert!(file.stash_graph.export_declarations.is_empty());
    assert!(export_boundaries(&file, "Consumer").is_empty());
}
