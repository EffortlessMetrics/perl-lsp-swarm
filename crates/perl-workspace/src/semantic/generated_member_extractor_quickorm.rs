//! Canonical generated-member extraction with bounded DBIx::QuickORM support.
//!
//! The existing generated-member producer remains the authority for Moo,
//! Moose, Mouse, Class::Tiny, and DBIx::Class. This wrapper adds one reviewed
//! successor-ORM subset: explicit DBIx::QuickORM table classes with statically
//! named `column` or `columns` declarations inside the table builder.
//!
//! Runtime schema fill, generated row classes, naming hooks, and relationship
//! accessors remain dynamic boundaries and are deliberately not inferred.

#[path = "generated_member_extractor.rs"]
mod base;

pub(crate) use base::GeneratedMemberFact;

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

/// Extract generated-member facts from the existing adapters plus the bounded
/// DBIx::QuickORM explicit-table-class subset.
pub(crate) fn extract_generated_member_facts(
    ast: &Node,
    file_id: FileId,
) -> Vec<GeneratedMemberFact> {
    let mut out = base::extract_generated_member_facts(ast, file_id);
    let mut ctx = QuickOrmWalkCtx::default();
    walk_quickorm(ast, file_id, &mut ctx, &mut out);
    out
}

fn walk_quickorm(
    node: &Node,
    file_id: FileId,
    ctx: &mut QuickOrmWalkCtx,
    out: &mut Vec<GeneratedMemberFact>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for statement in statements {
                walk_quickorm(statement, file_id, ctx, out);
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
            ctx.explicit_table_class_active |= is_explicit_table_class_import(args);
        }
        NodeKind::No { module, .. } if module == "DBIx::QuickORM" => {
            ctx.explicit_table_class_active = false;
        }
        NodeKind::ExpressionStatement { expression } if ctx.explicit_table_class_active => {
            extract_table_declaration(expression, file_id, ctx, out);
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
    let normalized: Vec<String> = args
        .iter()
        .map(|arg| normalize_use_arg(arg))
        .filter(|arg| !arg.is_empty() && arg != "," && arg != "=>")
        .collect();

    normalized.windows(2).any(|pair| pair[0] == "type" && pair[1] == "table")
}

fn normalize_use_arg(raw: &str) -> String {
    raw.trim()
        .trim_matches(|ch| matches!(ch, '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}'))
        .trim()
        .to_ascii_lowercase()
}

fn extract_table_declaration(
    expression: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<GeneratedMemberFact>,
) {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return;
    };
    if name != "table" {
        return;
    }

    let Some(builder) = args.iter().rev().find(|arg| is_anonymous_builder(arg)) else {
        return;
    };
    walk_table_builder(builder, file_id, ctx, out);
}

fn is_anonymous_builder(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Subroutine { name: None, .. } | NodeKind::Block { .. })
}

fn walk_table_builder(
    builder: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<GeneratedMemberFact>,
) {
    match &builder.kind {
        NodeKind::Subroutine { name: None, body, .. } => {
            walk_table_body(body, file_id, ctx, out);
        }
        NodeKind::Block { .. } => walk_table_body(builder, file_id, ctx, out),
        _ => {}
    }
}

