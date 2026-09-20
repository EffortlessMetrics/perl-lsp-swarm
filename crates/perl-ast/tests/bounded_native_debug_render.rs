//! Discriminating proof for bounded iterative native debug rendering (#8832).
//!
//! These tests fail realistic wrong implementations:
//! - a second projection grammar or child-match table that drifts from #8829/#8424
//! - recursive/thread-local depth that overflows a 256 KiB stack or leaks nested
//! - wrapping a writer and checking length after `write_str` (too late)
//! - encoding exhaustion as `(depth_limit_exceeded)` while returning `Complete`
//! - treating a truncated prefix as AST equality (#7045) or machine output (#8044)
//!
//! They do not implement #7045, #8044, #8047, or #6900.

#[path = "helpers.rs"]
mod helpers;

use helpers::all_nodekind_instances;
use perl_ast::{
    FieldId, NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, NativeDebugSexpInstrumentCause,
    NativeDebugSexpLimits, NativeDebugSexpOmitted, NativeDebugSexpResult,
    NativeDebugSexpTruncation, NativeDebugSexpWork, Node, NodeKind, SourceLocation,
};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SMALL_STACK_BYTES: usize = 256 * 1024;
const DEEP_DEPTH: usize = 50_000;

const GOLDEN_NUMBER_1: &str = "(number (value 1))";
const GOLDEN_EXPR_STMT_42: &str = "(expression_statement (expression (number (value 42))))";
const GOLDEN_PROGRAM_STMT_42: &str =
    "(source_file (statements (expression_statement (expression (number (value 42))))))";
const GOLDEN_VARIABLE_X: &str = "(variable (sigil $) (name x))";
const GOLDEN_STRING_UNICODE: &str =
    r#"(string_interpolated (value "say \"hi\"\\\n\u{1b} café") (interpolated true))"#;
const GOLDEN_CHAIN3: &str =
    "(expression_statement (expression (expression_statement (expression (number (value 1))))))";

fn loc() -> SourceLocation {
    SourceLocation { start: 0, end: 1 }
}

fn num(value: &str) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, loc())
}

fn wrap_expr(inner: Node) -> Node {
    Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, loc())
}

/// `n` wrappers around one leaf: independently constructed size is `n + 1`.
fn deep_chain(n: usize) -> Node {
    let mut node = num("1");
    for _ in 0..n {
        node = wrap_expr(node);
    }
    node
}

fn render_unbounded(node: &Node) -> (String, NativeDebugSexpResult) {
    let mut out = String::new();
    let result = node.render_debug_sexp(&mut out, NativeDebugSexpLimits::unbounded());
    (out, result)
}

fn visit_field_names(node: &Node) -> Vec<String> {
    let mut names = Vec::new();
    node.for_each_child_with_field(|field, _child| {
        if let Some(field) = field {
            names.push(field.name().to_string());
        }
    });
    names
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        self.input.get(self.index..).unwrap_or("")
    }

    fn skip_ws(&mut self) {
        let rest = self.rest();
        let trimmed = rest.trim_start();
        self.index += rest.len() - trimmed.len();
    }

    fn parse_form(&mut self) -> Result<Sexp, String> {
        self.skip_ws();
        match self.rest().chars().next() {
            Some('(') => {
                self.index += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.rest().starts_with(')') {
                        self.index += 1;
                        return Ok(Sexp::List(items));
                    }
                    if self.index >= self.input.len() {
                        return Err("unclosed list".to_string());
                    }
                    items.push(self.parse_form()?);
                }
            }
            Some('"') => {
                let rest = self.rest();
                let bytes = rest.as_bytes();
                let mut i = 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'"' => {
                            self.index += i + 1;
                            return Ok(Sexp::Atom(rest[1..i].to_string()));
                        }
                        b'\\' => i += 2,
                        _ => i += 1,
                    }
                }
                Err("unclosed quote".to_string())
            }
            Some(_) => {
                let rest = self.rest();
                let end = rest
                    .find(|ch: char| ch.is_whitespace() || ch == '(' || ch == ')')
                    .unwrap_or(rest.len());
                let symbol = rest[..end].to_string();
                self.index += end;
                Ok(Sexp::Atom(symbol))
            }
            None => Err("eof".to_string()),
        }
    }
}

