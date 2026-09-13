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
//!
//! # Containment (temporary, pending #13238)
//!
//! Upstream DBIx::QuickORM `0.000029` does not model package state as one
//! "current builder" bit. Each import installs a set of actual local names and
//! records them for a later `unimport`. A second import can overwrite some of
//! the names it installs while leaving a distinct earlier renamed alias live,
//! so no single bit — and no latest-import-wins rule — reproduces the result.
//!
//! Until #13238 supplies the exact installed-name state, this extractor admits
//! only the one shape it can prove:
//!
//! ```text
//! exactly one parser-proven exact QuickORM table import for the package
//! + no second QuickORM import/`no`/reconfiguration for that package
//! + the first admitted direct table builder
//! → candidate fields
//!
//! anything else
//! → no candidate for the whole package
//! ```
//!
//! Suppression is whole-package and retroactive: a later import event discards
//! candidates already collected for that package. Losing a non-published
//! candidate is preferable to preserving false exactness.

use crate::{Node, NodeKind};
use perl_semantic_facts::{
    AnchorFact, AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, Provenance,
};
use std::collections::{BTreeMap, BTreeSet};

/// What this extractor can prove about one package's QuickORM import state.
///
/// There is deliberately no variant meaning "table mode was replaced by a
/// later import": that is the upstream claim this containment slice retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageImportState {
    /// Exactly one QuickORM import, proven to be the exact table-class form.
    ExactTableImport,
    /// Exactly one QuickORM import, in a form this extractor does not admit.
    UnadmittedSingleImport,
    /// More than one QuickORM import/`no` event, or an event whose effect on
    /// installed names cannot be modeled. Suppresses the package entirely.
    NotProven,
}

#[derive(Debug, Clone, Default)]
struct QuickOrmWalkCtx {
    current_package: Option<String>,
    imports: BTreeMap<String, PackageImportState>,
    consumed_builders: BTreeSet<String>,
}

impl QuickOrmWalkCtx {
    fn package(&self) -> &str {
        self.current_package.as_deref().unwrap_or("main")
    }

    fn table_builder_active(&self) -> bool {
        let package = self.package();
        self.imports.get(package) == Some(&PackageImportState::ExactTableImport)
            && !self.consumed_builders.contains(package)
    }

    /// Record one `use DBIx::QuickORM` event for the current package.
    ///
    /// The first import is modeled from its own proven form. Any later import
    /// makes the package `NotProven`, because this extractor cannot tell which
    /// installed names survived it.
    fn record_import(&mut self, exact_table_class: bool) {
        let package = self.package().to_string();
        let next = match self.imports.get(&package) {
            None if exact_table_class => PackageImportState::ExactTableImport,
            None => PackageImportState::UnadmittedSingleImport,
            Some(_) => PackageImportState::NotProven,
        };
        self.imports.insert(package, next);
    }

    /// Record one `no DBIx::QuickORM` event for the current package.
    ///
    /// `unimport` removes the names recorded by a specific earlier import. This
    /// extractor does not retain those name sets, so the package is suppressed.
    fn record_unimport(&mut self) {
        let package = self.package().to_string();
        self.imports.insert(package, PackageImportState::NotProven);
    }

    /// Record that the package's one-shot table builder has already run.
    fn consume_current_builder(&mut self) {
        let package = self.package().to_string();
        self.consumed_builders.insert(package);
    }

    fn is_suppressed(&self, package: &str) -> bool {
        self.imports.get(package) == Some(&PackageImportState::NotProven)
    }
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

/// A candidate collected during the walk, retained with the package that
/// declared it so a later import event can suppress it.
#[derive(Debug, Clone)]
struct PendingCandidate {
    package: String,
    fact: QuickOrmColumnFact,
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
    let mut pending = Vec::new();
    let mut ctx = QuickOrmWalkCtx::default();
    walk_quickorm(ast, file_id, &mut ctx, &mut pending);

