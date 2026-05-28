//! Scenario 57 - loop-binding inline-completion quality receipt.
//!
//! This receipt exercises deterministic visible-collection inline completion
//! through a real stdio LSP process. It verifies that invoked ghost text after
//! `for ` uses arrays and hash keys already visible in the file instead of
//! falling back to placeholder loop snippets.

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

const SCENARIO_FILE: &str = "ux_scenario_57_loop_binding_inline_completion_quality.rs";
const ARRAY_LOOP_PATH: &str = "lib/Inline/LoopArray.pl";
const HASH_LOOP_PATH: &str = "lib/Inline/LoopHash.pl";
const ARRAY_PREFERENCE_PATH: &str = "lib/Inline/LoopArrayPreference.pl";
const STATUS_LOOP_PATH: &str = "lib/Inline/LoopStatus.pl";

const ARRAY_LOOP_SOURCE: &str = r#"use strict;
use warnings;

my @users = fetch_users();
for "#;

const HASH_LOOP_SOURCE: &str = r#"use strict;
use warnings;

my %users_by_id = load_users();
for "#;

const ARRAY_PREFERENCE_SOURCE: &str = r#"use strict;
use warnings;

my %users_by_id = load_users();
my @users = values %users_by_id;
for "#;

const STATUS_LOOP_SOURCE: &str = r#"use strict;
use warnings;

my @status = fetch_status();
for "#;

#[derive(Debug, Serialize)]
struct InlineLoopProbeReport {
    file: &'static str,
    trigger_kind: u8,
    candidate_count: usize,
    insert_texts: Vec<String>,
    expected_insert_text: &'static str,
    expected_present: bool,
    forbidden_fragments: Vec<&'static str>,
    matched_forbidden_fragments: Vec<&'static str>,
}

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default()
        .with_file(ARRAY_LOOP_PATH, ARRAY_LOOP_SOURCE)
        .with_file(HASH_LOOP_PATH, HASH_LOOP_SOURCE)
        .with_file(ARRAY_PREFERENCE_PATH, ARRAY_PREFERENCE_SOURCE)
        .with_file(STATUS_LOOP_PATH, STATUS_LOOP_SOURCE);
    config.client_capability_overrides = json!({
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });

    UxHarness::new(config)
}

