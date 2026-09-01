//! Sub::Exporter `-setup` export declarations (#2517).
//!
//! Sub::Exporter replaces `@EXPORT`/`@EXPORT_OK`/`%EXPORT_TAGS` with a
//! configuration hash, so the classic stash-variable path never observes these
//! packages. These contracts pin the HIR export declarations lowered from the
//! `-setup` hash, and pin a dynamic export boundary — never a partial export
//! list — for every configuration that cannot be enumerated statically.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    ExportDeclaration, ExportDeclarationKind, FrameworkAdapterRegistry, HirFile, StashConfidence,
    StashDynamicBoundaryKind, StashProvenance, lower_ast,
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
         };\n",
    );

    for (_, _, symbols) in declarations(&file, "My::Utils") {
        assert!(
            !symbols.contains(&"c".to_string()) && !symbols.contains(&"collectors".to_string()),
            "collector names are not exports: {symbols:?}"
        );
    }
}

#[test]
fn a_group_member_the_exports_do_not_declare_is_not_published() {
    // Sub::Exporter rejects a group naming something the exports configuration
    // does not declare, so publishing it would offer a symbol whose import
    // fails. The same holds when there is nothing to validate against.
    for setup in [
        "exports => [qw(a)], groups => { bad => [qw(b)] }",
        "exports => $computed, groups => { bad => [qw(b)] }",
        "groups => { bad => [qw(b)] }",
    ] {
        let file =
            lower(&format!("package My::Utils;\nuse Sub::Exporter -setup => {{ {setup} }};\n"));

        for (kind, tag, symbols) in declarations(&file, "My::Utils") {
            assert!(
                !symbols.contains(&"b".to_string()),
                "{setup}: undeclared group member published as {kind:?}/{tag:?}: {symbols:?}"
            );
        }
        assert!(
            export_boundaries(&file, "My::Utils")
                .iter()
                .any(|(symbol, _)| symbol.as_deref() == Some("bad")),
            "{setup}: expected a boundary naming the unresolved group"
        );
    }
}

#[test]
fn a_setup_that_installs_elsewhere_publishes_no_exports() {
    // `into`, `into_level`, and `installer` send the generated importer's
    // symbols somewhere other than the caller, so an ordinary `use` of this
    // module does not install them into the importing package.
    for redirect in [
        "as => 'do_import'",
        "into => 'Other::Package'",
        "into_level => 1",
        "installer => \\&install",
    ] {
        let file = lower(&format!(
            "package My::Utils;\n\
             use Sub::Exporter -setup => {{ exports => [qw(a b)], {redirect} }};\n",
        ));

        assert!(
            declarations(&file, "My::Utils").is_empty(),
            "{redirect}: exports must not be published when installation is redirected"
        );
        assert_eq!(
            export_boundaries(&file, "My::Utils")
                .iter()
                .map(|(_, reason)| reason.as_str())
                .collect::<Vec<_>>(),
            vec!["Sub::Exporter setup is not shown to install exports into the importing package"],
            "{redirect}: expected exactly one install-redirect boundary"
        );
    }
}

#[test]
fn a_second_setup_replaces_the_first_rather_than_adding_to_it() {
    // A second `-setup` installs a new importer over the first, so the earlier
    // configuration's names are stale, not additional.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(stale)] };\n\
         use Sub::Exporter -setup => { exports => [qw(fresh)] };\n",
    );

    let published: Vec<String> =
        declarations(&file, "My::Utils").into_iter().flat_map(|(_, _, s)| s).collect();
    assert!(
        published.contains(&"fresh".to_string()),
        "the surviving setup's exports must be published: {published:?}"
    );
    assert!(
        !published.contains(&"stale".to_string()),
        "the replaced setup's exports must not survive: {published:?}"
    );
}