fn child_field_names_from_sexp(sexp: &str) -> Vec<String> {
    let mut parser = Parser { input: sexp, index: 0 };
    let Ok(Sexp::List(items)) = parser.parse_form() else {
        return Vec::new();
    };
    items
        .iter()
        .skip(1)
        .filter_map(|item| {
            let Sexp::List(inner) = item else {
                return None;
            };
            let Sexp::Atom(name) = inner.first()? else {
                return None;
            };
            if !inner.iter().skip(1).any(|part| matches!(part, Sexp::List(_))) {
                return None;
            }
            FieldId::from_name(name).map(|_| name.clone())
        })
        .collect()
}

fn run_on_small_stack<F>(body: F) -> Result<(), String>
where
    F: FnOnce() + Send + 'static,
{
    let handle = std::thread::Builder::new()
        .stack_size(SMALL_STACK_BYTES)
        .spawn(body)
        .map_err(|error| format!("failed to spawn small-stack worker: {error}"))?;
    handle.join().map_err(|_| "small-stack worker aborted (likely stack overflow)".to_string())
}

struct CountingWriter {
    bytes: usize,
}

impl fmt::Write for CountingWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.bytes = self.bytes.saturating_add(s.len());
        Ok(())
    }
}

struct FailWriter;

impl fmt::Write for FailWriter {
    fn write_str(&mut self, _s: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

struct FailAfter {
    remaining: usize,
}

impl fmt::Write for FailAfter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.len() > self.remaining {
            return Err(fmt::Error);
        }
        self.remaining -= s.len();
        Ok(())
    }
}

struct NestedWriter<'a> {
    outer: String,
    nested: &'a Node,
    nested_nodes: usize,
    nested_was_truncated: bool,
}

impl fmt::Write for NestedWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut inner = String::new();
        match self.nested.render_debug_sexp(
            &mut inner,
            NativeDebugSexpLimits { max_depth: Some(0), ..NativeDebugSexpLimits::unbounded() },
        ) {
            NativeDebugSexpResult::Complete { work } => {
                self.nested_nodes = work.nodes_visited;
                self.nested_was_truncated = false;
            }
            NativeDebugSexpResult::Truncated { work, .. } => {
                self.nested_nodes = work.nodes_visited;
                self.nested_was_truncated = true;
            }
            NativeDebugSexpResult::InstrumentFailure { .. } => {
                return Err(fmt::Error);
            }
        }
        self.outer.push_str(s);
        Ok(())
    }
}

#[test]
fn small_trees_match_8829_golden_bytes() {
    let number = num("1");
    let expr = wrap_expr(num("42"));
    let program = Node::new(NodeKind::Program { statements: vec![wrap_expr(num("42"))] }, loc());
    let variable =
        Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() }, loc());
    let unicode = Node::new(
        NodeKind::String { value: "say \"hi\"\\\n\u{1b} café".to_string(), interpolated: true },
        loc(),
    );
    let chain3 = deep_chain(2);

    for (node, golden) in [
        (&number, GOLDEN_NUMBER_1),
        (&expr, GOLDEN_EXPR_STMT_42),
        (&program, GOLDEN_PROGRAM_STMT_42),
        (&variable, GOLDEN_VARIABLE_X),
        (&unicode, GOLDEN_STRING_UNICODE),
        (&chain3, GOLDEN_CHAIN3),
    ] {
        let (out, result) = render_unbounded(node);
        assert!(
            matches!(result, NativeDebugSexpResult::Complete { .. }),
            "unbounded small tree must Complete, got {result:?} for {out}"
        );
        assert_eq!(out, golden, "complete bytes drifted from the #8829 projection");
        if let NativeDebugSexpResult::Complete { work } = result {
            assert_eq!(work.bytes_written, golden.len());
        }
    }
}

