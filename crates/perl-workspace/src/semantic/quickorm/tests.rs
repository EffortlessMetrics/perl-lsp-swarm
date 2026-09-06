use super::*;
use crate::Parser;

fn import_specs_from_source(source: &str) -> Result<Vec<ImportSpec>, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let ast =
        parser.parse().map_err(|error| format!("failed to parse QuickORM import: {error:?}"))?;
    Ok(super::super::workspace_import_extractor::extract_import_specs_with_source(
        &ast,
        FileId(1),
        source,
    ))
}

fn generated_facts_from_source(
    source: &str,
) -> Result<Vec<GeneratedMemberFact>, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let ast = parser
        .parse()
        .map_err(|error| format!("failed to parse QuickORM table package: {error:?}"))?;
    Ok(super::super::generated_member_extractor::extract_generated_member_facts_with_source(
        &ast,
        FileId(2),
        source,
    ))
}

fn quickorm_spec(specs: &[ImportSpec]) -> Result<&ImportSpec, Box<dyn std::error::Error>> {
    specs
        .iter()
        .find(|spec| spec.module == QUICKORM_MODULE)
        .ok_or_else(|| "missing DBIx::QuickORM import spec".into())
}

fn find_quickorm_use(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Use { module, .. } if module == QUICKORM_MODULE) {
        return Some(node);
    }

    for child in node.children() {
        if let Some(found) = find_quickorm_use(child) {
            return Some(found);
        }
    }
    None
}

fn canonical_names(facts: &[GeneratedMemberFact]) -> Vec<&str> {
    let mut names: Vec<_> = facts.iter().map(|fact| fact.entity.canonical_name.as_str()).collect();
    names.sort_unstable();
    names
}

#[test]
fn configured_table_import_uses_default_dsl_exports() -> Result<(), Box<dyn std::error::Error>> {
    let specs = import_specs_from_source(
        "package User; use DBIx::QuickORM type => 'table'; table users => sub {};",
    )?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.kind, ImportKind::Use);
    assert_eq!(spec.symbols, ImportSymbols::Default);
    assert_eq!(spec.provenance, Provenance::ImportExportInference);
    assert_eq!(spec.confidence, Confidence::Medium);
    Ok(())
}

#[test]
fn compact_and_comment_separated_fat_arrow_imports_are_source_backed()
-> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "package User; use DBIx::QuickORM type=>\"table\"; table users => sub {};",
        "package User; use DBIx::QuickORM type # key\n => # arrow\n \"table\"; table users => sub {};",
    ] {
        let specs = import_specs_from_source(source)?;
        let spec = quickorm_spec(&specs)?;
        assert_eq!(spec.confidence, Confidence::Medium);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        assert_eq!(
            canonical_names(&generated_facts_from_source(source)?),
            vec!["User::qorm_table"]
        );
    }
    Ok(())
}

#[test]
fn semicolons_inside_import_comments_do_not_truncate_source_backed_shape()
-> Result<(), Box<dyn std::error::Error>> {
    let source =
        "package User; use DBIx::QuickORM type # fixed; mode\n => 'table'; table users => sub {};";
    let specs = import_specs_from_source(source)?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.confidence, Confidence::Medium);
    assert_eq!(spec.symbols, ImportSymbols::Default);
    assert_eq!(canonical_names(&generated_facts_from_source(source)?), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn zero_argument_current_package_qualified_call_does_not_consume_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; User::table(); table users => sub {};",
    )?;

    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn later_quickorm_import_reestablishes_authority_after_builder_shadow()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; sub table {}; use DBIx::QuickORM type => 'table'; table users => sub {};",
    )?;

    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn required_competing_import_call_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; require Other::DSL; Other::DSL->import('table'); table users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn qualified_competing_import_call_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; require Other::DSL; Other::DSL::import('table'); table users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn competing_view_import_invalidates_quickorm_authority() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; use Other::DSL qw(view); view users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn nested_qualified_builder_call_invalidates_outer_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; sub build { User::table 'nested' => sub {}; } table users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn nested_qualified_builder_initializer_invalidates_outer_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; my $builder = User::table 'nested' => sub {}; table users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn competing_view_method_import_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; Other::DSL->import(qw(view)); view users => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn dynamic_hash_competing_import_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; Other::DSL->import(table => sub {}); table users => sub {};",
    )?;

    assert!(
        facts.is_empty(),
        "an unknown hash-shaped competing importer must consume QuickORM authority"
    );
    Ok(())
}

