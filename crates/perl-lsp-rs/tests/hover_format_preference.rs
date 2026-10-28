//! Hover format negotiation respects the client's ordered preference.

mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::lsp_harness::LspHarness;

#[test]
fn hover_selects_first_supported_format() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (json!(["plaintext", "markdown"]), "plaintext"),
        (json!(["markdown", "plaintext"]), "markdown"),
        (json!(["plaintext"]), "plaintext"),
        (json!(["markdown"]), "markdown"),
        (json!(["other", "plaintext", "markdown"]), "plaintext"),
        (json!(["other", "markdown", "plaintext"]), "markdown"),
        (json!(["other"]), "plaintext"),
        (json!([]), "plaintext"),
        (Value::Null, "markdown"),
    ];
    for (formats, expected) in cases {
        let mut harness = LspHarness::new();
        let capabilities = if formats.is_null() {
            json!({})
        } else {
            json!({ "textDocument": { "hover": { "contentFormat": formats } } })
        };
        harness.initialize(Some(capabilities))?;
        let uri = "file:///hover_format_preference.pl";
        harness.open_document(uri, "my $value = 42;\nprint $value;\n")?;
        let result = harness.request_with_timeout(
            "textDocument/hover",
            json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 8 } }),
            Duration::from_secs(15),
        )?;
        assert_eq!(result["contents"]["kind"], expected, "formats={formats}: {result}");
    }
    Ok(())
}
