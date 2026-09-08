//! Native debug S-expression projection for [`Node`].
//!
//! This is a non-normative human/debug rendering. It is **not** Tree-sitter
//! compatibility, typed machine output, AST equality, or source fidelity.
//! Compatibility CST work belongs on issue 8047. Typed bounded execution of
//! this projection belongs on issue 8832.
//!
//! Grammar (versioned as [`NATIVE_DEBUG_SEXP_GRAMMAR`]):
//!
//! ```text
//! form     := '(' kind-atom payload-or-field* ')'
//! field    := '(' field-name catch-binder? form ')'
//! payload  := '(' payload-name atom* ')'
//! atom     := unquoted-symbol | quoted-utf8
//! ```
//!
//! Laws:
//! - one [`Node`] emits exactly one root form
//! - child fields come from the #8424 visit table and keep that order
//! - optional/empty payloads are omitted
//! - recovery kinds stay visible
//! - quoted atoms escape quotes, backslash, and controls; printable Unicode stays UTF-8
//!
//! [`Node::render_debug_sexp`] is the authority for completeness. It walks an
//! explicit heap stack, honors caller-selected limits, and returns
//! [`NativeDebugSexpResult`]. [`Node::to_sexp`] is a `String` convenience over
//! that engine and cannot prove completeness.

use super::{FieldId, GotoTargetForm, Node, NodeKind, Token, TokenKind};
use std::fmt::{self, Write as _};

/// Grammar identity for this native debug projection.
pub const NATIVE_DEBUG_SEXP_GRAMMAR: &str = "perl-ast-native-debug-sexp/v1";

/// Historical marker previously returned when the recursive depth guard fired.
///
/// This string is not a [`NodeKind`]. Bounded rendering reports
/// [`NativeDebugSexpResult::Truncated`] instead of injecting this form.
pub const NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER: &str = "(depth_limit_exceeded)";

/// Caller-selected bounds for [`Node::render_debug_sexp`].
///
/// `None` on a field means that dimension is unbounded. Root depth is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeDebugSexpLimits {
    /// Maximum nodes that may be admitted, including the root.
    pub max_nodes: Option<usize>,
    /// Inclusive maximum root-relative depth that may be entered.
    pub max_depth: Option<usize>,
    /// Maximum UTF-8 bytes that may be accepted by the bounded writer.
    pub max_bytes: Option<usize>,
    /// Maximum work units that may be charged (admit, payload, edge, close).
    pub max_work: Option<usize>,
}

impl NativeDebugSexpLimits {
    /// No caller-selected bound on any dimension.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self { max_nodes: None, max_depth: None, max_bytes: None, max_work: None }
    }
}

/// Work actually charged by one [`Node::render_debug_sexp`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeDebugSexpWork {
    /// Nodes admitted, including the root when it was entered.
    pub nodes_visited: usize,
    /// Child edges descended after a depth check succeeded.
    pub child_edges_visited: usize,
    /// Maximum root-relative depth actually entered.
    pub max_depth: usize,
    /// UTF-8 bytes successfully forwarded to the destination writer.
    pub bytes_written: usize,
    /// Admit, payload-phase, edge, and close units actually charged.
    pub work_units: usize,
}

/// Why a bounded render stopped before exhausting the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeDebugSexpTruncation {
    /// Admitting another node would exceed [`NativeDebugSexpLimits::max_nodes`].
    NodeLimit {
        /// Maximum nodes this render may admit.
        limit: usize,
    },
    /// A child would have been entered beyond [`NativeDebugSexpLimits::max_depth`].
    DepthLimit {
        /// Inclusive maximum root-relative depth that may be entered.
        limit: usize,
    },
    /// The next complete token would exceed [`NativeDebugSexpLimits::max_bytes`].
    ByteLimit {
        /// Maximum UTF-8 bytes this render may emit.
        limit: usize,
    },
    /// The next work unit would exceed [`NativeDebugSexpLimits::max_work`].
    WorkLimit {
        /// Maximum work units this render may charge.
        limit: usize,
    },
}

