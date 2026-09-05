//! `inlayHint/resolve` subject identity and currentness (#14672).
//!
//! These tests exercise the public LSP surface only: they drive
//! `textDocument/inlayHint` to obtain a real server-issued item and then hand
//! that item — or a deliberately corrupted variant of it — back through
//! `inlayHint/resolve`.
//!
//! The claim under test is that the resolved label part `location` is derived
//! only from a subject this server issued, in this session, for this exact hint
//! and this exact parsed snapshot. Every negative control below resolved
//! successfully before the migration, because the resolver read `uri` and
//! `functionName` straight out of the client-round-tripped `data`.

use parking_lot::Mutex;
use perl_lsp::LspServer;
use serde_json::{Value, json};
use std::io::{Cursor, Write};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DOC_URI: &str = "file:///tmp/inlay_resolve_identity.pl";

/// Two user-defined subs, each with a call site, so a valid envelope has a
/// second, wrong hint to be moved onto. The bodies deliberately avoid builtin
/// calls: builtin parameter hints also carry an envelope, but they name a
/// callable with no declaration in this document and so have no label location
/// to resolve.
const SOURCE: &str = r#"use strict;
sub greet($name, $greeting) { return "$greeting $name"; }
sub farewell($name, $parting) { return "$parting $name"; }
greet("Alice", "Hello");
farewell("Bob", "Bye");
"#;

fn start_server() -> LspServer {
    LspServer::with_output(Arc::new(Mutex::new(
        Box::new(Cursor::<Vec<u8>>::new(Vec::new())) as Box<dyn Write + Send>
    )))
}

/// Initialize a server advertising inlay hints plus `label.location` resolve
/// support, and open [`SOURCE`] at version 1.
fn open_server() -> Result<LspServer, Box<dyn std::error::Error>> {
    open_server_with_resolve_properties(&["tooltip", "label.location"])
}

/// Initialize and open [`SOURCE`], advertising exactly `properties` as the
/// client's `inlayHint.resolveSupport.properties`.
fn open_server_with_resolve_properties(
    properties: &[&str],
) -> Result<LspServer, Box<dyn std::error::Error>> {
    let server = start_server();
    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "capabilities": {
                "textDocument": {
                    "inlayHint": {
                        "dynamicRegistration": true,
                        "resolveSupport": { "properties": properties }
                    }
                }
            }
        }
    }))?);
    let _ = server.handle_request(serde_json::from_value(
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    )?);
    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {
                "uri": DOC_URI, "languageId": "perl", "version": 1, "text": SOURCE
            }
        }
    }))?);
    Ok(server)
}

/// Drive `textDocument/inlayHint` over the whole document.
fn request_hints(server: &LspServer) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let response = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "id": 2, "method": "textDocument/inlayHint", "params": {
            "textDocument": { "uri": DOC_URI },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 999, "character": 0 }
            }
        }
    }))?);
    Ok(response.and_then(|r| r.result).and_then(|r| r.as_array().cloned()).unwrap_or_default())
}

/// Return the server-issued hints that carry a resolve envelope, in order.
fn resolvable_hints(server: &LspServer) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let hints = request_hints(server)?;
    let resolvable: Vec<Value> = hints
        .iter()
        .filter(|hint| hint.pointer("/data/resolveEnvelope").and_then(Value::as_str).is_some())
        .cloned()
        .collect();
    assert!(
        resolvable.len() >= 2,
        "fixture must yield at least two envelope-bearing hints; got {hints:#?}"
    );
    Ok(resolvable)
}

/// The first envelope-bearing hint the producer recorded against `callable`.
///
/// Selecting by callable keeps the fixture honest: it pins the tests to a hint
/// whose declaration actually exists in this document, rather than whichever
/// hint happens to be emitted first.
fn hint_for(server: &LspServer, callable: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let hints = resolvable_hints(server)?;
    hints
        .into_iter()
        .find(|hint| hint.pointer("/data/functionName").and_then(Value::as_str) == Some(callable))
        .ok_or_else(|| format!("fixture must yield a resolvable hint for {callable}").into())
}