fn cursor_at_end(source: &str) -> Result<(u32, u32)> {
    position_from_byte_offset(source, source.len())
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

fn probe_loop_binding_inline_completion(
    harness: &UxHarness,
    file: &'static str,
    source: &str,
    expected_insert_text: &'static str,
    forbidden_fragments: &[&'static str],
) -> Result<InlineLoopProbeReport> {
    let (line, character) = cursor_at_end(source)?;
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
        if insert_texts.iter().any(|insert_text| insert_text == expected_insert_text)
            || Instant::now() >= deadline
        {
            break items;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let insert_texts = insert_texts_for(&items);
    let matched_forbidden_fragments = forbidden_fragments
        .iter()
        .copied()
        .filter(|forbidden| insert_texts.iter().any(|actual| actual.contains(forbidden)))
        .collect::<Vec<_>>();
    let expected_present = insert_texts.iter().any(|actual| actual == expected_insert_text);

    Ok(InlineLoopProbeReport {
        file,
        trigger_kind: 1,
        candidate_count: items.len(),
        insert_texts,
        expected_insert_text,
        expected_present,
        forbidden_fragments: forbidden_fragments.to_vec(),
        matched_forbidden_fragments,
    })
}

fn insert_texts_for(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(inline_insert_text).collect()
}

#[test]
fn scenario_57_loop_binding_inline_completion_quality_receipt() {
    run_ux_scenario(
        "loop_binding_inline_completion_quality",
        SCENARIO_FILE,
        "scenario_57_loop_binding_inline_completion_quality_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = create_harness()?;
            harness.open_file(ARRAY_LOOP_PATH, ARRAY_LOOP_SOURCE)?;
            harness.open_file(HASH_LOOP_PATH, HASH_LOOP_SOURCE)?;
            harness.open_file(ARRAY_PREFERENCE_PATH, ARRAY_PREFERENCE_SOURCE)?;
            harness.open_file(STATUS_LOOP_PATH, STATUS_LOOP_SOURCE)?;
            std::thread::sleep(Duration::from_millis(250));

            recorder.mark_request_start("dynamic_inline_registration");
            let dynamic_registration_seen = wait_for_inline_registration(&harness);
            if dynamic_registration_seen {
                recorder.mark_first_useful_result("dynamic_inline_registration");
            }

            recorder.mark_request_start("array_loop_binding_inline_completion");
            let array_report = probe_loop_binding_inline_completion(
                &harness,
                ARRAY_LOOP_PATH,
                ARRAY_LOOP_SOURCE,
                "my $user (@users) {\n    \n}",
                &["(@items)"],
            )?;
            if array_report.expected_present {
                recorder.mark_first_useful_result("array_loop_binding_inline_completion");
            }

            recorder.mark_request_start("hash_loop_binding_inline_completion");
            let hash_report = probe_loop_binding_inline_completion(
                &harness,
                HASH_LOOP_PATH,
                HASH_LOOP_SOURCE,
                "my $id (keys %users_by_id) {\n    \n}",
                &["(@items)"],
            )?;
            if hash_report.expected_present {
                recorder.mark_first_useful_result("hash_loop_binding_inline_completion");
            }

            recorder.mark_request_start("array_preference_loop_binding_inline_completion");
            let array_preference_report = probe_loop_binding_inline_completion(
                &harness,
                ARRAY_PREFERENCE_PATH,
                ARRAY_PREFERENCE_SOURCE,
                "my $user (@users) {\n    \n}",
                &["keys %users_by_id", "(@items)"],
            )?;
            if array_preference_report.expected_present {
                recorder
                    .mark_first_useful_result("array_preference_loop_binding_inline_completion");
            }

            recorder.mark_request_start("status_loop_binding_inline_completion");
            let status_report = probe_loop_binding_inline_completion(
                &harness,
                STATUS_LOOP_PATH,
                STATUS_LOOP_SOURCE,
                "my $item (@status) {\n    \n}",
                &["$statu", "(@items)"],
            )?;
            if status_report.expected_present {
                recorder.mark_first_useful_result("status_loop_binding_inline_completion");
            }

            let receipt = json!({
                "schema_version": 1,
                "receipt": "loop_binding_inline_completion_quality",
                "claim_boundary": "stdio inline-completion loop-binding quality receipt only; no provider behavior change, support-tier promotion, source mirror, release action, or AI behavior",
                "dynamic_registration_seen": dynamic_registration_seen,
                "array_probe": array_report,
                "hash_probe": hash_report,
                "array_preference_probe": array_preference_report,
                "status_probe": status_report,
            });
            eprintln!(
                "loop_binding_inline_completion_quality_receipt={}",
                serde_json::to_string_pretty(&receipt)?
            );

            recorder
                .check("dynamic inline registration was observed", dynamic_registration_seen)?;
            recorder.check(
                "array loop binding used visible @users collection",
                array_report.expected_present,
            )?;
            recorder.check(
                "array loop binding avoided placeholder snippets",
                array_report.matched_forbidden_fragments.is_empty(),
            )?;
            recorder.check(
                "hash loop binding used visible %users_by_id keys",
                hash_report.expected_present,
            )?;
            recorder.check(
                "hash loop binding avoided placeholder snippets",
                hash_report.matched_forbidden_fragments.is_empty(),
            )?;
            recorder.check(
                "array loop binding stays preferred over hash keys when both are visible",
                array_preference_report.expected_present,
            )?;
            recorder.check(
                "array preference loop binding avoided hash-key and placeholder fallbacks",
                array_preference_report.matched_forbidden_fragments.is_empty(),
            )?;
            recorder.check(
                "status loop binding avoided unsafe singular trimming",
                status_report.expected_present
                    && status_report.matched_forbidden_fragments.is_empty(),
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
