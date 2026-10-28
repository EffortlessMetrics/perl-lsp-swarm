//! Protocol regression for plaintext hover rendering.
mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;

#[test]
fn hover_preserves_variable_identifier_in_both_formats() -> Result<(), Box<dyn std::error::Error>> {
    for format in ["plaintext", "markdown"] {
        let mut harness = LspHarness::new();
        harness
            .initialize(Some(json!({"textDocument": {"hover": {"contentFormat": [format]}}})))?;
        let uri = "file:///hover-plaintext-identifier.pl";
        harness.open_document(uri, "my $my_variable = 42;\nprint $my_variable;\n")?;
        let hover = harness.request_with_timeout(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": {"line": 1, "character": 9}}),
            Duration::from_secs(10),
        )?;
        assert_eq!(hover["contents"]["kind"], format, "{hover}");
        let text = hover["contents"]["value"].as_str().ok_or("missing hover text")?;
        assert!(text.contains("$my_variable"), "{text}");
        assert!(!text.contains("$myvariable"), "{text}");
    }
    Ok(())
}
