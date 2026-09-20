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

/// Source-spelling form of a recognized literal at the start of a slice.
///
/// Deliberately distinct from the parser's quote-operator awareness: this
/// set covers only what the bounded QuickORM pilot reads as a static
/// literal. Execution boundaries (`qx{...}`, `` `...` ``) and heredocs are
/// not represented here and must be rejected at the call site so they do
/// not silently promote to a source-backed fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteLikeForm {
    /// `'literal'`
    SingleQuoted,
    /// `"interpolated"`
    DoubleQuoted,
    /// `q{literal}` / `q/literal/` / `q!literal!` (non-interpolating)
    QLiteral,
    /// `qq{interpolated}` (interpolating)
    QqInterpolating,
}

impl QuoteLikeForm {
    /// Whether this form interpolates Perl expressions in its body.
    const fn interpolates(self) -> bool {
        matches!(self, Self::DoubleQuoted | Self::QqInterpolating)
    }
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
                    spec.confidence = Confidence::Medium;
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
/// QuickORM table-package `table` or `view` builder, using the source text to
/// establish exact import authority.
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
        _ => walk_direct_statement(ast, file_id, &mut context, &mut facts, source, None),
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
    for (index, statement) in statements.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|prior| statements.get(prior));
        walk_direct_statement(statement, file_id, context, facts, source, previous);
    }
}