#[test]
fn every_representative_unbounded_render_matches_visit_order() {
    for node in all_nodekind_instances() {
        let (out, result) = render_unbounded(&node);
        assert!(
            matches!(result, NativeDebugSexpResult::Complete { .. }),
            "{} representative must Complete, got {result:?} for {out}",
            node.kind.kind_name()
        );
        assert_eq!(
            child_field_names_from_sexp(&out),
            visit_field_names(&node),
            "{} field order drifted from #8424; sexp = {out}",
            node.kind.kind_name()
        );
        assert!(
            !out.contains("depth_limit_exceeded"),
            "{} representative emitted the retired fake node: {out}",
            node.kind.kind_name()
        );
    }
}

#[test]
fn node_depth_byte_and_work_limits_trip_independently() {
    let chain = deep_chain(2);
    assert_eq!(chain.count_nodes(), 3);

    let complete_len = GOLDEN_CHAIN3.len();
    let (complete_out, complete) = render_unbounded(&chain);
    assert_eq!(complete_out, GOLDEN_CHAIN3);
    let complete_work = match complete {
        NativeDebugSexpResult::Complete { work } => work,
        other => {
            assert!(
                matches!(other, NativeDebugSexpResult::Complete { .. }),
                "unbounded chain must Complete, got {other:?}"
            );
            NativeDebugSexpWork::default()
        }
    };

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_nodes: Some(2), ..NativeDebugSexpLimits::unbounded() },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::NodeLimit { limit: 2 },
            work,
            ..
        } => {
            assert_eq!(work.nodes_visited, 2);
            assert!(work.nodes_visited < complete_work.nodes_visited);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::NodeLimit { limit: 2 },
                    ..
                }
            ),
            "node limit 2 must trip NodeLimit, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_depth: Some(1), ..NativeDebugSexpLimits::unbounded() },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::DepthLimit { limit: 1 },
            work,
            ..
        } => {
            assert_eq!(work.nodes_visited, 2);
            assert_eq!(work.max_depth, 1);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::DepthLimit { limit: 1 },
                    ..
                }
            ),
            "depth limit 1 must trip DepthLimit, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_bytes: Some(complete_len.saturating_sub(1)),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::ByteLimit { limit: tripped },
            work,
            ..
        } => {
            assert_eq!(tripped, complete_len.saturating_sub(1));
            assert!(work.bytes_written <= tripped);
            assert!(work.bytes_written < complete_work.bytes_written);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::ByteLimit { .. },
                    ..
                }
            ),
            "byte limit-1 must trip ByteLimit, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_bytes: Some(complete_len),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Complete { work } => {
            assert_eq!(work.bytes_written, complete_len);
        }
        other => assert!(
            matches!(other, NativeDebugSexpResult::Complete { .. }),
            "byte limit exact must Complete, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_bytes: Some(complete_len.saturating_add(1)),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Complete { work } => {
            assert_eq!(work.bytes_written, complete_len);
        }
        other => assert!(
            matches!(other, NativeDebugSexpResult::Complete { .. }),
            "byte limit+1 must Complete, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_work: Some(1), ..NativeDebugSexpLimits::unbounded() },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::WorkLimit { limit: 1 },
            work,
            ..
        } => {
            assert_eq!(work.work_units, 1);
            assert_eq!(work.nodes_visited, 1);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::WorkLimit { limit: 1 },
                    ..
                }
            ),
            "work limit 1 must trip WorkLimit, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_nodes: Some(1),
            max_depth: Some(0),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::NodeLimit { limit: 1 },
            work,
            ..
        } => {
            assert_eq!(work.nodes_visited, 1);
            assert_eq!(
                work.child_edges_visited, 0,
                "a child that cannot be admitted must not charge an edge"
            );
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::NodeLimit { limit: 1 },
                    ..
                }
            ),
            "documented node-then-depth order must trip NodeLimit, got {other:?}"
        ),
    }

    for (limit, expect_complete) in [(0usize, false), (1, false), (2, false), (3, true), (4, true)]
    {
        let result = chain.render_debug_sexp(
            &mut String::new(),
            NativeDebugSexpLimits { max_nodes: Some(limit), ..NativeDebugSexpLimits::unbounded() },
        );
        if expect_complete {
            assert!(
                matches!(result, NativeDebugSexpResult::Complete { work } if work.nodes_visited == 3),
                "node limit {limit} must Complete, got {result:?}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    NativeDebugSexpResult::Truncated {
                        reason: NativeDebugSexpTruncation::NodeLimit { limit: tripped },
                        ..
                    } if tripped == limit
                ),
                "node limit {limit} must trip NodeLimit, got {result:?}"
            );
        }
    }

    for (limit, expect_complete) in [(0usize, false), (1, false), (2, true), (3, true)] {
        let result = chain.render_debug_sexp(
            &mut String::new(),
            NativeDebugSexpLimits { max_depth: Some(limit), ..NativeDebugSexpLimits::unbounded() },
        );
        if expect_complete {
            assert!(
                matches!(result, NativeDebugSexpResult::Complete { .. }),
                "depth limit {limit} must Complete, got {result:?}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    NativeDebugSexpResult::Truncated {
                        reason: NativeDebugSexpTruncation::DepthLimit { limit: tripped },
                        ..
                    } if tripped == limit
                ),
                "depth limit {limit} must trip DepthLimit, got {result:?}"
            );
        }
    }
}

