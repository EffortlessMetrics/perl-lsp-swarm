//! Canonical TestItem discovery producer.
//!
//! Turns one accepted source generation and parser AST into a validated
//! [`TestItemSnapshot`]. This module owns discovery only: it does not run tests,
//! parse TAP, or emit LSP, DAP, or VS Code types.
//!
//! Framework identity is an explicit input. Seeing a `subtest` call is a
//! parser-backed compatibility fact, not canonical FrameworkAdapter proof.

use perl_parser_core::{Node, NodeKind};

use crate::file::{FileRole, ParseStatus};
use crate::id::Digest;
use crate::provenance::Confidence;
use crate::range::{SourceRange, Utf8LineIndex};
use crate::test_item::{
    TestFrameworkIdentity, TestItem, TestItemCapabilities, TestItemId, TestItemKind, TestItemName,
    TestItemSnapshot, TestItemValidationError,
};

/// Conservative named-subroutine discovery policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedSubroutinePolicy {
    /// Do not emit named-subroutine items.
    Off,
    /// File- and package-scope `sub test_*` / `sub *_test` only.
    ConservativeFileScope,
}

/// Inputs required to discover one generation-bound snapshot.
#[derive(Debug, Clone, Copy)]
pub struct TestItemDiscoveryRequest<'a> {
    /// Opaque revision-independent logical-source reference.
    pub source_ref: &'a crate::test_item::SourceIdentityRef,
    /// Exact source bytes for this generation.
    pub source: &'a str,
    /// Accepted source/document generation.
    pub generation: u64,
    /// Parser AST for this generation. May contain recovery nodes.
    pub ast: &'a Node,
    /// Parser terminal state for this generation.
    pub parse_status: ParseStatus,
    /// Display label for the file item. Not used as identity.
    pub display_name: &'a str,
    /// File role used to decide recognized-test-file behavior.
    pub file_role: FileRole,
    /// Framework identity from import/adapter facts, when available.
    pub framework: Option<&'a TestFrameworkIdentity>,
    /// Named-subroutine policy for this discovery.
    pub named_subroutine_policy: NamedSubroutinePolicy,
}

/// Failure to produce a validated discovery snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestItemDiscoveryError {
    /// File display name would violate the named-item contract.
    EmptyDisplayName,
    /// Source length cannot be represented as a `u32` range bound.
    SourceTooLarge {
        /// Observed byte length.
        observed: usize,
    },
    /// The assembled snapshot failed validation.
    InvalidSnapshot(TestItemValidationError),
}

impl std::fmt::Display for TestItemDiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDisplayName => {
                formatter.write_str("test discovery requires a non-empty file display name")
            }
            Self::SourceTooLarge { observed } => {
                write!(formatter, "source length {observed} exceeds u32 range bounds")
            }
            Self::InvalidSnapshot(error) => {
                write!(formatter, "discovered snapshot is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for TestItemDiscoveryError {}

/// Compatibility mismatch between canonical snapshot and parser-backed subtests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityMismatchKind {
    /// Parser-backed item has no snapshot counterpart.
    MissingItem,
    /// Snapshot item has no parser-backed counterpart.
    ExtraItem,
    /// Stable identity fields disagree for a matched pair.
    IdentityMismatch,
    /// Source ranges disagree for a matched pair.
    RangeMismatch,
    /// Parent/child structure disagrees for a matched pair.
    HierarchyMismatch,
    /// Static vs dynamic name state disagrees.
    NameStateMismatch,
    /// Capability bits disagree for a matched pair.
    CapabilityMismatch,
    /// Framework or provenance fields disagree.
    FrameworkOrProvenanceMismatch,
    /// Snapshot freshness disagrees with the compared generation.
    FreshnessMismatch,
    /// Parser-backed result is a known false positive/negative.
    LegacyKnownFalse,
    /// Snapshot result is a known false positive/negative.
    SnapshotKnownFalse,
    /// Comparison could not be completed.
    InstrumentFailure,
}

/// One classified difference against parser-backed discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityMismatch {
    /// Difference class.
    pub kind: CompatibilityMismatchKind,
    /// Snapshot item involved, when one exists.
    pub snapshot_id: Option<TestItemId>,
    /// Human-readable discriminator for fixtures and later consumers.
    pub detail: String,
}

