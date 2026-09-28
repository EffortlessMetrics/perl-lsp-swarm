#![expect(
    clippy::print_stderr,
    reason = "Scenario 71 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 71 — Completion during workspace indexing must not wedge the
//! server (#16651).
//!
//! On a ~2,000-file workspace the startup scan runs for seconds. A
//! `textDocument/completion` sent mid-scan used to coincide with a permanent
//! total wedge (issue #16651): the indexer's per-file critical section held
//! the `workspace_folders` mutex across `index_file`, while a diagnostics
//! publication inside `with_semantic_queries_for_uri` held the four semantic
//! read guards and resolved modules through that same mutex from inside its
//! callback — a permanent ABBA deadlock that froze indexing mid-scan and left
//! every later request (completion, hover, definition, didOpen, shutdown)
//! unanswered.
//!
//! Acceptance criteria (all on ONE server run, per the #16668 UX contract):
//! - a completion sent mid-scan answers within a bounded window;
//! - indexing still reaches a terminal `perl-lsp/index-ready` state;
//! - hover, a second completion, and a later didOpen stay responsive;
//! - shutdown/exit completes cleanly.
//!
//! The test fails as vacuous if indexing already reached its terminal state
//! before the mid-scan completion was sent, so a fast machine cannot pass
//! without exercising the race window.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::{Duration, Instant};

/// Workspace size matches the wire reproduction scale (~2,000 Perl files) so
/// the startup scan spans several seconds and the mid-scan window is real.
const INDEXED_MODULE_COUNT: usize = 2_000;

/// One large script (~2 MB) mirrors the wire reproduction (`script_big.pl`):
/// its single `index_file` commit spans seconds, which is what stretches the
/// #16651 deadlock window from a per-file race into a near-certain overlap
/// with a concurrent diagnostics publication.
const BIG_SCRIPT_LINES: usize = 40_000;

fn module_path(index: usize) -> String {
    format!("lib/App0/Unit{:02}/Mod{:04}.pm", (index / 100) % 50, index)
}

fn module_source(index: usize) -> String {
    format!(
        "package App0::Unit{:02}::Mod{:04};\nuse strict;\nuse warnings;\n\n\
         sub mod{:04}_helper {{ my ($class, $n) = @_; return $n + {}; }}\n\
         sub describe {{ return 'App0::Unit{:02}::Mod{:04}'; }}\n\n1;\n",
        (index / 100) % 50,
        index,
        index,
        index,
        (index / 100) % 50,
        index,
    )
}

fn main_script() -> String {
    "#!perl\nuse strict;\nuse warnings;\n\n\
     use App0::Unit00::Mod0000;\nuse App0::Unit04::Mod0200;\nuse App0::Unit08::Mod0400;\n\n\
     my $obj = App0::Unit00::Mod0000->new();\n\
     print $obj->describe(), \"\\n\";\n\
     my $r = App0::Unit00::Mod0000::mod0000_helper(undef, 21);\n\
     print \"r=$r\\n\";\n"
        .to_string()
}

fn big_script() -> String {
    let mut buf = String::with_capacity(BIG_SCRIPT_LINES * 52);
    buf.push_str("#!perl\nuse strict;\nuse warnings;\n\n");
    for i in 0..BIG_SCRIPT_LINES {
        buf.push_str(&format!("sub big_helper_{i} {{ my $x_{i} = {i}; return $x_{i} + 1; }}\n"));
    }
    buf.push_str("print \"done\\n\";\n");
    buf
}

fn fixture_config() -> ScenarioConfig {
    let mut config = ScenarioConfig::default().with_file("main.pl", main_script());
    for index in 0..INDEXED_MODULE_COUNT {
        config = config.with_file(module_path(index), module_source(index));
    }
    config = config.with_file("script_big.pl", big_script());
    config
}

/// True when a terminal `perl-lsp/index-ready` state is already buffered.
fn saw_terminal_index_ready(events: &[serde_json::Value]) -> bool {
    events.iter().any(|message| {
        message.get("method").and_then(|method| method.as_str()) == Some("perl-lsp/index-ready")
            && {
                let params = message.get("params");
                params.and_then(|params| params.get("ready")).and_then(|ready| ready.as_bool())
                    == Some(true)
                    || matches!(
                        params
                            .and_then(|params| params.get("state"))
                            .and_then(|state| state.as_str()),
                        Some("failed") | Some("degraded")
                    )
            }
    })
}

#[test]
fn scenario_71_completion_mid_indexing_answers_and_indexing_reaches_terminal() -> Result<(), String>
{
    if !binary_available() {
        eprintln!("SKIP scenario_71: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(fixture_config())
        .map_err(|error| format!("Failed to create harness: {error}"))?;

    let script = main_script();
    harness
        .open_file("main.pl", &script)
        .map_err(|error| format!("didOpen main.pl should succeed: {error}"))?;

    // Additional didOpens spawn additional diagnostics publications; each one
    // is an independent chance for the post-parse semantic tier to overlap the
    // startup scan, which is the #16651 deadlock partner.
    for index in 1..=3 {
        harness
            .open_file(module_path(index).as_str(), &module_source(index))
            .map_err(|error| format!("didOpen {index} should succeed: {error}"))?;
    }

    // Vacuous-failure guard: if the startup scan already reached its terminal
    // state, this run proves nothing about the mid-scan window (#16668 proof
    // plan requires observing the Building phase before the completion).
    if saw_terminal_index_ready(&harness.client.peek_raw_events()) {
        return Err(
            "vacuous: indexing already reached a terminal state before the mid-scan completion \
             was sent; the #16651 race window was not exercised"
                .to_string(),
        );
    }

    // The mid-scan completion. Bounded window: pre-fix, this request never
    // answered at all (wire receipts show 170-260 s of total inbound silence).
    let completion_started = Instant::now();
    let completions = harness
        .completion_with_timeout("main.pl", 7, 25, Duration::from_secs(45))
        .map_err(|error| {
            format!(
                "completion sent mid-indexing did not answer within 45s — #16651 wedge \
                 regression: {error}"
            )
        })?;
    let completion_elapsed = completion_started.elapsed();
    let _ = completions;

    // Indexing must still reach a terminal state on the same server run. The
    // pre-fix wedge froze the scan mid-progress with no terminal message ever.
    let terminal = harness
        .client
        .wait_for_raw_events(Duration::from_mins(2), |events| {
            if !saw_terminal_index_ready(events) {
                return None;
            }
            events
                .iter()
                .find(|message| {
                    message.get("method").and_then(|method| method.as_str())
                        == Some("perl-lsp/index-ready")
                })
                .cloned()
        })
        .map_err(|end| {
            format!(
                "indexing never reached a terminal state after the mid-scan completion \
                 (answered in {completion_elapsed:?}) — #16651 wedge regression: {end:?}"
            )
        })?;
    let _ = terminal;

    // Later requests stay responsive on the same process.
    harness
        .hover_with_timeout("main.pl", 7, 5, Duration::from_secs(20))
        .map_err(|error| format!("hover after indexing should answer: {error}"))?;
    harness
        .completion_with_timeout("main.pl", 8, 10, Duration::from_secs(20))
        .map_err(|error| format!("completion after indexing should answer: {error}"))?;
    harness
        .open_file("lib/App0/Unit00/Mod0001.pm", &module_source(1))
        .map_err(|error| format!("later didOpen should succeed: {error}"))?;

    harness.assert_no_crash();
    harness
        .client
        .shutdown_and_exit(Duration::from_secs(30))
        .map_err(|error| format!("clean shutdown should succeed: {error}"))?;
    Ok(())
}
