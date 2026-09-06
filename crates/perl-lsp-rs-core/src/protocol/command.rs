//! Canonical LSP 3.18 `Command` construction for outbound protocol values.
//!
//! CodeLens historically owned a local serializable `Command` shape so
//! `Command.tooltip` could ship before the protocol-type substrate carried the
//! field. That shape is the current-main 3.18-compatible Command type: title,
//! identifier, optional tooltip, and optional arguments. New command producers
//! must go through [`Command::presented`] so tooltip policy cannot be omitted
//! by another ad hoc JSON object.
//!
//! CodeLens continues to construct this type with explicit struct literals so
//! its existing receipts stay byte-identical.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// LSP Command as serialized on the wire (LSP 3.18, including `tooltip`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    /// Title of the command (shown in UI).
    pub title: String,
    /// The identifier of the command to execute.
    pub command: String,
    /// Plain text tooltip shown by clients that support LSP 3.18 command tooltips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Arguments to the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<Value>>,
}

impl Command {
    /// Construct a user-presented command with a required plain-text tooltip.
    ///
    /// The tooltip is additional hover text; it must not replace `title` or
    /// `command`. Callers keep argument order and values.
    pub fn presented(
        title: impl Into<String>,
        command: impl Into<String>,
        tooltip: impl Into<String>,
        arguments: Option<Vec<Value>>,
    ) -> Self {
        Self {
            title: title.into(),
            command: command.into(),
            tooltip: Some(tooltip.into()),
            arguments,
        }
    }
}

/// `CodeActionOptions.documentation` entries advertised when the client
/// supports `textDocument.codeAction.documentationSupport`.
///
/// Runtime initialize and the effective-surface projection share this
/// producer so tooltip policy cannot drift between the two writers.
pub fn code_action_documentation_entries() -> Value {
    json!([
        {
            "kind": "quickfix",
            "command": Command::presented(
                "Explain Perl quick fixes",
                "perl.explainProviderDecision",
                "Show why Perl quick-fix code actions are offered",
                Some(vec![json!({
                    "provider": "diagnostics",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_quickfix"
                })]),
            )
        },
        {
            "kind": "refactor",
            "command": Command::presented(
                "Explain Perl refactors",
                "perl.explainProviderDecision",
                "Show why Perl refactor code actions are offered",
                Some(vec![json!({
                    "provider": "rename",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_refactor"
                })]),
            )
        },
        {
            "kind": "source.fixAll",
            "command": Command::presented(
                "Explain Perl fix-all actions",
                "perl.explainProviderDecision",
                "Show why Perl source.fixAll actions are offered",
                Some(vec![json!({
                    "provider": "diagnostics",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_fix_all"
                })]),
            )
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::Command;
    use serde_json::{Value, json};

    fn serialized(command: &Command) -> Value {
        serde_json::to_value(command).unwrap_or(Value::Null)
    }

    #[test]
    fn presented_serializes_tooltip_without_replacing_identity() {
        let arguments = Some(vec![json!({"uri": "file:///a.pl"})]);
        let command = Command::presented(
            "Generate test",
            "perl.generateTest",
            "Insert a Test::More skeleton for this subroutine",
            arguments.clone(),
        );

        let value = serialized(&command);
        assert_eq!(value["title"], "Generate test");
        assert_eq!(value["command"], "perl.generateTest");
        assert_eq!(value["arguments"], json!([{"uri": "file:///a.pl"}]));
        assert_eq!(value["tooltip"], "Insert a Test::More skeleton for this subroutine");
        assert_ne!(
            value["title"].as_str(),
            value["tooltip"].as_str(),
            "tooltip must add information beyond the title"
        );
        assert_eq!(command.arguments, arguments);
    }

    #[test]
    fn omitted_optional_fields_are_absent_on_the_wire() {
        let command = Command {
            title: "Run".to_string(),
            command: "perl.runFixer".to_string(),
            tooltip: None,
            arguments: None,
        };
        let value = serialized(&command);
        assert!(value.get("tooltip").is_none());
        assert!(value.get("arguments").is_none());
        assert_eq!(value["title"], "Run");
        assert_eq!(value["command"], "perl.runFixer");
    }

    #[test]
    fn presented_keeps_argument_order() {
        let command = Command::presented(
            "Explain this diagnostic",
            "perl-lsp.explainDiagnostic",
            "Show why this diagnostic was produced and its claim boundary",
            Some(vec![json!(1), json!("keep"), json!({"k": true})]),
        );
        assert_eq!(serialized(&command)["arguments"], json!([1, "keep", {"k": true}]));
    }

    #[test]
    fn code_action_documentation_commands_carry_tooltip_without_replacing_identity() {
        let docs = super::code_action_documentation_entries();
        let Some(entries) = docs.as_array() else {
            panic!("documentation entries must serialize as an array");
        };
        assert_eq!(entries.len(), 3);
        for (kind, title, tooltip, provider, scenario) in [
            (
                "quickfix",
                "Explain Perl quick fixes",
                "Show why Perl quick-fix code actions are offered",
                "diagnostics",
                "lsp_318_code_action_documentation_quickfix",
            ),
            (
                "refactor",
                "Explain Perl refactors",
                "Show why Perl refactor code actions are offered",
                "rename",
                "lsp_318_code_action_documentation_refactor",
            ),
            (
                "source.fixAll",
                "Explain Perl fix-all actions",
                "Show why Perl source.fixAll actions are offered",
                "diagnostics",
                "lsp_318_code_action_documentation_fix_all",
            ),
        ] {
            let Some(entry) = entries
                .iter()
                .find(|entry| entry.get("kind").and_then(Value::as_str) == Some(kind))
            else {
                panic!("missing documentation kind {kind}");
            };
            let command = entry.get("command").unwrap_or(&Value::Null);
            assert_eq!(command.get("title").and_then(Value::as_str), Some(title));
            assert_eq!(
                command.get("command").and_then(Value::as_str),
                Some("perl.explainProviderDecision")
            );
            assert_eq!(command.get("tooltip").and_then(Value::as_str), Some(tooltip));
            assert_ne!(title, tooltip);
            assert_eq!(
                command.pointer("/arguments/0/provider").and_then(Value::as_str),
                Some(provider)
            );
            assert_eq!(
                command.pointer("/arguments/0/scenario").and_then(Value::as_str),
                Some(scenario)
            );
            assert_eq!(
                command.pointer("/arguments/0/receipt_id").and_then(Value::as_str),
                Some(
                    "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation"
                )
            );
        }
    }
}
