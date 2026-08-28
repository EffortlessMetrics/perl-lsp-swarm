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
            let Some(anchor) = direct_table_or_view_builder_anchor(expression) else {
                return;
            };
            let package = context.current_package.as_deref().unwrap_or("main");
            push_qorm_table_fact(package, anchor, file_id, facts);
            // QuickORM removes its DSL imports after the table or view package
            // is built, so a second direct builder has no QuickORM authority.
            context.table_package_active = false;
        }
        // Runtime-controlled and bare lexical blocks are not package-level
        // table declarations. Do not recurse into them or let their package /
        // framework state escape into the containing package.
        _ => {}
    }
}

fn direct_table_or_view_builder_anchor(expression: &Node) -> Option<&Node> {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return None;
    };
    if !matches!(name.as_str(), "table" | "view") || args.is_empty() {
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
            interpolated,
        } if !value.trim().is_empty()
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
mod tests {
    use super::*;
    use crate::Parser;

    fn import_specs_from_source(
        source: &str,
    ) -> Result<Vec<ImportSpec>, Box<dyn std::error::Error>> {
        let mut parser = Parser::new(source);
        let ast = parser
            .parse()
            .map_err(|error| format!("failed to parse QuickORM import: {error:?}"))?;
        Ok(super::super::workspace_import_extractor::extract_import_specs(
            &ast,
            FileId(1),
        ))
    }

    fn generated_facts_from_source(
        source: &str,
    ) -> Result<Vec<GeneratedMemberFact>, Box<dyn std::error::Error>> {
        let mut parser = Parser::new(source);
        let ast = parser
            .parse()
            .map_err(|error| format!("failed to parse QuickORM table package: {error:?}"))?;
        Ok(super::super::generated_member_extractor::extract_generated_member_facts(
            &ast,
            FileId(2),
        ))
    }

    fn quickorm_spec(specs: &[ImportSpec]) -> Result<&ImportSpec, Box<dyn std::error::Error>> {
        specs
            .iter()
            .find(|spec| spec.module == QUICKORM_MODULE)
            .ok_or_else(|| "missing DBIx::QuickORM import spec".into())
    }