/// Send one item through `inlayHint/resolve`.
fn resolve(server: &LspServer, hint: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let response = server
        .handle_request(serde_json::from_value(json!({
            "jsonrpc": "2.0", "id": 3, "method": "inlayHint/resolve", "params": hint
        }))?)
        .ok_or("inlayHint/resolve produced no response")?;
    Ok(response.result.ok_or("inlayHint/resolve produced no result")?)
}

/// A label part carrying `location`, i.e. the server accepted the subject
/// (LSP 3.17 `InlayHintLabelPart.location`, #14679 shape).
fn resolved_location(resolved: &Value) -> Option<&Value> {
    resolved.get("label")?.as_array()?.iter().find_map(|part| part.get("location"))
}

// ---------------------------------------------------------------------------
// Positive control: the parent provider's own item still resolves.
// ---------------------------------------------------------------------------

#[test]
fn a_hint_from_the_parent_provider_resolves_to_its_declaration() -> TestResult {
    let server = open_server()?;
    let hint = hint_for(&server, "greet")?;

    let resolved = resolve(&server, &hint)?;
    let location = resolved_location(&resolved)
        .ok_or_else(|| format!("a server-issued hint must resolve; got {resolved:#}"))?;

    assert_eq!(
        location.get("uri").and_then(Value::as_str),
        Some(DOC_URI),
        "the location must name the document recorded in the envelope, got {location:#}"
    );
    assert!(location.get("range").is_some(), "resolved location needs a range, got {location:#}");

    // The tooltip path is unchanged by the identity migration.
    assert!(resolved.get("tooltip").is_some(), "tooltip must still be populated");
    Ok(())
}

#[test]
fn a_client_without_label_location_support_receives_no_envelope() -> TestResult {
    let server = open_server_with_resolve_properties(&["tooltip"])?;
    let hints = request_hints(&server)?;

    assert!(!hints.is_empty(), "the fixture must still produce hints");
    assert!(
        hints.iter().all(|hint| hint.pointer("/data/resolveEnvelope").is_none()),
        "no envelope may be minted for a client that cannot redeem it; got {hints:#?}"
    );

    // The hints themselves are unaffected: presentation data still travels.
    assert!(
        hints.iter().any(|hint| hint.pointer("/data/functionName").is_some()),
        "hints must keep their presentation data, got {hints:#?}"
    );
    Ok(())
}

/// The envelope rides in every resolvable hint of every `textDocument/inlayHint`
/// response, so its wire size is a per-response cost multiplied by the hint cap.
/// This pins the measured size so the cost cannot drift silently.
#[test]
fn an_issued_envelope_stays_within_its_documented_wire_budget() -> TestResult {
    let server = open_server()?;
    let hint = hint_for(&server, "greet")?;
    let token = hint
        .pointer("/data/resolveEnvelope")
        .and_then(Value::as_str)
        .ok_or("issued hint must carry an envelope")?;

    // Measured at 1494 bytes for this fixture's URI. The bound leaves headroom
    // for a longer document URI while still catching a structural regression —
    // a new subject field, or a second encoding expansion on top of the
    // substrate's hex encoding, which already costs 2 wire bytes per byte.
    assert!(
        token.len() <= 2048,
        "resolve envelope grew to {} bytes; it is emitted per resolvable hint, up to the \
         inlay-hint cap, so growth here multiplies across every response",
        token.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative control for the original defect.
// ---------------------------------------------------------------------------

#[test]
fn a_fabricated_hint_no_request_produced_does_not_resolve() -> TestResult {
    let server = open_server()?;

    // Exactly the shape the pre-migration resolver honoured: a plausible
    // `data` object naming a real open document and a real subroutine, with no
    // preceding textDocument/inlayHint request at all.
    let fabricated = json!({
        "position": { "line": 3, "character": 6 },
        "label": "name:",
        "kind": 2,
        "data": { "uri": DOC_URI, "functionName": "greet", "paramIndex": 0 }
    });

    let resolved = resolve(&server, &fabricated)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "an item this server never issued must not resolve to a source range; got {resolved:#}"
    );
    Ok(())
}

#[test]
fn a_fabricated_hint_with_presentation_data_does_not_retain_client_locations() -> TestResult {
    let server = open_server()?;
    let fabricated = json!({
        "position": { "line": 3, "character": 6 },
        "label": [{
            "value": "forged:",
            "location": {
                "uri": "file:///attacker-selected",
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                }
            }
        }],
        "kind": 2,
        "tooltip": "forged tooltip",
        "labelDetails": {
            "location": {
                "uri": "file:///attacker-selected",
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                }
            }
        },
        "data": {}
    });

    let resolved = resolve(&server, &fabricated)?;
    assert_eq!(
        resolved.get("tooltip").and_then(Value::as_str),
        Some("forged tooltip"),
        "non-sensitive presentation data may be preserved"
    );
    assert!(
        resolved.get("labelDetails").is_none(),
        "client-supplied labelDetails must be discarded without an authenticated envelope; got {resolved:#}"
    );
    assert!(
        resolved_location(&resolved).is_none(),
        "a client-supplied label part location must be discarded without an authenticated envelope; got {resolved:#}"
    );
    assert_eq!(
        resolved.pointer("/label/0/value").and_then(Value::as_str),
        Some("forged:"),
        "the displayed label text is presentation data and is preserved"
    );
    Ok(())
}

