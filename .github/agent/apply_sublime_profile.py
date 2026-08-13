from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"expected one match in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


capabilities = ROOT / "crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs"

replace_once(
    capabilities,
    '''/// Build the workspace capabilities with file operation filters for Perl
/// extensions (#4995).
fn workspace_capabilities(workspace_folders_support: bool) -> Value {
    let perl_globs = ["**/*.pl", "**/*.pm", "**/*.t", "**/*.psgi"];
    let filters: Vec<Value> =
        perl_globs.iter().map(|glob| json!({ "pattern": { "glob": glob } })).collect();

    json!({
        "workspaceFolders": {
            "supported": workspace_folders_support,
            "changeNotifications": true
        },
        "fileOperations": {
            "willCreate": { "filters": filters.clone() },
            "didCreate": { "filters": filters.clone() },
            "willRename": { "filters": filters.clone() },
            "didRename": { "filters": filters.clone() },
            "willDelete": { "filters": filters.clone() },
            "didDelete": { "filters": filters }
        },
        "textDocumentContent": {
            "schemes": ["perldoc"]
        }
    })
}
''',
    '''/// File-operation requests the client explicitly declared it can send.
///
/// LSP file-operation support is negotiated per operation. A server that
/// advertises an operation the client omitted creates a false capability
/// surface even when the client never happens to invoke it.
#[derive(Debug, Clone, Copy, Default)]
struct FileOperationSupport {
    will_create: bool,
    did_create: bool,
    will_rename: bool,
    did_rename: bool,
    will_delete: bool,
    did_delete: bool,
}

impl FileOperationSupport {
    fn from_initialize_params(params: Option<&Value>) -> Self {
        let supported = |path: &str| {
            params
                .and_then(|params| params.pointer(path))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };

        Self {
            will_create: supported("/capabilities/workspace/fileOperations/willCreate"),
            did_create: supported("/capabilities/workspace/fileOperations/didCreate"),
            will_rename: supported("/capabilities/workspace/fileOperations/willRename"),
            did_rename: supported("/capabilities/workspace/fileOperations/didRename"),
            will_delete: supported("/capabilities/workspace/fileOperations/willDelete"),
            did_delete: supported("/capabilities/workspace/fileOperations/didDelete"),
        }
    }

    fn insert_capabilities(
        self,
        target: &mut serde_json::Map<String, Value>,
        filters: &[Value],
    ) {
        for (name, supported) in [
            ("willCreate", self.will_create),
            ("didCreate", self.did_create),
            ("willRename", self.will_rename),
            ("didRename", self.did_rename),
            ("willDelete", self.will_delete),
            ("didDelete", self.did_delete),
        ] {
            if supported {
                target.insert(name.to_string(), json!({ "filters": filters }));
            }
        }
    }
}

/// Build workspace capabilities, intersecting file operations with the exact
/// operations the client declared during initialize.
fn workspace_capabilities(
    workspace_folders_support: bool,
    file_operations: FileOperationSupport,
) -> Value {
    let perl_globs = ["**/*.pl", "**/*.pm", "**/*.t", "**/*.psgi"];
    let filters: Vec<Value> =
        perl_globs.iter().map(|glob| json!({ "pattern": { "glob": glob } })).collect();
    let mut file_operation_capabilities = serde_json::Map::new();
    file_operations.insert_capabilities(&mut file_operation_capabilities, &filters);

    let mut workspace = json!({
        "workspaceFolders": {
            "supported": workspace_folders_support,
            "changeNotifications": true
        },
        "textDocumentContent": {
            "schemes": ["perldoc"]
        }
    });
    if !file_operation_capabilities.is_empty() {
        workspace["fileOperations"] = Value::Object(file_operation_capabilities);
    }
    workspace
}
''',
)

replace_once(
    capabilities,
    '''        let inline_completion_dynamic_registration_support =
            self.client_capabilities.lock().inline_completion_dynamic_registration_support;

        match (features.inline_completion, inline_completion_dynamic_registration_support) {
            // LSP 3.18 dynamic registration is an alternate registration mode,
            // not an addition to static registration for the same selector.
            (true, true) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
            (true, false) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.insert(
                        "inlineCompletionProvider".to_string(),
                        Value::Object(serde_json::Map::new()),
                    );
                }
            }
            (false, _) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
        }
''',
    '''        let (inline_completion_support, inline_completion_dynamic_registration_support) = {
            let client_capabilities = self.client_capabilities.lock();
            (
                client_capabilities.inline_completion_support,
                client_capabilities.inline_completion_dynamic_registration_support,
            )
        };

        match (
            features.inline_completion,
            inline_completion_support,
            inline_completion_dynamic_registration_support,
        ) {
            // LSP 3.18 dynamic registration is an alternate registration mode,
            // not an addition to static registration for the same selector.
            (true, true, true) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
            (true, true, false) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.insert(
                        "inlineCompletionProvider".to_string(),
                        Value::Object(serde_json::Map::new()),
                    );
                }
            }
            _ => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
        }
''',
)

replace_once(
    capabilities,
    '''        // Workspace capabilities: typed helper for file operations (#4995)
        let workspace_folders_support = self.client_capabilities.lock().workspace_folders_support;
        capabilities["workspace"] = workspace_capabilities(workspace_folders_support);

        // Advertise experimental custom requests
        if features.inline_completion {
''',
    '''        // Workspace capabilities: intersect client-dependent file-operation
        // participation with the exact initialize declaration (#7682).
        let workspace_folders_support = self.client_capabilities.lock().workspace_folders_support;
        let file_operations = FileOperationSupport::from_initialize_params(params.as_ref());
        capabilities["workspace"] =
            workspace_capabilities(workspace_folders_support, file_operations);

        // Advertise experimental custom requests only to clients that declared
        // the corresponding standard inline-completion capability.
        if features.inline_completion && inline_completion_support {
''',
)

inline_tests = ROOT / "crates/perl-lsp-rs/tests/lsp_inline_completion_registration_tests.rs"
replace_once(
    inline_tests,
    '''#[test]
fn initialize_static_advertises_inline_completion_when_dynamic_not_supported() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize(Some(json!({
        "textDocument": {}
    })))?;

    assert_eq!(init.pointer("/capabilities/inlineCompletionProvider"), Some(&json!({})));
    assert_no_inline_completion_registration(harness.drain_server_requests(200));
    Ok(())
}
''',
    '''#[test]
fn initialize_omits_inline_completion_when_client_does_not_declare_support() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize(Some(json!({
        "textDocument": {}
    })))?;

    assert!(init.pointer("/capabilities/inlineCompletionProvider").is_none());
    assert!(init.pointer("/capabilities/experimental/perlInlineCompletionStream").is_none());
    assert_no_inline_completion_registration(harness.drain_server_requests(200));
    Ok(())
}
''',
)

profile_test = ROOT / "crates/perllsp/tests/sublime_lsp_2_13_profile_contract.rs"
profile_test.write_text(
    r'''//! Real-process compatibility contract for Sublime Text's LSP 2.13 client profile.
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

fn shutdown_and_exit(client: &mut RealProcessClient) -> Result<()> {
    client.notify("initialized", json!({}))?;
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
            capabilities
                .pointer(&format!("/workspace/fileOperations/{unsupported}"))
                .is_none(),
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

    shutdown_and_exit(&mut client)
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

    shutdown_and_exit(&mut client)
}
''',
    encoding="utf-8",
)
