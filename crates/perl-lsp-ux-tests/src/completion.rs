//! Completion-response guards shared by the round-trip benchmark passes.
//!
//! `textDocument/completion` responses only fail the transport on framing
//! errors: a JSON-RPC error and a zero-item answer both arrive as successful
//! decodes. The receipt pass and the criterion regression groups must agree
//! on what counts as a good response, or one pass records samples the other
//! rejects and a lane's numbers flatter themselves (#17731).

use anyhow::{Result, bail, ensure};
use serde_json::Value;

/// Map a completion response to an error when the server answered with a
/// JSON-RPC error — `UxClient::request` only fails on transport errors, so
/// an `error` field must be rejected explicitly or it would silently
/// flatter latency numbers.
pub fn completion_error_guard(response: &Value) -> Result<()> {
    if response.get("error").is_some() {
        bail!("completion returned a JSON-RPC error: {response}");
    }
    Ok(())
}

/// Extract the completion item list with the harness request parse
/// semantics (`result.items` array, bare `result` array, else empty).
pub fn completion_items(response: &Value) -> Vec<Value> {
    response["result"]["items"]
        .as_array()
        .or_else(|| response["result"].as_array())
        .cloned()
        .unwrap_or_default()
}

/// Validate a completion response with the combined guard both benchmark
/// passes apply: a JSON-RPC error or a zero-item list is a failure, never a
/// successful sample (#17731).
pub fn validate_completion_response(response: &Value, lane: &str) -> Result<()> {
    completion_error_guard(response)?;
    ensure!(!completion_items(response).is_empty(), "{lane} returned zero completion items");
    Ok(())
}

/// What a probe response says about one completion coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionProbeOutcome {
    /// The coordinate answered with a non-empty item list.
    Verified,
    /// The coordinate answered successfully but with zero items: probe the
    /// fallback coordinate before sampling anything.
    Empty,
}

/// Classify one completion probe response, error-guard first (#17731): a
/// JSON-RPC error is a failed probe and surfaces as an error — it must not
/// degrade into [`CompletionProbeOutcome::Empty`] and silently switch the
/// lane's subject to the fallback coordinate.
pub fn classify_completion_probe(response: &Value) -> Result<CompletionProbeOutcome> {
    completion_error_guard(response)?;
    if completion_items(response).is_empty() {
        Ok(CompletionProbeOutcome::Empty)
    } else {
        Ok(CompletionProbeOutcome::Verified)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_completion_response;
    use super::{CompletionProbeOutcome, classify_completion_probe, completion_items};
    use anyhow::ensure;
    use serde_json::json;

    const LANE: &str = "completion_guard_test_lane";

    /// A JSON-RPC error response is a failed validation, never a sample:
    /// `client.request` reports transport success for it.
    #[test]
    fn validation_rejects_json_rpc_error() -> anyhow::Result<()> {
        let response =
            json!({"jsonrpc": "2.0", "error": {"code": -32601, "message": "no such method"}});
        let message = validate_completion_response(&response, LANE)
            .err()
            .map(|error| error.to_string())
            .ok_or_else(|| anyhow::anyhow!("an error response must fail validation"))?;
        ensure!(
            message.contains("JSON-RPC error"),
            "the failure must name the JSON-RPC error: {message}"
        );
        Ok(())
    }

    /// An empty `result.items` list is a failed validation: recording it as
    /// a successful sample is the #17731 receipt undercount.
    #[test]
    fn validation_rejects_empty_items() -> anyhow::Result<()> {
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": {"items": []}});
        let message = validate_completion_response(&response, LANE)
            .err()
            .map(|error| error.to_string())
            .ok_or_else(|| anyhow::anyhow!("an empty item list must fail validation"))?;
        ensure!(
            message.contains("zero completion items") && message.contains(LANE),
            "the failure must name the lane and the empty list: {message}"
        );
        Ok(())
    }

    /// A bare empty `result` array parses as an empty item list.
    #[test]
    fn validation_rejects_empty_bare_result() -> anyhow::Result<()> {
        let response = json!({"result": []});
        ensure!(
            validate_completion_response(&response, LANE).is_err(),
            "a bare empty result array must fail validation: {response}"
        );
        Ok(())
    }

    /// Non-empty item lists — nested and bare — validate successfully.
    #[test]
    fn validation_accepts_non_empty_results() -> anyhow::Result<()> {
        let nested = json!({"result": {"items": [{"label": "startup"}]}});
        let bare = json!({"result": [{"label": "startup"}]});
        ensure!(
            validate_completion_response(&nested, LANE).is_ok(),
            "a non-empty result.items list must validate: {nested}"
        );
        ensure!(
            validate_completion_response(&bare, LANE).is_ok(),
            "a non-empty bare result array must validate: {bare}"
        );
        Ok(())
    }

    /// Item parsing follows the harness request semantics: `result.items`
    /// first, then a bare `result` array, else empty.
    #[test]
    fn item_extraction_prefers_result_items() -> anyhow::Result<()> {
        let nested = json!({"result": {"items": [{"label": "a"}]}});
        ensure!(completion_items(&nested).len() == 1, "result.items must be extracted: {nested}");
        let bare = json!({"result": [{"label": "a"}, {"label": "b"}]});
        ensure!(
            completion_items(&bare).len() == 2,
            "a bare result array must be extracted: {bare}"
        );
        let neither = json!({"result": {"something_else": true}});
        ensure!(
            completion_items(&neither).is_empty(),
            "a result without an item list parses as empty: {neither}"
        );
        Ok(())
    }

    /// The #17731 probe-order falsifier: a JSON-RPC error response must
    /// surface as an error, not parse as an empty list and silently select
    /// the fallback coordinate. The old shape checked emptiness first.
    #[test]
    fn classification_errors_before_emptiness() -> anyhow::Result<()> {
        let response =
            json!({"jsonrpc": "2.0", "error": {"code": -32603, "message": "internal error"}});
        let outcome = classify_completion_probe(&response);
        ensure!(
            outcome.is_err(),
            "a failed probe must be an error, not a fallback signal: {outcome:?}"
        );
        Ok(())
    }

    /// A successful zero-item probe reports the fallback signal.
    #[test]
    fn classification_reports_empty() -> anyhow::Result<()> {
        let response = json!({"result": {"items": []}});
        ensure!(
            matches!(classify_completion_probe(&response), Ok(CompletionProbeOutcome::Empty)),
            "a zero-item probe must report Empty: {response}"
        );
        Ok(())
    }

    /// A successful non-empty probe verifies the coordinate.
    #[test]
    fn classification_reports_verified() -> anyhow::Result<()> {
        let response = json!({"result": {"items": [{"label": "startup"}]}});
        ensure!(
            matches!(classify_completion_probe(&response), Ok(CompletionProbeOutcome::Verified)),
            "a non-empty probe must report Verified: {response}"
        );
        Ok(())
    }
}
