//! Boundary and mutation-killing tests for perl-lsp-input-validation.
//!
//! Complements the inline happy-path tests in lib.rs by targeting:
//! - Exact boundary values (max_file_size_bytes from perl-lsp-limits, MAX_PATH_LENGTH, line length 100_000)
//! - Every disallowed condition in validate_file_content, the generic request
//!   admission layer, and the document-sync sink validators (#8895)
//! - The absence of any generic parameter content scanner
//! - sanitize_string: exactly which characters are kept vs removed
//! - validate_file_path: extension filtering

use perl_lsp_rs_core::runtime::input_validation::{
    sanitize_string, validate_buffer_line_lengths, validate_document_uri, validate_file_content,
    validate_request_admission,
};
use perl_lsp_rs_core::runtime::limits::max_file_size_bytes;
use std::path::Path;

// ---------------------------------------------------------------------------
// validate_file_content: size boundary
// ---------------------------------------------------------------------------

#[test]
fn validate_file_content_at_exactly_max_size_is_ok() -> anyhow::Result<()> {
    // max_file_size_bytes() bytes exactly must pass (> not >=).
    // Use many short lines to avoid triggering the per-line length check.
    let max = max_file_size_bytes();
    // Each "x\n" = 2 bytes; max / 2 lines avoids the 100_000 char line check
    let line = "x\n";
    let lines = max / line.len();
    let content = line.repeat(lines);
    assert!(content.len() <= max, "content must not exceed max in this test");
    validate_file_content(&content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_one_byte_over_max_errors() {
    let max = max_file_size_bytes();
    // Use short lines spread across many rows so only the total size limit is hit
    let line = "x\n"; // 2 bytes each
    let lines = (max / line.len()) + 1;
    let content = line.repeat(lines);
    // content.len() > max here
    assert!(content.len() > max);
    let result = validate_file_content(&content, Path::new("test.pl"));
    assert!(result.is_err(), "content over max_file_size_bytes must be rejected");
}

// ---------------------------------------------------------------------------
// validate_file_content: null byte detection
// ---------------------------------------------------------------------------

#[test]
fn validate_file_content_null_byte_at_start_errors() {
    let content = "\0hello";
    let result = validate_file_content(content, Path::new("test.pl"));
    assert!(result.is_err(), "null byte at start must be rejected");
}

#[test]
fn validate_file_content_null_byte_at_end_errors() {
    let content = "print 'hello';\0";
    let result = validate_file_content(content, Path::new("test.pl"));
    assert!(result.is_err(), "null byte at end must be rejected");
}

#[test]
fn validate_file_content_without_null_bytes_is_ok() -> anyhow::Result<()> {
    let content = "print 'hello';\n";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// validate_file_content: line length check (100_000 chars)
// ---------------------------------------------------------------------------

#[test]
fn validate_file_content_line_at_exactly_100000_is_ok() -> anyhow::Result<()> {
    // Exactly 100_000 chars on one line must pass (> not >=)
    let content = "x".repeat(100_000);
    validate_file_content(&content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_line_one_char_over_100000_errors() {
    let content = "x".repeat(100_001);
    let result = validate_file_content(&content, Path::new("test.pl"));
    assert!(result.is_err(), "line of 100001 chars must be rejected");
}

#[test]
fn validate_file_content_long_line_on_second_line_errors() {
    let line1 = "use strict;\n";
    let long_line = "x".repeat(100_001);
    let content = format!("{line1}{long_line}");
    let result = validate_file_content(&content, Path::new("test.pl"));
    assert!(result.is_err(), "long line on second line must be rejected");
}

// ---------------------------------------------------------------------------
// validate_file_content: content-pattern scanning was REMOVED (issue #5256
// follow-up). Since #8895 the function has no production caller at all: the
// `textDocument/didOpen`/`didChange`/`didSave` buffer boundary applies its
// own configured size and binary-content guards in the perl-lsp-rs sync
// sink. Buffer content is the user's own editor source, not an
// attacker-controlled file read off disk; scanning it for HTML/script-like
// substrings made the server refuse to open every Mason file (Mason
// component blocks open with `<%`) and any Perl heredoc that emits
// `<script>` HTML. These tests assert the opposite of the old behavior —
// the content is ACCEPTED — so a pattern scan cannot silently return.
// ---------------------------------------------------------------------------

#[test]
fn validate_file_content_accepts_script_tag() -> anyhow::Result<()> {
    let content = "# <script>alert(1)</script>";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_javascript_protocol() -> anyhow::Result<()> {
    let content = "# javascript:void(0)";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_data_uri() -> anyhow::Result<()> {
    let content = "# data:text/html,<h1>xss</h1>";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_php_tag() -> anyhow::Result<()> {
    let content = "<?php echo 'hello'; ?>";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_mason_component_block_sigil() -> anyhow::Result<()> {
    // `<%` is the Mason component-block sigil (see
    // `perl-lsp-rs/tests/mason_navigation_tests.rs`) — this is the exact
    // pattern that made the server refuse to open every Mason file.
    let content = "<%method greet>\n  Hello from greet\n</%method>\n";
    validate_file_content(content, Path::new("test.mason"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_uppercase_script_tag() -> anyhow::Result<()> {
    let content = "# <SCRIPT>alert(1)</SCRIPT>";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

#[test]
fn validate_file_content_accepts_uppercase_javascript_protocol() -> anyhow::Result<()> {
    let content = "# JAVASCRIPT:void(0)";
    validate_file_content(content, Path::new("test.pl"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// validate_request_admission: method-name and payload resource bounds
//
// Admission is protocol-generic only (issue #8895). Any JSON-RPC method string
// within the length bound is admissible; parameter content is never scanned.
// ---------------------------------------------------------------------------

#[test]
fn admission_known_methods_are_ok() -> anyhow::Result<()> {
    let params = serde_json::json!({});
    validate_request_admission("textDocument/didChange", &params)?;
    validate_request_admission("textDocument/didSave", &params)?;
    validate_request_admission("$/perl-lsp/clientResponse", &params)?;
    Ok(())
}

/// A valid custom extension method carrying punctuation outside the old
/// project-specific allowlist must pass admission (issue #8895 negative
/// control 2: rejecting such a method reintroduces the charset policy).
#[test]
fn validate_request_admission_accepts_punctuation_outside_old_allowlist() -> anyhow::Result<()> {
    let params = serde_json::json!({});
    for method in
        ["custom/fmt.v2:preview", "vendor/method+suffix", "text/(x) [y] {z}", "space separated"]
    {
        validate_request_admission(method, &params).map_err(|e| {
            anyhow::anyhow!("method `{method}` must pass structural admission: {e}")
        })?;
    }
    Ok(())
}

#[test]
fn admission_method_over_100_chars_errors() {
    let method = "a".repeat(101);
    let params = serde_json::json!({});
    let result = validate_request_admission(&method, &params);
    assert!(result.is_err(), "method name > 100 chars must be rejected");
}

#[test]
fn admission_method_exactly_100_chars_is_ok() -> anyhow::Result<()> {
    let method = "a".repeat(100);
    let params = serde_json::json!({});
    validate_request_admission(&method, &params)?;
    Ok(())
}

/// Negative control (issue #8895 #1): no generic `<script>`/`javascript:`
/// substring scan may exist over arbitrary params. Browser-dangerous strings
/// in inert param payloads must be accepted by admission; only a sink that
/// renders such content into an active surface may refuse it.
#[test]
fn arbitrary_params_with_browser_dangerous_substrings_are_accepted() -> anyhow::Result<()> {
    let cases = [
        ("someOther/Method", serde_json::json!({ "url": "javascript:void(0)" })),
        ("someOther/Method", serde_json::json!({ "text": "<script>alert(1)</script>" })),
        ("workspace/symbol", serde_json::json!({ "query": "<script>alert('xss')</script>" })),
    ];
    for (method, params) in cases {
        validate_request_admission(method, &params).map_err(|e| {
            anyhow::anyhow!("{method} params are inert data and must not be content-scanned: {e}")
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Document-sync URI policy (moved out of generic preflight by #8895)
// ---------------------------------------------------------------------------

#[test]
fn validate_document_uri_accepts_supported_schemes() -> anyhow::Result<()> {
    for uri in [
        "file:///test.pl",
        "untitled:Untitled-1",
        "opencode:/w/lib/My.pm",
        "vscode-notebook-cell://wsl%2bubuntu/home/u/nb.ipynb#C1",
    ] {
        validate_document_uri(uri)
            .map_err(|e| anyhow::anyhow!("{uri} must be accepted at the sync sink: {e}"))?;
    }
    Ok(())
}

#[test]
fn validate_buffer_line_lengths_boundary() {
    let max_line = "x".repeat(100_000);
    assert!(validate_buffer_line_lengths(&max_line).is_ok());
    assert!(
        validate_buffer_line_lengths(&"x".repeat(100_001)).is_err(),
        "a line over MAX_LINE_LENGTH must be rejected"
    );
}

#[test]
fn validate_document_uri_rejects_unresolvable_scheme() {
    for uri in
        ["ftp://example.com/script.pl", "https://example.com/test.pl", "http://example.com/t.pl"]
    {
        assert!(
            validate_document_uri(uri).is_err(),
            "non-resolvable URI scheme `{uri}` must be rejected at the sync sink"
        );
    }
}

#[test]
fn validate_document_uri_rejects_overlong_uri() {
    let uri = format!("file:///{}", "a".repeat(5000));
    assert!(validate_document_uri(&uri).is_err(), "overlong URI must be rejected");
}

// ---------------------------------------------------------------------------
// Resource bounds remain explicit after the layering (#8895 acceptance):
// the flat serialized-params ceiling binds non-text-sync methods, while the
// text-sync ceiling is deliberate headroom above the configured file limit —
// the *precise* file limit is sink authority (store-without-parse guard in
// `perl-lsp-rs` text_sync), not a structural rejection.
// ---------------------------------------------------------------------------

#[test]
fn non_text_sync_params_keep_the_flat_one_megabyte_bound() {
    // Negative control: oversized generic params are still rejected by the
    // explicit structural bound. The relaxed ceiling is scoped to text
    // synchronization; an arbitrary method must not gain it.
    let params = serde_json::json!({ "blob": "a".repeat(1_000_001) });
    assert!(
        validate_request_admission("custom/whatever", &params).is_err(),
        "non-text-sync methods must keep the flat MAX_PARAMS_SIZE bound"
    );
}

#[test]
fn text_sync_params_at_the_configured_file_limit_are_accepted() -> anyhow::Result<()> {
    // A document exactly at the configured file limit must survive structural
    // admission. Sized from the live limit rather than a literal so the test
    // tracks configuration instead of pinning today's default.
    let text = document_of_len(max_file_size_bytes());
    for method in ["textDocument/didOpen", "textDocument/didChange", "textDocument/didSave"] {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///big.pl", "text": text }
        });
        validate_request_admission(method, &params).map_err(|e| {
            anyhow::anyhow!("{method} rejected a document at the configured file limit: {e}")
        })?;
    }
    Ok(())
}

#[test]
fn text_sync_params_over_the_configured_file_limit_are_a_sink_authority_concern() {
    // Structural admission carries headroom above the configured limit so the
    // sync sink can apply its own configured guard; it must not silently
    // re-import the old preflight rejection here.
    let text = document_of_len(max_file_size_bytes() + 1);
    let params = serde_json::json!({
        "textDocument": { "uri": "file:///toobig.pl", "text": text }
    });
    assert!(
        validate_request_admission("textDocument/didOpen", &params).is_ok(),
        "admission must leave over-limit documents to the sync sink's configured guard"
    );
}

// ---------------------------------------------------------------------------
// Item-bearing methods carry server-authored source-derived content.
//
// Under #8895 there is no exemption list to maintain because there is no scan:
// these payloads must simply be admitted like any other well-formed params.
// ---------------------------------------------------------------------------

#[test]
fn item_bearing_methods_accept_source_derived_content() {
    // One payload shape per method, each carrying the substrings the removed
    // catch-all arm used to scan for, placed in the field that method uses.
    let cases: &[(&str, serde_json::Value)] = &[
        (
            "textDocument/codeAction",
            serde_json::json!({
                "context": {"diagnostics": [{"message": "near print '<script>x</script>'"}]}
            }),
        ),
        (
            "codeAction/resolve",
            serde_json::json!({
                "title": "Fix",
                "command": {"title": "run <script>", "arguments": ["javascript:void 0"]}
            }),
        ),
        (
            "completionItem/resolve",
            serde_json::json!({"label": "f", "documentation": "POD quoting <script>"}),
        ),
        (
            "inlayHint/resolve",
            serde_json::json!({"label": "<script", "tooltip": "javascript: in POD"}),
        ),
        ("documentLink/resolve", serde_json::json!({"tooltip": "see <script> tag docs"})),
        (
            "codeLens/resolve",
            serde_json::json!({"command": {"title": "<script>", "arguments": ["javascript:"]}}),
        ),
    ];

    let mut rejected = Vec::new();
    for (method, params) in cases {
        if validate_request_admission(method, params).is_err() {
            rejected.push(*method);
        }
    }
    assert!(
        rejected.is_empty(),
        "these item-bearing methods carry server-authored source text and must not be \
         content-scanned, but were rejected: {rejected:?}"
    );
}

// ---------------------------------------------------------------------------
// sanitize_string: boundary character cases
// ---------------------------------------------------------------------------

#[test]
fn sanitize_string_keeps_tab_character() {
    assert_eq!(sanitize_string("\t"), "\t", "tab must be kept");
}

#[test]
fn sanitize_string_keeps_newline() {
    assert_eq!(sanitize_string("\n"), "\n", "newline must be kept");
}

#[test]
fn sanitize_string_keeps_carriage_return() {
    assert_eq!(sanitize_string("\r"), "\r", "carriage return must be kept");
}

#[test]
fn sanitize_string_keeps_printable_ascii() {
    let printable = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
    assert_eq!(sanitize_string(printable), printable, "all printable ASCII must be kept");
}

#[test]
fn sanitize_string_removes_control_chars() {
    // DEL (0x7F) and other control chars below space (0x20) except tab/LF/CR
    let input = "\x01\x02\x03\x7f";
    let result = sanitize_string(input);
    assert_eq!(result, "", "control characters must be removed");
}

#[test]
fn sanitize_string_keeps_unicode_above_127() {
    // Characters > 127 are allowed (unicode)
    let input = "Héllo Wörld";
    let result = sanitize_string(input);
    assert_eq!(result, input, "unicode chars > 127 must be kept");
}

#[test]
fn sanitize_string_removes_null_byte() {
    // Null byte (0x00) is below space and not tab/LF/CR → removed
    let result = sanitize_string("a\x00b");
    assert_eq!(result, "ab", "null byte must be removed by sanitize_string");
}

#[test]
fn sanitize_string_empty_input_returns_empty() {
    assert_eq!(sanitize_string(""), "", "empty input must return empty");
}

#[test]
fn sanitize_string_all_safe_returns_unchanged() {
    let input = "sub foo { return 42; }";
    assert_eq!(sanitize_string(input), input, "safe Perl code must be unchanged");
}

// ---------------------------------------------------------------------------
// Shared helper for the resource-bound tests above.
//
// MAX_PARAMS_SIZE is 1,000,000; the default maxFileSizeBytes is 1,048,576. A
// document in that band used to be rejected by the params guard before the
// configured file limit was consulted — and on didOpen/didChange, which are
// notifications, that rejection is silent, so the document is simply never
// stored.
// ---------------------------------------------------------------------------

/// Build a document of exactly `len` bytes shaped like real source: short
/// lines, so the separate per-line length guard is not what is under test.
fn document_of_len(len: usize) -> String {
    // 80 source bytes + '\n' per line.
    let line = format!("{}\n", "a".repeat(79));
    let mut text = line.repeat(len / line.len());
    text.push_str(&"b".repeat(len - text.len()));
    debug_assert_eq!(text.len(), len);
    text
}