#[test]
fn a_repeated_setup_key_resolves_to_its_last_value() {
    // Perl keeps the last value for a repeated hash key, so Sub::Exporter only
    // ever sees the last one.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(stale)], exports => [qw(fresh)] };\n",
    );

    let published: Vec<String> =
        declarations(&file, "My::Utils").into_iter().flat_map(|(_, _, s)| s).collect();
    assert!(published.contains(&"fresh".to_string()), "{published:?}");
    assert!(!published.contains(&"stale".to_string()), "{published:?}");
}

#[test]
fn a_computed_groups_value_suppresses_the_implicit_all_tag() {
    // A runtime `groups` value may define its own `all`, so synthesizing an
    // exact high-confidence one from `exports` would over-claim.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(a b)], groups => $groups };\n",
    );

    assert!(
        !declarations(&file, "My::Utils")
            .iter()
            .any(|(kind, tag, _)| *kind == ExportDeclarationKind::Tag
                && tag.as_deref() == Some("all")),
        "an implicit all tag must not be synthesized against a computed groups value"
    );
}

#[test]
fn a_repeated_name_is_declared_once_regardless_of_position() -> Result<(), String> {
    // `Vec::dedup` collapses only *consecutive* duplicates, so a repeated name
    // must be deduplicated by position-independent means. `ExportSet` sorts and
    // dedups on its own, which hides this; `FrameworkFactGraph` does not, so a
    // survived duplicate becomes a duplicated compiler fact. Assert the fact
    // graph, which is where it actually shows.
    for exports in ["qw(foo foo bar)", "qw(foo bar foo)"] {
        let file = lower(&format!(
            "package My::Utils;\n\
             use Sub::Exporter -setup => {{\n\
                 exports => [{exports}],\n\
                 groups  => {{ default => [qw(foo bar foo)] }},\n\
             }};\n",
        ));

        for (kind, _, symbols) in declarations(&file, "My::Utils") {
            let mut unique = symbols.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(
                unique.len(),
                symbols.len(),
                "{exports}: {kind:?} declaration repeats a name: {symbols:?}"
            );
        }

        let graph = FrameworkAdapterRegistry::default().project_file(&file);
        let mut identities: Vec<_> = graph
            .exported_symbols
            .iter()
            .map(|fact| {
                (
                    fact.package.clone(),
                    fact.name.clone(),
                    format!("{:?}", fact.kind),
                    fact.tag_name.clone(),
                )
            })
            .collect();
        let total = identities.len();
        identities.sort();
        identities.dedup();
        assert_eq!(
            identities.len(),
            total,
            "{exports}: framework fact graph repeats an exported-symbol identity"
        );
    }
    Ok(())
}

#[test]
fn a_generator_backed_export_is_not_a_high_confidence_declaration() -> Result<(), String> {
    // A live completion candidate is gated on `Confidence::High` and labelled
    // "compiler fact, high confidence". Sub::Exporter's `name => \&generator`
    // form exports the name but guarantees no `sub` of that name in source, so
    // the declaration carrying it must not claim High.
    let generated = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [ qw(plain), built => \\&build ] };\n",
    );
    let declaration =
        first_declaration(&generated, "My::Utils").ok_or("expected an export declaration")?;
    assert_eq!(
        declaration.confidence,
        StashConfidence::Medium,
        "a generator-backed export must not be declared at High confidence"
    );

    // The bareword form keeps High: Sub::Exporter's default generator does
    // require a sub of that name to exist.
    let anchored = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(plain other)] };\n",
    );
    let declaration =
        first_declaration(&anchored, "My::Utils").ok_or("expected an export declaration")?;
    assert_eq!(declaration.confidence, StashConfidence::High);

    // `undef` is Sub::Exporter's spelling for "no generator".
    let undef_generator = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => { plain => undef } };\n",
    );
    let declaration =
        first_declaration(&undef_generator, "My::Utils").ok_or("expected an export declaration")?;
    assert_eq!(declaration.confidence, StashConfidence::High);
    Ok(())
}

