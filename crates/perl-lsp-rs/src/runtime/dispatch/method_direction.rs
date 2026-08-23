//! Checked method-direction authority (#8896).
//!
//! One reviewed table answers, for every executable LSP and perl-lsp method,
//! which side of the connection may originate it and in which envelope kind.
//! Inbound routing consumes [`inbound_admission`] before any application
//! dispatch; the common outbound seams (`LspServer::send_request` /
//! `LspServer::notify`) consume [`outbound_admission`] before a frame can be
//! written.
//!
//! The table is deliberately local to the runtime that owns dispatch and
//! outbound construction. The canonical protocol-schema substrate
//! (`perl-lsp-rs-core::protocol::schema`) still covers only the lifecycle,
//! cancellation, and window-message families (#10477 owns completing it), so
//! this is the issue-sanctioned "small reviewed table with an explicit
//! migration path": a drift test pins agreement with every identity the core
//! substrate already registers, and the completeness tests below fail when an
//! inbound route or a production outbound call site appears without a table
//! row.

use std::fmt;

/// Which side of the connection may originate a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodDirection {
    /// Only the client (editor) sends this method to `perllsp`.
    ClientToServer,
    /// Only `perllsp` sends this method to the client.
    ServerToClient,
    /// Either party sends this method (currently only `$/cancelRequest`).
    Bidirectional,
}

impl MethodDirection {
    const fn allows_client(self) -> bool {
        matches!(self, Self::ClientToServer | Self::Bidirectional)
    }

    const fn allows_server(self) -> bool {
        matches!(self, Self::ServerToClient | Self::Bidirectional)
    }

    #[cfg(test)]
    fn from_schema_token(token: &str) -> Option<Self> {
        match token {
            "client_to_server" => Some(Self::ClientToServer),
            "server_to_client" => Some(Self::ServerToClient),
            _ => None,
        }
    }

    const fn token(self) -> &'static str {
        match self {
            Self::ClientToServer => "client_to_server",
            Self::ServerToClient => "server_to_client",
            Self::Bidirectional => "bidirectional",
        }
    }
}

/// JSON-RPC envelope kind a method uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvelopeKind {
    /// Method travels as a request carrying an ID.
    Request,
    /// Method travels as a notification without an ID.
    Notification,
}

impl EnvelopeKind {
    pub(crate) const fn wire_name(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Notification => "notification",
        }
    }

    const fn token(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Notification => "notification",
        }
    }

    #[cfg(test)]
    fn from_schema_token(token: &str) -> Option<Self> {
        match token {
            "request" => Some(Self::Request),
            "notification" => Some(Self::Notification),
            _ => None,
        }
    }
}

/// Whether a method comes from the standard LSP surface or is a perl-lsp
/// extension identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodOrigin {
    /// Stable LSP 3.17 method.
    StandardLsp317,
    /// Individually selected LSP 3.18-development method.
    StandardLsp318Development,
    /// Project-specific extension method (`perl/*`, `experimental/*`,
    /// `$ /perl-lsp/*`, or a gated test endpoint).
    ProjectExtension,
}

/// Lifecycle phase constraint recorded for review; lifecycle admission itself
/// stays owned by the existing preflight gates (#1403 owns any redesign).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecyclePhase {
    /// No phase constraint beyond normal admission.
    AnyPhase,
    /// Initialization handshake (`initialize`, `initialized`).
    InitializationHandshake,
    /// Terminal lifecycle (`shutdown`, `exit`).
    TerminalLifecycle,
}

/// One checked method row.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MethodDirectionEntry {
    /// Wire method name.
    pub(crate) method: &'static str,
    /// Envelope kind the method uses.
    pub(crate) kind: EnvelopeKind,
    /// Allowed originating side(s).
    pub(crate) direction: MethodDirection,
    /// Standard vs project-extension identity.
    pub(crate) origin: MethodOrigin,
    /// Lifecycle phase constraint.
    pub(crate) phase: LifecyclePhase,
    /// Owning inbound handler or outbound constructor.
    pub(crate) owner: &'static str,
}

const fn c2s_request(
    method: &'static str,
    origin: MethodOrigin,
    owner: &'static str,
) -> MethodDirectionEntry {
    MethodDirectionEntry {
        method,
        kind: EnvelopeKind::Request,
        direction: MethodDirection::ClientToServer,
        origin,
        phase: LifecyclePhase::AnyPhase,
        owner,
    }
}

const fn c2s_notification(
    method: &'static str,
    origin: MethodOrigin,
    owner: &'static str,
) -> MethodDirectionEntry {
    MethodDirectionEntry {
        method,
        kind: EnvelopeKind::Notification,
        direction: MethodDirection::ClientToServer,
        origin,
        phase: LifecyclePhase::AnyPhase,
        owner,
    }
}