#[test]
fn an_unparseable_envelope_does_not_resolve() -> TestResult {
    let server = open_server()?;
    let mut hint = hint_for(&server, "greet")?;

    hint["data"]["resolveEnvelope"] = json!("not-a-resolve-token");

    let resolved = resolve(&server, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "a malformed token must be refused, got {resolved:#}"
    );
    Ok(())
}

#[test]
fn a_tampered_envelope_does_not_resolve() -> TestResult {
    let server = open_server()?;
    let mut hint = hint_for(&server, "greet")?;

    // Flip one hex digit of the authenticated token body.
    let token = hint
        .pointer("/data/resolveEnvelope")
        .and_then(Value::as_str)
        .ok_or("issued hint must carry an envelope")?
        .to_string();
    let (head, last) = token.split_at(token.len() - 1);
    let flipped = if last == "0" { "1" } else { "0" };
    hint["data"]["resolveEnvelope"] = json!(format!("{head}{flipped}"));

    let resolved = resolve(&server, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "a tampered token must fail integrity, got {resolved:#}"
    );
    Ok(())
}

#[test]
fn an_envelope_from_another_session_does_not_resolve() -> TestResult {
    let issuing = open_server()?;
    let hint = hint_for(&issuing, "greet")?;

    // A second connection with the same document open but its own session key.
    let other = open_server()?;
    let _ = request_hints(&other)?;

    let resolved = resolve(&other, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "an envelope from another session must be refused, got {resolved:#}"
    );

    // Control: the issuing session still accepts its own item, so the refusal
    // above is session identity and not a broken fixture.
    assert!(
        resolved_location(&resolve(&issuing, &hint)?).is_some(),
        "the issuing session must still resolve its own item"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Currentness: the item is bound to the snapshot that produced it.
// ---------------------------------------------------------------------------

#[test]
fn an_edited_document_refuses_a_previously_issued_hint() -> TestResult {
    let server = open_server()?;
    let hint = hint_for(&server, "greet")?;

    // Control: current before the edit.
    assert!(
        resolved_location(&resolve(&server, &hint)?).is_some(),
        "the item must resolve before the document moves"
    );

    let edited = format!("# shift every line\n{SOURCE}");
    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": { "uri": DOC_URI, "version": 2 },
            "contentChanges": [{ "text": edited }]
        }
    }))?);

    let resolved = resolve(&server, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "a hint issued against the previous snapshot must be refused rather than \
         reprojected against the edited source; got {resolved:#}"
    );
    Ok(())
}

