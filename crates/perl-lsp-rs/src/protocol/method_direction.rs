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
    ext(
        "typeHierarchy/prepare",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::RequiresInitialized,
    ),
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
    ext(
        "workspace/symbol/resolve",
        EnvelopeKind::Request,
        MethodDirection::ClientToServer,
        LifecyclePhase::RequiresInitialized,
    ),
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
    enum TestModuleKind {
        External,
        Inline,
    }

    struct TestModuleDeclaration {
        kind: TestModuleKind,
        terminator_offset: usize,
    }

    fn skip_declaration_trivia(source: &str, mut index: usize) -> Option<usize> {
        let bytes = source.as_bytes();
        loop {
            while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                index += 1;
            }
            if bytes.get(index..).is_some_and(|tail| tail.starts_with(b"//")) {
                let Some(newline) =
                    bytes.get(index..).and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
                else {
                    return None;
                };
                index += newline + 1;
                continue;
            }
            if !bytes.get(index..).is_some_and(|tail| tail.starts_with(b"/*")) {
                return Some(index);
            }
            let mut depth = 1;
            index += 2;
            while index < bytes.len() {
                if bytes.get(index..).is_some_and(|tail| tail.starts_with(b"/*")) {
                    depth += 1;
                    index += 2;
                } else if bytes.get(index..).is_some_and(|tail| tail.starts_with(b"*/")) {
                    depth -= 1;
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    index += 1;
                }
            }
            if depth != 0 {
                return None;
            }
        }
    }

    fn visibility_close(source: &str) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut depth = 0;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index..].starts_with(b"//") {
                let newline = bytes[index..].iter().position(|byte| *byte == b'\n')?;
                index += newline + 1;
                continue;
            }
            if bytes[index..].starts_with(b"/*") {
                let mut comment_depth = 1;
                index += 2;
                while index < bytes.len() && comment_depth > 0 {
                    if bytes[index..].starts_with(b"/*") {
                        comment_depth += 1;
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        comment_depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                continue;
            }
            match bytes[index] {
                b'(' => depth += 1,
                b')' if depth == 1 => return Some(index),
                b')' if depth > 1 => depth -= 1,
                _ => {}
            }
            index += 1;
        }
        None
    }

    fn test_module_declaration(line: &str) -> Option<TestModuleDeclaration> {
        let mut rest = line.trim_start();

        loop {
            let attribute_start = skip_declaration_trivia(rest, 0)?;
            rest = &rest[attribute_start..];
            if !rest.starts_with("#[") {
                break;
            }
            let attribute_end = outer_attribute_end(rest)?;
            rest = &rest[attribute_end..];
        }

        if let Some(after_pub) = rest.strip_prefix("pub")
            && after_pub.chars().next().is_some_and(|next| next.is_whitespace() || next == '(')
        {
            rest = after_pub;
            let visibility_start = skip_declaration_trivia(rest, 0)?;
            rest = &rest[visibility_start..];
            if rest.starts_with('(') {
                let Some(close) = visibility_close(rest) else { return None };
                let after_visibility = skip_declaration_trivia(&rest[close + 1..], 0)?;
                rest = &rest[close + 1 + after_visibility..];
            }
        }

        let Some(after_mod) = rest.strip_prefix("mod") else { return None };
        if !after_mod.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }

        let name_start = skip_declaration_trivia(after_mod, 0)?;
        let rest = &after_mod[name_start..];
        let name_end = rest.char_indices().find_map(|(offset, character)| {
            (character.is_whitespace()
                || matches!(character, '{' | ';')
                || rest[offset..].starts_with("/*"))
            .then_some(offset)
        });
        let Some(name_end) = name_end else { return None };
        if name_end == 0 {
            return None;
        }

        let terminator_offset = skip_declaration_trivia(rest, name_end)?;
        let kind = match rest[terminator_offset..].chars().next() {
            Some(';') => TestModuleKind::External,
            Some('{') => TestModuleKind::Inline,
            _ => return None,
        };
        Some(TestModuleDeclaration {
            kind,
            terminator_offset: line.len() - rest.len() + terminator_offset,
        })
    }

    fn outer_attribute_end(source: &str) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut index = 2;
        let mut brackets = 1;
        let mut state = LexicalState::Normal;
        while index < bytes.len() {
            match state {
                LexicalState::Normal => {
                    if bytes[index..].starts_with(b"//") {
                        let newline = bytes[index..].iter().position(|byte| *byte == b'\n')?;
                        index += newline + 1;
                    } else if bytes[index..].starts_with(b"/*") {
                        state = LexicalState::BlockComment(1);
                        index += 2;
                    } else if let Some((next, hashes)) = raw_string_start(bytes, index) {
                        state = LexicalState::RawString { hashes };
                        index = next;
                    } else if bytes[index] == b'"' {
                        state = LexicalState::Quoted { delimiter: b'"', escaped: false };
                        index += 1;
                    } else if bytes[index] == b'\'' && char_literal_ends_on_line(bytes, index) {
                        state = LexicalState::Quoted { delimiter: b'\'', escaped: false };
                        index += 1;
                    } else if bytes[index] == b'[' {
                        brackets += 1;
                        index += 1;
                    } else if bytes[index] == b']' {
                        brackets -= 1;
                        index += 1;
                        if brackets == 0 {
                            return Some(index);
                        }
                    } else {
                        index += 1;
                    }
                }
                LexicalState::BlockComment(depth) => {
                    if bytes[index..].starts_with(b"/*") {
                        state = LexicalState::BlockComment(depth + 1);
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        index += 2;
                        if depth == 1 {
                            state = LexicalState::Normal;
                        } else {
                            state = LexicalState::BlockComment(depth - 1);
                        }
                    } else {
                        index += 1;
                    }
                }
                LexicalState::Quoted { delimiter, escaped } => {
                    if escaped {
                        state = LexicalState::Quoted { delimiter, escaped: false };
                    } else if bytes[index] == b'\\' {
                        state = LexicalState::Quoted { delimiter, escaped: true };
                    } else if bytes[index] == delimiter {
                        state = LexicalState::Normal;
                    }
                    index += 1;
                }
                LexicalState::RawString { hashes } => {
                    if raw_string_ends_at(bytes, index, hashes) {
                        state = LexicalState::Normal;
                        index += hashes + 1;
                    } else {
                        index += 1;
                    }
                }
            }
        }
        None
    }

    #[derive(Clone, Copy)]
    enum LexicalState {
        Normal,
        BlockComment(usize),
        Quoted { delimiter: u8, escaped: bool },
        RawString { hashes: usize },
    }

    fn raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
        let raw_index = match bytes.get(index) {
            Some(b'r') => index,
            Some(b'b') if bytes.get(index + 1) == Some(&b'r') => index + 1,
            _ => return None,
        };
        let mut cursor = raw_index + 1;
        while bytes.get(cursor) == Some(&b'#') {
            cursor += 1;
        }
        (bytes.get(cursor) == Some(&b'"')).then_some((cursor + 1, cursor - raw_index - 1))
    }

    fn char_literal_ends_on_line(bytes: &[u8], start: usize) -> bool {
        let Some(&first) = bytes.get(start + 1) else {
            return false;
        };
        let mut index = start + 2;
        if first == b'\\' {
            if bytes.get(index) == Some(&b'u') && bytes.get(index + 1) == Some(&b'{') {
                index += 2;
                while bytes.get(index).is_some_and(|byte| *byte != b'}') {
                    index += 1;
                }
                if bytes.get(index) == Some(&b'}') {
                    index += 1;
                }
            } else {
                index += 1;
            }
        } else if first == b'\'' || first == b'\n' || first == b'\r' {
            return false;
        } else if first >= 0x80 {
            let Ok(remainder) = std::str::from_utf8(&bytes[start + 1..]) else {
                return false;
            };
            let Some(character) = remainder.chars().next() else {
                return false;
            };
            index = start + 1 + character.len_utf8();
        }
        bytes.get(index) == Some(&b'\'')
    }

    fn raw_string_ends_at(bytes: &[u8], index: usize, hashes: usize) -> bool {
        bytes[index] == b'"'
            && bytes
                .get(index + 1..index + 1 + hashes)
                .is_some_and(|closing| closing.iter().all(|byte| *byte == b'#'))
    }

    fn scan_structural_braces(
        line: &str,
        state: &mut LexicalState,
        starting_depth: i64,
        start_index: usize,
    ) -> (i64, Option<usize>) {
        let bytes = line.as_bytes();
        let mut index = start_index;
        let mut depth_delta = 0;

        if let LexicalState::Quoted { delimiter, escaped: true } = *state {
            *state = LexicalState::Quoted { delimiter, escaped: false };
        }

        while index < bytes.len() {
            match *state {
                LexicalState::Normal => {
                    if bytes[index..].starts_with(b"//") {
                        break;
                    }
                    if bytes[index..].starts_with(b"/*") {
                        *state = LexicalState::BlockComment(1);
                        index += 2;
                        continue;
                    }
                    if let Some((next, hashes)) = raw_string_start(bytes, index) {
                        *state = LexicalState::RawString { hashes };
                        index = next;
                        continue;
                    }
                    if matches!(bytes[index], b'b') && bytes.get(index + 1) == Some(&b'"') {
                        *state = LexicalState::Quoted { delimiter: b'"', escaped: false };
                        index += 2;
                        continue;
                    }
                    if matches!(bytes[index], b'"') {
                        *state = LexicalState::Quoted { delimiter: b'"', escaped: false };
                        index += 1;
                        continue;
                    }
                    if matches!(bytes[index], b'\'') && char_literal_ends_on_line(bytes, index) {
                        *state = LexicalState::Quoted { delimiter: b'\'', escaped: false };
                        index += 1;
                        continue;
                    }
                    if bytes[index] == b'b'
                        && bytes.get(index + 1) == Some(&b'\'')
                        && char_literal_ends_on_line(bytes, index + 1)
                    {
                        *state = LexicalState::Quoted { delimiter: b'\'', escaped: false };
                        index += 2;
                        continue;
                    }
                    match bytes[index] {
                        b'{' => depth_delta += 1,
                        b'}' => {
                            depth_delta -= 1;
                            if starting_depth + depth_delta == 0 {
                                return (depth_delta, Some(index));
                            }
                        }
                        _ => {}
                    }
                    index += 1;
                }
                LexicalState::BlockComment(depth) => {
                    if bytes[index..].starts_with(b"/*") {
                        *state = LexicalState::BlockComment(depth + 1);
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        index += 2;
                        if depth == 1 {
                            *state = LexicalState::Normal;
                        } else {
                            *state = LexicalState::BlockComment(depth - 1);
                        }
                    } else {
                        index += 1;
                    }
                }
                LexicalState::Quoted { delimiter, escaped } => {
                    if escaped {
                        *state = LexicalState::Quoted { delimiter, escaped: false };
                        index += 1;
                    } else if bytes[index] == b'\\' {
                        *state = LexicalState::Quoted { delimiter, escaped: true };
                        index += 1;
                    } else if bytes[index] == delimiter {
                        *state = LexicalState::Normal;
                        index += 1;
                    } else {
                        index += 1;
                    }
                }
                LexicalState::RawString { hashes } => {
                    if raw_string_ends_at(bytes, index, hashes) {
                        *state = LexicalState::Normal;
                        index += hashes + 1;
                    } else {
                        index += 1;
                    }
                }
            }
        }

        (depth_delta, None)
    }

    fn strip_line_comment(source: &str) -> &str {
        let bytes = source.as_bytes();
        let mut state = LexicalState::Normal;
        let mut index = 0;
        while index < bytes.len() {
            match state {
                LexicalState::Normal => {
                    if bytes[index..].starts_with(b"//") {
                        return &source[..index];
                    }
                    if bytes[index..].starts_with(b"/*") {
                        state = LexicalState::BlockComment(1);
                        index += 2;
                        continue;
                    }
                    if let Some((next, hashes)) = raw_string_start(bytes, index) {
                        state = LexicalState::RawString { hashes };
                        index = next;
                        continue;
                    }
                    if bytes[index] == b'"'
                        || (bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'"'))
                    {
                        state = LexicalState::Quoted { delimiter: b'"', escaped: false };
                        index += usize::from(bytes[index] == b'b') + 1;
                        continue;
                    }
                    if bytes[index] == b'\'' && char_literal_ends_on_line(bytes, index) {
                        state = LexicalState::Quoted { delimiter: b'\'', escaped: false };
                    }
                    index += 1;
                }
                LexicalState::BlockComment(depth) => {
                    if bytes[index..].starts_with(b"/*") {
                        state = LexicalState::BlockComment(depth + 1);
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        index += 2;
                        state = if depth == 1 {
                            LexicalState::Normal
                        } else {
                            LexicalState::BlockComment(depth - 1)
                        };
                    } else {
                        index += 1;
                    }
                }
                LexicalState::Quoted { delimiter, escaped } => {
                    if escaped {
                        state = LexicalState::Quoted { delimiter, escaped: false };
                    } else if bytes[index] == b'\\' {
                        state = LexicalState::Quoted { delimiter, escaped: true };
                    } else if bytes[index] == delimiter {
                        state = LexicalState::Normal;
                    }
                    index += 1;
                }
                LexicalState::RawString { hashes } => {
                    if raw_string_ends_at(bytes, index, hashes) {
                        state = LexicalState::Normal;
                        index += hashes + 1;
                    } else {
                        index += 1;
                    }
                }
            }
        }
        source
    }

    fn test_cfg_attribute_start(source: &str, state: &mut LexicalState) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match *state {
                LexicalState::Normal => {
                    if bytes[index..].starts_with(b"//") {
                        return None;
                    }
                    if bytes[index..].starts_with(b"/*") {
                        *state = LexicalState::BlockComment(1);
                        index += 2;
                    } else if let Some((next, hashes)) = raw_string_start(bytes, index) {
                        *state = LexicalState::RawString { hashes };
                        index = next;
                    } else if bytes[index] == b'"'
                        || (bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'"'))
                    {
                        *state = LexicalState::Quoted { delimiter: b'"', escaped: false };
                        index += usize::from(bytes[index] == b'b') + 1;
                    } else if bytes[index] == b'\'' && char_literal_ends_on_line(bytes, index) {
                        *state = LexicalState::Quoted { delimiter: b'\'', escaped: false };
                        index += 1;
                    } else if bytes[index..].starts_with(b"#[cfg(test)]") {
                        return Some(index);
                    } else {
                        index += 1;
                    }
                }
                LexicalState::BlockComment(depth) => {
                    if bytes[index..].starts_with(b"/*") {
                        *state = LexicalState::BlockComment(depth + 1);
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        index += 2;
                        *state = if depth == 1 {
                            LexicalState::Normal
                        } else {
                            LexicalState::BlockComment(depth - 1)
                        };
                    } else {
                        index += 1;
                    }
                }
                LexicalState::Quoted { delimiter, escaped } => {
                    if escaped {
                        *state = LexicalState::Quoted { delimiter, escaped: false };
                    } else if bytes[index] == b'\\' {
                        *state = LexicalState::Quoted { delimiter, escaped: true };
                    } else if bytes[index] == delimiter {
                        *state = LexicalState::Normal;
                    }
                    index += 1;
                }
                LexicalState::RawString { hashes } => {
                    if raw_string_ends_at(bytes, index, hashes) {
                        *state = LexicalState::Normal;
                        index += hashes + 1;
                    } else {
                        index += 1;
                    }
                }
            }
        }
        None
    }

    fn strip_test_modules(source: &str) -> String {
        let mut result = String::with_capacity(source.len());
        let lines: Vec<&str> = source.lines().collect();
        let mut line_index = 0;
        let mut source_lexical_state = LexicalState::Normal;
        while let Some(&line) = lines.get(line_index) {
            line_index += 1;
            let Some(attribute_start) = test_cfg_attribute_start(line, &mut source_lexical_state)
            else {
                result.push_str(line);
                result.push('\n');
                continue;
            };
            let attribute_end = attribute_start + "#[cfg(test)]".len();
            let prefix = &line[..attribute_start];
            let inline_declaration = &line[attribute_end..];

            if !inline_declaration.trim().is_empty() {
                if let Some(declaration) = test_module_declaration(inline_declaration) {
                    if matches!(declaration.kind, TestModuleKind::External) {
                        let suffix = skip_declaration_trivia(
                            inline_declaration,
                            declaration.terminator_offset + 1,
                        )
                        .and_then(|start| inline_declaration.get(start..))
                        .map(strip_line_comment)
                        .unwrap_or_default();
                        let kept = format!("{}{}", prefix, suffix.trim_end());
                        if !kept.trim().is_empty() {
                            result.push_str(kept.trim_end());
                            result.push('\n');
                        }
                        continue;
                    }
                    let mut lexical_state = LexicalState::Normal;
                    let (initial_depth, close) = scan_structural_braces(
                        inline_declaration,
                        &mut lexical_state,
                        0,
                        declaration.terminator_offset,
                    );
                    if let Some(close) = close {
                        let suffix = skip_declaration_trivia(inline_declaration, close + 1)
                            .and_then(|start| inline_declaration.get(start..))
                            .map(strip_line_comment)
                            .unwrap_or_default();
                        let kept = format!("{}{}", prefix, suffix.trim_end());
                        if !kept.trim().is_empty() {
                            result.push_str(kept.trim_end());
                            result.push('\n');
                        }
                        continue;
                    }

                    // The declaration starts on the attribute line but its
                    // body may close on a later line. Continue the same
                    // structural scan instead of emitting the body as
                    // production source.
                    let mut depth = initial_depth;
                    let mut module_close = None;
                    while depth > 0 {
                        let Some(&body_line) = lines.get(line_index) else { break };
                        line_index += 1;
                        let (delta, body_close) =
                            scan_structural_braces(body_line, &mut lexical_state, depth, 0);
                        depth += delta;
                        if body_close.is_some() {
                            module_close = body_close.map(|offset| (line_index - 1, offset));
                            break;
                        }
                    }
                    if let Some((closing_line_index, close)) = module_close {
                        let closing_line =
                            lines.get(closing_line_index).copied().unwrap_or_default();
                        let suffix = skip_declaration_trivia(closing_line, close + 1)
                            .and_then(|start| closing_line.get(start..))
                            .map(strip_line_comment)
                            .unwrap_or_default();
                        let kept = format!("{}{}", prefix, suffix.trim_end());
                        if !kept.trim().is_empty() {
                            result.push_str(kept.trim_end());
                            result.push('\n');
                        }
                        continue;
                    }
                }
            }

            result.push_str(prefix);
            result.push('\n');
            if !inline_declaration.trim().is_empty() {
                // An attribute that is not followed by a complete same-line
                // module remains ordinary source; do not hide it.
                result.push_str(inline_declaration);
                result.push('\n');
                continue;
            }

            // Accumulate declaration trivia through the real `{` or `;` so
            // valid multiline visibility/comments do not leak test modules.
            let declaration_start = line_index;
            let mut declaration_text = String::new();
            let mut declaration = None;
            for end in declaration_start..lines.len() {
                if !declaration_text.is_empty() {
                    declaration_text.push('\n');
                }
                declaration_text.push_str(lines[end]);
                if let Some(found) = test_module_declaration(&declaration_text) {
                    declaration = Some((end, found));
                    break;
                }
            }
            let Some((declaration_end, declaration)) = declaration else {
                continue;
            };
            line_index = declaration_end + 1;

            // A `mod x;` declaration ends here. Preserve actual same-line
            // production code, but discard a trailing comment after the `;`.
            if matches!(declaration.kind, TestModuleKind::External) {
                let declaration_line = lines[declaration_end];
                let prefix_len = declaration_text.len() - declaration_line.len();
                let offset = declaration.terminator_offset.saturating_sub(prefix_len);
                let suffix = skip_declaration_trivia(declaration_line, offset + 1)
                    .and_then(|suffix_start| declaration_line.get(suffix_start..))
                    .map(strip_line_comment)
                    .unwrap_or_default();
                if !suffix.trim().is_empty() {
                    result.push_str(suffix.trim_end());
                    result.push('\n');
                }
                continue;
            }
            let mut lexical_state = LexicalState::Normal;
            let mut depth = 0;
            let mut module_close = None;
            for (offset, declaration_line) in
                lines[declaration_start..=declaration_end].iter().enumerate()
            {
                let line_offset = if offset + declaration_start == declaration_end {
                    declaration
                        .terminator_offset
                        .saturating_sub(declaration_text.len() - declaration_line.len())
                } else {
                    0
                };
                let (delta, close) = scan_structural_braces(
                    declaration_line,
                    &mut lexical_state,
                    depth,
                    line_offset,
                );
                depth += delta;
                if close.is_some() {
                    module_close = close;
                    break;
                }
            }
            while depth > 0 {
                let Some(&inner) = lines.get(line_index) else {
                    break;
                };
                line_index += 1;
                let (delta, close) = scan_structural_braces(inner, &mut lexical_state, depth, 0);
                depth += delta;
                if close.is_some() {
                    module_close = close;
                    break;
                }
            }
            if let Some(close) = module_close {
                let closing_line =
                    lines.get(line_index.saturating_sub(1)).copied().unwrap_or_default();
                let suffix = skip_declaration_trivia(closing_line, close + 1)
                    .and_then(|suffix_start| closing_line.get(suffix_start..))
                    .map(strip_line_comment)
                    .unwrap_or_default();
                if !suffix.trim().is_empty() {
                    result.push_str(suffix.trim_end());
                    result.push('\n');
                }
            }
            if depth < 0 {
                // A malformed declaration must not cause the remainder of
                // the source to disappear from the inventory.
                line_index = declaration_start;
                result.truncate(result.len().saturating_sub(line.len() + 1));
                result.push_str(line);
                result.push('\n');
            }
        }
        strip_non_code_comments(&result)
    }

    /// The inventory matcher below intentionally operates on source text. Remove
    /// comments before it sees that text so comment examples cannot masquerade as
    /// outbound method literals. Newlines are retained to keep diagnostics stable.
    fn strip_non_code_comments(source: &str) -> String {
        let bytes = source.as_bytes();
        let mut output = String::with_capacity(source.len());
        let mut state = LexicalState::Normal;
        let mut index = 0;
        while index < bytes.len() {
            match state {
                LexicalState::Normal if bytes[index..].starts_with(b"//") => {
                    output.push(' ');
                    index += 2;
                    while index < bytes.len() && bytes[index] != b'\n' {
                        index += 1;
                    }
                }
                LexicalState::Normal if bytes[index..].starts_with(b"/*") => {
                    output.push(' ');
                    state = LexicalState::BlockComment(1);
                    index += 2;
                }
                LexicalState::Normal => {
                    if let Some((next, hashes)) = raw_string_start(bytes, index) {
                        output.push_str(&source[index..next]);
                        state = LexicalState::RawString { hashes };
                        index = next;
                    } else if bytes[index] == b'"'
                        || (bytes[index] == b'\'' && char_literal_ends_on_line(bytes, index))
                    {
                        output.push(bytes[index] as char);
                        state = LexicalState::Quoted { delimiter: bytes[index], escaped: false };
                        index += 1;
                    } else {
                        let character_len =
                            source[index..].chars().next().map_or(1, char::len_utf8);
                        output.push_str(&source[index..index + character_len]);
                        index += character_len;
                    }
                }
                LexicalState::BlockComment(depth) => {
                    if bytes[index..].starts_with(b"/*") {
                        state = LexicalState::BlockComment(depth + 1);
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        state = if depth == 1 {
                            LexicalState::Normal
                        } else {
                            LexicalState::BlockComment(depth - 1)
                        };
                        index += 2;
                    } else {
                        if bytes[index] == b'\n' {
                            output.push('\n');
                        }
                        index += 1;
                    }
                }
                LexicalState::Quoted { delimiter, escaped } => {
                    let character_len = source[index..].chars().next().map_or(1, char::len_utf8);
                    output.push_str(&source[index..index + character_len]);
                    state = if escaped {
                        LexicalState::Quoted { delimiter, escaped: false }
                    } else if bytes[index] == b'\\' {
                        LexicalState::Quoted { delimiter, escaped: true }
                    } else if bytes[index] == delimiter {
                        LexicalState::Normal
                    } else {
                        LexicalState::Quoted { delimiter, escaped: false }
                    };
                    index += character_len;
                }
                LexicalState::RawString { hashes } => {
                    let character_len = source[index..].chars().next().map_or(1, char::len_utf8);
                    output.push_str(&source[index..index + character_len]);
                    if raw_string_ends_at(bytes, index, hashes) {
                        state = LexicalState::Normal;
                        for _ in 0..hashes.min(bytes.len().saturating_sub(index + 1)) {
                            index += 1;
                            output.push(bytes[index] as char);
                        }
                    }
                    index += character_len;
                }
            }
        }
        output
    }

    #[test]
    fn strip_test_modules_handles_visible_and_external_test_modules() {
        let source = r#"
#[cfg(test)] mod same_line_inline { fn send() { client.send_request("test-only/same-line-inline"); } } fn after_same_line_inline() { send("production/after-same-line-inline"); }
#[cfg(test)] pub(crate) mod same_line_external; // client.send_request("test-only/same-line-external")
#[cfg(test)] mod same_line_external_suffix; fn after_same_line_external() { send("production/after-same-line-external"); }
#[cfg(test)]
mod plain { const PLAIN: &str = "plain/test"; }
#[cfg(test)]
pub mod public { const PUBLIC: &str = "pub/test"; }
#[cfg(test)]
pub(crate) mod restricted { const RESTRICTED: &str = "crate/test"; }
#[cfg(test)]
pub(super) mod parent { const PARENT: &str = "super/test"; }
#[cfg(test)]
pub(self) mod private { const PRIVATE: &str = "self/test"; }
#[cfg(test)]
pub(in crate::protocol) mod scoped { const SCOPED: &str = "scoped/test"; }
#[cfg(test)]
pub(crate) mod external; // client.send_request("test-only/external-comment") {
#[cfg(test)]
mod plain_external;
#[cfg(test)]
pub(crate) /* comment containing { and } */ mod commented_visibility {
    fn send() { client.send_request("test-only/visibility-comment"); }
}
#[cfg(test)]
pub(crate) // visibility line trivia
mod line_trivia {
    fn send() { client.send_request("test-only/line-visibility"); }
}
#[cfg(test)]
mod /* name trivia */ name_trivia {
    fn send() { client.send_request("test-only/name-comment"); }
}
#[cfg(test)]
mod external_block; /* client.send_request("test-only/block-comment") */
#[cfg(test)]
pub(in /* fake ) delimiter */ crate::protocol) mod scoped_comment {
    fn send() { client.send_request("test-only/scoped-comment"); }
}
#[cfg(test)]
pub(crate) mod commented /* comment containing { and } */ ;
#[cfg(test)]
mod adjacent/* comment containing a fake send: client.send_request("test-only/adjacent-comment"); */{ fn send() { client.send_request("test-only/adjacent-body"); } }
#[cfg(test)]
pub(crate) mod same_line; fn after_external() { send("production/after_external"); }
#[cfg(test)]
mod compact { fn send() { client.send_request("test-only/compact"); } } fn after_compact() {
    send("production/after_compact");
}
#[cfg(test)]
pub(crate) mod lexical_forms {
    const CLOSE: &str = "}";
    const OPEN: &str = "{";
    // }
    /* outer { /* nested } */ still inside */
    const RAW: &str = r###("{ }")###;
    const BYTE_RAW: &[u8] = br##("{ }")##;
    const CHARACTER: char = '}';
    const BYTE_CHARACTER: u8 = b'{';
    const CONTINUED: &str = "opening\
";
}
#[cfg(test)]
pub(crate) /* visibility trivia
    with a fake ; and { } */
mod multiline /* declaration trivia ; { } */
{
    const MULTILINE: &str = "test-only/multiline";
}
fn after_multiline() { send("production/after_multiline"); }
fn production() { send("production/after"); }
pub(crate) mod visible { const KEEP: &str = "visible/production"; }
"#;

        let stripped = strip_test_modules(source);
        for test_only in [
            "test-only/same-line-inline",
            "test-only/same-line-external",
            "plain/test",
            "pub/test",
            "crate/test",
            "super/test",
            "self/test",
            "scoped/test",
        ] {
            assert!(!stripped.contains(test_only), "test-only literal leaked: {test_only}");
        }
        assert!(stripped.contains("production/after-same-line-inline"));
        assert!(stripped.contains("production/after-same-line-external"));
        assert!(!stripped.contains("pub(crate) mod external;"));
        assert!(!stripped.contains("test-only/external-comment"));
        assert!(!stripped.contains("test-only/visibility-comment"));
        assert!(!stripped.contains("test-only/line-visibility"));
        assert!(!stripped.contains("test-only/name-comment"));
        assert!(!stripped.contains("test-only/block-comment"));
        assert!(!stripped.contains("test-only/scoped-comment"));
        assert!(!stripped.contains("test-only/adjacent-comment"));
        assert!(!stripped.contains("test-only/adjacent-body"));
        assert!(!stripped.contains("test-only/compact"));
        assert!(!stripped.contains("pub(crate) mod commented"));
        assert!(!stripped.contains("pub(crate) mod lexical_forms"));
        assert!(!stripped.contains("CONTINUED"));
        assert!(!stripped.contains("test-only/multiline"));
        assert!(stripped.contains("production/after_external"));
        assert!(stripped.contains("production/after_compact"));
        assert!(stripped.contains("production/after_multiline"));
        assert!(stripped.contains("production/after"));
        assert!(stripped.contains("pub(crate) mod visible"));
        assert!(stripped.contains("visible/production"));
    }

    #[test]
    fn strip_test_modules_preserves_lexical_state_across_lines() {
        let source = r####"
/* a comment containing
#[cfg(test)] mod fake { client.send_request("comment-only"); }
*/
const RAW: &str = r###"
#[cfg(test)] mod fake_raw { client.send_request("raw-only"); }
"###;
#[cfg(test)] mod multiline {
    fn send() { client.send_request("test-only/multiline-body"); }
}
fn production() { client.send_request("production/after-multiline"); }
"####;

        let stripped = strip_test_modules(source);
        assert!(!stripped.contains("comment-only"));
        assert!(stripped.contains("raw-only"));
        assert!(!stripped.contains("test-only/multiline-body"));
        assert!(stripped.contains("production/after-multiline"));
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

        // The removed wrong-direction routes must stay out of the table. The
        // second assertion reads the comment-stripped source directly so a
        // re-added arm cannot hide behind a different line shape than the
        // arm-head heuristic; comments naming the methods are not routes.
        for banned in ["workspace/applyEdit", "workspace/configuration"] {
            assert!(
                !routed_methods.contains(banned),
                "`{banned}` must not return as an inbound application route (#8896)"
            );
            let code_only = source
                .lines()
                .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
                .collect::<String>();
            assert!(
                !code_only.contains(banned),
                "`{banned}` appears in routing code outside a comment (#8896)"
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

    /// Production Rust files under `src/runtime`, discovered by walking the
    /// directory (sorted for deterministic diagnostics) so a newly added file
    /// containing outbound send sites is scanned without extending a
    /// hand-maintained list. Each `.send_request`, `.send_notification`,
    /// `.notify`, or `.send_request_internal` call in these files must name a
    /// registry-classified server→client method of the matching envelope
    /// kind. An unclassified outbound method fails here instead of becoming
    /// a stringly-typed call (negative control 4).
    fn runtime_source_files(manifest_dir: &str) -> Vec<std::path::PathBuf> {
        fn visit(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        visit(&std::path::Path::new(manifest_dir).join("src").join("runtime"), &mut files);
        files
    }

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

        for path in runtime_source_files(manifest) {
            let relative = path
                .strip_prefix(manifest)
                .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
            let Ok(source) = std::fs::read_to_string(&path) else {
                violations.push(format!("{relative}: unreadable"));
                continue;
            };
            let stripped = strip_test_modules(&source);
            let lines: Vec<&str> = stripped.lines().collect();

            for (line_index, line) in lines.iter().enumerate() {
                // Concatenate the trigger line plus the following two so
                // multi-line calls still expose their arguments.
                let mut window = String::from(*line);
                for follow in lines.iter().skip(line_index + 1).take(2) {
                    window.push(' ');
                    window.push_str(follow);
                }

                // Scan each trigger occurrence separately so a line carrying
                // both envelope kinds classifies every call against its own
                // kind instead of the first trigger's.
                let mut triggers = Vec::new();
                for (trigger, kind) in request_triggers
                    .iter()
                    .map(|t| (*t, EnvelopeKind::Request))
                    .chain(notification_triggers.iter().map(|t| (*t, EnvelopeKind::Notification)))
                {
                    let mut from = 0usize;
                    while let Some(offset) = line[from..].find(trigger) {
                        triggers.push((from + offset + trigger.len(), kind));
                        from += offset + trigger.len();
                    }
                }

                for (trigger_end, required_kind) in triggers {
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
                            match lookup(&method) {
                                Some(descriptor)
                                    if descriptor.direction == MethodDirection::ServerToClient
                                        && descriptor.kind == required_kind => {}
                                Some(descriptor) => violations.push(format!(
                                    "{relative}: outbound `{method}` is registered {:?}/{:?}",
                                    descriptor.direction, descriptor.kind
                                )),
                                None => violations.push(format!(
                                    "{relative}: outbound `{method}` is unclassified"
                                )),
                            }
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
