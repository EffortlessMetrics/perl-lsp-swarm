//! Production-path proof that `inlayHint/resolve` label-location selects the
//! Perl-effective subroutine (#14675): last same-package definition, not the
//! first AST name match.
//!
//! These tests drive `textDocument/inlayHint` → `inlayHint/resolve` against an
//! open document. They fail if resolve still returns the first same-name `sub`.
//! Envelope authenticity (#14672) and provider migration (#8299) are out of scope.

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type ValueResult = Result<Value, Box<dyn std::error::Error>>;

fn init_with_label_location(srv: &LspServer) {
    srv.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "capabilities": {
                "textDocument": {
                    "inlayHint": {
                        "dynamicRegistration": true,
                        "resolveSupport": {
                            "properties": ["tooltip", "label.location"]
                        }
                    }
                }
            }
        })),
    });
    srv.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    });
}

fn open_document(srv: &LspServer, uri: &str, text: &str) {
    srv.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })),
    });
}

fn list_hints(srv: &LspServer, uri: &str) -> ValueResult {
    let res = srv
        .handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
            method: "textDocument/inlayHint".into(),
            params: Some(json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 999, "character": 0 }
                }
            })),
        })
        .ok_or("textDocument/inlayHint produced no response")?;
    res.result.ok_or_else(|| "textDocument/inlayHint returned no result".into())
}

fn resolve_hint(srv: &LspServer, hint: Value) -> ValueResult {
    let res = srv
        .handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(3_i64)),
            method: "inlayHint/resolve".into(),
            params: Some(hint),
        })
        .ok_or("inlayHint/resolve produced no response")?;
    res.result.ok_or_else(|| "inlayHint/resolve returned no result".into())
}

fn first_param_hint_for<'a>(hints: &'a [Value], function_name: &str) -> Result<&'a Value, String> {
    hints
        .iter()
        .find(|hint| {
            hint.get("kind").and_then(Value::as_u64) == Some(2)
                && hint.pointer("/data/functionName").and_then(Value::as_str).is_some_and(|name| {
                    name == function_name || name.ends_with(&format!("::{function_name}"))
                })
        })
        .ok_or_else(|| format!("no parameter hint for {function_name}; hints: {hints:?}"))
}

fn resolved_location_line(resolved: &Value) -> Result<u32, Box<dyn std::error::Error>> {
    let parsed: lsp_types::InlayHint = serde_json::from_value(resolved.clone())?;
    let parts = match parsed.label {
        lsp_types::InlayHintLabel::LabelParts(parts) => parts,
        lsp_types::InlayHintLabel::String(label) => {
            return Err(format!("resolved label remained a string: {label}").into());
        }
    };
    let location = parts
        .iter()
        .find_map(|part| part.location.as_ref())
        .ok_or("resolved label part location missing")?;
    Ok(location.range.start.line)
}

fn resolve_call_site_line(
    text: &str,
    uri: &str,
    function_name: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    init_with_label_location(&srv);
    open_document(&srv, uri, text);
    let hints_value = list_hints(&srv, uri)?;
    let hints = hints_value.as_array().ok_or("inlayHint result was not an array")?;
    let hint = first_param_hint_for(hints, function_name)?.clone();
    let resolved = resolve_hint(&srv, hint)?;
    resolved_location_line(&resolved)
}

fn line_of_nth_sub(text: &str, name: &str, n: usize) -> Result<u32, Box<dyn std::error::Error>> {
    let needle = format!("sub {name}");
    let mut from = 0;
    let mut hits = 0;
    while let Some(rel) = text[from..].find(&needle) {
        let at = from + rel;
        if hits == n {
            let line = u32::try_from(text[..at].bytes().filter(|b| *b == b'\n').count())?;
            return Ok(line);
        }
        hits += 1;
        from = at + needle.len();
    }
    Err(format!("did not find occurrence {n} of `{needle}`").into())
}

/// Same-package redefinition: Perl installs the last `sub greet`, so click-to-
/// definition from the call's parameter hint must land on the second declaration.
#[test]
fn resolve_selects_last_same_package_redefinition() -> TestResult {
    let text = r#"sub greet($name, $greeting) { return "first"; }
sub greet($name, $greeting) { return "second"; }
greet("Alice", "Hello");
"#;
    let line = resolve_call_site_line(text, "file:///redef_last.pl", "greet")?;
    let first = line_of_nth_sub(text, "greet", 0)?;
    let last = line_of_nth_sub(text, "greet", 1)?;
    assert_ne!(first, last, "fixture must contain two distinct greet declarations");
    assert_eq!(
        line, last,
        "resolve must select the last same-package greet (line {last}), not the first (line {first}); got {line}"
    );
    Ok(())
}