/// Remaining members not projected after truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDebugSexpOmitted {
    /// Exact remaining count already known without walking omitted subtrees.
    Known(usize),
    /// Remaining members were not counted.
    Unknown,
}

/// Internal arithmetic, writer, or frame-state failure.
///
/// Caller-selected bounds are [`NativeDebugSexpTruncation`], not this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeDebugSexpInstrumentCause {
    /// A checked node, edge, byte, or work counter overflowed `usize`.
    WorkCounterOverflow,
    /// The destination [`fmt::Write`] returned an error.
    WriterError,
    /// The explicit frame stack was empty or inconsistent.
    InternalFrameState,
}

/// Typed terminality for [`Node::render_debug_sexp`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum NativeDebugSexpResult {
    /// The projection exhausted the tree under the selected limits.
    Complete {
        /// Nodes, edges, bytes, and work actually charged.
        work: NativeDebugSexpWork,
    },
    /// A caller-selected bound stopped the walk.
    Truncated {
        /// Why the walk stopped.
        reason: NativeDebugSexpTruncation,
        /// Work charged before the rejected operation.
        work: NativeDebugSexpWork,
        /// Remaining members, or [`NativeDebugSexpOmitted::Unknown`].
        omitted: NativeDebugSexpOmitted,
    },
    /// Checked arithmetic, writer failure, or internal frame state failed.
    InstrumentFailure {
        /// Stable cause of the internal failure.
        cause: NativeDebugSexpInstrumentCause,
        /// Work charged before the failure.
        work: NativeDebugSexpWork,
    },
}

enum RenderStop {
    Truncated(NativeDebugSexpTruncation),
    Instrument(NativeDebugSexpInstrumentCause),
}

trait SexpSink {
    fn emit(&mut self, s: &str) -> Result<(), RenderStop>;
    fn emit_atom(&mut self, value: &str) -> Result<(), RenderStop>;
}

struct Frame<'a> {
    node: &'a Node,
    field: Option<FieldId>,
    catch_binder: Option<&'a str>,
    depth: usize,
    is_child: bool,
    opened: bool,
    children: Vec<(Option<FieldId>, &'a Node, Option<&'a str>)>,
    next_child: usize,
}

struct Renderer<'tree, 'write, W: fmt::Write> {
    writer: &'write mut W,
    limits: NativeDebugSexpLimits,
    work: NativeDebugSexpWork,
    scratch: String,
    stack: Vec<Frame<'tree>>,
}

impl Node {
    /// Render this node as a native debug S-expression.
    ///
    /// The result is a single-root, lossy projection for humans and snapshot
    /// tests. It is not a Tree-sitter compatibility CST (issue 8047), not a
    /// typed machine schema (issue 8044), and not AST equality (issue 7045).
    ///
    /// This convenience allocates a `String` and discards typed terminality.
    /// Callers that need completeness, truncation, or streaming must use
    /// [`Self::render_debug_sexp`].
    ///
    /// Child order follows the canonical #8424 visit table. Non-child payloads
    /// (keywords, attributes, operators, catch binders, recovery tokens) are
    /// renderer-local and nested under the owning root. Source-location
    /// payloads such as `name_span` and `body_span` are omitted on purpose.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let loc = SourceLocation { start: 0, end: 2 };
    /// let num = Node::new(NodeKind::Number { value: "42".to_string() }, loc);
    /// let program = Node::new(
    ///     NodeKind::Program { statements: vec![num] },
    ///     loc,
    /// );
    /// let sexp = program.to_sexp();
    /// assert!(sexp.starts_with("(source_file"));
    /// ```
    pub fn to_sexp(&self) -> String {
        let mut out = String::new();
        match self.render_debug_sexp(&mut out, NativeDebugSexpLimits::unbounded()) {
            NativeDebugSexpResult::Complete { .. }
            | NativeDebugSexpResult::Truncated { .. }
            | NativeDebugSexpResult::InstrumentFailure { .. } => out,
        }
    }

    /// Render this node with the same projection as [`Self::to_sexp`].
    ///
    /// Historically this unwrapped program-child expression statements. That
    /// parent-context rewrite is gone: one node has one debug form.
    pub fn to_sexp_inner(&self) -> String {
        self.to_sexp()
    }

