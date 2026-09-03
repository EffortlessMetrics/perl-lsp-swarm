//! Bounded non-recursive [`Node`] [`Debug`].
//!
//! Human diagnostics get kind, range, a selected payload summary, and a bounded
//! child projection. Truncation is visible. The walk uses an explicit heap
//! stack; it does not follow derived recursive `Debug` glue through
//! [`NodeKind`]. Rust [`Debug`] is not machine identity, equality, or a
//! durable metric oracle.

use super::{FieldId, Node, NodeKind};
use std::fmt::{self, Write as _};

/// Maximum child-expansion depth for [`Node`] [`Debug`] (root is depth 0).
pub const NODE_DEBUG_MAX_DEPTH: usize = 3;
/// Maximum direct children rendered at one node.
pub const NODE_DEBUG_MAX_CHILDREN: usize = 6;
/// Maximum nodes admitted into one [`Debug`] sketch.
pub const NODE_DEBUG_MAX_NODES: usize = 32;
/// Maximum characters kept from one payload string.
pub const NODE_DEBUG_MAX_PAYLOAD_CHARS: usize = 24;
/// Maximum bytes of a complete [`Node`] [`Debug`] rendering, including the
/// truncation marker when present.
pub const NODE_DEBUG_MAX_BYTES: usize = 2048;
/// Visible marker appended when any debug budget is exhausted.
pub const NODE_DEBUG_TRUNCATION_MARKER: &str = "#truncated";

const MARKER_SUFFIX: &str = " #truncated";

/// Operation-local debug work recorded by [`sketch_node`].
pub(super) trait DebugObserver {
    /// Called once per sketched node.
    fn on_enter(&mut self);
    /// Called whenever the explicit work-stack length is observed.
    fn on_stack_depth(&mut self, depth: usize);
}

impl DebugObserver for () {
    fn on_enter(&mut self) {}
    fn on_stack_depth(&mut self, _depth: usize) {}
}

struct PayloadSummary {
    text: Option<String>,
    truncated: bool,
}

struct SketchedChild {
    field: Option<FieldId>,
    sketch: Sketch,
}

struct Sketch {
    kind: &'static str,
    start: usize,
    end: usize,
    payload: Option<String>,
    children: Vec<SketchedChild>,
    omitted_children: usize,
    truncated: bool,
}

struct Frame<'a> {
    node: &'a Node,
    field: Option<FieldId>,
    depth: usize,
    children: Vec<(Option<FieldId>, &'a Node)>,
    next_child: usize,
    built_children: Vec<SketchedChild>,
    omitted_children: usize,
    truncated: bool,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render_node(self, &mut ()))
    }
}

impl fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind_name())?;
        let payload = payload_summary(self);
        if let Some(text) = payload.text.as_deref() {
            f.write_str(" payload={")?;
            f.write_str(text)?;
            f.write_str("}")?;
        }
        if payload.truncated {
            f.write_str(MARKER_SUFFIX)?;
        }
        Ok(())
    }
}

/// Render `node` under the documented debug budgets.
pub(super) fn render_node<O: DebugObserver>(node: &Node, observer: &mut O) -> String {
    let sketch = sketch_node(node, observer);
    let mut out = String::new();
    let mut writer = BoundedWriter { out: &mut out, truncated: false };
    let _ = write_sketch(&mut writer, &sketch);
    if writer.truncated || sketch.truncated {
        out.push_str(MARKER_SUFFIX);
        if out.len() > NODE_DEBUG_MAX_BYTES {
            let keep = NODE_DEBUG_MAX_BYTES.saturating_sub(MARKER_SUFFIX.len());
            let keep = floor_char_boundary(&out, keep);
            out.truncate(keep);
            out.push_str(MARKER_SUFFIX);
        }
    }
    out
}

