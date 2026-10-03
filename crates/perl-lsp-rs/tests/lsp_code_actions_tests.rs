/// Comprehensive tests for LSP code actions and refactorings
use perl_diagnostics::codes::DiagnosticCode;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

mod common;
use common::{
    initialize_lsp, send_notification, send_request, shutdown_and_exit, start_lsp_server,
};

/// Test extract variable refactoring
#[test]
fn test_extract_variable() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
my $str = "hello";
my $result = length($str) + 10;
print $result;
"#
                }
            }
        }),
    );

    // Request code actions for the expression "length($str)"
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 2, "character": 13 },
                    "end": { "line": 2, "character": 25 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(actions.iter().any(|a| {
        let title = a["title"].as_str().unwrap_or("");
        title.contains("Extract") && title.contains("variable")
    }));
    shutdown_and_exit(&server);
    Ok(())
}

/// Test adding error checking to file operations
#[test]
fn test_add_error_checking() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
open($fh, '<', 'data.txt');
print "Hello\n";
close($fh);
"#
                }
            }
        }),
    );

    // Request code actions for the open statement
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 30 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(actions.iter().any(|a| a["title"].as_str().unwrap_or("").contains("error checking")));
    shutdown_and_exit(&server);
    Ok(())
}

/// Regression guard for #9835: `textDocument/codeAction` used to panic the
/// server when a file operation was followed by multi-byte UTF-8 text within
/// the error-checking lookahead window, because the window was cut at a fixed
/// 50-*byte* offset that could land inside a character.
///
/// This is the wire-level proof: the request must answer normally — and still
/// offer the action — rather than taking the request path down.
#[test]
fn test_code_action_survives_multibyte_source() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///utf8.pl";
    // The accented comment starts within 50 bytes of the `open` call's end, so
    // the pre-fix 50-byte window ended inside an 'é'.
    let text = format!("\nopen($fh, '<', 'data.txt');\n#{}\n", "é".repeat(40));
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
                    "text": text
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 27 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"]
        .as_array()
        .ok_or("codeAction must return a result array on non-ASCII source")?;
    assert!(
        actions.iter().any(|a| a["title"].as_str().unwrap_or("").contains("error checking")),
        "the error-checking action must still be offered when non-ASCII text follows the call"
    );
    shutdown_and_exit(&server);
    Ok(())
}

/// Test converting old-style for loops to foreach
#[test]
fn test_convert_loop_style() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
for (my $i = 0; $i < @array; $i++) {
    print $array[$i];
}
"#
                }
            }
        }),
    );

    // Request code actions for the for loop
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 3, "character": 1 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(
        actions.iter().any(|a| a["title"].as_str().unwrap_or("").contains("foreach loop")),
        "Expected 'foreach loop' conversion action but got: {:?}",
        actions.iter().map(|a| a["title"].as_str()).collect::<Vec<_>>()
    );
    shutdown_and_exit(&server);
    Ok(())
}

/// Test converting to postfix form
#[test]
fn test_convert_to_postfix() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
if ($debug) {
    print "Debug mode\n";
}
"#
                }
            }
        }),
    );

    // Request code actions for the if statement
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 3, "character": 1 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(actions.iter().any(|a| a["title"].as_str().unwrap_or("").contains("postfix")));
    shutdown_and_exit(&server);
    Ok(())
}

/// Test adding missing pragmas
#[test]
fn test_add_missing_pragmas() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
#!/usr/bin/perl

my $x = 42;
print $x;
"#
                }
            }
        }),
    );

    // Request code actions for the entire document
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 4, "character": 0 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(actions.iter().any(|a| a["title"].as_str().unwrap_or("").contains("pragma")));
    shutdown_and_exit(&server);
    Ok(())
}

/// Test quick fix for undefined variable
#[test]
fn test_fix_undefined_variable() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
use strict;
use warnings;

print $undefined_var;
"#
                }
            }
        }),
    );

    // First get diagnostics
    let diag_response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "textDocument/diagnostic",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );

    // Request code actions with diagnostics
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 4, "character": 6 },
                    "end": { "line": 4, "character": 20 }
                },
                "context": {
                    "diagnostics": diag_response["result"]["items"].clone()
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(actions.iter().any(|a| {
        let title = a["title"].as_str().unwrap_or("");
        title.contains("Declare") && title.contains("my")
    }));
    shutdown_and_exit(&server);
    Ok(())
}

