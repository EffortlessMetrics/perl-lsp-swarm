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
use serde_json::Value;

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
}
