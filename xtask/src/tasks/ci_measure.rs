//! Local CI lane timing measurement (#211).
//!
//! The producer emits three artifacts under `artifacts/`:
//!
//! * `ci-time.ndjson` — one JSON object per lane (machine-readable stream).
//! * `ci-time.json`   — consolidated payload (machine-readable summary).
//! * `ci-time.md`     — human-readable Markdown summary.
//!
//! Each artifact carries an identical `schema_version` and `producer` field so
//! that consumers and the parallel `.ci/scripts/measure-ci-time.sh` script
//! cannot silently interleave with this producer. See the `SCHEMA_VERSION` and
//! `PRODUCER` constants below — they MUST stay byte-identical with the values
//! declared in `.ci/scripts/measure-ci-time.sh` so a reader can identify which
//! producer emitted the file.
//!
//! The Rust `cargo xtask ci-measure` producer is the canonical authority per
//! the documented xtask migration (`docs/project/XTASK_MIGRATION.md`); the
//! Python script is kept for the manual `bash` invocation but must remain
//! schema-compatible.

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::process::Command;
use std::time::Instant;

use crate::utils::project_root;

/// Schema contract for `ci-time.*` artifacts (`#15381`).
///
/// Increment only on a backwards-incompatible shape change. Consumers MUST
/// reject payloads whose `schema_version` does not match this constant.
pub const SCHEMA_VERSION: &str = "ci-time.v1";

/// Stable producer identity for `cargo xtask ci-measure`.
///
/// Kept byte-identical with the `PRODUCER` constant in
/// `.ci/scripts/measure-ci-time.sh`; a test in this module ratchets the two.
pub const PRODUCER: &str = "cargo-xtask-ci-measure";