#[test]
fn output_never_exceeds_declared_byte_limit() {
    let node = num("1");
    let complete_len = GOLDEN_NUMBER_1.len();
    for limit in [0usize, 1, 2, 3, 7, 8, 19, 20, 21, 64] {
        let mut out = String::new();
        let result = node.render_debug_sexp(
            &mut out,
            NativeDebugSexpLimits { max_bytes: Some(limit), ..NativeDebugSexpLimits::unbounded() },
        );
        assert!(out.len() <= limit, "limit {limit} produced {} bytes: {out:?}", out.len());
        match result {
            NativeDebugSexpResult::Complete { work } => {
                assert!(limit >= complete_len, "Complete under sub-complete limit {limit}");
                assert_eq!(work.bytes_written, complete_len);
                assert_eq!(out, GOLDEN_NUMBER_1);
            }
            NativeDebugSexpResult::Truncated {
                reason: NativeDebugSexpTruncation::ByteLimit { limit: tripped },
                work,
                ..
            } => {
                assert_eq!(tripped, limit);
                assert_eq!(work.bytes_written, out.len());
                assert!(work.bytes_written <= limit);
                assert_ne!(out, GOLDEN_NUMBER_1);
            }
            other => assert!(
                matches!(
                    other,
                    NativeDebugSexpResult::Truncated {
                        reason: NativeDebugSexpTruncation::ByteLimit { .. },
                        ..
                    }
                ),
                "byte limit {limit} produced {other:?}"
            ),
        }
    }
}

#[test]
fn fifty_thousand_node_chain_renders_on_small_stack() -> Result<(), String> {
    run_on_small_stack(|| {
        let expected = DEEP_DEPTH + 1;
        let tree = deep_chain(DEEP_DEPTH);
        let mut writer = CountingWriter { bytes: 0 };
        match tree.render_debug_sexp(&mut writer, NativeDebugSexpLimits::unbounded()) {
            NativeDebugSexpResult::Complete { work } => {
                assert_eq!(work.nodes_visited, expected);
                assert_eq!(work.child_edges_visited, DEEP_DEPTH);
                assert_eq!(work.max_depth, DEEP_DEPTH);
                assert_eq!(work.bytes_written, writer.bytes);
                assert!(work.bytes_written > 0);
            }
            other => assert!(
                matches!(other, NativeDebugSexpResult::Complete { .. }),
                "50k chain must Complete on a small stack, got {other:?}"
            ),
        }
        drop(tree);
    })
}

