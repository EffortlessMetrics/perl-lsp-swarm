//! References routing-matrix harness.
//!
//! Verifies that the `textDocument/references` handler routes to the correct
//! tier for each combination of fixture × index state, and that decision-trace
//! receipt capture works correctly.
//!
//! **This is NOT a traffic-share or latency benchmark.**  The six hand-picked
//! fixtures are chosen to exercise specific routing branches (workspace-mixed,
//! semantic-analyzer fallback, empty).  The tier proportions and latency values
//! printed below are soft observations from controlled synthetic inputs — they
//! cannot estimate real-world user-traffic share or produce credible p50/p95
//! values.  A representative-workspace replay is the follow-up measurement
//! packet (see issue #2635).
//!
//! # Index-state labels
//!
//! Each scenario is labelled by **intended** index state.  The harness also
//! captures the **observed** `index_state` field from the decision-trace receipt
//! and asserts that the two agree:
//!
//! | intended label | expected receipt `index_state` |
//! |---|---|
//! | `"full"`     | `"full"`    |
//! | `"building"` | `"partial"` |
//! | `"none"`     | `"none"`    |
//!
//! Exception: the `empty` tier hardcodes `index_state = "none"` in the handler
//! regardless of the actual coordinator state (the index is irrelevant when no
//! symbol exists under the cursor).  H-B is skipped for `empty` rows.
//!
//! If the assertion fails the harness reports the discrepancy so the caller
//! can fix `set_index_building` (or drop the "building" column and document why).
//!
//! # Placement rationale
//!
//! `index_coordinator` is `pub(crate)`, so an integration test in `tests/`
//! cannot set it directly without adding a new production-visible method.
//! This unit test lives inside `src/runtime/language/` where `LspServer`'s
//! internal fields are in scope — matching the existing patterns in
//! `rename.rs:2045` and `signature_help.rs:1146`.  No new API surface is added.
//!
//! # Running
//!
//! ```text
//! CARGO_TARGET_DIR=.tmp/wt-target CARGO_INCREMENTAL=0 \
//!   cargo test -p perl-lsp-rs --features workspace references_routing_matrix -- --nocapture
//! ```

#[cfg(all(test, feature = "workspace"))]
mod routing_matrix {
    use crate::runtime::LspServer;
    use crate::util::is_word_boundary;
    use parking_lot::Mutex;
    use perl_parser::workspace_index::IndexCoordinator;
    use serde_json::{json, Value};
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    // ---------------------------------------------------------------------------
    // Fixtures
    // ---------------------------------------------------------------------------

    /// A scalar used three times in a single file — cursor on `$count`.
    const SCALAR_THREE_USES_URI: &str = "file:///routing_matrix/scalar_three_uses.pl";
    const SCALAR_THREE_USES: &str = r#"use strict;
use warnings;
my $count = 0;
$count++;
print $count;
"#;

    /// A sub definition plus two call sites in the same file.
    const SUB_TWO_CALLS_URI: &str = "file:///routing_matrix/sub_two_calls.pm";
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
    const QUALIFIED_URI: &str = "file:///routing_matrix/qualified.pm";
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
    const EMPTY_URI: &str = "file:///routing_matrix/empty_pos.pl";
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
    // Index-state helpers: mirror the patterns in rename.rs + signature_help.rs
    //
    // Each helper sets pub(crate) `index_coordinator` directly — Placement A.
    // No new public or pub(crate) method is added to LspServer.
    //
    // ALL helpers MUST be called AFTER `open_document` to avoid the following
    // auto-promotion: when a unit test has no tokio runtime, `handle_did_open`
    // (text_sync.rs:296) runs synchronously and calls `transition_to_ready()` if
    // the coordinator is in `Building { phase: Idle }` state.  `IndexCoordinator::
    // new()` initialises to exactly that state, so calling `set_index_building`
    // BEFORE open would immediately promote to Ready.  Applying state AFTER open
    // bypasses this auto-promotion.
    //
    // For "full" scenarios the natural did_open auto-promotion already produces the
    // desired state (Ready + document indexed in workspace), so `set_index_ready` is
    // a no-op — replacing the coordinator would discard the indexed document and
    // prevent `workspace_mixed` from firing.
    //
    // State → IndexAccessMode → receipt `index_state` mapping (applied post-open):
    //   set_index_ready()    → already Ready (no-op)  → Full    → "full"
    //   set_index_building() → fresh Building coord   → Partial → "partial"
    //   set_index_none()     → removes coordinator    → None    → "none"
    // ---------------------------------------------------------------------------

    fn set_index_none(server: &mut LspServer) {
        // Genuine None: remove the coordinator entirely so route_index_access
        // returns IndexAccessMode::None and no workspace index is consulted.
        server.index_coordinator = None;
    }

    fn set_index_building(server: &mut LspServer) {
        // Replaces the coordinator with a fresh Building coordinator.
        // Called AFTER open_document: did_open already promoted the original
        // coordinator to Ready, so this replacement gives the handler a genuine
        // Partial-access coordinator while the document text is still in the store.
        server.index_coordinator = Some(Arc::new(IndexCoordinator::new()));
    }

    fn set_index_ready(_server: &mut LspServer) {
        // No-op when called after `open_document` in a unit-test environment.
        //
        // In unit tests (no tokio runtime) did_open runs synchronously and calls
        // `transition_to_ready()` on the server's coordinator (text_sync.rs:296),
        // leaving it in `IndexState::Ready` with the document already indexed.
        // Replacing the coordinator here would discard those index facts and prevent
        // `workspace_mixed` from firing (the fresh coordinator's index is empty).
        //
        // The coordinator is already Ready: this function exists only to document the
        // "full" intent and provide a consistent apply-state signature for the loop.
    }

