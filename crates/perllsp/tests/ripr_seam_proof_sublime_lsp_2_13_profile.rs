//! Real-process compatibility contract for Sublime Text's LSP 2.13 client profile.
//!
//! This is intentionally a `*-shaped` protocol receipt. It launches the exact
//! public `perllsp` Cargo binary but does not claim that Sublime Text itself was
//! launched; the actual-host receipt remains owned by #7694.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn sublime_lsp_2_13_capabilities() -> Value {
    json!({
        "general": {
            "positionEncodings": ["utf-16"],
            "markdown": { "parser": "marko" }
        },
        "textDocument": {
            "synchronization": {
                "dynamicRegistration": true,
                "willSave": true,
                "willSaveWaitUntil": true,
                "didSave": true
            },
            "completion": {
                "dynamicRegistration": true,
                "completionItem": {
                    "snippetSupport": true,
                    "documentationFormat": ["markdown", "plaintext"],
                    "deprecatedSupport": true,
                    "insertReplaceSupport": true,
                    "resolveSupport": {
                        "properties": ["detail", "documentation", "additionalTextEdits"]
                    },
                    "labelDetailsSupport": true
                },
                "completionList": {
                    "itemDefaults": ["editRange", "insertTextFormat", "data"]
                }
            },
            "hover": {
                "dynamicRegistration": true,
                "contentFormat": ["markdown", "plaintext"]
            },
            "signatureHelp": {
                "dynamicRegistration": true,
                "signatureInformation": {
                    "documentationFormat": ["markdown", "plaintext"],
                    "parameterInformation": { "labelOffsetSupport": true },
                    "activeParameterSupport": true,
                    "noActiveParameterSupport": true
                },
                "contextSupport": true
            },
            "declaration": { "dynamicRegistration": true, "linkSupport": true },
            "definition": { "dynamicRegistration": true, "linkSupport": true },
            "typeDefinition": { "dynamicRegistration": true, "linkSupport": true },
            "implementation": { "dynamicRegistration": true, "linkSupport": true },
            "rename": {
                "dynamicRegistration": true,
                "prepareSupport": true,
                "prepareSupportDefaultBehavior": 1,
                "honorsChangeAnnotations": true
            },
            "codeAction": {
                "dynamicRegistration": true,
                "dataSupport": true,
                "resolveSupport": { "properties": ["edit"] }
            },
            "semanticTokens": {
                "dynamicRegistration": true,
                "requests": { "range": true, "full": { "delta": true } },
                "tokenTypes": [
                    "namespace", "type", "class", "enum", "interface", "struct",
                    "typeParameter", "parameter", "variable", "property", "enumMember",
                    "event", "function", "method", "macro", "keyword", "modifier",
                    "comment", "string", "number", "regexp", "operator", "decorator"
                ],
                "tokenModifiers": [
                    "declaration", "definition", "readonly", "static", "deprecated",
                    "abstract", "async", "modification", "documentation", "defaultLibrary"
                ],
                "formats": ["relative"],
                "overlappingTokenSupport": false,
                "multilineTokenSupport": true,
                "augmentsSyntaxTokens": true
            },
            "inlayHint": {
                "dynamicRegistration": true,
                "resolveSupport": { "properties": ["textEdits", "label.command"] }
            },
            "diagnostic": {
                "dynamicRegistration": true,
                "relatedDocumentSupport": true,
                "relatedInformation": true,
                "codeDescriptionSupport": true,
                "markupMessageSupport": true,
                "dataSupport": true
            }
        },
        "workspace": {
            "applyEdit": true,
            "workspaceEdit": {
                "documentChanges": true,
                "resourceOperations": ["create", "rename", "delete"],
                "failureHandling": "abort",
                "normalizesLineEndings": true,
                "metadataSupport": true,
                "snippetEditSupport": true
            },
            "didChangeConfiguration": { "dynamicRegistration": true },
            "workspaceFolders": true,
            "configuration": true,
            "semanticTokens": { "refreshSupport": true },
            "codeLens": { "refreshSupport": true },
            "inlayHint": { "refreshSupport": true },
            "diagnostics": { "refreshSupport": true },
            "didChangeWatchedFiles": {
                "dynamicRegistration": true,
                "relativePatternSupport": true
            },
            "fileOperations": {
                "dynamicRegistration": true,
                "willRename": true,
                "didRename": true
            },
            "textDocumentContent": { "dynamicRegistration": true }
        },
        "window": {
            "workDoneProgress": true,
            "showDocument": { "support": true }
        }
    })
}