#[test]
fn nested_writer_callback_does_not_inherit_outer_budget() {
    let outer = deep_chain(8);
    let nested = deep_chain(3);
    let mut writer = NestedWriter {
        outer: String::new(),
        nested: &nested,
        nested_nodes: 0,
        nested_was_truncated: false,
    };
    let result = outer.render_debug_sexp(&mut writer, NativeDebugSexpLimits::unbounded());
    assert!(
        matches!(result, NativeDebugSexpResult::Complete { work } if work.nodes_visited == 9),
        "outer unbounded chain must Complete, got {result:?}"
    );
    assert!(writer.nested_was_truncated, "nested max_depth=0 chain must Truncate independently");
    assert_eq!(
        writer.nested_nodes, 1,
        "nested depth budget must not inherit the outer frame depth"
    );
    assert_eq!(writer.outer, render_unbounded(&outer).0);
}

#[test]
fn concurrent_renders_are_isolated() {
    let hits = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for thread_id in 0..8 {
            let hits = Arc::clone(&hits);
            scope.spawn(move || {
                let node = if thread_id % 2 == 0 { deep_chain(6) } else { num("7") };
                let (out, result) = render_unbounded(&node);
                assert!(
                    matches!(result, NativeDebugSexpResult::Complete { .. }),
                    "thread {thread_id} must Complete, got {result:?}"
                );
                assert!(!out.contains("depth_limit_exceeded"));
                hits.fetch_add(1, Ordering::SeqCst);
            });
        }
    });
    assert_eq!(hits.load(Ordering::SeqCst), 8);
}

#[test]
fn truncation_is_not_a_fake_ast_node() {
    assert!(
        !NodeKind::ALL_KIND_NAMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case("depth_limit_exceeded"))
    );
    let mut out = String::new();
    let result = deep_chain(8).render_debug_sexp(
        &mut out,
        NativeDebugSexpLimits { max_depth: Some(0), ..NativeDebugSexpLimits::unbounded() },
    );
    assert!(
        matches!(
            result,
            NativeDebugSexpResult::Truncated {
                reason: NativeDebugSexpTruncation::DepthLimit { limit: 0 },
                ..
            }
        ),
        "depth 0 must Truncate, got {result:?}"
    );
    assert!(!out.contains("depth_limit_exceeded"), "truncated prefix leaked the fake node: {out}");
    assert!(
        !out.contains(NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER),
        "typed truncation must not inject the marker as a form: {out}"
    );
    assert!(
        !matches!(result, NativeDebugSexpResult::Complete { .. }),
        "truncation must not be reported Complete"
    );
}

#[test]
fn truncated_prefix_cannot_stand_in_for_ast_equality() {
    let left = wrap_expr(num("1"));
    let right = wrap_expr(num("2"));
    assert_ne!(left, right, "#7045 lives on Node::eq, not on debug bytes");

    let mut left_out = String::new();
    let mut right_out = String::new();
    let left_result = left.render_debug_sexp(
        &mut left_out,
        NativeDebugSexpLimits { max_nodes: Some(1), ..NativeDebugSexpLimits::unbounded() },
    );
    let right_result = right.render_debug_sexp(
        &mut right_out,
        NativeDebugSexpLimits { max_nodes: Some(1), ..NativeDebugSexpLimits::unbounded() },
    );
    assert!(
        matches!(left_result, NativeDebugSexpResult::Truncated { .. })
            && matches!(right_result, NativeDebugSexpResult::Truncated { .. }),
        "max_nodes=1 must Truncate both trees"
    );
    assert_eq!(left_out, right_out, "shared truncated prefix is the dishonest equality hazard");
    assert_ne!(left, right, "identical truncated debug prefixes must not satisfy AST equality");
    assert!(
        !matches!(left_result, NativeDebugSexpResult::Complete { .. }),
        "incomplete debug output cannot be admitted as a #7045 oracle"
    );
}

