//! Layered request validation for the LSP runtime.
//!
//! Protocol-generic admission lives here ([`validate_request_admission`]);
//! everything method- or application-specific is owned by the operation that
//! hosts the dangerous sink:
//!
//! - document synchronization validates its own URIs through
//!   [`validate_document_uri`] at the
//!   `textDocument/didOpen|didChange|didSave` handlers, and enforces its own
//!   configured buffer-size and binary-content guards where documents are
//!   stored;
//! - `workspace/executeCommand` enforces command identity and argument shape in
//!   the execute-command dispatcher;
//! - path containment, trust, and rendering policy live with their sinks.
//!
//! Generic admission deliberately does not scan parameter *content*: source,
//! documentation, labels, diagnostics, and extension payloads are inert data
//! unless a later operation renders them into an active sink (issue #8895).

use crate::runtime::input_validation::constants::{
    ALLOWED_TEXT_DOCUMENT_URI_SCHEMES, MAX_METHOD_LENGTH, MAX_PARAMS_SIZE, MAX_URI_LENGTH,
};
use crate::runtime::limits::max_file_size_bytes as limits_max_file_size_bytes;
use anyhow::{Result, anyhow};

/// Methods whose params legitimately carry a whole editor buffer.
const TEXT_SYNC_METHODS: &[&str] =
    &["textDocument/didOpen", "textDocument/didChange", "textDocument/didSave"];

/// Serialized-params ceiling for `method`.
///
/// The flat [`MAX_PARAMS_SIZE`] guard is a generic resource bound, but for
/// text-synchronization methods the params *are* the document, and the
/// authority on how large a document may be is the configurable
/// `maxFileSizeBytes` limit enforced by [`validate_file_content`].
///
/// Those two disagreed: `MAX_PARAMS_SIZE` is 1,000,000 while the default file
/// limit is 1,048,576, so a document in that band — or any document at all once
/// an operator *raised* `maxFileSizeBytes` — was rejected here before the
/// configured limit was ever consulted. On `didOpen`/`didChange`, which are
/// notifications, that rejection is silent: the document is simply never
/// stored, with no diagnostic explaining why.
///
/// So text-sync methods get a ceiling derived from the configured file limit,
/// with headroom for JSON envelope and string escaping (worst-case escaping
/// roughly doubles the payload). `validate_file_content` then enforces the real
/// configured limit precisely. Every other method keeps the flat bound.
fn max_params_size_for(method: &str) -> usize {
    if TEXT_SYNC_METHODS.contains(&method) {
        let file_limit = limits_max_file_size_bytes();
        MAX_PARAMS_SIZE.max(file_limit.saturating_mul(2).saturating_add(4_096))
    } else {
        MAX_PARAMS_SIZE
    }
}

/// Admit a decoded JSON-RPC request on generic protocol grounds alone.
///
/// This is the pre-routing structural layer. It proves only that the request is
/// bounded enough to route safely:
///
/// - the method name fits [`MAX_METHOD_LENGTH`] (any JSON-RPC method string is
///   admissible; punctuation is ordinary naming, not an attack);
/// - the serialized params fit the resource ceiling for the method.
///
/// Rejections here are whole-request refusals (`InvalidRequest`, -32600).
/// Unknown methods, wrong parameter shapes, and application policy are *not*
/// this layer's concern: routing answers unknown methods with
/// `MethodNotFound` (-32601), handlers answer malformed params with
/// `InvalidParams` (-32602), and sinks answer policy refusals with their own
/// typed errors.
pub fn validate_request_admission(method: &str, params: &serde_json::Value) -> Result<()> {
    if method.len() > MAX_METHOD_LENGTH {
        return Err(anyhow!("LSP method name too long: {}", method));
    }

    let params_str = serde_json::to_string(params)?;
    if params_str.len() > max_params_size_for(method) {
        return Err(anyhow!("LSP parameters too large for method: {}", method));
    }

    Ok(())
}

/// Validate a document URI at the boundary that turns it into paths.
///
/// The server resolves document URIs into workspace-relative file access, so
/// only schemes it can actually resolve are admitted, and absurdly long URIs
/// are refused as a resource bound. Document-sync handlers call this on the
/// *normalized* key, so plain-path inputs the server deliberately accepts are
/// judged in the form they will actually be stored under.
pub fn validate_document_uri(uri: &str) -> Result<()> {
    if !ALLOWED_TEXT_DOCUMENT_URI_SCHEMES.iter().any(|scheme| uri.starts_with(scheme)) {
        return Err(anyhow!("Invalid URI scheme: {uri}"));
    }

    if uri.len() > MAX_URI_LENGTH {
        return Err(anyhow!("URI too long: {uri}"));
    }

    Ok(())
}