    // Import state is only final once the whole file has been walked: an import
    // event after a table declaration still suppresses the package.
    pending
        .into_iter()
        .filter(|candidate| !ctx.is_suppressed(&candidate.package))
        .map(|candidate| candidate.fact)
        .collect()
}

fn walk_quickorm(
    node: &Node,
    file_id: FileId,
    ctx: &mut QuickOrmWalkCtx,
    out: &mut Vec<PendingCandidate>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            for statement in statements {
                walk_quickorm(statement, file_id, ctx, out);
            }
        }
        NodeKind::Block { statements } => {
            // `package` is lexical to the block, but imported functions live in
            // package symbol tables. Restore only the current package; builder
            // installation/removal remains visible through its package key.
            let saved_package = ctx.current_package.clone();
            for statement in statements {
                walk_quickorm(statement, file_id, ctx, out);
            }
            ctx.current_package = saved_package;
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved_package = ctx.current_package.clone();
                ctx.current_package = Some(name.clone());
                walk_quickorm(block, file_id, ctx, out);
                ctx.current_package = saved_package;
            } else {
                ctx.current_package = Some(name.clone());
            }
        }
        NodeKind::Use { module, args, .. } if module == "DBIx::QuickORM" => {
            ctx.record_import(is_explicit_table_class_import(args));
        }
        NodeKind::No { module, .. } if module == "DBIx::QuickORM" => {
            ctx.record_unimport();
        }
        NodeKind::ExpressionStatement { expression } if ctx.table_builder_active() => {
            // In a type=table builder, the first table() call removes the DSL
            // functions from the package. Close the package candidate after
            // any table invocation, even when its identity is too dynamic to
            // model safely.
            if extract_table_declaration(expression, file_id, ctx, out) {
                ctx.consume_current_builder();
            }
        }
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {
            // A `table` call in a deferred definition has not executed, but
            // `use`/`no` are compile-time: they run wherever they appear. Scan
            // the body for import events only, so a nested import cannot escape
            // containment while a nested `table` call still stays deferred.
            record_nested_import_events(node, ctx);
        }
        _ => {
            // A declaration or nested call can still execute the one-shot
            // builder's first `table` invocation (for example
            // `my $first = table ...`). Detect it before descending so a
            // later bare statement cannot emit false candidates.
            if ctx.table_builder_active() && extract_table_declaration(node, file_id, ctx, out) {
                ctx.consume_current_builder();
            }
            for child in node.children() {
                walk_quickorm(child, file_id, ctx, out);
            }
        }
    }
}

/// Record QuickORM import events inside a deferred definition.
///
/// This deliberately emits no candidates and consumes no builder: only the
/// compile-time import events matter here.
fn record_nested_import_events(node: &Node, ctx: &mut QuickOrmWalkCtx) {
    match &node.kind {
        NodeKind::Block { statements } => {
            // Same lexical rule as the main walk: a `package` inside the body
            // must not leak back out to the enclosing statement sequence.
            let saved_package = ctx.current_package.clone();
            for statement in statements {
                record_nested_import_events(statement, ctx);
            }
            ctx.current_package = saved_package;
        }
        NodeKind::Package { name, block, .. } => {
            let saved_package = ctx.current_package.clone();
            ctx.current_package = Some(name.clone());
            if let Some(block) = block {
                record_nested_import_events(block, ctx);
                ctx.current_package = saved_package;
            }
        }
        NodeKind::Use { module, args, .. } if module == "DBIx::QuickORM" => {
            ctx.record_import(is_explicit_table_class_import(args));
        }
        NodeKind::No { module, .. } if module == "DBIx::QuickORM" => {
            ctx.record_unimport();
        }
        _ => {
            for child in node.children() {
                record_nested_import_events(child, ctx);
            }
        }
    }
}

/// One normalized `use DBIx::QuickORM` import argument.
///
/// `proven_static` records whether the *source form* proves the argument is a
/// literal string. A bareword is not proven: `type => table` is
/// indistinguishable from a constant or a `table()` call resolved at runtime,
/// so it cannot earn the same admission as `type => 'table'`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportArg {
    value: String,
    proven_static: bool,
}

/// An exact table-class import is `type` followed by a *proven static* `table`.
///
/// The key may be a bareword: under a fat arrow Perl autoquotes it, and the
/// `qw`/quoted spellings carry their own proof. The value may not, because a
/// bareword there is a live expression this extractor cannot evaluate.
fn is_explicit_table_class_import(args: &[String]) -> bool {
    let normalized = normalized_import_args(args);
    matches!(
        normalized.as_slice(),
        [type_key, table_value]
            if type_key.value == "type"
                && table_value.value == "table"
                && table_value.proven_static
    )
}

fn normalized_import_args(args: &[String]) -> Vec<ImportArg> {
    let mut normalized = Vec::new();

    for arg in args {
        let trimmed = arg.trim();
        if let Some(words) = parse_qw_words(trimmed) {
            // `qw` autoquotes every word, so each one is a proven literal.
            normalized.extend(words.into_iter().filter_map(|word| {
                normalize_symbol_name(&word).map(|value| ImportArg { value, proven_static: true })
            }));
        } else if !matches!(trimmed, "" | "," | "=>")
            && let Some(arg) = classify_import_arg(trimmed)
        {
            normalized.push(arg);
        }
    }

    normalized
}