#[test]
fn compile_time_quickorm_import_inside_subroutine_enables_following_builder()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; sub configure { use DBIx::QuickORM type => 'table'; } table users => sub {};",
    )?;

    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn double_dollar_interpolation_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; table \"cost$$\" => sub {};",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn parser_preserves_quickorm_configuration_as_key_value_args()
-> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("use DBIx::QuickORM type => 'table';");
    let ast =
        parser.parse().map_err(|error| format!("failed to parse QuickORM import: {error:?}"))?;
    let use_node = find_quickorm_use(&ast).ok_or("missing QuickORM use node")?;
    let NodeKind::Use { args, .. } = &use_node.kind else {
        return Err("expected QuickORM use node".into());
    };

    assert_eq!(args, &["type".to_string(), "'table'".to_string()]);
    Ok(())
}

#[test]
fn parser_preserves_quickorm_type_call_expression_syntax() -> Result<(), Box<dyn std::error::Error>>
{
    let mut parser = Parser::new("use DBIx::QuickORM type => table();");
    let ast = parser
        .parse()
        .map_err(|error| format!("failed to parse QuickORM call import: {error:?}"))?;
    let use_node = find_quickorm_use(&ast).ok_or("missing QuickORM use node")?;
    let NodeKind::Use { args, .. } = &use_node.kind else {
        return Err("expected QuickORM use node".into());
    };

    let raw = args.join(" ");
    assert!(
        raw.contains('(') && raw.contains(')'),
        "call expression punctuation must remain visible to the classifier: {args:?}"
    );
    Ok(())
}

#[test]
fn bare_quickorm_import_does_not_enable_table_package_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "package User; use DBIx::QuickORM; table users => sub {};";
    let specs = import_specs_from_source(source)?;
    let spec = quickorm_spec(&specs)?;
    assert_eq!(spec.kind, ImportKind::Use);
    assert_eq!(spec.symbols, ImportSymbols::Default);
    assert!(generated_facts_from_source(source)?.is_empty());
    Ok(())
}

#[test]
fn comma_without_fat_arrow_remains_dynamic_and_cannot_enable_table_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "package User; sub type () { 'only' }; use DBIx::QuickORM type, 'table'; table users => sub {};";
    let specs = import_specs_from_source(source)?;
    let spec = quickorm_spec(&specs)?;
    assert_eq!(spec.kind, ImportKind::ManualImport);
    assert_eq!(spec.symbols, ImportSymbols::Dynamic);
    assert_eq!(spec.provenance, Provenance::DynamicBoundary);
    assert!(generated_facts_from_source(source)?.is_empty());
    Ok(())
}

#[test]
fn source_free_generated_member_facade_is_conservative() -> Result<(), Box<dyn std::error::Error>> {
    let source = "package User; use DBIx::QuickORM type => 'table'; table users => sub {};";
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    assert!(
        super::super::generated_member_extractor::extract_generated_member_facts(&ast, FileId(2))
            .is_empty(),
        "source-free extraction must not infer separator-sensitive QuickORM authority"
    );
    Ok(())
}

#[test]
fn configured_orm_import_uses_default_dsl_exports() -> Result<(), Box<dyn std::error::Error>> {
    let specs = import_specs_from_source("package App; use DBIx::QuickORM type => 'orm';")?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.kind, ImportKind::Use);
    assert_eq!(spec.symbols, ImportSymbols::Default);
    assert_eq!(spec.provenance, Provenance::ImportExportInference);
    assert_eq!(spec.confidence, Confidence::Medium);
    Ok(())
}

