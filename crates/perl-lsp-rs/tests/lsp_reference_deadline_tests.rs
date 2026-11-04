//! Wire contract for deadline-stopped `textDocument/references` requests.
//!
//! This test has its own integration-test process because initialization
//! options update the process-global LSP limits.

mod support;

use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn set_process_reference_deadline(deadline: Duration) -> TestResult {
    let mut limits = perl_lsp_rs_core::runtime::limits::LSP_LIMITS
        .write()
        .map_err(|_| std::io::Error::other("LSP limits lock poisoned"))?;
    limits.reference_search_deadline = deadline;
    Ok(())
}

fn request_references(harness: &mut LspHarness, uri: &str, token: Option<&str>) -> Value {
    let mut params = json!({
        "textDocument": {"uri": uri},
        "position": {"line": 0, "character": 5},
        "context": {"includeDeclaration": true}
    });
    if let Some(token) = token {
        params["partialResultToken"] = json!(token);
    }
    harness.request_raw(json!({
        "jsonrpc": "2.0", "method": "textDocument/references", "params": params
    }))
}

#[test]
#[serial]
fn deadline_exhaustion_is_request_failed_with_or_without_partial_result_token() -> TestResult {
    let uri = "file:///reference_deadline.pl";
    let doc = "my $target = 1;\n$target++;\nprint $target;\n";
    for token in [None, Some("references-partial")] {
        for wrapped in [false, true] {
            // A positive baseline makes every initialization-options form prove
            // that it actually applied zero, instead of inheriting a prior case.
            set_process_reference_deadline(Duration::from_mins(1))?;
            let mut harness = LspHarness::new();
            let limits = json!({"limits": {"referenceSearchDeadlineMs": 0}});
            let options = if wrapped { json!({"perl": limits}) } else { limits };
            harness.initialize_with_init_options(None, options)?;
            assert_eq!(
                perl_lsp_rs_core::runtime::limits::reference_search_deadline(),
                Duration::ZERO,
                "each initialization-options form must apply the zero deadline"
            );
            harness.open_document(uri, doc)?;
            let response = request_references(&mut harness, uri, token);
            assert_eq!(response.pointer("/error/code"), Some(&json!(-32803)), "{response}");
            assert!(
                response
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.contains("deadline")),
                "{response}"
            );
            assert!(
                response.get("result").is_none(),
                "deadline failure must not include a Location[]: {response}"
            );
            let explanation = harness.request_raw(json!({
                "jsonrpc": "2.0",
                "method": "workspace/executeCommand",
                "params": {
                    "command": "perl.explainProviderDecision",
                    "arguments": [{"provider": "references", "request_id": response["id"]}]
                }
            }));
            let receipt = explanation
                .pointer("/result/request_receipt")
                .ok_or_else(|| format!("missing deadline request receipt: {explanation}"))?;
            assert_eq!(receipt["decision"], "blocked", "{receipt}");
            assert_eq!(receipt["reason"], "reference_search_deadline_exceeded", "{receipt}");
            assert_eq!(receipt["terminal_outcome"], "request_failed_deadline", "{receipt}");
            assert_eq!(receipt["result_count"], 0, "{receipt}");
            assert_eq!(receipt["answering_tier"], "none", "{receipt}");
            assert_eq!(receipt["deadline_exhausted"], true, "{receipt}");
            assert!(
                receipt["discarded_candidate_count"].is_number(),
                "discarded candidate count should be explicit: {receipt}"
            );
        }
    }
    Ok(())
}

#[test]
#[serial]
fn complete_search_is_nonempty_location_array_with_or_without_partial_result_token() -> TestResult {
    let uri = "file:///reference_deadline_complete.pl";
    let doc = "my $target = 1;\n$target++;\nprint $target;\n";
    for token in [None, Some("references-complete")] {
        // A zero baseline makes each complete control prove the generous
        // initialization setting instead of inheriting the previous control.
        set_process_reference_deadline(Duration::ZERO)?;
        let mut harness = LspHarness::new();
        harness.initialize_with_init_options(
            None,
            json!({"perl": {"limits": {"referenceSearchDeadlineMs": 60000}}}),
        )?;
        assert_eq!(
            perl_lsp_rs_core::runtime::limits::reference_search_deadline(),
            Duration::from_mins(1),
            "each complete control must apply the generous deadline"
        );
        harness.open_document(uri, doc)?;
        let response = request_references(&mut harness, uri, token);
        assert!(response.get("error").is_none(), "{response}");
        let locations = response.get("result").and_then(Value::as_array);
        assert!(locations.is_some_and(|locations| locations.len() >= 3), "{response}");
    }
    Ok(())
}