fn sketch_node<O: DebugObserver>(root: &Node, observer: &mut O) -> Sketch {
    let mut stack = vec![open_frame(root, None, 0)];
    observer.on_stack_depth(stack.len());
    // Root is already on the stack; further pushes must stay inside the node budget.
    let mut admitted = 1usize;

    loop {
        let child_job = stack.last().and_then(|frame| {
            frame
                .children
                .get(frame.next_child)
                .copied()
                .map(|(field, node)| (field, node, frame.depth.saturating_add(1)))
        });
        if let Some((field, node, depth)) = child_job {
            if admitted >= NODE_DEBUG_MAX_NODES {
                if let Some(frame) = stack.last_mut() {
                    let remaining = frame.children.len().saturating_sub(frame.next_child);
                    frame.omitted_children = frame.omitted_children.saturating_add(remaining);
                    frame.truncated = true;
                    frame.children.clear();
                    frame.next_child = 0;
                }
                continue;
            }
            if let Some(frame) = stack.last_mut() {
                frame.next_child = frame.next_child.saturating_add(1);
            }
            stack.push(open_frame(node, field, depth));
            admitted = admitted.saturating_add(1);
            observer.on_stack_depth(stack.len());
            continue;
        }

        let Some(frame) = stack.pop() else {
            break;
        };
        observer.on_enter();
        observer.on_stack_depth(stack.len());
        let payload = payload_summary(&frame.node.kind);
        let truncated = frame.truncated || payload.truncated;
        let sketch = Sketch {
            kind: frame.node.kind.kind_name(),
            start: frame.node.location.start(),
            end: frame.node.location.end(),
            payload: payload.text,
            children: frame.built_children,
            omitted_children: frame.omitted_children,
            truncated,
        };
        if let Some(parent) = stack.last_mut() {
            parent.truncated |= sketch.truncated;
            parent.built_children.push(SketchedChild { field: frame.field, sketch });
        } else {
            return sketch;
        }
    }

    Sketch {
        kind: root.kind.kind_name(),
        start: root.location.start(),
        end: root.location.end(),
        payload: payload_summary(&root.kind).text,
        children: Vec::new(),
        omitted_children: 0,
        truncated: true,
    }
}

fn open_frame(node: &Node, field: Option<FieldId>, depth: usize) -> Frame<'_> {
    let mut children = Vec::new();
    let mut total = 0usize;
    node.for_each_child_with_field(|child_field, child| {
        total = total.saturating_add(1);
        if depth < NODE_DEBUG_MAX_DEPTH && children.len() < NODE_DEBUG_MAX_CHILDREN {
            children.push((child_field, child));
        }
    });
    let mut truncated = false;
    let mut omitted_children = 0usize;
    if depth >= NODE_DEBUG_MAX_DEPTH {
        if total > 0 {
            omitted_children = total;
            truncated = true;
        }
        children.clear();
    } else if total > NODE_DEBUG_MAX_CHILDREN {
        omitted_children = total.saturating_sub(NODE_DEBUG_MAX_CHILDREN);
        truncated = true;
    }
    Frame {
        node,
        field,
        depth,
        children,
        next_child: 0,
        built_children: Vec::new(),
        omitted_children,
        truncated,
    }
}

fn write_sketch(out: &mut BoundedWriter<'_>, sketch: &Sketch) -> fmt::Result {
    write!(out, "Node({} @{}..{}", sketch.kind, sketch.start, sketch.end)?;
    if let Some(payload) = sketch.payload.as_deref() {
        write!(out, " payload={{{payload}}}")?;
    }
    if !sketch.children.is_empty() || sketch.omitted_children > 0 {
        out.write_str(" children=[")?;
        for (index, child) in sketch.children.iter().enumerate() {
            if index > 0 {
                out.write_str(", ")?;
            }
            if let Some(field) = child.field {
                write!(out, "{}:", field.name())?;
            }
            write_sketch(out, &child.sketch)?;
        }
        if sketch.omitted_children > 0 {
            if !sketch.children.is_empty() {
                out.write_str(", ")?;
            }
            write!(out, "... +{}", sketch.omitted_children)?;
        }
        out.write_str("]")?;
    }
    out.write_str(")")
}

struct BoundedWriter<'a> {
    out: &'a mut String,
    truncated: bool,
}