const fn s2c_request(method: &'static str, owner: &'static str) -> MethodDirectionEntry {
    MethodDirectionEntry {
        method,
        kind: EnvelopeKind::Request,
        direction: MethodDirection::ServerToClient,
        origin: MethodOrigin::StandardLsp317,
        phase: LifecyclePhase::AnyPhase,
        owner,
    }
}

const fn s2c_notification(
    method: &'static str,
    origin: MethodOrigin,
    owner: &'static str,
) -> MethodDirectionEntry {
    MethodDirectionEntry {
        method,
        kind: EnvelopeKind::Notification,
        direction: MethodDirection::ServerToClient,
        origin,
        phase: LifecyclePhase::AnyPhase,
        owner,
    }
}

const fn lifecycle(mut entry: MethodDirectionEntry, phase: LifecyclePhase) -> MethodDirectionEntry {
    entry.phase = phase;
    entry
}

/// Every executable method this server knows, in one place.
///
/// Client-origin rows must cover every arm of `routing.rs`; server-origin rows
/// must cover every production outbound call site. The completeness tests at
/// the bottom of this module mechanically enforce both inventories.
static METHOD_DIRECTIONS: &[MethodDirectionEntry] = &[
    // ---- Lifecycle -------------------------------------------------------
    lifecycle(
        c2s_request("initialize", MethodOrigin::StandardLsp317, "inbound:routing initialize"),
        LifecyclePhase::InitializationHandshake,
    ),
    lifecycle(
        c2s_notification(
            "initialized",
            MethodOrigin::StandardLsp317,
            "inbound:routing initialized",
        ),
        LifecyclePhase::InitializationHandshake,
    ),
    lifecycle(
        c2s_request("shutdown", MethodOrigin::StandardLsp317, "inbound:routing shutdown"),
        LifecyclePhase::TerminalLifecycle,
    ),
    lifecycle(
        c2s_notification("exit", MethodOrigin::StandardLsp317, "inbound:routing exit"),
        LifecyclePhase::TerminalLifecycle,
    ),
    // ---- Base protocol ---------------------------------------------------
    MethodDirectionEntry {
        method: "$/cancelRequest",
        kind: EnvelopeKind::Notification,
        direction: MethodDirection::Bidirectional,
        origin: MethodOrigin::StandardLsp317,
        phase: LifecyclePhase::AnyPhase,
        owner: "inbound:preflight handle_cancel_notification / outbound:none",
    },
    c2s_request(
        "$/test/slowOperation",
        MethodOrigin::ProjectExtension,
        "inbound:routing slowOperation (test/expose_lsp_test_api gate)",
    ),
    // ---- Text/document synchronization ----------------------------------
    c2s_notification(
        "textDocument/didOpen",
        MethodOrigin::StandardLsp317,
        "inbound:routing didOpen",
    ),
    c2s_notification(
        "textDocument/didChange",
        MethodOrigin::StandardLsp317,
        "inbound:routing didChange",
    ),
    c2s_notification(
        "textDocument/didClose",
        MethodOrigin::StandardLsp317,
        "inbound:routing didClose",
    ),
    c2s_notification(
        "textDocument/didSave",
        MethodOrigin::StandardLsp317,
        "inbound:routing didSave",
    ),
    c2s_notification(
        "textDocument/willSave",
        MethodOrigin::StandardLsp317,
        "inbound:routing willSave",
    ),
    c2s_request(
        "textDocument/willSaveWaitUntil",
        MethodOrigin::StandardLsp317,
        "inbound:routing willSaveWaitUntil",
    ),
    // ---- Notebook synchronization ----------------------------------------
    c2s_notification(
        "notebookDocument/didOpen",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing notebook didOpen",
    ),
    c2s_notification(
        "notebookDocument/didChange",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing notebook didChange",
    ),
    c2s_notification(
        "notebookDocument/didSave",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing notebook didSave",
    ),
    c2s_notification(
        "notebookDocument/didClose",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing notebook didClose",
    ),
    // ---- Language features -----------------------------------------------
    c2s_request(
        "textDocument/completion",
        MethodOrigin::StandardLsp317,
        "inbound:routing completion",
    ),
    c2s_request(
        "completionItem/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing completion resolve",
    ),
    c2s_request("textDocument/hover", MethodOrigin::StandardLsp317, "inbound:routing hover"),
    c2s_request(
        "textDocument/signatureHelp",
        MethodOrigin::StandardLsp317,
        "inbound:routing signatureHelp",
    ),
    c2s_request(
        "textDocument/declaration",
        MethodOrigin::StandardLsp317,
        "inbound:routing declaration",
    ),
    c2s_request(
        "textDocument/definition",
        MethodOrigin::StandardLsp317,
        "inbound:routing definition",
    ),
    c2s_request(
        "textDocument/typeDefinition",
        MethodOrigin::StandardLsp317,
        "inbound:routing typeDefinition",
    ),
    c2s_request(
        "textDocument/implementation",
        MethodOrigin::StandardLsp317,
        "inbound:routing implementation",
    ),
    c2s_request(
        "textDocument/references",
        MethodOrigin::StandardLsp317,
        "inbound:routing references",
    ),
    c2s_request(
        "textDocument/documentHighlight",
        MethodOrigin::StandardLsp317,
        "inbound:routing documentHighlight",
    ),
    c2s_request(
        "textDocument/documentSymbol",
        MethodOrigin::StandardLsp317,
        "inbound:routing documentSymbol",
    ),
    c2s_request(
        "textDocument/selectionRange",
        MethodOrigin::StandardLsp317,
        "inbound:routing selectionRange",
    ),
    c2s_request(
        "textDocument/foldingRange",
        MethodOrigin::StandardLsp317,
        "inbound:routing foldingRange",
    ),
    c2s_request(
        "textDocument/codeAction",
        MethodOrigin::StandardLsp317,
        "inbound:routing codeAction",
    ),
    c2s_request(
        "codeAction/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing codeAction resolve",
    ),
    c2s_request("textDocument/codeLens", MethodOrigin::StandardLsp317, "inbound:routing codeLens"),
    c2s_request(
        "codeLens/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing codeLens resolve",
    ),
    c2s_request(
        "textDocument/documentLink",
        MethodOrigin::StandardLsp317,
        "inbound:routing documentLink",
    ),
    c2s_request(
        "documentLink/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing documentLink resolve",
    ),
    c2s_request(
        "textDocument/documentColor",
        MethodOrigin::StandardLsp317,
        "inbound:routing documentColor",
    ),
    c2s_request(
        "textDocument/colorPresentation",
        MethodOrigin::StandardLsp317,
        "inbound:routing colorPresentation",
    ),
    c2s_request(
        "textDocument/formatting",
        MethodOrigin::StandardLsp317,
        "inbound:routing formatting",
    ),
    c2s_request(
        "textDocument/rangeFormatting",
        MethodOrigin::StandardLsp317,
        "inbound:routing rangeFormatting",
    ),
    c2s_request(
        "textDocument/rangesFormatting",
        MethodOrigin::ProjectExtension,
        "inbound:routing rangesFormatting alias",
    ),
    c2s_request(
        "textDocument/onTypeFormatting",
        MethodOrigin::StandardLsp317,
        "inbound:routing onTypeFormatting",
    ),
    c2s_request("textDocument/rename", MethodOrigin::StandardLsp317, "inbound:routing rename"),
    c2s_request(
        "textDocument/prepareRename",
        MethodOrigin::StandardLsp317,
        "inbound:routing prepareRename",
    ),
    c2s_request(
        "textDocument/linkedEditingRange",
        MethodOrigin::StandardLsp317,
        "inbound:routing linkedEditingRange",
    ),
    c2s_request("textDocument/moniker", MethodOrigin::StandardLsp317, "inbound:routing moniker"),
    c2s_request(
        "textDocument/prepareCallHierarchy",
        MethodOrigin::StandardLsp317,
        "inbound:routing prepareCallHierarchy",
    ),
    c2s_request(
        "callHierarchy/incomingCalls",
        MethodOrigin::StandardLsp317,
        "inbound:routing incomingCalls",
    ),
    c2s_request(
        "callHierarchy/outgoingCalls",
        MethodOrigin::StandardLsp317,
        "inbound:routing outgoingCalls",
    ),
    c2s_request(
        "textDocument/prepareTypeHierarchy",
        MethodOrigin::StandardLsp317,
        "inbound:routing prepareTypeHierarchy",
    ),
    c2s_request(
        "typeHierarchy/prepare",
        MethodOrigin::ProjectExtension,
        "inbound:routing typeHierarchy prepare alias",
    ),
    c2s_request(
        "typeHierarchy/supertypes",
        MethodOrigin::ProjectExtension,
        "inbound:routing typeHierarchy supertypes alias",
    ),
    c2s_request(
        "typeHierarchy/subtypes",
        MethodOrigin::ProjectExtension,
        "inbound:routing typeHierarchy subtypes alias",
    ),
    c2s_request(
        "textDocument/semanticTokens/full",
        MethodOrigin::StandardLsp317,
        "inbound:routing semanticTokens full",
    ),
    c2s_request(
        "textDocument/semanticTokens/full/delta",
        MethodOrigin::StandardLsp317,
        "inbound:routing semanticTokens delta",
    ),
    c2s_request(
        "textDocument/semanticTokens/range",
        MethodOrigin::StandardLsp317,
        "inbound:routing semanticTokens range",
    ),
    c2s_request(
        "textDocument/inlayHint",
        MethodOrigin::StandardLsp317,
        "inbound:routing inlayHint",
    ),
    c2s_request(
        "inlayHint/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing inlayHint resolve",
    ),
    c2s_request(
        "textDocument/inlineValue",
        MethodOrigin::StandardLsp317,
        "inbound:routing inlineValue",
    ),
    c2s_request(
        "textDocument/inlineCompletion",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing inlineCompletion",
    ),
    c2s_request(
        "textDocument/perlInlineCompletionStream",
        MethodOrigin::ProjectExtension,
        "inbound:routing perlInlineCompletionStream",
    ),
    c2s_request(
        "textDocument/diagnostic",
        MethodOrigin::StandardLsp317,
        "inbound:routing document diagnostic",
    ),
    // ---- Workspace features ----------------------------------------------
    c2s_request(
        "workspace/symbol",
        MethodOrigin::StandardLsp317,
        "inbound:routing workspace symbol",
    ),
    c2s_request(
        "workspace/symbol/resolve",
        MethodOrigin::StandardLsp317,
        "inbound:routing workspace symbol resolve",
    ),
    c2s_request(
        "workspace/executeCommand",
        MethodOrigin::StandardLsp317,
        "inbound:routing executeCommand",
    ),
    c2s_request(
        "workspace/diagnostic",
        MethodOrigin::StandardLsp317,
        "inbound:routing workspace diagnostic",
    ),
    c2s_notification(
        "workspace/didChangeConfiguration",
        MethodOrigin::StandardLsp317,
        "inbound:routing didChangeConfiguration",
    ),
    c2s_notification(
        "workspace/didChangeWatchedFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing didChangeWatchedFiles",
    ),
    c2s_notification(
        "workspace/didChangeWorkspaceFolders",
        MethodOrigin::StandardLsp317,
        "inbound:routing didChangeWorkspaceFolders",
    ),
    c2s_request(
        "workspace/willCreateFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing willCreateFiles",
    ),
    c2s_notification(
        "workspace/didCreateFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing didCreateFiles",
    ),
    c2s_request(
        "workspace/willRenameFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing willRenameFiles",
    ),
    c2s_notification(
        "workspace/didRenameFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing didRenameFiles",
    ),
    c2s_request(
        "workspace/willDeleteFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing willDeleteFiles",
    ),
    c2s_notification(
        "workspace/didDeleteFiles",
        MethodOrigin::StandardLsp317,
        "inbound:routing didDeleteFiles",
    ),
    c2s_request(
        "workspace/textDocumentContent",
        MethodOrigin::StandardLsp318Development,
        "inbound:routing workspace textDocumentContent",
    ),
    // ---- perl-lsp extension methods --------------------------------------
    c2s_request("perl/showAst", MethodOrigin::ProjectExtension, "inbound:routing showAst"),
    c2s_request(
        "experimental/testDiscovery",
        MethodOrigin::ProjectExtension,
        "inbound:routing testDiscovery",
    ),
    // Response carrier for server-initiated requests until #7010 replaces the
    // transport shim with registry-owned correlation. Deliberately a distinct
    // project identity; never a license to overload a standard method name.
    c2s_notification(
        "$/perl-lsp/clientResponse",
        MethodOrigin::ProjectExtension,
        "inbound:routing handle_client_response (#7010 disposition pending)",
    ),
    c2s_request(
        "$/perl-lsp/watchdog",
        MethodOrigin::ProjectExtension,
        "inbound:routing watchdog liveness probe",
    ),
    // ---- Progress / trace -------------------------------------------------
    c2s_notification("$/setTrace", MethodOrigin::StandardLsp317, "inbound:routing setTrace"),
    s2c_notification("$/logTrace", MethodOrigin::StandardLsp317, "outbound:lifecycle logTrace"),
    s2c_notification("$/progress", MethodOrigin::StandardLsp317, "outbound:window progress"),
    c2s_notification(
        "window/workDoneProgress/cancel",
        MethodOrigin::StandardLsp317,
        "inbound:routing progress cancel",
    ),
    // ---- Server→client requests (never admissible inbound) ---------------
    s2c_request("client/registerCapability", "outbound:lifecycle watchers registerCapability"),
    s2c_request("client/unregisterCapability", "outbound:unassigned (reserved standard identity)"),
    s2c_request("workspace/configuration", "outbound:workspace configuration pull"),
    s2c_request("workspace/applyEdit", "outbound:request_apply_workspace_edit_with_metadata"),
    s2c_request("window/showMessageRequest", "outbound:window showMessageRequest"),
    s2c_request("window/showDocument", "outbound:window showDocument"),
    s2c_request("window/workDoneProgress/create", "outbound:window workDoneProgress create"),
    s2c_request("workspace/codeLens/refresh", "outbound:request_code_lens_refresh"),
    s2c_request("workspace/semanticTokens/refresh", "outbound:request_semantic_tokens_refresh"),
    s2c_request("workspace/inlayHint/refresh", "outbound:request_inlay_hint_refresh"),
    s2c_request("workspace/inlineValue/refresh", "outbound:request_inline_value_refresh"),
    s2c_request("workspace/diagnostic/refresh", "outbound:request_diagnostic_refresh"),
    s2c_request("workspace/foldingRange/refresh", "outbound:request_folding_range_refresh"),
    s2c_request(
        "workspace/textDocumentContent/refresh",
        "outbound:virtual_content refresh request",
    ),
    // ---- Server→client notifications -------------------------------------
    s2c_notification(
        "textDocument/publishDiagnostics",
        MethodOrigin::StandardLsp317,
        "outbound:text_sync diagnostics publish",
    ),
    s2c_notification(
        "window/showMessage",
        MethodOrigin::StandardLsp317,
        "outbound:window showMessage",
    ),
    s2c_notification(
        "window/logMessage",
        MethodOrigin::StandardLsp317,
        "outbound:window logMessage",
    ),
    s2c_notification(
        "telemetry/event",
        MethodOrigin::StandardLsp317,
        "outbound:window telemetry event",
    ),
    s2c_notification(
        "perl-lsp/index-ready",
        MethodOrigin::ProjectExtension,
        "outbound:workspace_progress index ready",
    ),
    s2c_notification(
        "perl-lsp/active-document-ready",
        MethodOrigin::ProjectExtension,
        "outbound:workspace_progress active document ready",
    ),
];

