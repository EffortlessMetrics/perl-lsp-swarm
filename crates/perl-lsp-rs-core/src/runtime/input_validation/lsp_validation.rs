use crate::protocol::capabilities::SUPPORTED_COMMANDS;
use crate::runtime::input_validation::constants::{
    ALLOWED_COMMANDS, ALLOWED_TEXT_DOCUMENT_URI_SCHEMES, MAX_METHOD_LENGTH, MAX_PARAMS_SIZE,
    MAX_URI_LENGTH,
};
use crate::runtime::input_validation::file_validation::validate_file_content;
use anyhow::{Result, anyhow};
use std::path::Path;

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
    if params_str.len() > MAX_PARAMS_SIZE {
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