impl fmt::Write for BoundedWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.truncated {
            return Ok(());
        }
        let usable = NODE_DEBUG_MAX_BYTES.saturating_sub(MARKER_SUFFIX.len());
        let remaining = usable.saturating_sub(self.out.len());
        if remaining == 0 {
            self.truncated = true;
            return Ok(());
        }
        if s.len() <= remaining {
            self.out.push_str(s);
            return Ok(());
        }
        let take = floor_char_boundary(s, remaining);
        self.out.push_str(&s[..take]);
        self.truncated = true;
        Ok(())
    }
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut idx = index;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn payload_summary(kind: &NodeKind) -> PayloadSummary {
    let mut parts = Vec::new();
    let mut truncated = false;
    match kind {
        NodeKind::Variable { sigil, name } => {
            push_str(&mut parts, &mut truncated, "sigil", sigil);
            push_str(&mut parts, &mut truncated, "name", name);
        }
        NodeKind::VariableDeclaration { declarator, .. }
        | NodeKind::VariableListDeclaration { declarator, .. } => {
            push_str(&mut parts, &mut truncated, "declarator", declarator);
        }
        NodeKind::VariableWithAttributes { attributes, .. } => {
            push_list(&mut parts, &mut truncated, "attributes", attributes);
        }
        NodeKind::Assignment { op, .. }
        | NodeKind::Binary { op, .. }
        | NodeKind::Unary { op, .. } => {
            push_str(&mut parts, &mut truncated, "op", op);
        }
        NodeKind::Number { value } | NodeKind::VString { value } => {
            push_str(&mut parts, &mut truncated, "value", value);
        }
        NodeKind::String { value, interpolated } => {
            push_str(&mut parts, &mut truncated, "value", value);
            push_bool(&mut parts, "interpolated", *interpolated);
        }
        NodeKind::Heredoc { delimiter, content, interpolated, indented, command, .. } => {
            push_str(&mut parts, &mut truncated, "delimiter", delimiter);
            push_str(&mut parts, &mut truncated, "content", content);
            push_bool(&mut parts, "interpolated", *interpolated);
            push_bool(&mut parts, "indented", *indented);
            push_bool(&mut parts, "command", *command);
        }
        NodeKind::Readline { filehandle } => {
            if let Some(filehandle) = filehandle {
                push_str(&mut parts, &mut truncated, "filehandle", filehandle);
            }
        }
        NodeKind::Glob { pattern } => push_str(&mut parts, &mut truncated, "pattern", pattern),
        NodeKind::Typeglob { name } => push_str(&mut parts, &mut truncated, "name", name),
        NodeKind::If { keyword, .. } | NodeKind::While { keyword, .. } => {
            if let Some(keyword) = keyword {
                push_str(&mut parts, &mut truncated, "keyword", keyword);
            }
        }
        NodeKind::LabeledStatement { label, .. } => {
            push_str(&mut parts, &mut truncated, "label", label);
        }
        NodeKind::StatementModifier { modifier, .. } => {
            push_str(&mut parts, &mut truncated, "modifier", modifier);
        }
        NodeKind::Subroutine { name, declarator, attributes, .. } => {
            if let Some(name) = name {
                push_str(&mut parts, &mut truncated, "name", name);
            }
            if let Some(declarator) = declarator {
                push_str(&mut parts, &mut truncated, "declarator", declarator);
            }
            push_list(&mut parts, &mut truncated, "attributes", attributes);
        }
        NodeKind::NamedParameter { external_name, default_operator, required, .. } => {
            push_str(&mut parts, &mut truncated, "external_name", external_name);
            if let Some(op) = default_operator {
                push_str(&mut parts, &mut truncated, "default_operator", op);
            }
            push_bool(&mut parts, "required", *required);
        }
        NodeKind::Method { name, attributes, .. } => {
            push_str(&mut parts, &mut truncated, "name", name);
            push_list(&mut parts, &mut truncated, "attributes", attributes);
        }
        NodeKind::LoopControl { op, label } => {
            push_str(&mut parts, &mut truncated, "op", op);
            if let Some(label) = label {
                push_str(&mut parts, &mut truncated, "label", label);
            }
        }
        NodeKind::Goto { form, .. } => parts.push(format!("form={form:?}")),
        NodeKind::MethodCall { method, .. } | NodeKind::IndirectCall { method, .. } => {
            push_str(&mut parts, &mut truncated, "method", method);
        }
        NodeKind::FunctionCall { name, .. } | NodeKind::AmperCall { name, .. } => {
            push_str(&mut parts, &mut truncated, "name", name);
        }
        NodeKind::Regex { pattern, replacement, modifiers, has_embedded_code } => {
            push_str(&mut parts, &mut truncated, "pattern", pattern);
            if let Some(replacement) = replacement {
                push_str(&mut parts, &mut truncated, "replacement", replacement);
            }
            push_str(&mut parts, &mut truncated, "modifiers", modifiers);
            push_bool(&mut parts, "has_embedded_code", *has_embedded_code);
        }
        NodeKind::Match { pattern, modifiers, has_embedded_code, negated, .. } => {
            push_str(&mut parts, &mut truncated, "pattern", pattern);
            push_str(&mut parts, &mut truncated, "modifiers", modifiers);
            push_bool(&mut parts, "has_embedded_code", *has_embedded_code);
            push_bool(&mut parts, "negated", *negated);
        }
        NodeKind::Substitution {
            pattern,
            replacement,
            modifiers,
            has_embedded_code,
            negated,
            ..
        } => {
            push_str(&mut parts, &mut truncated, "pattern", pattern);
            push_str(&mut parts, &mut truncated, "replacement", replacement);
            push_str(&mut parts, &mut truncated, "modifiers", modifiers);
            push_bool(&mut parts, "has_embedded_code", *has_embedded_code);
            push_bool(&mut parts, "negated", *negated);
        }
        NodeKind::Transliteration { search, replace, modifiers, negated, .. } => {
            push_str(&mut parts, &mut truncated, "search", search);
            push_str(&mut parts, &mut truncated, "replace", replace);
            push_str(&mut parts, &mut truncated, "modifiers", modifiers);
            push_bool(&mut parts, "negated", *negated);
        }
        NodeKind::Package { name, .. } | NodeKind::Class { name, .. } => {
            push_str(&mut parts, &mut truncated, "name", name);
        }
        NodeKind::Format { name, body, .. } => {
            push_str(&mut parts, &mut truncated, "name", name);
            push_str(&mut parts, &mut truncated, "body", body);
        }
        NodeKind::Use { module, args, has_filter_risk }
        | NodeKind::No { module, args, has_filter_risk } => {
            push_str(&mut parts, &mut truncated, "module", module);
            push_list(&mut parts, &mut truncated, "args", args);
            push_bool(&mut parts, "has_filter_risk", *has_filter_risk);
        }
        NodeKind::PhaseBlock { phase, .. } => push_str(&mut parts, &mut truncated, "phase", phase),
        NodeKind::DataSection { marker, body, .. } => {
            push_str(&mut parts, &mut truncated, "marker", marker);
            if let Some(body) = body {
                push_str(&mut parts, &mut truncated, "body", body);
            }
        }
        NodeKind::Identifier { name } => push_str(&mut parts, &mut truncated, "name", name),
        NodeKind::Error { message, .. } => push_str(&mut parts, &mut truncated, "message", message),
        NodeKind::ChainedComparison { ops, .. } => {
            push_list(&mut parts, &mut truncated, "ops", ops);
        }
        NodeKind::Prototype { content } => {
            push_str(&mut parts, &mut truncated, "content", content);
        }
        NodeKind::Foreach { .. }
        | NodeKind::For { .. }
        | NodeKind::Given { .. }
        | NodeKind::When { .. }
        | NodeKind::Default { .. }
        | NodeKind::Program { .. }
        | NodeKind::Block { .. }
        | NodeKind::ExpressionStatement { .. }
        | NodeKind::NestedVariableList { .. }
        | NodeKind::ArraySlice { .. }
        | NodeKind::HashSlice { .. }
        | NodeKind::KeyValueSlice { .. }
        | NodeKind::Ternary { .. }
        | NodeKind::Diamond
        | NodeKind::Ellipsis
        | NodeKind::Undef
        | NodeKind::ArrayLiteral { .. }
        | NodeKind::HashLiteral { .. }
        | NodeKind::Eval { .. }
        | NodeKind::Do { .. }
        | NodeKind::Defer { .. }
        | NodeKind::Try { .. }
        | NodeKind::Tie { .. }
        | NodeKind::Untie { .. }
        | NodeKind::Signature { .. }
        | NodeKind::MandatoryParameter { .. }
        | NodeKind::OptionalParameter { .. }
        | NodeKind::SlurpyParameter { .. }
        | NodeKind::Return { .. }
        | NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock
        | NodeKind::UnknownRest => {}
    }
    PayloadSummary { text: if parts.is_empty() { None } else { Some(parts.join(" ")) }, truncated }
}

