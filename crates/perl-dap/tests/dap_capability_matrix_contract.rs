use perl_dap::{DapMessage, DebugAdapter};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;
use std::path::Path;

type TestResult = Result<(), Box<dyn Error>>;

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::other(message.into()))
}

fn json_wire_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[test]
fn initialize_values_match_the_matrix_wire_shapes() -> TestResult {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("perl-dap manifest must be nested under <root>/crates"))?;
    let matrix_path = root.join(".ci/dap/capability-matrix.json");
    let matrix_text = std::fs::read_to_string(&matrix_path)?;
    let matrix: Value = serde_json::from_str(&matrix_text)?;
    let rows = matrix
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| failure("capability matrix rows must be an array"))?;

    let mut expected = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let object = row
            .as_object()
            .ok_or_else(|| failure(format!("matrix row {index} must be an object")))?;
        let name = object
            .get("wire_name")
            .and_then(Value::as_str)
            .ok_or_else(|| failure(format!("matrix row {index} lacks wire_name")))?;
        let wire_type = object
            .get("wire_type")
            .and_then(Value::as_str)
            .ok_or_else(|| failure(format!("matrix row {index} lacks wire_type")))?;
        if expected
            .insert(name.to_string(), wire_type.to_string())
            .is_some()
        {
            return Err(failure(format!("duplicate capability matrix row: {name}")));
        }
    }

    let mut adapter = DebugAdapter::new();
    let body = match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response {
            success: true,
            body: Some(body),
            ..
        } => body,
        other => {
            return Err(failure(format!(
                "initialize did not return a successful body: {other:?}"
            )));
        }
    };
    let capabilities = body
        .as_object()
        .ok_or_else(|| failure("initialize body must be an object"))?;

    let expected_names: BTreeSet<_> = expected.keys().cloned().collect();
    let actual_names: BTreeSet<_> = capabilities.keys().cloned().collect();
    assert_eq!(
        actual_names, expected_names,
        "initialize field set must equal the capability matrix"
    );

    for (name, expected_type) in expected {
        let value = capabilities
            .get(&name)
            .ok_or_else(|| failure(format!("initialize omitted matrix field {name}")))?;
        assert_eq!(
            json_wire_type(value),
            expected_type,
            "initialize field {name} has the wrong JSON wire shape"
        );
    }
    Ok(())
}
