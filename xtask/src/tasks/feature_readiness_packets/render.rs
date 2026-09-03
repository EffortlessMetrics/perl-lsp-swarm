//! Deterministic projections of feature-readiness packets.
//!
//! `machine` is canonical sorted-key JSON, `markdown` is phone-readable
//! builder/reviewer navigation, and `compact` is a dense model projection
//! that must retain every load-bearing constraint (checked by
//! `validate_compact_lossless`). Rendering never mutates repository state.

use serde_json::{Map, Value};

use super::validate::Violation;

/// Canonical machine bytes: pretty JSON over BTreeMap-ordered keys plus a
/// trailing newline. Two renders of the same document are byte-identical.
pub fn canonical_json(doc: &Value) -> String {
    let mut text = serde_json::to_string_pretty(doc).unwrap_or_else(|_| {
        // serde_json serialization of a document we built from JSON values
        // cannot fail; a non-object key would be the only route and none is
        // constructed. Fall back to an empty object rather than panicking;
        // validation will reject the resulting bytes.
        "{}".to_owned()
    });
    text.push('\n');
    text
}

/// Re-parse helper for round-trip determinism checks.
pub fn parse_json(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str(text)
}

fn strings(values: Option<&Value>) -> Vec<&str> {
    values
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn push_section(out: &mut String, title: &str) {
    out.push_str("\n## ");
    out.push_str(title);
    out.push('\n');
}

fn push_kv(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("- **{key}:** {value}\n"));
}

fn push_list(out: &mut String, items: &[&str]) {
    for item in items {
        out.push_str(&format!("  - {item}\n"));
    }
}

fn render_row_like(
    out: &mut String,
    label: &str,
    rows: &[&serde_json::Map<String, Value>],
    keys: &[&str],
) {
    if rows.is_empty() {
        return;
    }
    push_section(out, label);
    for row in rows {
        let cells: Vec<String> = keys
            .iter()
            .filter_map(|key| {
                row.get(*key).and_then(Value::as_str).map(|value| format!("{key}={value}"))
            })
            .collect();
        out.push_str(&format!("- {}\n", cells.join(" | ")));
    }
}

fn objects<'a>(doc: &'a Value, pointer: &str) -> Vec<&'a Map<String, Value>> {
    doc.pointer(pointer)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