/// Parser-backed subtest tree used only for compatibility comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserBackedSubtest {
    /// Static or dynamic name.
    pub name: TestItemName,
    /// Full call range.
    pub range: SourceRange,
    /// Name-argument range.
    pub name_range: SourceRange,
    /// Nested parser-backed subtests.
    pub children: Vec<ParserBackedSubtest>,
}

/// Discover a validated generation-bound [`TestItemSnapshot`].
pub fn discover_test_item_snapshot(
    request: &TestItemDiscoveryRequest<'_>,
) -> Result<TestItemSnapshot, TestItemDiscoveryError> {
    if request.display_name.is_empty() {
        return Err(TestItemDiscoveryError::EmptyDisplayName);
    }
    let source_len = u32::try_from(request.source.len()).map_err(|_overflow| {
        TestItemDiscoveryError::SourceTooLarge { observed: request.source.len() }
    })?;
    let index = Utf8LineIndex::new(request.source);
    let digest = Digest::of(request.source);
    let file = file_item(request, &digest, &index, source_len);
    let mut items = vec![file.clone()];

    if request.parse_status != ParseStatus::Failed && request.parse_status != ParseStatus::NotParsed
    {
        let mut calls = Vec::new();
        collect_subtest_calls(
            request.ast,
            request.source,
            &index,
            source_len,
            SubtestNameSet::Canonical,
            &mut calls,
        );
        append_subtest_items(request, &digest, &file, &calls, &mut items);

        if request.named_subroutine_policy == NamedSubroutinePolicy::ConservativeFileScope {
            let mut named = Vec::new();
            collect_named_test_subs(request.ast, &index, source_len, &mut named);
            named.sort_by(|left, right| {
                left.range
                    .start_byte
                    .cmp(&right.range.start_byte)
                    .then(left.range.end_byte.cmp(&right.range.end_byte))
                    .then(left.name.cmp(&right.name))
            });
            append_named_sub_items(request, &digest, &file, &named, &mut items);
        }
    }

    let snapshot = TestItemSnapshot::new(
        request.source_ref.clone(),
        digest,
        request.generation,
        source_len,
        items,
    );
    snapshot.validate().map_err(TestItemDiscoveryError::InvalidSnapshot)?;
    Ok(snapshot)
}

/// Reconstruct the current parser-backed subtest tree for compatibility comparison.
///
/// This oracle matches the live `perl-lsp-rs-core` call-name set: bare
/// `subtest` / `subtest_buffered` / `subtest_streamed` only. Qualified
/// `Test::More::subtest` items are canonical extras, not legacy hits.
pub fn parser_backed_subtests(ast: &Node, source: &str) -> Vec<ParserBackedSubtest> {
    let Ok(source_len) = u32::try_from(source.len()) else {
        return Vec::new();
    };
    let index = Utf8LineIndex::new(source);
    let mut calls = Vec::new();
    collect_subtest_calls(ast, source, &index, source_len, SubtestNameSet::LegacyBare, &mut calls);
    calls.into_iter().map(parser_backed_from_call).collect()
}