/// A later same-name sub in a different package must not steal a call that is
/// still in the earlier package. Last-in-file would fail this control.
#[test]
fn resolve_does_not_select_later_other_package_definition() -> TestResult {
    let text = r#"package A;
sub run($x, $y) { return "A"; }
run(1, 2);
package B;
sub run($x, $y) { return "B"; }
"#;
    let line = resolve_call_site_line(text, "file:///other_pkg.pl", "run")?;
    let package_a = line_of_nth_sub(text, "run", 0)?;
    let package_b = line_of_nth_sub(text, "run", 1)?;
    assert_eq!(
        line, package_a,
        "call in package A must resolve to A's run (line {package_a}), not B's later run (line {package_b}); got {line}"
    );
    Ok(())
}

/// Call in package B after both A and B defined `run`: first-name-match lands
/// on A; Perl-effective selection is B.
#[test]
fn resolve_selects_call_site_package_not_first_file_match() -> TestResult {
    let text = r#"package A;
sub run($x, $y) { return "A"; }
package B;
sub run($x, $y) { return "B"; }
run(1, 2);
"#;
    let line = resolve_call_site_line(text, "file:///call_site_pkg.pl", "run")?;
    let package_a = line_of_nth_sub(text, "run", 0)?;
    let package_b = line_of_nth_sub(text, "run", 1)?;
    assert_eq!(
        line, package_b,
        "call in package B must resolve to B's run (line {package_b}), not A's first match (line {package_a}); got {line}"
    );
    Ok(())
}

/// Returning to package A after B defined the same name: last same-package
/// definition is A's second `run`, not B's intervening one and not A's first.
#[test]
fn resolve_selects_last_definition_after_returning_to_package() -> TestResult {
    let text = r#"package A;
sub run($x, $y) { return "A1"; }
package B;
sub run($x, $y) { return "B"; }
package A;
sub run($x, $y) { return "A2"; }
run(1, 2);
"#;
    let line = resolve_call_site_line(text, "file:///return_pkg.pl", "run")?;
    let a1 = line_of_nth_sub(text, "run", 0)?;
    let b = line_of_nth_sub(text, "run", 1)?;
    let a2 = line_of_nth_sub(text, "run", 2)?;
    assert_eq!(
        line, a2,
        "call in later package A must resolve to A's last run (line {a2}), not A1 ({a1}) or B ({b}); got {line}"
    );
    Ok(())
}

/// Block-scoped package: a call inside `package Inner { }` must not resolve to
/// the outer package's earlier same-name sub.
#[test]
fn resolve_respects_block_package_scope() -> TestResult {
    let text = r#"package Outer;
sub run($x, $y) { return "outer"; }
package Inner {
  sub run($x, $y) { return "inner"; }
  run(1, 2);
}
"#;
    let line = resolve_call_site_line(text, "file:///block_pkg.pl", "run")?;
    let outer = line_of_nth_sub(text, "run", 0)?;
    let inner = line_of_nth_sub(text, "run", 1)?;
    assert_eq!(
        line, inner,
        "call inside package Inner {{ }} must resolve to Inner's run (line {inner}), not Outer's (line {outer}); got {line}"
    );
    Ok(())
}

/// After a block package ends, the outer package is restored. Last-in-file
/// would pick Inner's `run`; the call is still Outer's.
#[test]
fn resolve_restores_outer_package_after_block() -> TestResult {
    let text = r#"package Outer;
sub run($x, $y) { return "outer"; }
package Inner {
  sub run($x, $y) { return "inner"; }
}
run(1, 2);
"#;
    let line = resolve_call_site_line(text, "file:///after_block_pkg.pl", "run")?;
    let outer = line_of_nth_sub(text, "run", 0)?;
    let inner = line_of_nth_sub(text, "run", 1)?;
    assert_eq!(
        line, outer,
        "call after package Inner {{ }} must resolve to Outer's run (line {outer}), not Inner's (line {inner}); got {line}"
    );
    Ok(())
}

/// A single definition remains selectable. Retention control so last-wins
/// cannot collapse into "never resolve".
#[test]
fn resolve_still_selects_the_only_declaration() -> TestResult {
    let text = r#"sub greet($name, $greeting) { return "only"; }
greet("Alice", "Hello");
"#;
    let line = resolve_call_site_line(text, "file:///only_decl.pl", "greet")?;
    let only = line_of_nth_sub(text, "greet", 0)?;
    assert_eq!(
        line, only,
        "the sole greet declaration must still resolve; got {line}, expected {only}"
    );
    Ok(())
}