    /// Stream the native debug projection into `writer` under `limits`.
    ///
    /// The walk is iterative. Child identity comes from the #8424 visit table.
    /// Payload disposition is the #8829 grammar. Completeness is this result,
    /// not the destination bytes. Truncation is not represented as an AST node.
    ///
    /// Check order for a rejected operation:
    ///
    /// 1. node limit, before admitting a node and before descend, so an
    ///    exhausted node budget wins over depth or edge-work on that child
    /// 2. depth limit, before descending; a rejected descent charges no edge
    ///    and no work
    /// 3. work limit, before admit / payload / edge / close of an allowed
    ///    operation
    /// 4. byte limit, before forwarding a complete token
    ///
    /// A `fmt::Write` error is [`NativeDebugSexpInstrumentCause::WriterError`],
    /// not truncation. Overflow of a checked counter is
    /// [`NativeDebugSexpInstrumentCause::WorkCounterOverflow`].
    pub fn render_debug_sexp<W: fmt::Write>(
        &self,
        writer: &mut W,
        limits: NativeDebugSexpLimits,
    ) -> NativeDebugSexpResult {
        let mut renderer = Renderer {
            writer,
            limits,
            work: NativeDebugSexpWork::default(),
            scratch: String::new(),
            stack: Vec::new(),
        };
        match renderer.run(self) {
            Ok(()) => NativeDebugSexpResult::Complete { work: renderer.work },
            Err(RenderStop::Truncated(reason)) => NativeDebugSexpResult::Truncated {
                reason,
                work: renderer.work,
                omitted: NativeDebugSexpOmitted::Unknown,
            },
            Err(RenderStop::Instrument(cause)) => {
                NativeDebugSexpResult::InstrumentFailure { cause, work: renderer.work }
            }
        }
    }
}

impl fmt::Display for Node {
    /// Formats as the native debug S-expression. See [`Node::to_sexp`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.render_debug_sexp(f, NativeDebugSexpLimits::unbounded()) {
            NativeDebugSexpResult::Complete { .. } | NativeDebugSexpResult::Truncated { .. } => {
                Ok(())
            }
            NativeDebugSexpResult::InstrumentFailure { .. } => Err(fmt::Error),
        }
    }
}

impl<'tree, 'write, W: fmt::Write> Renderer<'tree, 'write, W> {
    fn run(&mut self, root: &'tree Node) -> Result<(), RenderStop> {
        self.admit_node(0)?;
        self.stack.push(Frame {
            node: root,
            field: None,
            catch_binder: None,
            depth: 0,
            is_child: false,
            opened: false,
            children: Vec::new(),
            next_child: 0,
        });

        loop {
            let needs_open = self.stack.last().is_some_and(|frame| !frame.opened);
            if needs_open {
                self.open_top()?;
                continue;
            }

            let child_job = self.stack.last().and_then(|frame| {
                frame
                    .children
                    .get(frame.next_child)
                    .copied()
                    .map(|(field, node, binder)| (field, node, binder, frame.depth))
            });
            if let Some((field, node, binder, parent_depth)) = child_job {
                if let Some(frame) = self.stack.last_mut() {
                    frame.next_child = frame.next_child.saturating_add(1);
                }
                self.check_node_limit()?;
                let depth = self.descend(parent_depth)?;
                self.admit_node(depth)?;
                self.stack.push(Frame {
                    node,
                    field,
                    catch_binder: binder,
                    depth,
                    is_child: true,
                    opened: false,
                    children: Vec::new(),
                    next_child: 0,
                });
                continue;
            }

            let Some(frame) = self.stack.pop() else {
                break;
            };
            self.close_frame(frame.is_child)?;
        }
        Ok(())
    }