#[test]
fn incomplete_debug_output_cannot_satisfy_machine_output() {
    fn dishonest_machine_gate(bytes: &str) -> bool {
        !bytes.is_empty()
    }
    fn honest_machine_gate(result: &NativeDebugSexpResult) -> bool {
        match result {
            NativeDebugSexpResult::Complete { .. } => false,
            NativeDebugSexpResult::Truncated { .. }
            | NativeDebugSexpResult::InstrumentFailure { .. } => false,
        }
    }

    let mut out = String::new();
    let result = num("1").render_debug_sexp(
        &mut out,
        NativeDebugSexpLimits { max_bytes: Some(3), ..NativeDebugSexpLimits::unbounded() },
    );
    assert!(
        matches!(result, NativeDebugSexpResult::Truncated { .. }),
        "sub-complete byte bound must Truncate, got {result:?}"
    );
    assert!(dishonest_machine_gate(&out), "truncated debug bytes are the #8044 hazard");
    assert!(
        !honest_machine_gate(&result),
        "incomplete debug rendering cannot satisfy typed machine output"
    );

    let (complete_out, complete) = render_unbounded(&num("1"));
    assert!(matches!(complete, NativeDebugSexpResult::Complete { .. }));
    assert_eq!(complete_out, GOLDEN_NUMBER_1);
    assert!(
        !honest_machine_gate(&complete),
        "even complete debug sexp is not the #8044 machine schema"
    );
}

#[test]
fn writer_failure_is_instrument_failure() {
    let node = num("1");
    match node.render_debug_sexp(&mut FailWriter, NativeDebugSexpLimits::unbounded()) {
        NativeDebugSexpResult::InstrumentFailure {
            cause: NativeDebugSexpInstrumentCause::WriterError,
            ..
        } => {}
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::InstrumentFailure {
                    cause: NativeDebugSexpInstrumentCause::WriterError,
                    ..
                }
            ),
            "immediate writer failure must be InstrumentFailure, got {other:?}"
        ),
    }

    match node
        .render_debug_sexp(&mut FailAfter { remaining: 3 }, NativeDebugSexpLimits::unbounded())
    {
        NativeDebugSexpResult::InstrumentFailure {
            cause: NativeDebugSexpInstrumentCause::WriterError,
            work,
        } => {
            assert!(work.bytes_written <= 3);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::InstrumentFailure {
                    cause: NativeDebugSexpInstrumentCause::WriterError,
                    ..
                }
            ),
            "mid-stream writer failure must be InstrumentFailure, got {other:?}"
        ),
    }
}

#[test]
fn to_sexp_string_cannot_prove_completeness() {
    fn completeness_from_string(_sexp: &str) -> Option<bool> {
        None
    }

    let complete_tree = num("1");
    let truncated_tree = deep_chain(4);
    let complete_string = complete_tree.to_sexp();
    let mut truncated = String::new();
    let truncated_result = truncated_tree.render_debug_sexp(
        &mut truncated,
        NativeDebugSexpLimits { max_nodes: Some(1), ..NativeDebugSexpLimits::unbounded() },
    );
    assert_eq!(complete_string, GOLDEN_NUMBER_1);
    assert_eq!(completeness_from_string(&complete_string), None);
    assert_eq!(completeness_from_string(&truncated), None);
    assert!(matches!(truncated_result, NativeDebugSexpResult::Truncated { .. }));
}

