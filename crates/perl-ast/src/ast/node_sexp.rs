//! Native debug S-expression projection for [`Node`].
//!
//! This is a non-normative human/debug rendering. It is **not** Tree-sitter
//! compatibility, typed machine output, AST equality, or source fidelity.
//! Compatibility CST work belongs on issue 8047. Bounded iterative rendering
//! belongs on issue 8832. This module still uses the existing call-stack depth
//! guard and may return [`NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER`].
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

use super::{FieldId, GotoTargetForm, MAX_AST_DEPTH, Node, NodeKind, Token, TokenKind};
use std::cell::Cell;
use std::fmt::{self, Write as _};

/// Grammar identity for this native debug projection.
pub const NATIVE_DEBUG_SEXP_GRAMMAR: &str = "perl-ast-native-debug-sexp/v1";

/// Marker returned when the recursive depth guard fires.
///
/// This string is not a [`NodeKind`]. Issue 8832 owns typed truncation.
pub const NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER: &str = "(depth_limit_exceeded)";

thread_local! {
    static TO_SEXP_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct ToSexpDepthGuard;

impl Drop for ToSexpDepthGuard {
    fn drop(&mut self) {
        TO_SEXP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

impl Node {
    /// Render this node as a native debug S-expression.
    ///
    /// The result is a single-root, lossy projection for humans and snapshot
    /// tests. It is not a Tree-sitter compatibility CST (issue 8047), not a
    /// typed machine schema (issue 8044), and not AST equality (issue 7045).
    ///
    /// Child order follows the canonical #8424 visit table. Non-child payloads
    /// (keywords, attributes, operators, catch binders, recovery tokens) are
    /// renderer-local and nested under the owning root. Source-location
    /// payloads such as `name_span` and `body_span` are omitted on purpose.
    ///
    /// Deep trees may still return [`NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER`];
    /// that marker is compatibility debt for issue 8832, not a node kind.
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
        let depth = TO_SEXP_DEPTH.with(|d| {
            let v = d.get();
            d.set(v + 1);
            v
        });
        let _depth_guard = ToSexpDepthGuard;
        if depth >= MAX_AST_DEPTH {
            NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER.to_string()
        } else {
            let mut out = String::new();
            write_node(self, &mut out);
            out
        }
    }

    /// Render this node with the same projection as [`Self::to_sexp`].
    ///
    /// Historically this unwrapped program-child expression statements. That
    /// parent-context rewrite is gone: one node has one debug form.
    pub fn to_sexp_inner(&self) -> String {
        self.to_sexp()
    }
}

impl fmt::Display for Node {
    /// Formats as the native debug S-expression. See [`Node::to_sexp`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_sexp())
    }
}

fn write_node(node: &Node, out: &mut String) {
    out.push('(');
    write_atom(out, &node.kind.grammar_kind_name());
    write_payloads(&node.kind, out);
    write_children(node, out);
    out.push(')');
}

fn write_children(node: &Node, out: &mut String) {
    let mut catch_index = 0usize;
    node.for_each_child_with_field(|field, child| {
        out.push(' ');
        out.push('(');
        match field {
            Some(field) => write_atom(out, field.name()),
            None => write_atom(out, "child"),
        }
        if field == Some(FieldId::CATCH)
            && let NodeKind::Try { catch_blocks, .. } = &node.kind
        {
            if let Some((Some((name, _)), _)) = catch_blocks.get(catch_index) {
                out.push(' ');
                write_atom(out, name);
            }
            catch_index = catch_index.saturating_add(1);
        }
        out.push(' ');
        out.push_str(&child.to_sexp());
        out.push(')');
    });
}

fn write_payloads(kind: &NodeKind, out: &mut String) {
    match kind {
        NodeKind::Variable { sigil, name } => {
            write_named(out, "sigil", sigil);
            write_named(out, "name", name);
        }
        NodeKind::VariableDeclaration { declarator, attributes, .. }
        | NodeKind::VariableListDeclaration { declarator, attributes, .. } => {
            write_named(out, "declarator", declarator);
            write_named_list(out, "attributes", attributes);
        }
        NodeKind::VariableWithAttributes { attributes, .. } => {
            write_named_list(out, "attributes", attributes);
        }
        NodeKind::Assignment { op, .. }
        | NodeKind::Binary { op, .. }
        | NodeKind::Unary { op, .. } => {
            write_named(out, "op", op);
        }
        NodeKind::Number { value } | NodeKind::VString { value } => {
            write_named(out, "value", value);
        }
        NodeKind::String { value, interpolated } => {
            write_named(out, "value", value);
            write_flag(out, "interpolated", *interpolated);
        }
        NodeKind::Heredoc { delimiter, content, interpolated, indented, command, body_span: _ } => {
            write_named(out, "delimiter", delimiter);
            write_named(out, "content", content);
            write_flag(out, "interpolated", *interpolated);
            write_flag(out, "indented", *indented);
            write_flag(out, "command", *command);
        }
        NodeKind::Readline { filehandle } => {
            if let Some(filehandle) = filehandle {
                write_named(out, "filehandle", filehandle);
            }
        }
        NodeKind::Glob { pattern } => write_named(out, "pattern", pattern),
        NodeKind::Typeglob { name } => write_named(out, "name", name),
        NodeKind::If { keyword, .. } | NodeKind::While { keyword, .. } => {
            if let Some(keyword) = keyword {
                write_named(out, "keyword", keyword);
            }
        }
        NodeKind::LabeledStatement { label, .. } => write_named(out, "label", label),
        NodeKind::StatementModifier { modifier, .. } => write_named(out, "modifier", modifier),
        NodeKind::Subroutine { name, declarator, attributes, name_span: _, .. } => {
            if let Some(name) = name {
                write_named(out, "name", name);
            }
            if let Some(declarator) = declarator {
                write_named(out, "declarator", declarator);
            }
            write_named_list(out, "attributes", attributes);
        }
        NodeKind::NamedParameter { external_name, default_operator, required, .. } => {
            write_named(out, "external_name", external_name);
            if let Some(op) = default_operator {
                write_named(out, "default_operator", op);
            }
            write_flag(out, "required", *required);
        }
        NodeKind::Method { name, attributes, name_span: _, .. } => {
            write_named(out, "name", name);
            write_named_list(out, "attributes", attributes);
        }
        NodeKind::LoopControl { op, label } => {
            write_named(out, "op", op);
            if let Some(label) = label {
                write_named(out, "label", label);
            }
        }
        NodeKind::Goto { form, .. } => write_named(out, "form", goto_form_atom(form)),
        NodeKind::MethodCall { method, .. } | NodeKind::IndirectCall { method, .. } => {
            write_named(out, "method", method);
        }
        NodeKind::FunctionCall { name, .. } | NodeKind::AmperCall { name, .. } => {
            write_named(out, "name", name);
        }
        NodeKind::Regex { pattern, replacement, modifiers, has_embedded_code } => {
            write_named(out, "pattern", pattern);
            if let Some(replacement) = replacement {
                write_named(out, "replacement", replacement);
            }
            write_named(out, "modifiers", modifiers);
            write_flag(out, "has_embedded_code", *has_embedded_code);
        }
        NodeKind::Match { pattern, modifiers, has_embedded_code, negated, .. } => {
            write_named(out, "pattern", pattern);
            write_named(out, "modifiers", modifiers);
            write_flag(out, "has_embedded_code", *has_embedded_code);
            write_flag(out, "negated", *negated);
        }
        NodeKind::Substitution {
            pattern,
            replacement,
            modifiers,
            has_embedded_code,
            negated,
            ..
        } => {
            write_named(out, "pattern", pattern);
            write_named(out, "replacement", replacement);
            write_named(out, "modifiers", modifiers);
            write_flag(out, "has_embedded_code", *has_embedded_code);
            write_flag(out, "negated", *negated);
        }
        NodeKind::Transliteration { search, replace, modifiers, negated, .. } => {
            write_named(out, "search", search);
            write_named(out, "replace", replace);
            write_named(out, "modifiers", modifiers);
            write_flag(out, "negated", *negated);
        }
        NodeKind::Package { name, name_span: _, .. } => write_named(out, "name", name),
        NodeKind::Class { name, parents, name_span: _, .. } => {
            write_named(out, "name", name);
            write_named_list(out, "parents", parents);
        }
        NodeKind::Format { name, body, name_span: _, .. } => {
            write_named(out, "name", name);
            write_named(out, "body", body);
        }
        NodeKind::Use { module, args, has_filter_risk }
        | NodeKind::No { module, args, has_filter_risk } => {
            write_named(out, "module", module);
            write_named_list(out, "args", args);
            write_flag(out, "has_filter_risk", *has_filter_risk);
        }
        NodeKind::PhaseBlock { phase, phase_span: _, .. } => write_named(out, "phase", phase),
        NodeKind::DataSection { marker, body } => {
            write_named(out, "marker", marker);
            if let Some(body) = body {
                write_named(out, "body", body);
            }
        }
        NodeKind::Identifier { name } => write_named(out, "name", name),
        NodeKind::Error { message, expected, found, .. } => {
            write_named(out, "message", message);
            write_expected_tokens(out, expected);
            if let Some(token) = found {
                write_found_token(out, token);
            }
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
        | NodeKind::UnknownRest => {}
    }
}

fn goto_form_atom(form: &GotoTargetForm) -> &'static str {
    match form {
        GotoTargetForm::Label => "label",
        GotoTargetForm::Sub => "sub",
        GotoTargetForm::Expr => "expr",
    }
}

fn write_named(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push('(');
    write_atom(out, name);
    out.push(' ');
    write_atom(out, value);
    out.push(')');
}

fn write_named_list(out: &mut String, name: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push(' ');
    out.push('(');
    write_atom(out, name);
    for value in values {
        out.push(' ');
        write_atom(out, value);
    }
    out.push(')');
}

fn write_flag(out: &mut String, name: &str, value: bool) {
    if value {
        write_named(out, name, "true");
    }
}

fn write_expected_tokens(out: &mut String, expected: &[TokenKind]) {
    if expected.is_empty() {
        return;
    }
    out.push(' ');
    out.push('(');
    write_atom(out, "expected");
    for kind in expected {
        out.push(' ');
        write_atom(out, &format!("{kind:?}"));
    }
    out.push(')');
}

fn write_found_token(out: &mut String, token: &Token) {
    out.push(' ');
    out.push('(');
    write_atom(out, "found");
    out.push(' ');
    write_atom(out, &format!("{:?}", token.kind));
    out.push(' ');
    write_atom(out, token.text.as_ref());
    out.push(')');
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
    use super::{NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, needs_quoting, write_atom};
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
    }
}
