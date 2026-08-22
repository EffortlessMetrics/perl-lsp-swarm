//! Exact-process containment tests for withdrawn hard-coded missing-import
//! edits (issue #10690).
//!
//! Until #790/#8948 land exact candidate planning, neither the enhanced global
//! route nor the PL109 UnquotedBareword route may turn hard-coded function→module
//! affinity into an executable import edit on any surface: direct, filtered
//! (`quickfix`, `source.fixAll`), or resolve-shaped requests must fail closed,
//! while PL109's legitimate quote/filehandle fixes survive.

use serde_json::json;

mod common;
use common::{
    initialize_lsp, send_notification, send_request, shutdown_and_exit, start_lsp_server,
};

/// Package-first document calling a table-mapped function (`dumper`). The
/// withdrawn enhanced route inserted `use Data::Dumper;` before the package,
/// importing into `main` while the call lives in `App`.
const PACKAGE_FIRST_DUMPER: &str =
    "package App;\nuse strict;\nuse warnings;\nmy $value = dumper($value);\n1;\n";

fn assert_no_affinity_import_actions(actions: &[serde_json::Value]) {
    for action in actions {
        let title = action["title"].as_str().unwrap_or("");
        assert_ne!(
            title, "Add missing imports",
            "withdrawn enhanced missing-import action (#10690) must not be offered; got {actions:?}"
        );
        assert!(
            !title.starts_with("Import '"),
            "withdrawn PL109 import action (#10690) must not be offered; got {actions:?}"
        );

        let mut action_edits: Vec<&serde_json::Value> = Vec::new();
        if let Some(changes) =
            action.pointer("/edit/changes").and_then(|changes| changes.as_object())
        {
            for value in changes.values() {
                if let Some(list) = value.as_array() {
                    action_edits.extend(list.iter());
                }
            }
        }
        for edit in action_edits {
            let new_text = edit["newText"].as_str().unwrap_or("");
            let inserts_use_line =
                new_text.lines().any(|line| line.trim_start().starts_with("use "));
            // The missing-pragma quick fix legitimately inserts exactly these
            // pragma texts; any other `use` insertion is affinity-derived.
            let legitimate_pragma_insertion = matches!(
                new_text,
                "use strict;\n"
                    | "use warnings;\n"
                    | "use strict;\nuse warnings;\n"
                    | "use strict;\nuse warnings;\n\n"
            );
            assert!(
                !inserts_use_line || legitimate_pragma_insertion,
                "action {title:?} carries an import-insertion edit ({new_text:?}); hard-coded affinity must not authorize edits (#10690); got {actions:?}"
            );
        }
    }
}

/// The enhanced global missing-import route stays withdrawn over stdio.
#[test]
fn test_code_action_unfiltered_returns_no_table_derived_import()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///missing_import_containment.pl";
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": PACKAGE_FIRST_DUMPER
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 5, "character": 0 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert_no_affinity_import_actions(actions);
    shutdown_and_exit(&server);
    Ok(())
}

/// The `source.fixAll` aggregate cannot absorb a table-derived import edit.
#[test]
fn test_source_fix_all_filter_cannot_absorb_table_derived_import()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///missing_import_containment_fixall.pl";
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": PACKAGE_FIRST_DUMPER
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 5, "character": 0 }
                },
                "context": {
                    "only": ["source.fixAll"],
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert_no_affinity_import_actions(actions);
    shutdown_and_exit(&server);
    Ok(())
}

/// A client-supplied PL109 diagnostic cannot produce an import edit through the
/// production routing, while the legitimate quote fixes remain available.
#[test]
fn test_pl109_diagnostic_route_returns_no_import_but_keeps_quote_fixes()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    // "my $name = basename($path);" — `basename` spans characters 11..19.
    let source = "use strict;\nuse warnings;\nmy $name = basename($path);\n";
    let uri = "file:///missing_import_containment_pl109.pl";
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 2, "character": 11 },
                    "end": { "line": 2, "character": 19 }
                },
                "context": {
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 2, "character": 11 },
                            "end": { "line": 2, "character": 19 }
                        },
                        "severity": 1,
                        "code": "PL109",
                        "source": "perl-lsp",
                        "message": "Bareword 'basename' is not allowed under 'use strict' -- quote it as 'basename' or use it as a subroutine call"
                    }]
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert_no_affinity_import_actions(actions);

    assert!(
        actions.iter().any(|a| a["title"].as_str() == Some("Quote 'basename' with single quotes")),
        "PL109 single-quote fix must remain available; got {actions:?}"
    );
    shutdown_and_exit(&server);
    Ok(())
}

/// Resolve cannot reconstruct an affinity-derived import edit from opaque
/// action data; only the pragma contract inserts edits on resolve.
#[test]
fn test_resolve_cannot_reconstruct_affinity_import_from_foreign_data()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///missing_import_containment_resolve.pl";
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": PACKAGE_FIRST_DUMPER
                }
            }
        }),
    );

    // Fabricated foreign data shaped like an affinity candidate: resolving it
    // must not conjure an import edit.
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "codeAction/resolve",
            "params": {
                "title": "Import 'Data::Dumper'",
                "kind": "quickfix",
                "data": {
                    "uri": uri,
                    "symbol": "dumper",
                    "module": "Data::Dumper"
                }
            }
        }),
    );

    let resolved = response["result"].as_object().ok_or("Expected result to be an object")?;
    assert!(
        resolved.get("edit").is_none(),
        "resolve must not reconstruct a table-derived import edit (#10690); got {resolved:?}"
    );
    shutdown_and_exit(&server);
    Ok(())
}
