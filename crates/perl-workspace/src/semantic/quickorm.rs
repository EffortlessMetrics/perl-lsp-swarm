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
    normalize_import_specs_at_node(ast, specs);
}

fn normalize_import_specs_at_node(node: &Node, specs: &mut [ImportSpec]) {
    if let NodeKind::Use { module, args, .. } = &node.kind
        && module == QUICKORM_MODULE
    {
        let anchor_id = AnchorId(node.location.start as u64);
        if let Some(spec) = specs
            .iter_mut()
            .find(|spec| spec.module == QUICKORM_MODULE && spec.anchor_id == Some(anchor_id))
        {
            match classify_import_shape(args) {
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
        normalize_import_specs_at_node(child, specs);
    }
}

/// Extract the fixed `qorm_table` member installed by a direct, unfiltered
/// QuickORM table-package `table` or `view` builder.
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
            if !is_direct_table_or_view_call(expression) {
                return;
            }

            // QuickORM removes its DSL imports after any table-package table or
            // view build, including a build whose name or body is not statically
            // known. Consume authority before deciding whether the declaration
            // is precise enough to emit a source-backed fact.
            context.table_package_active = false;

            let Some(anchor) = direct_table_or_view_builder_anchor(expression) else {
                return;
            };
            let package = context.current_package.as_deref().unwrap_or("main");
            push_qorm_table_fact(package, anchor, file_id, facts);
        }
        // Runtime-controlled and bare lexical blocks are not package-level
        // table declarations. Do not recurse into them or let their package /
        // framework state escape into the containing package.
        _ => {}
    }
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
        || !args.iter().skip(1).any(contains_builder_body)
    {
        return None;
    }

    static_table_name_anchor(args.first()?)
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
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if matches!(character, '$' | '@' | '%') {
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

fn classify_import_shape(args: &[String]) -> QuickOrmImportShape {
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

    let Some(key) = static_import_atom(raw_key) else {
        return QuickOrmImportShape::Dynamic;
    };
    let Some(value) = static_import_atom(raw_value) else {
        return QuickOrmImportShape::Dynamic;
    };
    if key != "type" {
        return QuickOrmImportShape::Dynamic;
    }

    match value.as_str() {
        "orm" => QuickOrmImportShape::UnfilteredOrm,
        "table" => QuickOrmImportShape::UnfilteredTable,
        _ => QuickOrmImportShape::Dynamic,
    }
}

fn static_import_atom(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() >= 2 {
        let first = value.as_bytes().first().copied()?;
        let last = value.as_bytes().last().copied()?;
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            let body = &value[1..value.len() - 1];
            if body.is_empty() || (first == b'"' && contains_unescaped_interpolation(body)) {
                return None;
            }
            return Some(body.to_string());
        }
    }

    is_static_identifier(value).then_some(value.to_string())
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
