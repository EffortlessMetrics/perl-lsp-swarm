//! Production-path receipt for the bounded DBIx::QuickORM adapter.

use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

#[test]
fn workspace_index_receipts_keep_quickorm_virtual_and_source_symbols_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/User.pm")?;
    let source = r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';

table "users" => sub {
    column id;
    columns qw/name email/;
};
1;
"#;

    index.index_file(uri.clone(), source.to_string())?;

    let shard =
        index.file_fact_shard(uri.as_str()).ok_or("WorkspaceIndex did not retain a fact shard")?;
    let entity = shard
        .entities
        .iter()
        .find(|entity| entity.canonical_name == "MyApp::Schema::User::qorm_table")
        .ok_or("missing QuickORM qorm_table entity")?;
    assert_eq!(entity.provenance, perl_semantic_facts::Provenance::FrameworkSynthesis);
    assert_eq!(entity.confidence, perl_semantic_facts::Confidence::Medium);

    let anchor_id = entity.anchor_id.ok_or("entity lacks anchor")?;
    let anchor = shard
        .anchors
        .iter()
        .find(|anchor| anchor.id == anchor_id)
        .ok_or("missing QuickORM source anchor")?;
    assert_eq!(anchor.provenance, perl_semantic_facts::Provenance::FrameworkSynthesis);
    assert!(anchor.span_end_byte > anchor.span_start_byte);

    let generated = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].name, "qorm_table [generated/framework]");
    assert_eq!(generated[0].qualified_name.as_deref(), Some("MyApp::Schema::User::qorm_table"));
    assert_eq!(generated[0].uri, uri.as_str());

    assert!(
        index.search_source_symbols("qorm_table", None).is_empty(),
        "virtual QuickORM member must not be reported as a source declaration"
    );
    for manual_name in ["users", "id", "name", "email"] {
        assert!(
            index.search_generated_workspace_symbols(manual_name, None).is_empty(),
            "manual QuickORM metadata must not synthesize '{manual_name}'"
        );
    }

    Ok(())
}

#[test]
fn workspace_index_blocks_dynamic_quickorm_type_calls() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Dynamic.pm")?;
    let source = r#"
package MyApp::Schema::Dynamic;
use DBIx::QuickORM type => table();
table "users" => sub {};
1;
"#;

    index.index_file(uri.clone(), source.to_string())?;

    let shard =
        index.file_fact_shard(uri.as_str()).ok_or("WorkspaceIndex did not retain a fact shard")?;
    assert!(
        shard
            .entities
            .iter()
            .all(|entity| entity.canonical_name != "MyApp::Schema::Dynamic::qorm_table"),
        "runtime import configuration must not create a generated member fact"
    );
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "runtime import configuration must not reach generated workspace symbols"
    );

    Ok(())
}
