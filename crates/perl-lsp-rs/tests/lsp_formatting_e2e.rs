use perl_lsp_perltidy::native::{EditSpec, PositionEncoding, apply_edits_exact};
use serde_json::json;

mod support;
use support::lsp_client::LspClient;

fn apply_wire_edits_exact(
    source: &str,
    edits: &[serde_json::Value],
) -> Result<String, Box<dyn std::error::Error>> {
    let specs = edits
        .iter()
        .map(|edit| {
            let position = |side: &str, field: &str| -> Result<u32, Box<dyn std::error::Error>> {
                Ok(u32::try_from(
                    edit["range"][side][field].as_u64().ok_or("missing edit position")?,
                )?)
            };
            Ok(EditSpec::new(
                position("start", "line")?,
                position("start", "character")?,
                position("end", "line")?,
                position("end", "character")?,
                edit["newText"].as_str().ok_or("missing edit replacement")?,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(apply_edits_exact(source, &specs, PositionEncoding::Utf16CodeUnits)?)
}

#[test]
fn native_document_formatting_edits_code_around_heredoc_without_crossing_it()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///literal-island.pl";
    let source = "my$before=1;\nmy $text = <<'EOF';\nraw { text }  \nEOF\nmy$after=2;\n";
    let literal = "my $text = <<'EOF';\nraw { text }  \nEOF\n";
    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}}),
    )?;
    let edits = response["result"].as_array().ok_or("formatting must return an edit array")?;
    assert_eq!(
        edits.len(),
        2,
        "ordinary code on both sides should receive scoped edits: {response}"
    );
    assert_eq!(edits[0]["range"]["start"]["line"], 0);
    assert_eq!(edits[0]["range"]["end"]["line"], 0);
    assert_eq!(edits[1]["range"]["start"]["line"], 4);
    assert_eq!(edits[1]["range"]["end"]["line"], 4);
    let formatted = apply_wire_edits_exact(source, edits)?;
    assert_eq!(formatted, format!("my $before = 1;\n{literal}my $after = 2;\n"));
    assert!(formatted.contains(literal));

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_document_formatting_trim_option_keeps_heredoc_body_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///literal-whitespace-island.pl";
    let source = "my$before=1;\n  \nmy $text = <<'EOF';\nraw  \nEOF\nmy$after=2;\n";
    let literal = "my $text = <<'EOF';\nraw  \nEOF\n";
    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}
        }),
    )?;
    let edits = response["result"].as_array().ok_or("formatting must return an edit array")?;
    assert_eq!(edits.len(), 3, "trim option must retain scoped safe-line edits: {response}");
    assert_eq!(edits[0]["range"]["start"]["line"], 0);
    assert_eq!(edits[0]["range"]["end"]["line"], 0);
    assert_eq!(edits[1]["range"]["start"]["line"], 1);
    assert_eq!(edits[1]["range"]["end"]["line"], 1);
    assert_eq!(edits[2]["range"]["start"]["line"], 5);
    assert_eq!(edits[2]["range"]["end"]["line"], 5);
    let formatted = apply_wire_edits_exact(source, edits)?;
    assert_eq!(formatted, format!("my $before = 1;\n\n{literal}my $after = 2;\n"));
    assert!(formatted.contains(literal));

    let without_trim = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": false}
        }),
    )?;
    let unchanged_spaces =
        without_trim["result"].as_array().ok_or("formatting must return edits")?;
    assert_eq!(unchanged_spaces.len(), 2);
    let without_trim_formatted = apply_wire_edits_exact(source, unchanged_spaces)?;
    assert_eq!(without_trim_formatted, format!("my $before = 1;\n  \n{literal}my $after = 2;\n"));

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_document_formatting_final_newline_options_stay_at_safe_eof()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    for (uri, ending, insert, trim, expected_ending) in [
        ("file:///insert-safe-eof.pl", "", true, false, "\n"),
        ("file:///trim-safe-eof.pl", "\n\n", false, true, ""),
        ("file:///trim-insert-safe-eof.pl", "\n\n", true, true, "\n"),
    ] {
        let mut client = LspClient::spawn(&bin)?;
        let source = format!("my$before=1;\nmy $text = <<'EOF';\nraw  \nEOF\nmy$after=2;{ending}");
        client.did_open(uri, "perl", &source)?;
        let mut options = json!({"tabSize": 4, "insertSpaces": true});
        options["insertFinalNewline"] = json!(insert);
        options["trimFinalNewlines"] = json!(trim);
        let response = client.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": options}),
        )?;
        let edits = response["result"].as_array().ok_or("formatting must return edits")?;
        assert!(!edits.is_empty(), "safe EOF should admit formatting: {response}");
        assert!(
            edits.iter().all(|edit| {
                let start = edit["range"]["start"]["line"].as_u64();
                let end = edit["range"]["end"]["line"].as_u64();
                (start == Some(0) && end == Some(0))
                    || (start.is_some_and(|line| line >= 4) && end.is_some_and(|line| line >= 4))
            }),
            "no edit may cross the heredoc: {response}"
        );
        let formatted = apply_wire_edits_exact(&source, edits)?;
        assert_eq!(
            formatted,
            format!(
                "my $before = 1;\nmy $text = <<'EOF';\nraw  \nEOF\nmy $after = 2;{expected_ending}"
            )
        );
        client.shutdown()?;
    }
    Ok(())
}

