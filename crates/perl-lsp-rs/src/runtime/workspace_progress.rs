//! Workspace indexing progress and readiness notifications.

#[cfg(feature = "workspace")]
use super::outbound::OutboundSink;
#[cfg(feature = "workspace")]
use super::types::ServerRequestId;
#[cfg(feature = "workspace")]
use perl_parser::workspace_index::IndexState;
#[cfg(feature = "workspace")]
use serde_json::json;

#[cfg(feature = "workspace")]
pub(crate) const WORKSPACE_INDEX_PROGRESS_TOKEN: &str = "workspace-index";

#[cfg(feature = "workspace")]
pub(super) fn send_index_ready_notification(outbound: &dyn OutboundSink, state: &IndexState) {
    let payload = index_readiness_payload(state);
    if let Err(e) = outbound.send_notification("perl-lsp/index-ready", payload) {
        tracing::warn!(error = %e, "Failed to send index-ready notification");
    }
}

#[cfg(feature = "workspace")]
fn index_readiness_payload(state: &IndexState) -> serde_json::Value {
    let (ready, state_name, reason) = match state {
        IndexState::Building { .. } => (false, "building", None),
        IndexState::Ready { .. } => (true, "ready", None),
        IndexState::Degraded { reason, .. } => {
            (false, "ready_limited", Some(format!("{reason:?}")))
        }
        // Forward-compatible fallback for future variants (#2898)
        _ => (false, "unknown", None),
    };

    let mut payload = json!({
        "ready": ready,
        "state": state_name,
    });
    if let Some(reason) = reason {
        payload["reason"] = json!(reason);
    }
    payload
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use super::index_readiness_payload;
    use perl_parser::workspace_index::{DegradationReason, IndexState, ResourceKind};
    use std::time::Instant;

    #[test]
    fn degraded_index_readiness_is_transportable_as_limited() -> Result<(), String> {
        let payload = index_readiness_payload(&IndexState::Degraded {
            reason: DegradationReason::ResourceLimit { kind: ResourceKind::MaxFiles },
            available_symbols: 12,
            since: Instant::now(),
        });

        if payload.get("ready").and_then(|value| value.as_bool()) != Some(false) {
            return Err(format!("unexpected ready flag: {payload}"));
        }
        if payload.get("state").and_then(|value| value.as_str()) != Some("ready_limited") {
            return Err(format!("unexpected readiness state: {payload}"));
        }
        if payload.get("reason").and_then(|value| value.as_str())
            != Some("ResourceLimit { kind: MaxFiles }")
        {
            return Err(format!("unexpected readiness reason: {payload}"));
        }

        Ok(())
    }
}

#[cfg(feature = "workspace")]
pub(super) fn send_active_document_ready_notification(
    outbound: &dyn OutboundSink,
    uri: &str,
    generation: u64,
) {
    if let Err(e) = outbound.send_notification(
        "perl-lsp/active-document-ready",
        json!({ "uri": uri, "generation": generation }),
    ) {
        tracing::warn!(error = %e, "Failed to send active-document-ready notification");
    }
}

#[cfg(feature = "workspace")]
pub(super) fn send_progress_create(outbound: &dyn OutboundSink, request_id: ServerRequestId) {
    if let Err(e) = outbound.send_request(
        request_id,
        "window/workDoneProgress/create",
        json!({ "token": WORKSPACE_INDEX_PROGRESS_TOKEN }),
    ) {
        tracing::warn!(error = %e, "Failed to send workDoneProgress/create");
    }
}

#[cfg(feature = "workspace")]
pub(super) fn send_progress_begin(outbound: &dyn OutboundSink) {
    if let Err(e) = outbound.send_notification(
        "$/progress",
        json!({
            "token": WORKSPACE_INDEX_PROGRESS_TOKEN,
            "value": {
                "kind": "begin",
                "title": "Indexing workspace",
                "cancellable": true,
                "percentage": 0
            }
        }),
    ) {
        tracing::warn!(error = %e, "Failed to send progress begin");
    }
}

#[cfg(feature = "workspace")]
pub(super) fn send_progress_report(outbound: &dyn OutboundSink, indexed: usize, total: usize) {
    let percentage = (indexed * 100).checked_div(total).unwrap_or(0).min(99) as u32;
    let message = format!("Indexed {} of {} files", indexed, total);
    if let Err(e) = outbound.send_notification(
        "$/progress",
        json!({
            "token": WORKSPACE_INDEX_PROGRESS_TOKEN,
            "value": {
                "kind": "report",
                "message": message,
                "percentage": percentage
            }
        }),
    ) {
        tracing::warn!(error = %e, "Failed to send progress report");
    }
}

#[cfg(feature = "workspace")]
pub(super) fn send_progress_end(outbound: &dyn OutboundSink, message: &str) {
    if let Err(e) = outbound.send_notification(
        "$/progress",
        json!({
            "token": WORKSPACE_INDEX_PROGRESS_TOKEN,
            "value": {
                "kind": "end",
                "message": message
            }
        }),
    ) {
        tracing::warn!(error = %e, "Failed to send progress end");
    }
}