#[test]
fn dynamic_type_call_remains_dynamic_and_does_not_emit_qorm_table()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
package User;
use DBIx::QuickORM type => table();
table users => sub {};
1;
"#;
    let specs = import_specs_from_source(source)?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.kind, ImportKind::ManualImport);
    assert_eq!(spec.symbols, ImportSymbols::Dynamic);
    assert_eq!(spec.provenance, Provenance::DynamicBoundary);
    assert_eq!(spec.confidence, Confidence::Low);
    assert!(generated_facts_from_source(source)?.is_empty());
    Ok(())
}

#[test]
fn bare_constant_import_value_remains_dynamic() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
package User;
sub table () { 'orm' }
use DBIx::QuickORM type => table;
table users => sub {};
1;
"#;
    let specs = import_specs_from_source(source)?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.kind, ImportKind::ManualImport);
    assert_eq!(spec.symbols, ImportSymbols::Dynamic);
    assert_eq!(spec.provenance, Provenance::DynamicBoundary);
    assert_eq!(spec.confidence, Confidence::Low);
    assert!(
        generated_facts_from_source(source)?.is_empty(),
        "a parenthesis-free constant call must not earn literal table mode"
    );
    Ok(())
}

#[test]
fn filtered_quickorm_import_remains_a_dynamic_manual_import()
-> Result<(), Box<dyn std::error::Error>> {
    let specs = import_specs_from_source(
        "package User; use DBIx::QuickORM type => 'table', only => ['table'];",
    )?;
    let spec = quickorm_spec(&specs)?;

    assert_eq!(spec.kind, ImportKind::ManualImport);
    assert_eq!(spec.symbols, ImportSymbols::Dynamic);
    assert_eq!(spec.provenance, Provenance::DynamicBoundary);
    assert_eq!(spec.confidence, Confidence::Low);
    Ok(())
}

#[test]
fn lookalike_import_keeps_generic_import_classification() -> Result<(), Box<dyn std::error::Error>>
{
    let specs = import_specs_from_source("package User; use Local::DSL type => 'table';")?;
    let spec = specs
        .iter()
        .find(|spec| spec.module == "Local::DSL")
        .ok_or("missing lookalike import spec")?;

    assert_eq!(spec.kind, ImportKind::UseExplicitList);
    assert!(matches!(&spec.symbols, ImportSymbols::Explicit(_)));
    assert_eq!(spec.provenance, Provenance::ExactAst);
    Ok(())
}

#[test]
fn table_package_emits_only_fixed_qorm_table_member() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column id;
    columns qw/name email/;
};
1;
"#,
    )?;
    let names = canonical_names(&facts);

    assert_eq!(names, vec!["MyApp::Schema::User::qorm_table"]);
    let fact = facts.first().ok_or("missing qorm_table fact")?;
    assert_eq!(fact.entity.kind, EntityKind::GeneratedMember);
    assert_eq!(fact.entity.provenance, Provenance::FrameworkSynthesis);
    assert_eq!(fact.entity.confidence, Confidence::Medium);
    assert_eq!(fact.anchor.provenance, Provenance::FrameworkSynthesis);
    assert_eq!(fact.anchor.confidence, Confidence::Medium);
    assert!(fact.anchor.span_end_byte > fact.anchor.span_start_byte);
    Ok(())
}

#[test]
fn package_reentry_retains_unconsumed_table_authority() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
package Other::Package;
package MyApp::Schema::User;
table users => sub {};
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::User::qorm_table"]);
    Ok(())
}

#[test]
fn bare_lexical_block_preserves_same_package_import_and_isolates_nested_package()
-> Result<(), Box<dyn std::error::Error>> {
    let same_package = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
{
    use DBIx::QuickORM type => 'table';
}
table users => sub {};
1;
"#,
    )?;
    assert_eq!(canonical_names(&same_package), vec!["MyApp::Schema::User::qorm_table"]);

    let nested_package = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
{
    package Other::Package {
        use DBIx::QuickORM type => 'table';
        table other_users => sub {};
    }
}
table users => sub {};
1;
"#,
    )?;
    assert_eq!(canonical_names(&nested_package), vec!["Other::Package::qorm_table"]);

    let semicolon_package = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
{
    package Other::Package;
    use DBIx::QuickORM type => 'table';
    table other_users => sub {};
}
table users => sub {};
1;
"#,
    )?;
    assert_eq!(canonical_names(&semicolon_package), vec!["Other::Package::qorm_table"]);
    Ok(())
}

