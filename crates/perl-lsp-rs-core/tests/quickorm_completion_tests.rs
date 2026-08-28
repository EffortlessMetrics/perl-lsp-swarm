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
    Ok(
        CompletionProvider::new_with_index_and_source(&ast, source, Some(index))
            .get_completions(source, source.len()),
    )
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
