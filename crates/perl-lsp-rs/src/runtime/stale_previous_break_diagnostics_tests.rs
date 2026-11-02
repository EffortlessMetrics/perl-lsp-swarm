//! #18763: a new break must not republish the previous break's diagnostics.
//!
//! Probe sessions that fixed an unterminated heredoc, then introduced a
//! duplicate `my $grand_total`, observed the *previous* heredoc errors shifted
//! by the inserted line. The real redeclaration diagnostic arrived only after
//! the next keystroke. These tests pin the sequential-session publication
//! contract on the push path.

use super::LspServer;
use crate::features::diagnostics::PullDiagnosticsProvider;
use parking_lot::Mutex;
use perl_lsp_rs_core::governance::FeatureProfile;
use perl_lsp_rs_core::runtime::tuning::RuntimeTuning;
use perl_parser::Parser;
use perl_parser_core::RetainedRegexSession;
use perl_tdd_support::must;
use serde_json::Value;
use std::io::Write;
use std::sync::Arc as StdArc;
use std::time::{Duration, Instant};

struct SharedVecWriter {
    inner: StdArc<Mutex<Vec<u8>>>,
}

impl Write for SharedVecWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 79-line Moo class with a heredoc, matching the #18763 probe shape.
fn inventory_source() -> String {
    let mut lines = vec![
        "package Inventory;".to_string(),
        "use Moo;".to_string(),
        "use strict;".to_string(),
        "use warnings;".to_string(),
        String::new(),
    ];
    for index in 1..=20 {
        lines.push(format!("# inventory padding line {index}"));
    }
    lines.extend([
        String::new(),
        "has sku => (is => 'ro', required => 1);".to_string(),
        "has qty => (is => 'ro', default => sub { 0 });".to_string(),
        "has price => (is => 'ro', default => sub { 0 });".to_string(),
        String::new(),
        "sub summarize {".to_string(),
        "    my ($self) = @_;".to_string(),
        "    my $grand_total = 0;".to_string(),
        "    $grand_total += $self->qty * $self->price;".to_string(),
        "    my $body = <<'END_REPORT';".to_string(),
        "Inventory report".to_string(),
        "END_REPORT".to_string(),
        "    return $grand_total;".to_string(),
        "}".to_string(),
        String::new(),
        "1;".to_string(),
    ]);
    while lines.len() < 79 {
        lines.push(format!("# trailing padding {}", lines.len() + 1));
    }
    lines.join("\n") + "\n"
}

fn broken_heredoc(source: &str) -> String {
    source.replacen("\nEND_REPORT\n", "\nEND_REPOR\n", 1)
}

fn with_duplicate_grand_total(source: &str) -> String {
    source.replacen(
        "    my $grand_total = 0;\n",
        "    my $grand_total = 0;\n    my $grand_total = 0;\n",
        1,
    )
}

fn parse_messages_with_retained_session(source: &str) -> Vec<String> {
    let code_text = crate::util::code_slice(source);
    let session = RetainedRegexSession::begin(code_text);
    let mut parser = Parser::new(code_text);
    match parser.parse() {
        Ok(mut ast) => {
            let _table = session.finish(Some(&mut ast));
            parser.errors().iter().map(ToString::to_string).collect()
        }
        Err(error) => {
            let _table = session.finish(None);
            vec![error.to_string()]
        }
    }
}

fn pull_messages(source: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let provider = PullDiagnosticsProvider::new();
    let uri: lsp_types::Uri = "file:///inventory_18763.pm".parse()?;
    match provider.get_document_diagnostics(&uri, source, None, None) {
        lsp_types::DocumentDiagnosticReport::Full(full) => Ok(full
            .full_document_diagnostic_report
            .items
            .into_iter()
            .map(|item| item.message)
            .collect()),
        lsp_types::DocumentDiagnosticReport::Unchanged(_) => {
            Err("expected a full diagnostic report, got Unchanged".into())
        }
    }
}

fn message_is_unterminated_heredoc(message: &str) -> bool {
    message.contains("Unterminated heredoc")
}

fn message_is_grand_total_redeclaration(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("$grand_total")
        && (lower.contains("declared again")
            || lower.contains("declared more than once")
            || lower.contains("redeclared"))
}

fn make_server_with_capture(tuning: RuntimeTuning) -> (LspServer, StdArc<Mutex<Vec<u8>>>) {
    let buf = StdArc::new(Mutex::new(Vec::<u8>::new()));
    let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
    let server = LspServer::with_io_feature_profile_and_tuning(
        Box::new(std::io::Cursor::new(Vec::new())),
        Box::new(writer),
        FeatureProfile::current(),
        tuning,
    );
    server.publish_position_encoding_session_context();
    (server, buf)
}

