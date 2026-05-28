//! Scenario 55 - DBI receiver inline-completion quality receipt.
//!
//! This receipt exercises deterministic DBI receiver inline completion through
//! a real stdio LSP process. It records whether `$dbh->` and `$sth->f` ghost
//! text uses DBI handle methods instead of generic constructor guesses.

// UX receipt tests intentionally write structured receipts to stderr for --nocapture logs.
#![allow(clippy::print_stderr)]

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SCENARIO_FILE: &str = "ux_scenario_55_dbi_receiver_inline_completion_quality.rs";
const DBI_HANDLE_PATH: &str = "lib/Inline/DbiHandle.pl";
const DBI_STATEMENT_PATH: &str = "lib/Inline/DbiStatement.pl";
const DBI_HANDLE_MARKER: &str = "$dbh->";
const DBI_STATEMENT_MARKER: &str = "$sth->f";

const DBI_HANDLE_SOURCE: &str = r#"use strict;
use warnings;
use DBI;

my $dbh = DBI->connect($dsn);
$dbh->
"#;

const DBI_STATEMENT_SOURCE: &str = r#"use strict;
use warnings;
use DBI;

my $dbh = DBI->connect($dsn);
my $sth = $dbh->prepare($sql);
$sth->f
"#;

const EXPECTED_HANDLE_INSERTS: &[&str] = &["prepare()", "do()", "disconnect()"];
const EXPECTED_STATEMENT_INSERTS: &[&str] = &["fetchrow_hashref()", "fetchrow_array()", "finish()"];
const FORBIDDEN_INSERTS: &[&str] = &["new()"];

#[derive(Debug, Serialize)]
struct DbiReceiverProbeReport {
    file: &'static str,
    receiver_kind: &'static str,
    trigger_kind: u8,
    candidate_count: usize,
    insert_texts: Vec<String>,
    expected_insert_texts: Vec<&'static str>,
    missing_expected_insert_texts: Vec<&'static str>,
    forbidden_insert_texts: Vec<&'static str>,
}

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default()
        .with_file(DBI_HANDLE_PATH, DBI_HANDLE_SOURCE)
        .with_file(DBI_STATEMENT_PATH, DBI_STATEMENT_SOURCE);
    config.client_capability_overrides = json!({
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });

    UxHarness::new(config)
}

fn position_after(source: &str, needle: &str) -> Result<(u32, u32)> {
    let byte_offset =
        source.find(needle).with_context(|| format!("missing `{needle}`"))? + needle.len();
    position_from_byte_offset(source, byte_offset)
}

fn position_from_byte_offset(source: &str, byte_offset: usize) -> Result<(u32, u32)> {
    let prefix = source
        .get(..byte_offset)
        .with_context(|| format!("byte offset {byte_offset} is not a UTF-8 boundary"))?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let character = prefix.rsplit('\n').next().map(str::chars).map(Iterator::count).unwrap_or(0);
    Ok((u32::try_from(line)?, u32::try_from(character)?))
}

fn inline_insert_text(item: &Value) -> Option<String> {
    item.get("insertText").and_then(Value::as_str).map(str::to_string)
}

fn item_has_inline_shape(item: &Value) -> bool {
    item.get("insertText").and_then(Value::as_str).is_some()
}

fn inline_registration_seen(events: &[LspEvent]) -> bool {
    events.iter().any(|event| {
        let LspEvent::Other { method, params } = event else {
            return false;
        };
        method == "client/registerCapability"
            && params.get("registrations").and_then(Value::as_array).into_iter().flatten().any(
                |registration| {
                    registration.get("method").and_then(Value::as_str)
                        == Some("textDocument/inlineCompletion")
                        && registration.get("id").and_then(Value::as_str)
                            == Some("perl-inlineCompletion")
                },
            )
    })
}

