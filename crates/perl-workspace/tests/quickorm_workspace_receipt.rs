//! Production-path receipt for the bounded DBIx::QuickORM adapter.

use perl_workspace::semantic::queries::{DynamicCallableEvidence, SemanticQueries};
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

    index.index_initial_file(uri.clone(), source.to_string())?;

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

    let query_offset = u32::try_from(source.len())?;
    let dynamic_evidence = index
        .with_semantic_queries_for_uri(uri.as_str(), |file_id, queries| {
            queries.dynamic_callable_may_be_visible_at(file_id, query_offset, "unknown_callable")
        })
        .ok_or("literal QuickORM file was not available to semantic queries")?;
    assert!(
        dynamic_evidence.is_none(),
        "literal table-package configuration must not be stored as a dynamic import"
    );

    Ok(())
}

#[test]
fn workspace_index_receipts_cover_bare_block_package_restore()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/BareBlock.pm")?;
    let source = r#"
package MyApp::Schema::Outer;
use DBIx::QuickORM type => 'table';
{
    package MyApp::Schema::Inner;
    use DBIx::QuickORM type => 'table';
    table inner_users => sub {};
}
table outer_users => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    let generated = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(generated.len(), 2);
    assert!(
        generated
            .iter()
            .any(|symbol| symbol.qualified_name.as_deref()
                == Some("MyApp::Schema::Inner::qorm_table"))
    );
    assert!(
        generated
            .iter()
            .any(|symbol| symbol.qualified_name.as_deref()
                == Some("MyApp::Schema::Outer::qorm_table"))
    );
    Ok(())
}

#[test]
fn workspace_index_receipts_keep_perl_interpolation_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, table_name) in [
        ("Namespaced", "$::prefix_users"),
        ("Special", "$^O"),
        ("Match", "$&"),
        ("Postmatch", "$'"),
        ("Prematch", "$`"),
        ("MatchedIndexes", "@+"),
    ] {
        let index = WorkspaceIndex::new();
        let uri = Url::parse(&format!("file:///lib/MyApp/Schema/{name}.pm"))?;
        let source = format!(
            "package MyApp::Schema::{name};\nuse DBIx::QuickORM type => 'table';\ntable \"{table_name}\" => sub {{}};\n1;\n"
        );

        index.index_initial_file(uri, source)?;
        assert!(
            index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
            "interpolated table name {table_name} must not reach generated workspace symbols"
        );
    }
    Ok(())
}

#[test]
fn workspace_index_consumes_current_package_qualified_builder_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Qualified.pm")?;
    let source = r#"
package MyApp::Schema::Qualified;
use DBIx::QuickORM type => 'table';
MyApp::Schema::Qualified::table "qualified_users" => sub {};
table "later_users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "a current-package qualified table call must consume authority without earning a direct package fact"
    );
    Ok(())
}

#[test]
fn workspace_index_ignores_unrelated_qualified_builder_for_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/UnrelatedQualified.pm")?;
    let source = r#"
package MyApp::Schema::UnrelatedQualified;
use DBIx::QuickORM type => 'table';
Other::table "other_users" => sub {};
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    let generated = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(generated.len(), 1);
    assert_eq!(
        generated[0].qualified_name.as_deref(),
        Some("MyApp::Schema::UnrelatedQualified::qorm_table")
    );
    Ok(())
}

#[test]
fn workspace_index_blocks_competing_imported_table_builder()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Competing.pm")?;
    let source = r#"
package MyApp::Schema::Competing;
use DBIx::QuickORM type => 'table';
use Other::DSL qw(table);
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "a competing imported table builder must invalidate QuickORM authority in the production index"
    );
    Ok(())
}

#[test]
fn workspace_index_preserves_authority_after_zero_argument_qualified_call()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/ZeroArgQualified.pm")?;
    let source = r#"
package MyApp::Schema::ZeroArgQualified;
use DBIx::QuickORM type => 'table';
MyApp::Schema::ZeroArgQualified::table();
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert_eq!(index.search_generated_workspace_symbols("qorm_table", None).len(), 1);
    Ok(())
}