/// Test quick fixes preserve associated diagnostics in the LSP response
#[test]
fn test_quickfix_actions_include_associated_diagnostics() -> Result<(), Box<dyn std::error::Error>>
{
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
use strict;
use warnings;

print $undefined_var;
"#
                }
            }
        }),
    );

    let diag_response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 69,
            "method": "textDocument/diagnostic",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let reported_diagnostics =
        diag_response["result"]["items"].as_array().ok_or("Expected diagnostics result items")?;
    let reported_diagnostic = reported_diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(
                diagnostic["code"].as_str(),
                Some(code)
                    if code == DiagnosticCode::UndefinedVariable.as_str()
                        || matches!(code, "undeclared-variable" | "undefined-variable")
            )
        })
        .ok_or("Expected undefined-variable style diagnostic in pull diagnostics")?;

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 70,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 4, "character": 6 },
                    "end": { "line": 4, "character": 20 }
                },
                "context": {
                    "diagnostics": diag_response["result"]["items"].clone()
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    let declare_action = actions
        .iter()
        .find(|action| action["title"].as_str() == Some("Declare '$undefined_var' with 'my'"))
        .ok_or("Expected quick fix for undefined variable")?;

    let diagnostics = declare_action["diagnostics"]
        .as_array()
        .ok_or("Expected quick fix to include associated diagnostics")?;
    assert_eq!(diagnostics.len(), 1);

    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["range"], reported_diagnostic["range"]);
    assert_eq!(diagnostic["severity"], reported_diagnostic["severity"]);
    assert_eq!(diagnostic["code"], reported_diagnostic["code"]);
    assert_eq!(diagnostic["source"], reported_diagnostic["source"]);
    assert_eq!(diagnostic["message"], reported_diagnostic["message"]);

    shutdown_and_exit(&server);
    Ok(())
}

fn apply_extract_workspace_edit(
    source: &str,
    action: &serde_json::Value,
    uri: &str,
) -> Result<(String, Vec<(usize, usize, String)>), Box<dyn std::error::Error>> {
    let edits =
        action["edit"]["changes"][uri].as_array().ok_or("Expected WorkspaceEdit changes")?;
    let offset = |point: &serde_json::Value| -> Result<usize, Box<dyn std::error::Error>> {
        let line = point["line"].as_u64().ok_or("Missing line")? as usize;
        let character = point["character"].as_u64().ok_or("Missing character")? as usize;
        let line_text = source.split_inclusive('\n').nth(line).ok_or("Line outside document")?;
        let prefix: usize = source.split_inclusive('\n').take(line).map(str::len).sum();
        let mut units = 0;
        let mut bytes = 0;
        for ch in line_text.chars() {
            if units == character {
                break;
            }
            units += ch.len_utf16();
            bytes += ch.len_utf8();
        }
        if units != character {
            return Err("UTF-16 column outside line".into());
        }
        Ok(prefix + bytes)
    };
    let mut changes = edits
        .iter()
        .map(|edit| {
            Ok((
                offset(&edit["range"]["start"])?,
                offset(&edit["range"]["end"])?,
                edit["newText"].as_str().ok_or("Missing newText")?.to_string(),
            ))
        })
        .collect::<Result<Vec<(usize, usize, String)>, Box<dyn std::error::Error>>>()?;
    changes.sort_by_key(|change| std::cmp::Reverse(change.0));
    let mut edited = source.to_string();
    for (start, end, replacement) in &changes {
        edited.replace_range(*start..*end, replacement);
    }
    Ok((edited, changes))
}

fn run_perl(source: &str) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut perl = Command::new("perl")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    perl.stdin.take().ok_or("Missing perl stdin")?.write_all(source.as_bytes())?;
    Ok(perl.wait_with_output()?)
}

