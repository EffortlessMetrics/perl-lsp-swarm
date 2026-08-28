//! Bounded DBIx::QuickORM table-package semantic adaptation.
//!
//! In `type => 'table'` mode, a direct package-level `table` builder installs
//! one fixed method, `qorm_table`, on the containing package. This producer
//! emits only that source-backed generated member.
//!
//! Manual `column` and `columns` declarations are schema metadata. Named field
//! and link accessors are intentionally absent here because upstream creates
//! them through database-backed `autofill`/`autorow`, not from manual column
//! declarations alone.

use super::generated_member_extractor_core::GeneratedMemberFact;
use crate::{Node, NodeKind};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, Provenance,
};

const QUICKORM_MODULE: &str = "DBIx::QuickORM";
const QORM_TABLE_MEMBER: &str = "qorm_table";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickOrmImportShape {
    Bare,
    UnfilteredOrm,
    UnfilteredTable,
    Dynamic,
}

/// Extract the fixed `qorm_table` member installed by a direct, unfiltered
/// QuickORM table-package `table` builder.
pub(super) fn extract_generated_member_facts(
    ast: &Node,
    file_id: FileId,
) -> Vec<GeneratedMemberFact> {
    let mut facts = Vec::new();
    let mut context = WalkContext::default();

    match &ast.kind {
        NodeKind::Program { statements } => {
            walk_direct_statements(statements, file_id, &mut context, &mut facts);
        }
        _ => walk_direct_statement(ast, file_id, &mut context, &mut facts),
    }

    facts
}

#[derive(Debug, Clone, Default)]
struct WalkContext {
    current_package: Option<String>,
    table_package_active: bool,
}

fn walk_direct_statements(
    statements: &[Node],
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
) {
    for statement in statements {
        walk_direct_statement(statement, file_id, context, facts);
    }
}

fn walk_direct_statement(
    node: &Node,
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            walk_direct_statements(statements, file_id, context, facts);
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved = context.clone();
                context.current_package = Some(name.clone());
                context.table_package_active = false;

                if let NodeKind::Block { statements } = &block.kind {
                    walk_direct_statements(statements, file_id, context, facts);
                }

                *context = saved;
            } else {
                context.current_package = Some(name.clone());
                context.table_package_active = false;
            }
        }
        NodeKind::Use { module, args, .. } if module == QUICKORM_MODULE => {
            context.table_package_active =
                classify_import_shape(args) == QuickOrmImportShape::UnfilteredTable;
        }
        NodeKind::No { module, .. } if module == QUICKORM_MODULE => {
            context.table_package_active = false;
        }
        NodeKind::ExpressionStatement { expression } if context.table_package_active => {
            if let Some(anchor) = direct_table_builder_anchor(expression) {
                let package = context.current_package.as_deref().unwrap_or("main");
                push_qorm_table_fact(package, anchor, file_id, facts);

                // A completed table-package build removes the imported DSL
                // functions, so a second direct builder has no QuickORM
                // authority.
                context.table_package_active = false;
            }
        }
        // Runtime-controlled and bare lexical blocks are not package-level
        // table declarations. Do not recurse into them or let their package or
        // framework state escape into the containing package.
        _ => {}
    }
}

/// Return the literal table-name operand that anchors a reviewed package-level
/// `table NAME => BUILDER` declaration.
fn direct_table_builder_anchor(expression: &Node) -> Option<&Node> {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return None;
    };
    if name != "table" {
        return None;
    }

    let anchor = static_table_name_anchor(args.first()?)?;
    if !args.iter().skip(1).any(contains_builder_body) {
        return None;
    }

    Some(anchor)
}

fn static_table_name_anchor(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::String {
            value,
            interpolated: false,
        } if !value.trim().is_empty() => Some(node),
        NodeKind::Identifier { name } if !name.trim().is_empty() => Some(node),
        NodeKind::Binary { op, left, .. } if op == "=>" => static_table_name_anchor(left),
        _ => None,
    }
}

fn contains_builder_body(node: &Node) -> bool {
    if matches!(
        node.kind,
        NodeKind::Subroutine { .. } | NodeKind::Block { .. } | NodeKind::HashLiteral { .. }
    ) {
        return true;
    }

    node.children().into_iter().any(contains_builder_body)
}

fn push_qorm_table_fact(
    package: &str,
    source: &Node,
    file_id: FileId,
    facts: &mut Vec<GeneratedMemberFact>,
) {
    let canonical_name = format!("{package}::{QORM_TABLE_MEMBER}");
    if facts
        .iter()
        .any(|fact| fact.entity.canonical_name == canonical_name)
    {
        return;
    }

    let span_start = source.location.start;
    let span_end = source.location.end;
    let entity_id = EntityId(stable_id(
        "quickorm-generated-member-entity",
        file_id,
        span_start,
        package,
        QORM_TABLE_MEMBER,
    ));
    let anchor_id = AnchorId(stable_id(
        "quickorm-generated-member-anchor",
        file_id,
        span_start,
        package,
        QORM_TABLE_MEMBER,
    ));

    let anchor = AnchorFact {
        id: anchor_id,
        file_id,
        span_start_byte: span_start.min(u32::MAX as usize) as u32,
        span_end_byte: span_end.min(u32::MAX as usize) as u32,
        scope_id: None,
        provenance: Provenance::FrameworkSynthesis,
        confidence: Confidence::Medium,
    };
    let entity = EntityFact {
        id: entity_id,
        kind: EntityKind::GeneratedMember,
        canonical_name,
        anchor_id: Some(anchor_id),
        scope_id: None,
        provenance: Provenance::FrameworkSynthesis,
        confidence: Confidence::Medium,
    };

    facts.push(GeneratedMemberFact { entity, anchor });
}

