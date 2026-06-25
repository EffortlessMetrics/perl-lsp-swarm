//! References tier-share scorecard harness.
//!
//! Drives the real `textDocument/references` handler across multiple fixtures and
//! index states (Full/Ready, Partial/Building, None), captures the decision-trace
//! receipt via `explainProviderDecision`, and emits a tier-distribution + latency
//! report to stderr (matching the DAP scorecard convention).
//!
//! # Placement rationale
//!
//! `index_coordinator` is `pub(crate)`, so an integration test in `tests/` cannot
//! set it directly without a new test-only method.  This unit test lives inside
//! `src/runtime/language/` where `LspServer`'s internal fields are in scope —
//! matching the existing patterns in `rename.rs:2045` and `signature_help.rs:1146`.
//! No new API surface is added.
//!
//! # Running
//!
//! ```text
//! CARGO_TARGET_DIR=.tmp/wt-target CARGO_INCREMENTAL=0 \
//!   cargo test -p perl-lsp-rs references_tier_scorecard -- --nocapture
//! ```

#[cfg(all(test, feature = "workspace"))]
mod scorecard {
    use crate::runtime::LspServer;
    use parking_lot::Mutex;
    use perl_parser::workspace_index::IndexCoordinator;
    use serde_json::{Value, json};
    use std::io::Cursor;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    // ---------------------------------------------------------------------------
    // Fixtures
    // ---------------------------------------------------------------------------

    /// A scalar used three times in a single file — cursor on `$count`.
    const SCALAR_THREE_USES_URI: &str = "file:///scorecard/scalar_three_uses.pl";
    const SCALAR_THREE_USES: &str = r#"use strict;
use warnings;
my $count = 0;
$count++;
print $count;
"#;

    /// A sub definition plus two call sites in the same file.
    const SUB_TWO_CALLS_URI: &str = "file:///scorecard/sub_two_calls.pm";
    const SUB_TWO_CALLS: &str = r#"package Score::Calls;
use strict;
use warnings;

sub calculate {
    return 42;
}

sub run {
    my $x = calculate();
    my $y = calculate();
    return $x + $y;
}

1;
"#;

    /// A package-qualified reference: `Score::Qualified::helper()`.
    const QUALIFIED_URI: &str = "file:///scorecard/qualified.pm";
    const QUALIFIED: &str = r#"package Score::Qualified;
use strict;
use warnings;

sub helper {
    return 1;
}

sub caller_a {
    Score::Qualified::helper();
}

sub caller_b {
    Score::Qualified::helper();
}

1;
"#;

    /// A no-symbol position (whitespace only) — must yield `empty` tier.
    const EMPTY_URI: &str = "file:///scorecard/empty_pos.pl";
    const EMPTY_DOC: &str = r#"use strict;
use warnings;
# This file is intentionally sparse.

"#;

    // ---------------------------------------------------------------------------
    // Helpers mirrored from navigation_runtime_quality_tests
    // ---------------------------------------------------------------------------

    fn create_server() -> LspServer {
        let output = Arc::new(Mutex::new(
            Box::new(Cursor::new(Vec::new())) as Box<dyn std::io::Write + Send>
        ));
        LspServer::with_output(output)
    }