    fn open_top(&mut self) -> Result<(), RenderStop> {
        let (is_child, field, binder, node) = match self.stack.last() {
            Some(frame) => (frame.is_child, frame.field, frame.catch_binder, frame.node),
            None => {
                return Err(RenderStop::Instrument(
                    NativeDebugSexpInstrumentCause::InternalFrameState,
                ));
            }
        };
        if is_child {
            self.emit(" ")?;
            self.emit("(")?;
            match field {
                Some(field) => self.emit_atom(field.name())?,
                None => self.emit_atom("child")?,
            }
            if let Some(binder) = binder {
                self.emit(" ")?;
                self.emit_atom(binder)?;
            }
            self.emit(" ")?;
        }
        self.emit("(")?;
        let kind = node.kind.grammar_kind_name();
        self.emit_atom(&kind)?;
        self.charge_work(1)?;
        write_payloads(&node.kind, self)?;
        let children = load_children(node);
        match self.stack.last_mut() {
            Some(frame) => {
                frame.children = children;
                frame.opened = true;
            }
            None => {
                return Err(RenderStop::Instrument(
                    NativeDebugSexpInstrumentCause::InternalFrameState,
                ));
            }
        }
        Ok(())
    }

    fn close_frame(&mut self, is_child: bool) -> Result<(), RenderStop> {
        self.charge_work(1)?;
        self.emit(")")?;
        if is_child {
            self.emit(")")?;
        }
        Ok(())
    }

    fn next_node_count(&self) -> Result<usize, RenderStop> {
        self.work
            .nodes_visited
            .checked_add(1)
            .ok_or(RenderStop::Instrument(NativeDebugSexpInstrumentCause::WorkCounterOverflow))
    }

    fn check_node_limit(&self) -> Result<(), RenderStop> {
        let next_nodes = self.next_node_count()?;
        if let Some(limit) = self.limits.max_nodes
            && next_nodes > limit
        {
            return Err(RenderStop::Truncated(NativeDebugSexpTruncation::NodeLimit { limit }));
        }
        Ok(())
    }

    fn admit_node(&mut self, depth: usize) -> Result<(), RenderStop> {
        self.check_node_limit()?;
        let next_nodes = self.next_node_count()?;
        self.charge_work(1)?;
        self.work.nodes_visited = next_nodes;
        if depth > self.work.max_depth {
            self.work.max_depth = depth;
        }
        Ok(())
    }

    fn descend(&mut self, parent_depth: usize) -> Result<usize, RenderStop> {
        let child_depth = parent_depth
            .checked_add(1)
            .ok_or(RenderStop::Instrument(NativeDebugSexpInstrumentCause::WorkCounterOverflow))?;
        if let Some(limit) = self.limits.max_depth
            && child_depth > limit
        {
            return Err(RenderStop::Truncated(NativeDebugSexpTruncation::DepthLimit { limit }));
        }
        self.charge_work(1)?;
        let next_edges =
            self.work.child_edges_visited.checked_add(1).ok_or(RenderStop::Instrument(
                NativeDebugSexpInstrumentCause::WorkCounterOverflow,
            ))?;
        self.work.child_edges_visited = next_edges;
        Ok(child_depth)
    }

    fn charge_work(&mut self, units: usize) -> Result<(), RenderStop> {
        let next =
            self.work.work_units.checked_add(units).ok_or(RenderStop::Instrument(
                NativeDebugSexpInstrumentCause::WorkCounterOverflow,
            ))?;
        if let Some(limit) = self.limits.max_work
            && next > limit
        {
            return Err(RenderStop::Truncated(NativeDebugSexpTruncation::WorkLimit { limit }));
        }
        self.work.work_units = next;
        Ok(())
    }
}

impl<W: fmt::Write> SexpSink for Renderer<'_, '_, W> {
    fn emit(&mut self, s: &str) -> Result<(), RenderStop> {
        if s.is_empty() {
            return Ok(());
        }
        let next =
            self.work.bytes_written.checked_add(s.len()).ok_or(RenderStop::Instrument(
                NativeDebugSexpInstrumentCause::WorkCounterOverflow,
            ))?;
        if let Some(limit) = self.limits.max_bytes
            && next > limit
        {
            return Err(RenderStop::Truncated(NativeDebugSexpTruncation::ByteLimit { limit }));
        }
        self.writer
            .write_str(s)
            .map_err(|_| RenderStop::Instrument(NativeDebugSexpInstrumentCause::WriterError))?;
        self.work.bytes_written = next;
        Ok(())
    }