#[test]
fn a_group_keeps_its_plain_members_when_one_member_is_renamed() {
    // Sub::Exporter::Tutorial's documented group form. `rabbit` is installed
    // as `coney`, so the export name must not be published for this group —
    // but `beef` and `lox` are exactly known and must survive.
    let file = lower(
        "package Food;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(beef lox rabbit)],\n\
             groups  => { default => [ qw(beef lox), rabbit => { -as => 'coney' } ] },\n\
         };\n",
    );

    let default: Vec<Vec<String>> = declarations(&file, "Food")
        .into_iter()
        .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Default)
        .map(|(_, _, symbols)| symbols)
        .collect();
    assert_eq!(
        default,
        vec![vec!["beef".to_string(), "lox".to_string()]],
        "statically known group members must survive a renamed sibling"
    );
    assert!(
        export_boundaries(&file, "Food")
            .iter()
            .any(|(symbol, _)| symbol.as_deref() == Some("default")),
        "the renamed member must still be recorded as an unresolved group member"
    );
}

#[test]
fn a_group_keeps_its_plain_members_beside_a_group_reference() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(a b)],\n\
             groups  => { default => [qw(a -all)] },\n\
         };\n",
    );

    assert!(
        declarations(&file, "My::Utils")
            .iter()
            .any(|(kind, _, symbols)| *kind == ExportDeclarationKind::Default
                && symbols == &vec!["a".to_string()]),
        "a member named beside a group reference is still exactly known"
    );
    assert!(
        export_boundaries(&file, "My::Utils")
            .iter()
            .any(|(symbol, _)| symbol.as_deref() == Some("default"))
    );
}

#[test]
fn a_second_setup_replaces_the_first_even_when_it_cannot_be_read() {
    // The replacement must not depend on the *new* configuration being
    // readable: a computed second `-setup` still installs a new importer, so
    // the first configuration's names are stale either way.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(stale)] };\n\
         use Sub::Exporter -setup => $computed;\n",
    );

    assert!(
        declarations(&file, "My::Utils").is_empty(),
        "a computed second setup must not leave the first setup's exports standing"
    );
    assert!(
        export_boundaries(&file, "My::Utils")
            .iter()
            .any(|(_, reason)| reason.contains("not a static -setup hash"))
    );
}

#[test]
fn a_bare_use_after_a_setup_leaves_the_configuration_standing() {
    // A bare `use Sub::Exporter;` carries no `-setup`, so it establishes no
    // configuration and must not clear one that a previous statement proved.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(kept)] };\n\
         use Sub::Exporter;\n",
    );

    assert!(
        declarations(&file, "My::Utils")
            .iter()
            .any(|(_, _, symbols)| symbols.contains(&"kept".to_string())),
        "a bare use must not discard an established -setup configuration"
    );
}

#[test]
fn a_repeated_group_name_resolves_to_its_last_definition() {
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(stale fresh)],\n\
             groups  => { default => [qw(stale)], default => [qw(fresh)] },\n\
         };\n",
    );

    let default: Vec<Vec<String>> = declarations(&file, "My::Utils")
        .into_iter()
        .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Default)
        .map(|(_, _, symbols)| symbols)
        .collect();
    assert_eq!(
        default,
        vec![vec!["fresh".to_string()]],
        "a repeated group name must resolve to its last definition, not the union"
    );
}

#[test]
fn a_nested_as_option_does_not_suppress_the_setup() {
    // The install-redirect check looks for `as` at the *top level* of the
    // setup hash. `-as` appearing inside an entry's option hash is an
    // import-time rename of one export, not a redirect of the exporter, and
    // must leave the rest of the configuration published.
    for setup in
        ["exports => [ foo => { -as => 'bar' } ]", "exports => [qw(a)], collectors => [qw(as)]"]
    {
        let file =
            lower(&format!("package My::Utils;\nuse Sub::Exporter -setup => {{ {setup} }};\n"));

        assert!(
            !declarations(&file, "My::Utils").is_empty(),
            "{setup}: a nested `as` must not suppress the whole setup"
        );
        assert!(
            !export_boundaries(&file, "My::Utils")
                .iter()
                .any(|(_, reason)| reason.contains("not shown to install")),
            "{setup}: no install-redirect boundary should be recorded"
        );
    }
}