/// Classify differences between a snapshot and parser-backed subtests.
pub fn compare_with_parser_backed(
    snapshot: &TestItemSnapshot,
    legacy: &[ParserBackedSubtest],
) -> Vec<CompatibilityMismatch> {
    let mut mismatches = Vec::new();
    let Some(file) = snapshot.items.iter().find(|item| item.kind == TestItemKind::File) else {
        mismatches.push(CompatibilityMismatch {
            kind: CompatibilityMismatchKind::InstrumentFailure,
            snapshot_id: None,
            detail: "snapshot has no file item".to_string(),
        });
        return mismatches;
    };

    mismatches.push(CompatibilityMismatch {
        kind: CompatibilityMismatchKind::ExtraItem,
        snapshot_id: Some(file.id.clone()),
        detail: "file_item".to_string(),
    });

    for item in &snapshot.items {
        if item.kind == TestItemKind::NamedSubroutine {
            mismatches.push(CompatibilityMismatch {
                kind: CompatibilityMismatchKind::ExtraItem,
                snapshot_id: Some(item.id.clone()),
                detail: "named_subroutine".to_string(),
            });
        }
    }

    compare_trees(snapshot, snapshot.children_of(&file.id), legacy, &mut mismatches);
    mismatches
}

fn compare_trees(
    snapshot: &TestItemSnapshot,
    snapshot_children: Vec<&TestItem>,
    legacy_children: &[ParserBackedSubtest],
    mismatches: &mut Vec<CompatibilityMismatch>,
) {
    let snapshot_subtests: Vec<&TestItem> =
        snapshot_children.into_iter().filter(|item| item.kind == TestItemKind::Subtest).collect();
    let paired = snapshot_subtests.len().min(legacy_children.len());
    for index in 0..paired {
        let item = snapshot_subtests[index];
        let legacy = &legacy_children[index];
        if item.name != legacy.name {
            mismatches.push(CompatibilityMismatch {
                kind: CompatibilityMismatchKind::NameStateMismatch,
                snapshot_id: Some(item.id.clone()),
                detail: format!("sibling_index={index}"),
            });
        }
        if item.range != legacy.range || item.name_range != Some(legacy.name_range) {
            mismatches.push(CompatibilityMismatch {
                kind: CompatibilityMismatchKind::RangeMismatch,
                snapshot_id: Some(item.id.clone()),
                detail: format!("sibling_index={index}"),
            });
        }
        if item.generation != snapshot.generation || item.source_digest != snapshot.source_digest {
            mismatches.push(CompatibilityMismatch {
                kind: CompatibilityMismatchKind::FreshnessMismatch,
                snapshot_id: Some(item.id.clone()),
                detail: format!("sibling_index={index}"),
            });
        }
        compare_trees(snapshot, snapshot.children_of(&item.id), &legacy.children, mismatches);
    }
    for item in snapshot_subtests.iter().skip(paired) {
        mismatches.push(CompatibilityMismatch {
            kind: CompatibilityMismatchKind::ExtraItem,
            snapshot_id: Some(item.id.clone()),
            detail: "subtest".to_string(),
        });
    }
    for _legacy in legacy_children.iter().skip(paired) {
        mismatches.push(CompatibilityMismatch {
            kind: CompatibilityMismatchKind::MissingItem,
            snapshot_id: None,
            detail: "subtest".to_string(),
        });
    }
}

#[derive(Debug, Clone)]
struct SubtestCall {
    name: TestItemName,
    range: SourceRange,
    name_range: SourceRange,
    children: Vec<SubtestCall>,
}

#[derive(Debug, Clone)]
struct NamedSubCall {
    name: String,
    range: SourceRange,
    name_range: Option<SourceRange>,
}

fn file_item(
    request: &TestItemDiscoveryRequest<'_>,
    digest: &Digest,
    index: &Utf8LineIndex,
    source_len: u32,
) -> TestItem {
    let mut limitations = Vec::new();
    push_parse_limitations(request.parse_status, &mut limitations);
    if request.framework.is_none() {
        limitations.push("parser_backed_compatibility".to_string());
    }
    if request.file_role != FileRole::Test {
        limitations.push("unrecognized_test_file".to_string());
    }
    assemble_item(
        request,
        digest,
        ItemDraft {
            parent_id: None,
            order: 0,
            kind: TestItemKind::File,
            name: TestItemName::Named(request.display_name.to_string()),
            structural_key: "file".to_string(),
            range: index.source_range(0, source_len),
            name_range: None,
            limitations,
        },
    )
}

