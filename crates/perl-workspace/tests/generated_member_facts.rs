//! Workspace fact extraction tests for framework-generated members.

use perl_semantic_facts::{Confidence, EntityKind, Provenance};
use perl_workspace::semantic::queries::{QueryContext, SemanticQueries};
use perl_workspace::workspace::workspace_index::{SymbolKind, WorkspaceIndex};
use std::io;

#[test]
fn moo_has_generates_member_entity_with_attribute_anchor() -> Result<(), Box<dyn std::error::Error>>
{
    let index = WorkspaceIndex::new();
    let uri = "file:///generated-member.pm";
    let source = "\
package Accuracy::GeneratedAccessor;

use Moo;

has name => (is => 'ro');

1;
";
    let expected_start =
        source.find("name =>").ok_or_else(|| io::Error::other("missing attribute name"))?;
    let expected_end = expected_start + "name".len();

    index.index_file(url::Url::parse(uri)?, source.to_string())?;
    let shard = index.file_fact_shard(uri).ok_or_else(|| io::Error::other("missing fact shard"))?;
    let entity = shard
        .entities
        .iter()
        .find(|entity| entity.canonical_name == "Accuracy::GeneratedAccessor::name")
        .ok_or_else(|| io::Error::other("missing generated member entity"))?;

    assert_eq!(entity.kind, EntityKind::GeneratedMember);
    assert_eq!(entity.provenance, Provenance::FrameworkSynthesis);
    assert_eq!(entity.confidence, Confidence::Medium);

    let anchor_id =
        entity.anchor_id.ok_or_else(|| io::Error::other("missing generated member anchor"))?;
    let anchor = shard
        .anchors
        .iter()
        .find(|anchor| anchor.id == anchor_id)
        .ok_or_else(|| io::Error::other("missing anchor fact"))?;
    assert_eq!(anchor.span_start_byte, expected_start as u32);
    assert_eq!(anchor.span_end_byte, expected_end as u32);
    assert_eq!(anchor.provenance, Provenance::FrameworkSynthesis);
    assert_eq!(anchor.confidence, Confidence::Medium);

    Ok(())
}

#[test]
fn plain_has_without_framework_does_not_generate_member_entity()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///plain-has.pm";
    let source = "\
package Plain::Package;

has name => (is => 'ro');

1;
";

    index.index_file(url::Url::parse(uri)?, source.to_string())?;
    let shard = index.file_fact_shard(uri).ok_or_else(|| io::Error::other("missing fact shard"))?;

    assert!(
        shard.entities.iter().all(|entity| entity.kind != EntityKind::GeneratedMember),
        "plain has without Moo/Moose/Mouse must not synthesize generated member facts"
    );

    Ok(())
}

#[test]
fn dbix_class_members_feed_method_candidates_and_definition_lookup()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///generated-dbix.pm";
    let source = "package MyApp::Schema::Result::Author;\n\
use DBIx::Class(:Core);\n\
__PACKAGE__->add_columns(qw(id name));\n\
__PACKAGE__->has_many('posts' => 'MyApp::Schema::Result::Post', 'author_id');\n\
1;\n";

    index.index_file(url::Url::parse(uri)?, source.to_string())?;
    let shard = index.file_fact_shard(uri).ok_or_else(|| io::Error::other("missing fact shard"))?;
    for name in ["id", "posts"] {
        let canonical_name = format!("MyApp::Schema::Result::Author::{name}");
        let entity = shard
            .entities
            .iter()
            .find(|entity| entity.canonical_name == canonical_name)
            .ok_or_else(|| {
            io::Error::other(format!("missing generated member {canonical_name}"))
        })?;
        assert_eq!(entity.kind, EntityKind::GeneratedMember);
        assert_eq!(entity.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(entity.confidence, Confidence::Medium);
    }

    let candidates = index
        .with_semantic_queries_for_uri(uri, |_, queries| {
            queries.method_candidates("MyApp::Schema::Result::Author", "posts")
        })
        .ok_or_else(|| io::Error::other("missing semantic query facade"))?;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].canonical_name, "MyApp::Schema::Result::Author::posts");

    let definitions = index
        .with_semantic_queries_for_uri(uri, |file_id, queries| {
            queries.definitions(
                "MyApp::Schema::Result::Author::posts",
                &QueryContext::new(file_id, None, None),
            )
        })
        .ok_or_else(|| io::Error::other("missing semantic query facade"))?;
    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    let expected_start = source.find("__PACKAGE__->has_many").ok_or("missing has_many")?;
    let anchor = shard
        .anchors
        .iter()
        .find(|anchor| anchor.id == definition.anchor_id)
        .ok_or_else(|| io::Error::other("missing generated relationship anchor"))?;
    assert_eq!(anchor.span_start_byte, expected_start as u32);
    Ok(())
}

#[test]
fn dbix_class_members_are_exposed_as_package_methods() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///generated-dbix-package-members.pm";
    let source = "package MyApp::Schema::Result::Author;\n\
use DBIx::Class(:Core);\n\
__PACKAGE__->add_columns(qw(id name));\n\
__PACKAGE__->has_many('posts' => 'MyApp::Schema::Result::Post');\n\
1;\n";

    index.index_file(url::Url::parse(uri)?, source.to_string())?;
    let members = index.get_package_members("MyApp::Schema::Result::Author");

    for name in ["id", "name", "posts"] {
        let member = members
            .iter()
            .find(|member| member.name == name)
            .ok_or_else(|| io::Error::other(format!("missing generated package member {name}")))?;
        assert_eq!(member.kind, SymbolKind::Method);
        assert_eq!(member.container_name.as_deref(), Some("MyApp::Schema::Result::Author"));
        assert_eq!(member.uri, uri);
    }

    Ok(())
}