#[test]
fn nested_braced_package_is_attributed_to_inner_package() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        r#"
package Outer::Package {
    use DBIx::QuickORM type => 'table';
    package Inner::Package {
        use DBIx::QuickORM type => 'table';
        table inner_users => sub {};
    }
}
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["Inner::Package::qorm_table"]);
    assert!(
        facts.iter().all(|fact| fact.entity.canonical_name != "Outer::Package::qorm_table"),
        "nested compile-time declarations must not emit a false outer-package fact"
    );
    Ok(())
}

#[test]
fn self_delimited_qw_table_import_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
package User;
use DBIx::QuickORM type => 'table';
use Other::DSL qw/table/;
table users => sub {};
1;
"#;

    let mut parser = Parser::new(source);
    let ast = parser
        .parse()
        .map_err(|error| format!("failed to parse competing qw import: {error:?}"))?;
    let competing_use = ast
        .children()
        .into_iter()
        .find(|node| matches!(&node.kind, NodeKind::Use { module, .. } if module == "Other::DSL"))
        .ok_or("missing competing Other::DSL use node")?;
    let NodeKind::Use { args, .. } = &competing_use.kind else {
        return Err("expected competing Other::DSL use node".into());
    };
    assert_eq!(args, &["qw/table/".to_string()]);

    assert!(
        generated_facts_from_source(source)?.is_empty(),
        "the self-delimited qw/table/ import must invalidate QuickORM authority"
    );
    Ok(())
}

#[test]
fn dynamic_builder_consumes_table_package_authority() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table $runtime_name => sub {};
table users => sub {};
1;
"#,
    )?;

    assert!(
        facts.is_empty(),
        "the first direct builder consumes QuickORM's imported DSL authority"
    );
    Ok(())
}

#[test]
fn dynamic_builder_body_consumes_table_package_authority() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table users => $runtime_builder;
table later_users => sub {};
1;
"#,
    )?;

    assert!(
        facts.is_empty(),
        "a runtime builder body consumes QuickORM authority without earning a fact"
    );
    Ok(())
}

#[test]
fn competing_table_import_invalidates_quickorm_authority() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; use Other::DSL qw(table); table users => sub {};",
    )?;

    assert!(
        facts.is_empty(),
        "a competing imported table builder must not retain QuickORM authority"
    );
    Ok(())
}

#[test]
fn comment_separated_quickorm_import_retains_authority() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use# comment\nDBIx::QuickORM type => 'table'; table users => sub {};",
    )?;

    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn competing_quote_like_table_imports_invalidate_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    for delimiter in ["/table/", "(table)", "[table]", "{table}", "<table>"] {
        let source = format!(
            "package User; use DBIx::QuickORM type => 'table'; use Other::DSL qw{delimiter}; table users => sub {{}};",
            delimiter = delimiter
        );
        assert!(
            generated_facts_from_source(&source)?.is_empty(),
            "competing qw{delimiter} import must invalidate QuickORM authority: {source}"
        );
    }
    Ok(())
}

#[test]
fn malformed_quote_like_table_imports_remain_fail_closed() -> Result<(), Box<dyn std::error::Error>>
{
    let mut parser = Parser::new(
        "package User; use DBIx::QuickORM type => 'table'; use Other::DSL qw/table; table users => sub {};",
    );
    let ast =
        parser.parse().map_err(|error| format!("failed to inspect malformed import: {error:?}"))?;
    let malformed_use = ast
        .children()
        .into_iter()
        .find(|node| matches!(&node.kind, NodeKind::Use { module, .. } if module == "Other::DSL"))
        .ok_or("missing malformed Other::DSL use node")?;
    let NodeKind::Use { args, .. } = &malformed_use.kind else {
        return Err("expected malformed Other::DSL use node".into());
    };
    assert!(
        !imports_table_builder(args),
        "the parser-recovered malformed quote-like argument is not an actionable competing import"
    );

    for import in
        ["use Other::DSL qw{table] ;", "use Other::DSL qw[table};", "use Other::DSL qw(table] ;"]
    {
        let source = format!(
            "package User; use DBIx::QuickORM type => 'table'; table users => sub {{}}; {import}"
        );
        assert_eq!(
            canonical_names(&generated_facts_from_source(&source)?),
            vec!["User::qorm_table"],
            "malformed recovered import must not invalidate an already-earned fact: {source}"
        );
    }
    Ok(())
}

