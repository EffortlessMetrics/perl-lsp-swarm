//! DATA/END acceptance through full, range and delta semantic-token requests.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type Token = (u32, u32, u32, String, u32);

/// Initialize against only this fixture, preserving the workspace until the
/// harness has shut down. Explicitly exercise the current UTF-16 negotiation.
fn open(source: &str) -> TestResult<(TempWorkspace, LspHarness, Vec<String>, String)> {
    let workspace = TempWorkspace::new()?;
    workspace.write("tokens.pl", source)?;
    let uri = workspace.uri("tokens.pl");
    let mut harness = LspHarness::new_raw();
    let initialized = harness.initialize_ready(
        &workspace.root_uri,
        Some(json!({
            "general": {"positionEncodings": ["utf-8", "utf-16"]},
            "textDocument": {"semanticTokens": {
                "requests": {"full": {"delta": true}, "range": true},
                "tokenTypes": ["comment", "string", "variable", "number"],
                "tokenModifiers": [], "formats": ["relative"],
                "multilineTokenSupport": false, "overlappingTokenSupport": false
            }}
        })),
    )?;
    assert_eq!(initialized["capabilities"]["positionEncoding"], "utf-16");
    let legend = initialized["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
        .as_array()
        .ok_or("missing advertised token legend")?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or("invalid legend entry".into()))
        .collect::<TestResult<Vec<_>>>()?;
    harness.open(&uri, source)?;
    Ok((workspace, harness, legend, uri))
}

fn decode(result: &Value, legend: &[String]) -> TestResult<Vec<Token>> {
    let data = result["data"].as_array().ok_or("missing semantic-token data")?;
    let chunks = data.chunks_exact(5);
    assert!(chunks.remainder().is_empty(), "semantic tokens must be five-tuples");
    let (mut line, mut col) = (0u32, 0u32);
    let mut tokens = Vec::new();
    for chunk in chunks {
        let values = chunk
            .iter()
            .map(|value| Ok(u32::try_from(value.as_u64().ok_or("invalid token integer")?)?))
            .collect::<TestResult<Vec<_>>>()?;
        line += values[0];
        col = if values[0] == 0 { col + values[1] } else { values[1] };
        let kind = legend.get(values[3] as usize).ok_or("token type outside advertised legend")?;
        tokens.push((line, col, values[2], kind.clone(), values[4]));
    }
    Ok(tokens)
}

fn range(
    harness: &mut LspHarness,
    uri: &str,
    start: (u32, u32),
    end: (u32, u32),
) -> TestResult<Value> {
    Ok(harness.request(
        "textDocument/semanticTokens/range",
        json!({"textDocument": {"uri": uri}, "range": {
            "start": {"line": start.0, "character": start.1},
            "end": {"line": end.0, "character": end.1}
        }}),
    )?)
}

#[test]
fn data_sections_keep_whole_utf16_tokens_when_range_starts_inside() -> TestResult {
    for marker in ["__DATA__", "__END__"] {
        let source = format!("my $before = 1;\r\n{marker}\r\n😀tail\r\n\r\nmy $x = 1;");
        let (_workspace, mut harness, legend, uri) = open(&source)?;
        let full = harness
            .request("textDocument/semanticTokens/full", json!({"textDocument": {"uri": uri}}))?;
        assert!(full["resultId"].as_str().is_some(), "full response needs a resultId");
        let full = decode(&full, &legend)?;
        let comment = |line, length| (line, 0, length, "comment".to_owned(), 0);
        let expected = vec![comment(1, marker.len() as u32), comment(2, 6), comment(4, 10)];
        assert_eq!(full.iter().filter(|token| token.0 >= 1).cloned().collect::<Vec<_>>(), expected);
        assert!(full.iter().any(|token| token.0 == 0 && token.3 == "variable"));

        // Retain canonical geometry on overlap at either boundary, including
        // a boundary inside an astral character's two UTF-16 code units.
        assert_eq!(
            decode(&range(&mut harness, &uri, (1, 2), (1, 4))?, &legend)?,
            vec![expected[0].clone()]
        );
        assert_eq!(
            decode(&range(&mut harness, &uri, (2, 2), (2, 3))?, &legend)?,
            vec![expected[1].clone()]
        );
        assert_eq!(
            decode(&range(&mut harness, &uri, (2, 0), (2, 1))?, &legend)?,
            vec![expected[1].clone()]
        );
        let packed = range(&mut harness, &uri, (2, 2), (4, 3))?;
        assert_eq!(decode(&packed, &legend)?, expected[1..]);
        let comment_kind =
            legend.iter().position(|kind| kind == "comment").ok_or("missing comment kind")?;
        assert_eq!(packed["data"], json!([2, 0, 6, comment_kind, 0, 2, 0, 10, comment_kind, 0]));
    }
    Ok(())
}