fn classify_import_arg(raw: &str) -> Option<ImportArg> {
    let trimmed = raw.trim();
    let value = normalize_symbol_name(trimmed)?;
    Some(ImportArg { value, proven_static: is_static_string_literal(trimmed) })
}

/// Whether a raw import token is a quoted literal with no interpolation.
fn is_static_string_literal(raw: &str) -> bool {
    // Taking both ends from the same iterator rejects a lone quote character,
    // where `next_back` finds nothing left.
    let mut chars = raw.chars();
    let (Some(open), Some(close)) = (chars.next(), chars.next_back()) else {
        return false;
    };
    match (open, close) {
        ('\'', '\'') => true,
        // A double-quoted value interpolates, so only a sigil-free body is a
        // proven literal.
        ('"', '"') => !raw.contains('$') && !raw.contains('@'),
        _ => false,
    }
}

/// Find the first `table` invocation reachable in an executable expression.
///
/// The search does not descend into `sub`/`method` bodies: those are deferred
/// definitions, so a `table` call inside them has not executed yet.
fn find_executable_table_call(expression: &Node) -> Option<&Node> {
    match &expression.kind {
        NodeKind::FunctionCall { name, .. } if name == "table" => Some(expression),
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => None,
        _ => expression.children().into_iter().find_map(find_executable_table_call),
    }
}

/// Inspect one statement or expression and return whether it executed the
/// one-shot table DSL.
fn extract_table_declaration(
    expression: &Node,
    file_id: FileId,
    ctx: &QuickOrmWalkCtx,
    out: &mut Vec<PendingCandidate>,
) -> bool {
    let Some(call) = find_executable_table_call(expression) else {
        return false;
    };
    let NodeKind::FunctionCall { args, .. } = &call.kind else {
        return false;
    };

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
    out: &mut Vec<PendingCandidate>,
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
    out: &mut Vec<PendingCandidate>,
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
    out: &mut Vec<PendingCandidate>,
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
    out: &mut Vec<PendingCandidate>,
) {
    let Some(name) = normalize_static_field_name(&candidate.name) else {
        return;
    };
    push_field(ctx.package(), &name, &candidate, file_id, out);
}

