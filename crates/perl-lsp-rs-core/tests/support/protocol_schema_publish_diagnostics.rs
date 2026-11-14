//! Direct oracles for the production callbacks used by the publishDiagnostics registry row.
//! Public validator and real-process tests separately prove consumer routing.

use super::{SchemaError, publish_diagnostics_params};
use serde_json::{Value, json};

const METHOD: &str = "textDocument/publishDiagnostics";

fn range() -> Value {
    json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}})
}

fn diagnostic(extra: Value) -> Value {
    let mut item = json!({
        "range": range(),
        "message": "warn"
    });
    if let Some(object) = extra.as_object() {
        for (key, value) in object {
            item[key] = value.clone();
        }
    }
    item
}

fn params(uri: &str, diagnostics: Value, version: Option<Value>) -> Value {
    let mut object = json!({
        "uri": uri,
        "diagnostics": diagnostics
    });
    if let Some(version) = version {
        object["version"] = version;
    }
    object
}

#[test]
fn params_success_is_observed_with_absent_null_and_integer_version() {
    let base = params("file:///main.pl", json!([]), None);
    assert_eq!(publish_diagnostics_params(METHOD, &base), Ok(()));
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params("file:///main.pl", json!([]), Some(Value::Null))
        ),
        Ok(())
    );
    assert_eq!(
        publish_diagnostics_params(METHOD, &params("file:///main.pl", json!([]), Some(json!(7)))),
        Ok(())
    );
}

#[test]
fn populated_diagnostic_fields_are_observed_for_string_and_integer_codes() {
    let diagnostics = json!([{
        "range": range(),
        "message": "detail",
        "severity": 2,
        "code": "PL002",
        "source": "perl-lsp",
        "tags": [1, 2],
        "relatedInformation": [{
            "location": {
                "uri": "file:///lib.pm",
                "range": range()
            },
            "message": "imported here"
        }],
        "codeDescription": {"href": "file:///docs/PL002.html"}
    }]);
    assert_eq!(
        publish_diagnostics_params(METHOD, &params("file:///main.pl", diagnostics.clone(), None)),
        Ok(())
    );
    let mut with_numeric_code = diagnostics;
    with_numeric_code[0]["code"] = json!(42);
    assert_eq!(
        publish_diagnostics_params(METHOD, &params("file:///main.pl", with_numeric_code, None)),
        Ok(())
    );
}

#[test]
fn missing_diagnostics_reports_the_exact_error() {
    assert_eq!(
        publish_diagnostics_params(METHOD, &json!({"uri": "file:///main.pl"})),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics".to_string(),
            expected: "array".to_string(),
            observed: "missing".to_string(),
        })
    );
}

#[test]
fn non_array_diagnostics_reports_the_exact_payload_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &json!({"uri": "file:///main.pl", "diagnostics": false})
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics".to_string(),
            expected: "array".to_string(),
            observed: "boolean:false".to_string(),
        })
    );
}

#[test]
fn out_of_range_severity_reports_the_exact_severity_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params("file:///main.pl", json!([diagnostic(json!({"severity": 0}))]), None)
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics[0].severity".to_string(),
            expected: "DiagnosticSeverity 1..=4".to_string(),
            observed: "0".to_string(),
        })
    );
}

#[test]
fn out_of_range_numeric_code_reports_the_exact_integer_bounds_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params("file:///main.pl", json!([diagnostic(json!({"code": 2147483648_i64}))]), None)
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics[0].code".to_string(),
            expected: "integer within -2147483648..=2147483647".to_string(),
            observed: "2147483648".to_string(),
        })
    );
}

#[test]
fn non_array_tags_reports_the_exact_payload_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params("file:///main.pl", json!([diagnostic(json!({"tags": "bad"}))]), None)
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics[0].tags".to_string(),
            expected: "array".to_string(),
            observed: "string(len=3)".to_string(),
        })
    );
}

#[test]
fn out_of_range_tag_reports_the_exact_tag_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params("file:///main.pl", json!([diagnostic(json!({"tags": [3]}))]), None)
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics[0].tags[0]".to_string(),
            expected: "DiagnosticTag 1 or 2".to_string(),
            observed: "3".to_string(),
        })
    );
}

#[test]
fn invalid_code_description_href_reports_the_exact_uri_error() {
    assert_eq!(
        publish_diagnostics_params(
            METHOD,
            &params(
                "file:///main.pl",
                json!([diagnostic(json!({"codeDescription": {"href": "not-a-uri"}}))]),
                None
            )
        ),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.diagnostics[0].codeDescription.href".to_string(),
            expected: "URI string with a scheme separator".to_string(),
            observed: "string(len=9)".to_string(),
        })
    );
}