fn walk_direct_statement(
    node: &Node,
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
    source: Option<&str>,
    previous: Option<&Node>,
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
            // authority map.  A semicolon-style package declaration inside the
            // block changes the traversal context temporarily, so restore the
            // containing package before returning. Control-flow and subroutine
            // bodies are not reached here because this walker intentionally
            // does not recurse through their enclosing nodes.
            let saved_package = context.current_package.clone();
            walk_direct_statements(statements, file_id, context, facts, source);
            context.current_package = saved_package;
        }
        NodeKind::Use { module, args, .. } if module == QUICKORM_MODULE => {
            let package = current_package(context).to_string();
            let source_segment = source.and_then(|text| source_import_segment(text, node));
            if classify_import_shape(args, source_segment) == QuickOrmImportShape::UnfilteredTable {
                // A valid later import re-establishes QuickORM's compile-time
                // authority after an earlier package-local builder shadow.
                context.shadowed_builders.remove(&package);
                context.table_package_authority.insert(package);
            } else {
                // A subsequent bare, dynamic, or ORM-mode use merely prevents
                // the next direct `table`/`view` call from being attributed
                // to QuickORM; the already-installed qorm_table fact remains
                // source-backed and is not invalidated here. A future explicit
                // `sub qorm_table {}` is the construct that overwrites it.
                context.table_package_authority.remove(&package);
            }
        }
        NodeKind::Use { module, args, .. }
            if module != QUICKORM_MODULE && imports_table_builder(args) =>
        {
            // A competing importer of the `table`/`view` keyword shadows the
            // builder for any *future* direct calls; it does not by itself
            // remove the already-installed qorm_table member from the package.
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::ExpressionStatement { expression } if is_competing_import_call(expression) => {
            // A competing method/function `import` call drops QuickORM's
            // builder authority for future direct calls but does not delete
            // or redefine the installed qorm_table member.
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::No { module, .. } if module == QUICKORM_MODULE => {
            // `no DBIx::QuickORM` only retracts the import; the package-level
            // qorm_table member installed by a prior build is unaffected.
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::ExpressionStatement { expression } => {
            // The parser cannot disambiguate `table q(NAME) => BODY` from a
            // bare hash literal because the table-name argument begins with a
            // quote-like operator the parser does not parse as a literal.
            // Recover the builder shape when the immediately previous direct
            // statement was a bareword `table`/`view` identifier and this
            // statement is a single-pair `HashLiteral`.
            if let NodeKind::HashLiteral { pairs } = &expression.kind
                && let Some(builder_keyword) = previous_table_or_view_identifier(previous)
            {
                handle_hash_literal_builder_call(
                    pairs,
                    &builder_keyword,
                    file_id,
                    context,
                    facts,
                    source,
                );
                return;
            }

            let package = current_package(context).to_string();
            if !context.table_package_authority.contains(&package)
                || context.shadowed_builders.contains(&package)
                || !is_table_or_view_call(expression, &package)
            {
                return;
            }

            // QuickORM removes its DSL imports after any table-package table or
            // view build, including a build whose name or body is not statically
            // known. Consume authority before deciding whether the declaration
            // is precise enough to emit a source-backed fact.
            context.table_package_authority.remove(&package);

            let Some(anchor) = direct_table_or_view_builder_anchor(expression, source) else {
                invalidate_qorm_table_fact(&package, facts);
                return;
            };
            push_qorm_table_fact(&package, anchor, file_id, facts);
        }
        NodeKind::Subroutine { name: Some(name), .. } if is_builder_name(name) => {
            let package = current_package(context).to_string();
            if name == QORM_TABLE_MEMBER {
                // A local `sub qorm_table {}` literally redefines the
                // installed generated member; the fact must be invalidated.
                context.table_package_authority.remove(&package);
                invalidate_qorm_table_fact(&package, facts);
            } else {
                // A local `sub table {}` / `sub view {}` only shadows the
                // builder keyword for future direct calls. The already
                // installed qorm_table member remains source-backed and is
                // not invalidated by the shadow alone; a separate explicit
                // `sub qorm_table {}` would still invalidate it.
                context.table_package_authority.remove(&package);
                context.shadowed_builders.insert(package);
            }

            if let NodeKind::Subroutine { body, .. } = &node.kind {
                walk_compile_time_descendants(body, file_id, context, facts, source);
            }
        }
        NodeKind::Subroutine { body, .. } => {
            // `use`/`no` remain compile-time declarations when written inside
            // a subroutine body. Inspect those declarations, plus qualified
            // builder calls in nested executable descendants, without treating
            // ordinary runtime `table` calls as package-level declarations.
            walk_compile_time_descendants(body, file_id, context, facts, source);
        }
        // Other direct statements can contain executable descendants, such as
        // a qualified builder in a variable initializer. Inspect those
        // descendants for invalidating evidence without treating a direct
        // runtime `table` call as a package-level declaration.
        _ => walk_compile_time_descendants(node, file_id, context, facts, source),
    }
}

fn walk_compile_time_descendants(
    node: &Node,
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
    source: Option<&str>,
) {
    match &node.kind {
        NodeKind::Block { statements } => {
            // Compile-time containers retain Perl's package state across
            // semicolon-style declarations, but must restore the package that
            // enclosed the container when they return.  Walking each child
            // through this function also keeps direct runtime builders out of
            // the compile-time-only path.
            let saved_package = context.current_package.clone();
            for statement in statements {
                walk_compile_time_descendants(statement, file_id, context, facts, source);
            }
            context.current_package = saved_package;
            return;
        }
        NodeKind::Package { name, block, .. } => {
            // Package declarations can occur below a compile-time container,
            // where the ordinary direct-statement walker is not involved.
            // Attribute nested declarations to their package and restore the
            // enclosing package after a braced declaration.  A semicolon-style
            // declaration intentionally remains in effect for later siblings
            // in the containing block, just as it does in Perl source order.
            let saved_package = context.current_package.clone();
            context.current_package = Some(name.clone());
            if let Some(block) = block {
                walk_compile_time_descendants(block, file_id, context, facts, source);
                context.current_package = saved_package;
            }
            return;
        }
        NodeKind::Use { module, args, .. } if module == QUICKORM_MODULE => {
            let package = current_package(context).to_string();
            let source_segment = source.and_then(|text| source_import_segment(text, node));
            if classify_import_shape(args, source_segment) == QuickOrmImportShape::UnfilteredTable {
                context.shadowed_builders.remove(&package);
                context.table_package_authority.insert(package);
            } else {
                // See the matching branch in walk_direct_statement: a later
                // bare/dynamic/orm configuration stops future direct builds
                // but does not invalidate the installed qorm_table fact.
                context.table_package_authority.remove(&package);
            }
        }
        NodeKind::Use { module, args, .. }
            if module != QUICKORM_MODULE && imports_table_builder(args) =>
        {
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::No { module, .. } if module == QUICKORM_MODULE => {
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::ExpressionStatement { expression } if is_competing_import_call(expression) => {
            // A nested competing import call only drops builder authority;
            // it does not by itself remove or redefine the installed member.
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::ExpressionStatement { expression }
            if is_qualified_table_or_view_call(expression, current_package(context)) =>
        {
            // A current-package-qualified `Package::table`/`Package::view`
            // call replaces the installed qorm_table member and must
            // invalidate the prior fact. This branch is *not* an
            // authority-loss-only event.
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
            invalidate_qorm_table_fact(&package, facts);
        }
        NodeKind::MethodCall { .. } if is_competing_import_call(node) => {
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
        }
        NodeKind::FunctionCall { .. }
            if is_qualified_table_or_view_call(node, current_package(context)) =>
        {
            let package = current_package(context).to_string();
            context.table_package_authority.remove(&package);
            invalidate_qorm_table_fact(&package, facts);
        }
        _ => {}
    }

    for child in node.children() {
        walk_compile_time_descendants(child, file_id, context, facts, source);
    }
}

fn current_package(context: &WalkContext) -> &str {
    context.current_package.as_deref().unwrap_or("main")
}

fn imports_table_builder(args: &[String]) -> bool {
    args.iter().any(|arg| imports_table_builder_value(arg))
}

fn imports_table_builder_value(value: &str) -> bool {
    let value = value.trim();
    if matches!(value, "table" | "view" | "'table'" | "\"table\"" | "'view'" | "\"view\"") {
        return true;
    }

    quote_like_words(value)
        .is_some_and(|words| words.into_iter().any(|word| matches!(word, "table" | "view")))
}

/// Parse a Perl quote-like word list as retained by the parser's raw argument
/// representation. This deliberately validates the complete delimiter pair:
/// an incomplete or mismatched recovered token must not invalidate a valid
/// QuickORM authority based on guessed text.
fn quote_like_words(value: &str) -> Option<Vec<&str>> {
    let remainder = value.strip_prefix("qw")?;
    let delimiter = remainder.chars().next()?;
    if delimiter.is_ascii_alphanumeric() || delimiter.is_ascii_whitespace() {
        return None;
    }

    let (closing, paired) = match delimiter {
        '(' => (')', true),
        '[' => (']', true),
        '{' => ('}', true),
        '<' => ('>', true),
        _ => (delimiter, false),
    };
    let content = &remainder[delimiter.len_utf8()..];
    let mut depth = if paired { 1 } else { 0 };
    let mut escaped = false;
    let mut end = None;

    for (offset, character) in content.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if paired && character == delimiter {
            depth += 1;
        } else if character == closing {
            if paired {
                depth -= 1;
                if depth != 0 {
                    continue;
                }
            }
            end = Some(offset);
            break;
        }
    }

    let end = end?;
    if !content[end + closing.len_utf8()..].trim().is_empty() {
        return None;
    }
    Some(content[..end].split_whitespace().collect())
}

fn is_competing_import_call(expression: &Node) -> bool {
    let module = match &expression.kind {
        NodeKind::MethodCall { object, method, .. } if method == "import" => {
            // An unknown receiver may still own the imported DSL. Only an
            // explicitly named QuickORM receiver is known not to compete;
            // variable-held, computed, and otherwise unknown receivers
            // invalidate authority. The argument shape may be a hash/binary
            // expression or another parser form that is not statically
            // enumerable, so unknown external imports are fail-closed.
            let is_quickorm_receiver =
                matches!(&object.kind, NodeKind::Identifier { name } if name == QUICKORM_MODULE);
            if is_quickorm_receiver { QUICKORM_MODULE } else { "<dynamic>" }
        }
        NodeKind::FunctionCall { name, .. } => {
            let Some(module) = name.strip_suffix("::import") else {
                return false;
            };
            module
        }
        _ => return false,
    };

    module != QUICKORM_MODULE
}

fn invalidate_qorm_table_fact(package: &str, facts: &mut Vec<GeneratedMemberFact>) {
    let canonical_name = format!("{package}::{QORM_TABLE_MEMBER}");
    facts.retain(|fact| fact.entity.canonical_name != canonical_name);
}

fn is_direct_table_or_view_call(expression: &Node) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    matches!(name.as_str(), "table" | "view") && !args.is_empty()
}

fn is_table_or_view_call(expression: &Node, package: &str) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    is_direct_table_or_view_call(expression)
        || (!args.is_empty() && is_current_package_qualified_call(name, package))
}

fn is_qualified_table_or_view_call(expression: &Node, package: &str) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    !args.is_empty()
        && (name.strip_suffix("::table") == Some(package)
            || name.strip_suffix("::view") == Some(package))
}