fn initialize(client: &mut RealProcessClient, capabilities: Value) -> Result<Value> {
    client.request(
        json!("sublime-initialize"),
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "Sublime Text LSP",
                "version": "2.13.0"
            },
            "rootUri": null,
            "workspaceFolders": null,
            "capabilities": capabilities
        }),
        timeout(),
    )
}

/// Answer the server requests the profile actually negotiated.
///
/// A declaration such as `workspace.configuration` or
/// `workspace.didChangeWatchedFiles.dynamicRegistration` obliges the client to
/// answer the matching server request, so a profile receipt that never replies
/// is not the client it claims to model. Each named method must arrive; any
/// request the profile did not negotiate stays unanswered and still fails
/// `assert_transport_clean` below.
fn answer_negotiated_server_requests(
    client: &mut RealProcessClient,
    negotiated_requests: &[&str],
) -> Result<()> {
    for method in negotiated_requests {
        let request = client.receive_server_request(method, timeout())?;
        let id = request
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{method} request omitted id: {request}"))?;
        let result = if *method == "workspace/configuration" {
            let items = request
                .pointer("/params/items")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("{method} request omitted items: {request}"))?
                .len();
            Value::Array(vec![Value::Null; items])
        } else {
            Value::Null
        };
        client.respond(id, result)?;
    }
    Ok(())
}

fn shutdown_and_exit(client: &mut RealProcessClient, negotiated_requests: &[&str]) -> Result<()> {
    client.notify("initialized", json!({}))?;
    answer_negotiated_server_requests(client, negotiated_requests)?;
    let shutdown = client.request(json!("sublime-shutdown"), "shutdown", Value::Null, timeout())?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null), "shutdown failed: {shutdown}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "Sublime-shaped lifecycle exited with {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

#[test]
fn sublime_lsp_2_13_receives_only_declared_file_operations() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(&mut client, sublime_lsp_2_13_capabilities())?;
    let capabilities = response
        .pointer("/result/capabilities")
        .ok_or_else(|| anyhow::anyhow!("initialize omitted capabilities: {response}"))?;

    ensure!(capabilities.get("positionEncoding") == Some(&json!("utf-16")));
    ensure!(capabilities.pointer("/workspace/fileOperations/willRename").is_some());
    ensure!(capabilities.pointer("/workspace/fileOperations/didRename").is_some());
    for unsupported in ["willCreate", "didCreate", "willDelete", "didDelete"] {
        ensure!(
            capabilities.pointer(&format!("/workspace/fileOperations/{unsupported}")).is_none(),
            "Sublime profile received unsupported {unsupported}: {capabilities}"
        );
    }
    ensure!(
        capabilities.get("inlineCompletionProvider").is_none(),
        "Sublime LSP 2.13 does not declare textDocument.inlineCompletion"
    );
    ensure!(
        capabilities.pointer("/experimental/perlInlineCompletionStream").is_none(),
        "custom inline stream must follow standard client capability consent"
    );
    ensure!(capabilities.pointer("/semanticTokensProvider/full/delta") == Some(&json!(true)));
    ensure!(capabilities.pointer("/semanticTokensProvider/range") == Some(&json!(true)));
    ensure!(capabilities.pointer("/diagnosticProvider").is_some());
    ensure!(
        capabilities.pointer("/workspace/textDocumentContent/schemes/0") == Some(&json!("perldoc"))
    );

    shutdown_and_exit(
        &mut client,
        &["workspace/configuration", "client/registerCapability", "window/workDoneProgress/create"],
    )
}

#[test]
fn malformed_file_operation_declarations_do_not_create_consent() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(
        &mut client,
        json!({
            "general": { "positionEncodings": ["utf-16"] },
            "workspace": {
                "workspaceFolders": true,
                "fileOperations": {
                    "willRename": "true",
                    "didRename": 1,
                    "willCreate": null
                }
            }
        }),
    )?;
    let capabilities = response
        .pointer("/result/capabilities")
        .ok_or_else(|| anyhow::anyhow!("initialize omitted capabilities: {response}"))?;

    ensure!(
        capabilities.pointer("/workspace/fileOperations").is_none(),
        "non-boolean file-operation values must not be treated as support: {capabilities}"
    );
    ensure!(capabilities.get("inlineCompletionProvider").is_none());

    // This profile declares neither workspace configuration, dynamic
    // watched-file registration, nor work-done progress, so the server must not
    // issue any request for it to answer.
    shutdown_and_exit(&mut client, &[])
}
