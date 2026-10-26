//! DATA/END acceptance through the negotiated LSP consumer, including range overlap.
//!
//! The repository's LSP_FEATURES_OVERVIEW.md promises full/range support per
//! LSP 3.16. The partial-overlap rule is retained in the LSP 3.17 contract:
//! <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#semanticTokens_rangeRequest>.

use perl_lsp::{JsonRpcRequest, LspServer, protocol::JsonRpcId};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type Token = (u32, u32, u32, String, u32);
const URI: &str = "file:///data-section-range.pl";

fn request(server: &LspServer, method: &str, params: Value) -> TestResult<Value> {
    let response = server
        .handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(JsonRpcId::Integer(1)),
            method: method.into(),
            params: Some(params),
        })
        .ok_or("request returned no response")?;
    if let Some(error) = response.error {
        return Err(format!("{method} failed: {error:?}").into());
    }
    response.result.ok_or_else(|| "response had no result".into())
}

fn notify(server: &LspServer, method: &str, params: Value) {
    server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: method.into(),
        params: Some(params),
    });
}

fn open(source: &str) -> TestResult<(LspServer, Vec<String>)> {
    let server = LspServer::new();
    let initialized = request(
        &server,
        "initialize",
        json!({"capabilities": {
            "general": {"positionEncodings": ["utf-8", "utf-16"]},
            "textDocument": {"semanticTokens": {
                "requests": {"full": true, "range": true},
                "tokenTypes": ["comment", "string", "variable", "number"],
                "tokenModifiers": [], "formats": ["relative"],
                "multilineTokenSupport": false, "overlappingTokenSupport": false
            }}
        }}),
    )?;
    assert_eq!(initialized["capabilities"]["positionEncoding"], "utf-16");
    let legend = initialized["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
        .as_array()
        .ok_or("missing advertised token legend")?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or("invalid legend entry".into()))
        .collect::<TestResult<Vec<_>>>()?;
    notify(&server, "initialized", json!({}));
    notify(
        &server,
        "textDocument/didOpen",
        json!({"textDocument": {"uri": URI, "languageId": "perl", "version": 1, "text": source}}),
    );
    Ok((server, legend))
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

fn range(server: &LspServer, start: (u32, u32), end: (u32, u32)) -> TestResult<Value> {
    request(
        server,
        "textDocument/semanticTokens/range",
        json!({"textDocument": {"uri": URI}, "range": {
            "start": {"line": start.0, "character": start.1},
            "end": {"line": end.0, "character": end.1}
        }}),
    )
}

#[test]
fn data_sections_keep_whole_utf16_tokens_when_range_starts_inside() -> TestResult {
    for marker in ["__DATA__", "__END__"] {
        let source = format!("my $before = 1;\r\n{marker}\r\n😀tail\r\n\r\nmy $x = 1;");
        let (server, legend) = open(&source)?;
        let full = request(
            &server,
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": URI}}),
        )?;
        let full = decode(&full, &legend)?;
        let comment = |line, length| (line, 0, length, "comment".to_owned(), 0);
        let expected = vec![comment(1, marker.len() as u32), comment(2, 6), comment(4, 10)];
        assert_eq!(full.iter().filter(|token| token.0 >= 1).cloned().collect::<Vec<_>>(), expected);
        assert!(full.iter().any(|token| token.0 == 0 && token.3 == "variable"));

        // LSP 3.17 semanticTokens/range includes tokens partially overlapping
        // either boundary. Preserve canonical geometry instead of clipping it.
        assert_eq!(decode(&range(&server, (1, 2), (1, 4))?, &legend)?, vec![expected[0].clone()]);
        // Column 2 is immediately after the astral character in negotiated UTF-16.
        assert_eq!(decode(&range(&server, (2, 2), (2, 3))?, &legend)?, vec![expected[1].clone()]);
        assert_eq!(decode(&range(&server, (2, 0), (2, 1))?, &legend)?, vec![expected[1].clone()]);
        let packed = range(&server, (2, 2), (4, 3))?;
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
            let (server, legend) = open(&source)?;
            let full = request(
                &server,
                "textDocument/semanticTokens/full",
                json!({"textDocument": {"uri": URI}}),
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
                decode(&range(&server, (1, 2), (1, 4))?, &legend)?,
                vec![(1, 0, marker.len() as u32, "string".to_owned(), 0)]
            );
        }
    }
    Ok(())
}

#[test]
fn data_section_range_boundaries_do_not_invent_overlap() -> TestResult {
    let (server, legend) = open("__DATA__\n😀tail\n")?;
    assert_eq!(
        decode(&range(&server, (1, 0), (1, 6))?, &legend)?,
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
        assert!(decode(&range(&server, start, end)?, &legend)?.is_empty(), "{start:?}..{end:?}");
    }
    Ok(())
}
