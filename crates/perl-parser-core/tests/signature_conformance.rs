//! Independently authored signature contracts (#8912). The ignored report is
//! deliberately failing until production conformance is established.
use perl_parser_core::{
    Node, NodeKind, ParseError, ParseOutput, ParseStopCause, Parser, RecoverySalvageClass,
    RecoverySalvageProfile,
};
use serde_json::{Value, json};
use std::error::Error;
use std::path::PathBuf;
type R = Result<(), Box<dyn Error>>;
const MATRIX: &str = include_str!("../../../.ci/signature-conformance/cases.json");

fn cases() -> Result<Vec<Value>, Box<dyn Error>> {
    let matrix: Value = serde_json::from_str(MATRIX)?;
    if matrix["schema"] != "signature-conformance/v1" {
        return Err("wrong matrix schema".into());
    }
    matrix["cases"].as_array().cloned().ok_or_else(|| "missing cases".into())
}
fn subroutine(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Subroutine { name: Some(name), .. } if name == "f") {
        return Some(node);
    }
    node.children().into_iter().find_map(subroutine)
}
fn span(node: &Node) -> Value {
    json!([node.location.start, node.location.end])
}
fn compare(label: &str, actual: &Value, expected: &Value, failures: &mut Vec<String>) {
    if actual != expected {
        failures.push(format!("{label}: expected {expected}, got {actual}"));
    }
}
fn disposition(output: &ParseOutput) -> Option<bool> {
    if output.stop_cause().is_some() {
        return None;
    }
    let salvage = RecoverySalvageProfile::from_parse(&output.ast, &output.diagnostics, false);
    Some(salvage.class == RecoverySalvageClass::Clean)
}
fn observe(case: &Value) -> Result<Value, Box<dyn Error>> {
    let source = case["source"].as_str().ok_or("source missing")?;
    observe_output(case, &Parser::new(source).parse_with_recovery())
}
fn observe_output(case: &Value, output: &ParseOutput) -> Result<Value, Box<dyn Error>> {
    let source = case["source"].as_str().ok_or("source missing")?;
    let expected = &case["native"];
    let diagnostics: Vec<_> = output.diagnostics.iter().map(ToString::to_string).collect();
    let accepted = disposition(output);
    let mut failures = Vec::new();
    if let Some(accepted) = accepted {
        compare("disposition", &json!(accepted), &expected["accepted"], &mut failures);
    } else {
        failures
            .push("NOT_PROVEN: parser terminated before a complete language disposition".into());
    }
    let mut observed = json!({"accepted": accepted, "diagnostics": diagnostics, "stop_cause": output.stop_cause().map(|cause| format!("{cause:?}")), "error_node_count": RecoverySalvageProfile::from_parse(&output.ast, &output.diagnostics, output.terminated_early()).error_node_count});
    if expected["accepted"] == true {
        if let Some(Node { kind: NodeKind::Subroutine { prototype, signature, .. }, .. }) =
            subroutine(&output.ast)
        {
            let header = signature.as_deref().or(prototype.as_deref());
            if let Some(header) = header {
                observed["header_kind"] = json!(header.kind.kind_name());
                observed["header_span"] = span(header);
                compare(
                    "header kind",
                    &observed["header_kind"],
                    &expected["header_kind"],
                    &mut failures,
                );
                compare("header span", &span(header), &expected["header_span"], &mut failures);
                let parameters = match &header.kind {
                    NodeKind::Signature { parameters } => parameters.as_slice(),
                    _ => &[],
                };
                let wanted = expected["parameters"].as_array().ok_or("parameters missing")?;
                compare(
                    "parameter count",
                    &json!(parameters.len()),
                    &json!(wanted.len()),
                    &mut failures,
                );
                let mut parameter_observations = Vec::new();
                for (index, (parameter, want)) in parameters.iter().zip(wanted).enumerate() {
                    let label = format!("parameter {index}");
                    let mut actual =
                        json!({"kind": parameter.kind.kind_name(), "span": span(parameter)});
                    compare(
                        &format!("{label} kind"),
                        &actual["kind"],
                        &want["kind"],
                        &mut failures,
                    );
                    compare(
                        &format!("{label} span"),
                        &span(parameter),
                        &want["span"],
                        &mut failures,
                    );
                    let (variable, default) = match &parameter.kind {
                        NodeKind::MandatoryParameter { variable }
                        | NodeKind::SlurpyParameter { variable } => (Some(variable.as_ref()), None),
                        NodeKind::OptionalParameter {
                            variable,
                            default_value,
                            default_operator,
                            default_operator_span,
                        } => {
                            actual["operator"] = json!({"text":default_operator,"span":[default_operator_span.start,default_operator_span.end]});
                            (Some(variable.as_ref()), Some(default_value.as_ref()))
                        }
                        NodeKind::NamedParameter {
                            variable,
                            default_value,
                            default_operator,
                            default_operator_span,
                            external_name,
                            required,
                        } => {
                            actual["operator"] = match (default_operator, default_operator_span) {
                                (Some(operator), Some(range)) => {
                                    json!({"text":operator,"span":[range.start,range.end]})
                                }
                                (None, None) => Value::Null,
                                _ => {
                                    failures
                                        .push(format!("{label}: inconsistent operator metadata"));
                                    Value::Null
                                }
                            };
                            actual["external_name"] = json!(external_name);
                            actual["required"] = json!(required);
                            compare(
                                &format!("{label} external name"),
                                &actual["external_name"],
                                &want["external_name"],
                                &mut failures,
                            );
                            compare(
                                &format!("{label} required"),
                                &actual["required"],
                                &want["required"],
                                &mut failures,
                            );
                            if want.get("operator").is_none() && default_operator.is_some() {
                                failures.push(format!("{label}: unexpected default operator"));
                            }
                            if want.get("operator").is_some() {
                                compare(
                                    &format!("{label} operator text"),
                                    &json!(default_operator),
                                    &want["operator"]["text"],
                                    &mut failures,
                                );
                            }
                            (Some(variable.as_ref()), default_value.as_deref())
                        }
                        _ => (None, None),
                    };
                    for (role, node) in [("variable", variable), ("default", default)] {
                        if want.get(role).is_none() && node.is_some() {
                            failures.push(format!("{label}: unexpected {role}"));
                        }
                        if let Some(want_field) = want.get(role) {
                            if let Some(node) = node {
                                let text = source.get(node.location.start..node.location.end);
                                actual[role] = json!({"span": span(node), "text": text});
                                compare(
                                    &format!("{label} {role} span"),
                                    &span(node),
                                    &want_field["span"],
                                    &mut failures,
                                );
                                compare(
                                    &format!("{label} {role} text"),
                                    &json!(text),
                                    &want_field["text"],
                                    &mut failures,
                                );
                            } else {
                                failures.push(format!("{label}: missing {role}"));
                            }
                        }
                    }
                    if let Some(wanted_operator) = want.get("operator") {
                        compare(
                            &format!("{label} operator"),
                            &actual["operator"],
                            wanted_operator,
                            &mut failures,
                        );
                    } else if !actual["operator"].is_null() {
                        failures.push(format!("{label}: unexpected operator geometry"));
                    }
                    parameter_observations.push(actual);
                }
                observed["parameters"] = json!(parameter_observations);
            } else {
                failures.push("missing callable header".into());
            }
        } else {
            failures.push("missing subroutine f".into());
        }
    }
    Ok(
        json!({"status": if accepted.is_none() { "NOT_PROVEN" } else { "OBSERVED" }, "id": case["id"], "source": source, "expected": expected, "observed": observed, "failures": failures}),
    )
}