fn append_subtest_items(
    request: &TestItemDiscoveryRequest<'_>,
    digest: &Digest,
    file: &TestItem,
    calls: &[SubtestCall],
    items: &mut Vec<TestItem>,
) {
    append_subtest_level(request, digest, &file.id, calls, items);
}

fn append_subtest_level(
    request: &TestItemDiscoveryRequest<'_>,
    digest: &Digest,
    parent_id: &TestItemId,
    calls: &[SubtestCall],
    items: &mut Vec<TestItem>,
) {
    for (order, call) in calls.iter().enumerate() {
        let Ok(order_in_parent) = u32::try_from(order) else {
            continue;
        };
        let mut limitations = Vec::new();
        push_parse_limitations(request.parse_status, &mut limitations);
        if request.framework.is_none() {
            limitations.push("parser_backed_compatibility".to_string());
        }
        if matches!(call.name, TestItemName::Dynamic) {
            limitations.push("dynamic_name".to_string());
        }
        limitations.push("no_isolated_execution".to_string());
        let item = assemble_item(
            request,
            digest,
            ItemDraft {
                parent_id: Some(parent_id.clone()),
                order: order_in_parent,
                kind: TestItemKind::Subtest,
                name: call.name.clone(),
                structural_key: format!("subtest:{order_in_parent}"),
                range: call.range,
                name_range: Some(call.name_range),
                limitations,
            },
        );
        let id = item.id.clone();
        items.push(item);
        append_subtest_level(request, digest, &id, &call.children, items);
    }
}

fn append_named_sub_items(
    request: &TestItemDiscoveryRequest<'_>,
    digest: &Digest,
    file: &TestItem,
    named: &[NamedSubCall],
    items: &mut Vec<TestItem>,
) {
    let existing_file_children =
        items.iter().filter(|item| item.parent_id.as_ref() == Some(&file.id)).count();
    for (offset, call) in named.iter().enumerate() {
        let Some(order_in_parent) =
            existing_file_children.checked_add(offset).and_then(|order| u32::try_from(order).ok())
        else {
            continue;
        };
        let mut limitations = vec!["conservative_named_subroutine".to_string()];
        push_parse_limitations(request.parse_status, &mut limitations);
        if request.framework.is_none() {
            limitations.push("parser_backed_compatibility".to_string());
        }
        items.push(assemble_item(
            request,
            digest,
            ItemDraft {
                parent_id: Some(file.id.clone()),
                order: order_in_parent,
                kind: TestItemKind::NamedSubroutine,
                name: TestItemName::Named(call.name.clone()),
                structural_key: format!("named_sub:{}:{offset}", call.name),
                range: call.range,
                name_range: call.name_range,
                limitations,
            },
        ));
    }
}

struct ItemDraft {
    parent_id: Option<TestItemId>,
    order: u32,
    kind: TestItemKind,
    name: TestItemName,
    structural_key: String,
    range: SourceRange,
    name_range: Option<SourceRange>,
    limitations: Vec<String>,
}

fn assemble_item(
    request: &TestItemDiscoveryRequest<'_>,
    digest: &Digest,
    draft: ItemDraft,
) -> TestItem {
    let id = TestItemId::new(
        request.source_ref,
        draft.parent_id.as_ref(),
        draft.kind,
        draft.structural_key.as_str(),
    );
    TestItem {
        id,
        parent_id: draft.parent_id,
        order_in_parent: draft.order,
        source_ref: request.source_ref.clone(),
        source_digest: digest.clone(),
        generation: request.generation,
        structural_key: draft.structural_key,
        kind: draft.kind,
        name: draft.name,
        name_range: draft.name_range,
        range: draft.range,
        framework: request.framework.cloned(),
        confidence: confidence_for(request.parse_status),
        capabilities: capabilities_for(draft.kind),
        limitations: draft.limitations,
    }
}