#[test]
fn competing_quote_like_method_imports_invalidate_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    for delimiter in ["/table/", "(table)", "[table]", "{table}", "<table>"] {
        let source = format!(
            "package User; use DBIx::QuickORM type => 'table'; Other::DSL->import(qw{delimiter}); table users => sub {{}};",
            delimiter = delimiter
        );
        assert!(
            generated_facts_from_source(&source)?.is_empty(),
            "competing method qw{delimiter} import must invalidate QuickORM authority: {source}"
        );
    }
    Ok(())
}

#[test]
fn dynamic_receiver_method_import_invalidates_quickorm_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
package User;
use DBIx::QuickORM type => 'table';
table "first" => sub {};
my $dsl = Other::DSL;
$dsl->import(qw(table));
table "second" => sub {};
1;
"#;

    assert!(
        generated_facts_from_source(source)?.is_empty(),
        "a parser-backed import through an unknown receiver must consume QuickORM authority"
    );
    Ok(())
}

#[test]
fn double_quoted_static_table_name_emits_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table "users" => sub {};
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::User::qorm_table"]);
    Ok(())
}

#[test]
fn percent_in_double_quoted_table_name_remains_static() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::Archive;
use DBIx::QuickORM type => 'table';
table "archive%2026" => sub {};
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::Archive::qorm_table"]);
    Ok(())
}

#[test]
fn interpolated_table_name_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table "${prefix}_users" => sub {};
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn trailing_dollar_in_double_quoted_table_name_is_literal() -> Result<(), Box<dyn std::error::Error>>
{
    let facts = generated_facts_from_source(
        "package MyApp::Schema::Dollar; use DBIx::QuickORM type => 'table'; table \"cost$\" => sub {};",
    )?;
    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::Dollar::qorm_table"]);
    Ok(())
}

#[test]
fn trailing_at_in_double_quoted_table_name_is_literal() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package MyApp::Schema::At; use DBIx::QuickORM type => 'table'; table \"archive@\" => sub {};",
    )?;
    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::At::qorm_table"]);
    Ok(())
}

#[test]
fn array_interpolation_in_double_quoted_table_name_remains_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::Users;
use DBIx::QuickORM type => 'table';
table "@users" => sub {};
1;
"#,
    )?;
    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn perl_namespace_and_special_scalar_interpolation_remains_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    for (package, table_name) in [("Namespaced", "$::prefix_users"), ("Special", "$^O")] {
        let source = format!(
            "package MyApp::Schema::{package}; use DBIx::QuickORM type => 'table'; table \"{table_name}\" => sub {{}};"
        );
        assert!(
            generated_facts_from_source(&source)?.is_empty(),
            "Perl interpolation must not promote {table_name}"
        );
    }
    Ok(())
}

#[test]
fn punctuation_special_variable_interpolation_remains_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    for (package, table_name) in
        [("Match", "$&"), ("Postmatch", "$'"), ("Prematch", "$`"), ("MatchedIndexes", "@+")]
    {
        let source = format!(
            "package MyApp::Schema::{package}; use DBIx::QuickORM type => 'table'; table \"{table_name}\" => sub {{}};"
        );
        assert!(
            generated_facts_from_source(&source)?.is_empty(),
            "special Perl variable interpolation must not promote {table_name}"
        );
    }
    Ok(())
}