fn immediate_tuning() -> RuntimeTuning {
    let mut tuning = RuntimeTuning::normal_defaults();
    tuning.diagnostic_debounce_ms = 0;
    tuning
}

fn make_async_server_with_capture() -> (StdArc<LspServer>, StdArc<Mutex<Vec<u8>>>) {
    make_async_server_with_tuning(immediate_tuning())
}

fn make_async_server_with_tuning(
    tuning: RuntimeTuning,
) -> (StdArc<LspServer>, StdArc<Mutex<Vec<u8>>>) {
    let buf = StdArc::new(Mutex::new(Vec::<u8>::new()));
    let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
    let server = StdArc::new(LspServer::with_io_feature_profile_and_tuning(
        Box::new(std::io::Cursor::new(Vec::new())),
        Box::new(writer),
        FeatureProfile::current(),
        tuning,
    ));
    server.publish_position_encoding_session_context();
    server.test_install_parse_worker();
    (server, buf)
}

fn make_production_like_server() -> (StdArc<LspServer>, StdArc<Mutex<Vec<u8>>>) {
    let (server, buf) = make_async_server_with_tuning(RuntimeTuning::normal_defaults());
    server.install_default_diagnostic_debouncer();
    (server, buf)
}

fn published_frames(text: &str) -> Vec<Value> {
    let mut remaining = text;
    let mut frames = Vec::new();
    while let Some(header_start) = remaining.find("Content-Length:") {
        let Some(after_header) = remaining.get(header_start + "Content-Length:".len()..) else {
            break;
        };
        let Some((header, body)) = after_header.split_once("\r\n\r\n") else {
            break;
        };
        let Some(length_text) = header.lines().next() else {
            break;
        };
        let Ok(length) = length_text.trim().parse::<usize>() else {
            break;
        };
        let body_bytes = body.as_bytes();
        let Some(frame_bytes) = body_bytes.get(..length) else {
            break;
        };
        let Some(rest) = body_bytes.get(length..) else {
            break;
        };
        let Ok(frame) = std::str::from_utf8(frame_bytes) else {
            break;
        };
        if let Ok(value) = serde_json::from_str::<Value>(frame)
            && value.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
        {
            frames.push(value);
        }
        let Ok(next) = std::str::from_utf8(rest) else {
            break;
        };
        remaining = next;
    }
    frames
}

fn messages_from_params(params: &Value) -> Vec<String> {
    params
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("message").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn frames_for_version(text: &str, uri: &str, version: i64) -> Vec<Vec<String>> {
    published_frames(text)
        .into_iter()
        .filter_map(|frame| {
            let params = frame.get("params")?;
            if params.get("uri").and_then(Value::as_str) != Some(uri) {
                return None;
            }
            if params.get("version").and_then(Value::as_i64) != Some(version) {
                return None;
            }
            Some(messages_from_params(params))
        })
        .collect()
}

fn latest_messages_for_version(text: &str, uri: &str, version: i64) -> Option<Vec<String>> {
    frames_for_version(text, uri, version).into_iter().next_back()
}

fn captured_text(buf: &StdArc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock()).into_owned()
}

fn assert_no_unterminated_heredoc_on_version(text: &str, uri: &str, version: i64, context: &str) {
    let frames = frames_for_version(text, uri, version);
    for messages in &frames {
        assert!(
            !messages.iter().any(|message| message_is_unterminated_heredoc(message)),
            "{context}: version {version} must not publish Unterminated heredoc; frames={frames:?}"
        );
    }
}