#[test]
fn workspace_index_later_import_reestablishes_shadowed_builder_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/LaterImport.pm")?;
    let source = r#"
package MyApp::Schema::LaterImport;
sub table {}
use DBIx::QuickORM type => 'table';
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert_eq!(index.search_generated_workspace_symbols("qorm_table", None).len(), 1);
    Ok(())
}

#[test]
fn workspace_index_blocks_required_competing_import_call() -> Result<(), Box<dyn std::error::Error>>
{
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/RequiredImport.pm")?;
    let source = r#"
package MyApp::Schema::RequiredImport;
use DBIx::QuickORM type => 'table';
require Other::DSL;
Other::DSL->import('table');
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());
    Ok(())
}

#[test]
fn workspace_index_blocks_competing_view_method_import() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/CompetingView.pm")?;
    let source = r#"
package MyApp::Schema::CompetingView;
use DBIx::QuickORM type => 'table';
Other::DSL->import(qw(view));
view "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());
    Ok(())
}

#[test]
fn workspace_index_blocks_hash_shaped_competing_import() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/CompetingHashImport.pm")?;
    let source = r#"
package MyApp::Schema::CompetingHashImport;
use DBIx::QuickORM type => 'table';
Other::DSL->import(table => sub {});
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "an unknown hash-shaped competing importer must invalidate authority in the production index"
    );
    Ok(())
}

#[test]
fn workspace_index_blocks_nested_qualified_builder_initializer()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/NestedInitializer.pm")?;
    let source = r#"
package MyApp::Schema::NestedInitializer;
use DBIx::QuickORM type => 'table';
my $builder = MyApp::Schema::NestedInitializer::table "nested_users" => sub {};
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());
    Ok(())
}

#[test]
fn workspace_index_receipts_cover_package_reentry_and_fresh_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Reentry.pm")?;
    let source = r#"
package MyApp::Schema::A;
use DBIx::QuickORM type => 'table';
table "first_users" => sub {};
package MyApp::Schema::B;
use DBIx::QuickORM type => 'table';
table "other_users" => sub {};
package MyApp::Schema::A;
use DBIx::QuickORM type => 'table';
table "second_users" => sub {};
1;
"#;

    index.index_initial_file(uri.clone(), source.to_string())?;
    let generated = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(generated.len(), 2);
    assert!(
        generated
            .iter()
            .any(|symbol| symbol.qualified_name.as_deref() == Some("MyApp::Schema::A::qorm_table"))
    );
    assert!(
        generated
            .iter()
            .any(|symbol| symbol.qualified_name.as_deref() == Some("MyApp::Schema::B::qorm_table"))
    );
    Ok(())
}

#[test]
fn workspace_index_blocks_bare_quickorm_import() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Bare.pm")?;
    let source = r#"
package MyApp::Schema::Bare;
use DBIx::QuickORM;
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri.clone(), source.to_string())?;
    let shard =
        index.file_fact_shard(uri.as_str()).ok_or("WorkspaceIndex did not retain a fact shard")?;
    assert!(
        shard
            .entities
            .iter()
            .all(|entity| entity.canonical_name != "MyApp::Schema::Bare::qorm_table"),
        "bare QuickORM import must not create a generated member fact"
    );
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "bare QuickORM import must not reach generated workspace symbols"
    );
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

    index.index_initial_file(uri.clone(), source.to_string())?;

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

    let query_offset = u32::try_from(source.len())?;
    let dynamic_evidence = index
        .with_semantic_queries_for_uri(uri.as_str(), |file_id, queries| {
            queries.dynamic_callable_may_be_visible_at(file_id, query_offset, "unknown_callable")
        })
        .ok_or("dynamic QuickORM file was not available to semantic queries")?
        .ok_or("dynamic QuickORM configuration did not reach ImportExportIndex queries")?;
    match dynamic_evidence {
        DynamicCallableEvidence::DynamicImport { module, .. } => {
            assert_eq!(module, "DBIx::QuickORM");
        }
        DynamicCallableEvidence::EvalSub { .. } => {
            return Err("QuickORM import was misclassified as eval-sub evidence".into());
        }
    }

    Ok(())
}