    fn emit_atom(&mut self, value: &str) -> Result<(), RenderStop> {
        self.scratch.clear();
        write_atom(&mut self.scratch, value);
        let encoded = std::mem::take(&mut self.scratch);
        let result = self.emit(&encoded);
        self.scratch = encoded;
        result
    }
}

fn load_children(node: &Node) -> Vec<(Option<FieldId>, &Node, Option<&str>)> {
    let mut catch_index = 0usize;
    let mut children = Vec::new();
    node.for_each_child_with_field(|field, child| {
        let binder = if field == Some(FieldId::CATCH) {
            if let NodeKind::Try { catch_blocks, .. } = &node.kind {
                let name = catch_blocks
                    .get(catch_index)
                    .and_then(|(binder, _)| binder.as_ref().map(|(name, _)| name.as_str()));
                catch_index = catch_index.saturating_add(1);
                name
            } else {
                None
            }
        } else {
            None
        };
        children.push((field, child, binder));
    });
    children
}

fn write_payloads(kind: &NodeKind, out: &mut impl SexpSink) -> Result<(), RenderStop> {
    match kind {
        NodeKind::Variable { sigil, name } => {
            write_named(out, "sigil", sigil)?;
            write_named(out, "name", name)
        }
        NodeKind::VariableDeclaration { declarator, attributes, .. }
        | NodeKind::VariableListDeclaration { declarator, attributes, .. } => {
            write_named(out, "declarator", declarator)?;
            write_named_list(out, "attributes", attributes)
        }
        NodeKind::VariableWithAttributes { attributes, .. } => {
            write_named_list(out, "attributes", attributes)
        }
        NodeKind::Assignment { op, .. }
        | NodeKind::Binary { op, .. }
        | NodeKind::Unary { op, .. } => write_named(out, "op", op),
        NodeKind::Number { value } | NodeKind::VString { value } => {
            write_named(out, "value", value)
        }
        NodeKind::String { value, interpolated } => {
            write_named(out, "value", value)?;
            write_flag(out, "interpolated", *interpolated)
        }
        NodeKind::Heredoc { delimiter, content, interpolated, indented, command, body_span: _ } => {
            write_named(out, "delimiter", delimiter)?;
            write_named(out, "content", content)?;
            write_flag(out, "interpolated", *interpolated)?;
            write_flag(out, "indented", *indented)?;
            write_flag(out, "command", *command)
        }
        NodeKind::Readline { filehandle } => {
            if let Some(filehandle) = filehandle {
                write_named(out, "filehandle", filehandle)?;
            }
            Ok(())
        }
        NodeKind::Glob { pattern } => write_named(out, "pattern", pattern),
        NodeKind::Typeglob { name } => write_named(out, "name", name),
        NodeKind::If { keyword, .. } | NodeKind::While { keyword, .. } => {
            if let Some(keyword) = keyword {
                write_named(out, "keyword", keyword)?;
            }
            Ok(())
        }
        NodeKind::LabeledStatement { label, .. } => write_named(out, "label", label),
        NodeKind::StatementModifier { modifier, .. } => write_named(out, "modifier", modifier),
        NodeKind::Subroutine { name, declarator, attributes, name_span: _, .. } => {
            if let Some(name) = name {
                write_named(out, "name", name)?;
            }
            if let Some(declarator) = declarator {
                write_named(out, "declarator", declarator)?;
            }
            write_named_list(out, "attributes", attributes)
        }
        NodeKind::NamedParameter { external_name, default_operator, required, .. } => {
            write_named(out, "external_name", external_name)?;
            if let Some(op) = default_operator {
                write_named(out, "default_operator", op)?;
            }
            write_flag(out, "required", *required)
        }
        NodeKind::Method { name, attributes, name_span: _, .. } => {
            write_named(out, "name", name)?;
            write_named_list(out, "attributes", attributes)
        }
        NodeKind::LoopControl { op, label } => {
            write_named(out, "op", op)?;
            if let Some(label) = label {
                write_named(out, "label", label)?;
            }
            Ok(())
        }
        NodeKind::Goto { form, .. } => write_named(out, "form", goto_form_atom(form)),
        NodeKind::MethodCall { method, .. } | NodeKind::IndirectCall { method, .. } => {
            write_named(out, "method", method)
        }
        NodeKind::FunctionCall { name, .. } | NodeKind::AmperCall { name, .. } => {
            write_named(out, "name", name)
        }
        NodeKind::Regex { pattern, replacement, modifiers, has_embedded_code } => {
            write_named(out, "pattern", pattern)?;
            if let Some(replacement) = replacement {
                write_named(out, "replacement", replacement)?;
            }
            write_named(out, "modifiers", modifiers)?;
            write_flag(out, "has_embedded_code", *has_embedded_code)
        }
        NodeKind::Match { pattern, modifiers, has_embedded_code, negated, .. } => {
            write_named(out, "pattern", pattern)?;
            write_named(out, "modifiers", modifiers)?;
            write_flag(out, "has_embedded_code", *has_embedded_code)?;
            write_flag(out, "negated", *negated)
        }
        NodeKind::Substitution {
            pattern,
            replacement,
            modifiers,
            has_embedded_code,
            negated,
            ..
        } => {
            write_named(out, "pattern", pattern)?;
            write_named(out, "replacement", replacement)?;
            write_named(out, "modifiers", modifiers)?;
            write_flag(out, "has_embedded_code", *has_embedded_code)?;
            write_flag(out, "negated", *negated)
        }
        NodeKind::Transliteration { search, replace, modifiers, negated, .. } => {
            write_named(out, "search", search)?;
            write_named(out, "replace", replace)?;
            write_named(out, "modifiers", modifiers)?;
            write_flag(out, "negated", *negated)
        }
        NodeKind::Package { name, name_span: _, .. } => write_named(out, "name", name),
        NodeKind::Class { name, parents, name_span: _, .. } => {
            write_named(out, "name", name)?;
            write_named_list(out, "parents", parents)
        }
        NodeKind::Format { name, body, name_span: _, .. } => {
            write_named(out, "name", name)?;
            write_named(out, "body", body)
        }
        NodeKind::Use { module, args, has_filter_risk }
        | NodeKind::No { module, args, has_filter_risk } => {
            write_named(out, "module", module)?;
            write_named_list(out, "args", args)?;
            write_flag(out, "has_filter_risk", *has_filter_risk)
        }
        NodeKind::PhaseBlock { phase, phase_span: _, .. } => write_named(out, "phase", phase),
        NodeKind::DataSection { marker, body, marker_span: _, body_span: _ } => {
            write_named(out, "marker", marker)?;
            if let Some(body) = body {
                write_named(out, "body", body)?;
            }
            Ok(())
        }
        NodeKind::Identifier { name } => write_named(out, "name", name),
        NodeKind::Error { message, expected, found, .. } => {
            write_named(out, "message", message)?;
            write_expected_tokens(out, expected)?;
            if let Some(token) = found {
                write_found_token(out, token)?;
            }
            Ok(())
        }
        NodeKind::ChainedComparison { ops, .. } => write_named_list(out, "ops", ops),
        NodeKind::Prototype { content } => write_named(out, "content", content),
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
        | NodeKind::UnknownRest => Ok(()),
    }
}