fn wait_for_version_messages(
    buf: &StdArc<Mutex<Vec<u8>>>,
    uri: &str,
    version: i64,
    timeout: Duration,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let text = String::from_utf8_lossy(&buf.lock()).into_owned();
        if let Some(messages) = latest_messages_for_version(&text, uri, version) {
            return Ok(messages);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for version {version} publishDiagnostics; captured: {text}"
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_parse_settle(server: &LspServer, uri: &str) {
    assert!(
        server.test_wait_for_parse_worker_settled(uri, Duration::from_secs(5)),
        "parse worker must settle for {uri}"
    );
}

fn assert_session_after_second_break(messages: &[String], context: &str) {
    assert!(
        !messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "{context}: new break must not republish the previous unterminated-heredoc diagnostic; got {messages:?}"
    );
    assert!(
        messages.iter().any(|message| message_is_grand_total_redeclaration(message)),
        "{context}: new break must publish the duplicate $grand_total diagnostic in-session; got {messages:?}"
    );
}

#[test]
fn sequential_parser_passes_do_not_keep_a_fixed_heredoc_broken() {
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    let broken_messages = parse_messages_with_retained_session(&broken);
    assert!(
        broken_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "broken terminator must be an unterminated heredoc: {broken_messages:?}"
    );

    let fixed_messages = parse_messages_with_retained_session(&clean);
    assert!(
        !fixed_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "repairing the terminator must clear the heredoc diagnostic: {fixed_messages:?}"
    );

    let duplicate_messages = parse_messages_with_retained_session(&duplicate);
    assert!(
        !duplicate_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "a later duplicate-my edit must not resurrect the heredoc diagnostic: {duplicate_messages:?}"
    );
}

#[test]
fn fresh_pull_of_duplicate_my_does_not_report_the_prior_heredoc() {
    let clean = inventory_source();
    let duplicate = with_duplicate_grand_total(&clean);
    let messages = must(pull_messages(&duplicate));
    assert_session_after_second_break(&messages, "fresh pull of the combined source");
}

#[test]
fn sequential_push_session_publishes_the_new_break_not_the_previous_heredoc()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, buf) = make_server_with_capture(immediate_tuning());
    let uri = "file:///lib/Inventory.pm";
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    server.test_apply_did_open(uri, &clean, 1)?;
    let opened = wait_for_version_messages(&buf, uri, 1, Duration::from_secs(5))?;
    assert!(
        !opened.iter().any(|message| message_is_unterminated_heredoc(message)),
        "clean open must not report an unterminated heredoc: {opened:?}"
    );

    server.test_apply_did_change(uri, &broken, 2)?;
    let broken_messages = wait_for_version_messages(&buf, uri, 2, Duration::from_secs(5))?;
    assert!(
        broken_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "breaking the terminator must publish Unterminated heredoc; got {broken_messages:?}"
    );

    server.test_apply_did_change(uri, &clean, 3)?;
    let cleared = wait_for_version_messages(&buf, uri, 3, Duration::from_secs(5))?;
    assert!(
        !cleared.iter().any(|message| message_is_unterminated_heredoc(message)),
        "fixing the terminator must clear the heredoc diagnostic; got {cleared:?}"
    );

    server.test_apply_did_change(uri, &duplicate, 4)?;
    let second_break = wait_for_version_messages(&buf, uri, 4, Duration::from_secs(5))?;
    assert_session_after_second_break(&second_break, "synchronous sequential push session");
    assert_no_unterminated_heredoc_on_version(
        &captured_text(&buf),
        uri,
        4,
        "synchronous sequential push session",
    );
    Ok(())
}

#[test]
fn async_sequential_push_session_publishes_the_new_break_not_the_previous_heredoc()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, buf) = make_async_server_with_capture();
    let uri = "file:///lib/InventoryAsync.pm";
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    server.test_apply_did_open(uri, &clean, 1)?;
    wait_for_parse_settle(&server, uri);
    let opened = wait_for_version_messages(&buf, uri, 1, Duration::from_secs(5))?;
    assert!(
        !opened.iter().any(|message| message_is_unterminated_heredoc(message)),
        "clean async open must not report an unterminated heredoc: {opened:?}"
    );

    server.test_apply_did_change(uri, &broken, 2)?;
    wait_for_parse_settle(&server, uri);
    let broken_messages = wait_for_version_messages(&buf, uri, 2, Duration::from_secs(5))?;
    assert!(
        broken_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "async break must publish Unterminated heredoc; got {broken_messages:?}"
    );

    server.test_apply_did_change(uri, &clean, 3)?;
    wait_for_parse_settle(&server, uri);
    let cleared = wait_for_version_messages(&buf, uri, 3, Duration::from_secs(5))?;
    assert!(
        !cleared.iter().any(|message| message_is_unterminated_heredoc(message)),
        "async fix must clear the heredoc diagnostic; got {cleared:?}"
    );

    server.test_apply_did_change(uri, &duplicate, 4)?;
    wait_for_parse_settle(&server, uri);
    let second_break = wait_for_version_messages(&buf, uri, 4, Duration::from_secs(5))?;
    assert_session_after_second_break(&second_break, "async sequential push session");
    assert_no_unterminated_heredoc_on_version(
        &captured_text(&buf),
        uri,
        4,
        "async sequential push session",
    );
    Ok(())
}