fn classify_import_shape(args: &[String]) -> QuickOrmImportShape {
    if args.is_empty() {
        return QuickOrmImportShape::Bare;
    }

    let raw_args = args.join(" ");
    if raw_args
        .chars()
        .any(|ch| matches!(ch, '{' | '}' | '[' | ']'))
    {
        return QuickOrmImportShape::Dynamic;
    }

    let tokens = normalized_import_tokens(args);
    match tokens.as_slice() {
        [key, value] if key == "type" && value == "orm" => QuickOrmImportShape::UnfilteredOrm,
        [key, value] if key == "type" && value == "table" => {
            QuickOrmImportShape::UnfilteredTable
        }
        _ => QuickOrmImportShape::Dynamic,
    }
}

/// Normalize both parser-produced arguments (`["type", "table"]`) and
/// hand-built/source-token arguments that still include `=>`.
fn normalized_import_tokens(args: &[String]) -> Vec<String> {
    let mut joined = args.join(" ").replace("=>", " => ");
    for delimiter in [',', '(', ')', '[', ']', '{', '}'] {
        joined = joined.replace(delimiter, " ");
    }

    joined
        .split_whitespace()
        .map(|token| token.trim_matches('\'').trim_matches('"').to_string())
        .filter(|token| !token.is_empty() && token != "=>")
        .collect()
}

fn stable_id(label: &str, file_id: FileId, anchor_start: usize, package: &str, name: &str) -> u64 {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = FNV_OFFSET;
    for byte in label
        .as_bytes()
        .iter()
        .chain(file_id.0.to_le_bytes().iter())
        .chain((anchor_start as u64).to_le_bytes().iter())
        .chain(package.as_bytes())
        .chain(name.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn parse_source(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
        let mut parser = Parser::new(source);
        parser
            .parse()
            .map_err(|error| format!("failed to parse QuickORM source: {error:?}").into())
    }

    fn generated_facts_from_source(
        source: &str,
    ) -> Result<Vec<GeneratedMemberFact>, Box<dyn std::error::Error>> {
        let ast = parse_source(source)?;
        Ok(
            super::super::generated_member_extractor::extract_generated_member_facts(
                &ast,
                FileId(2),
            ),
        )
    }

    fn canonical_names(facts: &[GeneratedMemberFact]) -> Vec<&str> {
        let mut names: Vec<_> = facts
            .iter()
            .map(|fact| fact.entity.canonical_name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn parser_normalizes_table_import_to_two_tokens(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ast = parse_source(
            "package User; use DBIx::QuickORM type => 'table'; table users => sub {};",
        )?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected program AST".into());
        };
        let args = statements
            .iter()
            .find_map(|statement| match &statement.kind {
                NodeKind::Use { module, args, .. } if module == QUICKORM_MODULE => Some(args),
                _ => None,
            })
            .ok_or("missing QuickORM use node")?;
        let normalized: Vec<&str> = args.iter().map(String::as_str).collect();

        assert_eq!(normalized, ["type", "table"]);
        assert_eq!(
            classify_import_shape(args),
            QuickOrmImportShape::UnfilteredTable
        );
        Ok(())
    }

    #[test]
    fn table_package_emits_only_fixed_qorm_table_member(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column id;
    columns qw/name email/;
};
1;
"#;
        let facts = generated_facts_from_source(source)?;
        let names = canonical_names(&facts);

        assert_eq!(names, vec!["MyApp::Schema::User::qorm_table"]);
        let fact = facts.first().ok_or("missing qorm_table fact")?;
        assert_eq!(fact.entity.kind, EntityKind::GeneratedMember);
        assert_eq!(fact.entity.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.entity.confidence, Confidence::Medium);
        assert_eq!(fact.anchor.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.anchor.confidence, Confidence::Medium);
        assert_eq!(
            source.get(
                fact.anchor.span_start_byte as usize..fact.anchor.span_end_byte as usize
            ),
            Some("users")
        );
        Ok(())
    }

    #[test]
    fn non_table_package_profiles_emit_nothing() -> Result<(), Box<dyn std::error::Error>> {
        for source in [
            "package Plain; table before => sub {}; use DBIx::QuickORM type => 'table';",
            "package Bare; use DBIx::QuickORM; table bare => sub {};",
            "package Orm; use DBIx::QuickORM type => 'orm'; table orm => sub {};",
            "package Filtered; use DBIx::QuickORM type => 'table', only => ['table']; table filtered => sub {};",
            "package Disabled; use DBIx::QuickORM type => 'table'; no DBIx::QuickORM; table disabled => sub {};",
            "package View; use DBIx::QuickORM type => 'table'; view active => sub {};",
        ] {
            assert!(
                generated_facts_from_source(source)?.is_empty(),
                "unexpected QuickORM generated fact for: {source}"
            );
        }
        Ok(())
    }

    #[test]
    fn runtime_nested_and_incomplete_builders_emit_nothing(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for source in [
            "package Nested; use DBIx::QuickORM type => 'table'; sub later { table users => sub {}; }",
            "package Conditional; use DBIx::QuickORM type => 'table'; if ($enabled) { table users => sub {}; }",
            "package Dynamic; use DBIx::QuickORM type => 'table'; table $name => sub {};",
            "package Incomplete; use DBIx::QuickORM type => 'table'; table users;",
        ] {
            assert!(
                generated_facts_from_source(source)?.is_empty(),
                "unexpected QuickORM generated fact for: {source}"
            );
        }
        Ok(())
    }

    #[test]
    fn bare_lexical_block_does_not_leak_package_or_framework_state(
    ) -> Result<(), Box<dyn std::error::Error>> {
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

        assert_eq!(
            canonical_names(&facts),
            vec!["MyApp::Schema::User::qorm_table"]
        );
        Ok(())
    }
}