fn is_current_package_qualified_call(name: &str, package: &str) -> bool {
    name.strip_suffix("::table") == Some(package) || name.strip_suffix("::view") == Some(package)
}

fn direct_table_or_view_builder_anchor<'a>(
    expression: &'a Node,
    source: Option<&str>,
) -> Option<&'a Node> {
    let NodeKind::FunctionCall { args, .. } = &expression.kind else {
        return None;
    };
    if !is_direct_table_or_view_call(expression)
        || !args.iter().skip(1).any(is_direct_builder_argument)
    {
        return None;
    }

    static_table_name_anchor(args.first()?, source)
}

/// Permissive argument-shape check, not a QuickORM grammar check: it decides
/// only whether the call carries a direct package-level body to anchor to.
/// `HashLiteral` is admitted for that reason alone and must not be used to
/// distinguish a real builder body from arbitrary argument syntax.
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

fn static_table_name_anchor<'a>(node: &'a Node, source: Option<&str>) -> Option<&'a Node> {
    match &node.kind {
        NodeKind::String { value, .. } => {
            let raw = source
                .and_then(|text| text.get(node.location.start..node.location.end))
                .unwrap_or(value);
            // Quote-like operator path: `q{}/q()/q//q!!`, `qq{}/qq()/qq//qq!!`,
            // and the plain `'`/`"` delimiters. Backticks and `qx{}/qx()` are
            // execution boundaries and never reach this gate; their forms are
            // intentionally absent from `QuoteLikeForm`.
            if let Some((body, form, _remainder)) = parse_quote_like_literal(raw) {
                if body.trim().is_empty() {
                    return None;
                }
                if form.interpolates() && contains_unescaped_interpolation(body) {
                    return None;
                }
                return Some(node);
            }
            // Parser-supplied fallback for `'...'` / `"..."` when the source
            // slice is unavailable (older workspace_index path). The body
            // shape has already been validated by `string_literal_inner_text`.
            if let Some((literal, delimiter)) = string_literal_inner_text(raw) {
                if literal.trim().is_empty()
                    || (delimiter == b'"' && contains_unescaped_interpolation(raw))
                {
                    return None;
                }
                return Some(node);
            }
            is_static_identifier(raw).then_some(node)
        }
        NodeKind::Identifier { name } if is_static_identifier(name) => Some(node),
        NodeKind::Binary { op, left, .. } if op == "=>" => static_table_name_anchor(left, source),
        _ => None,
    }
}