fn push_str(parts: &mut Vec<String>, truncated: &mut bool, key: &str, value: &str) {
    let (head, cut) = trunc_chars(value, NODE_DEBUG_MAX_PAYLOAD_CHARS);
    if cut {
        *truncated = true;
        parts.push(format!("{key}:\"{}...\"", head.escape_debug()));
    } else {
        parts.push(format!("{key}:\"{}\"", head.escape_debug()));
    }
}

fn push_bool(parts: &mut Vec<String>, key: &str, value: bool) {
    parts.push(format!("{key}={value}"));
}

fn push_list(parts: &mut Vec<String>, truncated: &mut bool, key: &str, values: &[String]) {
    const MAX_ITEMS: usize = 3;
    let mut rendered = Vec::new();
    for value in values.iter().take(MAX_ITEMS) {
        let (head, cut) = trunc_chars(value, NODE_DEBUG_MAX_PAYLOAD_CHARS);
        if cut {
            *truncated = true;
            rendered.push(format!("\"{}...\"", head.escape_debug()));
        } else {
            rendered.push(format!("\"{}\"", head.escape_debug()));
        }
    }
    if values.len() > MAX_ITEMS {
        *truncated = true;
        rendered.push(format!("... +{}", values.len() - MAX_ITEMS));
    }
    parts.push(format!("{key}=[{}]", rendered.join(", ")));
}