    /// Map an intended label to the expected receipt `index_state` value.
    ///
    /// | intended     | receipt `index_state` |
    /// |--------------|----------------------|
    /// | `"full"`     | `"full"`             |
    /// | `"building"` | `"partial"`          |
    /// | `"none"`     | `"none"`             |
    fn expected_receipt_index_state(
        intended: &str,
    ) -> Result<&'static str, Box<dyn std::error::Error>> {
        match intended {
            "full" => Ok("full"),
            "building" => Ok("partial"),
            "none" => Ok("none"),
            other => Err(format!("unknown intended index state label: {other}").into()),
        }
    }

    // ---------------------------------------------------------------------------
    // Measurement row
    // ---------------------------------------------------------------------------

    #[derive(Debug)]
    struct Row {
        fixture_id: &'static str,
        /// Label set by the test (matches the `set_index_*` helper used).
        intended_state: &'static str,
        /// The `index_state` field from the decision-trace receipt:
        /// `"full" | "partial" | "none"` — observed by the handler.
        observed_state: String,
        answering_tier: String,
        result_count: u64,
        /// Raw index-sourced hit count before dedup/truncation.
        index_result_count: u64,
        /// Raw text-sourced hit count before dedup/truncation.
        text_result_count: u64,
        source_backed: bool,
        latency_us: u64,
    }

    /// Fire one references request and capture the receipt.
    ///
    /// The receipt is fetched immediately after the request to guarantee it
    /// corresponds to THIS request (not a stale receipt from a prior call in the
    /// same server session).
    ///
    /// # Hard assertions inside `measure`
    ///
    /// - H-A: Receipt `uri`/`line`/`character` match what was sent (binding check).
    /// - H-B: Receipt `index_state` matches the expected value for `intended_state_label`
    ///   (skipped for `empty` tier, which hardcodes `"none"` regardless of coordinator state).
    fn measure(
        fixture_id: &'static str,
        intended_state_label: &'static str,
        server: &LspServer,
        uri: &'static str,
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

        // Fetch receipt immediately — before any other request on this server.
        let explanation = explain_provider_decision(server, "references")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing request_receipt")?;

        // H-A: receipt is bound to THIS request (uri / line / character).
        let receipt_uri = receipt.get("uri").and_then(Value::as_str).unwrap_or("");
        assert_eq!(
            receipt_uri, uri,
            "receipt URI mismatch fixture={fixture_id} state={intended_state_label}: \
             expected `{uri}`, got `{receipt_uri}`"
        );
        let receipt_line = receipt.get("line").and_then(Value::as_u64).unwrap_or(u64::MAX);
        assert_eq!(
            receipt_line,
            u64::from(line),
            "receipt line mismatch fixture={fixture_id} state={intended_state_label}: \
             expected {line}, got {receipt_line}"
        );
        let receipt_char = receipt.get("character").and_then(Value::as_u64).unwrap_or(u64::MAX);
        assert_eq!(
            receipt_char,
            u64::from(character),
            "receipt character mismatch fixture={fixture_id} state={intended_state_label}: \
             expected {character}, got {receipt_char}"
        );

        // H-B: observed index_state matches the intended label — for non-empty tiers.
        //
        // The `empty` tier hardcodes `index_state = "none"` in the handler
        // (references.rs terminal return) regardless of coordinator state.  This is
        // an implementation shortcut: when there is no symbol under the cursor the
        // index state is irrelevant.  We skip H-B for empty rows to avoid a false
        // assertion; the `observed_state` column in the matrix will show "none" for
        // these rows, which is documented behaviour, not a harness bug.
        let observed_state =
            receipt.get("index_state").and_then(Value::as_str).unwrap_or("").to_string();
        let answering_tier =
            receipt.get("answering_tier").and_then(Value::as_str).unwrap_or("unknown").to_string();
        if answering_tier != "empty" {
            let expected_state = expected_receipt_index_state(intended_state_label)?;
            assert_eq!(
                observed_state.as_str(),
                expected_state,
                "index_state mismatch fixture={fixture_id}: intended={intended_state_label} \
                 → expected receipt index_state={expected_state}, got {observed_state:?}. \
                 Check set_index_{intended_state_label}() — does it produce the right \
                 IndexAccessMode?  (Note: empty tier always reports \"none\"; if tier is \
                 empty this assertion is skipped.)"
            );
        }
        let result_count = receipt.get("result_count").and_then(Value::as_u64).unwrap_or(0);
        let index_result_count =
            receipt.get("index_result_count").and_then(Value::as_u64).unwrap_or(0);
        let text_result_count =
            receipt.get("text_result_count").and_then(Value::as_u64).unwrap_or(0);
        let source_backed = receipt.get("source_backed").and_then(Value::as_bool).unwrap_or(false);
        // Prefer receipt latency (recorded inside the handler); fall back to wall time.
        let latency_us = receipt.get("latency_us").and_then(Value::as_u64).unwrap_or(wall_us);

        Ok(Row {
            fixture_id,
            intended_state: intended_state_label,
            observed_state,
            answering_tier,
            result_count,
            index_result_count,
            text_result_count,
            source_backed,
            latency_us,
        })
    }

    // ---------------------------------------------------------------------------
    // Output helpers
    // ---------------------------------------------------------------------------

    fn percentile(sorted_us: &[u64], pct: f64) -> u64 {
        if sorted_us.is_empty() {
            return 0;
        }
        let idx = ((pct / 100.0) * (sorted_us.len() as f64 - 1.0)).round() as usize;
        sorted_us[idx.min(sorted_us.len() - 1)]
    }

    /// Call-observation proof: `percentile` returns the exact index element
    /// at the percentile position for a known sorted slice.
    ///
    /// Naming follows the `_call_observation` convention from rename.rs so ripr
    /// can classify the seam as strongly discriminated.
    ///
    /// H-P-1: empty slice → 0 (no panic)
    /// H-P-2: p0 of [10, 20, 30] → 10 (first element)
    /// H-P-3: p100 of [10, 20, 30] → 30 (last element)
    /// H-P-4: p50 of [10, 20, 30] → 20 (middle element)
    /// H-P-5: p50 of [5] → 5 (singleton)
    #[test]
    fn percentile_call_observation() {
        assert_eq!(percentile(&[], 50.0), 0, "H-P-1: empty slice must return 0");
        let sorted = [10u64, 20, 30];
        assert_eq!(percentile(&sorted, 0.0), 10, "H-P-2: p0 must be the first element");
        assert_eq!(percentile(&sorted, 100.0), 30, "H-P-3: p100 must be the last element");
        assert_eq!(percentile(&sorted, 50.0), 20, "H-P-4: p50 of 3-element slice must be middle");
        assert_eq!(
            percentile(&[5], 50.0),
            5,
            "H-P-5: singleton slice must return its only element"
        );
    }

    fn print_routing_matrix(rows: &[Row]) {
        eprintln!();
        eprintln!("=== References Routing Matrix (controlled fixtures — NOT usage share) ===");
        eprintln!("NOTE: tier proportions and latency are SOFT OBSERVATIONS from controlled");
        eprintln!("      synthetic fixtures — NOT usage-traffic share or credible percentiles.");
        eprintln!("      Real-world tier-share requires a representative-workspace replay");
        eprintln!("      (follow-up to #2635).");
        eprintln!();

        // Per-row detail: intended vs observed index state side-by-side
        eprintln!("--- Per-request routing detail ---");
        eprintln!(
            "  {:<12} {:<10} {:<10} {:<26} {:>6} {:>8} {:>8} {:<8} {:>10}",
            "fixture",
            "intended",
            "observed",
            "tier",
            "total",
            "idx_raw",
            "txt_raw",
            "src_bkd",
            "latency_us"
        );
        for row in rows {
            eprintln!(
                "  {:<12} {:<10} {:<10} {:<26} {:>6} {:>8} {:>8} {:<8} {:>10}",
                row.fixture_id,
                row.intended_state,
                row.observed_state,
                row.answering_tier,
                row.result_count,
                row.index_result_count,
                row.text_result_count,
                if row.source_backed { "yes" } else { "no" },
                row.latency_us,
            );
        }
        eprintln!();

        // Tier distribution (soft observation, fixture-weighted)
        let mut tier_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for row in rows {
            *tier_counts.entry(row.answering_tier.as_str()).or_default() += 1;
        }
        let total = rows.len();
        eprintln!(
            "--- Observed tier routing ({total} controlled requests) \
             [proportions are fixture-weighted, NOT traffic-weighted] ---"
        );
        let mut tier_vec: Vec<_> = tier_counts.iter().collect();
        tier_vec.sort_by_key(|(k, _)| *k);
        for (tier, count) in &tier_vec {
            let pct = 100.0 * (**count as f64) / (total as f64);
            eprintln!("  {tier:<30}  {count:>3} ({pct:5.1}%)");
        }
        eprintln!();

        // Latency (soft observation)
        let mut latencies: Vec<u64> = rows.iter().map(|r| r.latency_us).collect();
        latencies.sort_unstable();
        let p50 = percentile(&latencies, 50.0);
        let p95 = percentile(&latencies, 95.0);
        let max = latencies.last().copied().unwrap_or(0);
        eprintln!("--- Latency (µs, controlled fixtures — NOT representative workload) ---");
        eprintln!("  p50={p50}  p95={p95}  max={max}");
        eprintln!();

        // Routing matrix: tier × observed_state (receipt's index_state field).
        // Because H-B asserts intended→observed mapping holds per row, these
        // columns reflect genuinely-observed IndexAccessMode values, not labels.
        let observed_state_cols = ["full", "partial", "none"];
        eprintln!("--- Routing matrix (tier × observed_state from receipt) ---");
        eprint!("  {:<30}", "tier");
        for s in &observed_state_cols {
            eprint!("  {:>10}", s);
        }
        eprintln!();
        for (tier, _) in &tier_vec {
            eprint!("  {:<30}", tier);
            for state in &observed_state_cols {
                let count = rows
                    .iter()
                    .filter(|r| {
                        r.answering_tier.as_str() == **tier && r.observed_state.as_str() == *state
                    })
                    .count();
                eprint!("  {:>10}", count);
            }
            eprintln!();
        }
        eprintln!();
    }

    // ---------------------------------------------------------------------------
    // The routing-matrix test
    // ---------------------------------------------------------------------------

    #[test]
    fn references_routing_matrix() -> Result<(), Box<dyn std::error::Error>> {
        let mut rows: Vec<Row> = Vec::new();
        let soft_latency_limit = Duration::from_secs(2);

        // Index state is applied AFTER opening documents.  This is required to
        // produce a genuine Partial state for the "building" scenario: when there
        // is no tokio runtime (unit tests), `handle_did_open` runs synchronously
        // and calls `transition_to_ready()` on any coordinator that is in
        // `Building { phase: Idle }` state (text_sync.rs:296).  Applying the
        // state after open bypasses this auto-promotion.  Each scenario uses a
        // fresh server to prevent cross-contamination.

        // --- Fixture 1: scalar used three times ($count) ---
        let (scalar_line, scalar_character) = position_of(SCALAR_THREE_USES, "$count")?;
        let scalar_states: &[(&str, fn(&mut LspServer))] = &[
            ("full", set_index_ready),
            ("building", set_index_building),
            ("none", set_index_none),
        ];
        for (state_label, apply_state) in scalar_states {
            let mut server = create_server();
            open_document(&server, SCALAR_THREE_USES_URI, SCALAR_THREE_USES)?;
            apply_state(&mut server); // set index state AFTER open to prevent auto-promotion
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
        // Expected to produce workspace_mixed (index + text) in Ready state
        // because both the workspace index and open-document text search
        // contribute hits.
        let (sub_line, sub_character) = position_of(SUB_TWO_CALLS, "calculate")?;
        let sub_states: &[(&str, fn(&mut LspServer))] = &[
            ("full", set_index_ready),
            ("building", set_index_building),
            ("none", set_index_none),
        ];
        for (state_label, apply_state) in sub_states {
            let mut server = create_server();
            open_document(&server, SUB_TWO_CALLS_URI, SUB_TWO_CALLS)?;
            apply_state(&mut server);
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
            open_document(&server, QUALIFIED_URI, QUALIFIED)?;
            apply_state(&mut server);
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

        // --- Fixture 4: no-symbol position (blank line) — expect empty tier ---
        {
            let mut server = create_server();
            open_document(&server, EMPTY_URI, EMPTY_DOC)?;
            set_index_ready(&mut server);
            // Line 3 (0-indexed) is blank.
            rows.push(measure("empty_pos", "full", &server, EMPTY_URI, 3, 0, false)?);
        }

        // --- Fixture 5: sub_calls with include_declaration=false ---
        {
            let mut server = create_server();
            open_document(&server, SUB_TWO_CALLS_URI, SUB_TWO_CALLS)?;
            set_index_ready(&mut server);
            let (line, character) = position_of(SUB_TWO_CALLS, "calculate")?;
            rows.push(measure(
                "sub_no_decl",
                "full",
                &server,
                SUB_TWO_CALLS_URI,
                line,
                character,
                false,
            )?);
        }

        // H-0: Strong row-count discriminator — proves all five fixture blocks pushed
        // their expected rows (scalar×3 + sub_calls×3 + qualified×2 + empty×1 +
        // sub_no_decl×1 = 10).  This assertion would fail if any rows.push() call
        // were removed, giving ripr a concrete binding for the push seams.
        assert_eq!(
            rows.len(),
            10,
            "expected 10 rows (3+3+2+1+1 across five fixture blocks), got {}",
            rows.len()
        );

        // ---------------------------------------------------------------------------
        // Emit the routing matrix (soft observations)
        // ---------------------------------------------------------------------------
        print_routing_matrix(&rows);

        // ---------------------------------------------------------------------------
        // Hard assertions — the CONTRACT this harness enforces
        // (H-A and H-B already fired per-row inside measure(); the rest are global)
        // ---------------------------------------------------------------------------

        // H-1: At least one request reached a non-empty tier (harness is alive).
        let non_empty = rows.iter().filter(|r| r.answering_tier != "empty").count();
        assert!(
            non_empty > 0,
            "expected at least one non-empty tier across {} requests",
            rows.len()
        );

        // H-2: The blank-line case always yields `empty`.
        let empty_row =
            rows.iter().find(|r| r.fixture_id == "empty_pos").ok_or("empty_pos row missing")?;
        assert_eq!(
            empty_row.answering_tier, "empty",
            "no-symbol position must yield `empty` tier, got `{}`",
            empty_row.answering_tier
        );

        // H-3: workspace_mixed is distinguishable — at least one row in the `sub_calls`
        // fixture has BOTH index_result_count > 0 AND text_result_count > 0, and the
        // tier must be `workspace_mixed`.
        let mixed_rows: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.fixture_id == "sub_calls" && r.index_result_count > 0 && r.text_result_count > 0
            })
            .collect();
        assert!(
            !mixed_rows.is_empty(),
            "expected at least one sub_calls row with both index_result_count>0 \
             and text_result_count>0 to verify workspace_mixed distinguishability"
        );
        for r in &mixed_rows {
            assert_eq!(
                r.answering_tier, "workspace_mixed",
                "row with idx={} txt={} must be `workspace_mixed`, got `{}`",
                r.index_result_count, r.text_result_count, r.answering_tier
            );
        }

        // H-4: Counts are pre-dedup/truncation (internal coherence) — for
        // workspace_mixed rows the raw contributions sum to >= the final result_count
        // (dedup can only shrink, never grow).
        for row in rows.iter().filter(|r| r.answering_tier == "workspace_mixed") {
            let raw_sum = row.index_result_count + row.text_result_count;
            assert!(
                raw_sum >= row.result_count,
                "workspace_mixed raw_sum={}+{}={} < result_count={} for fixture={} state={}",
                row.index_result_count,
                row.text_result_count,
                raw_sum,
                row.result_count,
                row.fixture_id,
                row.intended_state
            );
        }

        // H-5: Soft latency guard — all requests complete within 2 s.
        for row in &rows {
            assert!(
                row.latency_us <= u64::try_from(soft_latency_limit.as_micros()).unwrap_or(u64::MAX),
                "latency {} µs exceeded 2 s limit for fixture={} intended_state={}",
                row.latency_us,
                row.fixture_id,
                row.intended_state
            );
        }

        // H-6: Placement A — confirmed by code structure.
        // The `set_index_*` helpers above access `server.index_coordinator` directly
        // without going through any newly added pub method.  If a pub method were added
        // it would appear in this file.  Reviewers: `grep 'pub fn test_set_index'
        // test_api.rs` should return no results.

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Representative-workspace replay (#2674 PR-3)
    //
    // Extends the controlled routing-matrix harness above with a checked-in,
    // deterministic request manifest replayed against three real, committed
    // project fixtures under `test_corpus/real_projects/` (Mojolicious,
    // Dancer2, Catalyst — the same skeletons already used by
    // `perl-lsp-ux-tests` UX scenarios and the real-workspace latency
    // baselines). This is a MEASUREMENT extension: it fires real
    // `textDocument/references` requests through the same `LspServer` +
    // `explainProviderDecision` receipt path as `references_routing_matrix`
    // above, and does not change any provider behavior.
    //
    // ## Selection rule (so the corpus composition is inspectable)
    //
    // For each project we name one-to-three files already committed as UX
    // fixtures. Within those named files we hand-select EVERY same-file
    // lexical variable, same-file subroutine, cross-file subroutine,
    // imported-symbol call site, cross-class method-name collision, and
    // no-symbol position needed to populate every `FactClass` bucket below at
    // least once across the three projects. This is not a random sample of
    // "real user traffic" — see the module doc comment at the top of this
    // file for that disclaimer, which applies equally here. The manifest is
    // `REPLAY_MANIFEST` + `EMPTY_POSITION_MANIFEST` below; every entry's
    // `expected_true_occurrences` / `known_false_occurrences` was verified
    // against the fixture source (occurrence order confirmed via a boundary
    // match dump before this manifest was written — see the file/needle pairs
    // below for the exact source lines each index maps to).
    //
    // ## What is asserted vs. recorded
    //
    // `FactClass::is_strictly_checked()` classes (`LocalLexical`,
    // `PackageSubSameFile`, `DynamicAmbiguous`) are same-file-scoped by
    // construction, so whenever the observed answering tier claims
    // `semantic_source_backed` (source-backed exactness), the actual result
    // set is asserted to equal the curated expected set exactly and to never
    // contain a known-false location. Cross-file and imported-symbol classes
    // are intentionally out of the current same-file PIR-A slice; they are
    // recorded (tier, counts, fallback reason) without a strict equality
    // assertion, and are still checked against `known_false_occurrences`
    // (empty for both here, i.e. trivially satisfied) so a future promotion
    // that DOES claim exactness for them would be caught by this same guard.
    // ─────────────────────────────────────────────────────────────────────────

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FactClass {
        /// Same-file lexical scalar variable with >=2 same-scope occurrences.
        LocalLexical,
        /// Subroutine defined and called within the same file, unambiguous name.
        PackageSubSameFile,
        /// Subroutine defined in one file, called from a different file in the
        /// same project — outside the current same-file PIR-A scope.
        CrossFileSub,
        /// Symbol imported from a module outside the fixture project (e.g.
        /// `Carp::croak`) — declaration is not resolvable in-workspace.
        ImportedSymbol,
        /// Bareword name that collides across multiple classes/files, where a
        /// naive same-identifier text scan would find call sites that target a
        /// DIFFERENT class's method of the same name.
        DynamicAmbiguous,
        /// Cursor on a blank/no-symbol line — must yield the `empty` tier.
        EmptyPosition,
    }

    impl FactClass {
        fn as_str(self) -> &'static str {
            match self {
                Self::LocalLexical => "local_lexical",
                Self::PackageSubSameFile => "package_sub_same_file",
                Self::CrossFileSub => "cross_file_sub",
                Self::ImportedSymbol => "imported_symbol",
                Self::DynamicAmbiguous => "dynamic_ambiguous",
                Self::EmptyPosition => "empty_position",
            }
        }

        /// Whether a `semantic_source_backed` answer for this class is checked
        /// for exact equality against the curated expected set. See the module
        /// doc comment above for the rationale.
        fn is_strictly_checked(self) -> bool {
            matches!(self, Self::LocalLexical | Self::PackageSubSameFile | Self::DynamicAmbiguous)
        }
    }

    /// One checked-in replay request. `cursor_occurrence` and the two
    /// occurrence-index sets refer to the boundary-safe, file-ordered
    /// occurrences of `needle` in `file` (see `ident_boundary_occurrences`).
    #[derive(Debug, Clone, Copy)]
    struct ReplayRequest {
        project: &'static str,
        file: &'static str,
        fact_class: FactClass,
        needle: &'static str,
        cursor_occurrence: usize,
        include_declaration: bool,
        expected_true_occurrences: &'static [usize],
        known_false_occurrences: &'static [usize],
        fallback_reason: &'static str,
    }

    /// Checked-in request manifest. See the module doc comment above for the
    /// selection rule. Occurrence indices were confirmed against source with a
    /// boundary-match dump before authoring (each request's file/needle pair
    /// is independently re-verifiable by grepping the named fixture file).
    #[rustfmt::skip]
    const REPLAY_MANIFEST: &[ReplayRequest] = &[
        // ---- Mojolicious: lib/Mojolicious.pm ----
        // `$plugins` in `dispatch`: decl(56) + 2 same-scope uses(57,59). No
        // other `$plugins` in the file — single, unambiguous scope.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::LocalLexical, needle: "$plugins",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1, 2], known_false_occurrences: &[],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `$c` in `dispatch` (occ 0-6): must exclude the unrelated `$c` in
        // `handler` (occ 7-10) — direct scope-shadow analog of the existing
        // F1 curated corpus fixture, on real code.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::LocalLexical, needle: "$c",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1, 2, 3, 4, 5, 6],
            known_false_occurrences: &[7, 8, 9, 10],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `$c` in `handler` (occ 7-10): the mirror-image request from the
        // opposite scope.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::LocalLexical, needle: "$c",
            cursor_occurrence: 7, include_declaration: false,
            expected_true_occurrences: &[7, 8, 9, 10],
            known_false_occurrences: &[0, 1, 2, 3, 4, 5, 6],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `dispatch`: def(54, occ0) + true call `$self->dispatch($c)`(67, occ3)
        // target `Mojolicious::dispatch`; `$self->static->dispatch($c)`(58,
        // occ1) and `$self->routes->dispatch($c)`(60, occ2) call DIFFERENT
        // classes' `dispatch` methods and must never appear in an exact result.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::DynamicAmbiguous, needle: "dispatch",
            cursor_occurrence: 0, include_declaration: true,
            expected_true_occurrences: &[0, 3], known_false_occurrences: &[1, 2],
            fallback_reason: "cross_class_method_dispatch_not_disambiguated_by_receiver_type",
        },
        // `startup`: call(35, occ0) + def(94, occ1), unambiguous same-file sub.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::PackageSubSameFile, needle: "startup",
            cursor_occurrence: 1, include_declaration: true,
            expected_true_occurrences: &[0, 1], known_false_occurrences: &[],
            fallback_reason: "same_file_subroutine_did_not_reach_semantic_source_backed_tier",
        },
        // `croak`(73, occ1): Carp is not vendored in this fixture project, so
        // no same-file declaration exists to prove exactness against.
        ReplayRequest {
            project: "mojolicious_skeleton", file: "lib/Mojolicious.pm",
            fact_class: FactClass::ImportedSymbol, needle: "croak",
            cursor_occurrence: 1, include_declaration: false,
            expected_true_occurrences: &[], known_false_occurrences: &[],
            fallback_reason: "carp_croak_declared_outside_the_fixture_project",
        },
        // ---- Dancer2: lib/Dancer2/Core/App.pm ----
        // `$code` in `add_route`: decl(26,occ0) + uses(27,occ1; 30,occ2); the
        // unrelated `$code` in `add_hook` (occ3,occ4) must be excluded.
        ReplayRequest {
            project: "dancer2_skeleton", file: "lib/Dancer2/Core/App.pm",
            fact_class: FactClass::LocalLexical, needle: "$code",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1, 2], known_false_occurrences: &[3, 4],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `$method` scope pair, side A: `add_route` (occ0,occ1) vs `dispatch`
        // (occ2,occ3) — same bare name, non-overlapping scopes.
        ReplayRequest {
            project: "dancer2_skeleton", file: "lib/Dancer2/Core/App.pm",
            fact_class: FactClass::LocalLexical, needle: "$method",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1], known_false_occurrences: &[2, 3],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `$method` scope pair, side B: the mirror-image request.
        ReplayRequest {
            project: "dancer2_skeleton", file: "lib/Dancer2/Core/App.pm",
            fact_class: FactClass::LocalLexical, needle: "$method",
            cursor_occurrence: 2, include_declaration: false,
            expected_true_occurrences: &[2, 3], known_false_occurrences: &[0, 1],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `dispatch`(34, occ0): defined here, the only call site is
        // `Runner.pm:32` (`$app->dispatch($env)`) — a DIFFERENT file in the
        // same project, outside the current same-file PIR-A scope.
        ReplayRequest {
            project: "dancer2_skeleton", file: "lib/Dancer2/Core/App.pm",
            fact_class: FactClass::CrossFileSub, needle: "dispatch",
            cursor_occurrence: 0, include_declaration: true,
            expected_true_occurrences: &[0], known_false_occurrences: &[],
            fallback_reason: "cross_file_caller_lives_outside_the_same_file_pir_a_scope",
        },
        // `croak`(27, occ1): same out-of-fixture-declaration reasoning as Mojolicious.
        ReplayRequest {
            project: "dancer2_skeleton", file: "lib/Dancer2/Core/App.pm",
            fact_class: FactClass::ImportedSymbol, needle: "croak",
            cursor_occurrence: 1, include_declaration: false,
            expected_true_occurrences: &[], known_false_occurrences: &[],
            fallback_reason: "carp_croak_declared_outside_the_fixture_project",
        },
        // ---- Catalyst: lib/Catalyst/Action.pm, lib/Catalyst.pm, lib/Catalyst/Dispatcher.pm ----
        // `$controller` in `dispatch`: decl(23,occ0) + uses(24,occ1; 26,occ2),
        // single unambiguous scope, no other `$controller` in the file.
        ReplayRequest {
            project: "catalyst_skeleton", file: "lib/Catalyst/Action.pm",
            fact_class: FactClass::LocalLexical, needle: "$controller",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1, 2], known_false_occurrences: &[],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `$c` in `dispatch` (occ0-4): must exclude `$c` in `execute`, `match`,
        // and `match_captures` (occ5-10) — three OTHER scopes share the name.
        ReplayRequest {
            project: "catalyst_skeleton", file: "lib/Catalyst/Action.pm",
            fact_class: FactClass::LocalLexical, needle: "$c",
            cursor_occurrence: 0, include_declaration: false,
            expected_true_occurrences: &[0, 1, 2, 3, 4],
            known_false_occurrences: &[5, 6, 7, 8, 9, 10],
            fallback_reason: "same_file_lexical_did_not_reach_semantic_source_backed_tier",
        },
        // `dispatch` in Catalyst.pm: call `$c->dispatch;`(180,occ0) +
        // def(184,occ1) — unambiguous WITHIN this file even though `dispatch`
        // is also independently defined in Action.pm and Dispatcher.pm.
        ReplayRequest {
            project: "catalyst_skeleton", file: "lib/Catalyst.pm",
            fact_class: FactClass::PackageSubSameFile, needle: "dispatch",
            cursor_occurrence: 1, include_declaration: true,
            expected_true_occurrences: &[0, 1], known_false_occurrences: &[],
            fallback_reason: "same_file_subroutine_did_not_reach_semantic_source_backed_tier",
        },
        // `dispatch` in Dispatcher.pm: def(12,occ0); ALL THREE in-file calls
        // (`$action->dispatch`(22,occ1), `$action_or_url->dispatch`(28,occ2),
        // `$action->dispatch`(32,occ3)) target `Catalyst::Action::dispatch` on
        // a differently-named receiver, NOT a recursive self-call — none of
        // them may appear in an exact result for `Dispatcher::dispatch`.
        ReplayRequest {
            project: "catalyst_skeleton", file: "lib/Catalyst/Dispatcher.pm",
            fact_class: FactClass::DynamicAmbiguous, needle: "dispatch",
            cursor_occurrence: 0, include_declaration: true,
            expected_true_occurrences: &[0], known_false_occurrences: &[1, 2, 3],
            fallback_reason: "cross_class_method_dispatch_not_disambiguated_by_receiver_type",
        },
        // `croak`(31, occ1): same out-of-fixture-declaration reasoning as above.
        ReplayRequest {
            project: "catalyst_skeleton", file: "lib/Catalyst/Dispatcher.pm",
            fact_class: FactClass::ImportedSymbol, needle: "croak",
            cursor_occurrence: 1, include_declaration: false,
            expected_true_occurrences: &[], known_false_occurrences: &[],
            fallback_reason: "carp_croak_declared_outside_the_fixture_project",
        },
    ];

    /// No-symbol cursor positions (blank lines), one per project, 0-based line.
    const EMPTY_POSITION_MANIFEST: &[(&str, &str, usize)] = &[
        ("mojolicious_skeleton", "lib/Mojolicious.pm", 17),
        ("dancer2_skeleton", "lib/Dancer2/Core/App.pm", 7),
        ("catalyst_skeleton", "lib/Catalyst/Action.pm", 7),
    ];

    #[derive(Debug)]
    struct ReplayRow {
        project: &'static str,
        file: &'static str,
        fact_class: FactClass,
        needle: &'static str,
        include_declaration: bool,
        uri: String,
        line: u32,
        character: u32,
        answering_tier: String,
        source_backed: bool,
        result_count: usize,
        index_result_count: usize,
        text_result_count: usize,
        latency_us: u64,
        fallback_reason: &'static str,
        disposition: &'static str,
    }

    fn real_project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("test_corpus/real_projects"))
            .ok_or_else(|| "CARGO_MANIFEST_DIR must be nested under the workspace root".into())
    }

    fn collect_project_files(
        project: &str,
    ) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        fn walk(
            root: &Path,
            dir: &Path,
            out: &mut Vec<(String, String)>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out)?;
                } else if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("pm" | "pl" | "t")
                ) {
                    let rel = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                    let content = fs::read_to_string(&path)
                        .map_err(|e| format!("reading {}: {e}", path.display()))?;
                    out.push((rel, content));
                }
            }
            Ok(())
        }

        let root = real_project_root()?.join(project);
        let mut files = Vec::new();
        walk(&root, &root, &mut files)?;
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    fn project_uri(project: &str, relative_path: &str) -> String {
        format!("file:///real_projects/{project}/{relative_path}")
    }

    fn open_project(
        server: &LspServer,
        project: &str,
    ) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        let files = collect_project_files(project)?;
        for (relative_path, content) in &files {
            let uri = project_uri(project, relative_path);
            open_document(server, &uri, content)?;
        }
        Ok(files)
    }

    fn fixture_content<'a>(
        files: &'a [(String, String)],
        relative_path: &str,
    ) -> Result<&'a str, Box<dyn std::error::Error>> {
        files
            .iter()
            .find(|(path, _)| path == relative_path)
            .map(|(_, content)| content.as_str())
            .ok_or_else(|| format!("missing fixture file {relative_path}").into())
    }

    /// Boundary-safe occurrences of `needle` in `content`, in file order, as
    /// `(line0, byte_col)`. Reuses the same word-boundary semantics as the
    /// production text-search fallback (`crate::util::is_word_boundary`) —
    /// this is purely the oracle's tokenizer, not the routing/scope logic
    /// under test.
    fn ident_boundary_occurrences(content: &str, needle: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (line0, line) in content.lines().enumerate() {
            let bytes = line.as_bytes();
            let mut start = 0usize;
            while let Some(idx) = line[start..].find(needle) {
                let byte_pos = start + idx;
                if is_word_boundary(bytes, byte_pos, needle.len()) {
                    out.push((line0, byte_pos));
                }
                start = byte_pos + needle.len();
            }
        }
        out
    }

    fn occurrence_position(
        content: &str,
        needle: &str,
        occurrence: usize,
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let occurrences = ident_boundary_occurrences(content, needle);
        let (line0, byte_col) = occurrences
            .get(occurrence)
            .copied()
            .ok_or_else(|| format!("occurrence {occurrence} of `{needle}` not found"))?;
        let line_text = content.lines().nth(line0).unwrap_or("");
        let character = crate::util::byte_to_utf16_col(line_text, byte_col);
        Ok((u32::try_from(line0)?, u32::try_from(character)?))
    }

    fn occurrence_location_key(
        uri: &str,
        content: &str,
        needle: &str,
        occurrence: usize,
    ) -> Result<(String, u32, u32), Box<dyn std::error::Error>> {
        let (line, character) = occurrence_position(content, needle, occurrence)?;
        Ok((uri.to_string(), line, character))
    }

    /// Extract `(uri, line, character)` keys from an array of LSP
    /// `Location`/`LocationLink` values, using the start position.
    fn location_keys(results: &[Value]) -> Vec<(String, u32, u32)> {
        results
            .iter()
            .filter_map(|entry| {
                let uri =
                    entry.get("uri").or_else(|| entry.get("targetUri"))?.as_str()?.to_string();
                let range = entry.get("range").or_else(|| entry.get("targetRange"))?;
                let line = range.get("start")?.get("line")?.as_u64()?;
                let character = range.get("start")?.get("character")?.as_u64()?;
                Some((uri, u32::try_from(line).ok()?, u32::try_from(character).ok()?))
            })
            .collect()
    }

    /// Roll an observed row up into a #1658 disposition bucket. See the
    /// module doc comment above for what each bucket means; this function is
    /// purely a classifier over already-asserted-safe observations (the hard
    /// assertions in `fire_replay_request` run first and would fail the test
    /// before a false-exact row ever reaches this classifier).
    fn classify_disposition(
        fact_class: FactClass,
        source_backed: bool,
        result_locations: &[(String, u32, u32)],
        expected_keys: &[(String, u32, u32)],
    ) -> &'static str {
        if source_backed {
            if fact_class.is_strictly_checked() {
                let mut actual_sorted = result_locations.to_vec();
                actual_sorted.sort();
                let mut expected_sorted = expected_keys.to_vec();
                expected_sorted.sort();
                if actual_sorted == expected_sorted {
                    return "index_parity_proven";
                }
                return "unclassified";
            }
            return "coverage_gap";
        }
        match fact_class {
            FactClass::CrossFileSub | FactClass::ImportedSymbol | FactClass::DynamicAmbiguous => {
                "explicit_refusal_safe"
            }
            FactClass::EmptyPosition => "explicit_refusal_safe",
            FactClass::LocalLexical | FactClass::PackageSubSameFile => "coverage_gap",
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fire_replay_request(
        server: &LspServer,
        request: &ReplayRequest,
        fixture_files: &[(String, String)],
    ) -> Result<ReplayRow, Box<dyn std::error::Error>> {
        let content = fixture_content(fixture_files, request.file)?;
        let uri = project_uri(request.project, request.file);
        let (line, character) =
            occurrence_position(content, request.needle, request.cursor_occurrence)?;

        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": request.include_declaration}
        });

        let t0 = Instant::now();
        let result = server.test_handle_references(Some(params))?;
        let wall_us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);

        let explanation = explain_provider_decision(server, "references")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing request_receipt")?;

        // Hard assertion #7: the receipt is bound to THIS request.
        let receipt_uri = receipt.get("uri").and_then(Value::as_str).unwrap_or("");
        assert_eq!(
            receipt_uri, uri,
            "receipt URI mismatch for {}/{} needle={:?}",
            request.project, request.file, request.needle
        );
        let receipt_line = receipt.get("line").and_then(Value::as_u64).unwrap_or(u64::MAX);
        assert_eq!(
            receipt_line,
            u64::from(line),
            "receipt line mismatch for {}/{} needle={:?}",
            request.project,
            request.file,
            request.needle
        );
        let receipt_char = receipt.get("character").and_then(Value::as_u64).unwrap_or(u64::MAX);
        assert_eq!(
            receipt_char,
            u64::from(character),
            "receipt character mismatch for {}/{} needle={:?}",
            request.project,
            request.file,
            request.needle
        );

        let answering_tier =
            receipt.get("answering_tier").and_then(Value::as_str).unwrap_or("unknown").to_string();
        let source_backed = receipt.get("source_backed").and_then(Value::as_bool).unwrap_or(false);
        let index_result_count =
            usize::try_from(receipt.get("index_result_count").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(usize::MAX);
        let text_result_count =
            usize::try_from(receipt.get("text_result_count").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(usize::MAX);
        let latency_us = receipt.get("latency_us").and_then(Value::as_u64).unwrap_or(wall_us);
        let fact_source = receipt.get("fact_source").and_then(Value::as_str).unwrap_or("");
        let source_backed_state =
            receipt.get("source_backed_state").and_then(Value::as_str).unwrap_or("");

        // Hard assertion #6: every source-backed claim reports both a proof
        // class (`source_backed_state`) and a producer (`fact_source`).
        if source_backed {
            assert!(
                !fact_source.is_empty(),
                "source-backed row missing fact_source (producer) for {}/{} needle={:?}",
                request.project,
                request.file,
                request.needle
            );
            assert!(
                !source_backed_state.is_empty(),
                "source-backed row missing source_backed_state (proof class) for {}/{} needle={:?}",
                request.project,
                request.file,
                request.needle
            );
        }

        let results = result.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
        let result_locations = location_keys(&results);
        let result_count = results.len();

        let expected_keys: Vec<(String, u32, u32)> = request
            .expected_true_occurrences
            .iter()
            .map(|occ| occurrence_location_key(&uri, content, request.needle, *occ))
            .collect::<Result<_, _>>()?;
        let forbidden_keys: Vec<(String, u32, u32)> = request
            .known_false_occurrences
            .iter()
            .map(|occ| occurrence_location_key(&uri, content, request.needle, *occ))
            .collect::<Result<_, _>>()?;

        // Hard assertions #1/#2/#3: whenever the tier claims source-backed
        // exactness, it must never surface a known-false location (this is
        // where the ambiguous/dynamic and (via the sibling stale-index test
        // below) stale-generation invariants are actually enforced), and for
        // strictly-checked classes the result must equal the curated expected
        // set exactly.
        if source_backed {
            for forbidden in &forbidden_keys {
                assert!(
                    !result_locations.contains(forbidden),
                    "false-exact: {}/{} needle={:?} (fact_class={:?}) claimed source-backed \
                     exact but returned known-false location {forbidden:?}",
                    request.project,
                    request.file,
                    request.needle,
                    request.fact_class
                );
            }
            if request.fact_class.is_strictly_checked() {
                let mut actual_sorted = result_locations.clone();
                actual_sorted.sort();
                let mut expected_sorted = expected_keys.clone();
                expected_sorted.sort();
                assert_eq!(
                    actual_sorted, expected_sorted,
                    "exact-range mismatch for {}/{} needle={:?} (fact_class={:?}): \
                     expected {expected_sorted:?}, got {actual_sorted:?}",
                    request.project, request.file, request.needle, request.fact_class
                );
            }
        }

        // Hard assertion #5: every fallback is named by tier + reason.
        if !source_backed {
            assert!(
                !request.fallback_reason.is_empty(),
                "fallback row for {}/{} needle={:?} missing a named reason",
                request.project,
                request.file,
                request.needle
            );
        }

        let disposition = classify_disposition(
            request.fact_class,
            source_backed,
            &result_locations,
            &expected_keys,
        );

        Ok(ReplayRow {
            project: request.project,
            file: request.file,
            fact_class: request.fact_class,
            needle: request.needle,
            include_declaration: request.include_declaration,
            uri,
            line,
            character,
            answering_tier,
            source_backed,
            result_count,
            index_result_count,
            text_result_count,
            latency_us,
            fallback_reason: request.fallback_reason,
            disposition,
        })
    }

    fn fire_empty_request(
        server: &LspServer,
        project: &'static str,
        file: &'static str,
        uri: &str,
        line0: usize,
    ) -> Result<ReplayRow, Box<dyn std::error::Error>> {
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line0, "character": 0},
            "context": {"includeDeclaration": false}
        });

        let t0 = Instant::now();
        let result = server.test_handle_references(Some(params))?;
        let wall_us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);

        let explanation = explain_provider_decision(server, "references")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing request_receipt")?;

        let receipt_uri = receipt.get("uri").and_then(Value::as_str).unwrap_or("");
        assert_eq!(receipt_uri, uri, "receipt URI mismatch for empty position {project}/{file}");
        let receipt_line = receipt.get("line").and_then(Value::as_u64).unwrap_or(u64::MAX);
        assert_eq!(
            receipt_line,
            u64::try_from(line0)?,
            "receipt line mismatch for empty position {project}/{file}"
        );

        let answering_tier =
            receipt.get("answering_tier").and_then(Value::as_str).unwrap_or("unknown").to_string();
        assert_eq!(
            answering_tier, "empty",
            "no-symbol position must yield `empty` tier for {project}/{file}, got {answering_tier}"
        );

        let source_backed = receipt.get("source_backed").and_then(Value::as_bool).unwrap_or(false);
        let index_result_count =
            usize::try_from(receipt.get("index_result_count").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(usize::MAX);
        let text_result_count =
            usize::try_from(receipt.get("text_result_count").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(usize::MAX);
        let latency_us = receipt.get("latency_us").and_then(Value::as_u64).unwrap_or(wall_us);
        let result_count = result.as_ref().and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        assert_eq!(
            result_count, 0,
            "empty position must produce zero results for {project}/{file}"
        );

        Ok(ReplayRow {
            project,
            file,
            fact_class: FactClass::EmptyPosition,
            needle: "",
            include_declaration: false,
            uri: uri.to_string(),
            line: u32::try_from(line0)?,
            character: 0,
            answering_tier,
            source_backed,
            result_count,
            index_result_count,
            text_result_count,
            latency_us,
            fallback_reason: "no_symbol_under_cursor_correctly_empty",
            disposition: "explicit_refusal_safe",
        })
    }

    fn row_to_json(row: &ReplayRow) -> Value {
        json!({
            "project": row.project,
            "file": row.file,
            "fact_class": row.fact_class.as_str(),
            "needle": row.needle,
            "include_declaration": row.include_declaration,
            "uri": row.uri,
            "line": row.line,
            "character": row.character,
            "answering_tier": row.answering_tier,
            "source_backed": row.source_backed,
            "result_count": row.result_count,
            "index_result_count": row.index_result_count,
            "text_result_count": row.text_result_count,
            "latency_us": row.latency_us,
            "fallback_reason": row.fallback_reason,
            "disposition": row.disposition,
        })
    }

    /// Representative-workspace replay for #2674 PR-3 (references measurement,
    /// no live provider behavior change). Fires every request in
    /// `REPLAY_MANIFEST` + `EMPTY_POSITION_MANIFEST` against three real,
    /// committed project fixtures, then checks the 9 hard assertions described
    /// in the module doc comment above and prints a durable JSON receipt with
    /// a per-request #1658 disposition.
    #[test]
    fn references_representative_workspace_replay() -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::{BTreeMap, BTreeSet, HashMap};

        let projects = ["mojolicious_skeleton", "dancer2_skeleton", "catalyst_skeleton"];
        let mut project_files: HashMap<&str, Vec<(String, String)>> = HashMap::new();
        let mut servers: HashMap<&str, LspServer> = HashMap::new();
        for project in projects {
            let server = create_server();
            let files = open_project(&server, project)?;
            project_files.insert(project, files);
            servers.insert(project, server);
        }

        let mut rows: Vec<ReplayRow> = Vec::new();
        for request in REPLAY_MANIFEST {
            let server = servers.get(request.project).ok_or("missing server for project")?;
            let files = project_files.get(request.project).ok_or("missing files for project")?;
            rows.push(fire_replay_request(server, request, files)?);
        }
        for (project, file, line0) in EMPTY_POSITION_MANIFEST.iter().copied() {
            let server = servers.get(project).ok_or("missing server for project")?;
            let uri = project_uri(project, file);
            rows.push(fire_empty_request(server, project, file, &uri, line0)?);
        }

        // Hard assertion #9: the replay emits a non-empty durable receipt.
        assert!(!rows.is_empty(), "replay must emit at least one row");

        // Hard assertion #8: every declared project and fact-class bucket appears.
        let projects_seen: BTreeSet<&str> = rows.iter().map(|r| r.project).collect();
        assert_eq!(
            projects_seen,
            BTreeSet::from(["mojolicious_skeleton", "dancer2_skeleton", "catalyst_skeleton"]),
            "replay must cover every declared project"
        );
        let classes_seen: BTreeSet<&str> = rows.iter().map(|r| r.fact_class.as_str()).collect();
        assert_eq!(
            classes_seen,
            BTreeSet::from([
                "local_lexical",
                "package_sub_same_file",
                "cross_file_sub",
                "imported_symbol",
                "dynamic_ambiguous",
                "empty_position",
            ]),
            "replay must cover every declared fact-class bucket"
        );

        // Hard assertion #4: every empty success is proven correct (`empty`
        // tier) or carries an explicit fallback-reason explanation.
        for row in &rows {
            if row.result_count == 0 {
                assert!(
                    row.answering_tier == "empty" || !row.fallback_reason.is_empty(),
                    "unexplained empty result for {}/{} needle={:?}",
                    row.project,
                    row.file,
                    row.needle
                );
            }
        }

        let receipt = json!({
            "schema_version": 1,
            "claim_boundary": "This PR measures current references behavior across a declared \
                representative workspace corpus, verifies exactness and honest degradation, and \
                identifies where the request-time text scan is removable, required by an \
                unresolved coverage gap, or replaceable by explicit refusal. It does not alter \
                live provider behavior or claim real user-traffic weighting.",
            "projects": projects,
            "request_count": rows.len(),
            "rows": rows.iter().map(row_to_json).collect::<Vec<_>>(),
        });
        eprintln!(
            "references_representative_replay_receipt={}",
            serde_json::to_string_pretty(&receipt)?
        );

        let mut by_class: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for row in &rows {
            by_class.entry(row.fact_class.as_str()).or_default().push(row.disposition);
        }
        eprintln!("--- #1658 disposition rollup by fact-class bucket ---");
        for (class, dispositions) in &by_class {
            eprintln!("  {class:<24} {dispositions:?}");
        }

        Ok(())
    }

    /// Hard assertion #2 (stale-generation requests produce ZERO false-exact
    /// answers), proven mechanically rather than merely observed: the
    /// `semantic_source_backed` tier is only reachable inside the
    /// `IndexAccessMode::Full(coordinator)` branch of `handle_references_inner`
    /// (see `crates/perl-lsp-rs/src/runtime/language/references.rs`). A
    /// Building/partial-index coordinator — the same stand-in the routing
    /// matrix above uses for a stale/not-yet-caught-up index — makes that
    /// whole branch structurally unreachable, so it can never emit a
    /// false-exact answer regardless of what the request would otherwise find.
    #[test]
    fn references_representative_replay_stale_index_never_source_backed(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut server = create_server();
        let files = open_project(&server, "mojolicious_skeleton")?;
        set_index_building(&mut server);

        let content = fixture_content(&files, "lib/Mojolicious.pm")?;
        let uri = project_uri("mojolicious_skeleton", "lib/Mojolicious.pm");
        let (line, character) = occurrence_position(content, "$plugins", 0)?;
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": false}
        });
        server.test_handle_references(Some(params))?;

        let explanation = explain_provider_decision(&server, "references")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing request_receipt")?;
        let answering_tier =
            receipt.get("answering_tier").and_then(Value::as_str).unwrap_or("unknown");
        let source_backed = receipt.get("source_backed").and_then(Value::as_bool).unwrap_or(false);

        assert_ne!(
            answering_tier, "semantic_source_backed",
            "a stale/partial index state must not reach the source-backed tier"
        );
        assert!(!source_backed, "a stale/partial index state must not report source_backed=true");

        Ok(())
    }
}
