//! Conservative output-builtin parameter-role projection.
//!
//! The legacy parameter-hint provider derives builtin labels from display
//! signatures. That is useful for ordinary builtins, but it is not sufficient
//! authority for Perl's `print`/`say`/`printf` call forms. This compatibility
//! layer removes those unproven labels and projects only roles that the current
//! AST preserves exactly. Canonical call facts tracked by
//! EffortlessMetrics/perl-lsp#10130 and #10138 replace this bounded layer.

use std::collections::HashSet;

use perl_parser_core::ast::{Node, NodeKind};
use perl_position_tracking::{WirePosition as Position, WireRange as Range};
use perl_semantic_analyzer::declaration::get_node_children;
use serde_json::{Value, json};

use super::impl_inlay_hints::{self, InlayHint, InlayHintKind};

/// Inlay hints provider using conservative output-builtin argument roles.
pub struct InlayHintsProvider;

impl InlayHintsProvider {
    /// Create a new inlay hints provider.
    pub fn new() -> Self {
        Self
    }

    /// Generate parameter and trivial type hints for the given AST.
    pub fn generate_hints(
        &self,
        ast: &Node,
        to_pos16: &impl Fn(usize) -> (u32, u32),
        range: Option<Range>,
    ) -> Vec<InlayHint> {
        let mut hints = self.parameter_hints(ast, to_pos16, range);
        hints.extend(self.trivial_type_hints(ast, to_pos16, range));
        hints
    }

    /// Generate parameter hints.
    pub fn parameter_hints(
        &self,
        ast: &Node,
        to_pos16: &impl Fn(usize) -> (u32, u32),
        range: Option<Range>,
    ) -> Vec<InlayHint> {
        parameter_hints(ast, to_pos16, range)
            .into_iter()
            .filter_map(json_hint_to_model)
            .collect()
    }

    /// Generate trivial type hints.
    pub fn trivial_type_hints(
        &self,
        ast: &Node,
        to_pos16: &impl Fn(usize) -> (u32, u32),
        range: Option<Range>,
    ) -> Vec<InlayHint> {
        impl_inlay_hints::trivial_type_hints(ast, to_pos16, range)
            .into_iter()
            .filter_map(json_hint_to_model)
            .collect()
    }
}

impl Default for InlayHintsProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn json_hint_to_model(value: Value) -> Option<InlayHint> {
    let position = value.get("position")?;
    let label = impl_inlay_hints::inlay_hint_label_str(&value)?.to_string();
    let kind = match value.get("kind").and_then(Value::as_u64).unwrap_or(1) {
        2 => InlayHintKind::Parameter,
        _ => InlayHintKind::Type,
    };
    let tooltip = value.get("tooltip").and_then(Value::as_str).map(str::to_string);

    Some(InlayHint {
        position: Position::new(
            position.get("line")?.as_u64()? as u32,
            position.get("character")?.as_u64()? as u32,
        ),
        label,
        kind,
        padding_left: value.get("paddingLeft").and_then(Value::as_bool).unwrap_or(false),
        padding_right: value.get("paddingRight").and_then(Value::as_bool).unwrap_or(false),
        tooltip,
        location: None,
    })
}

/// Generate parameter hints using conservative output-builtin role handling.
pub fn parameter_hints(
    ast: &Node,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    range: Option<Range>,
) -> Vec<Value> {
    parameter_hints_with_resolver(ast, to_pos16, range, None)
}

