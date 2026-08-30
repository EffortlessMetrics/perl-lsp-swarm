//! Canonical generated-member publication gate.
//!
//! The historical extractor still contains permissive DBIx::Class recognition
//! based on raw source spelling. DBIx::Class is registered as a shadow adapter
//! with no provider surfaces, so those compatibility facts are not publication
//! authority. This wrapper preserves the legacy implementation as a comparison
//! oracle while removing only its DBIx::Class rows from the canonical path.
//!
//! Remove this quarantine only through #13979 after #9736/#9739/#9741 publish
//! the equivalent admitted facts and the matching provider surfaces prove
//! same-request parity.

use super::legacy_generated_member_extractor;
pub(crate) use super::legacy_generated_member_extractor::GeneratedMemberFact;
use crate::{Node, NodeKind};
use perl_semantic_facts::FileId;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
struct WalkCtx {
    current_package: Option<String>,
    dbix_class_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameCandidate {
    name: String,
    span_start: usize,
    span_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct QuarantinedMember {
    canonical_name: String,
    span_start_byte: u32,
    span_end_byte: u32,
}

/// Extract generated-member facts admitted to the canonical workspace shard.
pub(crate) fn extract_generated_member_facts(
    ast: &Node,
    file_id: FileId,
) -> Vec<GeneratedMemberFact> {
    let mut facts = legacy_generated_member_extractor::extract_generated_member_facts(ast, file_id);
    let mut quarantined = BTreeSet::new();
    collect_quarantined_dbix_members(ast, &mut WalkCtx::default(), &mut quarantined);

    facts.retain(|fact| {
        !quarantined.contains(&QuarantinedMember {
            canonical_name: fact.entity.canonical_name.clone(),
            span_start_byte: fact.anchor.span_start_byte,
            span_end_byte: fact.anchor.span_end_byte,
        })
    });
    facts
}

fn collect_quarantined_dbix_members(
    node: &Node,
    ctx: &mut WalkCtx,
    out: &mut BTreeSet<QuarantinedMember>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for statement in statements {
                collect_quarantined_dbix_members(statement, ctx, out);
            }
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved = ctx.clone();
                ctx.current_package = Some(name.clone());
                ctx.dbix_class_active = false;
                collect_quarantined_dbix_members(block, ctx, out);
                *ctx = saved;
            } else {
                ctx.current_package = Some(name.clone());
                ctx.dbix_class_active = false;
            }
        }
        NodeKind::Use { module, .. } if is_dbix_class_module(module) => {
            ctx.dbix_class_active = true;
        }
        NodeKind::Use { module, args, .. }
            if (module == "base" || module == "parent") && use_args_include_dbix_class(args) =>
        {
            ctx.dbix_class_active = true;
        }
        NodeKind::No { module, .. } if is_dbix_class_module(module) => {
            ctx.dbix_class_active = false;
        }
        NodeKind::ExpressionStatement { expression } if ctx.dbix_class_active => {
            collect_dbix_class_call(expression, ctx, out);
        }
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {}
        _ => {
            for child in node.children() {
                collect_quarantined_dbix_members(child, ctx, out);
            }
        }
    }
}

fn collect_dbix_class_call(
    expression: &Node,
    ctx: &WalkCtx,
    out: &mut BTreeSet<QuarantinedMember>,
) {
    let NodeKind::MethodCall { object, method, args } = &expression.kind else {
        return;
    };
    if !package_target_matches_current_package(object, ctx) {
        return;
    }

    let package = ctx.current_package.as_deref().unwrap_or("main");
    match method.as_str() {
        "add_columns" => {
            let mut arg_idx = 0;
            while arg_idx < args.len() {
                if let (Some(key), Some(value)) = (args.get(arg_idx), args.get(arg_idx + 1))
                    && dbix_column_pair_shape(key, value)
                {
                    if let Some(candidate) = dbix_column_accessor_candidate_from_pair(key, value) {
                        push_quarantined_member(package, &candidate, out);
                    }
                    arg_idx += 2;
                } else {
                    for candidate in collect_dbix_column_accessor_candidates(&args[arg_idx]) {
                        push_quarantined_member(package, &candidate, out);
                    }
                    arg_idx += 1;
                }
            }
        }
        "has_many" | "belongs_to" | "has_one" | "might_have" => {
            if let Some(first_arg) = args.first() {
                for candidate in collect_name_candidates(first_arg) {
                    if is_dbix_relationship_name(&candidate.name) {
                        push_quarantined_member(package, &candidate, out);
                    }
                }
            }
        }
        _ => {}
    }
}