/// Phone-readable Markdown projection of either packet kind.
pub fn markdown(doc: &Value) -> String {
    let is_builder =
        doc.get("schema").and_then(Value::as_str) == Some(super::build::BUILDER_SCHEMA);
    let id = doc
        .get(if is_builder { "packet_id" } else { "review_id" })
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut out = String::new();
    out.push_str(&format!(
        "# feature-readiness {} packet `{}`\n",
        if is_builder { "builder" } else { "reviewer" },
        id
    ));
    if is_builder {
        let work = doc.pointer("/work").and_then(Value::as_object);
        if let Some(work) = work {
            push_section(&mut out, "Work identity");
            for key in [
                "node_id",
                "issues",
                "controller_issue",
                "domain",
                "role",
                "disposition",
                "profile",
            ] {
                if let Some(value) = work.get(key) {
                    let rendered = value.to_string();
                    push_kv(&mut out, key, rendered.trim_matches('"'));
                }
            }
            if let Some(objective) = work.get("objective_sentence").and_then(Value::as_str) {
                push_section(&mut out, "One-PR objective");
                out.push_str(objective);
                out.push('\n');
            }
        }
        if let Some(claim) = doc.pointer("/claim_ceiling").and_then(Value::as_object) {
            push_section(&mut out, "Claim ceiling");
            push_kv(
                &mut out,
                "prerequisites",
                claim.get("prerequisite_disposition").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "rollback",
                claim.get("rollback_meaning").and_then(Value::as_str).unwrap_or(""),
            );
            out.push_str("- establishes:\n");
            push_list(&mut out, &strings(claim.get("establishes")));
            out.push_str("- cannot establish:\n");
            push_list(&mut out, &strings(claim.get("cannot_establish")));
            out.push_str("- remaining not-proven:\n");
            push_list(&mut out, &strings(claim.get("remaining_not_proven")));
        }
        render_row_like(
            &mut out,
            "Authority map",
            &objects(doc, "/authorities"),
            &["group", "ref", "subject"],
        );
        render_row_like(
            &mut out,
            "Feature/operation matrix",
            &objects(doc, "/operations"),
            &["feature", "provider_or_client", "canonical_owner", "old_path_disposition"],
        );
        if let Some(surfaces) = doc.pointer("/surfaces").and_then(Value::as_object) {
            push_section(&mut out, "Allowed / forbidden surfaces");
            out.push_str("- allowed:\n");
            push_list(&mut out, &strings(surfaces.get("allowed")));
            out.push_str("- forbidden:\n");
            push_list(&mut out, &strings(surfaces.get("forbidden")));
        }
        render_row_like(
            &mut out,
            "Artifact worklist",
            &objects(doc, "/artifacts"),
            &["id", "mode", "owner", "check_command", "claim_impact"],
        );
        if let Some(spec) = doc.pointer("/durable_spec").and_then(Value::as_object) {
            push_section(&mut out, "Durable-spec disposition");
            push_kv(
                &mut out,
                "disposition",
                spec.get("disposition").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(&mut out, "owner", spec.get("owner").and_then(Value::as_str).unwrap_or(""));
            push_kv(&mut out, "note", spec.get("note").and_then(Value::as_str).unwrap_or(""));
        }
        if let Some(sequence) = doc.get("sequence").and_then(Value::as_array) {
            push_section(&mut out, "Implementation sequence");
            for (index, step) in sequence.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", index + 1, step.as_str().unwrap_or("?")));
            }
        }
        if let Some(proof) = doc.pointer("/proof/first_falsifier").and_then(Value::as_object) {
            push_section(&mut out, "Shift-left proof");
            push_kv(
                &mut out,
                "first falsifier",
                proof.get("description").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "expected red reason",
                proof.get("expected_red_reason").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "positive discriminator",
                doc.pointer("/proof/positive_discriminator").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "instrument failure",
                doc.pointer("/proof/instrument_failure_behavior")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        }
        render_row_like(
            &mut out,
            "Proof commands",
            &objects(doc, "/proof/commands"),
            &["id", "command", "scope"],
        );
        if let Some(delivery) = doc.pointer("/delivery").and_then(Value::as_object) {
            push_section(&mut out, "Delivery");
            push_kv(
                &mut out,
                "branch",
                delivery.get("branch_suggestion").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "pr title",
                delivery.get("pr_title_suggestion").and_then(Value::as_str).unwrap_or(""),
            );
            push_kv(
                &mut out,
                "base/head",
                &delivery.get("base_head").map(|v| v.to_string()).unwrap_or_default(),
            );
        }
        if let Some(stop) = doc.pointer("/stop").and_then(Value::as_object) {
            push_section(&mut out, "Stop conditions");
            push_list(&mut out, &strings(stop.get("conditions")));
            out.push_str("- forbidden actions:\n");
            push_list(&mut out, &strings(stop.get("forbidden_actions")));
            push_kv(&mut out, "handoff", stop.get("handoff").and_then(Value::as_str).unwrap_or(""));
        }
        if let Some(live) = doc.pointer("/planes/live") {
            push_section(&mut out, "Live plane");
            push_kv(
                &mut out,
                "state",
                live.get("state").map(|v| v.to_string()).unwrap_or_default().trim_matches('"'),
            );
            push_kv(
                &mut out,
                "preflight required",
                &live.get("preflight_required").map(|v| v.to_string()).unwrap_or_default(),
            );
        }
    } else {
        if let Some(subject) = doc.pointer("/subject").and_then(Value::as_object) {
            push_section(&mut out, "Review subject");
            for key in ["node_id", "issues", "role", "profile"] {
                if let Some(value) = subject.get(key) {
                    let rendered = value.to_string();
                    push_kv(&mut out, key, rendered.trim_matches('"'));
                }
            }
        }
        if let Some(currentness) = doc.pointer("/currentness").and_then(Value::as_object) {
            push_section(&mut out, "Currentness");
            push_kv(
                &mut out,
                "base/head",
                &currentness.get("base_head").map(|v| v.to_string()).unwrap_or_default(),
            );
            push_kv(
                &mut out,
                "stale rule",
                currentness.get("stale_rule").and_then(Value::as_str).unwrap_or(""),
            );
        }
        push_section(&mut out, "Required review lenses");
        if let Some(lenses) = doc.get("lenses").and_then(Value::as_array) {
            for lens in lenses.iter().filter_map(Value::as_object) {
                let name = lens.get("name").and_then(Value::as_str).unwrap_or("?");
                let applicable = lens.get("applicable").and_then(Value::as_bool).unwrap_or(false);
                let reason = lens.get("reason").and_then(Value::as_str).unwrap_or("");
                if applicable {
                    out.push_str(&format!("- **{name}** — {reason}\n"));
                    push_list(&mut out, &strings(lens.get("questions")));
                } else {
                    out.push_str(&format!("- {name}: not applicable ({reason})\n"));
                }
            }
        }
        render_row_like(
            &mut out,
            "Stage falsification examples",
            &objects(doc, "/stage_falsification_examples"),
            &["stage", "question"],
        );
        render_row_like(
            &mut out,
            "Negative-control audit",
            &objects(doc, "/negative_control_audit"),
            &["subject", "requirement"],
        );
        render_row_like(
            &mut out,
            "Old-path audit",
            &objects(doc, "/old_path_audit"),
            &["seam", "terminal_disposition"],
        );
        if let Some(stop) = doc.pointer("/stop").and_then(Value::as_object) {
            push_section(&mut out, "Reviewer must not");
            push_list(&mut out, &strings(stop.get("reviewer_must_not")));
        }
    }
    out
}

/// Dense model projection retaining the load-bearing constraints.
pub fn compact(doc: &Value) -> String {
    let is_builder =
        doc.get("schema").and_then(Value::as_str) == Some(super::build::BUILDER_SCHEMA);
    let mut out = String::new();
    if is_builder {
        let get =
            |pointer: &str| doc.pointer(pointer).map(|value| value.to_string()).unwrap_or_default();
        let clean = |pointer: &str| get(pointer).trim_matches('"').to_owned();
        out.push_str(&format!(
            "PKT fr={} node={} role={} profile={} disposition={}\n",
            clean("/packet_id"),
            clean("/work/node_id"),
            clean("/work/role"),
            clean("/work/profile"),
            clean("/work/disposition"),
        ));
        out.push_str(&format!("CEILING: {}\n", clean("/work/objective_sentence")));
        out.push_str(&format!(
            "ESTABLISHES: {} \nCANNOT: {}\n",
            strings(doc.pointer("/claim_ceiling/establishes")).join("; "),
            strings(doc.pointer("/claim_ceiling/cannot_establish")).join("; "),
        ));
        out.push_str(&format!(
            "PREREQUISITES: disposition={} successors={} remaining_not_proven={} rollback={}\n",
            clean("/claim_ceiling/prerequisite_disposition"),
            compact_json(doc.pointer("/claim_ceiling/successors")),
            compact_json(doc.pointer("/claim_ceiling/remaining_not_proven")),
            clean("/claim_ceiling/rollback_meaning"),
        ));
        out.push_str("AUTHORITIES: ");
        let authorities = objects(doc, "/authorities");
        for (index, entry) in authorities.iter().enumerate() {
            if index > 0 {
                out.push_str("; ");
            }
            out.push_str(&format!(
                "{}[{}]={}",
                entry.get("group").and_then(Value::as_str).unwrap_or("?"),
                entry.get("ref").and_then(Value::as_str).unwrap_or("?"),
                entry.get("subject").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push('\n');
        out.push_str("OPERATIONS: ");
        out.push_str(&compact_json(doc.get("operations")));
        out.push('\n');
        out.push_str(&format!("SEQUENCE: {}\n", compact_json(doc.get("sequence"))));
        out.push_str("ALLOWED: ");
        out.push_str(&strings(doc.pointer("/surfaces/allowed")).join("; "));
        out.push_str("\nFORBIDDEN: ");
        out.push_str(&strings(doc.pointer("/surfaces/forbidden")).join("; "));
        out.push('\n');
        out.push_str("ARTIFACTS: ");
        for (index, artifact) in objects(doc, "/artifacts").iter().enumerate() {
            if index > 0 {
                out.push_str("; ");
            }
            // The whole artifact worklist is load-bearing (#11286): dropping
            // owner/proof/check/lens/impact cells would strip the consumer's
            // generator identity and claim consequences from the projection.
            out.push_str(&format!(
                "{}[{}]({}) owner={} now={} proof={} check={} lens={} impact={}",
                artifact.get("id").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("kind").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("mode").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("owner").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("current_disposition").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("required_change_or_proof").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("check_command").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("review_lens").and_then(Value::as_str).unwrap_or("?"),
                artifact.get("claim_impact").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push('\n');
        out.push_str(&format!(
            "FALSIFIER: {} => RED:{}\n",
            clean("/proof/first_falsifier/description"),
            clean("/proof/first_falsifier/expected_red_reason"),
        ));
        out.push_str(&format!(
            "PROOF: positive={} controls={} instrument=not_proven\n",
            clean("/proof/positive_discriminator"),
            objects(doc, "/proof/controls").len(),
        ));
        for command in objects(doc, "/proof/commands") {
            out.push_str(&format!(
                "CMD {} [{}]: {}\n",
                command.get("id").and_then(Value::as_str).unwrap_or("?"),
                command.get("scope").and_then(Value::as_str).unwrap_or("?"),
                command.get("command").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        for control in objects(doc, "/proof/controls") {
            out.push_str(&format!(
                "CONTROL {}: {}\n",
                control.get("class").and_then(Value::as_str).unwrap_or("?"),
                control.get("subject").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push_str("OLDPATH: ");
        for (index, row) in objects(doc, "/delivery/old_path_dispositions").iter().enumerate() {
            if index > 0 {
                out.push_str("; ");
            }
            out.push_str(&format!(
                "{}=>{}",
                row.get("seam").and_then(Value::as_str).unwrap_or("?"),
                row.get("terminal_disposition").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push('\n');
        out.push_str(&format!(
            "ROUTING: {}\nDURABLE_SPEC: {}\n",
            compact_json(doc.pointer("/delivery/issues")),
            compact_json(doc.get("durable_spec")),
        ));
        out.push_str(&format!(
            "DELIVERY: surfaces={} limitations={} review_map={} stop_before={}\n",
            compact_json(doc.pointer("/delivery/changed_surfaces")),
            compact_json(doc.pointer("/delivery/limitations")),
            compact_json(doc.pointer("/delivery/review_map")),
            compact_json(doc.pointer("/delivery/stop_before")),
        ));
        out.push_str(&format!("LIVE: {}\n", compact_json(doc.pointer("/planes/live")),));
        out.push_str("STOP: ");
        out.push_str(&strings(doc.pointer("/stop/conditions")).join(" / "));
        out.push_str("\nNEVER: ");
        out.push_str(&strings(doc.pointer("/stop/forbidden_actions")).join(", "));
        out.push_str("\nHANDOFF: ");
        out.push_str(&clean("/stop/handoff"));
        out.push('\n');
    } else {
        let clean = |pointer: &str| {
            doc.pointer(pointer)
                .map(|value| value.to_string())
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned()
        };
        out.push_str(&format!(
            "REVIEW rv={} node={} role={} profile={} issues={} builder={} digest={}\n",
            clean("/review_id"),
            clean("/subject/node_id"),
            clean("/subject/role"),
            clean("/subject/profile"),
            compact_json(doc.pointer("/subject/issues")),
            clean("/builder_ref/packet_id"),
            clean("/builder_ref/digest"),
        ));
        out.push_str(&format!("CEILING: {}\n", clean("/subject/claim_ceiling_sentence")));
        out.push_str(&format!(
            "CURRENTNESS: base={} state={} invalidators={} stale_rule={}\n",
            clean("/currentness/base_head"),
            clean("/currentness/live_state"),
            compact_json(doc.pointer("/currentness/invalidators")),
            clean("/currentness/stale_rule"),
        ));
        out.push_str("LENSES: ");
        for (index, lens) in objects(doc, "/lenses").iter().enumerate() {
            let applicable = lens.get("applicable").and_then(Value::as_bool).unwrap_or(false);
            if index > 0 {
                out.push_str("; ");
            }
            out.push_str(lens.get("name").and_then(Value::as_str).unwrap_or("?"));
            out.push_str(&format!(
                " applicable={} reason={}",
                applicable,
                lens.get("reason").and_then(Value::as_str).unwrap_or("?")
            ));
            for question in strings(lens.get("questions")) {
                out.push_str(&format!("\n  Q: {question}"));
            }
        }
        out.push('\n');
        for example in objects(doc, "/stage_falsification_examples") {
            out.push_str(&format!(
                "FALSIFY[{}]: {}\n",
                example.get("stage").and_then(Value::as_str).unwrap_or("?"),
                example.get("question").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        for row in objects(doc, "/negative_control_audit") {
            out.push_str(&format!(
                "AUDIT {}: {}\n",
                row.get("subject").and_then(Value::as_str).unwrap_or("?"),
                row.get("requirement").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push_str("OLDPATH: ");
        for (index, row) in objects(doc, "/old_path_audit").iter().enumerate() {
            if index > 0 {
                out.push_str("; ");
            }
            out.push_str(&format!(
                "{}=>{}",
                row.get("seam").and_then(Value::as_str).unwrap_or("?"),
                row.get("terminal_disposition").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        out.push_str("\nMUST-NOT: ");
        out.push_str(&strings(doc.pointer("/stop/reviewer_must_not")).join(" / "));
        out.push('\n');
    }
    out
}

/// Prove the compact projection retained the load-bearing constraints: node,
/// action, subject, ceiling, authority boundaries, artifacts, first
/// falsifier, old-path dispositions, and stop conditions.
pub fn validate_compact_lossless(builder: &Value, compact_text: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let node_id = builder.pointer("/work/node_id").and_then(Value::as_str).unwrap_or("");
    let role = builder.pointer("/work/role").and_then(Value::as_str).unwrap_or("");
    for token in [
        node_id.to_owned(),
        role.to_owned(),
        builder.pointer("/packet_id").and_then(Value::as_str).unwrap_or("").to_owned(),
        builder
            .pointer("/proof/first_falsifier/description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(24)
            .collect(),
    ] {
        if token.is_empty() || !compact_text.contains(&token) {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped load-bearing constraint {token:?}"),
            ));
        }
    }
    let artifact_section = compact_text
        .split_once("ARTIFACTS: ")
        .and_then(|(_, rest)| rest.split_once('\n').map(|(section, _)| section))
        .unwrap_or("");
    let artifact_ids: Vec<&str> = objects(builder, "/artifacts")
        .iter()
        .filter_map(|artifact| artifact.get("id").and_then(Value::as_str))
        .collect();
    for artifact in objects(builder, "/artifacts") {
        let id = artifact.get("id").and_then(Value::as_str).unwrap_or("");
        let row_start = artifact_section.find(&format!("{id}["));
        let row = row_start.map(|start| {
            let end = artifact_ids
                .iter()
                .filter(|other| **other != id)
                .filter_map(|other| artifact_section[start..].find(&format!("{other}[")))
                .map(|offset| start + offset)
                .min()
                .unwrap_or(artifact_section.len());
            &artifact_section[start..end]
        });
        if id.is_empty() || row.is_none() {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped artifact {id:?}"),
            ));
        }
        for field in [
            "kind",
            "mode",
            "owner",
            "current_disposition",
            "required_change_or_proof",
            "check_command",
            "review_lens",
            "claim_impact",
        ] {
            let value = artifact.get(field).and_then(Value::as_str).unwrap_or("");
            if !value.is_empty() && !row.is_some_and(|text| text.contains(value)) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!(
                        "compact projection dropped artifact {id:?} cell {field:?} ({value:?})"
                    ),
                ));
            }
        }
    }
    for condition in strings(builder.pointer("/stop/conditions")) {
        if !condition.is_empty() && !compact_text.contains(condition) {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped stop condition {condition:?}"),
            ));
        }
    }
    for (label, pointer) in [
        ("changed surface", "/delivery/changed_surfaces"),
        ("limitation", "/delivery/limitations"),
        ("review-map entry", "/delivery/review_map"),
        ("stop-before condition", "/delivery/stop_before"),
    ] {
        for value in strings(builder.pointer(pointer)) {
            if !value.is_empty() && !compact_text.contains(value) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact projection dropped {label} {value:?}"),
                ));
            }
        }
    }
    for (index, control) in objects(builder, "/proof/controls").iter().enumerate() {
        for field in ["class", "subject"] {
            let value = control.get(field).and_then(Value::as_str).unwrap_or("");
            if !value.is_empty() && !compact_text.contains(value) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact projection dropped proof control {index} {field} {value:?}"),
                ));
            }
        }
    }
    for (index, command) in objects(builder, "/proof/commands").iter().enumerate() {
        for field in ["id", "command", "scope"] {
            let value = command.get(field).and_then(Value::as_str).unwrap_or("");
            if !value.is_empty() && !compact_text.contains(value) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact projection dropped proof command {index} {field} {value:?}"),
                ));
            }
        }
    }
    for (label, pointer) in
        [("rollback", "/claim_ceiling/rollback_meaning"), ("handoff", "/stop/handoff")]
    {
        let value = builder.pointer(pointer).and_then(Value::as_str).unwrap_or("");
        if !value.is_empty() && !compact_text.contains(value) {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped {label} {value:?}"),
            ));
        }
    }
    for disposition in strings_of(builder.pointer("/surfaces/forbidden")) {
        if !disposition.is_empty() && !compact_text.contains(disposition) {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped forbidden surface {disposition:?}"),
            ));
        }
    }
    for (label, pointer) in [
        ("operations", "/operations"),
        ("prerequisite disposition", "/claim_ceiling/prerequisite_disposition"),
        ("successors", "/claim_ceiling/successors"),
        ("remaining not-proven", "/claim_ceiling/remaining_not_proven"),
        ("sequence", "/sequence"),
        ("routing", "/delivery/issues"),
        ("durable spec", "/durable_spec"),
        ("live observations", "/planes/live"),
    ] {
        let Some(value) = builder.pointer(pointer) else {
            continue;
        };
        let encoded =
            value.as_str().map(str::to_owned).unwrap_or_else(|| compact_json(Some(value)));
        if encoded.is_empty() || !compact_text.contains(&encoded) {
            violations.push(Violation::new(
                "compact_loss",
                format!("compact projection dropped {label}"),
            ));
        }
    }
    violations
}

/// Validate every load-bearing reviewer field in the compact projection.
pub fn validate_reviewer_compact_lossless(reviewer: &Value, compact_text: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (label, pointer) in [
        ("review id", "/review_id"),
        ("node", "/subject/node_id"),
        ("issues", "/subject/issues"),
        ("role", "/subject/role"),
        ("profile", "/subject/profile"),
        ("claim ceiling", "/subject/claim_ceiling_sentence"),
        ("builder id", "/builder_ref/packet_id"),
        ("builder digest", "/builder_ref/digest"),
        ("base head", "/currentness/base_head"),
        ("live state", "/currentness/live_state"),
        ("invalidators", "/currentness/invalidators"),
        ("stale rule", "/currentness/stale_rule"),
        ("must-not", "/stop/reviewer_must_not"),
    ] {
        let Some(value) = reviewer.pointer(pointer) else { continue };
        let present = compact_value_present(value, compact_text);
        if !present {
            violations
                .push(Violation::new("compact_loss", format!("compact reviewer dropped {label}")));
        }
    }
    for token in strings_of(reviewer.pointer("/stop/reviewer_must_not")) {
        if !compact_text.contains(token) {
            violations.push(Violation::new(
                "compact_loss",
                "compact reviewer dropped a must-not constraint",
            ));
        }
    }
    for (index, lens) in objects(reviewer, "/lenses").iter().enumerate() {
        for field in ["name", "reason"] {
            let value = lens.get(field).and_then(Value::as_str).unwrap_or("");
            if !value.is_empty() && !compact_text.contains(value) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact reviewer dropped lens {index} {field}"),
                ));
            }
        }
        let applicable = lens.get("applicable").and_then(Value::as_bool);
        if let Some(applicable) = applicable {
            let marker = format!("applicable={applicable}");
            if !compact_text.contains(&marker) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact reviewer dropped lens {index} applicability"),
                ));
            }
        }
        for (question_index, question) in strings_of(lens.get("questions")).iter().enumerate() {
            if !question.is_empty() && !compact_text.contains(question) {
                violations.push(Violation::new(
                    "compact_loss",
                    format!("compact reviewer dropped lens {index} question {question_index}"),
                ));
            }
        }
    }
    for (label, pointer, fields) in [
        ("stage falsification", "/stage_falsification_examples", &["stage", "question"] as &[&str]),
        ("negative-control audit", "/negative_control_audit", &["subject", "requirement"]),
        ("old-path audit", "/old_path_audit", &["seam", "terminal_disposition"]),
    ] {
        for (index, row) in objects(reviewer, pointer).iter().enumerate() {
            for field in fields {
                let value = row.get(*field).and_then(Value::as_str).unwrap_or("");
                if !value.is_empty() && !compact_text.contains(value) {
                    violations.push(Violation::new(
                        "compact_loss",
                        format!("compact reviewer dropped {label} {index} {field}"),
                    ));
                }
            }
        }
    }
    if let Some(issues) = reviewer.pointer("/subject/issues") {
        let encoded = compact_json(Some(issues));
        if encoded.is_empty() || !compact_text.contains(&encoded) {
            violations
                .push(Violation::new("compact_loss", "compact reviewer dropped subject issues"));
        }
    }
    violations
}

fn compact_json(value: Option<&Value>) -> String {
    value.map(|value| serde_json::to_string(value).unwrap_or_default()).unwrap_or_default()
}

fn compact_value_present(value: &Value, compact_text: &str) -> bool {
    if value.is_array() && strings_of(Some(value)).len() == value.as_array().map_or(0, Vec::len) {
        return strings_of(Some(value)).iter().all(|token| compact_text.contains(token));
    }
    let encoded = value.as_str().map(str::to_owned).unwrap_or_else(|| compact_json(Some(value)));
    !encoded.is_empty() && compact_text.contains(&encoded)
}

fn strings_of(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}
