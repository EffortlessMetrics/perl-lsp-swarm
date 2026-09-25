//! Scenario 63 seam-watch guard - inline completion derives candidates from
//! the session's active position encoding.
//!
//! #15963 / #15973: scenario_63 inline-completion quality regressed when
//! session-owned active position encoding landed (#13171) and was healed by
//! the text-sync envelope repair (#14897); the sensitivity was never pinned.
//! This guard pins the seam end to end:
//!
//! 1. The client prefers UTF-8, and the session must keep the advertised
//!    `positionEncoding` pinned to "utf-16" - the mandatory default. Until
//!    phase 2 threads a negotiated encoding through the providers, advertising
//!    anything else would silently corrupt every position-bearing response for
//!    non-ASCII content.
//! 2. The trigger line carries non-BMP characters whose UTF-16 column differs
//!    from its UTF-8 byte column, and the loop-binding candidate must still be
//!    produced from the trigger position encoded in UTF-16 code units. The
//!    historical defect this discriminates - decoding the position as raw byte
//!    offsets - lands the trigger on the closing quote, loses the `for `
//!    keyword, and drops the candidate.

// UX receipt tests intentionally write structured receipts to stderr for --nocapture logs.
#![allow(clippy::print_stderr)]

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available, missing_binary_skip,
    run_ux_scenario,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SCENARIO_FILE: &str = "ux_scenario_63_inline_completion_position_encoding_guard.rs";
const APP_PATH: &str = "lib/My/App.pm";
const ENCODING_LOOP_PATH: &str = "script/project-encoding-loop.pl";

const APP_PM: &str = r#"package My::App;
use strict;
use warnings;

sub new {
    my $class = shift;
    return bless {}, $class;
}

sub fetch_users { return (); }

1;
"#;

/// The trigger line carries three non-BMP characters (4 UTF-8 bytes / 2 UTF-16
/// units each), so the UTF-16 column and the UTF-8 byte column of the cursor
/// diverge by 6. Reading the UTF-16 column as raw bytes cuts the line at the
/// closing quote and loses the trailing `for ` keyword, while the session
/// encoding reading ends exactly after `for `.
const ENCODING_LOOP_SOURCE: &str = r#"use strict;
use warnings;
use lib 'lib';
use My::App;

my @users = My::App::fetch_users();
my $note = "Nutzer 🚀🚀🚀"; for "#;

const EXPECTED_INSERT_TEXT: &str = "my $user (@users) {\n    \n}";
const FORBIDDEN_INSERT_TEXTS: &[&str] = &["(@items)", "My::App;"];

#[derive(Debug, Serialize)]
struct EncodingGuardReport {
    negotiated_encoding: String,
    client_preference: Vec<String>,
    trigger_line_utf16_column: usize,
    trigger_line_utf8_column: usize,
    candidate_count: usize,
    insert_texts: Vec<String>,
    expected_present: bool,
    forbidden_hits: Vec<String>,
}

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default()
        .with_file(APP_PATH, APP_PM)
        .with_file(ENCODING_LOOP_PATH, ENCODING_LOOP_SOURCE);
    config.client_capability_overrides = json!({
        "general": {
            "positionEncodings": ["utf-8", "utf-16"]
        },
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });
    UxHarness::new(config)
}

fn negotiated_encoding(initialize_result: &Value) -> Option<String> {
    let capabilities = initialize_result
        .get("result")
        .and_then(|result| result.get("capabilities"))
        .or_else(|| initialize_result.get("capabilities"));
    capabilities
        .and_then(|capabilities| capabilities.get("positionEncoding"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// End-of-source position with the last line's column counted in the units of
/// the session's active position encoding (UTF-16 code units), unlike raw byte
/// or char-count readings.
fn cursor_at_end_session_columns(source: &str) -> Result<(u32, u32)> {
    let line = u32::try_from(source.bytes().filter(|byte| *byte == b'\n').count())?;
    let column = u32::try_from(
        source.rsplit('\n').next().map_or(0, |last_line| last_line.encode_utf16().count()),
    )?;
    Ok((line, column))
}

fn insert_texts(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get("insertText").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[test]
fn scenario_63_inline_completion_position_encoding_guard() {
    run_ux_scenario(
        "inline_completion_position_encoding_guard",
        SCENARIO_FILE,
        "scenario_63_inline_completion_position_encoding_guard",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = create_harness()?;
            harness.open_file(APP_PATH, APP_PM)?;
            harness.open_file(ENCODING_LOOP_PATH, ENCODING_LOOP_SOURCE)?;
            std::thread::sleep(Duration::from_millis(300));

            let active_encoding = negotiated_encoding(&harness.client.initialize_result())
                .context("initialize result did not advertise capabilities.positionEncoding")?;
            recorder.check(
                "session kept the advertised position encoding pinned to utf-16 even when the client prefers utf-8",
                active_encoding == "utf-16",
            )?;

            let (line, character) = cursor_at_end_session_columns(ENCODING_LOOP_SOURCE)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let items = loop {
                let items = harness.inline_completion_with_trigger_kind(
                    ENCODING_LOOP_PATH,
                    line,
                    character,
                    1,
                )?;
                let texts = insert_texts(&items);
                if texts.iter().any(|text| text == EXPECTED_INSERT_TEXT)
                    || Instant::now() >= deadline
                {
                    break items;
                }
                std::thread::sleep(Duration::from_millis(100));
            };

            let texts = insert_texts(&items);
            let expected_present = texts.iter().any(|text| text == EXPECTED_INSERT_TEXT);
            let forbidden_hits = FORBIDDEN_INSERT_TEXTS
                .iter()
                .filter(|forbidden| texts.iter().any(|text| text == *forbidden))
                .map(|forbidden| (*forbidden).to_string())
                .collect::<Vec<_>>();

            let last_line = ENCODING_LOOP_SOURCE.rsplit('\n').next().unwrap_or("");
            let report = EncodingGuardReport {
                negotiated_encoding: active_encoding.clone(),
                client_preference: vec!["utf-8".to_string(), "utf-16".to_string()],
                trigger_line_utf16_column: character as usize,
                trigger_line_utf8_column: last_line.len(),
                candidate_count: items.len(),
                insert_texts: texts,
                expected_present,
                forbidden_hits,
            };
            eprintln!(
                "inline_completion_position_encoding_guard_receipt={}",
                serde_json::to_string_pretty(&report)?
            );

            recorder.check(
                "loop binding candidate derives from the session's utf-16 position encoding over non-BMP content",
                expected_present,
            )?;
            recorder.check(
                "loop binding candidate avoided encoding-guard forbidden insert texts",
                report.forbidden_hits.is_empty(),
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
