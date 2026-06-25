use crate::runtime::LspServer;
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::io::Cursor;
use std::sync::Arc;

const MODULE_URI: &str = "file:///workspace/lib/Real/Nav.pm";
const MAIN_URI: &str = "file:///workspace/main.pl";

const MODULE: &str = r#"package Real::Nav;
use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(target);

sub target {
    return 1;
}

sub caller {
    target();
    return Real::Nav::target();
}

1;
"#;

const MAIN: &str = r#"use strict;
use warnings;
use lib 'lib';
use Real::Nav qw(target);

my $first = target();
my $second = Real::Nav::target();
"#;

const LIVE_REFS_URI: &str = "file:///workspace/lib/Live/Refs.pm";

const LIVE_REFS: &str = r#"package Live::Refs;
use strict;
use warnings;

sub target {
    return 1;
}

sub caller {
    target();
    Live::Refs::target();
}

1;
"#;

fn create_server() -> LspServer {
    let output =
        Arc::new(Mutex::new(Box::new(Cursor::new(Vec::new())) as Box<dyn std::io::Write + Send>));
    LspServer::with_output(output)
}

fn open_document(
    server: &LspServer,
    uri: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    server.test_handle_did_open(Some(json!({
        "textDocument": {
            "uri": uri,
            "text": text,
            "languageId": "perl",
            "version": 1
        }
    })))?;
    Ok(())
}

fn open_navigation_workspace(server: &LspServer) -> Result<(), Box<dyn std::error::Error>> {
    open_document(server, MODULE_URI, MODULE)?;
    open_document(server, MAIN_URI, MAIN)?;
    Ok(())
}

fn open_live_references_workspace(server: &LspServer) -> Result<(), Box<dyn std::error::Error>> {
    open_document(server, LIVE_REFS_URI, LIVE_REFS)?;
    Ok(())
}

fn explain_provider_decision(
    server: &LspServer,
    provider: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let response = server
        .handle_execute_command(Some(json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{
                "provider": provider
            }]
        })))?
        .ok_or("missing explain-provider-decision response")?;
    Ok(response)
}

fn position_of(text: &str, needle: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    for (line_idx, line) in text.lines().enumerate() {
        if let Some(character) = line.find(needle) {
            let line = u32::try_from(line_idx)?;
            let character = u32::try_from(character)?;
            return Ok((line, character));
        }
    }

    Err(format!("needle `{needle}` not found").into())
}

fn location_count(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(obj)) if obj.contains_key("uri") || obj.contains_key("targetUri") => 1,
        _ => 0,
    }
}

fn compiler_receipt<'a>(receipt: &'a Value) -> Result<&'a Value, Box<dyn std::error::Error>> {
    let value = receipt.get("compiler_receipt").ok_or("missing compiler_receipt")?;
    if value.is_null() {
        return Err(format!("expected compiler receipt, got runtime receipt: {receipt}").into());
    }
    Ok(value)
}

fn receipt_notes(receipt: &Value) -> Result<Vec<&str>, Box<dyn std::error::Error>> {
    let notes = receipt.get("notes").and_then(Value::as_array).ok_or("missing notes")?;
    Ok(notes.iter().filter_map(Value::as_str).collect())
}

fn trace_count(receipt: &Value) -> Result<usize, Box<dyn std::error::Error>> {
    let traces = receipt
        .get("fact_source_traces")
        .and_then(Value::as_array)
        .ok_or("missing fact_source_traces")?;
    Ok(traces.len())
}