/// Outcome of checking one inbound message against the authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundAdmission {
    /// A registered client-origin method or an unknown method; continue to
    /// normal routing (unknown methods keep their ordinary fall-through).
    Admit,
    /// Registered server-origin method received as a request: answer
    /// `-32601 MethodNotFound` without touching application state.
    RejectRequest,
    /// Registered server-origin method received as a notification: emit no
    /// response and run no application handler.
    IgnoreNotification,
}

/// Classify an inbound client-originated frame by method name.
///
/// The wire shape decides request vs notification through `has_id`. Unknown
/// methods admit so existing tolerant fall-through behavior is preserved;
/// registration is required only for rejection decisions and mechanical
/// inventory checks.
pub(crate) fn inbound_admission(method: &str, has_id: bool) -> InboundAdmission {
    let entries: Vec<_> = METHOD_DIRECTIONS.iter().filter(|entry| entry.method == method).collect();
    if entries.iter().any(|entry| entry.direction.allows_client()) {
        return InboundAdmission::Admit;
    }
    if entries.is_empty() {
        return InboundAdmission::Admit;
    }
    if has_id { InboundAdmission::RejectRequest } else { InboundAdmission::IgnoreNotification }
}

/// Failure explaining why an outbound method was refused before emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutboundDirectionError {
    method: String,
    kind: EnvelopeKind,
}

