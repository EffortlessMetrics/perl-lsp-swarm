//! Non-published DBIx::QuickORM table-column candidate extraction.
//!
//! This module proves one bounded successor-ORM subset without admitting it to
//! canonical shards or live providers: explicit DBIx::QuickORM table classes
//! with default DSL names and statically named `column` or `columns`
//! declarations inside the table builder.
//!
//! These declarations are modeled as row fields, not generated methods.
//! DBIx::QuickORM rows expose ordinary columns through `field($name)`; named
//! field accessors are an `autorow` feature and remain outside this candidate.
//! Runtime schema fill, generated row classes, import-symbol customization,
//! naming hooks, relationship accessors, dynamic identities, and edit
//! authorization also remain blocked.

use crate::{Node, NodeKind};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, Provenance,
};

#[derive(Debug, Clone, Default)]
struct QuickOrmWalkCtx {
    current_package: Option<String>,
    explicit_table_class_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameCandidate {
    name: String,
    span_start: usize,
    span_end: usize,
}

/// Source-backed DBIx::QuickORM column candidate plus its declaration anchor.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuickOrmColumnFact {
    pub(crate) entity: EntityFact,
    pub(crate) anchor: AnchorFact,
}

/// Extract the bounded DBIx::QuickORM column candidates without publishing them.
///
/// There is intentionally no non-test caller while canonical admission and
/// provider behavior remain blocked. A later promotion must add an explicit
/// consumer and receipt rather than silently joining the generated-member
/// stream.
#[allow(dead_code)]
pub(crate) fn extract_dbix_quickorm_column_candidates(
    ast: &Node,
    file_id: FileId,
) -> Vec<QuickOrmColumnFact> {
    let mut out = Vec::new();
    let mut ctx = QuickOrmWalkCtx::default();
    walk_quickorm(ast, file_id, &mut ctx, &mut out);
    out
}

fn walk_quickorm(
    node: &Node,
    file_id: FileId,
    ctx: &mut QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            for statement in statements {
                walk_quickorm(statement, file_id, ctx, out);
            }
        }
        NodeKind::Block { statements } => {
            let mut block_ctx = ctx.clone();
            for statement in statements {
                walk_quickorm(statement, file_id, &mut block_ctx, out);
            }
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved = ctx.clone();
                ctx.current_package = Some(name.clone());
                ctx.explicit_table_class_active = false;
                walk_quickorm(block, file_id, ctx, out);
                *ctx = saved;
            } else {
                ctx.current_package = Some(name.clone());
                ctx.explicit_table_class_active = false;
            }
        }
        NodeKind::Use { module, args, .. } if module == "DBIx::QuickORM" => {
            // Every import creates and installs a fresh builder in the caller.
            // A later plain import therefore replaces a table builder with the
            // default ORM builder instead of preserving table-class activation.
            ctx.explicit_table_class_active = is_explicit_table_class_import(args);
        }
        NodeKind::No { module, .. } if module == "DBIx::QuickORM" => {
            ctx.explicit_table_class_active = false;
        }
        NodeKind::ExpressionStatement { expression } if ctx.explicit_table_class_active => {
            // In a type=table builder, the first table() call removes the DSL
            // functions from the package. Close the candidate after any table
            // invocation, even when its identity is too dynamic to model.
            if extract_table_declaration(expression, file_id, ctx, out) {
                ctx.explicit_table_class_active = false;
            }
        }
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {}
        _ => {
            for child in node.children() {
                walk_quickorm(child, file_id, ctx, out);
            }
        }
    }
}

fn is_explicit_table_class_import(args: &[String]) -> bool {
    normalized_import_args(args) == ["type", "table"]
}

fn normalized_import_args(args: &[String]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| expand_symbol_list(arg))
        .filter_map(|arg| normalize_import_arg(&arg))
        .collect()
}

fn normalize_import_arg(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_matches(|ch| matches!(ch, '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}'))
        .trim();
    if trimmed.is_empty() || matches!(trimmed, "," | "=>") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Inspect one expression and return whether it consumed the one-shot table DSL.
fn extract_table_declaration(
    expression: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    if name != "table" {
        return false;
    }

    let builder = args.iter().rev().find(|arg| is_anonymous_builder(arg));
    if args.first().is_some_and(is_static_table_name)
        && let Some(builder) = builder
    {
        walk_table_builder(builder, file_id, ctx, out);
    }

    true
}

fn is_static_table_name(node: &Node) -> bool {
    single_static_name_candidate(node)
        .and_then(|candidate| normalize_symbol_name(&candidate.name))
        .is_some()
}

fn is_anonymous_builder(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Subroutine { name: None, .. } | NodeKind::Block { .. })
}

fn walk_table_builder(
    builder: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    match &builder.kind {
        NodeKind::Subroutine { name: None, body, .. } => walk_table_body(body, file_id, ctx, out),
        NodeKind::Block { .. } => walk_table_body(builder, file_id, ctx, out),
        _ => {}
    }
}