/// Recover the bareword builder keyword (`table` or `view`) from the
/// immediately previous direct statement when it was an isolated
/// `ExpressionStatement(Identifier("table"))` or `Identifier("view")`.
///
/// The parser emits this two-statement shape for `table q(NAME) => BODY`
/// because it cannot disambiguate the quote-like operator from a hash
/// literal context.
fn previous_table_or_view_identifier(previous: Option<&Node>) -> Option<String> {
    let previous = previous?;
    let NodeKind::ExpressionStatement { expression } = &previous.kind else {
        return None;
    };
    let NodeKind::Identifier { name } = &expression.kind else {
        return None;
    };
    matches!(name.as_str(), "table" | "view").then(|| name.clone())
}

/// Apply the QuickORM table-package fact rules to a `HashLiteral` recovered
/// from the `table KEY => BODY` parser shape. Mirrors the FunctionCall arm
/// in `walk_direct_statement`: authority is consumed first, then a static
/// table-name anchor emits the fact and a non-static body invalidates any
/// prior installed fact. The `builder_keyword` parameter is accepted for
/// symmetry and possible future routing; today only its `table`/`view`
/// validity determines admission.
fn handle_hash_literal_builder_call(
    pairs: &[(Node, Node)],
    _builder_keyword: &str,
    file_id: FileId,
    context: &mut WalkContext,
    facts: &mut Vec<GeneratedMemberFact>,
    source: Option<&str>,
) {
    let package = current_package(context).to_string();

    if !context.table_package_authority.contains(&package)
        || context.shadowed_builders.contains(&package)
    {
        return;
    }

    let Some((key_node, value_node)) = pairs.first() else {
        return;
    };
    if pairs.len() > 1 {
        // A multi-pair hash literal cannot be a QuickORM builder call.
        return;
    }
    if !is_direct_builder_argument(value_node) {
        // The body is not a direct `sub {}` / `{}` / `HashLiteral` shape; if
        // a fact was previously installed it must be invalidated because
        // QuickORM consumes authority on every builder attempt.
        context.table_package_authority.remove(&package);
        invalidate_qorm_table_fact(&package, facts);
        return;
    }

    // Authority consumed regardless of the anchor outcome.
    context.table_package_authority.remove(&package);

    let Some(anchor) = static_table_name_anchor(key_node, source) else {
        invalidate_qorm_table_fact(&package, facts);
        return;
    };
    push_qorm_table_fact(&package, anchor, file_id, facts);
}