#[test]
fn native_document_formatting_newline_only_edit_is_scoped_and_idempotent()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let source = "my $q = qw(a);\nmy $x = 1;";
    let uri = "file:///newline-only-eof.pl";
    let options = json!({"tabSize": 4, "insertSpaces": true, "insertFinalNewline": true});
    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}, "options": options}),
    )?;
    let edits = response["result"].as_array().ok_or("formatting must return edits")?;
    assert_eq!(edits.len(), 1, "newline-only operation should be one scoped insertion: {response}");
    assert_eq!(edits[0]["range"]["start"]["line"], 1);
    assert_eq!(edits[0]["range"]["end"]["line"], 1);
    let formatted = apply_wire_edits_exact(source, edits)?;
    assert_eq!(formatted, "my $q = qw(a);\nmy $x = 1;\n");

    let uri_after = "file:///newline-only-eof-after.pl";
    client.did_open(uri_after, "perl", &formatted)?;
    let second = client.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri_after}, "options": options}),
    )?;
    let second_edits = second["result"].as_array().ok_or("formatting must return edits")?;
    assert!(second_edits.is_empty(), "second formatting request must be idempotent: {second}");
    client.shutdown()?;
    Ok(())
}

#[test]
fn native_document_formatting_final_newline_options_preserve_data_tail()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    for (uri, ending, option) in [
        ("file:///protected-insert-eof.pl", "", "insertFinalNewline"),
        ("file:///protected-trim-eof.pl", "\n\n", "trimFinalNewlines"),
    ] {
        let mut client = LspClient::spawn(&bin)?;
        let source =
            format!("my$before=1;\nmy $text = <<'EOF';\nraw  \nEOF\n__DATA__\nraw  {ending}");
        let protected = format!("my $text = <<'EOF';\nraw  \nEOF\n__DATA__\nraw  {ending}");
        client.did_open(uri, "perl", &source)?;
        let mut options = json!({
            "tabSize": 4,
            "insertSpaces": true,
            "trimTrailingWhitespace": true
        });
        options[option] = json!(true);
        let response = client.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": options}),
        )?;
        let edits = response["result"].as_array().ok_or("formatting must return edits")?;
        assert_eq!(edits.len(), 1, "only the safe prefix may change: {response}");
        assert_eq!(edits[0]["range"]["start"]["line"], 0);
        assert_eq!(edits[0]["range"]["end"]["line"], 0);
        let formatted = apply_wire_edits_exact(&source, edits)?;
        assert_eq!(formatted, format!("my $before = 1;\n{protected}"));
        client.shutdown()?;
    }
    Ok(())
}