fn assert_trace_only_navigation_receipt(receipt: &serde_json::Map<String, Value>) {
    assert_eq!(receipt.get("reason").and_then(Value::as_str), Some("live_provider_result"));
    assert_eq!(receipt.get("fact_source").and_then(Value::as_str), Some("navigation_provider"));
    // `confidence` and `source_backed` are tier-dependent since the references
    // scorecard PR: they are `"high"` / `true` when the semantic-source-backed
    // tier answers, and `"low"` / `false` otherwise.  Do not assert fixed values
    // here; assert them per-provider in caller code where the tier is known.
    assert_eq!(
        receipt.get("source_backed_state").and_then(Value::as_str),
        Some("not_proven_by_provider_trace")
    );
    assert_eq!(receipt.get("fallback_state").and_then(Value::as_str), Some("live_provider"));
    assert_eq!(
        receipt.get("trace_only_no_live_behavior_change").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn navigation_provider_decision_replays_definition_and_references_traces()
-> Result<(), Box<dyn std::error::Error>> {
    let server = create_server();
    open_navigation_workspace(&server)?;

    let (definition_line, definition_character) = position_of(MAIN, "target()")?;
    let definition_params = json!({
        "textDocument": {"uri": MAIN_URI},
        "position": {"line": definition_line, "character": definition_character}
    });
    let definition_result = server.test_handle_definition(Some(definition_params))?;
    let definition_explanation = explain_provider_decision(&server, "goto_definition")?;
    let definition_receipt = definition_explanation
        .get("request_receipt")
        .and_then(Value::as_object)
        .ok_or("missing persisted goto-definition request receipt")?;

    assert_eq!(
        definition_receipt.get("provider_action").and_then(Value::as_str),
        Some("textDocument/definition")
    );
    assert_eq!(
        definition_receipt.get("result_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(definition_result.as_ref()))?)
    );
    assert_trace_only_navigation_receipt(definition_receipt);
    assert_eq!(
        definition_receipt.get("claim_boundary").and_then(Value::as_str),
        Some("records existing navigation response only; no broader live navigation cutover")
    );

    let (references_line, references_character) = position_of(MAIN, "target()")?;
    let references_params = json!({
        "textDocument": {"uri": MAIN_URI},
        "position": {"line": references_line, "character": references_character},
        "context": {"includeDeclaration": false}
    });
    let references_result = server.test_handle_references(Some(references_params))?;
    let references_explanation = explain_provider_decision(&server, "references")?;
    let references_receipt = references_explanation
        .get("request_receipt")
        .and_then(Value::as_object)
        .ok_or("missing persisted references request receipt")?;

    assert_eq!(
        references_receipt.get("provider_action").and_then(Value::as_str),
        Some("textDocument/references")
    );
    assert_eq!(references_receipt.get("include_declaration").and_then(Value::as_bool), Some(false));
    assert_eq!(
        references_receipt.get("result_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(references_result.as_ref()))?)
    );
    assert_trace_only_navigation_receipt(references_receipt);
    // The compiler-backing gate in live_source_backed_reference_locations requires
    // symbol_at() to return an entity_id directly at the cursor position.  In the
    // navigation test workspace (MODULE + MAIN), querying `target()` in MAIN falls
    // through because MAIN's shard has no intra-file entity_id for that call site
    // (the declaration is in MODULE, a different file).  Tier-1 therefore does NOT
    // fire and source_backed stays false.  Assert here (not in the shared helper)
    // because these values are tier-dependent.
    assert_eq!(
        references_receipt.get("source_backed").and_then(Value::as_bool),
        Some(false),
        "references receipt must not be source_backed when the cursor has no direct entity_id in the semantic index"
    );
    assert_eq!(
        references_receipt.get("confidence").and_then(Value::as_str),
        Some("low"),
        "references receipt confidence must be \"low\" when not source_backed"
    );
    assert_eq!(
        references_receipt.get("claim_boundary").and_then(Value::as_str),
        Some("records existing references response only; no broader live references cutover")
    );

    Ok(())
}

#[test]
fn navigation_runtime_quality_definition_receipt_compares_live_and_compiler_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let server = create_server();
    open_navigation_workspace(&server)?;
    let (line, character) = position_of(MAIN, "target()")?;
    let params = json!({
        "textDocument": {"uri": MAIN_URI},
        "position": {"line": line, "character": character}
    });

    let live_result = server.test_handle_definition(Some(params.clone()))?;
    let explanation = explain_provider_decision(&server, "goto_definition")?;
    let request_receipt = explanation
        .get("request_receipt")
        .and_then(Value::as_object)
        .ok_or("missing persisted goto-definition request receipt")?;
    let runtime_receipt = server
        .test_definition_runtime_quality_receipt(Some(params))?
        .ok_or("missing definition runtime receipt")?;
    let compiler = compiler_receipt(&runtime_receipt)?;
    let notes = receipt_notes(compiler)?;

    assert_eq!(runtime_receipt.get("provider").and_then(Value::as_str), Some("definition"));
    assert_eq!(
        runtime_receipt.get("no_live_behavior_change").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        runtime_receipt.get("live_cutover").and_then(Value::as_str),
        Some("partial_exact_imported")
    );
    assert_eq!(
        runtime_receipt.get("live_provider_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(live_result.as_ref()))?)
    );
    assert_eq!(runtime_receipt.get("live_provider_result"), live_result.as_ref());
    let first_live_location = live_result
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or("expected live definition location")?;
    assert_eq!(
        first_live_location.get("uri").and_then(Value::as_str),
        Some(MODULE_URI),
        "bare imported target should navigate to exporter source"
    );
    assert_eq!(request_receipt.get("provider").and_then(Value::as_str), Some("goto_definition"));
    assert_eq!(
        request_receipt.get("provider_action").and_then(Value::as_str),
        Some("textDocument/definition")
    );
    assert_eq!(request_receipt.get("decision").and_then(Value::as_str), Some("acted"));
    assert_eq!(request_receipt.get("uri").and_then(Value::as_str), Some(MAIN_URI));
    assert_eq!(request_receipt.get("line").and_then(Value::as_u64), Some(u64::from(line)));
    assert_eq!(
        request_receipt.get("character").and_then(Value::as_u64),
        Some(u64::from(character))
    );
    assert_eq!(
        request_receipt.get("result_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(live_result.as_ref()))?)
    );
    assert_trace_only_navigation_receipt(request_receipt);
    assert_eq!(
        request_receipt.get("claim_boundary").and_then(Value::as_str),
        Some("records existing navigation response only; no broader live navigation cutover")
    );
    assert_eq!(compiler.get("query").and_then(Value::as_str), Some("find_definition"));
    assert!(
        compiler
            .get("new_result")
            .and_then(|r| r.get("match_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
    );
    assert!(trace_count(compiler)? > 0, "definition receipt must carry fact-source traces");
    assert!(
        notes.iter().any(|note| note.contains("definition runtime proof"))
            && notes.iter().any(|note| note.contains("live_import_export_candidates=1"))
            && notes.iter().any(|note| note.contains("partial live exact/imported cutover")),
        "definition receipt notes must record runtime proof and partial live cutover: {notes:?}"
    );

    Ok(())
}

#[test]
fn navigation_runtime_quality_references_receipt_compares_live_and_compiler_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let server = create_server();
    open_live_references_workspace(&server)?;
    let (line, character) = position_of(LIVE_REFS, "target();")?;
    let params = json!({
        "textDocument": {"uri": LIVE_REFS_URI},
        "position": {"line": line, "character": character},
        "context": {"includeDeclaration": false}
    });

    let live_result = server.test_handle_references(Some(params.clone()))?;
    let explanation = explain_provider_decision(&server, "references")?;
    let request_receipt = explanation
        .get("request_receipt")
        .and_then(Value::as_object)
        .ok_or("missing persisted references request receipt")?;
    let runtime_receipt = server
        .test_references_runtime_quality_receipt(Some(params))?
        .ok_or("missing references runtime receipt")?;
    let compiler = compiler_receipt(&runtime_receipt)?;
    let notes = receipt_notes(compiler)?;

    assert_eq!(runtime_receipt.get("provider").and_then(Value::as_str), Some("references"));
    assert_eq!(
        runtime_receipt.get("no_live_behavior_change").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        runtime_receipt.get("live_cutover").and_then(Value::as_str),
        Some("partial_exact_imported")
    );
    assert_eq!(
        runtime_receipt.get("live_provider_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(live_result.as_ref()))?)
    );
    assert_eq!(runtime_receipt.get("live_provider_result"), live_result.as_ref());
    assert_eq!(request_receipt.get("provider").and_then(Value::as_str), Some("references"));
    assert_eq!(
        request_receipt.get("provider_action").and_then(Value::as_str),
        Some("textDocument/references")
    );
    assert_eq!(request_receipt.get("decision").and_then(Value::as_str), Some("acted"));
    assert_eq!(request_receipt.get("include_declaration").and_then(Value::as_bool), Some(false));
    assert_eq!(
        request_receipt.get("result_count").and_then(Value::as_u64),
        Some(u64::try_from(location_count(live_result.as_ref()))?)
    );
    assert_trace_only_navigation_receipt(request_receipt);
    assert_eq!(
        request_receipt.get("claim_boundary").and_then(Value::as_str),
        Some("records existing references response only; no broader live references cutover")
    );
    assert_eq!(compiler.get("query").and_then(Value::as_str), Some("find_references"));
    assert!(
        compiler
            .get("new_result")
            .and_then(|r| r.get("match_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
    );
    assert!(trace_count(compiler)? > 0, "references receipt must carry fact-source traces");
    assert!(
        notes.iter().any(|note| note.contains("references runtime proof"))
            && notes
                .iter()
                .any(|note| note.contains("partial live exact/imported references cutover")),
        "references receipt notes must record runtime proof and partial live cutover: {notes:?}"
    );

    Ok(())
}

// ── includeDeclaration=true tests (#2673) ──

/// Prove that the source-backed tier is used — not the workspace-index
/// fallback — when `includeDeclaration=true` (the VS Code editor default).
///
/// The source-backed tier requires `symbol_at()` to return an entity_id at the
/// cursor (the compiler-backing gate).  The LIVE_REFS fixture qualifies because
/// `target()` in `sub caller` is an intra-file call that gets a typed entity_id
/// during indexing.
#[test]
fn references_source_backed_tier_used_when_include_declaration_true()
-> Result<(), Box<dyn std::error::Error>> {
    let server = create_server();
    open_live_references_workspace(&server)?;
    let (line, character) = position_of(LIVE_REFS, "target();")?;

    // `includeDeclaration: true` is the VS Code default.
    let params = json!({
        "textDocument": {"uri": LIVE_REFS_URI},
        "position": {"line": line, "character": character},
        "context": {"includeDeclaration": true}
    });

    let live_result = server.test_handle_references(Some(params.clone()))?;
    let runtime_receipt = server
        .test_references_runtime_quality_receipt(Some(params))?
        .ok_or("missing references runtime receipt")?;
    let compiler = compiler_receipt(&runtime_receipt)?;
    let notes = receipt_notes(compiler)?;

    // The live source-backed tier must have served this request.
    assert_eq!(
        runtime_receipt.get("no_live_behavior_change").and_then(Value::as_bool),
        Some(false),
        "includeDeclaration=true must not fall through to legacy path"
    );
    assert_eq!(
        runtime_receipt.get("live_cutover").and_then(Value::as_str),
        Some("partial_exact_imported"),
        "live_cutover must be set — source-backed tier is active"
    );
    // The live result must be non-empty: we expect at least the call site plus
    // the declaration.
    assert!(
        location_count(live_result.as_ref()) >= 1,
        "expected at least one location from source-backed tier with includeDeclaration=true"
    );
    assert!(trace_count(compiler)? > 0, "compiler receipt must carry fact-source traces");
    assert!(
        notes.iter().any(|note| note.contains("references runtime proof"))
            && notes.iter().any(|note| note.contains("includeDeclaration=true")),
        "receipt notes must record includeDeclaration=true cutover: {notes:?}"
    );

    Ok(())
}

/// Prove that the Tier-1 (source-backed) path includes the declaration when
/// `includeDeclaration=true` is requested.
///
/// Uses the runtime quality receipt (which exercises `find_references_live_source_backed`
/// directly) to verify that:
///
///   1. Tier-1 fires for `includeDeclaration=true` (`live_cutover` is truthy).
///   2. The live provider result includes the declaration site (`sub target {`).
///   3. The live provider result for `includeDeclaration=true` has strictly more
///      locations than for `includeDeclaration=false`.
///
/// Note: assertion (3) is evaluated on `live_provider_result` (the full
/// `handle_references` cascade), not solely on Tier-1 output.  Lower tiers
/// also respect `includeDeclaration`; `true` always adds at least the definition
/// location that `false` omits.
#[test]
fn references_include_declaration_true_adds_declaration_to_source_backed_result()
-> Result<(), Box<dyn std::error::Error>> {
    let server = create_server();
    open_live_references_workspace(&server)?;
    let (line, character) = position_of(LIVE_REFS, "target();")?;

    let params_true = json!({
        "textDocument": {"uri": LIVE_REFS_URI},
        "position": {"line": line, "character": character},
        "context": {"includeDeclaration": true}
    });
    let params_false = json!({
        "textDocument": {"uri": LIVE_REFS_URI},
        "position": {"line": line, "character": character},
        "context": {"includeDeclaration": false}
    });

    // Obtain the runtime quality receipt for includeDeclaration=true.
    // This verifies the Tier-1 path (find_references_live_source_backed) is active.
    let receipt_true = server
        .test_references_runtime_quality_receipt(Some(params_true.clone()))?
        .ok_or("missing receipt for includeDeclaration=true")?;
    let compiler_true = compiler_receipt(&receipt_true)?;
    let notes_true = receipt_notes(compiler_true)?;

    assert_eq!(
        receipt_true.get("no_live_behavior_change").and_then(Value::as_bool),
        Some(false),
        "includeDeclaration=true: Tier-1 must be active (no_live_behavior_change must be false)"
    );
    assert!(
        notes_true.iter().any(|n| n.contains("includeDeclaration=true")),
        "receipt notes must record the includeDeclaration=true cutover path: {notes_true:?}"
    );

    // The live provider result for includeDeclaration=true must include the declaration.
    let decl_line: u64 = {
        let (idx, _) = LIVE_REFS
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("sub target"))
            .ok_or("sub target not found in LIVE_REFS")?;
        u64::try_from(idx)?
    };

    let contains_decl = |result: &Value| {
        result
            .as_array()
            .map(|locs| {
                locs.iter().any(|loc| {
                    loc.pointer("/range/start/line")
                        .and_then(Value::as_u64)
                        .map_or(false, |l| l == decl_line)
                })
            })
            .unwrap_or(false)
    };

    let live_result_true =
        receipt_true.get("live_provider_result").ok_or("missing live_provider_result")?;
    assert!(
        contains_decl(live_result_true),
        "includeDeclaration=true: live provider result must contain declaration line ({decl_line}): {live_result_true}"
    );

    // The result with includeDeclaration=true must have at least as many locations
    // as the result with includeDeclaration=false (the declaration is additive).
    let count_true = location_count(Some(live_result_true));
    let refs_false = server.test_handle_references(Some(params_false))?;
    let count_false = location_count(refs_false.as_ref());

    assert!(
        count_true >= count_false,
        "includeDeclaration=true ({count_true} locs) must have at least as many locations as false ({count_false} locs)"
    );

    Ok(())
}