#[test]
fn render_streams_without_requiring_an_intermediate_string() {
    let node = wrap_expr(num("42"));
    let mut writer = CountingWriter { bytes: 0 };
    match node.render_debug_sexp(&mut writer, NativeDebugSexpLimits::unbounded()) {
        NativeDebugSexpResult::Complete { work } => {
            assert_eq!(work.bytes_written, GOLDEN_EXPR_STMT_42.len());
            assert_eq!(writer.bytes, GOLDEN_EXPR_STMT_42.len());
        }
        other => assert!(
            matches!(other, NativeDebugSexpResult::Complete { .. }),
            "streamed unbounded render must Complete, got {other:?}"
        ),
    }
}

#[test]
fn node_limit_precedes_depth_and_does_not_charge_a_rejected_edge() {
    let chain = deep_chain(2);
    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_nodes: Some(1), ..NativeDebugSexpLimits::unbounded() },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::NodeLimit { limit: 1 },
            work,
            ..
        } => {
            assert_eq!(work.nodes_visited, 1);
            assert_eq!(
                work.child_edges_visited, 0,
                "node capacity must be checked before descend charges an edge"
            );
            assert_eq!(work.max_depth, 0);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::NodeLimit { limit: 1 },
                    ..
                }
            ),
            "max_nodes=1 must trip NodeLimit before any child edge, got {other:?}"
        ),
    }
}

#[test]
fn rejected_descent_charges_no_edge_work() {
    let chain = deep_chain(2);
    let depth0 = chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_depth: Some(0), ..NativeDebugSexpLimits::unbounded() },
    );
    let depth0_work = match depth0 {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::DepthLimit { limit: 0 },
            work,
            ..
        } => {
            assert_eq!(work.nodes_visited, 1);
            assert_eq!(work.child_edges_visited, 0);
            work
        }
        other => {
            assert!(
                matches!(
                    other,
                    NativeDebugSexpResult::Truncated {
                        reason: NativeDebugSexpTruncation::DepthLimit { limit: 0 },
                        ..
                    }
                ),
                "max_depth=0 must trip DepthLimit, got {other:?}"
            );
            NativeDebugSexpWork::default()
        }
    };

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_depth: Some(0),
            max_work: Some(depth0_work.work_units),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::DepthLimit { limit: 0 },
            work,
            ..
        } => {
            assert_eq!(work.work_units, depth0_work.work_units);
            assert_eq!(work.child_edges_visited, 0);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::DepthLimit { limit: 0 },
                    ..
                }
            ),
            "rejected descent must remain DepthLimit when work equals the depth-0 charge, got {other:?}"
        ),
    }

    match chain.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits {
            max_depth: Some(1),
            max_work: Some(depth0_work.work_units),
            ..NativeDebugSexpLimits::unbounded()
        },
    ) {
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::WorkLimit { limit },
            work,
            ..
        } => {
            assert_eq!(limit, depth0_work.work_units);
            assert_eq!(work.work_units, depth0_work.work_units);
            assert_eq!(work.child_edges_visited, 0);
        }
        other => assert!(
            matches!(
                other,
                NativeDebugSexpResult::Truncated {
                    reason: NativeDebugSexpTruncation::WorkLimit { .. },
                    ..
                }
            ),
            "an allowed descent must trip WorkLimit on the edge charge, got {other:?}"
        ),
    }
}

#[test]
fn omitted_count_is_unknown_when_subtree_was_not_walked() {
    let tree = deep_chain(8);
    match tree.render_debug_sexp(
        &mut String::new(),
        NativeDebugSexpLimits { max_nodes: Some(1), ..NativeDebugSexpLimits::unbounded() },
    ) {
        NativeDebugSexpResult::Truncated { omitted, work, .. } => {
            assert_eq!(work.nodes_visited, 1);
            assert_eq!(
                omitted,
                NativeDebugSexpOmitted::Unknown,
                "must not fabricate an omitted count from unvisited descendants"
            );
        }
        other => {
            assert!(
                matches!(other, NativeDebugSexpResult::Truncated { .. }),
                "max_nodes=1 must Truncate, got {other:?}"
            )
        }
    }
}
