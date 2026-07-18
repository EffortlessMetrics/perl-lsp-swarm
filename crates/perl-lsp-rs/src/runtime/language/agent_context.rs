//! Read-only orientation for LSP clients and agents.

use super::super::{JsonRpcError, LspServer};
use serde_json::{Value, json};

const AGENT_CONTEXT_SCHEMA_VERSION: &str = "agent_context.v1";

impl LspServer {
    /// Return a compact, read-only orientation envelope for an LSP client.
    pub(crate) fn agent_context(
        &self,
        argument: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let Some(workspace_trust_report) = self.workspace_trust_report(argument)? else {
            return Ok(None);
        };
        let advertised_feature_ids = self.advertised_feature_ids.lock().clone();

        Ok(Some(json!({
            "schema_version": AGENT_CONTEXT_SCHEMA_VERSION,
            "command": "perl.agentContext",
            "user_message": "Perl LSP agent context assembled from current server state.",
            "claim_boundary": "This orientation envelope references existing runtime state and advertised commands only. It does not scan files, probe Perl, run perldoc, launch DAP, apply edits, or execute follow-up commands.",
            "request": {
                "method": "workspace/executeCommand",
                "arguments_required": true,
                "arguments_shape": "[optional client_runtime_state object]"
            },
            "workspace_trust_report": workspace_trust_report,
            "advertised_feature_ids": advertised_feature_ids,
            "execute_commands": crate::execute_command::get_supported_commands(),
            "next_actions": [
                {
                    "id": "apply_setup_hints",
                    "source": "workspace_trust_report.setup_hints.hints",
                    "description": "Review advisory setup hints and apply configuration changes only when appropriate."
                },
                {
                    "id": "explain_provider_decision",
                    "command": "perl.explainProviderDecision",
                    "description": "Request a structured explanation when a provider result, fallback, or edit decision needs context."
                },
                {
                    "id": "explain_missing_module_lookup",
                    "command": "perl.explainMissingModuleLookup",
                    "description": "Request a bounded module-resolution explanation when an import cannot be resolved."
                },
                {
                    "id": "preview_before_edit",
                    "commands": ["perl.previewSafeDelete", "perl.previewPackageRename"],
                    "description": "Preview supported workspace edits before asking a client to apply them."
                }
            ]
        })))
    }
}