fn push_quarantined_member(
    package: &str,
    candidate: &NameCandidate,
    out: &mut BTreeSet<QuarantinedMember>,
) {
    if candidate.name.is_empty() {
        return;
    }
    out.insert(QuarantinedMember {
        canonical_name: format!("{package}::{}", candidate.name),
        span_start_byte: candidate.span_start.min(u32::MAX as usize) as u32,
        span_end_byte: candidate.span_end.min(u32::MAX as usize) as u32,
    });
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

fn collect_dbix_column_accessor_candidates(node: &Node) -> Vec<NameCandidate> {
    match &node.kind {
        NodeKind::HashLiteral { pairs } => pairs
            .iter()
            .filter_map(|(key, value)| dbix_column_accessor_candidate_from_pair(key, value))
            .collect(),
        NodeKind::Binary { op, left, right } if op == "=>" => {
            dbix_column_accessor_candidate_from_pair(left, right)
                .into_iter()
                .collect()
        }
        NodeKind::Binary { op, left, right } if op == "," => {
            let mut names = collect_dbix_column_accessor_candidates(left);
            names.extend(collect_dbix_column_accessor_candidates(right));
            names
        }
        _ => collect_name_candidates(node),
    }
}

fn dbix_column_accessor_candidate_from_pair(key: &Node, value: &Node) -> Option<NameCandidate> {
    let key_candidate = collect_name_candidates(key).into_iter().next()?;
    let column_name = normalize_attribute_name(&key_candidate.name)?;
    let accessor = match &value.kind {
        NodeKind::HashLiteral { pairs } => option_value(pairs, "accessor"),
        _ => None,
    };
    Some(NameCandidate {
        name: accessor.unwrap_or(column_name),
        span_start: key_candidate.span_start,
        span_end: key_candidate.span_end,
    })
}

fn option_value(pairs: &[(Node, Node)], wanted: &str) -> Option<String> {
    let mut found = None;
    for (key, value) in pairs {
        let Some(key) = collect_name_candidates(key).into_iter().next() else {
            continue;
        };
        if key.name == wanted {
            found = Some(value_summary(value));
        }
    }
    found
}

fn value_summary(node: &Node) -> String {
    match &node.kind {
        NodeKind::String { value, .. } => {
            normalize_symbol_name(value).unwrap_or_else(|| value.clone())
        }
        NodeKind::Identifier { name } => name.clone(),
        NodeKind::Number { value } => value.clone(),
        _ => "expr".to_string(),
    }
}

fn dbix_column_pair_shape(key: &Node, value: &Node) -> bool {
    matches!(key.kind, NodeKind::String { .. } | NodeKind::Identifier { .. })
        && matches!(value.kind, NodeKind::HashLiteral { .. })
}

fn use_args_include_dbix_class(args: &[String]) -> bool {
    args.iter()
        .flat_map(|arg| expand_symbol_list(arg.trim()))
        .any(|name| is_dbix_class_module(&name))
}

fn normalize_symbol_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_attribute_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    normalize_symbol_name(trimmed.strip_prefix('+').unwrap_or(trimmed))
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

fn is_dbix_class_module(module: &str) -> bool {
    matches!(module, "DBIx::Class" | "DBIx::Class::Core")
}

fn is_dbix_relationship_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn package_target_matches_current_package(object: &Node, ctx: &WalkCtx) -> bool {
    let current_package = ctx.current_package.as_deref().unwrap_or("main");
    match &object.kind {
        NodeKind::Identifier { name } => name == "__PACKAGE__" || name == current_package,
        NodeKind::String { value, .. } => {
            normalize_symbol_name(value).is_some_and(|name| name == current_package)
        }
        NodeKind::Variable { sigil, name } if sigil == "$" => name == current_package,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        parser.parse_with_recovery().ast
    }

    fn names(facts: &[GeneratedMemberFact]) -> Vec<&str> {
        facts
            .iter()
            .map(|fact| fact.entity.canonical_name.as_str())
            .collect()
    }

    #[test]
    fn legacy_dbix_columns_are_not_canonical_publication_authority() {
        let ast = parse(
            r#"
package MyApp::Schema::Result::User;
use DBIx::Class;
__PACKAGE__->add_columns(qw/id name email/);
1;
"#,
        );

        let legacy =
            legacy_generated_member_extractor::extract_generated_member_facts(&ast, FileId(1));
        assert!(names(&legacy).contains(&"MyApp::Schema::Result::User::id"));

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(
            admitted.is_empty(),
            "raw DBIx spelling must not publish generated members"
        );
    }

    #[test]
    fn legacy_dbix_relationships_and_base_activation_are_quarantined() {
        let ast = parse(
            r#"
package MyApp::Schema::Result::Author;
use base 'DBIx::Class::Core';
__PACKAGE__->has_many('posts', 'MyApp::Schema::Result::Post', 'author_id');
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(
            !names(&admitted).contains(&"MyApp::Schema::Result::Author::posts"),
            "raw base inheritance must not authorize relationship publication"
        );
    }

    #[test]
    fn non_dbix_generated_members_remain_admitted() {
        let ast = parse(
            r#"
package MyApp::User;
use Moo;
has 'name' => (is => 'ro');
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(names(&admitted).contains(&"MyApp::User::name"));
    }

    #[test]
    fn mixed_package_preserves_moo_member_and_quarantines_dbix_member() {
        let ast = parse(
            r#"
package MyApp::User;
use Moo;
use DBIx::Class::Core;
has 'name' => (is => 'ro');
__PACKAGE__->add_columns(qw/id/);
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        let admitted_names = names(&admitted);
        assert!(admitted_names.contains(&"MyApp::User::name"));
        assert!(!admitted_names.contains(&"MyApp::User::id"));
    }

    #[test]
    fn same_named_dsl_without_dbix_activation_remains_non_dbix() {
        let ast = parse(
            r#"
package Plain::Package;
__PACKAGE__->add_columns(qw/id/);
__PACKAGE__->has_many('children', 'Plain::Child', 'parent_id');
1;
"#,
        );

        assert!(extract_generated_member_facts(&ast, FileId(1)).is_empty());
    }
}
