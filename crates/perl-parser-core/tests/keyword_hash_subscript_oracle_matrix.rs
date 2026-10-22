//! Independently authored Perl 5.40.1 oracle slice for #14059 / #14056.
//! Perl semantics precede native observation; representation follows the public AST contract.
use perl_parser_core::{Node, NodeKind, Parser, RecoverySalvageClass, RecoverySalvageProfile};
use serde_json::{Value, json};
use std::collections::BTreeSet;

type R = Result<(), Box<dyn std::error::Error>>;
const MATRIX: &str = include_str!("../../../.ci/keyword-hash-subscript/cases.json");
fn matrix() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATRIX)
}
fn span(node: &Node) -> Value {
    json!([node.location.start, node.location.end])
}
fn shape(node: &Node) -> Value {
    let fields = match &node.kind {
        NodeKind::Binary { op, left, right } => {
            json!({"op":op,"left":shape(left),"right":shape(right)})
        }
        NodeKind::HashSlice { target, keys } => json!({"target":shape(target),"keys":shape(keys)}),
        NodeKind::Identifier { name } => json!({"name":name}),
        NodeKind::Variable { sigil, name } => json!({"sigil":sigil,"name":name}),
        NodeKind::String { value, interpolated } => {
            json!({"value":value,"interpolated":interpolated})
        }
        NodeKind::Number { value } => json!({"value":value}),
        NodeKind::ArrayLiteral { elements } => {
            json!({"elements":elements.iter().map(shape).collect::<Vec<_>>()})
        }
        NodeKind::LoopControl { op, label } => json!({"op":op,"label":label}),
        NodeKind::Return { value } => json!({"value":value.as_deref().map(shape)}),
        _ => json!({"children":node.children().into_iter().map(shape).collect::<Vec<_>>()}),
    };
    let mut value = fields;
    value["kind"] = json!(node.kind.kind_name());
    value["span"] = span(node);
    value
}
fn subjects<'a>(node: &'a Node, wanted: &Value, found: &mut Vec<&'a Node>) {
    // Statement wrappers share the expression span; the authored subject is the expression.
    if span(node) == *wanted && !matches!(&node.kind, NodeKind::ExpressionStatement { .. }) {
        found.push(node);
    }
    for child in node.children() {
        subjects(child, wanted, found);
    }
}
fn observe(row: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let source = row["source"].as_str().ok_or("missing source")?;
    let output = Parser::new(source).parse_with_recovery();
    let unproven = output.stop_cause().is_some();
    let recovered = !output.diagnostics.is_empty()
        || RecoverySalvageProfile::from_parse(&output.ast, &output.diagnostics, false).class
            != RecoverySalvageClass::Clean;
    let mut found = Vec::new();
    subjects(&output.ast, &row["subject_span"], &mut found);
    let actual =
        if found.len() == 1 { shape(found[0]) } else { json!({"subject_count":found.len()}) };
    Ok(
        json!({"id":row["id"],"source_sha256":row["source_sha256"],"profile":row["profile"],"disposition":if unproven {"NOT_PROVEN"} else if recovered {"recovered"} else {"clean"},"ast":actual,"diagnostics":format!("{:?}",output.diagnostics),"mismatch_owner":row["mismatch_owner"]}),
    )
}
fn agrees(row: &Value, observation: &Value) -> bool {
    match row["expected"]["disposition"].as_str() {
        Some("clean") => {
            observation["disposition"] == "clean" && observation["ast"] == row["expected"]["ast"]
        }
        // An unrelated recovery never proves rejection for the intended strict-subs reason.
        Some("rejected") => false,
        _ => false,
    }
}
#[test]
fn terminal_comma_slice_denominator_is_explicit_and_unique() -> R {
    let m = matrix()?;
    let rows = m["cases"].as_array().ok_or("missing cases")?;
    let ids: BTreeSet<_> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
    if rows.len() != 44 || ids.len() != rows.len() {
        return Err("missing/duplicate case identity".into());
    }
    let mut wanted = BTreeSet::new();
    for word in ["next", "last", "redo", "return"] {
        for form in ["scalar", "arrow", "slice"] {
            for context in ["terminal", "comma"] {
                let id = format!("{word}-{form}-{context}");
                let row = rows.iter().find(|row| row["id"] == id).ok_or("missing row")?;
                let base = match form {
                    "scalar" => "$h",
                    "arrow" => "$h->",
                    _ => "@h",
                };
                let key = if context == "comma" { format!("{word}, $x") } else { word.into() };
                let expression = format!("{base}{{{key}}}");
                let source = [
                    "use strict; use warnings; our (%h, $h, $x); sub probe { while (1) { ",
                    &expression,
                    "; } }\n",
                ]
                .concat();
                if row["source"] != source {
                    return Err("case identity/source substitution".into());
                }
                wanted.insert(id);
            }
        }
    }
    let declared: BTreeSet<_> = m["denominator"]["matrix_ids"]
        .as_array()
        .ok_or("missing denominator")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if declared != wanted || !wanted.iter().all(|id| ids.contains(id.as_str())) {
        return Err("terminal/comma/form denominator drift".into());
    }
    let controls: BTreeSet<_> = m["denominator"]["control_ids"]
        .as_array()
        .ok_or("missing controls")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if controls.len() != 20
        || declared.iter().any(|id| controls.contains(id.as_str()))
        || declared.len() + controls.len() != ids.len()
    {
        return Err("control denominator drift".into());
    }
    if m["profiles"][0]["version"] != "5.40.1"
        || m["profiles"][0]["other_versions"] != "NOT_PROVEN"
        || m["denominator"]["remaining"]["quote_like"].as_array().is_none_or(Vec::is_empty)
    {
        return Err("profile/remainder lost".into());
    }
    Ok(())
}
#[test]
fn exact_native_ast_and_ranges_for_conforming_rows() -> R {
    let mut checked = 0;
    for row in matrix()?["cases"].as_array().ok_or("missing cases")? {
        let known_mismatch = matches!(
            row["id"].as_str(),
            Some(
                "scalar-ordinary-comma"
                    | "arrow-ordinary-comma"
                    | "slice-ordinary-comma"
                    | "scalar-return-list-operands"
                    | "arrow-return-list-operands"
                    | "slice-return-list-operands"
            )
        );
        if known_mismatch {
            if row["mismatch_owner"] != 14065 {
                return Err("known mismatch owner lost".into());
            }
        } else {
            if !row["mismatch_owner"].is_null() {
                return Err("unexpected native row exclusion".into());
            }
            checked += 1;
            let observation = observe(row)?;
            if !agrees(row, &observation) {
                return Err(format!(
                    "{}: expected {}, observed {}",
                    row["id"], row["expected"], observation
                )
                .into());
            }
        }
    }
    if checked != 38 {
        return Err("native conforming denominator did not execute".into());
    }
    Ok(())
}
#[test]
fn false_static_control_order_form_and_equal_substring_ranges_are_rejected() -> R {
    let m = matrix()?;
    let rows = m["cases"].as_array().ok_or("missing cases")?;
    let row =
        rows.iter().find(|r| r["id"] == "next-scalar-comma").ok_or("missing comma control")?;
    let good = observe(row)?;
    if !agrees(row, &good) {
        return Err("native control did not execute cleanly".into());
    }
    for mutation in 0..7 {
        let mut bad = good.clone();
        match mutation {
            0 => {
                bad["ast"]["right"]["elements"][0] = json!({"kind":"Identifier","name":"next","span":row["expected"]["ast"]["right"]["elements"][0]["span"]});
            }
            1 => {
                bad["ast"]["right"]["elements"][0]["kind"] = json!("String");
            }
            2 => {
                bad["ast"]["right"]["elements"].as_array_mut().ok_or("missing list")?.swap(0, 1);
            }
            3 => {
                bad["ast"]["op"] = json!("->{}");
            }
            4 => {
                bad["ast"]["left"]["sigil"] = json!("@");
            }
            // `$x` occurs in the declaration before the real key expression. Equal text is insufficient.
            5 => {
                let source = row["source"].as_str().ok_or("missing source")?;
                let start = source.find("$x").ok_or("missing equal substring")?;
                bad["ast"]["right"]["elements"][1]["span"] = json!([start, start + 2]);
            }
            _ => {
                bad["disposition"] = json!("NOT_PROVEN");
            }
        }
        if agrees(row, &bad) {
            return Err(format!("accepted false native observation {mutation}").into());
        }
    }
    // Terminal proof cannot substitute for comma evidence, even with the same word/form.
    let terminal =
        rows.iter().find(|r| r["id"] == "next-scalar-terminal").ok_or("missing terminal")?;
    if agrees(row, &observe(terminal)?) {
        return Err("terminal evidence passed as comma evidence".into());
    }
    Ok(())
}
#[test]
#[ignore = "explicit #14059 conformance report: known #14065 mismatches must fail"]
fn native_keyword_hash_subscript_conformance_report() -> R {
    let m = matrix()?;
    let mut observations = Vec::new();
    let mut mismatches = 0;
    let mut unproven = false;
    for row in m["cases"].as_array().ok_or("missing cases")? {
        let mut observation = observe(row)?;
        let equal = agrees(row, &observation);
        unproven |= observation["disposition"] == "NOT_PROVEN"
            || (row["expected"]["disposition"] == "rejected"
                && observation["disposition"] == "recovered");
        observation["conforms"] = json!(equal);
        observation["expected"] = row["expected"].clone();
        mismatches += usize::from(!equal);
        observations.push(observation);
    }
    let path = std::env::var_os("KEYWORD_SUBSCRIPT_NATIVE_REPORT")
        .ok_or("KEYWORD_SUBSCRIPT_NATIVE_REPORT must name retained JSON")?;
    let report = json!({"schema":"keyword-hash-subscript-native/v1","status":if unproven {"NOT_PROVEN"} else if mismatches==0 {"NATIVE_MATCH"} else {"CONFORMANCE_MISMATCH"},"mismatch_count":mismatches,"rows":observations});
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    if unproven {
        return Err(
            "NOT_PROVEN: rejection/terminal instrument proof is incomplete; report retained".into(),
        );
    }
    if mismatches != 0 {
        return Err(format!(
            "CONFORMANCE_MISMATCH: {mismatches}; report retained; no production repair in #14059"
        )
        .into());
    }
    Ok(())
}