#[test]
fn authored_matrix_and_comparator_reject_wrong_geometry() -> R {
    let rows = cases()?;
    if rows.len() != 27 {
        return Err("signature denominator changed without contract update".into());
    }
    let mut failures = Vec::new();
    compare("geometry", &json!([3, 8]), &json!([3, 9]), &mut failures);
    compare("disposition", &json!(true), &json!(false), &mut failures);
    compare("kind", &json!("Prototype"), &json!("Signature"), &mut failures);
    if failures.len() != 3 {
        return Err("comparator accepted deliberately wrong evidence".into());
    }
    for row in rows {
        let source = row["source"].as_str().ok_or("missing source")?;
        let endpoints = row["native"]["header_span"].as_array().ok_or("missing span")?;
        let start = endpoints.first().and_then(Value::as_u64).ok_or("missing start")? as usize;
        let end = endpoints.get(1).and_then(Value::as_u64).ok_or("missing end")? as usize;
        if !source.get(start..end).is_some_and(|s| s.starts_with('(') && s.ends_with(')')) {
            return Err("invalid authored header geometry".into());
        }
    }
    Ok(())
}

#[test]
#[ignore = "explicit conformance report; known production mismatches are not a pass"]
fn native_signature_conformance_report() -> R {
    let rows: Vec<_> = cases()?.iter().map(observe).collect::<Result<_, _>>()?;
    let mismatches =
        rows.iter().filter(|row| row["failures"].as_array().is_some_and(|v| !v.is_empty())).count();
    let unproven = rows.iter().any(|row| row["status"] == "NOT_PROVEN");
    let report = json!({"schema": "signature-native-report/v1", "matrix": serde_json::from_str::<Value>(MATRIX)?, "status": if unproven { "NOT_PROVEN" } else if mismatches == 0 { "PASS" } else { "CONFORMANCE_MISMATCH" }, "mismatch_count": mismatches, "rows": rows});
    let path = std::env::var_os("SIGNATURE_NATIVE_REPORT")
        .map(PathBuf::from)
        .ok_or("SIGNATURE_NATIVE_REPORT must name the retained report")?;
    let path = if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    if unproven {
        return Err("NOT_PROVEN: terminal parse; report retained".into());
    }
    if mismatches != 0 {
        return Err(format!("CONFORMANCE_MISMATCH: {mismatches}; report retained").into());
    }
    Ok(())
}