fn walk_table_body(
    body: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<GeneratedMemberFact>,
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
    out: &mut Vec<GeneratedMemberFact>,
) {
    match &expression.kind {
        NodeKind::FunctionCall { name, args } if name == "column" => {
            let Some(first_arg) = args.first() else {
                return;
            };
            if let Some(candidate) = collect_name_candidates(first_arg).into_iter().next() {
                emit_candidate(candidate, file_id, ctx, out);
            }
        }
        NodeKind::FunctionCall { name, args } if name == "columns" => {
            for arg in args.iter().take_while(|arg| !is_anonymous_builder(arg)) {
                for candidate in collect_name_candidates(arg) {
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
    out: &mut Vec<GeneratedMemberFact>,
) {
    let Some(name) = normalize_static_column_name(&candidate.name) else {
        return;
    };
    let package = ctx.current_package.as_deref().unwrap_or("main");
    push_member(package, &name, &candidate, file_id, out);
}

fn collect_name_candidates(node: &Node) -> Vec<NameCandidate> {
    match &node.kind {
        NodeKind::String { value, .. } | NodeKind::Identifier { name: value } => {
            expand_symbol_list(value)
                .into_iter()
                .map(|name| NameCandidate {
                    name,
                    span_start: node.location.start,
                    span_end: node.location.end,
                })
                .collect()
        }
        NodeKind::ArrayLiteral { elements } => {
            elements.iter().flat_map(collect_name_candidates).collect()
        }
        NodeKind::Binary { op, left, right } if op == "," => {
            let mut names = collect_name_candidates(left);
            names.extend(collect_name_candidates(right));
            names
        }
        _ => Vec::new(),
    }
}

fn normalize_static_column_name(raw: &str) -> Option<String> {
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

    if raw.starts_with("qw(") && raw.ends_with(')') {
        return raw[3..raw.len() - 1]
            .split_whitespace()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();
    }

    if raw.starts_with("qw") && raw.len() > 2 {
        let open = raw.chars().nth(2).unwrap_or(' ');
        let close = match open {
            '(' => ')',
            '{' => '}',
            '[' => ']',
            '<' => '>',
            delimiter => delimiter,
        };
        if let (Some(start), Some(end)) = (raw.find(open), raw.rfind(close))
            && start < end
        {
            return raw[start + 1..end]
                .split_whitespace()
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
        }
    }

    normalize_symbol_name(raw).into_iter().collect()
}

fn push_member(
    package: &str,
    member_name: &str,
    source_name: &NameCandidate,
    file_id: FileId,
    out: &mut Vec<GeneratedMemberFact>,
) {
    let canonical_name = format!("{package}::{member_name}");
    if out.iter().any(|fact| {
        fact.entity.canonical_name == canonical_name
            && fact.anchor.span_start_byte as usize == source_name.span_start
            && fact.anchor.span_end_byte as usize == source_name.span_end
    }) {
        return;
    }

    let entity_id = EntityId(stable_id(
        "generated-member-entity",
        file_id,
        source_name.span_start,
        package,
        member_name,
    ));
    let anchor_id = AnchorId(stable_id(
        "generated-member-anchor",
        file_id,
        source_name.span_start,
        package,
        member_name,
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
        kind: EntityKind::GeneratedMember,
        canonical_name,
        anchor_id: Some(anchor_id),
        scope_id: None,
        provenance: Provenance::FrameworkSynthesis,
        confidence: Confidence::Medium,
    };
    out.push(GeneratedMemberFact { entity, anchor });
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

    fn extract_from_source(source: &str) -> Vec<GeneratedMemberFact> {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        extract_generated_member_facts(&output.ast, FileId(1))
    }

    fn has_name(facts: &[GeneratedMemberFact], canonical_name: &str) -> bool {
        facts.iter().any(|fact| fact.entity.canonical_name == canonical_name)
    }

    #[test]
    fn explicit_table_class_emits_singular_and_plural_column_members() {
        let facts = extract_from_source(
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
    }

    #[test]
    fn later_plain_import_does_not_erase_explicit_table_class_activation() {
        let facts = extract_from_source(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM;

table users => sub {
    column id => sub { primary_key };
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn db_name_does_not_replace_the_row_accessor_name() {
        let facts = extract_from_source(
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
    fn plain_quickorm_schema_import_does_not_attach_inline_columns_to_orm_package() {
        let facts = extract_from_source(
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
    fn dynamic_column_names_remain_a_dynamic_boundary() {
        let facts = extract_from_source(
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
    fn quickorm_member_facts_keep_generated_provenance_and_real_anchors()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = extract_from_source(
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
            .ok_or("missing QuickORM generated member fact")?;

        assert_eq!(fact.entity.kind, EntityKind::GeneratedMember);
        assert_eq!(fact.entity.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.entity.confidence, Confidence::Medium);
        assert_eq!(fact.anchor.provenance, Provenance::FrameworkSynthesis);
        assert!(fact.anchor.span_end_byte > fact.anchor.span_start_byte);
        Ok(())
    }
}