    fn find_quickorm_use(node: &Node) -> Option<&Node> {
        if matches!(&node.kind, NodeKind::Use { module, .. } if module == QUICKORM_MODULE) {
            return Some(node);
        }

        for child in node.children() {
            if let Some(found) = find_quickorm_use(child) {
                return Some(found);
            }
        }
        None
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
    fn configured_table_import_uses_default_dsl_exports() -> Result<(), Box<dyn std::error::Error>>
    {
        let specs = import_specs_from_source(
            "package User; use DBIx::QuickORM type => 'table'; table users => sub {};",
        )?;
        let spec = quickorm_spec(&specs)?;

        assert_eq!(spec.kind, ImportKind::Use);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        assert_eq!(spec.provenance, Provenance::ImportExportInference);
        assert_eq!(spec.confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn parser_preserves_quickorm_configuration_as_key_value_args()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = Parser::new("use DBIx::QuickORM type => 'table';");
        let ast = parser
            .parse()
            .map_err(|error| format!("failed to parse QuickORM import: {error:?}"))?;
        let use_node = find_quickorm_use(&ast).ok_or("missing QuickORM use node")?;
        let NodeKind::Use { args, .. } = &use_node.kind else {
            return Err("expected QuickORM use node".into());
        };

        assert_eq!(args, &["type".to_string(), "'table'".to_string()]);
        Ok(())
    }

    #[test]
    fn parser_preserves_quickorm_type_call_expression_syntax()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = Parser::new("use DBIx::QuickORM type => table();");
        let ast = parser
            .parse()
            .map_err(|error| format!("failed to parse QuickORM call import: {error:?}"))?;
        let use_node = find_quickorm_use(&ast).ok_or("missing QuickORM use node")?;
        let NodeKind::Use { args, .. } = &use_node.kind else {
            return Err("expected QuickORM use node".into());
        };

        let raw = args.join(" ");
        assert!(
            raw.contains('(') && raw.contains(')'),
            "call expression punctuation must remain visible to the classifier: {args:?}"
        );
        Ok(())
    }

    #[test]
    fn configured_orm_import_uses_default_dsl_exports() -> Result<(), Box<dyn std::error::Error>> {
        let specs = import_specs_from_source("package App; use DBIx::QuickORM type => 'orm';")?;
        let spec = quickorm_spec(&specs)?;

        assert_eq!(spec.kind, ImportKind::Use);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        assert_eq!(spec.provenance, Provenance::ImportExportInference);
        assert_eq!(spec.confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn dynamic_type_call_remains_dynamic_and_does_not_emit_qorm_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package User;
use DBIx::QuickORM type => table();
table users => sub {};
1;
"#;
        let specs = import_specs_from_source(source)?;
        let spec = quickorm_spec(&specs)?;

        assert_eq!(spec.kind, ImportKind::ManualImport);
        assert_eq!(spec.symbols, ImportSymbols::Dynamic);
        assert_eq!(spec.provenance, Provenance::DynamicBoundary);
        assert_eq!(spec.confidence, Confidence::Low);
        assert!(generated_facts_from_source(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn filtered_quickorm_import_remains_a_dynamic_manual_import()
    -> Result<(), Box<dyn std::error::Error>> {
        let specs = import_specs_from_source(
            "package User; use DBIx::QuickORM type => 'table', only => ['table'];",
        )?;
        let spec = quickorm_spec(&specs)?;

        assert_eq!(spec.kind, ImportKind::ManualImport);
        assert_eq!(spec.symbols, ImportSymbols::Dynamic);
        assert_eq!(spec.provenance, Provenance::DynamicBoundary);
        assert_eq!(spec.confidence, Confidence::Low);
        Ok(())
    }

    #[test]
    fn lookalike_import_keeps_generic_import_classification()
    -> Result<(), Box<dyn std::error::Error>> {
        let specs = import_specs_from_source("package User; use Local::DSL type => 'table';")?;
        let spec = specs
            .iter()
            .find(|spec| spec.module == "Local::DSL")
            .ok_or("missing lookalike import spec")?;

        assert_eq!(spec.kind, ImportKind::UseExplicitList);
        assert!(matches!(&spec.symbols, ImportSymbols::Explicit(_)));
        assert_eq!(spec.provenance, Provenance::ExactAst);
        Ok(())
    }

    #[test]
    fn table_package_emits_only_fixed_qorm_table_member() -> Result<(), Box<dyn std::error::Error>>
    {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    column id;
    columns qw/name email/;
};
1;
"#,
        )?;
        let names = canonical_names(&facts);

        assert_eq!(names, vec!["MyApp::Schema::User::qorm_table"]);
        let fact = facts.first().ok_or("missing qorm_table fact")?;
        assert_eq!(fact.entity.kind, EntityKind::GeneratedMember);
        assert_eq!(fact.entity.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.entity.confidence, Confidence::Medium);
        assert_eq!(fact.anchor.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(fact.anchor.confidence, Confidence::Medium);
        assert!(fact.anchor.span_end_byte > fact.anchor.span_start_byte);
        Ok(())
    }

    #[test]
    fn double_quoted_static_table_name_emits_qorm_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table "users" => sub {};
1;
"#,
        )?;

        assert_eq!(
            canonical_names(&facts),
            vec!["MyApp::Schema::User::qorm_table"]
        );
        Ok(())
    }

    #[test]
    fn interpolated_table_name_does_not_emit_qorm_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
table "${prefix}_users" => sub {};
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn view_package_emits_fixed_qorm_table_member() -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::ActiveUser;
use DBIx::QuickORM type => 'table';
view active_users => sub {};
1;
"#,
        )?;

        assert_eq!(
            canonical_names(&facts),
            vec!["MyApp::Schema::ActiveUser::qorm_table"]
        );
        Ok(())
    }

    #[test]
    fn orm_mode_inline_schema_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::ORM;
use DBIx::QuickORM type => 'orm';
schema app => sub {
    table users => sub {};
};
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn filtered_table_import_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table', only => ['table'];
table users => sub {};
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn table_mode_without_builder_does_not_emit_qorm_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            "package MyApp::Schema::User; use DBIx::QuickORM type => 'table'; 1;",
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn table_call_inside_subroutine_is_not_treated_as_package_builder()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
sub build_later {
    table users => sub {};
}
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn table_call_inside_runtime_control_is_not_treated_as_package_builder()
    -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use DBIx::QuickORM type => 'table';
if ($enabled) {
    table users => sub {};
}
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }

    #[test]
    fn bare_lexical_block_does_not_leak_package_or_framework_state()
    -> Result<(), Box<dyn std::error::Error>> {
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

    #[test]
    fn lookalike_table_dsl_does_not_emit_qorm_table() -> Result<(), Box<dyn std::error::Error>> {
        let facts = generated_facts_from_source(
            r#"
package MyApp::Schema::User;
use Local::DSL type => 'table';
table users => sub {};
1;
"#,
        )?;

        assert!(facts.is_empty());
        Ok(())
    }
}