fn walk_table_body(
    body: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    let NodeKind::Block { statements } = &body.kind else {
        return;
    };

    for statement in statements {
        let expression = match &statement.kind {
            NodeKind::ExpressionStatement { expression } => expression.as_ref(),
            _ => statement,
        };
        extract_column_expression(expression, file_id, ctx, out);
    }
}

fn extract_column_expression(
    expression: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    match &expression.kind {
        NodeKind::FunctionCall { name, args } if name == "column" => {
            let Some(candidate) = args.first().and_then(single_static_name_candidate) else {
                return;
            };
            emit_candidate(candidate, file_id, ctx, out);
        }
        NodeKind::FunctionCall { name, args } if name == "columns" => {
            for arg in args.iter().take_while(|arg| !is_anonymous_builder(arg)) {
                for candidate in collect_static_name_candidates(arg) {
                    emit_candidate(candidate, file_id, ctx, out);
                }
            }
        }
        NodeKind::Binary { op, left, right } if op == "," => {
            extract_column_expression(left, file_id, ctx, out);
            extract_column_expression(right, file_id, ctx, out);
        }
        _ => {}
    }
}

fn emit_candidate(
    candidate: NameCandidate,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    let Some(name) = normalize_static_field_name(&candidate.name) else {
        return;
    };
    let package = ctx.current_package.as_deref().unwrap_or("main");
    push_field(package, &name, &candidate, file_id, out);
}

fn single_static_name_candidate(node: &Node) -> Option<NameCandidate> {
    let mut candidates = collect_static_name_candidates(node).into_iter();
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn collect_static_name_candidates(node: &Node) -> Vec<NameCandidate> {
    match &node.kind {
        NodeKind::String { value, interpolated: false }
        | NodeKind::Identifier { name: value } => expand_symbol_list(value)
            .into_iter()
            .map(|name| NameCandidate {
                name,
                span_start: node.location.start,
                span_end: node.location.end,
            })
            .collect(),
        NodeKind::ArrayLiteral { elements } => {
            elements.iter().flat_map(collect_static_name_candidates).collect()
        }
        NodeKind::Binary { op, left, right } if op == "," => {
            let mut names = collect_static_name_candidates(left);
            names.extend(collect_static_name_candidates(right));
            names
        }
        _ => Vec::new(),
    }
}

fn normalize_static_field_name(raw: &str) -> Option<String> {
    let name = normalize_symbol_name(raw)?;
    let mut chars = name.chars();
    let first = chars.next()?;
    if (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Some(name)
    } else {
        None
    }
}

fn normalize_symbol_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"').trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn expand_symbol_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();

    if let Some(delimited) = raw.strip_prefix("qw")
        && let Some(open) = delimited.chars().next()
    {
        let close = match open {
            '(' => ')',
            '{' => '}',
            '[' => ']',
            '<' => '>',
            delimiter => delimiter,
        };
        if let Some(inner) = delimited.strip_prefix(open).and_then(|body| body.strip_suffix(close))
        {
            return inner
                .split_whitespace()
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
        }
    }

    normalize_symbol_name(raw).into_iter().collect()
}

fn push_field(
    package: &str,
    field_name: &str,
    source_name: &NameCandidate,
    file_id: FileId,
    out: &mut Vec<QuickOrmColumnFact>,
) {
    let canonical_name = format!("{package}::{field_name}");
    if out.iter().any(|fact| {
        fact.entity.canonical_name == canonical_name
            && fact.anchor.span_start_byte as usize == source_name.span_start
            && fact.anchor.span_end_byte as usize == source_name.span_end
    }) {
        return;
    }

    let entity_id = EntityId(stable_id(
        "quickorm-candidate-column-entity",
        file_id,
        source_name.span_start,
        package,
        field_name,
    ));
    let anchor_id = AnchorId(stable_id(
        "quickorm-candidate-column-anchor",
        file_id,
        source_name.span_start,
        package,
        field_name,
    ));
    let anchor = AnchorFact {
        id: anchor_id,
        file_id,
        span_start_byte: source_name.span_start as u32,
        span_end_byte: source_name.span_end.min(u32::MAX as usize) as u32,
        scope_id: None,
        provenance: Provenance::FrameworkSynthesis,
        confidence: Confidence::Medium,
    };
    let entity = EntityFact {
        id: entity_id,
        kind: EntityKind::Field,
        canonical_name,
        anchor_id: Some(anchor_id),
        scope_id: None,
        provenance: Provenance::FrameworkSynthesis,
        confidence: Confidence::Medium,
    };
    out.push(QuickOrmColumnFact { entity, anchor });
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

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        parser.parse_with_recovery().ast
    }

    fn candidate_facts(source: &str) -> Vec<QuickOrmColumnFact> {
        extract_dbix_quickorm_column_candidates(&parse(source), FileId(1))
    }

    fn has_name(facts: &[QuickOrmColumnFact], canonical_name: &str) -> bool {
        facts.iter().any(|fact| fact.entity.canonical_name == canonical_name)
    }

    #[test]
    fn explicit_table_class_emits_singular_and_plural_column_candidates() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column id => sub { primary_key };
    columns(qw/name email/, sub { type VARCHAR });
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
        assert!(has_name(&facts, "My::ORM::Table::User::name"));
        assert!(has_name(&facts, "My::ORM::Table::User::email"));
        assert!(facts.iter().all(|fact| fact.entity.kind == EntityKind::Field));
    }

    #[test]
    fn equivalent_import_list_forms_activate_the_same_table_builder() {
        for source in [
            r#"
package My::ORM::Table::FatArrow;
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
            r#"
package My::ORM::Table::QuotedList;
use DBIx::QuickORM 'type', 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
            r#"
package My::ORM::Table::QwList;
use DBIx::QuickORM qw(type table);
table users => sub { column id => sub { primary_key }; };
1;
"#,
        ] {
            let facts = candidate_facts(source);
            assert!(
                facts.iter().any(|fact| fact.entity.canonical_name.ends_with("::id")),
                "semantic type/table import list should activate: {source}"
            );
        }
    }

    #[test]
    fn case_changed_import_parameters_do_not_activate_the_candidate() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM TYPE => 'TABLE';

