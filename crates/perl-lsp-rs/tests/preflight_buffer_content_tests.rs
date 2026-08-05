//! Regression tests for issue #5256 follow-up: the preflight `validate_lsp_request`
//! wiring must not reject ordinary user buffer content that happens to contain
//! substrings from `SUSPICIOUS_PATTERNS` (`<script`, `javascript:`, `data:text/html`,
//! `<?php`, `<%`).
//!
//! `<%` is the Mason component-block sigil (see `mason_navigation_tests.rs`), so an
//! unqualified pattern scan over buffer text makes the server refuse to open every
//! Mason file. `<script` similarly appears legitimately inside Perl heredocs that
//! emit HTML. Buffer content is the user's own source, not an attacker-controlled
//! file read off disk — these checks must not apply to `textDocument/didOpen`.

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hover_value(result: &serde_json::Value) -> Option<String> {
    result.get("contents").and_then(|c| c.get("value")).and_then(|v| v.as_str()).map(String::from)
}

/// A Mason buffer beginning `<%method greet>` must open successfully via `didOpen`
/// and answer a subsequent hover request — proving the document was actually stored,
/// not silently dropped by preflight's `-32600` rejection path.
#[test]
fn mason_buffer_with_percent_sigil_opens_and_answers_hover() -> TestResult {
    const MASON: &str = "<%method greet>\n  Hello from greet\n</%method>\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///greet.mason", MASON)?;

    // Hover inside the body text; the important thing is that the server has a
    // document to respond about at all (an error/None response here would mean
    // preflight silently dropped the didOpen).
    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///greet.mason"},
                "position": {"line": 1, "character": 3}
            }),
        )
        .unwrap_or(json!(null));

    assert!(
        !result.is_null(),
        "hover on a Mason buffer returned null — didOpen was likely rejected by preflight"
    );
    Ok(())
}

/// A Perl buffer containing a `<script>` tag inside a heredoc (e.g. HTML-emitting
/// CGI code) must open successfully and answer a subsequent hover request.
#[test]
fn heredoc_with_script_tag_opens_and_answers_hover() -> TestResult {
    const PERL: &str = concat!(
        "my $greeting = 'hi';\n",
        "print <<\"HTML\";\n",
        "<script>alert('hi')</script>\n",
        "HTML\n",
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///heredoc_script.pl", PERL)?;

    let result = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///heredoc_script.pl"},
                "position": {"line": 0, "character": 4}
            }),
        )
        .unwrap_or(json!(null));

    let val = hover_value(&result);
    assert!(
        val.is_some() || result.get("contents").is_some(),
        "hover on a heredoc-with-<script> buffer returned nothing — didOpen was \
         likely rejected by preflight, got: {result:?}"
    );
    Ok(())
}
