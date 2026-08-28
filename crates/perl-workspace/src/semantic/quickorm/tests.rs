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
    assert_eq!(spec.confidence, Confidence::High);
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
    assert_eq!(spec.confidence, Confidence::High);
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