table users => sub { column id => sub { primary_key }; };
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn column_candidates_do_not_masquerade_as_generated_methods() {
        let source = r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#;
        let ast = parse(source);
        let candidate = extract_dbix_quickorm_column_candidates(&ast, FileId(1));
        let production =
            crate::semantic::generated_member_extractor::extract_generated_member_facts(
                &ast,
                FileId(1),
            );

        assert!(has_name(&candidate, "My::ORM::Table::User::id"));
        assert!(candidate.iter().all(|fact| fact.entity.kind == EntityKind::Field));
        assert!(!production
            .iter()
            .any(|fact| fact.entity.canonical_name == "My::ORM::Table::User::id"));
    }

    #[test]
    fn lexical_block_restores_outer_package_and_activation() {
        let facts = candidate_facts(
            r#"
package Outer;
use DBIx::QuickORM type => 'table';
if (1) {
    package Inner;
}
table users => sub { column id => sub { primary_key }; };
1;
"#,
        );

        assert!(has_name(&facts, "Outer::id"));
        assert!(!has_name(&facts, "Inner::id"));
    }

    #[test]
    fn later_plain_import_replaces_table_class_activation() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM;

table users => sub { column id => sub { primary_key }; };
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn customized_import_symbols_remain_outside_the_candidate_contract() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table', rename => { column => 'field' };

table users => sub { column id => sub { primary_key }; };
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn first_table_declaration_closes_the_one_shot_builder() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub { column id => sub { primary_key }; };
table admins => sub { column admin_id => sub { primary_key }; };
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
        assert!(!has_name(&facts, "My::ORM::Table::User::admin_id"));
    }

    #[test]
    fn db_name_does_not_replace_the_logical_field_name() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column display_name => sub { db_name 'display_name_db' };
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::display_name"));
        assert!(!has_name(&facts, "My::ORM::Table::User::display_name_db"));
    }

    #[test]
    fn plain_schema_import_does_not_attach_inline_columns_to_orm_package() {
        let facts = candidate_facts(
            r#"
package My::ORM;
use DBIx::QuickORM;

schema app => sub {
    table users => sub {
        column id => sub { primary_key };
    };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::id"));
    }

    #[test]
    fn dynamic_table_names_consume_but_do_not_publish_the_table_builder() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
my $table_name = 'users';

table $table_name => sub {
    column dynamic_id => sub { primary_key };
};
table users => sub {
    column later_id => sub { primary_key };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::dynamic_id"));
        assert!(!has_name(&facts, "My::ORM::Table::User::later_id"));
    }

    #[test]
    fn interpolated_table_names_remain_a_dynamic_boundary() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
my $suffix = 'users';

table "app_$suffix" => sub {
    column id => sub { primary_key };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn dynamic_column_names_remain_a_dynamic_boundary() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
my $column_name = 'nickname';

table users => sub {
    column $column_name => sub { type VARCHAR };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::nickname"));
        assert!(!has_name(&facts, "My::ORM::Table::User::column_name"));
    }

    #[test]
    fn interpolated_column_names_remain_a_dynamic_boundary() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
my $suffix = 'name';

table users => sub {
    column "display_$suffix" => sub { type VARCHAR };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::display_name"));
    }

    #[test]
    fn candidate_facts_keep_framework_provenance_and_real_anchors()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
        );
        let fact = facts
            .iter()
            .find(|fact| fact.entity.canonical_name == "My::ORM::Table::User::id")
            .ok_or("missing QuickORM column candidate fact")?;

        assert_eq!(fact.entity.kind, EntityKind::Field);
        assert_eq!(fact.entity.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.entity.confidence, Confidence::Medium);
        assert_eq!(fact.anchor.provenance, Provenance::FrameworkSynthesis);
        assert!(fact.anchor.span_end_byte > fact.anchor.span_start_byte);
        Ok(())
    }
}
