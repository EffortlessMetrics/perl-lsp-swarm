//! Capability-complete edit delivery selection (#8954).
//!
//! Separates computed edit planning from how the client can consume the edit.

use crate::state::ClientCapabilities;
use serde_json::Value;

/// Externally visible edit delivery disposition for one validated edit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditDeliveryDisposition {
    ReturnInlineWorkspaceEdit,
    RequestClientApplyEdit,
    PreviewOnly,
    RefusedUnsupportedClient,
    ApplyEditSendFailed,
}

impl EditDeliveryDisposition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ReturnInlineWorkspaceEdit => "return_inline_workspace_edit",
            Self::RequestClientApplyEdit => "request_client_apply_edit",
            Self::PreviewOnly => "preview_only",
            Self::RefusedUnsupportedClient => "refused_unsupported_client",
            Self::ApplyEditSendFailed => "apply_edit_send_failed",
        }
    }
}

/// Where the edit is offered in the LSP surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditDeliverySurface {
    /// Response types that carry `WorkspaceEdit` directly (code action, rename, …).
    InlineResponse,
    /// `workspace/executeCommand` paths that require client-side mutation.
    ExecuteCommandMutation,
    /// Product contract forbids live mutation (preview commands).
    PreviewOnlyContract,
}

/// Choose exactly one delivery disposition from client capabilities and surface.
pub(crate) fn select_edit_delivery(
    surface: EditDeliverySurface,
    caps: &ClientCapabilities,
    edit: &Value,
    has_nonempty_edit: bool,
) -> EditDeliveryDisposition {
    if matches!(surface, EditDeliverySurface::PreviewOnlyContract) {
        return EditDeliveryDisposition::PreviewOnly;
    }
    if !has_nonempty_edit {
        return EditDeliveryDisposition::PreviewOnly;
    }
    if !workspace_edit_representable_for_client(edit, caps) {
        return EditDeliveryDisposition::RefusedUnsupportedClient;
    }
    match surface {
        EditDeliverySurface::InlineResponse => EditDeliveryDisposition::ReturnInlineWorkspaceEdit,
        EditDeliverySurface::ExecuteCommandMutation => {
            if caps.workspace_apply_edit_support {
                EditDeliveryDisposition::RequestClientApplyEdit
            } else {
                EditDeliveryDisposition::RefusedUnsupportedClient
            }
        }
        EditDeliverySurface::PreviewOnlyContract => EditDeliveryDisposition::PreviewOnly,
    }
}

pub(crate) fn workspace_edit_representable_for_client(
    edit: &Value,
    caps: &ClientCapabilities,
) -> bool {
    if edit.get("documentChanges").is_some() && !caps.workspace_edit_document_changes_support {
        return false;
    }
    if workspace_edit_uses_snippet_text_edits(edit) && !caps.workspace_edit_snippet_edit_support {
        return false;
    }
    true
}

fn workspace_edit_uses_snippet_text_edits(edit: &Value) -> bool {
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        for edits in changes.values() {
            if edits.as_array().is_some_and(|items| items.iter().any(snippet_text_edit_item)) {
                return true;
            }
        }
    }
    if let Some(doc_changes) = edit.get("documentChanges").and_then(Value::as_array) {
        for change in doc_changes {
            if change.get("edits").is_some_and(|edits| {
                edits.as_array().is_some_and(|items| items.iter().any(snippet_text_edit_item))
            }) {
                return true;
            }
        }
    }
    false
}

fn snippet_text_edit_item(item: &Value) -> bool {
    item.get("snippet").is_some()
}

pub(crate) fn execute_command_mutation_refusal_message() -> &'static str {
    "This editor profile does not support server-initiated apply-edit for execute-command \
     mutations, and this command does not define an inline edit application contract. \
     No source changes were applied."
}

pub(crate) fn execute_command_mutation_send_failure_message() -> &'static str {
    "The server could not send the workspace/applyEdit request to this editor. \
     No source changes were applied."
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn caps_with(apply_edit: bool, metadata: bool, document_changes: bool) -> ClientCapabilities {
        ClientCapabilities {
            workspace_apply_edit_support: apply_edit,
            workspace_edit_metadata_support: metadata,
            workspace_edit_document_changes_support: document_changes,
            ..ClientCapabilities::default()
        }
    }

    #[test]
    fn matrix_apply_edit_with_metadata_requests_client_apply() {
        let edit = json!({"changes": {"file:///a.pl": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": ""}]}});
        let caps = caps_with(true, true, false);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::ExecuteCommandMutation, &caps, &edit, true),
            EditDeliveryDisposition::RequestClientApplyEdit
        );
    }

    #[test]
    fn matrix_apply_edit_without_metadata_still_requests_client_apply() {
        let edit = json!({"changes": {"file:///a.pl": []}});
        let caps = caps_with(true, false, false);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::ExecuteCommandMutation, &caps, &edit, true),
            EditDeliveryDisposition::RequestClientApplyEdit
        );
    }

    #[test]
    fn matrix_no_apply_edit_execute_command_refused() {
        let edit = json!({"changes": {"file:///a.pl": []}});
        let caps = caps_with(false, false, false);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::ExecuteCommandMutation, &caps, &edit, true),
            EditDeliveryDisposition::RefusedUnsupportedClient
        );
    }

    #[test]
    fn matrix_no_apply_edit_inline_still_returns_edit() {
        let edit = json!({"changes": {"file:///a.pl": []}});
        let caps = caps_with(false, false, false);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::InlineResponse, &caps, &edit, true),
            EditDeliveryDisposition::ReturnInlineWorkspaceEdit
        );
    }

    #[test]
    fn matrix_document_changes_required_but_unsupported_refused() {
        let edit = json!({"documentChanges": []});
        let caps = caps_with(true, true, false);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::ExecuteCommandMutation, &caps, &edit, true),
            EditDeliveryDisposition::RefusedUnsupportedClient
        );
    }

    #[test]
    fn matrix_preview_only_surface() {
        let edit = json!({"changes": {"file:///a.pl": []}});
        let caps = caps_with(true, true, true);
        assert_eq!(
            select_edit_delivery(EditDeliverySurface::PreviewOnlyContract, &caps, &edit, true),
            EditDeliveryDisposition::PreviewOnly
        );
    }
}