fn trunc_chars(value: &str, max_chars: usize) -> (String, bool) {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    (head, chars.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::{
        DebugObserver, NODE_DEBUG_MAX_BYTES, NODE_DEBUG_MAX_CHILDREN, NODE_DEBUG_MAX_DEPTH,
        NODE_DEBUG_MAX_NODES, NODE_DEBUG_MAX_PAYLOAD_CHARS, NODE_DEBUG_TRUNCATION_MARKER,
        render_node, sketch_node,
    };
    use crate::ast::{Node, NodeKind, SourceLocation};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    struct Recording {
        nodes_entered: u64,
        max_explicit_stack_depth: usize,
    }

    impl DebugObserver for Recording {
        fn on_enter(&mut self) {
            self.nodes_entered = self.nodes_entered.saturating_add(1);
        }

        fn on_stack_depth(&mut self, depth: usize) {
            if depth > self.max_explicit_stack_depth {
                self.max_explicit_stack_depth = depth;
            }
        }
    }

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation::new(start, end)
    }

    fn numbered(value: &str, start: usize) -> Node {
        Node::new(NodeKind::Number { value: value.to_string() }, loc(start, start + 1))
    }

    fn program(children: Vec<Node>) -> Node {
        Node::new(NodeKind::Program { statements: children }, loc(0, 1))
    }

    fn wrap_expr(inner: Node) -> Node {
        let location = inner.location;
        Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, location)
    }

    fn chain(depth: usize, leaf: Node) -> Node {
        let mut node = leaf;
        for _ in 0..depth {
            node = wrap_expr(node);
        }
        node
    }

    fn hash_debug(rendered: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        rendered.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn leaf_shows_kind_range_and_payload() {
        let node = numbered("42", 3);
        let rendered = format!("{node:?}");
        assert!(rendered.contains("Number"), "rendered = {rendered:?}");
        assert!(rendered.contains("@3..4"), "rendered = {rendered:?}");
        assert!(rendered.contains("value:\"42\""), "rendered = {rendered:?}");
        assert!(
            !rendered.contains("location: SourceLocation"),
            "must not be derived Debug: {rendered}"
        );
        assert!(!rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
    }

    #[test]
    fn parent_projects_named_children() {
        let node = wrap_expr(numbered("1", 0));
        let rendered = format!("{node:?}");
        assert!(rendered.contains("ExpressionStatement"), "rendered = {rendered:?}");
        assert!(rendered.contains("expression:"), "rendered = {rendered:?}");
        assert!(rendered.contains("Number"), "rendered = {rendered:?}");
    }

    #[test]
    fn large_string_payload_is_truncated_visibly() {
        let value = "é".repeat(NODE_DEBUG_MAX_PAYLOAD_CHARS + 8);
        let node =
            Node::new(NodeKind::String { value: value.clone(), interpolated: false }, loc(0, 1));
        let rendered = format!("{node:?}");
        assert!(rendered.contains("String"), "rendered = {rendered:?}");
        assert!(rendered.contains("..."), "rendered = {rendered:?}");
        assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
        assert!(!rendered.contains(&value), "full payload must not appear: {rendered}");
        assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
    }

    #[test]
    fn heredoc_content_is_bounded() {
        let content = "secret-source\n".repeat(40);
        let node = Node::new(
            NodeKind::Heredoc {
                delimiter: "EOF".to_string(),
                content: content.clone(),
                interpolated: false,
                indented: false,
                command: false,
                body_span: None,
            },
            loc(0, 80),
        );
        let rendered = format!("{node:?}");
        assert!(rendered.contains("delimiter:\"EOF\""), "missing delimiter in {}", rendered);
        assert!(
            rendered.contains(NODE_DEBUG_TRUNCATION_MARKER),
            "missing truncation marker in {}",
            rendered
        );
        assert!(!rendered.contains(&content), "full heredoc leaked in {}", rendered);
    }

    #[test]
    fn wide_program_omits_extra_children() {
        let statements: Vec<Node> =
            (0..(NODE_DEBUG_MAX_CHILDREN + 5)).map(|i| numbered(&i.to_string(), i)).collect();
        let omitted = statements.len() - NODE_DEBUG_MAX_CHILDREN;
        let node = program(statements);
        let rendered = format!("{node:?}");
        assert!(rendered.contains("Program"), "rendered = {rendered:?}");
        assert!(rendered.contains(&format!("... +{omitted}")), "rendered = {rendered:?}");
        assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
        assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
    }

    #[test]
    fn depth_cap_hides_leaf_payload() {
        let left = chain(NODE_DEBUG_MAX_DEPTH + 4, numbered("1", 0));
        let right = chain(NODE_DEBUG_MAX_DEPTH + 4, numbered("leaf-hidden", 0));
        assert_ne!(left, right);
        let left_dbg = format!("{left:?}");
        let right_dbg = format!("{right:?}");
        assert_eq!(left_dbg, right_dbg, "truncated Debug must not be identity");
        assert!(left_dbg.contains(NODE_DEBUG_TRUNCATION_MARKER), "left = {left_dbg:?}");
        assert!(!left_dbg.contains("leaf-hidden"), "left = {left_dbg:?}");
        assert_eq!(hash_debug(&left_dbg), hash_debug(&right_dbg));
    }

    #[test]
    fn debug_bytes_are_not_an_equality_oracle() {
        let left = chain(NODE_DEBUG_MAX_DEPTH + 2, numbered("a", 0));
        let right = chain(NODE_DEBUG_MAX_DEPTH + 2, numbered("b", 0));
        assert_ne!(left, right, "PartialEq remains exact");
        let left_dbg = format!("{left:?}");
        let right_dbg = format!("{right:?}");
        assert_eq!(left_dbg, right_dbg);
        assert_ne!(left, right);
    }

    #[test]
    fn equal_trees_render_deterministically() {
        let left = program(vec![wrap_expr(numbered("1", 0)), numbered("2", 1)]);
        let right = program(vec![wrap_expr(numbered("1", 0)), numbered("2", 1)]);
        assert_eq!(left, right);
        assert_eq!(format!("{left:?}"), format!("{right:?}"));
        assert_eq!(format!("{left:?}"), format!("{left:?}"));
    }

    #[test]
    fn observer_uses_explicit_stack_and_bounds_visits() {
        let node = chain(20, numbered("1", 0));
        let mut work = Recording { nodes_entered: 0, max_explicit_stack_depth: 0 };
        let rendered = render_node(&node, &mut work);
        assert!(work.nodes_entered > 0);
        assert!(
            work.nodes_entered <= NODE_DEBUG_MAX_NODES as u64,
            "entered={}",
            work.nodes_entered
        );
        assert!(work.max_explicit_stack_depth >= 1, "must use the heap stack");
        assert!(work.max_explicit_stack_depth <= NODE_DEBUG_MAX_DEPTH + 2);
        assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
        let _ = sketch_node(&node, &mut ());
    }

    fn bushy(depth: usize, width: usize) -> Node {
        if depth == 0 {
            return numbered("1", 0);
        }
        program((0..width).map(|_| bushy(depth.saturating_sub(1), width)).collect())
    }

    fn count_sketched(sketch: &super::Sketch) -> usize {
        sketch
            .children
            .iter()
            .fold(1usize, |total, child| total.saturating_add(count_sketched(&child.sketch)))
    }

    fn collect_omitted(sketch: &super::Sketch, out: &mut Vec<usize>) {
        if sketch.omitted_children > 0 {
            out.push(sketch.omitted_children);
        }
        for child in &sketch.children {
            collect_omitted(&child.sketch, out);
        }
    }

    #[test]
    fn node_budget_counts_active_frames() {
        let node = bushy(2, NODE_DEBUG_MAX_CHILDREN);
        let mut work = Recording { nodes_entered: 0, max_explicit_stack_depth: 0 };
        let sketch = sketch_node(&node, &mut work);
        assert!(
            work.nodes_entered <= NODE_DEBUG_MAX_NODES as u64,
            "entered={}",
            work.nodes_entered
        );
        assert!(
            count_sketched(&sketch) <= NODE_DEBUG_MAX_NODES,
            "sketched={}",
            count_sketched(&sketch)
        );
        assert!(sketch.truncated, "bushy tree must exhaust the node budget");
        let rendered = render_node(&node, &mut ());
        assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
        assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
    }

    #[test]
    fn node_budget_preserves_omitted_child_count() {
        let node = bushy(2, NODE_DEBUG_MAX_CHILDREN);
        let sketch = sketch_node(&node, &mut ());
        let mut omitted = Vec::new();
        collect_omitted(&sketch, &mut omitted);
        assert!(
            omitted.iter().any(|&count| count > 1),
            "remaining unvisited siblings must be counted, omitted={omitted:?}"
        );
    }

    #[test]
    fn nodekind_debug_does_not_dump_children() {
        let statements: Vec<Node> = (0..40).map(|i| numbered(&"x".repeat(8), i)).collect();
        let node = program(statements);
        let rendered = format!("{:?}", node.kind);
        assert!(rendered.starts_with("Program"), "rendered = {rendered:?}");
        assert!(!rendered.contains("Number"), "NodeKind Debug must not dump children: {rendered}");
        assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
    }

    #[test]
    fn derived_recursive_shape_is_absent() {
        let node = wrap_expr(numbered("1", 0));
        let rendered = format!("{node:?}");
        assert!(!rendered.contains("kind: ExpressionStatement"), "rendered = {rendered:?}");
        assert!(!rendered.contains("location: SourceLocation"), "rendered = {rendered:?}");
        assert!(rendered.starts_with("Node("), "rendered = {rendered:?}");
    }
}
