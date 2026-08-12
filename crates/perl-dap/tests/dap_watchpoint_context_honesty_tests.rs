use perl_dap::{DapMessage, DebugAdapter};
use serde_json::{json, Value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data_breakpoint_info(
    adapter: &mut DebugAdapter,
    arguments: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    match adapter.handle_request(1, "dataBreakpointInfo", Some(arguments)) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        DapMessage::Response { success, message, .. } => Err(format!(
            "dataBreakpointInfo returned success={success}: {}",
            message.unwrap_or_else(|| "<no message>".to_string())
        )
        .into()),
        other => Err(format!("unexpected dataBreakpointInfo response: {other:?}").into()),
    }
}

fn data_id_is_explicit_null(body: &Value) -> bool {
    body.get("dataId").is_some_and(Value::is_null)
}

#[test]
fn variables_reference_is_not_silently_ignored() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(
        &mut adapter,
        json!({
            "name": "$value",
            "variablesReference": 11
        }),
    )?;

    assert!(
        data_id_is_explicit_null(&body),
        "an unvalidated variablesReference must emit dataId: null: {body}"
    );
    let description = body.get("description").and_then(Value::as_str).unwrap_or_default();
    assert!(
        description.contains("variablesReference") && description.contains("unproven"),
        "context refusal must name the unproven container: {description:?}"
    );
    assert!(body.get("accessTypes").is_none(), "unsupported contextual target has no accessTypes");
    Ok(())
}

#[test]
fn frame_id_is_not_silently_ignored() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(
        &mut adapter,
        json!({
            "name": "$value",
            "frameId": 7
        }),
    )?;

    assert!(
        data_id_is_explicit_null(&body),
        "an unvalidated frameId must emit dataId: null: {body}"
    );
    let description = body.get("description").and_then(Value::as_str).unwrap_or_default();
    assert!(
        description.contains("frameId") && description.contains("unproven"),
        "context refusal must name the unproven stopped frame: {description:?}"
    );
    assert!(body.get("accessTypes").is_none(), "unsupported contextual target has no accessTypes");
    Ok(())
}

#[test]
fn variables_reference_precedes_frame_id_when_both_are_present() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(
        &mut adapter,
        json!({
            "name": "$value",
            "variablesReference": 11,
            "frameId": 7
        }),
    )?;

    assert!(data_id_is_explicit_null(&body));
    let description = body.get("description").and_then(Value::as_str).unwrap_or_default();
    assert!(
        description.contains("variablesReference"),
        "variablesReference must own precedence when both context forms are present: {description:?}"
    );
    Ok(())
}

#[test]
fn zero_context_values_are_still_explicit_context() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(
        &mut adapter,
        json!({
            "name": "$value",
            "variablesReference": 0,
            "frameId": 0
        }),
    )?;

    assert!(
        data_id_is_explicit_null(&body),
        "explicit zero context must emit dataId: null: {body}"
    );
    Ok(())
}

#[test]
fn invalid_expression_remains_invalid_before_context_classification() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(
        &mut adapter,
        json!({
            "name": "$value; system('id')",
            "variablesReference": 11
        }),
    )?;

    assert!(data_id_is_explicit_null(&body));
    assert_eq!(
        body.get("description").and_then(Value::as_str),
        Some("Cannot watch this expression")
    );
    Ok(())
}

#[test]
fn context_free_compatibility_path_is_preserved() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = data_breakpoint_info(&mut adapter, json!({ "name": "$value" }))?;

    assert_eq!(body.get("dataId").and_then(Value::as_str), Some("$value"));
    assert_eq!(
        body.get("accessTypes")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("write")
    );
    Ok(())
}
