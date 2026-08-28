//! Bounded DBIx::QuickORM semantic adaptation.
//!
//! QuickORM's import arguments configure its custom DSL importer; they are not
//! an Exporter-style explicit symbol list. Exact unfiltered `type => 'orm'`
//! and `type => 'table'` forms select QuickORM's default DSL export set. Forms
//! with filters, renames, calls, or otherwise unknown configuration remain a
//! dynamic import boundary until the canonical import model can represent that
//! mapping.
//!
//! In table-package mode, executing a direct package-level `table` or `view`
//! builder installs one fixed method, `qorm_table`.
//!
//! Named field and link accessors are intentionally absent here. Upstream
//! installs those through database-backed `autofill`/`autorow`, not from a
//! manual `column` declaration alone.

use super::generated_member_extractor_core::GeneratedMemberFact;
use crate::{Node, NodeKind};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, ImportKind,
    ImportSpec, ImportSymbols, Provenance,
};
use std::collections::BTreeSet;

const QUICKORM_MODULE: &str = "DBIx::QuickORM";
const QORM_TABLE_MEMBER: &str = "qorm_table";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickOrmImportShape {
    Bare,
    UnfilteredOrm,
    UnfilteredTable,
    Dynamic,
}

/// Rewrite generic import facts where QuickORM arguments are importer
/// configuration rather than an Exporter-style symbol list.
pub(super) fn normalize_import_specs(ast: &Node, specs: &mut [ImportSpec]) {
    normalize_import_specs_at_node(ast, specs, None);
}

pub(super) fn normalize_import_specs_with_source(
    ast: &Node,
    specs: &mut [ImportSpec],
    source: &str,
) {
    normalize_import_specs_at_node(ast, specs, Some(source));
}

fn normalize_import_specs_at_node(node: &Node, specs: &mut [ImportSpec], source: Option<&str>) {
    if let NodeKind::Use { module, args, .. } = &node.kind
        && module == QUICKORM_MODULE
    {
        let anchor_id = AnchorId(node.location.start as u64);
        if let Some(spec) = specs
            .iter_mut()
            .find(|spec| spec.module == QUICKORM_MODULE && spec.anchor_id == Some(anchor_id))
        {
            let source_segment = source.and_then(|text| source_import_segment(text, node));
            match classify_import_shape(args, source_segment) {
                QuickOrmImportShape::Bare => {}
                QuickOrmImportShape::UnfilteredOrm | QuickOrmImportShape::UnfilteredTable => {
                    // Both exact unfiltered modes install QuickORM's complete
                    // documented DSL export set. Keep `builder` and ORM-mode
                    // downstream `import` machinery outside this bounded fact.
                    spec.kind = ImportKind::Use;
                    spec.symbols = ImportSymbols::Default;
                    spec.provenance = Provenance::ImportExportInference;
                    spec.confidence = Confidence::High;
                }
                QuickOrmImportShape::Dynamic => {
                    // The selected or renamed namespace is not known. ManualImport
                    // prevents the visibility layer from exposing every default
                    // export while ImportSymbols::Dynamic preserves conservative
                    // dynamic-call evidence after the import site.
                    spec.kind = ImportKind::ManualImport;
                    spec.symbols = ImportSymbols::Dynamic;
                    spec.provenance = Provenance::DynamicBoundary;
                    spec.confidence = Confidence::Low;
                }
            }
        }
    }

    for child in node.children() {
        normalize_import_specs_at_node(child, specs, source);
    }
}

/// Extract the fixed `qorm_table` member installed by a direct, unfiltered
/// QuickORM table-package `table` or `view` builder.
pub(super) fn extract_generated_member_facts(
    ast: &Node,
    file_id: FileId,
) -> Vec<GeneratedMemberFact> {
    extract_generated_member_facts_from_source(ast, file_id, None)
}

pub(super) fn extract_generated_member_facts_with_source(
    ast: &Node,
    file_id: FileId,
    source: &str,
) -> Vec<GeneratedMemberFact> {
    extract_generated_member_facts_from_source(ast, file_id, Some(source))
}

fn extract_generated_member_facts_from_source(
    ast: &Node,
    file_id: FileId,
    source: Option<&str>,
) -> Vec<GeneratedMemberFact> {
    let mut facts = Vec::new();
    let mut context = WalkContext::default();

    match &ast.kind {
        NodeKind::Program { statements } => {
            walk_direct_statements(statements, file_id, &mut context, &mut facts, source);
        }
        _ => walk_direct_statement(ast, file_id, &mut context, &mut facts, source),
    }

    facts
}

#[derive(Debug, Default)]
struct WalkContext {
    current_package: Option<String>,
    table_package_authority: BTreeSet<String>,
    shadowed_builders: BTreeSet<String>,
}