#[test]
fn replacing_a_sub_exporter_setup_leaves_classic_exporter_facts_alone() {
    // The replacement identifies prior Sub::Exporter declarations by their
    // `DesugaredAst` provenance, which the classic `@EXPORT` path does not
    // use. Pin that boundary so a future producer sharing the provenance —
    // or a widening of the retain — is caught here rather than by silently
    // dropping another mechanism's exports.
    let file = lower(
        "package My::Utils;\n\
         our @EXPORT = qw(classic);\n\
         use Sub::Exporter -setup => { exports => [qw(modern)] };\n",
    );

    let published: Vec<String> =
        declarations(&file, "My::Utils").into_iter().flat_map(|(_, _, s)| s).collect();
    assert!(
        published.contains(&"classic".to_string()),
        "a Sub::Exporter setup must not clear classic Exporter declarations: {published:?}"
    );
    assert!(published.contains(&"modern".to_string()), "{published:?}");
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
                "Sub::Exporter group has members that do not resolve to a declared export name"
                    .to_string()
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
            "Sub::Exporter groups value is not a static list".to_string()
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

#[test]
fn quoted_setup_keys_are_read_like_bare_ones() {
    // `{ 'exports' => ... }` is ordinary Perl and means what the bareword
    // spelling means. Comparing raw tokens read it as an absent key, which
    // published nothing and recorded no boundary — a silent partial answer.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter '-setup' => {\n\
             'exports' => [qw(foo bar)],\n\
             \"groups\"  => { default => [qw(foo)] },\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils"),
        vec![
            (ExportDeclarationKind::Optional, None, vec!["foo".to_string(), "bar".to_string()]),
            (ExportDeclarationKind::Default, None, vec!["foo".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["foo".to_string(), "bar".to_string()]
            ),
        ]
    );
    assert!(export_boundaries(&file, "My::Utils").is_empty());
}

#[test]
fn array_form_groups_declare_the_same_groups_as_the_hash_form() {
    // "The `groups` list can be passed in the same forms as `exports`", and an
    // `exports` list "may be provided as an array reference or a hash
    // reference" — so the arrayref spelling is a static configuration, not a
    // dynamic boundary.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(foo bar)],\n\
             groups  => [ default => [qw(foo)], tools => [qw(bar)] ],\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils"),
        vec![
            (ExportDeclarationKind::Optional, None, vec!["foo".to_string(), "bar".to_string()]),
            (ExportDeclarationKind::Default, None, vec!["foo".to_string()]),
            (ExportDeclarationKind::Tag, Some("tools".to_string()), vec!["bar".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["foo".to_string(), "bar".to_string()]
            ),
        ]
    );
    assert!(export_boundaries(&file, "My::Utils").is_empty());
}

#[test]
fn a_custom_generator_lowers_confidence_for_every_export() {
    // `generator` is "a callback used to produce the code that will be
    // installed", defaulting to Sub::Exporter's own generator — the one that
    // turns a plain `exports` name into this package's sub of that name.
    // Replacing it removes that anchoring for every export, so no declaration
    // from this setup may claim high confidence.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports   => [qw(foo bar)],\n\
             groups    => { default => [qw(foo)] },\n\
             generator => \\&build_any,\n\
         };\n",
    );

    let confidences: Vec<StashConfidence> = file
        .stash_graph
        .export_declarations
        .iter()
        .filter(|declaration| declaration.package == "My::Utils")
        .map(|declaration| declaration.confidence)
        .collect();

    assert_eq!(confidences.len(), 3, "optional, default and implicit-all declarations");
    assert!(
        confidences.iter().all(|confidence| *confidence == StashConfidence::Medium),
        "a custom generator leaves no declaration at high confidence: {confidences:?}"
    );
}

