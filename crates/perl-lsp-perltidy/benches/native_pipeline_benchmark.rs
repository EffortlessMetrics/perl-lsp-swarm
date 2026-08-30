//! Criterion subject benches for the native formatter production pipeline
//! (#10302).
//!
//! Every subject drives the real typed pipeline end-to-end
//! (`format_document_typed_with_counters`, including classification and evidence) over the
//! checked-in scaling cohort from `support/perf_subjects.rs` — never a
//! helper-only microbenchmark. Subject identity (content digest, production
//! config fingerprint, engine, toolchain tag) is emitted to
//! `target/criterion/native-pipeline-measurements.v1.json` for the nightly
//! receipt chain, which uploads it alongside Criterion output.
//!
//! Identity construction is a deliberate two-pass trade-off:
//! `build_subject_identities` measures each subject through
//! `format_document_typed_with_counters` (scope install, `record_with`,
//! `pipeline_invocations`) while Criterion's timing iterations re-run the plain
//! `format_document_typed` path. The sidecar `elapsed` and Criterion's estimates
//! therefore come from different code paths, and each subject is measured twice
//! (nightly bench work scales with 2x subject count). Closing that divergence is
//! out of scope for this bounded slice.
//!
//! Wall-clock here is advisory evidence only: per #3979/#5282 the nightly
//! baseline comparison stays `continue-on-error: true` and no PR gate reads
//! these numbers.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

// The subject registry is a shared superset surface: the canary tests consume
// the scaling/variant constructors while this bench consumes the enrolled
// cohort, so per-target dead-code analysis necessarily sees unused items.
#[allow(dead_code)]
#[path = "support/perf_subjects.rs"]
mod perf_subjects;

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use perf_subjects::{BENCH_GROUP, bench_rows, identity_row_with_counters, toolchain_tag};
use perl_lsp_perltidy::native::{
    FormatConfig, FormatContext, NativeFormatter, NativePipelineCounters,
};

/// The one run identity carried by both the sidecar envelope and every enrolled
/// row. The nightly validator requires the two to be equal, so the fallback used
/// when `NATIVE_PIPELINE_RUN_ID` is absent must be derived exactly once.
fn run_id(toolchain: &str) -> String {
    std::env::var("NATIVE_PIPELINE_RUN_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("local-{toolchain}"))
}

/// Fail closed when the receipt identity file cannot be written: a bench run
/// without per-subject identity rows is exactly the aggregate-only evidence
/// NPC-008 forbids.
fn write_subject_identities(rows: &[serde_json::Value], run_id: &str) {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/criterion/native-pipeline-measurements.v1.json");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| {
            eprintln!("native pipeline bench: cannot create {}: {error}", parent.display());
            std::process::exit(2);
        });
    }
    let payload = serde_json::json!({
        "schema": "native-pipeline-measurements-v1",
        "run_id": run_id,
        "subjects": rows,
    });
    let serialized = serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        eprintln!("native pipeline bench: cannot serialize {}: {error}", output.display());
        std::process::exit(2);
    });
    std::fs::write(&output, serialized).unwrap_or_else(|error| {
        eprintln!("native pipeline bench: cannot write {}: {error}", output.display());
        std::process::exit(2);
    });
}

fn build_subject_identities(toolchain: &str, run_id: &str) -> Vec<serde_json::Value> {
    bench_rows()
        .iter()
        .map(|spec| {
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let typed = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            identity_row_with_counters(spec, &typed, toolchain, run_id, &counters)
        })
        .collect()
}

fn native_pipeline_document(c: &mut Criterion) {
    let toolchain = toolchain_tag();
    let run_id = run_id(&toolchain);
    let identities = build_subject_identities(&toolchain, &run_id);
    write_subject_identities(&identities, &run_id);

    let mut group = c.benchmark_group(BENCH_GROUP);
    for spec in bench_rows() {
        let source = spec.source();
        group.bench_function(spec.id(), |b| {
            b.iter(|| {
                black_box(NativeFormatter::new().format_document_typed(
                    black_box(&source),
                    &FormatConfig::default(),
                    &FormatContext::default(),
                ))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, native_pipeline_document);
criterion_main!(benches);