fn goto_form_atom(form: &GotoTargetForm) -> &'static str {
    match form {
        GotoTargetForm::Label => "label",
        GotoTargetForm::Sub => "sub",
        GotoTargetForm::Expr => "expr",
    }
}

fn write_named(out: &mut impl SexpSink, name: &str, value: &str) -> Result<(), RenderStop> {
    out.emit(" ")?;
    out.emit("(")?;
    out.emit_atom(name)?;
    out.emit(" ")?;
    out.emit_atom(value)?;
    out.emit(")")
}

fn write_named_list(
    out: &mut impl SexpSink,
    name: &str,
    values: &[String],
) -> Result<(), RenderStop> {
    if values.is_empty() {
        return Ok(());
    }
    out.emit(" ")?;
    out.emit("(")?;
    out.emit_atom(name)?;
    for value in values {
        out.emit(" ")?;
        out.emit_atom(value)?;
    }
    out.emit(")")
}

fn write_flag(out: &mut impl SexpSink, name: &str, value: bool) -> Result<(), RenderStop> {
    if value { write_named(out, name, "true") } else { Ok(()) }
}

fn write_expected_tokens(
    out: &mut impl SexpSink,
    expected: &[TokenKind],
) -> Result<(), RenderStop> {
    if expected.is_empty() {
        return Ok(());
    }
    out.emit(" ")?;
    out.emit("(")?;
    out.emit_atom("expected")?;
    for kind in expected {
        out.emit(" ")?;
        out.emit_atom(&format!("{kind:?}"))?;
    }
    out.emit(")")
}

