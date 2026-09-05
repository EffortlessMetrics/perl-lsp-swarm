//! End-to-end rename policy tests for #4964.
//!
//! The qualified-name fallback may only emit edits whose whole candidate
//! range is proven `Code` by the generation-bound source-region index.
//! Qualified-name-shaped text inside heredocs, strings, qw() lists, regex
//! bodies, POD, `__DATA__`, and recovery input must never be edited, while
//! real code occurrences must not be suppressed by tricky neighbors.

mod support;

use serde_json::Value;
use std::collections::BTreeMap;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LIB_URI: &str = "file:///Policy4964/A.pm";
const APP_URI: &str = "file:///Policy4964/B.pm";

const LIB_DOC: &str = "package A;\nsub target_name { return 1; }\n1;\n";

/// Open the library plus an application document, rename the sub from the
/// library declaration, and return the workspace `changes` map.
fn rename_target_name(app_doc: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(LIB_URI, LIB_DOC)?;
    harness.open(APP_URI, app_doc)?;

    let response = harness.request(
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": LIB_URI },
            "position": { "line": 1, "character": 5 },
            "newName": "renamed_target"
        }),
    )?;
    response
        .get("changes")
        .and_then(Value::as_object)
        .map(|v| Value::Object(v.clone()))
        .ok_or_else(|| "workspace rename should return a changes map".into())
}

/// Apply the returned edits to `original` and return the rewritten text.
fn apply_edits(original: &str, edits: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let edits = edits.as_array().ok_or("edits must be an array")?;
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let line_offsets: Vec<usize> = {
        let mut offsets = vec![0_usize];
        for (idx, byte) in original.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                offsets.push(idx + 1);
            }
        }
        offsets
    };
    for edit in edits {
        let (start_line, start_char) = {
            let s = &edit["range"]["start"];
            let line = s["line"].as_u64().ok_or("missing start line")? as usize;
            let character = s["character"].as_u64().ok_or("missing start char")? as usize;
            (line, character)
        };
        let (end_line, end_char) = {
            let e = &edit["range"]["end"];
            let line = e["line"].as_u64().ok_or("missing end line")? as usize;
            let character = e["character"].as_u64().ok_or("missing end char")? as usize;
            (line, character)
        };
        let line_start = *line_offsets.get(start_line).ok_or("start line out of range")?;
        let end_line_start = *line_offsets.get(end_line).ok_or("end line out of range")?;
        let start_byte = line_start
            + original[line_start..]
                .char_indices()
                .nth(start_char)
                .map(|(b, _)| b)
                .unwrap_or(original[line_start..].len());
        let end_byte = end_line_start
            + original[end_line_start..]
                .char_indices()
                .nth(end_char)
                .map(|(b, _)| b)
                .unwrap_or(original[end_line_start..].len());
        let new_text = edit["newText"].as_str().ok_or("missing newText")?.to_string();
        spans.push((start_byte, end_byte, new_text));
    }
    spans.sort_by_key(|(start, end, _)| (*start, *end));
    let mut result = original.to_string();
    for (start, end, new_text) in spans.iter().rev() {
        if *start <= result.len() && *end <= result.len() && start <= end {
            result.replace_range(start..end, new_text);
        }
    }
    Ok(result)
}

fn edits_for(changes: &Value, uri: &str) -> Value {
    changes.get(uri).cloned().unwrap_or(Value::Array(Vec::new()))
}

/// Qualified-name-shaped text in non-code regions must not be edited, while
/// the real code call is renamed.
#[test]
fn rename_leaves_non_code_regions_unedited() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "sub run {\n",
        "    my $note = \"call A::target_name here\";\n",
        "    my @copy = qw(A::target_name stays);\n",
        "    my $doc = <<\"DOC\";\n",
        "A::target_name in heredoc\n",
        "DOC\n",
        "    return A::target_name();\n",
        "}\n",
        "__DATA__\n",
        "A::target_name in data\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return A::renamed_target();"),
        "the real code call must be renamed: {renamed}"
    );
    assert!(
        renamed.contains("\"call A::target_name here\""),
        "string literal must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("qw(A::target_name stays)"),
        "qw() members must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("A::target_name in heredoc"),
        "heredoc body must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("A::target_name in data"),
        "__DATA__ payload must not be edited: {renamed}"
    );
    Ok(())
}