/// Close/reopen ABA (#14676 review, found independently by two reviewers).
///
/// `didClose` + `didOpen` on the same URI installs a fresh `DocumentState`
/// whose generation restarts at `FIRST_ACCEPTED_DOCUMENT_GENERATION`, and
/// identical text reproduces the same content hash — so a subject bound only to
/// those two values is revived by a reopen and resolves against a *different*
/// document instance. This is the same hazard
/// `text_sync::document_generation_still_current` closes with `Arc::ptr_eq`.
#[test]
fn a_reopened_document_refuses_a_hint_from_the_previous_instance() -> TestResult {
    let server = open_server()?;
    let hint = hint_for(&server, "greet")?;

    // Control: current before the close/reopen cycle.
    assert!(
        resolved_location(&resolve(&server, &hint)?).is_some(),
        "the item must resolve before the document instance is replaced"
    );

    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didClose", "params": {
            "textDocument": { "uri": DOC_URI }
        }
    }))?);
    // Byte-identical text and version, so generation and content hash both
    // repeat: only the instance identity distinguishes the two opens.
    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {
                "uri": DOC_URI, "languageId": "perl", "version": 1, "text": SOURCE
            }
        }
    }))?);

    let resolved = resolve(&server, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "a hint issued for the previous open instance must not survive a reopen, \
         even on identical text; got {resolved:#}"
    );

    // A hint issued by the new instance still resolves, so the refusal above is
    // instance identity rather than the document having become unresolvable.
    let reissued = hint_for(&server, "greet")?;
    assert!(
        resolved_location(&resolve(&server, &reissued)?).is_some(),
        "a hint issued by the current instance must resolve"
    );
    Ok(())
}

/// Unlike the other negative controls, this one also held before the migration
/// — the legacy resolver returned `None` simply because the document was not in
/// the open-document map. It is kept as a regression guard: the envelope records
/// a URI, and resolving it against an index or on-disk copy instead of the exact
/// open snapshot would silently reintroduce a stale-source read.
#[test]
fn a_closed_document_refuses_a_previously_issued_hint() -> TestResult {
    let server = open_server()?;
    let hint = hint_for(&server, "greet")?;

    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didClose", "params": {
            "textDocument": { "uri": DOC_URI }
        }
    }))?);

    let resolved = resolve(&server, &hint)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "a hint for a closed document must not resolve, got {resolved:#}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The envelope authenticates one exact hint, and siblings in `data` are inert.
// ---------------------------------------------------------------------------

#[test]
fn a_valid_envelope_cannot_be_moved_onto_another_hint() -> TestResult {
    let server = open_server()?;
    let hints = resolvable_hints(&server)?;

    // Find two envelope-bearing hints at genuinely different positions.
    let first = hints[0].clone();
    let other = hints
        .iter()
        .find(|candidate| candidate.get("position") != first.get("position"))
        .ok_or("fixture must yield two hints at different positions")?
        .clone();

    // Keep the second hint's presentation, graft the first hint's envelope.
    let mut grafted = other;
    grafted["data"]["resolveEnvelope"] = first
        .pointer("/data/resolveEnvelope")
        .cloned()
        .ok_or("first hint must carry an envelope")?;

    let resolved = resolve(&server, &grafted)?;
    assert!(
        resolved_location(&resolved).is_none(),
        "an envelope issued for one hint must not resolve another hint, got {resolved:#}"
    );
    Ok(())
}

#[test]
fn sibling_data_fields_cannot_redirect_a_valid_envelope() -> TestResult {
    let server = open_server()?;
    let mut hint = hint_for(&server, "greet")?;

    // Open a second document holding a same-named declaration, then point the
    // legacy sibling fields at it. Only the envelope may choose the document.
    let other_uri = "file:///tmp/inlay_resolve_identity_other.pl";
    let _ = server.handle_request(serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {
                "uri": other_uri, "languageId": "perl", "version": 1,
                "text": "sub greet($name, $greeting) { 1; }\nsub farewell($a, $b) { 1; }\n"
            }
        }
    }))?);

    hint["data"]["uri"] = json!(other_uri);
    hint["data"]["functionName"] = json!("farewell");
    hint["data"]["function"] = json!("farewell");

    let resolved = resolve(&server, &hint)?;
    let location = resolved_location(&resolved)
        .ok_or_else(|| format!("the authenticated subject must still resolve; got {resolved:#}"))?;

    assert_eq!(
        location.get("uri").and_then(Value::as_str),
        Some(DOC_URI),
        "sibling data.uri must not redirect the document, got {location:#}"
    );
    Ok(())
}