fn single_static_name_candidate(node: &Node) -> Option<NameCandidate> {
    let mut candidates = collect_static_name_candidates(node).into_iter();
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn collect_static_name_candidates(node: &Node) -> Vec<NameCandidate> {
    match &node.kind {
        NodeKind::String { value, interpolated: false } | NodeKind::Identifier { name: value } => {
            expand_symbol_list(value)
                .into_iter()
                .map(|name| NameCandidate {
                    name,
                    span_start: node.location.start,
                    span_end: node.location.end,
                })
                .collect()
        }
        NodeKind::Binary { op, left, right } if op == "," => {
            let mut names = collect_static_name_candidates(left);
            names.extend(collect_static_name_candidates(right));
            names
        }
        // A bare `qw/name email/` argument parses as an array literal of word
        // strings. An explicit arrayref group wraps it in another array
        // literal and stays unmodeled (see the arrayref negative control).
        NodeKind::ArrayLiteral { elements } => elements
            .iter()
            .filter_map(|element| match &element.kind {
                NodeKind::String { value, interpolated: false } => Some(NameCandidate {
                    name: normalize_symbol_name(value)?,
                    span_start: element.location.start,
                    span_end: element.location.end,
                }),
                _ => None,
            })
            .collect(),
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
    parse_qw_words(raw).unwrap_or_else(|| normalize_symbol_name(raw).into_iter().collect())
}

fn parse_qw_words(raw: &str) -> Option<Vec<String>> {
    let delimited = raw.trim().strip_prefix("qw")?.trim_start();
    let open = delimited.chars().next()?;
    let close = match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        delimiter if !delimiter.is_ascii_alphanumeric() && !delimiter.is_whitespace() => delimiter,
        _ => return None,
    };
    let inner = delimited.strip_prefix(open)?.strip_suffix(close)?;
    Some(inner.split_whitespace().filter(|name| !name.is_empty()).map(str::to_string).collect())
}

fn push_field(
    package: &str,
    field_name: &str,
    source_name: &NameCandidate,
    file_id: FileId,
    out: &mut Vec<PendingCandidate>,
) {
    let canonical_name = format!("{package}::{field_name}");
    if out.iter().any(|candidate| {
        candidate.fact.entity.canonical_name == canonical_name
            && candidate.fact.anchor.span_start_byte as usize == source_name.span_start
            && candidate.fact.anchor.span_end_byte as usize == source_name.span_end
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
    out.push(PendingCandidate {
        package: package.to_string(),
        fact: QuickOrmColumnFact { entity, anchor },
    });
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
            r#"
package My::ORM::Table::Parenthesized;
use DBIx::QuickORM(type => 'table');
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
    fn qw_punctuation_is_a_value_not_import_syntax() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM qw(type => table);
table users => sub { column id => sub { primary_key }; };
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn case_or_extra_import_parameters_do_not_activate_the_candidate() {
        for source in [
            r#"
package My::ORM::Table::Case;
use DBIx::QuickORM TYPE => 'TABLE';
table users => sub { column id => sub { primary_key }; };
1;
"#,
            r#"
package My::ORM::Table::Extra;
use DBIx::QuickORM type => 'table', skip => [];
table users => sub { column id => sub { primary_key }; };
1;
"#,
        ] {
            let facts = candidate_facts(source);
            assert!(
                !facts.iter().any(|fact| fact.entity.canonical_name.ends_with("::id")),
                "unsupported import list must remain blocked: {source}"
            );
        }
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
        assert!(
            !production.iter().any(|fact| fact.entity.canonical_name == "My::ORM::Table::User::id")
        );
    }

    #[test]
    fn package_builder_state_survives_blocks_and_package_reentry() {
        let facts = candidate_facts(
            r#"
package Outer;
{
    use DBIx::QuickORM type => 'table';
    package Inner;
    use DBIx::QuickORM type => 'table';
}

package Outer;
table outer => sub { column outer_id => sub { primary_key }; };

package Inner;
table inner => sub { column inner_id => sub { primary_key }; };
1;
"#,
        );

        assert!(has_name(&facts, "Outer::outer_id"));
        assert!(has_name(&facts, "Inner::inner_id"));
    }

    #[test]
    fn a_second_import_fails_the_package_closed_without_affecting_its_neighbour() {
        let facts = candidate_facts(
            r#"
package Outer;
use DBIx::QuickORM type => 'table';

package Inner;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM;

package Outer;
table outer => sub { column outer_id => sub { primary_key }; };

package Inner;
table inner => sub { column inner_id => sub { primary_key }; };
1;
"#,
        );

        // `Inner` is suppressed because two imports happened, not because the
        // second one is modeled as replacing the first.
        assert!(has_name(&facts, "Outer::outer_id"));
        assert!(!has_name(&facts, "Inner::inner_id"));
    }

    #[test]
    fn repeated_import_order_and_shape_never_selects_a_surviving_builder() {
        // Each case has two QuickORM import events for one package. Under the
        // containment slice none of them may guess which names stayed live.
        for (label, source) in [
            (
                "table then plain",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM;
table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "plain then table",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM;
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "table then renamed alias",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM type => 'table', rename => { table => 'qorm_table' };
table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "two distinct aliases",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table', rename => { table => 'tbl_a' };
use DBIx::QuickORM type => 'table', rename => { table => 'tbl_b' };
table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "table then exact repeat",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
        ] {
            let facts = candidate_facts(source);
            assert!(
                !has_name(&facts, "My::ORM::Table::User::id"),
                "repeated import must fail closed: {label}"
            );
        }
    }

    #[test]
    fn an_import_after_the_table_declaration_retracts_the_package_candidates() {
        // The walk reaches `table` while the package still looks exact. Import
        // state is only final at end of file, so the candidate must be dropped.
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub { column id => sub { primary_key }; };

use DBIx::QuickORM;
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn an_import_inside_a_deferred_definition_still_fails_the_package_closed() {
        // `use`/`no` run at compile time wherever they appear, so a sub body is
        // not a hiding place for a second import event. The companion control
        // in `deferred_sub_body_table_call_does_not_consume_the_builder` proves
        // a nested `table` call is still treated as deferred.
        for (label, source) in [
            (
                "nested plain import",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

sub helper { use DBIx::QuickORM; }

table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "nested unimport",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

sub helper { no DBIx::QuickORM; }

table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            // `method` is a distinct node kind sharing the same walk arm, so it
            // needs its own case rather than inheriting the `sub` result.
            (
                "nested import in a method body",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

method helper { use DBIx::QuickORM; }

table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
            (
                "nested unimport in a method body",
                r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

method helper { no DBIx::QuickORM; }

table users => sub { column id => sub { primary_key }; };
1;
"#,
            ),
        ] {
            let facts = candidate_facts(source);
            assert!(
                !has_name(&facts, "My::ORM::Table::User::id"),
                "nested import event must fail the package closed: {label}"
            );
        }
    }

    #[test]
    fn a_package_inside_a_deferred_definition_does_not_leak_to_the_outer_walk() {
        // If the nested import scan left `Inner` current, the outer `table`
        // call would be attributed to the wrong package and silently lost.
        let facts = candidate_facts(
            r#"
package Outer;
use DBIx::QuickORM type => 'table';

sub helper {
    package Inner;
    use DBIx::QuickORM;
}

table outer => sub { column outer_id => sub { primary_key }; };
1;
"#,
        );

        assert!(has_name(&facts, "Outer::outer_id"));
        assert!(!has_name(&facts, "Inner::outer_id"));
    }

    #[test]
    fn unimport_removes_candidate_authority_for_the_whole_package() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub { column id => sub { primary_key }; };

no DBIx::QuickORM;
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::id"));
    }

    #[test]
    fn a_bareword_table_value_is_not_proven_static() {
        // `type => table` is indistinguishable from a constant or a `table()`
        // call, so it must not inherit the admission of `type => 'table'`.
        for source in [
            r#"
package My::ORM::Table::Bare;
use DBIx::QuickORM type => table;
table users => sub { column id => sub { primary_key }; };
1;
"#,
            r#"
package My::ORM::Table::Call;
use DBIx::QuickORM type => table();
table users => sub { column id => sub { primary_key }; };
1;
"#,
        ] {
            let facts = candidate_facts(source);
            assert!(
                !facts.iter().any(|fact| fact.entity.canonical_name.ends_with("::id")),
                "unproven import value must remain blocked: {source}"
            );
        }

        // The proven quoted spelling still activates, so the block above is
        // discriminating rather than vacuous.
        let proven = candidate_facts(
            r#"
package My::ORM::Table::Quoted;
use DBIx::QuickORM type => 'table';
table users => sub { column id => sub { primary_key }; };
1;
"#,
        );
        assert!(has_name(&proven, "My::ORM::Table::Quoted::id"));
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
    fn assigned_static_table_call_publishes_and_consumes_the_builder() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

my $first = table users => sub {
    column id => sub { primary_key };
};
table admins => sub {
    column admin_id => sub { primary_key };
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
        assert!(!has_name(&facts, "My::ORM::Table::User::admin_id"));
    }

    #[test]
    fn assigned_dynamic_table_call_consumes_without_publishing() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

my $first = table $dynamic => sub {
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
    fn table_call_nested_in_a_call_argument_consumes_the_builder() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

register_schema( table users => sub {
    column id => sub { primary_key };
} );
table admins => sub {
    column admin_id => sub { primary_key };
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
        assert!(!has_name(&facts, "My::ORM::Table::User::admin_id"));
    }

    #[test]
    fn deferred_sub_body_table_call_does_not_consume_the_builder() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

sub install_later {
    table deferred => sub {
        column deferred_id => sub { primary_key };
    };
}

table users => sub {
    column id => sub { primary_key };
};
1;
"#,
        );

        assert!(has_name(&facts, "My::ORM::Table::User::id"));
        assert!(!has_name(&facts, "My::ORM::Table::User::deferred_id"));
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
    fn dynamic_and_interpolated_column_names_remain_unmodeled() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';
my $column_name = 'nickname';
my $suffix = 'name';

table users => sub {
    column $column_name => sub { type VARCHAR };
    column "display_$suffix" => sub { type VARCHAR };
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::nickname"));
        assert!(!has_name(&facts, "My::ORM::Table::User::column_name"));
        assert!(!has_name(&facts, "My::ORM::Table::User::display_name"));
    }

    #[test]
    fn arrayref_column_names_are_not_scalar_dsl_arguments() {
        let facts = candidate_facts(
            r#"
package My::ORM::Table::User;
use DBIx::QuickORM type => 'table';

table users => sub {
    columns([qw/name email/], sub { type VARCHAR });
};
1;
"#,
        );

        assert!(!has_name(&facts, "My::ORM::Table::User::name"));
        assert!(!has_name(&facts, "My::ORM::Table::User::email"));
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
