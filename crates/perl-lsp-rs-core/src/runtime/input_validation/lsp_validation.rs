use crate::protocol::capabilities::SUPPORTED_COMMANDS;
use crate::runtime::input_validation::constants::{
    ALLOWED_COMMANDS, ALLOWED_TEXT_DOCUMENT_URI_SCHEMES, MAX_METHOD_LENGTH, MAX_PARAMS_SIZE,
    MAX_URI_LENGTH,
};
use crate::runtime::input_validation::file_validation::validate_file_content;
use crate::runtime::limits::max_file_size_bytes as limits_max_file_size_bytes;
use anyhow::{Result, anyhow};
use std::path::Path;

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

/// Validates LSP request parameters to ensure they're safe.
pub fn validate_lsp_request(method: &str, params: &serde_json::Value) -> Result<()> {
    if method.len() > MAX_METHOD_LENGTH
        || !method
            .chars()
            .all(|character| character.is_alphanumeric() || character == '/' || character == '$')
    {
        return Err(anyhow!("Invalid LSP method: {}", method));
    }

    let params_str = serde_json::to_string(params)?;
    if params_str.len() > max_params_size_for(method) {
        return Err(anyhow!("LSP parameters too large for method: {}", method));
    }

    match method {
        "textDocument/didOpen" | "textDocument/didChange" | "textDocument/didSave" => {
            validate_text_document_params(params)?;
        }
        "workspace/executeCommand" => {
            validate_execute_command_params(params)?;
        }
        // These methods legitimately carry content derived from the user's own
        // source: `codeAction`'s `context.diagnostics[].message` can quote
        // source text verbatim, and `completionItem/resolve`'s `documentation`
        // can carry POD. A blanket content-pattern scan here would false-positive
        // on ordinary Perl/POD source the same way it did for Mason buffers on
        // `didOpen` (issue #5256 follow-up) — so these are exempted from the
        // catch-all scan below rather than silently rejected.
        // `codeAction/resolve` carries the same server-authored diagnostics back
        // for resolution, so it needs the exemption for the same reason
        // `textDocument/codeAction` does.
        "textDocument/codeAction" | "codeAction/resolve" | "completionItem/resolve" => {}
        _ => {
            if params_str.contains("javascript:") || params_str.contains("<script") {
                return Err(anyhow!("Suspicious content in parameters for method: {}", method));
            }
        }
    }

    Ok(())
}

fn validate_text_document_params(params: &serde_json::Value) -> Result<()> {
    if let Some(uri) = params
        .get("textDocument")
        .and_then(|text_document| text_document.get("uri"))
        .and_then(serde_json::Value::as_str)
    {
        if !ALLOWED_TEXT_DOCUMENT_URI_SCHEMES.iter().any(|scheme| uri.starts_with(scheme)) {
            return Err(anyhow!("Invalid URI scheme: {}", uri));
        }

        if uri.len() > MAX_URI_LENGTH {
            return Err(anyhow!("URI too long: {}", uri));
        }
    }

    if let Some(text) = params
        .get("textDocument")
        .and_then(|text_document| text_document.get("text"))
        .and_then(serde_json::Value::as_str)
    {
        validate_file_content(text, Path::new("<lsp_input>"))?;
    }

    Ok(())
}

/// Returns `true` when `command` is one the server will actually dispatch.
///
/// Two sources, deliberately unioned:
///
/// - [`SUPPORTED_COMMANDS`] is the set advertised in the `executeCommand`
///   capability. Rejecting any of these before dispatch would make the server
///   refuse work it just told the client it could do — with this validator now
///   reachable from preflight, that would have disabled every `run*`/`debug*`
///   command plus `goToTest`, `goToImplementation`, and
///   `explainProviderDecision`.
/// - [`ALLOWED_COMMANDS`] carries handlers that are dispatchable but not
///   advertised (for example `perl.extractVariable`, exercised by the
///   LSP 3.17 workspace and comprehensive e2e suites).
///
/// Anything in neither set is still rejected, which is the point of the check.
fn is_dispatchable_command(command: &str) -> bool {
    SUPPORTED_COMMANDS.contains(&command) || ALLOWED_COMMANDS.contains(&command)
}

fn validate_execute_command_params(params: &serde_json::Value) -> Result<()> {
    if let Some(command) = params.get("command").and_then(serde_json::Value::as_str)
        && !is_dispatchable_command(command)
    {
        return Err(anyhow!("Command not allowed: {}", command));
    }

    if let Some(arguments) = params.get("arguments")
        && !arguments.is_array()
    {
        return Err(anyhow!("Execute command arguments must be an array when present"));
    }

    Ok(())
}