#[test]
fn marker_text_in_strings_and_heredocs_keeps_string_and_later_code() -> TestResult {
    for marker in ["__DATA__", "__END__"] {
        for source in [
            format!("my $s = '🙂\r\n{marker}\r\n';\r\nmy $after = 7;\r\n"),
            format!("my $s = <<'EOF';\r\n{marker}\r\nEOF\r\nmy $after = 7;\r\n"),
        ] {
            let (_workspace, mut harness, legend, uri) = open(&source)?;
            let full = harness.request(
                "textDocument/semanticTokens/full",
                json!({"textDocument": {"uri": uri}}),
            )?;
            let tokens = decode(&full, &legend)?;
            assert!(!tokens.iter().any(|token| token.3 == "comment"), "{source}");
            assert!(
                tokens.iter().any(|token| token.0 == 3
                    && token.1 == 3
                    && token.2 == 6
                    && token.3 == "variable"),
                "{tokens:?}"
            );
            assert!(tokens.iter().any(|token| token.0 == 3 && token.3 == "number"), "{tokens:?}");
            assert_eq!(
                decode(&range(&mut harness, &uri, (1, 2), (1, 4))?, &legend)?,
                vec![(1, 0, marker.len() as u32, "string".to_owned(), 0)]
            );
        }
    }
    Ok(())
}

#[test]
fn data_section_range_boundaries_do_not_invent_overlap() -> TestResult {
    let (_workspace, mut harness, legend, uri) = open("__DATA__\n😀tail\n")?;
    assert_eq!(
        decode(&range(&mut harness, &uri, (1, 0), (1, 6))?, &legend)?,
        vec![(1, 0, 6, "comment".to_owned(), 0)],
        "boundary controls require a real payload token"
    );
    for (start, end) in [
        ((1, 6), (1, 9)), // starts exactly after the payload
        ((0, 8), (1, 0)), // only the intervening newline
        ((1, 2), (1, 2)), // empty range inside a token
        ((1, 4), (1, 2)), // reversed same-line range
        ((1, 2), (0, 4)), // reversed multiline range
    ] {
        assert!(
            decode(&range(&mut harness, &uri, start, end)?, &legend)?.is_empty(),
            "{start:?}..{end:?}"
        );
    }
    Ok(())
}

#[test]
fn data_section_delta_reconstructs_the_fresh_full_stream() -> TestResult {
    for marker in ["__DATA__", "__END__"] {
        let source = format!("my $before = 1;\r\n{marker}\r\n😀tail\r\n");
        let (_workspace, mut harness, legend, uri) = open(&source)?;
        let full = harness
            .request("textDocument/semanticTokens/full", json!({"textDocument": {"uri": uri}}))?;
        let previous_id = full["resultId"].as_str().ok_or("missing full resultId")?.to_owned();
        let unchanged = harness.request(
            "textDocument/semanticTokens/full/delta",
            json!({"textDocument": {"uri": uri}, "previousResultId": previous_id}),
        )?;
        assert_eq!(unchanged["edits"], json!([]));
        harness.change_full(
            &uri,
            2,
            &format!("my $before = 1;\r\n{marker}\r\n😀longer\r\nextra"),
        )?;
        let delta = harness.request(
            "textDocument/semanticTokens/full/delta",
            json!({"textDocument": {"uri": uri}, "previousResultId": previous_id}),
        )?;
        let edits = delta["edits"].as_array().ok_or("changed delta missing edits")?;
        assert!(!edits.is_empty(), "changed data geometry must change the token stream");
        let mut reconstructed = full["data"].as_array().ok_or("missing old data")?.clone();
        // LSP edit offsets index the previous flat stream, so apply from the end.
        for edit in edits.iter().rev() {
            let start = usize::try_from(edit["start"].as_u64().ok_or("invalid edit start")?)?;
            let delete =
                usize::try_from(edit["deleteCount"].as_u64().ok_or("invalid delete count")?)?;
            let end = start.checked_add(delete).ok_or("edit bounds overflow")?;
            if end > reconstructed.len() {
                return Err("delta edit exceeds the previous stream".into());
            }
            let replacement = match edit.get("data") {
                Some(Value::Array(values)) => values.clone(),
                None => Vec::new(),
                _ => return Err("delta replacement is not an array".into()),
            };
            reconstructed.splice(start..end, replacement);
        }
        let fresh = harness
            .request("textDocument/semanticTokens/full", json!({"textDocument": {"uri": uri}}))?;
        assert_eq!(json!(reconstructed), fresh["data"]);
        assert_eq!(delta["resultId"], fresh["resultId"]);
        assert_eq!(
            decode(&fresh, &legend)?.into_iter().filter(|token| token.0 >= 1).collect::<Vec<_>>(),
            vec![
                (1, 0, marker.len() as u32, "comment".into(), 0),
                (2, 0, 8, "comment".into(), 0),
                (3, 0, 5, "comment".into(), 0),
            ]
        );
    }
    Ok(())
}