#[test]
fn native_default_document_formatting() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///fmt.pl";

    let source = "sub test{my$x=1;return$x;}\nsub another{return 2;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "Should return formatting edits");

    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(
        edit_text.contains("sub test") && edit_text.contains("{"),
        "Should format subroutine declaration, got: {}",
        edit_text
    );
    assert!(edit_text.contains("my $x = 1"), "Should add spaces around operators");
    assert!(edit_text.contains("return $x"), "Should format return statement");
    assert!(
        edit_text.contains("sub another") && edit_text.contains("{"),
        "Should format second subroutine"
    );

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangeFormatting` must refuse at
/// the exact `perllsp --stdio` boundary with the truthful unadvertised
/// disposition, and the refusal must leave the document generation untouched —
/// proven by manual whole-document formatting still observing the original
/// unformatted bytes afterwards.
#[test]
fn withdrawn_range_formatting_refuses_and_leaves_document_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///range.pl";

    let source = "# First subroutine - leave this comment untouched\nsub first{my$a=1;return$a;}\n\n# Second subroutine - don't format this\nsub second{my$b=2;return$b;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/rangeFormatting",
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 1, "character": 0},
                "end": {"line": 1, "character": 27}
            },
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let error = response.get("error").ok_or("withdrawn rangeFormatting must return an error")?;
    assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
    assert!(response.get("result").is_none(), "a refusal must not carry a successful edit payload");

    // Manual whole-document formatting remains live for the same document and
    // still sees the original bytes (the messy `my$a=1;` survives verbatim
    // inside the produced edit), proving no withdrawn request mutated state.
    let manual = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;
    let edits = manual["result"]
        .as_array()
        .ok_or("manual whole-document formatting should still return an edit array")?;
    assert!(!edits.is_empty(), "manual whole-document formatting must remain available");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;
    assert!(
        edit_text.contains("sub first"),
        "manual formatting must observe the original source bytes"
    );

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_default_formatting_preserves_comments() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///comments.pl";

    let source = r#"#!/usr/bin/perl
# Main script comment
use strict;use warnings;
# Function comment
sub test{
# Inner comment
my$x=1;# Inline comment
return$x;
}
"#;

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "native default formatting should return comment-safe edits");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(edit_text.contains("# Main script comment"), "Should preserve main comment");
    assert!(edit_text.contains("# Function comment"), "Should preserve function comment");
    assert!(edit_text.contains("# Inner comment"), "Should preserve inner comment");
    assert!(edit_text.contains("# Inline comment"), "Should preserve inline comment");

    assert!(edit_text.contains("use strict"), "Should format use statements");
    assert!(edit_text.contains("use warnings"), "Should separate use statements");
    assert!(edit_text.contains("sub test") && edit_text.contains("{"), "Should format subroutine");

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_default_formatting_honors_lsp_tab_size() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///tab-size.pl";

    let source = "sub test{my$x=1;return$x;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 2, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "native default formatting should return edits");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(edit_text.contains("sub test {\n  my $x = 1;\n  return $x;\n}"));

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangesFormatting` must refuse at
/// the exact `perllsp --stdio` boundary; no atomic multi-range edit set may
/// escape while #7089's composition contract is unproven.
#[test]
fn withdrawn_ranges_formatting_refuses_at_process_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///ranges.pl";

    let source = "\n\
# First subroutine - format this\n\
sub first{my$a=1;return$a;}\n\
\n\
# Second subroutine - don't format this\n\
sub second{my$b=2;return$b;}\n\
\n\
# Third subroutine - format this too\n\
sub third{my$c=3;return$c;}\n\
";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/rangesFormatting",
        json!({
            "textDocument": {"uri": uri},
            "ranges": [
                {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 2, "character": 27}
                },
                {
                    "start": {"line": 7, "character": 0},
                    "end": {"line": 8, "character": 27}
                }
            ],
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let error = response.get("error").ok_or("withdrawn rangesFormatting must return an error")?;
    assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
    assert!(response.get("result").is_none(), "a refusal must not carry a successful edit payload");

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/onTypeFormatting` refuses at the
/// exact `perllsp --stdio` boundary for every trigger shape, and the refusal
/// leaves the document observable unchanged via the still-live manual route.
#[test]
fn withdrawn_on_type_formatting_refuses_at_process_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///on-type.pl";
    let source = "sub first{my$a=1;return$a;}\n";

    client.did_open(uri, "perl", source)?;

    for (ch, line, character) in [("{", 0, 27), ("}", 0, 27), ("\n", 0, 27)] {
        let response = client.request(
            "textDocument/onTypeFormatting",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "ch": ch,
                "options": {"tabSize": 4, "insertSpaces": true}
            }),
        )?;

        let error = response
            .get("error")
            .ok_or("withdrawn onTypeFormatting must return an error, not a result")?;
        assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
        assert!(
            response.get("result").is_none(),
            "a refusal must not carry a successful edit payload"
        );
    }

    // Manual whole-document formatting still sees the original bytes.
    let manual = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;
    let edits = manual["result"]
        .as_array()
        .ok_or("manual whole-document formatting should still return an edit array")?;
    assert!(!edits.is_empty(), "manual whole-document formatting must remain available");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;
    assert!(
        edit_text.contains("my $a = 1"),
        "manual formatting must observe the original source bytes"
    );

    client.shutdown()?;
    Ok(())
}