fn capabilities_for(kind: TestItemKind) -> TestItemCapabilities {
    match kind {
        TestItemKind::File => TestItemCapabilities {
            runnable: true,
            debuggable: true,
            focusable: false,
            selectively_runnable: false,
        },
        TestItemKind::Subtest | TestItemKind::NamedSubroutine => TestItemCapabilities {
            runnable: false,
            debuggable: false,
            focusable: true,
            selectively_runnable: false,
        },
        TestItemKind::Generated | TestItemKind::Other => TestItemCapabilities::default(),
    }
}

fn confidence_for(parse_status: ParseStatus) -> Confidence {
    match parse_status {
        ParseStatus::Clean => Confidence::High,
        ParseStatus::Recovered => Confidence::Medium,
        ParseStatus::Failed | ParseStatus::NotParsed => Confidence::Low,
    }
}

fn push_parse_limitations(parse_status: ParseStatus, limitations: &mut Vec<String>) {
    match parse_status {
        ParseStatus::Clean => {}
        ParseStatus::Recovered => limitations.push("parser_recovery".to_string()),
        ParseStatus::Failed => limitations.push("parse_failed".to_string()),
        ParseStatus::NotParsed => limitations.push("ast_unavailable".to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubtestNameSet {
    /// Exact call names used by current `perl-lsp-rs-core` subtest discovery.
    LegacyBare,
    /// Canonical producer names, including reviewed Test::More / Test2 qualified forms.
    Canonical,
}

fn collect_subtest_calls(
    node: &Node,
    source: &str,
    index: &Utf8LineIndex,
    source_len: u32,
    names: SubtestNameSet,
    out: &mut Vec<SubtestCall>,
) {
    if let Some(call) = try_as_subtest(node, source, index, source_len, names) {
        out.push(call);
        return;
    }
    for child in structural_children(node) {
        collect_subtest_calls(child, source, index, source_len, names, out);
    }
}

fn try_as_subtest(
    node: &Node,
    source: &str,
    index: &Utf8LineIndex,
    source_len: u32,
    names: SubtestNameSet,
) -> Option<SubtestCall> {
    let NodeKind::FunctionCall { name, args } = &node.kind else {
        return None;
    };
    if !is_subtest_call_name(name, names) {
        return None;
    }
    let first = args.first()?;
    let name = subtest_name_from_arg(first);
    if matches!(&name, TestItemName::Named(value) if value.is_empty()) {
        return None;
    }
    let range = node_range(index, node, source_len)?;
    let name_range = node_range(index, first, source_len)?;
    if name_range.start_byte < range.start_byte || name_range.end_byte > range.end_byte {
        return None;
    }
    let mut children = Vec::new();
    for arg in args.iter().skip(1) {
        if let NodeKind::Subroutine { body, .. } = &arg.kind {
            collect_subtest_calls(body, source, index, source_len, names, &mut children);
        }
    }
    Some(SubtestCall { name, range, name_range, children })
}

fn collect_named_test_subs(
    node: &Node,
    index: &Utf8LineIndex,
    source_len: u32,
    out: &mut Vec<NamedSubCall>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for statement in statements {
                collect_named_test_subs_in_scope(statement, index, source_len, out);
            }
        }
        NodeKind::Package { block: Some(block), .. } => {
            collect_named_test_subs(block, index, source_len, out);
        }
        _ => {}
    }
}

fn collect_named_test_subs_in_scope(
    node: &Node,
    index: &Utf8LineIndex,
    source_len: u32,
    out: &mut Vec<NamedSubCall>,
) {
    match &node.kind {
        NodeKind::Subroutine { name: Some(name), name_span, body: _, .. }
            if is_conservative_test_sub_name(name) =>
        {
            let Some(range) = node_range(index, node, source_len) else {
                return;
            };
            let name_range = name_span
                .and_then(|span| span_range(index, span.start, span.end, source_len))
                .filter(|name_range| {
                    name_range.start_byte >= range.start_byte
                        && name_range.end_byte <= range.end_byte
                });
            out.push(NamedSubCall { name: name.clone(), range, name_range });
        }
        NodeKind::Package { block: Some(block), .. } => {
            collect_named_test_subs(block, index, source_len, out);
        }
        NodeKind::ExpressionStatement { expression } => {
            collect_named_test_subs_in_scope(expression, index, source_len, out);
        }
        _ => {}
    }
}

fn is_subtest_call_name(name: &str, names: SubtestNameSet) -> bool {
    match names {
        SubtestNameSet::LegacyBare => {
            matches!(name, "subtest" | "subtest_buffered" | "subtest_streamed")
        }
        SubtestNameSet::Canonical => {
            let (prefix, bare) = match name.rsplit_once("::") {
                Some((prefix, bare)) => (Some(prefix), bare),
                None => (None, name),
            };
            if !matches!(bare, "subtest" | "subtest_buffered" | "subtest_streamed") {
                return false;
            }
            match prefix {
                None => true,
                Some(prefix) if prefix == "Test::More" || prefix.starts_with("Test2") => true,
                Some(_) => false,
            }
        }
    }
}

fn is_conservative_test_sub_name(name: &str) -> bool {
    if name == "subtest" || !is_perl_identifier(name) {
        return false;
    }
    name.starts_with("test_") || name.ends_with("_test")
}

fn is_perl_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn subtest_name_from_arg(arg: &Node) -> TestItemName {
    match &arg.kind {
        NodeKind::String { value, interpolated } => {
            let unquoted = strip_string_quotes(value);
            if unquoted.is_empty() || (*interpolated && unquoted.contains('$')) {
                TestItemName::Dynamic
            } else {
                TestItemName::Named(unquoted.to_string())
            }
        }
        NodeKind::Identifier { name } if is_perl_identifier(name) && !name.is_empty() => {
            TestItemName::Named(name.clone())
        }
        _ => TestItemName::Dynamic,
    }
}

fn strip_string_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'\'' || first == b'"') && first == last {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn node_range(index: &Utf8LineIndex, node: &Node, source_len: u32) -> Option<SourceRange> {
    span_range(index, node.location.start, node.location.end, source_len)
}

fn span_range(
    index: &Utf8LineIndex,
    start: usize,
    end: usize,
    source_len: u32,
) -> Option<SourceRange> {
    let start = u32::try_from(start).ok()?;
    let end = u32::try_from(end).ok()?;
    if start > end || end > source_len {
        return None;
    }
    Some(index.source_range(start, end))
}

fn structural_children(node: &Node) -> Vec<&Node> {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            statements.iter().collect()
        }
        NodeKind::ExpressionStatement { expression } => vec![expression.as_ref()],
        NodeKind::FunctionCall { args, .. } => args.iter().collect(),
        NodeKind::Subroutine { body, .. } => vec![body.as_ref()],
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            let mut children: Vec<&Node> = vec![condition.as_ref(), then_branch.as_ref()];
            for (cond, branch) in elsif_branches {
                children.push(cond.as_ref());
                children.push(branch.as_ref());
            }
            if let Some(else_branch) = else_branch {
                children.push(else_branch.as_ref());
            }
            children
        }
        NodeKind::While { condition, body, .. } => vec![condition.as_ref(), body.as_ref()],
        NodeKind::For { condition, body, .. } => {
            let mut children = Vec::new();
            if let Some(condition) = condition {
                children.push(condition.as_ref());
            }
            children.push(body.as_ref());
            children
        }
        NodeKind::Foreach { list, body, .. } => vec![list.as_ref(), body.as_ref()],
        NodeKind::StatementModifier { statement, condition, .. } => {
            vec![statement.as_ref(), condition.as_ref()]
        }
        NodeKind::Package { block: Some(block), .. } => vec![block.as_ref()],
        _ => Vec::new(),
    }
}

fn parser_backed_from_call(call: SubtestCall) -> ParserBackedSubtest {
    ParserBackedSubtest {
        name: call.name,
        range: call.range,
        name_range: call.name_range,
        children: call.children.into_iter().map(parser_backed_from_call).collect(),
    }
}
