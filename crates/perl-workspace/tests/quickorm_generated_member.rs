use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use perl_workspace::{NodeKind, Parser};
use url::Url;

#[test]
fn quickorm_qorm_table_reaches_generated_member_surfaces() -> Result<(), Box<dyn std::error::Error>>
{
    let source = r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column id;
    columns qw/name email/;
};
1;
"#;

    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| {
        std::io::Error::other(format!("failed to parse QuickORM fixture: {error:?}"))
    })?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected program AST".into());
    };
    let use_args = statements
        .iter()
        .find_map(|statement| match &statement.kind {
            NodeKind::Use { module, args, .. } if module == "DBIx::QuickORM" => Some(args),
            _ => None,
        })
        .ok_or("missing DBIx::QuickORM use node")?;
    let normalized_use_args: Vec<&str> = use_args.iter().map(String::as_str).collect();
    assert_eq!(normalized_use_args, ["type", "'table'"]);

    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///lib/MyApp/Schema/User.pm")?;
    index.index_file(uri, source.to_string()).map_err(std::io::Error::other)?;

    let generated = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(generated.len(), 1, "expected one generated qorm_table symbol: {generated:?}");
    let symbol = &generated[0];
    assert_eq!(symbol.name, "qorm_table [generated/framework]");
    assert_eq!(symbol.qualified_name.as_deref(), Some("MyApp::Schema::User::qorm_table"));
    assert_eq!(symbol.container_name.as_deref(), Some("MyApp::Schema::User [generated/framework]"));
    assert_eq!(
        source.get(symbol.range.start.byte..symbol.range.end.byte),
        Some("users"),
        "generated fact must anchor to the literal table declaration"
    );

    assert!(
        index.search_source_symbols("qorm_table", None).is_empty(),
        "qorm_table must not enter the exact source-symbol slice"
    );

    let members = index.get_generated_package_members("MyApp::Schema::User");
    let member_names: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
    assert_eq!(member_names, ["qorm_table"]);
    for unearned_member in ["users", "id", "name", "email"] {
        assert!(
            !member_names.contains(&unearned_member),
            "manual schema metadata must not become a generated row member: {unearned_member}"
        );
    }

    Ok(())
}