fn walk_direct_statements(
    statements: &[Node],
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
    source: Option<&str>,
) {
    for statement in statements {
        walk_direct_statement(statement, file_id, context, facts, source);
    }
}

fn walk_direct_statement(
    node: &Node,
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
    source: Option<&str>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            walk_direct_statements(statements, file_id, context, facts, source);
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved_package = context.current_package.clone();
                context.current_package = Some(name.clone());

                if let NodeKind::Block { statements } = &block.kind {
                    walk_direct_statements(statements, file_id, context, facts, source);
                }

                context.current_package = saved_package;
            } else {
                context.current_package = Some(name.clone());
            }
        }
        NodeKind::Block { statements } => {
            // A bare lexical block does not change the current package.  Its
            // compile-time QuickORM imports still configure that package, so
            // walk its direct statements while preserving the package-scoped
            // authority map.  Control-flow and subroutine bodies are not
            // reached here because this walker intentionally does not recurse
            // through their enclosing nodes.
            walk_direct_statements(statements, file_id, context, facts, source);
        }
        NodeKind::Use { module, args, .. } if module == QUICKORM_MODULE => {
            let package = current_package(context).to_string();
            let source_segment = source.and_then(|text| source_import_segment(text, node));
            if classify_import_shape(args, source_segment) == QuickOrmImportShape::UnfilteredTable
                && !context.shadowed_builders.contains(&package)
            {
                context.table_package_authority.insert(package);
            } else {
                context.table_package_authority.remove(&package);
            }
        }
        NodeKind::No { module, .. } if module == QUICKORM_MODULE => {
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::ExpressionStatement { expression } => {
            let package = current_package(context).to_string();
            if !context.table_package_authority.contains(&package)
                || context.shadowed_builders.contains(&package)
                || !is_direct_table_or_view_call(expression)
            {
                return;
            }

            // QuickORM removes its DSL imports after any table-package table or
            // view build, including a build whose name or body is not statically
            // known. Consume authority before deciding whether the declaration
            // is precise enough to emit a source-backed fact.
            context.table_package_authority.remove(&package);

            let Some(anchor) = direct_table_or_view_builder_anchor(expression) else {
                return;
            };
            push_qorm_table_fact(&package, anchor, file_id, facts);
        }
        NodeKind::Subroutine { name: Some(name), .. } if is_builder_name(name) => {
            let package = current_package(context).to_string();
            context.shadowed_builders.insert(package.clone());
            context.table_package_authority.remove(&package);
            facts.retain(|fact| {
                fact.entity.canonical_name != format!("{package}::{QORM_TABLE_MEMBER}")
            });
        }
        // Runtime-controlled nodes and nested subroutine bodies are not
        // package-level table declarations. Do not recurse into them or let
        // runtime-controlled framework state escape into the containing package.
        _ => {}
    }
}

fn current_package(context: &WalkContext) -> &str {
    context.current_package.as_deref().unwrap_or("main")
}

fn is_direct_table_or_view_call(expression: &Node) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    matches!(name.as_str(), "table" | "view") && !args.is_empty()
}

fn direct_table_or_view_builder_anchor(expression: &Node) -> Option<&Node> {
    let NodeKind::FunctionCall { args, .. } = &expression.kind else {
        return None;
    };
    if !is_direct_table_or_view_call(expression)
        || !args.iter().skip(1).any(is_direct_builder_argument)
    {
        return None;
    }

    static_table_name_anchor(args.first()?)
}

fn is_direct_builder_argument(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Subroutine { .. } | NodeKind::Block { .. } | NodeKind::HashLiteral { .. } => true,
        NodeKind::Binary { op, right, .. } if op == "=>" => is_direct_builder_argument(right),
        _ => false,
    }
}

fn is_builder_name(name: &str) -> bool {
    matches!(name, "table" | "view" | QORM_TABLE_MEMBER)
}

fn static_table_name_anchor(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::String { value, interpolated }
            if !value.trim().is_empty()
                && (!*interpolated || !contains_unescaped_interpolation(value)) =>
        {
            Some(node)
        }
        NodeKind::Identifier { name } if is_static_identifier(name) => Some(node),
        NodeKind::Binary { op, left, .. } if op == "=>" => static_table_name_anchor(left),
        _ => None,
    }
}

fn contains_unescaped_interpolation(value: &str) -> bool {
    let mut escaped = false;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if matches!(character, '@' | '$')
            && characters
                .peek()
                .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '{'))
        {
            return true;
        }
    }
    false
}