#[test]
fn workspace_index_blocks_quickorm_comma_without_fat_arrow()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Comma.pm")?;
    let source = r#"
package MyApp::Schema::Comma;
sub type () { 'only' };
use DBIx::QuickORM type, 'table';
table "users" => sub {};
1;
"#;

    index.index_initial_file(uri, source.to_string())?;
    assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());
    Ok(())
}

#[test]
fn workspace_index_blocks_quickorm_rename_skip_and_unknown_options()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, options) in [
        ("Rename", "rename => { table => 'make_table' }"),
        ("Skip", "skip => ['table']"),
        ("Unknown", "unknown => 'value'"),
    ] {
        let index = WorkspaceIndex::new();
        let uri = Url::parse(&format!("file:///lib/MyApp/Schema/{name}.pm"))?;
        let source = format!(
            "package MyApp::Schema::{name};\nuse DBIx::QuickORM type => 'table', {options};\ntable users => sub {{}};\n1;\n"
        );

        index.index_initial_file(uri.clone(), source.clone())?;
        assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());

        let query_offset = u32::try_from(source.len())?;
        let dynamic_evidence = index
            .with_semantic_queries_for_uri(uri.as_str(), |file_id, queries| {
                queries.dynamic_callable_may_be_visible_at(
                    file_id,
                    query_offset,
                    "unknown_callable",
                )
            })
            .ok_or("QuickORM option file was not available to semantic queries")?
            .ok_or("QuickORM option boundary did not reach ImportExportIndex queries")?;
        match dynamic_evidence {
            DynamicCallableEvidence::DynamicImport { module, .. } => {
                assert_eq!(module, "DBIx::QuickORM");
            }
            DynamicCallableEvidence::EvalSub { .. } => {
                return Err("QuickORM option was misclassified as eval-sub evidence".into());
            }
        }
    }
    Ok(())
}

#[test]
fn workspace_index_blocks_competing_quote_like_imports() -> Result<(), Box<dyn std::error::Error>> {
    for (name, delimiter) in [
        ("Slash", "/table/"),
        ("Paren", "(table)"),
        ("Bracket", "[table]"),
        ("Brace", "{table}"),
        ("Angle", "<table>"),
    ] {
        let index = WorkspaceIndex::new();
        let uri = Url::parse(&format!("file:///lib/MyApp/Schema/Competing{name}.pm"))?;
        let source = format!(
            "package MyApp::Schema::Competing{name};\nuse DBIx::QuickORM type => 'table';\nuse Other::DSL qw{delimiter};\ntable users => sub {{}};\n1;\n",
            name = name,
            delimiter = delimiter
        );

        index.index_initial_file(uri, source)?;
        assert!(index.search_generated_workspace_symbols("qorm_table", None).is_empty());
    }
    Ok(())
}

#[test]
fn workspace_index_invalidates_stale_qorm_table_after_dynamic_reconfiguration()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/Reconfigured.pm")?;
    let source = r#"
package MyApp::Schema::Reconfigured;
use DBIx::QuickORM type => 'table';
table first => sub {};
use DBIx::QuickORM type => table();
table second => sub {};
1;
"#;

    index.index_initial_file(uri.clone(), source.to_string())?;
    let shard =
        index.file_fact_shard(uri.as_str()).ok_or("WorkspaceIndex did not retain a fact shard")?;
    assert!(
        shard
            .entities
            .iter()
            .all(|entity| entity.canonical_name != "MyApp::Schema::Reconfigured::qorm_table"),
        "dynamic QuickORM reconfiguration must invalidate the prior generated fact"
    );
    assert!(
        index.search_generated_workspace_symbols("qorm_table", None).is_empty(),
        "dynamic QuickORM reconfiguration must not retain a stale workspace symbol"
    );
    Ok(())
}