#[test]
fn production_debounced_session_publishes_the_new_break_not_the_previous_heredoc()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, buf) = make_production_like_server();
    let uri = "file:///lib/InventoryDebounced.pm";
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    server.test_apply_did_open(uri, &clean, 1)?;
    wait_for_parse_settle(&server, uri);
    wait_for_version_messages(&buf, uri, 1, Duration::from_secs(5))?;

    server.test_apply_did_change(uri, &broken, 2)?;
    wait_for_parse_settle(&server, uri);
    let broken_messages = wait_for_version_messages(&buf, uri, 2, Duration::from_secs(5))?;
    assert!(
        broken_messages.iter().any(|message| message_is_unterminated_heredoc(message)),
        "debounced break must publish Unterminated heredoc; got {broken_messages:?}"
    );

    server.test_apply_did_change(uri, &clean, 3)?;
    wait_for_parse_settle(&server, uri);
    let cleared = wait_for_version_messages(&buf, uri, 3, Duration::from_secs(5))?;
    assert!(
        !cleared.iter().any(|message| message_is_unterminated_heredoc(message)),
        "debounced fix must clear the heredoc diagnostic; got {cleared:?}"
    );

    server.test_apply_did_change(uri, &duplicate, 4)?;
    wait_for_parse_settle(&server, uri);
    let second_break = wait_for_version_messages(&buf, uri, 4, Duration::from_secs(5))?;
    // The probe's first-diagnostics window is ~250ms debounce plus parse; wait
    // once more so a late stale frame cannot hide behind the first version-4
    // publication.
    std::thread::sleep(Duration::from_millis(400));
    assert_session_after_second_break(&second_break, "debounced production-like session");
    assert_no_unterminated_heredoc_on_version(
        &captured_text(&buf),
        uri,
        4,
        "debounced production-like session",
    );
    Ok(())
}

#[test]
fn rapid_second_break_must_not_republish_shifted_heredoc_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, buf) = make_production_like_server();
    let uri = "file:///lib/InventoryRapid.pm";
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    server.test_apply_did_open(uri, &clean, 1)?;
    wait_for_parse_settle(&server, uri);
    wait_for_version_messages(&buf, uri, 1, Duration::from_secs(5))?;

    server.test_apply_did_change(uri, &broken, 2)?;
    wait_for_parse_settle(&server, uri);
    wait_for_version_messages(&buf, uri, 2, Duration::from_secs(5))?;

    // Fix and immediately introduce the new break, without waiting for the
    // fix's cleared publication. This is the overlapping debounce window.
    server.test_apply_did_change(uri, &clean, 3)?;
    server.test_apply_did_change(uri, &duplicate, 4)?;
    wait_for_parse_settle(&server, uri);
    let second_break = wait_for_version_messages(&buf, uri, 4, Duration::from_secs(5))?;
    std::thread::sleep(Duration::from_millis(400));
    assert_session_after_second_break(&second_break, "rapid second-break session");
    assert_no_unterminated_heredoc_on_version(
        &captured_text(&buf),
        uri,
        4,
        "rapid second-break session",
    );
    Ok(())
}

#[test]
fn pending_parse_gap_must_not_remap_previous_heredoc_onto_the_new_break()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, buf) = make_server_with_capture(immediate_tuning());
    let uri = "file:///lib/InventoryGap.pm";
    let clean = inventory_source();
    let broken = broken_heredoc(&clean);
    let duplicate = with_duplicate_grand_total(&clean);

    server.test_apply_did_open(uri, &clean, 1)?;
    wait_for_version_messages(&buf, uri, 1, Duration::from_secs(5))?;
    server.test_apply_did_change(uri, &broken, 2)?;
    wait_for_version_messages(&buf, uri, 2, Duration::from_secs(5))?;

    server
        .test_apply_text_change_without_reparse(uri, &duplicate, 3)
        .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
    server.publish_parse_errors_fast(uri);
    server.publish_diagnostics(uri);
    std::thread::sleep(Duration::from_millis(50));
    assert_no_unterminated_heredoc_on_version(
        &captured_text(&buf),
        uri,
        3,
        "pending-parse gap must not remap the previous heredoc onto the new version",
    );

    server
        .test_publish_parse_for_current_generation(uri)
        .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
    server.publish_diagnostics(uri);
    let second_break = wait_for_version_messages(&buf, uri, 3, Duration::from_secs(5))?;
    assert_session_after_second_break(&second_break, "closed pending-parse gap");
    Ok(())
}
