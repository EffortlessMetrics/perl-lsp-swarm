//! Parsed arrow-method edits must not retain the suffix under the cursor.
use anyhow::{Result, anyhow, ensure};
use perl_lsp_rs_core::providers::completion::CompletionProvider;
use perl_parser_core::Parser;
use perl_workspace::workspace_index::WorkspaceIndex;
use std::sync::Arc;
use url::Url;

#[test]
fn parsed_arrow_method_edits_replace_the_whole_identifier() -> Result<()> {
    let index = Arc::new(WorkspaceIndex::new());
    index
        .index_file(
            Url::parse("file:///workspace/Remote.pm")?,
            "package Remote; sub leap {} 1;".to_string(),
        )
        .map_err(anyhow::Error::msg)?;
    for (marked, label, expected) in [
        (
            "package Dog; sub speak {} my $dog = Dog->new(); $dog->sp|eak;",
            "speak",
            "package Dog; sub speak {} my $dog = Dog->new(); $dog->speak();",
        ),
        ("$unknown->AU|TOLOAD_42, 1;", "AUTOLOAD", "$unknown->AUTOLOAD(), 1;"),
        (
            "use HTTP::Tiny; my $h = HTTP::Tiny->new(); $h->po|st_form;",
            "post_form",
            "use HTTP::Tiny; my $h = HTTP::Tiny->new(); $h->post_form();",
        ),
        (
            "use DBI; my $dbh = DBI->connect('dbi:SQLite:'); $dbh->i|sa;",
            "isa",
            "use DBI; my $dbh = DBI->connect('dbi:SQLite:'); $dbh->isa();",
        ),
        ("my $r = Remote->new(); $r->le|ap;", "leap", "my $r = Remote->new(); $r->leap();"),
        ("my $r = Remote->new(); $r->|leap;", "leap", "my $r = Remote->new(); $r->leap();"),
        ("my $r = Remote->new(); $r->leap|;", "leap", "my $r = Remote->new(); $r->leap();"),
        (
            "print \"😀\"; my $r = Remote->new(); $r->le|ap\r\n",
            "leap",
            "print \"😀\"; my $r = Remote->new(); $r->leap()\r\n",
        ),
    ] {
        let position = marked.find('|').ok_or_else(|| anyhow!("fixture missing cursor marker"))?;
        let source = marked.replacen('|', "", 1);
        let ast = Parser::new(&source).parse()?;
        let provider =
            CompletionProvider::new_with_index_and_source(&ast, &source, Some(Arc::clone(&index)));
        let items = provider.get_completions_with_path(
            &source,
            position,
            Some("file:///workspace/caller.pl"),
        );
        let item = items
            .iter()
            .find(|item| item.label == label)
            .ok_or_else(|| anyhow!("missing {label} for {marked}: {items:?}"))?;
        let (start, end) = item.text_edit_range.ok_or_else(|| anyhow!("missing edit: {item:?}"))?;
        let insertion =
            item.insert_text.as_deref().ok_or_else(|| anyhow!("missing insertion: {item:?}"))?;
        let mut applied = source.clone();
        applied.replace_range(start..end, insertion);
        ensure!(applied == expected, "{marked}: actual {applied:?}, expected {expected:?}");
    }
    Ok(())
}

#[test]
fn indirect_object_method_edit_geometry_remains_prefix_only() -> Result<()> {
    let index = Arc::new(WorkspaceIndex::new());
    index
        .index_file(
            Url::parse("file:///workspace/Remote.pm")?,
            "package Remote; sub leap {} 1;".to_string(),
        )
        .map_err(anyhow::Error::msg)?;
    let source = "process Remote";
    let ast = Parser::new(source).parse()?;
    let provider = CompletionProvider::new_with_index_and_source(&ast, source, Some(index));
    let items = provider.get_completions(source, 3);
    let item = items
        .iter()
        .find(|item| item.label == "leap")
        .ok_or_else(|| anyhow!("indirect method fixture did not route: {items:?}"))?;
    // Indirect dispatch synthesizes an arrow context without an arrow in source.
    // Preserve its existing contract while repairing actual arrow-token extent.
    ensure!(item.text_edit_range == Some((0, 3)), "indirect edit changed: {item:?}");
    ensure!(item.insert_text.as_deref() == Some("leap"), "indirect insertion changed: {item:?}");
    Ok(())
}