    fn open_document(
        server: &LspServer,
        uri: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "text": text,
                "languageId": "perl",
                "version": 1
            }
        })))?;
        Ok(())
    }

    fn explain_provider_decision(
        server: &LspServer,
        provider: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = server
            .handle_execute_command(Some(json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": provider}]
            })))?
            .ok_or("missing explain-provider-decision response")?;
        Ok(response)
    }

    fn position_of(text: &str, needle: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        for (line_idx, line) in text.lines().enumerate() {
            if let Some(character) = line.find(needle) {
                return Ok((u32::try_from(line_idx)?, u32::try_from(character)?));
            }
        }
        Err(format!("needle `{needle}` not found").into())
    }

    // ---------------------------------------------------------------------------
    // Index-state helpers: mirrors the patterns in rename.rs + signature_help.rs
    // ---------------------------------------------------------------------------

    fn set_index_none(server: &mut LspServer) {
        server.index_coordinator = None;
    }

    fn set_index_building(server: &mut LspServer) {
        // Default `IndexCoordinator::new()` is already in Building/Scanning state.
        server.index_coordinator = Some(Arc::new(IndexCoordinator::new()));
    }

    fn set_index_ready(server: &mut LspServer) {
        let coordinator = Arc::new(IndexCoordinator::new());
        coordinator.transition_to_ready(3, 3);
        server.index_coordinator = Some(coordinator);
    }

    // ---------------------------------------------------------------------------
    // Measurement row
    // ---------------------------------------------------------------------------

    #[derive(Debug)]
    struct Row {
        fixture_id: &'static str,
        index_state: &'static str,
        answering_tier: String,
        result_count: u64,
        index_result_count: u64,
        text_result_count: u64,
        source_backed: bool,
        latency_us: u64,
    }

    /// Fire one references request, capture the receipt, return a measurement row.
    fn measure(
        fixture_id: &'static str,
        index_state_label: &'static str,
        server: &LspServer,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Row, Box<dyn std::error::Error>> {
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": include_declaration}
        });

        let t0 = Instant::now();
        server.test_handle_references(Some(params))?;
        let wall_us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);

        let explanation = explain_provider_decision(server, "references")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing request_receipt")?;

        let answering_tier =
            receipt.get("answering_tier").and_then(Value::as_str).unwrap_or("unknown").to_string();
        let result_count = receipt.get("result_count").and_then(Value::as_u64).unwrap_or(0);
        let index_result_count =
            receipt.get("index_result_count").and_then(Value::as_u64).unwrap_or(0);
        let text_result_count =
            receipt.get("text_result_count").and_then(Value::as_u64).unwrap_or(0);
        let source_backed = receipt.get("source_backed").and_then(Value::as_bool).unwrap_or(false);
        // Prefer receipt latency; fall back to wall time.
        let latency_us = receipt.get("latency_us").and_then(Value::as_u64).unwrap_or(wall_us);

        Ok(Row {
            fixture_id,
            index_state: index_state_label,
            answering_tier,
            result_count,
            index_result_count,
            text_result_count,
            source_backed,
            latency_us,
        })
    }

    // ---------------------------------------------------------------------------
    // Aggregation helpers
    // ---------------------------------------------------------------------------

    fn percentile(sorted_us: &[u64], pct: f64) -> u64 {
        if sorted_us.is_empty() {
            return 0;
        }
        let idx = ((pct / 100.0) * (sorted_us.len() as f64 - 1.0)).round() as usize;
        sorted_us[idx.min(sorted_us.len() - 1)]
    }

    fn print_scorecard(rows: &[Row]) {
        eprintln!();
        eprintln!("=== References Tier-Share Scorecard ===");
        eprintln!();

        // Tier distribution
        let mut tier_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for row in rows {
            *tier_counts.entry(row.answering_tier.as_str()).or_default() += 1;
        }
        let total = rows.len();
        eprintln!("--- Tier Distribution ({total} requests) ---");
        let mut tier_vec: Vec<_> = tier_counts.iter().collect();
        tier_vec.sort_by_key(|(k, _)| *k);
        for (tier, count) in &tier_vec {
            let pct = 100.0 * (**count as f64) / (total as f64);
            eprintln!("  {tier:<30}  {count:>3} ({pct:5.1}%)");
        }
        eprintln!();

        // Tier × index_state matrix
        eprintln!("--- Tier × Index-State Matrix ---");
        let states = ["full", "building", "none"];
        print!("  {:<30}", "tier");
        for s in &states {
            print!("  {:>10}", s);
        }
        eprintln!();
        for (tier, _) in &tier_vec {
            print!("  {:<30}", tier);
            for state in &states {
                let count = rows
                    .iter()
                    .filter(|r| r.answering_tier.as_str() == **tier && r.index_state == *state)
                    .count();
                print!("  {:>10}", count);
            }
            eprintln!();
        }
        eprintln!();

        // Latency percentiles
        let mut latencies: Vec<u64> = rows.iter().map(|r| r.latency_us).collect();
        latencies.sort_unstable();
        let p50 = percentile(&latencies, 50.0);
        let p95 = percentile(&latencies, 95.0);
        let max = latencies.last().copied().unwrap_or(0);
        eprintln!("--- Latency (µs) ---");
        eprintln!("  p50={p50}  p95={p95}  max={max}");
        eprintln!();

        // Per-row detail
        eprintln!("--- Per-Request Detail ---");
        eprintln!(
            "  {:<12} {:<10} {:<26} {:>6} {:>8} {:>8} {:<8} {:>10}",
            "fixture", "idx_state", "tier", "total", "idx", "text", "src_bkd", "latency_us"
        );
        for row in rows {
            eprintln!(
                "  {:<12} {:<10} {:<26} {:>6} {:>8} {:>8} {:<8} {:>10}",
                row.fixture_id,
                row.index_state,
                row.answering_tier,
                row.result_count,
                row.index_result_count,
                row.text_result_count,
                if row.source_backed { "yes" } else { "no" },
                row.latency_us,
            );
        }
        eprintln!();
    }

    // ---------------------------------------------------------------------------
    // The scorecard test
    // ---------------------------------------------------------------------------

    #[test]
    fn references_tier_share_scorecard() -> Result<(), Box<dyn std::error::Error>> {
        let mut rows: Vec<Row> = Vec::new();
        let soft_latency_limit = Duration::from_secs(2);

        // --- Fixture 1: scalar used three times ($count) ---
        // Same fixture, iterated across three index states.
        let (scalar_line, scalar_character) = position_of(SCALAR_THREE_USES, "$count")?;
        let scalar_states: &[(&str, fn(&mut LspServer))] = &[
            ("full", set_index_ready),
            ("building", set_index_building),
            ("none", set_index_none),
        ];
        for (state_label, apply_state) in scalar_states {
            let mut server = create_server();
            apply_state(&mut server);
            open_document(&server, SCALAR_THREE_USES_URI, SCALAR_THREE_USES)?;
            rows.push(measure(
                "scalar",
                state_label,
                &server,
                SCALAR_THREE_USES_URI,
                scalar_line,
                scalar_character,
                false,
            )?);
        }

        // --- Fixture 2: sub with two call sites ---
        let (sub_line, sub_character) = position_of(SUB_TWO_CALLS, "calculate")?;
        let sub_states: &[(&str, fn(&mut LspServer))] = &[
            ("full", set_index_ready),
            ("building", set_index_building),
            ("none", set_index_none),
        ];
        for (state_label, apply_state) in sub_states {
            let mut server = create_server();
            apply_state(&mut server);
            open_document(&server, SUB_TWO_CALLS_URI, SUB_TWO_CALLS)?;
            rows.push(measure(
                "sub_calls",
                state_label,
                &server,
                SUB_TWO_CALLS_URI,
                sub_line,
                sub_character,
                true, // include_declaration = true
            )?);
        }

        // --- Fixture 3: package-qualified reference ---
        let (qual_line, qual_character) = position_of(QUALIFIED, "helper")?;
        let qual_states: &[(&str, fn(&mut LspServer))] =
            &[("full", set_index_ready), ("building", set_index_building)];
        for (state_label, apply_state) in qual_states {
            let mut server = create_server();
            apply_state(&mut server);
            open_document(&server, QUALIFIED_URI, QUALIFIED)?;
            rows.push(measure(
                "qualified",
                state_label,
                &server,
                QUALIFIED_URI,
                qual_line,
                qual_character,
                false,
            )?);
        }

        // --- Fixture 4: no-symbol position (whitespace) — expect empty tier ---
        {
            let mut server = create_server();
            set_index_ready(&mut server);
            open_document(&server, EMPTY_URI, EMPTY_DOC)?;
            // Line 3 is a blank line — no symbol.
            rows.push(measure("empty_pos", "full", &server, EMPTY_URI, 3, 0, false)?);
        }

        // --- Fixture 5: sub_calls with include_declaration=false (full state) ---
        {
            let mut server = create_server();
            set_index_ready(&mut server);
            open_document(&server, SUB_TWO_CALLS_URI, SUB_TWO_CALLS)?;
            let (line, character) = position_of(SUB_TWO_CALLS, "calculate")?;
            rows.push(measure(
                "sub_no_decl",
                "full",
                &server,
                SUB_TWO_CALLS_URI,
                line,
                character,
                false, // include_declaration = false
            )?);
        }

        // ---------------------------------------------------------------------------
        // Emit the scorecard
        // ---------------------------------------------------------------------------
        print_scorecard(&rows);

        // ---------------------------------------------------------------------------
        // Soft assertions
        // ---------------------------------------------------------------------------

        // No panics or timeouts above this line means we got here.
        // Soft assertion 1: at least one request reached a non-empty tier.
        let non_empty = rows.iter().filter(|r| r.answering_tier != "empty").count();
        assert!(
            non_empty > 0,
            "expected at least one non-empty tier in {total} requests",
            total = rows.len()
        );

        // Soft assertion 2: the blank-line case yields `empty`.
        let empty_row = rows.iter().find(|r| r.fixture_id == "empty_pos");
        if let Some(row) = empty_row {
            assert_eq!(
                row.answering_tier, "empty",
                "no-symbol position should yield `empty` tier, got `{}`",
                row.answering_tier
            );
        }

        // Soft assertion 3: all latencies under the generous threshold.
        for row in &rows {
            assert!(
                row.latency_us <= u64::try_from(soft_latency_limit.as_micros()).unwrap_or(u64::MAX),
                "latency {} µs exceeded soft limit for fixture={} state={}",
                row.latency_us,
                row.fixture_id,
                row.index_state
            );
        }

        Ok(())
    }
}
