use perl_workspace::WorkspaceIndex;
use std::error::Error;

#[test]
fn quickorm_table_package_fact_reaches_generated_member_surfaces() -> Result<(), Box<dyn Error>> {
    let index = WorkspaceIndex::new();
    let package = "My::ORM::Table::User";
    let uri = url::Url::parse("file:///lib/My/ORM/Table/User.pm")?;
    let source = r#"package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
table user => sub {
    column id => sub { identity; };
    columns qw(name email);
};
1;
"#;

    index.index_file(uri, source.to_string())?;

    let members = index.get_generated_package_members(package);
    let names: Vec<_> = members.iter().map(|member| member.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["qorm_table"],
        "the table package must expose only QuickORM's statically proven generated method"
    );

    let symbols = index.search_generated_workspace_symbols("qorm_table", None);
    assert_eq!(symbols.len(), 1);
    let symbol = &symbols[0];
    assert_eq!(symbol.name, "qorm_table [generated/framework]");
    assert_eq!(symbol.qualified_name.as_deref(), Some("My::ORM::Table::User::qorm_table"));
    assert_eq!(symbol.container_name.as_deref(), Some("My::ORM::Table::User [generated/framework]"));
    assert!(
        symbol.range.end.byte > symbol.range.start.byte,
        "the generated method must retain the source table-declaration anchor"
    );

    assert!(
        index.search_source_symbols("qorm_table", None).is_empty(),
        "QuickORM's generated method must not enter the exact source-symbol slice"
    );
    assert!(
        index.search_generated_workspace_symbols("id", None).is_empty(),
        "manual QuickORM columns are schema metadata, not generated row methods"
    );
    Ok(())
}