#[test]
fn repeated_valid_imports_refresh_the_qorm_table_anchor() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "package User; use DBIx::QuickORM type => 'table'; table 'first' => sub {}; use DBIx::QuickORM type => 'table'; table 'second' => sub {};";
    let facts = generated_facts_from_source(source)?;
    let fact = facts.first().ok_or("missing qorm_table fact")?;
    let second_start = source.find("'second'").ok_or("missing second table name")?;
    let second_end = second_start + "'second'".len();

    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    assert_eq!(fact.anchor.span_start_byte as usize, second_start);
    assert_eq!(fact.anchor.span_end_byte as usize, second_end);
    Ok(())
}

#[test]
fn nested_builder_does_not_promote_outer_table_call() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; table users, wrapper(sub {});",
    )?;
    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn package_builder_redefinition_invalidates_prior_generated_fact()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; use DBIx::QuickORM type => 'table'; table users => sub {}; sub table { 1 };",
    )?;
    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn dynamic_reconfiguration_invalidates_prior_generated_fact()
-> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "package User; use DBIx::QuickORM type => 'table'; table 'first' => sub {}; use DBIx::QuickORM type => table(); table 'second' => sub {};",
        "package User; use DBIx::QuickORM type => 'table'; table 'first' => sub {}; use DBIx::QuickORM type => 'table'; table 'second' => make_builder(sub {});",
    ] {
        assert!(
            generated_facts_from_source(source)?.is_empty(),
            "dynamic QuickORM reconfiguration must not retain a stale qorm_table fact"
        );
    }
    Ok(())
}

#[test]
fn package_level_table_shadow_is_replaced_by_later_import_promotion()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package User; sub table { 1 }; use DBIx::QuickORM type => 'table'; table users => sub {};",
    )?;
    assert_eq!(canonical_names(&facts), vec!["User::qorm_table"]);
    Ok(())
}

#[test]
fn view_package_emits_fixed_qorm_table_member() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::ActiveUser;
use DBIx::QuickORM type => 'table';
view active_users => sub {};
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::ActiveUser::qorm_table"]);
    Ok(())
}

#[test]
fn orm_mode_inline_schema_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::ORM;
use DBIx::QuickORM type => 'orm';
schema app => sub {
    table users => sub {};
};
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn filtered_table_import_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table', only => ['table'];
table users => sub {};
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn rename_skip_and_unknown_quickorm_options_remain_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "package User; use DBIx::QuickORM type => 'table', rename => { table => 'make_table' }; table users => sub {};",
        "package User; use DBIx::QuickORM type => 'table', skip => ['table']; table users => sub {};",
        "package User; use DBIx::QuickORM type => 'table', unknown => 'value'; table users => sub {};",
    ] {
        let specs = import_specs_from_source(source)?;
        let spec = quickorm_spec(&specs)?;
        assert_eq!(spec.kind, ImportKind::ManualImport);
        assert_eq!(spec.symbols, ImportSymbols::Dynamic);
        assert_eq!(spec.provenance, Provenance::DynamicBoundary);
        assert_eq!(spec.confidence, Confidence::Low);
        assert!(generated_facts_from_source(source)?.is_empty());
    }
    Ok(())
}

#[test]
fn table_mode_without_builder_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        "package MyApp::Schema::User; use DBIx::QuickORM type => 'table'; 1;",
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn table_call_inside_subroutine_is_not_treated_as_package_builder()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
sub build_later {
    table users => sub {};
}
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn table_call_inside_runtime_control_is_not_treated_as_package_builder()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
if ($enabled) {
    table users => sub {};
}
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}

#[test]
fn bare_lexical_block_does_not_leak_package_or_framework_state()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
{
    package Other::Package;
    use DBIx::QuickORM type => 'orm';
}
table users => sub {};
1;
"#,
    )?;

    assert_eq!(canonical_names(&facts), vec!["MyApp::Schema::User::qorm_table"]);
    Ok(())
}

#[test]
fn lookalike_table_dsl_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
    let facts = generated_facts_from_source(
        r#"
package MyApp::Schema::User;
use Local::DSL type => 'table';
table users => sub {};
1;
"#,
    )?;

    assert!(facts.is_empty());
    Ok(())
}
