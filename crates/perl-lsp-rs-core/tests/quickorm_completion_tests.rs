//! Completion receipts for canonical DBIx::QuickORM generated-member facts.

use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_parser::Parser;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use std::sync::Arc;
use url::Url;

fn completion_items(
    index: Arc<WorkspaceIndex>,
    source: &str,
) -> Result<Vec<CompletionItem>, Box<dyn std::error::Error>> {
    let ast = Parser::new(source).parse_with_recovery().ast;
    Ok(CompletionProvider::new_with_index_and_source(&ast, source, Some(index))
        .get_completions(source, source.len()))
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_ref()).collect()
}

#[test]
fn quickorm_table_package_surfaces_only_earned_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/User.pm")?,
        r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table users => sub {
    column id;
    columns qw/name email/;
};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::User->q")?;
    let labels = labels(&completions);

    assert!(
        labels.contains(&"qorm_table"),
        "canonical QuickORM generated member must reach method completion: {labels:?}"
    );
    for unearned in ["users", "id", "name", "email"] {
        assert!(
            !labels.contains(&unearned),
            "manual schema metadata must not reach completion as {unearned}: {labels:?}"
        );
    }
    Ok(())
}

#[test]
fn dynamic_quickorm_configuration_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/Dynamic.pm")?,
        r#"
package MyApp::Schema::Dynamic;
use DBIx::QuickORM type => table();
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::Dynamic->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "dynamic importer configuration must not promote qorm_table completion: {labels:?}"
    );
    Ok(())
}

#[test]
fn bare_quickorm_import_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/Bare.pm")?,
        r#"
package MyApp::Schema::Bare;
use DBIx::QuickORM;
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::Bare->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "bare importer configuration must not promote qorm_table completion: {labels:?}"
    );
    Ok(())
}

#[test]
fn current_package_qualified_builder_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/Qualified.pm")?,
        r#"
package MyApp::Schema::Qualified;
use DBIx::QuickORM type => 'table';
MyApp::Schema::Qualified::table users => sub {};
table later_users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::Qualified->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "a qualified current-package builder must consume authority without promoting qorm_table: {labels:?}"
    );
    Ok(())
}

#[test]
fn unrelated_qualified_builder_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/UnrelatedQualified.pm")?,
        r#"
package MyApp::Schema::UnrelatedQualified;
use DBIx::QuickORM type => 'table';
Other::table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::UnrelatedQualified->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "an unrelated qualified builder must not earn qorm_table completion: {labels:?}"
    );
    Ok(())
}

#[test]
fn competing_imported_table_builder_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/Competing.pm")?,
        r#"
package MyApp::Schema::Competing;
use DBIx::QuickORM type => 'table';
use Other::DSL qw(table);
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::Competing->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "a competing imported table builder must not reach canonical completion: {labels:?}"
    );
    Ok(())
}

#[test]
fn zero_argument_qualified_call_preserves_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/ZeroArgQualified.pm")?,
        r#"
package MyApp::Schema::ZeroArgQualified;
use DBIx::QuickORM type => 'table';
MyApp::Schema::ZeroArgQualified::table();
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::ZeroArgQualified->q")?;
    let labels = labels(&completions);
    assert!(
        labels.contains(&"qorm_table"),
        "a zero-argument qualified call must not consume authority: {labels:?}"
    );
    Ok(())
}

#[test]
fn later_quickorm_import_restores_generated_completion_after_builder_shadow()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/LaterImport.pm")?,
        r#"
package MyApp::Schema::LaterImport;
sub table {}
use DBIx::QuickORM type => 'table';
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::LaterImport->q")?;
    let labels = labels(&completions);
    assert!(
        labels.contains(&"qorm_table"),
        "a later valid QuickORM import must restore authority: {labels:?}"
    );
    Ok(())
}

#[test]
fn required_competing_import_call_does_not_surface_generated_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    index.index_file(
        Url::parse("file:///workspace/MyApp/Schema/RequiredImport.pm")?,
        r#"
package MyApp::Schema::RequiredImport;
use DBIx::QuickORM type => 'table';
require Other::DSL;
Other::DSL->import('table');
table users => sub {};
1;
"#
        .to_string(),
    )?;

    let completions = completion_items(index, "MyApp::Schema::RequiredImport->q")?;
    let labels = labels(&completions);
    assert!(
        !labels.contains(&"qorm_table"),
        "a required competing import call must invalidate QuickORM authority: {labels:?}"
    );
    Ok(())
}