const LANE_COMMANDS: &[(&str, &[&str])] = &[
    ("ci-format", &["ci-format"]),
    ("ci-docs-check", &["ci-docs-check"]),
    ("ci-clippy-lib", &["ci-clippy-lib"]),
    ("clippy-prod-no-unwrap", &["clippy-prod-no-unwrap"]),
    ("ci-test-lib", &["ci-test-lib"]),
    ("ci-lsp-def", &["ci-lsp-def"]),
    ("status-check", &["status-check"]),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CiLaneResult {
    pub schema_version: &'static str,
    pub producer: &'static str,
    pub name: String,
    pub seconds: f64,
    pub returncode: i32,
}

impl CiLaneResult {
    fn measure(name: &str, start: Instant, status: std::process::ExitStatus) -> Self {
        let elapsed = (start.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;
        let returncode = status.code().unwrap_or(-1);
        Self {
            schema_version: SCHEMA_VERSION,
            producer: PRODUCER,
            name: name.to_string(),
            seconds: elapsed,
            returncode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CiMeasurePayload {
    pub schema_version: &'static str,
    pub producer: &'static str,
    pub generated_at: String,
    pub lanes: Vec<CiLaneResult>,
    pub total_seconds: f64,
}

/// Build a consolidated payload from already-collected lane results.
///
/// Extracted so the shape is unit-testable without invoking `just`. The
/// `generated_at` argument is taken as an `String` rather than `Utc::now()` so
/// tests can pin a deterministic timestamp.
pub fn build_payload(generated_at: String, lanes: Vec<CiLaneResult>) -> CiMeasurePayload {
    let total_seconds: f64 = lanes.iter().map(|lane| lane.seconds).sum();
    CiMeasurePayload {
        schema_version: SCHEMA_VERSION,
        producer: PRODUCER,
        generated_at,
        lanes,
        total_seconds: (total_seconds * 1000.0).round() / 1000.0,
    }
}

pub fn run() -> Result<()> {
    let root = project_root()?;

    let artifacts_dir = root.join("artifacts");
    fs::create_dir_all(&artifacts_dir).context("Failed to create artifacts directory")?;

    let mut lanes = Vec::new();
    let mut ndjson = String::new();

    for &(name, args) in LANE_COMMANDS.iter() {
        println!("==> {}", name);

        let start = Instant::now();
        let status = Command::new("just")
            .current_dir(&root)
            .args(args)
            .status()
            .with_context(|| format!("Failed to run '{name}' lane"))?;

        let record = CiLaneResult::measure(name, start, status);
        ndjson.push_str(&serde_json::to_string(&record)?);
        ndjson.push('\n');

        if !status.success() {
            bail!("CI lane '{name}' failed with exit code {}", record.returncode);
        }

        lanes.push(record);
    }

    let payload = build_payload(Utc::now().to_rfc3339(), lanes);

    let ndjson_path = artifacts_dir.join("ci-time.ndjson");
    let json_path = artifacts_dir.join("ci-time.json");
    let md_path = artifacts_dir.join("ci-time.md");

    fs::write(&ndjson_path, ndjson).context("Failed to write ci-time.ndjson")?;
    fs::write(&json_path, serde_json::to_string_pretty(&payload)?.as_bytes())
        .context("Failed to write ci-time.json")?;

    let markdown = render_markdown(&payload);
    fs::write(&md_path, markdown).context("Failed to write ci-time.md")?;

    println!("Wrote:");
    println!("  - {}", json_path.display());
    println!("  - {}", md_path.display());
    println!("  - {}", ndjson_path.display());

    Ok(())
}

/// Render the human-readable Markdown summary for a payload.
///
/// Extracted so tests can pin both the header schema-version line and the
/// lane table without invoking the live CI suite.
pub fn render_markdown(payload: &CiMeasurePayload) -> String {
    let mut markdown = String::new();
    markdown.push_str("# CI Timing Baseline\n\n");
    markdown.push_str(&format!(
        "- Generated at: `{}`\n- Total: `{}s`\n- Schema: `{}`\n- Producer: `{}`\n\n",
        payload.generated_at, payload.total_seconds, payload.schema_version, payload.producer,
    ));
    markdown.push_str("| Lane | Seconds | RC |\n|------|---------|----|\n");
    for lane in &payload.lanes {
        markdown
            .push_str(&format!("| `{}` | {} | {} |\n", lane.name, lane.seconds, lane.returncode));
    }
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lane(name: &str, seconds: f64, returncode: i32) -> CiLaneResult {
        CiLaneResult {
            schema_version: SCHEMA_VERSION,
            producer: PRODUCER,
            name: name.to_string(),
            seconds,
            returncode,
        }
    }

    #[test]
    fn schema_version_and_producer_constants_are_pinned() {
        // The exact byte strings are part of the cross-producer contract with
        // `.ci/scripts/measure-ci-time.sh`. Bumping them is a breaking change.
        assert_eq!(SCHEMA_VERSION, "ci-time.v1");
        assert_eq!(PRODUCER, "cargo-xtask-ci-measure");
    }

    #[test]
    fn build_payload_threads_schema_version_into_payload_and_lanes() {
        let lanes = vec![sample_lane("ci-format", 0.123, 0), sample_lane("ci-docs-check", 1.5, 0)];
        let payload = build_payload("2026-09-18T20:00:00+00:00".to_string(), lanes.clone());

        assert_eq!(payload.schema_version, "ci-time.v1");
        assert_eq!(payload.producer, "cargo-xtask-ci-measure");
        assert_eq!(payload.lanes.len(), 2);
        for (lane, source) in payload.lanes.iter().zip(lanes.iter()) {
            assert_eq!(lane.schema_version, "ci-time.v1");
            assert_eq!(lane.producer, "cargo-xtask-ci-measure");
            assert_eq!(lane.name, source.name);
            assert_eq!(lane.returncode, source.returncode);
        }
        assert!((payload.total_seconds - 1.623).abs() < 1e-9);
    }

    #[test]
    fn payload_serialization_carries_schema_version_field() {
        let payload = build_payload(
            "2026-09-18T20:00:00+00:00".to_string(),
            vec![sample_lane("ci-format", 0.5, 0)],
        );
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();
        assert_eq!(value["schema_version"], "ci-time.v1");
        assert_eq!(value["producer"], "cargo-xtask-ci-measure");
        assert_eq!(value["lanes"][0]["schema_version"], "ci-time.v1");
        assert_eq!(value["lanes"][0]["producer"], "cargo-xtask-ci-measure");
    }

    #[test]
    fn lane_serialization_carries_schema_version_field() {
        let lane = sample_lane("ci-format", 0.5, 0);
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&lane).unwrap()).unwrap();
        assert_eq!(value["schema_version"], "ci-time.v1");
        assert_eq!(value["producer"], "cargo-xtask-ci-measure");
        assert_eq!(value["name"], "ci-format");
    }

    #[test]
    fn render_markdown_includes_schema_and_producer_header() {
        let payload = build_payload(
            "2026-09-18T20:00:00+00:00".to_string(),
            vec![sample_lane("ci-format", 0.5, 0)],
        );
        let md = render_markdown(&payload);
        assert!(md.contains("- Schema: `ci-time.v1`"), "{md}");
        assert!(md.contains("- Producer: `cargo-xtask-ci-measure`"), "{md}");
        assert!(md.contains("| `ci-format` | 0.5 | 0 |"), "{md}");
    }

    #[test]
    fn python_script_emits_identical_schema_version_and_producer() {
        // The Rust producer is the canonical authority; the Python script
        // remains a fallback for manual `bash` invocation. A consumer reading
        // `ci-time.json` MUST be able to identify which producer wrote the
        // file, so both scripts must publish the same schema_version.
        // The producer strings may differ (they identify the producer), but
        // schema_version is the cross-producer discriminator and must match.
        let script = include_str!("../../../.ci/scripts/measure-ci-time.sh");
        let script_schema = extract_python_constant(script, "SCHEMA_VERSION");
        let script_producer = extract_python_constant(script, "PRODUCER");
        assert_eq!(
            script_schema.as_deref(),
            Some(SCHEMA_VERSION),
            "Python script's SCHEMA_VERSION constant drifted from the Rust producer"
        );
        assert_eq!(
            script_producer.as_deref(),
            Some(PRODUCER),
            "Python script's PRODUCER constant drifted from the Rust producer"
        );
    }

    /// Extract the value of an uppercase Python or shell constant assignment,
    /// e.g. `SCHEMA_VERSION="ci-time.v1"` or `SCHEMA_VERSION = "ci-time.v1"`.
    fn extract_python_constant(source: &str, name: &str) -> Option<String> {
        for line in source.lines() {
            let trimmed = line.trim_start();
            // Accept both Python (`NAME = "value"`) and shell (`NAME="value"`).
            let with_space = format!("{name} = ");
            let no_space = format!("{name}=");
            let (rest, _) = if let Some(value) = trimmed.strip_prefix(&with_space) {
                (value, ())
            } else if let Some(value) = trimmed.strip_prefix(&no_space) {
                (value, ())
            } else {
                continue;
            };
            let value = rest.trim();
            // Strip surrounding quotes.
            let inner = value
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))?;
            return Some(inner.to_string());
        }
        None
    }
}
