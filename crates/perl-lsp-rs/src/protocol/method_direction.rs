//! Checked method-direction authority for LSP methods (#8896).
//!
//! This module is the single classification table answering, for every method
//! this server knows: which envelope kind carries it (request | notification),
//! which protocol direction it belongs to (client→server | server→client),
//! what lifecycle phase constrains it, and whether it is standard LSP or a
//! perl-lsp project extension.
//!
//! Consumers:
//!
//! - inbound routing (`runtime/dispatch/routing.rs`) admits only
//!   client→server methods to application handlers after #7010 has classified
//!   the envelope;
//! - outbound construction (`runtime/outbound.rs`) refuses to emit frames whose
//!   method is positively classified client→server;
//! - tests mechanically check both executable inventories against this table.
//!
//! # Migration path toward schema authority
//!
//! This is a reviewed local table, not the final authority. It must be kept
//! mechanically synchronized with the executable route and outbound inventories
//! (see the inventory tests below) until #7113/#7116 generate method metadata
//! from the protocol schema. When that lands, this table becomes a projection
//! of the schema authority and the inventory checks become schema receipts.

/// Direction of a JSON-RPC method relative to this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodDirection {
    /// The client sends this method to the server.
    ClientToServer,
    /// The server sends this method to the client; client-originated traffic
    /// with this name never reaches application handlers (#8896).
    ServerToClient,
}

/// Envelope kind that legitimately carries the method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvelopeKind {
    /// JSON-RPC request (has an id, expects a response).
    Request,
    /// JSON-RPC notification (no id, no response).
    Notification,
}

/// Lifecycle phase constraint recorded for the method.
///
/// Admission does not newly enforce lifecycle here (`routing.rs` owns the
/// existing gates); the field records the reviewed constraint so later work
/// (#1403 lifecycle redesign, #7116 schema receipts) consumes one authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecyclePhase {
    /// Part of the initialize/shutdown/exit handshake itself.
    LifecycleHandshake,
    /// Legal once initialization completed; rejected with
    /// ServerNotInitialized before that by the existing route gate.
    RequiresInitialized,
    /// Processed at any phase (e.g. cancellation is handled in preflight).
    Anytime,
}

/// Whether the method comes from standard LSP or from this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodOrigin {
    /// Standard LSP method.
    Standard,
    /// perl-lsp extension method (`perl/…`, `perl-lsp/…`, `$/perl-lsp/…`,
    /// `experimental/…`, test-only endpoints).
    ProjectExtension,
}

/// One reviewed registry row.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MethodDescriptor {
    /// Wire method name.
    pub(crate) method: &'static str,
    /// Envelope kind that legitimately carries the method.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "recorded for #7116 schema receipts and #8068 provider projection; \
                      admission currently consumes only `direction`"
        )
    )]
    pub(crate) kind: EnvelopeKind,
    /// Protocol direction of the method.
    pub(crate) direction: MethodDirection,
    /// Lifecycle phase constraint.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "lifecycle redesign input for #1403; recorded now so the table \
                      carries one reviewed constraint per method"
        )
    )]
    pub(crate) lifecycle: LifecyclePhase,
    /// Standard vs project extension.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "standard-vs-extension split feeds later authority projection (#7113)"
        )
    )]
    pub(crate) origin: MethodOrigin,
}

const fn c2s(
    method: &'static str,
    kind: EnvelopeKind,
    lifecycle: LifecyclePhase,
) -> MethodDescriptor {
    MethodDescriptor {
        method,
        kind,
        direction: MethodDirection::ClientToServer,
        lifecycle,
        origin: MethodOrigin::Standard,
    }
}

const fn s2c(method: &'static str, kind: EnvelopeKind) -> MethodDescriptor {
    MethodDescriptor {
        method,
        kind,
        direction: MethodDirection::ServerToClient,
        lifecycle: LifecyclePhase::RequiresInitialized,
        origin: MethodOrigin::Standard,
    }
}

const fn ext(
    method: &'static str,
    kind: EnvelopeKind,
    direction: MethodDirection,
    lifecycle: LifecyclePhase,
) -> MethodDescriptor {
    MethodDescriptor { method, kind, direction, lifecycle, origin: MethodOrigin::ProjectExtension }
}