fn string_literal_inner_text(value: &str) -> Option<(&str, u8)> {
    let bytes = value.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(quote), Some(closing_quote))
            if value.len() >= 2 && quote == closing_quote && matches!(*quote, b'\'' | b'"') =>
        {
            Some((&value[1..value.len() - 1], *quote))
        }
        _ => None,
    }
}

/// Parse a quote-like literal at the start of `source`.
///
/// Recognises `'...'`, `"..."`, the non-interpolating `q{}` family, and the
/// interpolating `qq{}` family. Rejects `` `...` `` and the `qx{}` family
/// (execution boundaries) and any unterminated or unbalanced form. The
/// returned `&str` is the body strictly between the opening and closing
/// delimiters; the third element is the remainder of `source` after the
/// closing delimiter.
///
/// The body is taken verbatim — no escape processing, no interpolation
/// resolution. The caller is responsible for deciding whether the form's
/// interpolation behavior admits the body as a static literal.
fn parse_quote_like_literal(source: &str) -> Option<(&str, QuoteLikeForm, &str)> {
    let bytes = source.as_bytes();
    let first = *bytes.first()?;

    let (operator_len, form) = match first {
        b'\'' => (1usize, QuoteLikeForm::SingleQuoted),
        b'"' => (1usize, QuoteLikeForm::DoubleQuoted),
        b'`' => return None,
        b'q' => {
            let next = *bytes.get(1)?;
            match next {
                b'q' => (2, QuoteLikeForm::QqInterpolating),
                b'x' => return None,
                _ if next.is_ascii_alphabetic() => return None,
                _ => (1, QuoteLikeForm::QLiteral),
            }
        }
        _ => return None,
    };

    let after_operator = source.get(operator_len..)?;
    let opening = *after_operator.as_bytes().first()?;
    if opening.is_ascii_whitespace() {
        return None;
    }

    let paired = matches!(opening, b'(' | b'[' | b'{' | b'<');
    let closing = if paired {
        match opening {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            _ => unreachable!(),
        }
    } else {
        opening
    };

    let after_open = after_operator.get(1..)?;
    let after_open_bytes = after_open.as_bytes();

    if paired {
        // Balance nesting so `q((foo))` reads `(foo)` as the body. The walk
        // is single-byte because all four paired delimiters are ASCII; the
        // outer bytes of a non-paired form are likewise ASCII.
        let mut depth = 1usize;
        let mut escaped = false;
        let mut index = 0usize;
        while index < after_open_bytes.len() {
            let byte = after_open_bytes[index];
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == opening {
                depth += 1;
            } else if byte == closing {
                depth -= 1;
                if depth == 0 {
                    let body = &after_open[..index];
                    let remainder = &after_open[index + 1..];
                    return Some((body, form, remainder));
                }
            }
            index += 1;
        }
        None
    } else {
        // Non-paired delimiters can occur in the body when escaped. Track the
        // escape state so a literal delimiter does not truncate the value.
        let mut escaped = false;
        let end = after_open_bytes.iter().position(|&byte| {
            if escaped {
                escaped = false;
                return false;
            }
            if byte == b'\\' {
                escaped = true;
                return false;
            }
            byte == closing
        })?;
        let body = &after_open[..end];
        let remainder = &after_open[end + 1..];
        Some((body, form, remainder))
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
            && characters.peek().is_some_and(is_perl_interpolation_name_start)
        {
            return true;
        }
    }
    false
}