fn is_static_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn push_qorm_table_fact(
    package: &str,
    source: &Node,
    file_id: FileId,
    facts: &mut Vec<GeneratedMemberFact>,
) {
    let canonical_name = format!("{package}::{QORM_TABLE_MEMBER}");
    if facts.iter().any(|fact| fact.entity.canonical_name == canonical_name) {
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

fn classify_import_shape(args: &[String], source: Option<&str>) -> QuickOrmImportShape {
    if args.is_empty() {
        return QuickOrmImportShape::Bare;
    }

    let (raw_key, raw_value) = match args {
        // The parser intentionally drops the fat arrow from a normal
        // `use Module key => value` import and retains expression punctuation
        // as additional raw args. Exactly two static atoms are therefore the
        // admitted parser-backed shape.
        [key, value] => (key.as_str(), value.as_str()),
        // Preserve support for callers that provide source-level tokens.
        [key, arrow, value] if arrow.trim() == "=>" => (key.as_str(), value.as_str()),
        _ => return QuickOrmImportShape::Dynamic,
    };

    let Some(source) = source else {
        return QuickOrmImportShape::Dynamic;
    };
    let Some((source_key, source_value)) = exact_source_import_pair(source) else {
        return QuickOrmImportShape::Dynamic;
    };
    if source_key != "type" || !matches!(source_value, "orm" | "table") {
        return QuickOrmImportShape::Dynamic;
    }

    let Some(key) = static_import_key(raw_key) else {
        return QuickOrmImportShape::Dynamic;
    };
    let Some(value) = quoted_import_value(raw_value) else {
        return QuickOrmImportShape::Dynamic;
    };
    if key != source_key || value != source_value {
        return QuickOrmImportShape::Dynamic;
    }

    match value.as_str() {
        "orm" => QuickOrmImportShape::UnfilteredOrm,
        "table" => QuickOrmImportShape::UnfilteredTable,
        _ => QuickOrmImportShape::Dynamic,
    }
}

fn source_import_segment<'a>(source: &'a str, node: &Node) -> Option<&'a str> {
    let remainder = source.get(node.location.start..)?;
    let end =
        remainder.find(';').map_or(node.location.end - node.location.start, |index| index + 1);
    remainder.get(..end)
}

fn exact_source_import_pair(source: &str) -> Option<(&str, &str)> {
    let mut rest = trim_source_trivia(source);
    let use_keyword = rest.strip_prefix("use")?;
    if !use_keyword.chars().next().is_none_or(|c| c.is_ascii_whitespace()) {
        return None;
    }
    rest = trim_source_trivia(use_keyword);
    rest = rest.strip_prefix(QUICKORM_MODULE)?;
    if !rest.chars().next().is_none_or(|c| c.is_ascii_whitespace() || c == ';') {
        return None;
    }
    rest = trim_source_trivia(rest);
    if rest.is_empty() || rest == ";" {
        return None;
    }

    let (key, remainder) = parse_source_quoted_or_identifier(rest)?;
    rest = trim_source_trivia(remainder);
    rest = rest.strip_prefix("=>")?.trim_start();
    rest = trim_source_trivia(rest);
    let (value, remainder) = parse_source_quoted_or_identifier(rest)?;
    let remainder = trim_source_trivia(remainder);
    if remainder != ";" && !remainder.is_empty() {
        return None;
    }
    Some((key, value))
}

fn trim_source_trivia(mut source: &str) -> &str {
    loop {
        source = source.trim_start();
        if let Some(comment) = source.strip_prefix('#') {
            source = comment;
            if let Some(newline) = source.find(['\r', '\n']) {
                source = &source[newline..];
            } else {
                return "";
            }
            continue;
        }
        return source;
    }
}

fn parse_source_quoted_or_identifier(source: &str) -> Option<(&str, &str)> {
    let first = source.as_bytes().first().copied()?;
    if first == b'\'' || first == b'"' {
        let end = source[1..].find(char::from(first))? + 1;
        let value = &source[1..end];
        if value.is_empty() || value.contains(['\\', '$', '@', '%']) {
            return None;
        }
        return Some((value, &source[end + 1..]));
    }
    let end = source
        .char_indices()
        .find(|(_, character)| character.is_ascii_whitespace() || matches!(character, ';' | '='))
        .map_or(source.len(), |(index, _)| index);
    let value = &source[..end];
    (!value.is_empty()).then_some((value, &source[end..]))
}

fn static_import_key(raw: &str) -> Option<String> {
    quoted_import_value(raw).or_else(|| {
        let value = raw.trim();
        is_static_identifier(value).then_some(value.to_string())
    })
}

fn quoted_import_value(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() < 2 {
        return None;
    }

    let first = value.as_bytes().first().copied()?;
    let last = value.as_bytes().last().copied()?;
    if !((first == b'\'' && last == b'\'') || (first == b'"' && last == b'"')) {
        return None;
    }

    let body = &value[1..value.len() - 1];
    if body.is_empty() || (first == b'"' && contains_unescaped_interpolation(body)) {
        return None;
    }
    Some(body.to_string())
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
mod tests;