fn wait_for_inline_registration(harness: &UxHarness) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if inline_registration_seen(&harness.client.peek_events()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn probe_dbi_receiver_inline_completion(
    harness: &UxHarness,
    file: &'static str,
    source: &str,
    receiver_kind: &'static str,
    marker: &'static str,
    expected_insert_texts: &[&'static str],
) -> Result<DbiReceiverProbeReport> {
    let (line, character) = position_after(source, marker)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let items = loop {
        let items = harness.inline_completion_with_trigger_kind(file, line, character, 1)?;
        for item in &items {
            anyhow::ensure!(
                item_has_inline_shape(item),
                "inline item must include insertText: {item:?}"
            );
        }
        let insert_texts = insert_texts_for(&items);
        if expected_insert_texts
            .iter()
            .all(|expected| insert_texts.iter().any(|actual| actual == expected))
            || Instant::now() >= deadline
        {
            break items;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let insert_texts = insert_texts_for(&items);
    let missing_expected_insert_texts = expected_insert_texts
        .iter()
        .copied()
        .filter(|expected| !insert_texts.iter().any(|actual| actual == expected))
        .collect::<Vec<_>>();
    let forbidden_insert_texts = FORBIDDEN_INSERTS
        .iter()
        .copied()
        .filter(|forbidden| insert_texts.iter().any(|actual| actual == forbidden))
        .collect::<Vec<_>>();

    Ok(DbiReceiverProbeReport {
        file,
        receiver_kind,
        trigger_kind: 1,
        candidate_count: items.len(),
        insert_texts,
        expected_insert_texts: expected_insert_texts.to_vec(),
        missing_expected_insert_texts,
        forbidden_insert_texts,
    })
}

fn insert_texts_for(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(inline_insert_text).collect()
}

#[test]
fn scenario_55_dbi_receiver_inline_completion_quality_receipt() {
    run_ux_scenario(
        "dbi_receiver_inline_completion_quality",
        SCENARIO_FILE,
        "scenario_55_dbi_receiver_inline_completion_quality_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = create_harness()?;
            harness.open_file(DBI_HANDLE_PATH, DBI_HANDLE_SOURCE)?;
            harness.open_file(DBI_STATEMENT_PATH, DBI_STATEMENT_SOURCE)?;
            std::thread::sleep(Duration::from_millis(250));

            recorder.mark_request_start("dynamic_inline_registration");
            let dynamic_registration_seen = wait_for_inline_registration(&harness);
            if dynamic_registration_seen {
                recorder.mark_first_useful_result("dynamic_inline_registration");
            }

            recorder.mark_request_start("dbi_handle_inline_completion");
            let handle_report = probe_dbi_receiver_inline_completion(
                &harness,
                DBI_HANDLE_PATH,
                DBI_HANDLE_SOURCE,
                "database_handle",
                DBI_HANDLE_MARKER,
                EXPECTED_HANDLE_INSERTS,
            )?;
            if handle_report.missing_expected_insert_texts.is_empty() {
                recorder.mark_first_useful_result("dbi_handle_inline_completion");
            }

            recorder.mark_request_start("dbi_statement_inline_completion");
            let statement_report = probe_dbi_receiver_inline_completion(
                &harness,
                DBI_STATEMENT_PATH,
                DBI_STATEMENT_SOURCE,
                "statement_handle",
                DBI_STATEMENT_MARKER,
                EXPECTED_STATEMENT_INSERTS,
            )?;
            if statement_report.missing_expected_insert_texts.is_empty() {
                recorder.mark_first_useful_result("dbi_statement_inline_completion");
            }

            let receipt = json!({
                "schema_version": 1,
                "receipt": "dbi_receiver_inline_completion_quality",
                "claim_boundary": "stdio inline-completion DBI receiver quality receipt only; no provider behavior change, support-tier promotion, source mirror, release action, or AI behavior",
                "dynamic_registration_seen": dynamic_registration_seen,
                "database_handle_probe": handle_report,
                "statement_handle_probe": statement_report,
            });
            eprintln!(
                "dbi_receiver_inline_completion_quality_receipt={}",
                serde_json::to_string_pretty(&receipt)?
            );

            recorder
                .check("dynamic inline registration was observed", dynamic_registration_seen)?;
            recorder.check(
                "DBI database handle inline completion returned candidates",
                handle_report.candidate_count > 0,
            )?;
            recorder.check(
                "DBI database handle inline completion used DBI handle methods",
                handle_report.missing_expected_insert_texts.is_empty(),
            )?;
            recorder.check(
                "DBI database handle inline completion avoided generic constructor guesses",
                handle_report.forbidden_insert_texts.is_empty(),
            )?;
            recorder.check(
                "DBI statement handle inline completion used statement methods",
                statement_report.missing_expected_insert_texts.is_empty(),
            )?;
            recorder.check(
                "DBI statement handle inline completion avoided generic constructor guesses",
                statement_report.forbidden_insert_texts.is_empty(),
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