#[test]
fn native_observer_rejects_corrupted_variable_geometry() -> R {
    let mut row = cases()?
        .into_iter()
        .find(|row| row["id"] == "positional_slurpy_array")
        .ok_or("missing control")?;
    let control = observe(&row)?;
    if control["failures"].as_array().is_none_or(|items| !items.is_empty()) {
        return Err("known conforming native control failed".into());
    }
    row["native"]["parameters"][0]["variable"]["span"][0] = json!(999);
    let corrupted = observe(&row)?;
    if !corrupted["failures"].as_array().is_some_and(|items| {
        items.iter().any(|item| item.as_str().is_some_and(|text| text.contains("variable span")))
    }) {
        return Err("native observer accepted corrupted variable geometry".into());
    }
    Ok(())
}

#[test]
fn native_disposition_respects_advisory_error_nodes_and_terminal_stop() -> R {
    let mut output = Parser::new("sub f($x) { $x }").parse_with_recovery();
    output.diagnostics.push(ParseError::nested_quantifier_advisory(0));
    if disposition(&output) != Some(true) {
        return Err("advisory rejected clean AST".into());
    }
    output.diagnostics.clear();
    output.ast.kind = NodeKind::Error {
        message: "synthetic error node".into(),
        expected: vec![],
        found: None,
        partial: None,
    };
    if disposition(&output) != Some(false) {
        return Err("error node accepted without diagnostics".into());
    }
    let row = cases()?
        .into_iter()
        .find(|row| row["id"] == "named_array")
        .ok_or("missing negative control")?;
    output.set_stop_cause(Some(ParseStopCause::Cancelled));
    let observation = observe_output(&row, &output)?;
    if observation["status"] != "NOT_PROVEN"
        || observation["observed"]["accepted"] != Value::Null
        || observation["observed"]["stop_cause"] != "Cancelled"
    {
        return Err("terminal output counted as ordinary rejection".into());
    }
    Ok(())
}

#[test]
fn native_observer_rejects_named_metadata_and_unexpected_defaults() -> R {
    let row = cases()?
        .into_iter()
        .find(|row| row["id"] == "named_slurpy_array")
        .ok_or("missing named control")?;
    let source = row["source"].as_str().ok_or("missing source")?;
    let baseline = Parser::new(source).parse_with_recovery();
    if observe_output(&row, &baseline)?["failures"].as_array().is_none_or(|v| !v.is_empty()) {
        return Err("named baseline does not conform".into());
    }
    for mutation in 0..4 {
        let mut output = Parser::new(source).parse_with_recovery();
        let NodeKind::Program { statements } = &mut output.ast.kind else {
            return Err("missing program".into());
        };
        let callable = statements.iter_mut().find(|node| matches!(&node.kind, NodeKind::Subroutine { name: Some(name), .. } if name == "f")).ok_or("missing callable")?;
        let NodeKind::Subroutine { signature: Some(signature), .. } = &mut callable.kind else {
            return Err("missing signature".into());
        };
        let NodeKind::Signature { parameters } = &mut signature.kind else {
            return Err("wrong header".into());
        };
        let parameter = parameters.first_mut().ok_or("missing parameter")?;
        let NodeKind::NamedParameter {
            variable,
            required,
            external_name,
            default_value,
            default_operator,
            ..
        } = &mut parameter.kind
        else {
            return Err("missing named parameter".into());
        };
        let expected_failure = match mutation {
            0 => {
                *required = false;
                "required"
            }
            1 => {
                *external_name = "wrong".into();
                "external name"
            }
            2 => {
                *default_value = Some(variable.clone());
                "unexpected default"
            }
            _ => {
                *default_operator = Some("=".into());
                "unexpected default operator"
            }
        };
        let observed = observe_output(&row, &output)?;
        if !observed["failures"].as_array().is_some_and(|v| {
            v.iter().any(|f| f.as_str().is_some_and(|s| s.contains(expected_failure)))
        }) {
            return Err(format!("accepted named AST mutation {mutation}").into());
        }
    }
    Ok(())
}