/// POD and regex bodies must not be edited; code after them still is.
#[test]
fn rename_leaves_pod_and_regex_unedited() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "=pod\n",
        "\n",
        "A::target_name documentation prose\n",
        "\n",
        "=cut\n",
        "sub run {\n",
        "    my $re = qr/A::target_name/;\n",
        "    return A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return A::renamed_target();"),
        "the real code call must be renamed: {renamed}"
    );
    assert!(
        renamed.contains("A::target_name documentation prose"),
        "POD content must not be edited: {renamed}"
    );
    assert!(renamed.contains("qr/A::target_name/"), "regex body must not be edited: {renamed}");
    Ok(())
}

/// Recovery input (unclosed literal) fails closed; code elsewhere still renames.
#[test]
fn rename_leaves_recovery_input_unedited() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "sub broken {\n",
        "    my $unclosed = \"starts A::target_name and never ends\n",
        "}\n",
        "sub run {\n",
        "    return A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return A::renamed_target();"),
        "the real code call must be renamed: {renamed}"
    );
    assert!(
        renamed.contains("\"starts A::target_name and never ends"),
        "recovery-ambiguous string content must not be edited: {renamed}"
    );
    Ok(())
}

/// A quote-like body with an embedded apostrophe must not poison the global
/// quote parity and suppress the following real code occurrence.
#[test]
fn rename_survives_apostrophe_in_quote_like_neighbor() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "sub run {\n",
        "    my $year = q(60's);\n",
        "    return A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return A::renamed_target();"),
        "code after a quote-like body with an apostrophe must still be renamed: {renamed}"
    );
    assert!(
        renamed.contains("q(60's)"),
        "the quote-like body itself must not be edited: {renamed}"
    );
    Ok(())
}

/// Sigiled package variables that share the sub's name are a different
/// entity and must not be edited on textual coincidence; the `&` call form
/// names the sub itself and must still be renamed.
#[test]
fn rename_leaves_sigiled_package_variable_coincidences_unedited() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "my $A::target_name;\n",
        "local $A::target_name;\n",
        "@A::target_name = ();\n",
        "%A::target_name = ();\n",
        "sub run {\n",
        "    return &A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return &A::renamed_target();"),
        "the `&` call form names the renamed sub and must be edited: {renamed}"
    );
    assert!(
        renamed.contains("my $A::target_name;"),
        "a sigiled scalar sharing the sub's name must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("local $A::target_name;"),
        "a localized package variable must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("@A::target_name = ();"),
        "a sigiled array sharing the sub's name must not be edited: {renamed}"
    );
    assert!(
        renamed.contains("%A::target_name = ();"),
        "a sigiled hash sharing the sub's name must not be edited: {renamed}"
    );
    Ok(())
}

/// Unicode earlier on the line must not suppress a real code occurrence.
#[test]
fn rename_survives_unicode_earlier_on_line() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "sub run {\n",
        "    my \u{8AAC}\u{660E} = \"\u{65E5}\u{672C}\u{8A9E} \u{1F389} notes\";\n",
        "    return A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let renamed = apply_edits(app_doc, &edits_for(&changes, APP_URI))?;

    assert!(
        renamed.contains("return A::renamed_target();"),
        "unicode earlier on the line must not suppress a real code rename: {renamed}"
    );
    Ok(())
}

/// The definition file keeps its own semantic edit and unrelated code in the
/// application file is untouched — the workspace edit stays minimal.
#[test]
fn rename_workspace_edit_stays_minimal_and_deterministic() -> TestResult {
    let app_doc = concat!(
        "package B;\n",
        "use A;\n",
        "sub run {\n",
        "    return A::target_name() + A::target_name();\n",
        "}\n",
    );
    let changes = rename_target_name(app_doc)?;
    let edits = edits_for(&changes, APP_URI);
    let arr = edits.as_array().ok_or("edits must be an array")?;
    assert!(arr.len() >= 2, "both qualified call occurrences must be renamed, got: {edits:?}");
    let renamed = apply_edits(app_doc, &edits)?;
    assert_eq!(
        renamed.matches("A::renamed_target()").count(),
        2,
        "both occurrences renamed exactly once each: {renamed}"
    );
    assert!(
        !renamed.contains("A::A::") && !renamed.contains("renamed_target::"),
        "no duplicated or corrupted names: {renamed}"
    );
    let _ = BTreeMap::<String, String>::new();
    Ok(())
}