fn write_found_token(out: &mut impl SexpSink, token: &Token) -> Result<(), RenderStop> {
    out.emit(" ")?;
    out.emit("(")?;
    out.emit_atom("found")?;
    out.emit(" ")?;
    out.emit_atom(&format!("{:?}", token.kind()))?;
    out.emit(" ")?;
    out.emit_atom(token.text.as_ref())?;
    out.emit(")")
}

fn write_atom(out: &mut String, value: &str) {
    if needs_quoting(value) {
        out.push('"');
        for ch in value.chars() {
            write_escaped_char(out, ch);
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

fn write_escaped_char(out: &mut String, ch: char) {
    match ch {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        ch if ch.is_control() => {
            out.push_str("\\u{");
            let _ = write!(out, "{:x}", ch as u32);
            out.push('}');
        }
        ch => out.push(ch),
    }
}

fn needs_quoting(value: &str) -> bool {
    value.is_empty()
        || value.chars().any(|ch| {
            ch == '('
                || ch == ')'
                || ch == '"'
                || ch == '\\'
                || ch == ';'
                || ch.is_whitespace()
                || ch.is_control()
        })
}

#[cfg(test)]
mod tests {
    use super::{
        NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, NativeDebugSexpLimits, NativeDebugSexpResult,
        needs_quoting, write_atom,
    };
    use crate::ast::{Node, NodeKind, SourceLocation};

    fn loc() -> SourceLocation {
        SourceLocation { start: 0, end: 1 }
    }

    #[test]
    fn empty_and_special_atoms_are_quoted() {
        assert!(needs_quoting(""));
        assert!(needs_quoting("a b"));
        assert!(needs_quoting("a(b)"));
        assert!(needs_quoting("say\"hi"));
        assert!(needs_quoting("a\\b"));
        assert!(needs_quoting("a\nb"));
        assert!(!needs_quoting("binary_+"));
        assert!(!needs_quoting("source_file"));
        assert!(!needs_quoting("café"));
    }

    #[test]
    fn quoted_atoms_preserve_printable_unicode() {
        let mut out = String::new();
        write_atom(&mut out, "café\n");
        assert_eq!(out, "\"café\\n\"");
    }

    #[test]
    fn escape_policy_covers_quotes_backslash_newline_and_controls() {
        let mut out = String::new();
        write_atom(&mut out, "say \"hi\"\\\n\t");
        assert_eq!(out, r#""say \"hi\"\\\n\t""#);
    }

    #[test]
    fn depth_limit_marker_is_not_a_node_kind() {
        assert!(!NodeKind::ALL_KIND_NAMES.contains(&"depth_limit_exceeded"));
        assert_eq!(NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, "(depth_limit_exceeded)");
        let node = Node::new(NodeKind::Number { value: "1".to_string() }, loc());
        assert_ne!(node.to_sexp(), NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER);
        let mut rendered = String::new();
        let result = node.render_debug_sexp(&mut rendered, NativeDebugSexpLimits::unbounded());
        assert!(matches!(result, NativeDebugSexpResult::Complete { .. }));
        assert_eq!(rendered, "(number (value 1))");
    }
}