/// Generate parameter hints with optional workspace method resolution.
///
/// Non-output calls remain delegated to the existing parameter-hint provider.
/// For `print`, `say`, and `printf`, legacy display-signature labels are removed
/// from ordinary `FunctionCall` arguments. Exact roles are then projected from
/// parser-preserved `IndirectCall` operands. An ordinary `printf` string literal
/// is admitted as an exact format operand; other flattened forms fail closed.
pub fn parameter_hints_with_resolver(
    ast: &Node,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    range: Option<Range>,
    method_resolver: Option<&dyn Fn(&str) -> Option<Vec<String>>>,
) -> Vec<Value> {
    let mut output_function_args = HashSet::new();
    let mut user_output_subs = HashSet::new();

    walk_ast(ast, &mut |node| match &node.kind {
        NodeKind::FunctionCall { name, args } if output_builtin(name).is_some() => {
            for arg in args {
                let (line, character) = to_pos16(arg.location.start);
                output_function_args.insert((line, character, name.clone()));
            }
        }
        NodeKind::Subroutine { name: Some(name), .. } if output_builtin(name).is_some() => {
            user_output_subs.insert(name.clone());
        }
        _ => {}
    });

    let mut hints = impl_inlay_hints::parameter_hints_with_resolver(
        ast,
        to_pos16,
        range,
        method_resolver,
    );

    // Remove only legacy hints attached to output-builtin FunctionCall
    // arguments. Same-named method hints occupy their own source positions and
    // remain intact.
    hints.retain(|hint| {
        let Some(function_name) = hint
            .get("data")
            .and_then(|data| data.get("functionName"))
            .and_then(Value::as_str)
        else {
            return true;
        };
        if output_builtin(function_name).is_none() {
            return true;
        }
        let Some(position) = hint.get("position") else {
            return true;
        };
        let Some(line) = position.get("line").and_then(Value::as_u64) else {
            return true;
        };
        let Some(character) = position.get("character").and_then(Value::as_u64) else {
            return true;
        };

        !output_function_args.contains(&(line as u32, character as u32, function_name.to_string()))
    });

    walk_ast(ast, &mut |node| match &node.kind {
        NodeKind::IndirectCall { method, object, args } => {
            let Some(kind) = output_builtin(method) else {
                return;
            };
            if !is_core_qualified(method) && user_output_subs.contains(method) {
                return;
            }

            push_role_hint(
                &mut hints,
                method,
                "filehandle",
                0,
                object,
                to_pos16,
                range,
            );
            if kind == OutputBuiltin::Printf
                && let Some(format) = args.first()
            {
                push_role_hint(
                    &mut hints,
                    method,
                    "format",
                    1,
                    format,
                    to_pos16,
                    range,
                );
            }
        }
        NodeKind::FunctionCall { name, args } => {
            if output_builtin(name) != Some(OutputBuiltin::Printf) {
                return;
            }
            if !is_core_qualified(name) && user_output_subs.contains(name) {
                return;
            }
            let Some(format) = args.first() else {
                return;
            };
            if !matches!(&format.kind, NodeKind::String { .. }) {
                return;
            }

            push_role_hint(&mut hints, name, "format", 0, format, to_pos16, range);
        }
        _ => {}
    });

    hints.sort_by_key(hint_sort_key);
    hints
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputBuiltin {
    Print,
    Printf,
    Say,
}

fn output_builtin(name: &str) -> Option<OutputBuiltin> {
    match name.strip_prefix("CORE::").unwrap_or(name) {
        "print" => Some(OutputBuiltin::Print),
        "printf" => Some(OutputBuiltin::Printf),
        "say" => Some(OutputBuiltin::Say),
        _ => None,
    }
}

fn is_core_qualified(name: &str) -> bool {
    name.starts_with("CORE::")
}

fn push_role_hint(
    hints: &mut Vec<Value>,
    function_name: &str,
    role: &str,
    param_index: usize,
    subject: &Node,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    range: Option<Range>,
) {
    let (line, character) = to_pos16(subject.location.start);
    let position = Position::new(line, character);
    if range.is_some_and(|filter| !pos_in_range(position, filter)) {
        return;
    }

    hints.push(json!({
        "position": { "line": line, "character": character },
        "label": [{ "value": format!("{role}:") }],
        "kind": 2,
        "paddingLeft": false,
        "paddingRight": true,
        "data": {
            "functionName": function_name,
            "paramIndex": param_index,
            "outputBuiltinRole": role,
            "compatibilityOwner": "EffortlessMetrics/perl-lsp-swarm#18132"
        }
    }));
}

fn pos_in_range(position: Position, range: Range) -> bool {
    if position.line < range.start.line || position.line > range.end.line {
        return false;
    }
    if position.line == range.start.line && position.character < range.start.character {
        return false;
    }
    if position.line == range.end.line && position.character >= range.end.character {
        return false;
    }
    true
}

fn hint_sort_key(hint: &Value) -> (u64, u64, String) {
    let line = hint
        .get("position")
        .and_then(|position| position.get("line"))
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let character = hint
        .get("position")
        .and_then(|position| position.get("character"))
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let label = impl_inlay_hints::inlay_hint_label_str(hint).unwrap_or_default().to_string();
    (line, character, label)
}

fn walk_ast(node: &Node, visitor: &mut impl FnMut(&Node)) {
    visitor(node);
    for child in get_node_children(node) {
        walk_ast(child, visitor);
    }
}
