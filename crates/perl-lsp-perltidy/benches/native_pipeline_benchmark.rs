//! Criterion subject benches for the native formatter production pipeline
//! (#10302).
//!
//! Every subject drives the real typed pipeline end-to-end
//! (`format_document_typed`, including classification and evidence) over the
//! checked-in scaling cohort from `support/perf_subjects.rs` — never a
//! helper-only microbenchmark. Subject identity (content digest, production
//! config fingerprint, engine, toolchain tag) is emitted to
//! `benchmarks/results/native-pipeline-subjects.json` for the nightly receipt
//! chain, which consumes it unmodified (NPC-008).
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
use perf_subjects::{BENCH_GROUP, SubjectSpec, bench_rows, identity_row, toolchain_tag};
use perl_lsp_perltidy::native::{FormatConfig, FormatContext, NativeFormatter};

/// Fail closed when the receipt identity file cannot be written: a bench run
/// without per-subject identity rows is exactly the aggregate-only evidence
/// NPC-008 forbids.
fn write_subject_identities(rows: &[serde_json::Value]) {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/native-pipeline-subjects.json");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| {
            eprintln!("native pipeline bench: cannot create {}: {error}", parent.display());
            std::process::exit(2);
        });
    }
    let payload = serde_json::json!({ "subjects": rows });
    std::fs::write(&output, serde_json::to_string_pretty(&payload).unwrap_or_default())
        .unwrap_or_else(|error| {
            eprintln!("native pipeline bench: cannot write {}: {error}", output.display());
            std::process::exit(2);
        });
}

fn build_subject_identities() -> Vec<serde_json::Value> {
    // Observe the production config fingerprint once through the real typed
    // path so receipt rows carry the exact fingerprint the pipeline records.
    let probe = SubjectSpec { family: "delimited", line_ending: "lf", indent: "tabs", units: 1 };
    let probe_source = probe.source();
    let fingerprint = NativeFormatter::new()
        .format_document_typed(&probe_source, &FormatConfig::default(), &FormatContext::default())
        .outcome
        .identity
        .config_fingerprint;
    let toolchain = toolchain_tag();
    bench_rows().iter().map(|spec| identity_row(spec, &fingerprint, &toolchain)).collect()
}

fn native_pipeline_document(c: &mut Criterion) {
    let identities = build_subject_identities();
    write_subject_identities(&identities);

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
