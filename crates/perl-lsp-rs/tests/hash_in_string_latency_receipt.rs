//! Latency receipt (#19206): cost of adjudicating the comment guard with
//! the snapshot `SourceRegionIndex` on lines containing `#`.
//!
//! Ignored by default; run manually with `--ignored --nocapture`.
//! Lane A (`#` in a string before code) pays the adjudication; lane C (no
//! `#` on the line) short-circuits in the naive check, so A - C isolates
//! the guard delta from resolver cost. Cold lanes time the first request
//! of each fresh generation, which pays the once-per-generation index
//! build.

mod support;

use serde_json::{Value, json};
use std::time::Instant;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn percentile(sorted: &[u128], pct: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() * pct) / 100).min(sorted.len() - 1);
    sorted[idx]
}

fn summarize(label: &str, mut samples: Vec<u128>) {
    samples.sort_unstable();
    eprintln!(
        "receipt {label}: n={} p50={}us p95={}us max={}us",
        samples.len(),
        percentile(&samples, 50),
        percentile(&samples, 95),
        samples.last().copied().unwrap_or(0),
    );
}

fn request_timed(
    harness: &mut LspHarness,
    method: &str,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<(Value, u128), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let result = harness.request(
        method,
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
    )?;
    Ok((result, start.elapsed().as_micros()))
}

fn warm_lane(doc: &str, uri: &str, needle: &str, label: &str) -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document(uri, doc)?;
    harness.barrier();
    let (line, character) = doc
        .lines()
        .enumerate()
        .find_map(|(idx, text)| text.find(needle).map(|col| (idx as u32, col as u32)))
        .ok_or("needle not found")?;

    // Warm-up so the first (cold) request does not pollute the warm lane.
    let _ = request_timed(&mut harness, "textDocument/definition", uri, line, character + 2)?;
    let mut samples = Vec::new();
    for _ in 0..100 {
        let (_, micros) =
            request_timed(&mut harness, "textDocument/definition", uri, line, character + 2)?;
        samples.push(micros);
    }
    summarize(label, samples);
    Ok(())
}

#[test]
#[ignore = "manual latency receipt"]
fn hash_guard_latency_receipt() -> TestResult {
    warm_lane(
        "sub target_sub { return 1; }\nmy $sep = \"#\"; target_sub();\n",
        "file:///lane_a.pl",
        "target_sub();",
        "warm definition A hash-in-string small",
    )?;
    warm_lane(
        "sub target_sub { return 1; }\nmy $sep = \"x\"; target_sub();\n",
        "file:///lane_c.pl",
        "target_sub();",
        "warm definition C no-hash small",
    )?;

    // Large-document lanes: the once-per-generation index build scans the
    // whole source, so its cost only becomes visible at scale.
    let mut large = String::new();
    for i in 0..600 {
        large.push_str(&format!("sub s{i} {{ return {i}; }}\n"));
    }
    large.push_str("sub target_sub { return 1; }\n");
    let large_a = format!("{large}my $sep = \"#\"; target_sub();\n");
    let large_c = format!("{large}my $sep = \"x\"; target_sub();\n");
    warm_lane(&large_a, "file:///lane_a_large.pl", "target_sub();", "warm definition A large")?;
    warm_lane(&large_c, "file:///lane_c_large.pl", "target_sub();", "warm definition C large")?;

    // Cold lane: first definition request of each fresh generation.
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///lane_cold.pl", &large_a)?;
    harness.barrier();
    let mut samples = Vec::new();
    for generation in 0..20 {
        let tweaked = format!("{large_a}# generation {generation}\n");
        harness.change_full("file:///lane_cold.pl", generation + 2, &tweaked)?;
        harness.barrier();
        let lines = tweaked.lines().count() as u32;
        let (_, micros) = request_timed(
            &mut harness,
            "textDocument/definition",
            "file:///lane_cold.pl",
            lines - 2,
            17,
        )?;
        samples.push(micros);
    }
    summarize("cold definition A large first-per-generation", samples);
    Ok(())
}