impl fmt::Display for OutboundDirectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "method `{}` is not registered as a server-to-client {} in the \
             #8896 method-direction authority; register it there with its \
             owning constructor before emitting it (canonical schema migration: \
             #10477/#7116)",
            self.method,
            self.kind.wire_name()
        )
    }
}

impl std::error::Error for OutboundDirectionError {}

/// Verify a method may be emitted toward the client in the given envelope
/// kind. Fails closed: unregistered methods never leave as frames.
pub(crate) fn outbound_admission(
    method: &str,
    kind: EnvelopeKind,
) -> Result<(), OutboundDirectionError> {
    let admitted = METHOD_DIRECTIONS.iter().any(|entry| {
        entry.method == method && entry.kind == kind && entry.direction.allows_server()
    });
    if admitted { Ok(()) } else { Err(OutboundDirectionError { method: method.to_string(), kind }) }
}

/// Deterministic `direction:kind:method:origin:owner` identities for drift
/// checks by tests and future consumers such as the provider contract registry
/// (#8068).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "#8068 consumes this projection when its registry wires up")
)]
pub(crate) fn registered_method_identities() -> Vec<String> {
    let mut identities: Vec<String> = METHOD_DIRECTIONS
        .iter()
        .map(|entry| {
            let origin = match entry.origin {
                MethodOrigin::StandardLsp317 => "lsp-3.17",
                MethodOrigin::StandardLsp318Development => "lsp-3.18-development",
                MethodOrigin::ProjectExtension => "perl-lsp-extension",
            };
            format!(
                "{}:{}:{}:{}:{}",
                entry.direction.token(),
                entry.kind.token(),
                entry.method,
                origin,
                entry.owner
            )
        })
        .collect();
    identities.sort();
    identities.dedup();
    identities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_reversed_standard_methods_stay_server_to_client() {
        for method in [
            "workspace/applyEdit",
            "workspace/configuration",
            "client/registerCapability",
            "client/unregisterCapability",
        ] {
            assert_eq!(
                inbound_admission(method, true),
                InboundAdmission::RejectRequest,
                "{method} must never be admissible as an inbound request"
            );
            assert_eq!(
                inbound_admission(method, false),
                InboundAdmission::IgnoreNotification,
                "{method} must never be admissible as an inbound notification"
            );
        }
    }

    #[test]
    fn server_to_client_notifications_are_ignored_without_dispatch() {
        for method in [
            "window/showMessage",
            "window/logMessage",
            "$/progress",
            "$/logTrace",
            "telemetry/event",
            "textDocument/publishDiagnostics",
        ] {
            assert_eq!(
                inbound_admission(method, false),
                InboundAdmission::IgnoreNotification,
                "{method} sent by the client must be ignored"
            );
        }
    }

    #[test]
    fn ordinary_client_to_server_traffic_is_admitted() {
        assert_eq!(inbound_admission("initialize", true), InboundAdmission::Admit);
        assert_eq!(inbound_admission("initialized", false), InboundAdmission::Admit);
        assert_eq!(inbound_admission("textDocument/hover", true), InboundAdmission::Admit);
        assert_eq!(
            inbound_admission("workspace/didChangeConfiguration", false),
            InboundAdmission::Admit
        );
        assert_eq!(inbound_admission("$/perl-lsp/clientResponse", false), InboundAdmission::Admit);
    }

    #[test]
    fn bidirectional_cancel_and_unknown_methods_admit() {
        assert_eq!(inbound_admission("$/cancelRequest", false), InboundAdmission::Admit);
        assert_eq!(
            inbound_admission("totally/custom/method", true),
            InboundAdmission::Admit,
            "unknown methods keep their normal routing fall-through"
        );
    }

    #[test]
    fn outbound_seams_accept_only_registered_server_to_client_methods() -> Result<(), String> {
        assert!(outbound_admission("workspace/configuration", EnvelopeKind::Request).is_ok());
        assert!(outbound_admission("workspace/applyEdit", EnvelopeKind::Request).is_ok());
        assert!(outbound_admission("client/registerCapability", EnvelopeKind::Request).is_ok());
        assert!(outbound_admission("$/progress", EnvelopeKind::Notification).is_ok());
        assert!(
            outbound_admission("textDocument/publishDiagnostics", EnvelopeKind::Notification)
                .is_ok()
        );

        // A client→server method must not become an outbound frame.
        let Err(hover) = outbound_admission("textDocument/hover", EnvelopeKind::Request) else {
            return Err("hover is client-origin and must be refused outbound".to_string());
        };
        assert!(hover.to_string().contains("textDocument/hover"));
        let Err(did_open) = outbound_admission("textDocument/didOpen", EnvelopeKind::Notification)
        else {
            return Err("didOpen is client-origin and must be refused outbound".to_string());
        };
        assert!(did_open.to_string().contains("didOpen"));

        // Envelope-kind confusion fails closed too.
        assert!(
            outbound_admission("$/progress", EnvelopeKind::Request).is_err(),
            "a notification-only method must not be emitted as a request"
        );
        Ok(())
    }

    /// The #8068/#7116 projection carries direction, kind, origin class, and
    /// the owning constructor for every row.
    #[test]
    fn projection_identities_expose_origin_and_owner() {
        let identities = registered_method_identities();
        assert!(
            identities.iter().any(|identity| identity
                .starts_with("server_to_client:request:workspace/applyEdit:lsp-3.17:outbound:")),
            "applyEdit must project as an s2c request with its outbound owner: {identities:?}"
        );
        assert!(
            identities.iter().any(|identity| identity.starts_with(
                "client_to_server:notification:$/perl-lsp/clientResponse:perl-lsp-extension:"
            )),
            "the response carrier must project as an explicit extension row: {identities:?}"
        );
    }

    #[test]
    fn lifecycle_rows_carry_phase_constraints() {
        let phase = |method: &str| {
            METHOD_DIRECTIONS.iter().find(|entry| entry.method == method).map(|entry| entry.phase)
        };
        assert_eq!(phase("initialize"), Some(LifecyclePhase::InitializationHandshake));
        assert_eq!(phase("initialized"), Some(LifecyclePhase::InitializationHandshake));
        assert_eq!(phase("shutdown"), Some(LifecyclePhase::TerminalLifecycle));
        assert_eq!(phase("exit"), Some(LifecyclePhase::TerminalLifecycle));
        assert_eq!(phase("textDocument/hover"), Some(LifecyclePhase::AnyPhase));
    }

    /// The runtime projection must agree with every identity the canonical
    /// schema substrate already registers (#7113). This is the explicit
    /// migration path: when #10477 completes the core registry, each new
    /// identity lands here first through this failing check.
    #[test]
    fn projection_agrees_with_core_protocol_schema_registry() -> Result<(), String> {
        let core = perl_lsp_rs_core::protocol::schema::registered_schema_identities();
        assert!(!core.is_empty(), "core schema registry must expose identities");
        for identity in &core {
            // Identity shape: `direction:kind:method:version`.
            let mut parts = identity.splitn(4, ':');
            let Some(direction_token) = parts.next() else {
                return Err(format!("malformed core schema identity: {identity}"));
            };
            let Some(kind_token) = parts.next() else {
                return Err(format!("malformed core schema identity: {identity}"));
            };
            let Some(method) = parts.next() else {
                return Err(format!("malformed core schema identity: {identity}"));
            };

            let Some(direction) = MethodDirection::from_schema_token(direction_token) else {
                return Err(format!("unknown direction token in {identity}"));
            };
            let Some(kind) = EnvelopeKind::from_schema_token(kind_token) else {
                return Err(format!("unknown envelope-kind token in {identity}"));
            };

            let matched = METHOD_DIRECTIONS.iter().any(|entry| {
                entry.method == method
                    && entry.kind == kind
                    && match direction {
                        MethodDirection::ClientToServer => entry.direction.allows_client(),
                        MethodDirection::ServerToClient => entry.direction.allows_server(),
                        MethodDirection::Bidirectional => false,
                    }
            });
            assert!(matched, "runtime authority disagrees with core schema identity {identity}");
        }
        Ok(())
    }

    /// Every method matched by an inbound routing arm must be a registered
    /// client-origin row, and every client-origin row must have a live arm.
    /// Re-adding a wrong-direction standard arm fails here unless its row is
    /// flipped — and the pinned-direction test above fails on that flip.
    #[test]
    fn routing_arms_and_client_origin_rows_are_complete() -> Result<(), String> {
        let arms = routing_arm_methods(include_str!("routing.rs"));
        assert!(!arms.is_empty(), "routing arm extraction found no methods");

        let mut client_rows: Vec<&str> = METHOD_DIRECTIONS
            .iter()
            // Bidirectional rows (e.g. `$/cancelRequest`) are consumed before
            // method routing by preflight cancellation handling, so only
            // strictly client→server rows must own a routing arm.
            .filter(|entry| entry.direction == MethodDirection::ClientToServer)
            .map(|entry| entry.method)
            .collect();
        client_rows.sort_unstable();
        client_rows.dedup();

        let mut sorted_arms = arms.clone();
        sorted_arms.sort();
        sorted_arms.dedup();

        for arm in &sorted_arms {
            let admitted = METHOD_DIRECTIONS
                .iter()
                .any(|entry| entry.method == *arm && entry.direction.allows_client());
            assert!(
                admitted,
                "routing arm `{arm}` has no client-origin row in the direction authority"
            );
        }
        for row in &client_rows {
            assert!(
                sorted_arms.iter().any(|arm| arm == row),
                "client-origin row `{row}` has no inbound routing arm"
            );
        }
        Ok(())
    }

    /// Every production outbound call site must name a registered
    /// server-to-client method of the matching envelope kind. A new stringly
    /// typed emission without registry ownership fails this check instead of
    /// shipping a frame.
    #[test]
    fn outbound_call_sites_are_registry_checked() -> Result<(), String> {
        const NOTIFICATION_MARKERS: [&str; 2] = [".send_notification(", ".notify("];
        const REQUEST_MARKERS: [&str; 2] = [".send_request(", ".send_request_internal("];
        const SCAN_WINDOW: usize = 160;

        let sources: [(&str, &str); 14] = [
            ("../client_requests.rs", include_str!("../client_requests.rs")),
            ("../window.rs", include_str!("../window.rs")),
            ("../workspace_progress.rs", include_str!("../workspace_progress.rs")),
            ("../workspace.rs", include_str!("../workspace.rs")),
            ("../mod.rs", include_str!("../mod.rs")),
            ("../text_sync.rs", include_str!("../text_sync.rs")),
            ("../text_sync/lifecycle.rs", include_str!("../text_sync/lifecycle.rs")),
            ("../diagnostics.rs", include_str!("../diagnostics.rs")),
            ("lifecycle.rs", include_str!("lifecycle.rs")),
            ("../lifecycle/mod.rs", include_str!("../lifecycle/mod.rs")),
            ("../lifecycle/workspace.rs", include_str!("../lifecycle/workspace.rs")),
            ("../lifecycle/watchers.rs", include_str!("../lifecycle/watchers.rs")),
            ("../language/virtual_content.rs", include_str!("../language/virtual_content.rs")),
            ("../language/streaming.rs", include_str!("../language/streaming.rs")),
        ];

        for (file, source) in sources {
            for (markers, kind) in [
                (&NOTIFICATION_MARKERS, EnvelopeKind::Notification),
                (&REQUEST_MARKERS, EnvelopeKind::Request),
            ] {
                for marker in markers {
                    let mut cursor = 0usize;
                    while let Some(offset) = source[cursor..].find(marker) {
                        let after = cursor + offset + marker.len();
                        let rest = &source[after.min(source.len())..];
                        let window = rest.get(..SCAN_WINDOW.min(rest.len())).unwrap_or("");
                        // Skip non-literal arguments and log/message strings:
                        // only method-shaped literals are emission sites.
                        if let Some(method) = first_quoted(window)
                            && looks_like_method_name(method)
                            && !method.contains(' ')
                        {
                            outbound_admission(method, kind).map_err(|error| {
                                format!("{file}:{marker} emits `{method}`: {error}")
                            })?;
                        }
                        cursor = after;
                    }
                }
            }
        }
        Ok(())
    }

    /// Method-shaped literal heuristic shared by both inventory scans:
    /// lifecycle names, anything with a `/`, or a non-trivial `$/` method.
    fn looks_like_method_name(literal: &str) -> bool {
        matches!(literal, "initialize" | "initialized" | "shutdown" | "exit")
            || literal.contains('/') && literal.len() > 2
            || literal.starts_with("$/") && literal.len() > 2
    }

    /// Extract method-shaped string literals from the pattern side of every
    /// `match method.as_str()` table in the routing source. Cumulative brace
    /// depth (tracked outside string literals) closes each table exactly when
    /// its match block ends, so arm bodies never leak into other tables.
    fn routing_arm_methods(source: &str) -> Vec<String> {
        // (net brace delta outside strings, saw `=>` outside strings)
        fn scan_line(line: &str) -> (i64, bool) {
            let bytes = line.as_bytes();
            let mut delta = 0_i64;
            let mut arrow = false;
            let mut in_string = false;
            let mut index = 0usize;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => in_string = !in_string,
                    b'=' if !in_string && bytes.get(index + 1) == Some(&b'>') => {
                        arrow = true;
                        index += 1;
                    }
                    b'{' if !in_string => delta += 1,
                    b'}' if !in_string => delta -= 1,
                    _ => {}
                }
                index += 1;
            }
            (delta, arrow)
        }

        fn collect(text: &str, methods: &mut Vec<String>) {
            let bytes = text.as_bytes();
            let mut index = 0usize;
            while index < bytes.len() {
                if bytes[index] == b'"'
                    && let Some(end) = text[index + 1..].find('"')
                {
                    let literal = &text[index + 1..index + 1 + end];
                    if looks_like_method_name(literal) {
                        methods.push(literal.to_string());
                    }
                    index += end + 2;
                } else {
                    index += 1;
                }
            }
        }

        const TABLE_HEAD: &str = "match method.as_str() {";
        let mut methods = Vec::new();
        let mut pattern = String::new();
        let mut in_table = false;
        let mut depth = 0_i64;

        for line in source.lines() {
            let (delta, arrow) = scan_line(line);
            if in_table {
                pattern.push_str(line);
                pattern.push('\n');
                if arrow {
                    collect(&pattern, &mut methods);
                    pattern.clear();
                }
                depth += delta;
                if depth <= 0 {
                    collect(&pattern, &mut methods);
                    pattern.clear();
                    in_table = false;
                }
            } else if line.contains(TABLE_HEAD) && delta > 0 {
                in_table = true;
                depth = delta;
                pattern.clear();
                pattern.push_str(line);
                pattern.push('\n');
            }
        }

        methods.sort();
        methods.dedup();
        methods
    }

    fn first_quoted(text: &str) -> Option<&str> {
        let start = text.find('"')? + 1;
        let end = text[start..].find('"')? + start;
        Some(&text[start..end])
    }
}