/// The reviewed local method-direction table (#8896).
///
/// Classification source: current LSP 3.18 standard method directions plus the
/// executable route table (`runtime/dispatch/routing.rs`) and outbound send
/// sites. The inventory tests below fail when either executable surface gains
/// or loses a method without this table being updated.
pub(crate) const REGISTRY: &[MethodDescriptor] = &[
    // ── Lifecycle handshake (client→server) ────────────────────────────────
    c2s("initialize", EnvelopeKind::Request, LifecyclePhase::LifecycleHandshake),
    c2s("initialized", EnvelopeKind::Notification, LifecyclePhase::LifecycleHandshake),
    c2s("shutdown", EnvelopeKind::Request, LifecyclePhase::LifecycleHandshake),
    c2s("exit", EnvelopeKind::Notification, LifecyclePhase::LifecycleHandshake),
    // ── Document sync (client→server) ──────────────────────────────────────
    c2s("textDocument/didOpen", EnvelopeKind::Notification, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/didChange", EnvelopeKind::Notification, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/didClose", EnvelopeKind::Notification, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/didSave", EnvelopeKind::Notification, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/willSave", EnvelopeKind::Notification, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/willSaveWaitUntil",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    // ── Notebook sync (client→server) ──────────────────────────────────────
    c2s(
        "notebookDocument/didOpen",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "notebookDocument/didChange",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "notebookDocument/didSave",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "notebookDocument/didClose",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    // ── Language features (client→server requests) ─────────────────────────
    c2s("textDocument/completion", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("completionItem/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/hover", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/signatureHelp", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/definition", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/declaration", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/typeDefinition", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/implementation", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/references", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/documentHighlight",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("textDocument/documentSymbol", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/codeAction", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("codeAction/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/codeLens", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("codeLens/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/formatting", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/rangeFormatting", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/rangesFormatting",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "textDocument/onTypeFormatting",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("textDocument/prepareRename", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/rename", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/linkedEditingRange",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "textDocument/prepareTypeHierarchy",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("typeHierarchy/prepare", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("typeHierarchy/supertypes", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("typeHierarchy/subtypes", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/prepareCallHierarchy",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("callHierarchy/incomingCalls", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("callHierarchy/outgoingCalls", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/inlineCompletion",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("textDocument/inlineValue", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/documentColor", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/colorPresentation",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("textDocument/moniker", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/diagnostic", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("workspace/diagnostic", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "textDocument/semanticTokens/full",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "textDocument/semanticTokens/range",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "textDocument/semanticTokens/full/delta",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("textDocument/inlayHint", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("inlayHint/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/documentLink", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("documentLink/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/foldingRange", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("textDocument/selectionRange", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    // ── Workspace features (client→server) ─────────────────────────────────
    c2s("workspace/symbol", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("workspace/symbol/resolve", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("workspace/executeCommand", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "workspace/textDocumentContent",
        EnvelopeKind::Request,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s("workspace/willCreateFiles", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("workspace/willRenameFiles", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s("workspace/willDeleteFiles", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
    c2s(
        "workspace/didCreateFiles",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "workspace/didDeleteFiles",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "workspace/didRenameFiles",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "workspace/didChangeWatchedFiles",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "workspace/didChangeConfiguration",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    c2s(
        "workspace/didChangeWorkspaceFolders",
        EnvelopeKind::Notification,
        LifecyclePhase::RequiresInitialized,
    ),
    // ── Window / protocol plumbing (client→server) ─────────────────────────
    c2s("window/workDoneProgress/cancel", EnvelopeKind::Notification, LifecyclePhase::Anytime),
    c2s("$/setTrace", EnvelopeKind::Notification, LifecyclePhase::Anytime),
    c2s("$/cancelRequest", EnvelopeKind::Notification, LifecyclePhase::Anytime),
    // ── Server→client requests (outbound only) ─────────────────────────────
    s2c("workspace/applyEdit", EnvelopeKind::Request),
    s2c("workspace/configuration", EnvelopeKind::Request),
    s2c("client/registerCapability", EnvelopeKind::Request),
    s2c("client/unregisterCapability", EnvelopeKind::Request),
    s2c("window/showMessageRequest", EnvelopeKind::Request),
    s2c("window/showDocument", EnvelopeKind::Request),
    s2c("window/workDoneProgress/create", EnvelopeKind::Request),
    s2c("workspace/codeLens/refresh", EnvelopeKind::Request),
    s2c("workspace/semanticTokens/refresh", EnvelopeKind::Request),
    s2c("workspace/inlayHint/refresh", EnvelopeKind::Request),
    s2c("workspace/inlineValue/refresh", EnvelopeKind::Request),
    s2c("workspace/diagnostic/refresh", EnvelopeKind::Request),
    s2c("workspace/foldingRange/refresh", EnvelopeKind::Request),
    s2c("workspace/textDocumentContent/refresh", EnvelopeKind::Request),
    // ── Server→client notifications (outbound only) ────────────────────────
    s2c("textDocument/publishDiagnostics", EnvelopeKind::Notification),
    s2c("window/showMessage", EnvelopeKind::Notification),
    s2c("window/logMessage", EnvelopeKind::Notification),
    s2c("$/logTrace", EnvelopeKind::Notification),
    s2c("$/progress", EnvelopeKind::Notification),
    s2c("telemetry/event", EnvelopeKind::Notification),
    // ── perl-lsp project extensions ────────────────────────────────────────
    ext(
        "$/perl-lsp/clientResponse",
        EnvelopeKind::Notification,
        MethodDirection::ClientToServer,
        LifecyclePhase::Anytime,
    ),
    ext(
        "$/perl-lsp/watchdog",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::Anytime,
    ),
    ext(
        "perl-lsp/index-ready",
        EnvelopeKind::Notification,
        MethodDirection::ServerToClient,
        LifecyclePhase::Anytime,
    ),
    ext(
        "perl-lsp/active-document-ready",
        EnvelopeKind::Notification,
        MethodDirection::ServerToClient,
        LifecyclePhase::Anytime,
    ),
    ext(
        "perl/showAst",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::RequiresInitialized,
    ),
    ext(
        "experimental/testDiscovery",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::RequiresInitialized,
    ),
    ext(
        "textDocument/perlInlineCompletionStream",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::RequiresInitialized,
    ),
    ext(
        "$/test/slowOperation",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::Anytime,
    ),
];

/// Look up the descriptor for a method, if classified.
pub(crate) fn lookup(method: &str) -> Option<&'static MethodDescriptor> {
    REGISTRY.iter().find(|descriptor| descriptor.method == method)
}

/// Inbound admission decision for a client-originated message (#8896).
///
/// Called after #7010 envelope classification, so every message reaching this
/// check carries a method name. Only positively classified server→client
/// methods are gated; unknown methods keep their normal routing behavior
/// (MethodNotFound for requests, silent tolerance for notifications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundDecision {
    /// Continue into ordinary method routing.
    Allow,
    /// Wrong-direction request: respond MethodNotFound (-32601).
    RejectRequest,
    /// Wrong-direction notification: drop silently, no state mutation.
    IgnoreNotification,
}

/// Decide admission for an inbound message that carries `method` and is a
/// request when `is_request` holds (it has an id), otherwise a notification.
pub(crate) fn inbound_decision(method: &str, is_request: bool) -> InboundDecision {
    match lookup(method) {
        Some(descriptor) if descriptor.direction == MethodDirection::ServerToClient => {
            if is_request {
                InboundDecision::RejectRequest
            } else {
                InboundDecision::IgnoreNotification
            }
        }
        _ => InboundDecision::Allow,
    }
}

/// Outbound admission for server-originated frames (#8896 §3).
///
/// Returns the rejection reason when `method` is positively classified as a
/// client→server method — such a frame can only be a reversed-direction bug.
/// Unknown names stay tolerated at this transport seam; new outbound literals
/// are forced into the registry by the outbound-inventory test instead.
pub(crate) fn outbound_rejection(method: &str) -> Option<&'static str> {
    match lookup(method) {
        Some(descriptor) if descriptor.direction == MethodDirection::ClientToServer => {
            Some("is registered client-to-server and may not be sent by the server")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn registry_method_names_are_unique() {
        let mut seen = BTreeSet::new();
        for descriptor in REGISTRY {
            assert!(
                seen.insert(descriptor.method),
                "duplicate registry entry for `{}`",
                descriptor.method
            );
        }
    }

    /// Lifecycle rows must match the gates routing actually applies today:
    /// only the handshake quartet participates in the initialize/shutdown
    /// gates, cancellation is handled anytime in preflight, and everything
    /// else sits behind the ServerNotInitialized gate (#1403 consumes this
    /// table when it redesigns those phases).
    #[test]
    fn lifecycle_rows_match_current_routing_gates() {
        let lifecycle = |method: &str| lookup(method).map(|descriptor| descriptor.lifecycle);
        assert_eq!(lifecycle("initialize"), Some(LifecyclePhase::LifecycleHandshake));
        assert_eq!(lifecycle("initialized"), Some(LifecyclePhase::LifecycleHandshake));
        assert_eq!(lifecycle("shutdown"), Some(LifecyclePhase::LifecycleHandshake));
        assert_eq!(lifecycle("exit"), Some(LifecyclePhase::LifecycleHandshake));
        assert_eq!(lifecycle("$/cancelRequest"), Some(LifecyclePhase::Anytime));
        assert_eq!(lifecycle("textDocument/hover"), Some(LifecyclePhase::RequiresInitialized));
        assert_eq!(lifecycle("workspace/applyEdit"), Some(LifecyclePhase::RequiresInitialized));
    }

    #[test]
    fn wrong_direction_standard_methods_are_classified_server_to_client() {
        let mut violations = Vec::new();
        for method in [
            "workspace/applyEdit",
            "workspace/configuration",
            "client/registerCapability",
            "client/unregisterCapability",
        ] {
            match lookup(method) {
                None => violations.push(format!("`{method}` must be classified")),
                Some(descriptor) => {
                    if descriptor.direction != MethodDirection::ServerToClient {
                        violations.push(format!("`{method}` direction must be server-to-client"));
                    }
                    if descriptor.kind != EnvelopeKind::Request {
                        violations.push(format!("`{method}` envelope kind must be request"));
                    }
                    if descriptor.origin != MethodOrigin::Standard {
                        violations.push(format!("`{method}` must be standard, not an extension"));
                    }
                }
            }
        }
        assert!(
            violations.is_empty(),
            "wrong-direction standard methods misclassified:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn inbound_decision_rejects_wrong_direction_requests_and_drops_notifications() {
        assert_eq!(inbound_decision("textDocument/hover", true), InboundDecision::Allow);
        assert_eq!(inbound_decision("$/perl-lsp/clientResponse", false), InboundDecision::Allow);
        assert_eq!(inbound_decision("workspace/applyEdit", true), InboundDecision::RejectRequest);
        assert_eq!(
            inbound_decision("workspace/configuration", true),
            InboundDecision::RejectRequest
        );
        assert_eq!(
            inbound_decision("client/registerCapability", true),
            InboundDecision::RejectRequest
        );
        assert_eq!(inbound_decision("$/progress", false), InboundDecision::IgnoreNotification);
        assert_eq!(
            inbound_decision("window/showMessage", false),
            InboundDecision::IgnoreNotification
        );
        assert_eq!(
            inbound_decision("workspace/applyEdit", false),
            InboundDecision::IgnoreNotification,
            "a wrong-direction notification must never reach application handlers \
             regardless of its malformed envelope"
        );
    }

    #[test]
    fn outbound_admission_blocks_only_positively_client_to_server_names() {
        assert!(outbound_rejection("initialize").is_some());
        assert!(outbound_rejection("textDocument/hover").is_some());
        assert!(outbound_rejection("textDocument/didOpen").is_some());
        assert!(outbound_rejection("workspace/applyEdit").is_none());
        assert!(outbound_rejection("workspace/configuration").is_none());
        assert!(outbound_rejection("client/registerCapability").is_none());
        assert!(
            outbound_rejection("slot/unregistered-test-method").is_none(),
            "unknown names stay tolerated at the transport seam; the inventory \
             test owns forcing new literals into the registry"
        );
    }

    // ── Mechanical inventory checks ────────────────────────────────────────
    //
    // The local table is acceptable only while it is mechanically checked
    // against the two executable surfaces (#8896 §1). Both scans below read
    // the live source of this crate; neither depends on a hand-maintained
    // list of method names.

    /// Remove trailing `#[cfg(test)] mod … { … }` regions so test-only string
    /// literals cannot satisfy or pollute production-inventory scans.
    fn strip_test_modules(source: &str) -> String {
        let mut result = String::with_capacity(source.len());
        let mut lines = source.lines().peekable();
        while let Some(line) = lines.next() {
            result.push_str(line);
            result.push('\n');
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            let Some(next) = lines.peek() else { break };
            let next_trimmed = next.trim_start();
            let is_mod_open = next_trimmed.starts_with("mod ")
                && (next_trimmed.contains('{') || next_trimmed.ends_with(';'));
            if !is_mod_open {
                continue;
            }
            // Consume the attribute/mod pair. A `mod x;` declaration ends
            // here; a `mod x {` block is skipped by brace counting.
            let Some(mod_line) = lines.next() else { break };
            if mod_line.trim_end().ends_with(';') {
                continue;
            }
            let mut depth =
                mod_line.matches('{').count() as i64 - mod_line.matches('}').count() as i64;
            while depth > 0 {
                match lines.next() {
                    Some(inner) => {
                        depth += inner.matches('{').count() as i64;
                        depth -= inner.matches('}').count() as i64;
                    }
                    None => break,
                }
            }
        }
        result
    }

    fn quoted_literals(line: &str) -> Vec<&str> {
        line.split('"')
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, part)| part)
            .collect()
    }

    fn is_method_like(literal: &str) -> bool {
        literal.len() > 2
            && literal.contains('/')
            && literal
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '$')
            && literal
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '$' | '.'))
    }

    /// Every method-shaped literal on a routing-table arm head must be
    /// classified client→server in the registry. Re-adding a server→client
    /// method as an inbound route (negative controls 1 and 6) fails here even
    /// before any behavior test runs.
    ///
    /// The formatting-policy cutover (`dispatch/formatting_policy.rs`)
    /// intercepts its methods after preflight and before the routing table,
    /// so its arms are scanned with the same rule — otherwise that seam could
    /// reintroduce a wrong-direction route without tripping this gate.
    #[test]
    fn route_inventory_matches_registry_directions() {
        let routing = include_str!("../runtime/dispatch/routing.rs");
        let formatting_policy = include_str!("../runtime/dispatch/formatting_policy.rs");
        let source =
            format!("{}\n{}", strip_test_modules(routing), strip_test_modules(formatting_policy));

        let mut routed_methods = BTreeSet::new();
        for line in source.lines() {
            let trimmed = line.trim_start();
            let is_arm_head = trimmed.starts_with('"') || trimmed.starts_with("| \"");
            if !is_arm_head {
                continue;
            }
            for literal in quoted_literals(line) {
                if is_method_like(literal) {
                    routed_methods.insert(literal.to_string());
                }
            }
        }

        assert!(
            routed_methods.len() >= 50,
            "the route scan stopped finding the routing table; inspect the \
             scanner before trusting green: {routed_methods:?}"
        );

        let mut violations = Vec::new();
        for method in &routed_methods {
            match lookup(method) {
                Some(descriptor) if descriptor.direction == MethodDirection::ClientToServer => {}
                Some(descriptor) => violations.push(format!(
                    "`{method}` is routed inbound but registered {:?}",
                    descriptor.direction
                )),
                None => violations.push(format!("`{method}` is routed but unclassified")),
            }
        }
        assert!(
            violations.is_empty(),
            "route inventory disagrees with the method-direction registry:\n{}",
            violations.join("\n")
        );

        // The removed wrong-direction routes must stay out of the table.
        for banned in ["workspace/applyEdit", "workspace/configuration"] {
            assert!(
                !routed_methods.contains(banned),
                "`{banned}` must not return as an inbound application route (#8896)"
            );
        }
    }

    fn core_method_constants() -> BTreeSet<(String, String)> {
        let methods_rs = include_str!("../../../perl-lsp-rs-core/src/protocol/methods.rs");
        let mut constants = BTreeSet::new();
        for line in methods_rs.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("pub const ") else { continue };
            let Some((name, value)) = rest.split_once(": &str = \"") else { continue };
            let Some(value_end) = value.strip_suffix("\";") else { continue };
            constants.insert((name.trim().to_string(), value_end.to_string()));
        }
        constants
    }

    /// Files containing production outbound send sites. Each `.send_request`,
    /// `.send_notification`, `.notify`, or `.send_request_internal` call in
    /// these files must name a registry-classified server→client method of
    /// the matching envelope kind. An unclassified outbound method fails
    /// here instead of becoming a stringly-typed call (negative control 4).
    const OUTBOUND_SCAN_FILES: &[&str] = &[
        "src/runtime/client_requests.rs",
        "src/runtime/workspace.rs",
        "src/runtime/window.rs",
        "src/runtime/diagnostics.rs",
        "src/runtime/text_sync.rs",
        "src/runtime/text_sync/lifecycle.rs",
        "src/runtime/lifecycle/mod.rs",
        "src/runtime/lifecycle/workspace.rs",
        "src/runtime/lifecycle/watchers.rs",
        "src/runtime/language/streaming.rs",
        "src/runtime/language/virtual_content.rs",
        "src/runtime/workspace_progress.rs",
        "src/runtime/dispatch/lifecycle.rs",
    ];

    /// What a send-call site passes as its method argument.
    enum MethodArg {
        /// A nameable method: string literal or resolved protocol constant.
        Named(String),
        /// Pure forwarding plumbing whose arguments are bare variables.
        Plumbing,
        /// Anything else — flagged for review.
        Unresolved,
    }

    /// Split the text following a trigger into its top-level arguments.
    fn split_top_level_args(after_trigger: &str) -> Vec<&str> {
        let mut args = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        for (index, ch) in after_trigger.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        args.push(after_trigger[start..index].trim());
                        break;
                    }
                    depth -= 1;
                }
                ',' if depth == 0 => {
                    args.push(after_trigger[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }
        args
    }

    fn is_plain_identifier(arg: &str) -> bool {
        !arg.is_empty() && arg.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    }

    /// Classify the method argument of a send-call site.
    ///
    /// Call-site conventions differ: `LspServer::send_request(method, …)`
    /// takes the method first while `OutboundSender::send_request(id,
    /// method, …)` takes it second, so every top-level argument is
    /// considered and the first one shaped like a method name (string
    /// literal or ALL_CAPS protocol constant) wins.
    fn classify_method_arg(
        after_trigger: &str,
        constants: &BTreeSet<(String, String)>,
    ) -> MethodArg {
        let args = split_top_level_args(after_trigger);
        for arg in &args {
            if let Some(method) =
                arg.strip_prefix('"').and_then(|literal| literal.split('"').next())
            {
                return MethodArg::Named(method.to_string());
            }
            let ident: String =
                arg.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let constant_shaped =
                ident.len() > 3 && ident.chars().all(|c| c.is_ascii_uppercase() || c == '_');
            if constant_shaped
                && let Some((_, value)) = constants.iter().find(|(name, _)| name == &ident)
            {
                return MethodArg::Named(value.clone());
            }
        }
        if !args.is_empty() && args.iter().all(|arg| is_plain_identifier(arg)) {
            return MethodArg::Plumbing;
        }
        MethodArg::Unresolved
    }

    #[test]
    fn outbound_inventory_matches_registry_directions() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let constants = core_method_constants();

        let request_triggers = [".send_request(", ".send_request_internal("];
        let notification_triggers = [".send_notification(", ".notify("];

        let mut violations = Vec::new();
        let mut checked_calls = 0usize;

        for relative in OUTBOUND_SCAN_FILES {
            let path = std::path::Path::new(manifest).join(relative);
            let Ok(source) = std::fs::read_to_string(&path) else {
                violations.push(format!("{relative}: unreadable"));
                continue;
            };
            let stripped = strip_test_modules(&source);
            let lines: Vec<&str> = stripped.lines().collect();

            for (line_index, line) in lines.iter().enumerate() {
                let is_request_call = request_triggers.iter().any(|t| line.contains(t));
                let is_notification_call = notification_triggers.iter().any(|t| line.contains(t));
                if !is_request_call && !is_notification_call {
                    continue;
                }

                // Concatenate the trigger line plus the following two so
                // multi-line calls still expose their arguments.
                let mut window = String::from(*line);
                for follow in lines.iter().skip(line_index + 1).take(2) {
                    window.push(' ');
                    window.push_str(follow);
                }

                let trigger_end = request_triggers
                    .iter()
                    .chain(notification_triggers.iter())
                    .filter_map(|t| line.find(t).map(|pos| pos + t.len()))
                    .min();
                let Some(trigger_end) = trigger_end else { continue };

                checked_calls += 1;
                let tail = &window[trigger_end.min(window.len())..];
                match classify_method_arg(tail, &constants) {
                    MethodArg::Plumbing => continue,
                    MethodArg::Unresolved => {
                        violations.push(format!(
                            "{relative}: could not extract the method argument of an \
                             outbound send"
                        ));
                        continue;
                    }
                    MethodArg::Named(method) => {
                        if method.is_empty() {
                            continue;
                        }
                        let required_kind = if is_request_call {
                            EnvelopeKind::Request
                        } else {
                            EnvelopeKind::Notification
                        };
                        match lookup(&method) {
                            Some(descriptor)
                                if descriptor.direction == MethodDirection::ServerToClient
                                    && descriptor.kind == required_kind => {}
                            Some(descriptor) => violations.push(format!(
                                "{relative}: outbound `{method}` is registered {:?}/{:?}",
                                descriptor.direction, descriptor.kind
                            )),
                            None => violations
                                .push(format!("{relative}: outbound `{method}` is unclassified")),
                        }
                    }
                }
            }
        }

        assert!(
            checked_calls >= 25,
            "the outbound scan stopped seeing send sites ({checked_calls}); \
             inspect the scanner before trusting green"
        );
        assert!(
            violations.is_empty(),
            "outbound inventory disagrees with the method-direction registry:\n{}",
            violations.join("\n")
        );
    }

    /// The registry classifies the shared protocol constant values used by
    /// outbound helpers, so constant-named call sites cannot dodge the table.
    #[test]
    fn refresh_constants_are_classified_server_to_client_requests() {
        let constants = core_method_constants();
        let mut violations = Vec::new();

        for name in [
            "WORKSPACE_APPLY_EDIT",
            "WORKSPACE_CONFIGURATION",
            "WORKSPACE_CODE_LENS_REFRESH",
            "WORKSPACE_SEMANTIC_TOKENS_REFRESH",
            "WORKSPACE_INLAY_HINT_REFRESH",
            "WORKSPACE_INLINE_VALUE_REFRESH",
            "WORKSPACE_DIAGNOSTIC_REFRESH",
            "WORKSPACE_FOLDING_RANGE_REFRESH",
            "WORKSPACE_TEXT_DOCUMENT_CONTENT_REFRESH",
        ] {
            let Some((_, method)) = constants.iter().find(|(candidate, _)| candidate == name)
            else {
                violations.push(format!("{name} missing from protocol constants"));
                continue;
            };
            match lookup(method) {
                None => violations.push(format!("`{method}` ({name}) must be classified")),
                Some(descriptor) => {
                    if descriptor.direction != MethodDirection::ServerToClient {
                        violations.push(format!("{name} must be server-to-client"));
                    }
                    if descriptor.kind != EnvelopeKind::Request {
                        violations.push(format!("{name} must be a request"));
                    }
                }
            }
        }
        assert!(
            violations.is_empty(),
            "outbound protocol constants disagree with the registry:\n{}",
            violations.join("\n")
        );
    }
}