/// Test extract subroutine refactoring
#[test]
fn test_extract_subroutine() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
    let source = "use strict;\r\n# café 🍀\r\nuse warnings;\r\nsub worker {\r\n    my $base = 10;\r\n    {\r\n        my $x = $base * 2;\r\n        $x + 1;\r\n    }\r\n}\r\nprint worker(), \"\\n\";\r\n";
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

    // Request code actions for the block
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 5, "character": 4 },
                    "end": { "line": 8, "character": 5 }
                },
                "context": {
                    "diagnostics": [],
                    "only": ["refactor.extract.subroutine"]
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    let action = actions
        .iter()
        .find(|a| a["title"] == "Extract to subroutine")
        .ok_or("Expected extract subroutine action")?;
    assert_eq!(action["kind"], "refactor.extract.subroutine");
    let (edited, changes) = apply_extract_workspace_edit(source, action, uri)?;
    assert_eq!(changes.len(), 2, "extraction must serialize insertion and replacement");
    let sub_start = source.find("sub worker").ok_or("Missing sub")?;
    assert!(changes.iter().any(|(start, end, _)| start == end && *start == sub_start));
    assert!(changes.iter().any(|(start, end, _)| {
        source.get(*start..*end).is_some_and(|text| text.starts_with('{') && text.ends_with('}'))
    }));
    assert!(edited.contains("my ($base) = @_;"));
    assert!(edited.contains("process_data($base);"));
    assert!(edited.contains("# café 🍀\r\nuse warnings;"), "untouched UTF-8 prefix changed");
    assert!(!edited.replace("\r\n", "").contains('\n'), "edit introduced LF-only lines");
    assert!(edited.ends_with("print worker(), \"\\n\";\r\n"), "untouched suffix changed");
    let mut perl = Command::new("perl")
        .args(["-c", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    perl.stdin.take().ok_or("Missing perl stdin")?.write_all(edited.as_bytes())?;
    let output = perl.wait_with_output()?;
    assert!(
        output.status.success(),
        "applied WorkspaceEdit failed perl -c:\n{}\n{edited}",
        String::from_utf8_lossy(&output.stderr)
    );
    shutdown_and_exit(&server);
    Ok(())
}

/// A line that looks like a subroutine declaration inside a Perl string must
/// never become the insertion point for an extract edit.
#[test]
fn test_extract_subroutine_ignores_sub_text_inside_multiline_string()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);
    let uri = "file:///string-sub.pl";
    let source = "use strict;\nsub worker {\n my $s = 'before\nsub marker\n';\n { my $x = 2; $x + 1; }\n}\nprint worker(), \"\\n\";\n";
    let original = run_perl(source)?;
    assert!(
        original.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&original.stderr)
    );
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": source}}
        }),
    );
    let block_line = source.lines().nth(5).ok_or("Missing block line")?;
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0", "id": 81, "method": "textDocument/codeAction",
            "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 5, "character": 1},
                          "end": {"line": 5, "character": block_line.len()}},
                "context": {"diagnostics": []}
            }
        }),
    );
    let actions = response["result"].as_array().ok_or("Expected actions array")?;
    if let Some(action) = actions.iter().find(|action| action["title"] == "Extract to subroutine") {
        let (edited, changes) = apply_extract_workspace_edit(source, action, uri)?;
        assert_eq!(changes.len(), 2, "offered extraction lacked its two edits");
        let result = run_perl(&edited)?;
        assert!(
            result.status.success(),
            "offered edit failed at runtime: {}\n{edited}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, original.stdout, "offered edit changed runtime output");
    }
    shutdown_and_exit(&server);
    Ok(())
}

/// #16779: the basic extract-function fallback must not reach the wire with a
/// parameterless edit that captures a surrounding `my` variable.
#[test]
fn test_extract_function_withholds_capturing_if_body() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nuse warnings;\n{\n    my $x = 1;\n    if (1) { warn $x; } else { warn 0; }\n}\n";
    let original = run_perl(source)?;
    assert!(
        original.status.success(),
        "fixture failed perl -c: {}",
        String::from_utf8_lossy(&original.stderr)
    );

    let server = start_lsp_server();
    initialize_lsp(&server);
    let uri = "file:///capture-extract.pl";
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

    // LSP line 4 / UTF-16 characters 11..23 selects `{ warn $x; }`.
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 82,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 4, "character": 11 },
                    "end": { "line": 4, "character": 23 }
                },
                "context": { "diagnostics": [] }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected codeAction result array")?;
    assert!(
        actions.iter().all(|action| action["title"] != "Extract to function"),
        "capturing if-body must not publish Extract to function: {actions:?}"
    );
    let published_extract_edits = actions.iter().flat_map(|action| {
        action["edit"]["changes"][uri]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|edit| edit["newText"].as_str().map(str::to_string))
    });
    for new_text in published_extract_edits {
        assert!(
            !new_text.contains("sub extracted_function")
                || new_text.contains("my ($x) = @_")
                || !new_text.contains("warn $x"),
            "parameterless extracted_function captured outer $x:\n{new_text}\nactions={actions:?}"
        );
    }
    shutdown_and_exit(&server);
    Ok(())
}