#[test]
fn a_setup_whose_brackets_do_not_close_publishes_no_exports() {
    // A negative control on delimiter matching: nesting depth alone would let
    // `[ ... }` balance and hand the reader a body spanning a delimiter that
    // was never closed. An unreadable setup is a boundary, never a partial
    // export list.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(foo bar) };\n",
    );

    assert!(
        declarations(&file, "My::Utils").is_empty(),
        "no export list is published from a setup that cannot be delimited"
    );
    assert_eq!(
        export_boundaries(&file, "My::Utils"),
        vec![(None, "Sub::Exporter export configuration is not a static -setup hash".to_string())]
    );
}

#[test]
fn a_group_member_carrying_only_generator_arguments_is_published() {
    // `reformat => { -as => 'email_format', width => 72 }` in the documentation
    // shows both kinds of group-member option side by side: `-as` renames what
    // is installed, `width` is a generator argument. A member whose options
    // carry no dashed directive at all installs under its own name, so
    // withholding it would hide an import a consumer really gets.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(item other)],\n\
             groups  => { default => [ qw(other), item => { width => 72 } ] },\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils"),
        vec![
            (ExportDeclarationKind::Optional, None, vec!["item".to_string(), "other".to_string()]),
            (ExportDeclarationKind::Default, None, vec!["other".to_string(), "item".to_string()]),
            (
                ExportDeclarationKind::Tag,
                Some("all".to_string()),
                vec!["item".to_string(), "other".to_string()]
            ),
        ]
    );
    assert!(
        export_boundaries(&file, "My::Utils").is_empty(),
        "a member that keeps its name leaves the group complete"
    );
}

#[test]
fn a_renamed_group_member_is_still_withheld_beside_a_plain_options_member() {
    // The control for the test above: adding options must not become a blanket
    // "publish it anyway". A dashed directive still withholds that member and
    // still marks the group incomplete.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(item rabbit)],\n\
             groups  => { default => [ item => { width => 72 }, rabbit => { -as => 'coney' } ] },\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils")
            .into_iter()
            .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Default)
            .map(|(_, _, symbols)| symbols)
            .collect::<Vec<_>>(),
        vec![vec!["item".to_string()]],
        "the renamed member is withheld, the argument-only member is kept"
    );
    assert_eq!(
        export_boundaries(&file, "My::Utils"),
        vec![(
            Some("default".to_string()),
            "Sub::Exporter group has members that do not resolve \
             to a declared export name"
                .to_string()
        )]
    );
}

#[test]
fn a_group_member_with_a_computed_option_key_is_withheld() {
    // A key that is not a literal cannot be shown *not* to evaluate to `-as`,
    // so it proves nothing about the installed name. The member stays
    // unresolved and the group reads as incomplete, rather than publishing a
    // name a consumer may never receive.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(item other)],\n\
             groups  => { default => [ qw(other), item => { $option => 'coney' } ] },\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils")
            .into_iter()
            .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Default)
            .map(|(_, _, symbols)| symbols)
            .collect::<Vec<_>>(),
        vec![vec!["other".to_string()]],
        "a computed option key withholds the member it decorates"
    );
    assert_eq!(
        export_boundaries(&file, "My::Utils"),
        vec![(
            Some("default".to_string()),
            "Sub::Exporter group has members that do not resolve \
             to a declared export name"
                .to_string()
        )]
    );
}

#[test]
fn a_quoted_literal_option_key_still_publishes_its_member() {
    // The control in the other direction: tightening the rule to literal keys
    // must not withdraw the quoted spelling of an ordinary generator argument.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(item)],\n\
             groups  => { default => [ item => { 'width' => 72 } ] },\n\
         };\n",
    );

    assert_eq!(
        declarations(&file, "My::Utils")
            .into_iter()
            .filter(|(kind, _, _)| *kind == ExportDeclarationKind::Default)
            .map(|(_, _, symbols)| symbols)
            .collect::<Vec<_>>(),
        vec![vec!["item".to_string()]]
    );
    assert!(export_boundaries(&file, "My::Utils").is_empty());
}