/// Sigil-suffix characters that begin Perl special variables (perldata):
/// `$^O`, `$::name`, `$&`, `$'`, `` $` ``, `$+`, `$-`, `$?`, `$!`, `$@`, `$#`,
/// `$;`, `$=`, `$.`, `$~`, `$<`, `$>`, `$%`, `$(`, `$)`, `$|`, `$*`, `$$`,
/// `$[`, `$]`.
///
/// Identifier starts are Unicode-aware: under `use utf8` a Perl identifier may
/// begin with any Unicode word character, so those are interpolation, not literal
/// text. The punctuation/special-variable set below is closed on purpose, so
/// admitting a new special form requires a falsifier test alongside the addition.
fn is_perl_interpolation_name_start(character: &char) -> bool {
    character.is_alphanumeric()
        || matches!(
            character,
            '_' | '{'
                | ':'
                | '^'
                | '&'
                | '\''
                | '`'
                | '+'
                | '-'
                | '?'
                | '!'
                | '@'
                | '#'
                | ';'
                | '='
                | '.'
                | '~'
                | '<'
                | '>'
                | '%'
                | '('
                | ')'
                | '|'
                | '*'
                | '$'
                | '['
                | ']'
        )
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
    let existing_index = facts.iter().position(|fact| fact.entity.canonical_name == canonical_name);

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
        // Exact source-backed QuickORM configuration, direct package-level
        // builder shape, and a source anchor satisfy the policy requirement
        // for live generated workspace symbols.
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

    let fact = GeneratedMemberFact { entity, anchor };
    if let Some(index) = existing_index {
        // A later valid import/build is a fresh source anchor for the same
        // canonical member. Do not retain the stale first declaration.
        facts[index] = fact;
    } else {
        facts.push(fact);
    }
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
    let end = source_statement_end(remainder)
        .unwrap_or_else(|| node.location.end.saturating_sub(node.location.start));
    remainder.get(..end)
}

fn source_statement_end(source: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;

    for (index, character) in source.char_indices() {
        if comment {
            if matches!(character, '\r' | '\n') {
                comment = false;
            }
            continue;
        }

        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }

        match character {
            '#' => comment = true,
            '\'' | '"' => quote = Some(character),
            ';' => return Some(index + character.len_utf8()),
            _ => {}
        }
    }

    None
}

fn exact_source_import_pair(source: &str) -> Option<(&str, &str)> {
    let mut rest = trim_source_trivia(source);
    let use_keyword = rest.strip_prefix("use")?;
    if !use_keyword.chars().next().is_none_or(|c| c.is_ascii_whitespace() || c == '#') {
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
    // Quote-like operators on the import side: `q{key}`, `qq{value}`, etc.
    // The classifier only needs the body and remainder; interpolation and
    // emptiness checks live in `quoted_import_value` so the static-key path
    // stays symmetric for `'key'` and `q{key}`.
    if let Some((body, form, remainder)) = parse_quote_like_literal(source) {
        if matches!(form, QuoteLikeForm::SingleQuoted | QuoteLikeForm::DoubleQuoted) {
            // Plain quote forms were caught by the branch above; reaching
            // here means the closing delimiter is missing from the slice.
            // Fall through to bareword handling rather than silently accept.
        } else {
            return Some((body, remainder));
        }
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
    if value.is_empty() {
        return None;
    }

    // Quote-like operator path: `q(value)`, `qq(value)`, etc. Rejects `qx`
    // and backticks because `parse_quote_like_literal` returns `None` for
    // those forms, leaving the plain-delimiter branches to fail closed.
    if let Some((body, form, _remainder)) = parse_quote_like_literal(value) {
        if body.is_empty() {
            return None;
        }
        if form.interpolates() && contains_unescaped_interpolation(body) {
            return None;
        }
        return Some(body.to_string());
    }

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