/// The legacy organize-imports action stays withdrawn (#8305)
#[test]
fn test_organize_imports() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
#!/usr/bin/perl
use JSON;
use Data::Dumper;
use warnings;
use File::Path;
use strict;
use lib './lib';

print "test\n";
"#
                }
            }
        }),
    );

    // Request code actions for the import section
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 7, "character": 0 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;
    assert!(
        actions.iter().all(|a| a["title"].as_str().unwrap_or("") != "Organize imports"),
        "the withdrawn legacy organizer (#8305) must not be offered; got {actions:?}"
    );
    assert!(
        actions.iter().all(|a| a["kind"].as_str().unwrap_or("") != "source.organizeImports"),
        "no action may carry the withdrawn source.organizeImports kind; got {actions:?}"
    );
    shutdown_and_exit(&server);
    Ok(())
}

/// Test multiple code action kinds available for the same selection
#[test]
fn test_multiple_code_action_kinds() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
use strict;
use warnings;

open(my $fh, '<', 'data.txt');
"#
                }
            }
        }),
    );

    // Request code actions for the file operation. Current behavior offers a
    // refactor plus other applicable action kinds for the same selection.
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 4, "character": 0 },
                    "end": { "line": 4, "character": 30 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;

    // Should have multiple action kinds available for the same selection.
    assert!(!actions.is_empty(), "Expected code actions but got none");
    assert!(actions.iter().any(|a| a["kind"].as_str() == Some("refactor.rewrite")));
    assert!(actions.iter().any(|a| a["kind"].as_str() == Some("quickfix")));
    shutdown_and_exit(&server);
    Ok(())
}

/// Test that context.only filters code actions to the requested kind family
#[test]
fn test_context_only_filters_to_requested_code_action_kinds()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///test.pl";
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
                    "text": r#"
use strict;
use warnings;

open(my $fh, '<', 'data.txt');
"#
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 4, "character": 0 },
                    "end": { "line": 4, "character": 30 }
                },
                "context": {
                    "diagnostics": [],
                    "only": ["refactor"]
                }
            }
        }),
    );

    let actions = response["result"].as_array().ok_or("Expected result to be an array")?;

    assert!(!actions.is_empty(), "Expected refactor code actions but got none");
    assert!(actions.iter().all(|action| {
        action["kind"]
            .as_str()
            .is_some_and(|kind| kind == "refactor" || kind.starts_with("refactor."))
    }));
    assert!(actions.iter().any(|action| action["kind"].as_str() == Some("refactor.rewrite")));
    assert!(!actions.iter().any(|action| action["kind"].as_str() == Some("quickfix")));

    shutdown_and_exit(&server);
    Ok(())
}

/// Regression (issue #1787 follow-up): overlapping code-action providers must
/// not return byte-identical duplicate quick-fixes. A file missing `use strict`
/// previously yielded three identical "Add 'use strict'" actions plus two
/// identical "Add missing pragmas" actions; the response must now contain each
/// distinct (kind, title, edit) action at most once.
#[test]
fn test_code_actions_have_no_exact_duplicates() -> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///dedupe.pl";
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
                    "text": "print 'hi';\n"
                }
            }
        }),
    );

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 7878,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 11 }
                },
                "context": {
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 5 }
                        },
                        "severity": 2,
                        "code": "TestingAndDebugging::RequireUseStrict",
                        "source": "perl-lsp-critic",
                        "message": "Code before strictures are enabled"
                    }]
                }
            }
        }),
    );

    let actions = response["result"].as_array().cloned().unwrap_or_default();

    // No two actions may share the same (kind, title, edit).
    let mut seen = std::collections::HashSet::new();
    for action in &actions {
        let key = (
            action["kind"].as_str().unwrap_or("").to_string(),
            action["title"].as_str().unwrap_or("").to_string(),
            action["edit"].to_string(),
        );
        assert!(
            seen.insert(key.clone()),
            "duplicate code action returned: {key:?}\nfull response: {actions:#?}"
        );
    }

    // The "Add 'use strict'" quick-fix must appear exactly once.
    let strict_count =
        actions.iter().filter(|a| a["title"].as_str() == Some("Add 'use strict'")).count();
    assert_eq!(
        strict_count, 1,
        "expected exactly one \"Add 'use strict'\" action, got {strict_count}: {actions:#?}"
    );

    shutdown_and_exit(&server);
    Ok(())
}
