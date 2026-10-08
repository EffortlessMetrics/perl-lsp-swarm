//! Benchmarks for the `ripr-facts` packet builder (#17154).
//!
//! P1 `packet_build` measures [`build_ripr_facts_packet`] over generated
//! small / medium / large fixture trees. P2 `packet_fingerprint` measures
//! consumer-side fingerprint re-derivation (sorted semantic-tuple projection +
//! SHA-256, the recipe `packet.rs` pins) over one large packet.
//!
//! Budgets are SLOs, not hard asserts: small <100ms, medium <1s, large <10s
//! (see `.ci/benchmark-thresholds.yaml`, `targets.ripr`). Criterion reports the
//! timings with per-packet-byte throughput; CI compares runs against baselines.
//!
//! Fixtures live under the fixed repo-relative `target/ripr-bench/<size>`
//! directories, mirroring `tests/packet_characterization.rs`:
//! [`RiprFactsRequest::root`] must be repo-relative, so `std::env::temp_dir()`
//! is unusable here. Packet building only reads, so each tree is staged once
//! outside the timed loop (no per-iteration `iter_batched` setup needed).
//!
//! No `unwrap`/`expect`/`panic!` here: fallible setup goes through the
//! `perl-test-must` helpers (re-exported by `perl-tdd-support`), which satisfy
//! the workspace `deny` lints without an `allow` header.

#![deny(clippy::map_err_ignore)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_with};
use sha2::{Digest, Sha256};
use std::hint::black_box;

const FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// Number of `.pm` modules in the large tree (each ships with one `.t` test).
const LARGE_MODULES: usize = 100;

/// (bench id, `.pm` module count); each module ships with one `.t` test file.
const TREE_SIZES: &[(&str, usize)] = &[("small", 2), ("medium", 20), ("large", LARGE_MODULES)];

/// One generated module: a package with two subs, one containing an
/// `eval { }` dynamic boundary (same shape as the characterization fixture).
fn module_source(index: usize) -> String {
    format!(
        "use strict;\nuse warnings;\n\npackage Mod{index};\n\nsub alpha {{\n    my ($amount) = @_;\n    if ($amount > 100) {{\n        return $amount * 0.9;\n    }}\n    return $amount;\n}}\n\nsub risky {{\n    eval {{ die \"boom\" }};\n    return 1;\n}}\n\n1;\n"
    )
}

/// One generated test: `Test::More` with package-qualified calls into the
/// matching module, exercising the oracle and relation emitters.
fn test_source(index: usize) -> String {
    format!(
        "use strict;\nuse warnings;\nuse Test::More;\n\nuse Mod{index};\n\nis(Mod{index}::alpha(50), 50, 'no discount under threshold');\nis(Mod{index}::alpha(200), 180, 'discount applied above threshold');\nok(Mod{index}::risky(), 'risky returns true');\n\ndone_testing;\n"
    )
}

/// Stage (or re-stage) the fixture tree for `id` and return its repo-relative root.
fn stage_fixture(id: &str, modules: usize) -> String {
    let root = format!("target/ripr-bench/{id}");
    let _ = std::fs::remove_dir_all(&root);
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "create bench fixture lib dir");
    must_with(std::fs::create_dir_all(format!("{root}/t")), "create bench fixture t dir");
    for index in 0..modules {
        must_with(
            std::fs::write(format!("{root}/lib/Mod{index}.pm"), module_source(index)),
            format!("write bench module {index}"),
        );
        must_with(
            std::fs::write(format!("{root}/t/mod{index}.t"), test_source(index)),
            format!("write bench test {index}"),
        );
    }
    root
}

fn packet_request(root: &str) -> RiprFactsRequest<'_> {
    RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: FACT_CLASSES,
        diff: None,
    }
}

/// P1: full packet build over the small / medium / large fixture trees.
fn bench_packet_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_build");
    for &(id, modules) in TREE_SIZES {
        let root = stage_fixture(id, modules);
        let request = packet_request(&root);
        let probe = must(build_ripr_facts_packet(&request));
        let probe_len = must_with(serde_json::to_string(&probe), "serialize probe packet").len();
        group.throughput(Throughput::Bytes(probe_len as u64));
        group.bench_function(BenchmarkId::from_parameter(id), |b| {
            b.iter(|| {
                let packet = must(build_ripr_facts_packet(black_box(&request)));
                black_box(packet);
            });
        });
    }
    group.finish();
}

fn bench_packet_array<'a>(
    packet: &'a serde_json::Value,
    key: &str,
) -> impl Iterator<Item = &'a serde_json::Value> {
    packet.get(key).and_then(serde_json::Value::as_array).into_iter().flatten()
}

fn bench_string_field<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(serde_json::Value::as_str).unwrap_or("")
}

/// Consumer-side fingerprint re-derivation over an already-built packet.
///
/// RIPR validates `packet_fingerprint` from semantic identity tuples rather
/// than trusting the producer string, so this measures that validation cost on
/// a large packet: project the (file, owner, change, oracle, relation) tuples,
/// sort, and SHA-256 them under the same `tag\0field\0...` framing the emitter
/// uses. The one-time drift check in [`bench_packet_fingerprint`] fails the
/// bench loudly if this mirror ever disagrees with the embedded fingerprint.
fn derive_fingerprint(packet: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();

    let mut files: Vec<(String, String)> = bench_packet_array(packet, "files")
        .map(|file| {
            (
                bench_string_field(file, "file_id").to_owned(),
                bench_string_field(file, "path").replace('\\', "/"),
            )
        })
        .collect();
    files.sort();
    for (file_id, path) in &files {
        hasher.update(b"file\0");
        hasher.update(file_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
    }

    let mut owner_ids: Vec<String> = bench_packet_array(packet, "owners")
        .map(|owner| bench_string_field(owner, "owner_id").to_owned())
        .collect();
    owner_ids.sort();
    for owner_id in &owner_ids {
        hasher.update(b"owner\0");
        hasher.update(owner_id.as_bytes());
        hasher.update(b"\0");
    }

    let mut change_ids: Vec<String> = bench_packet_array(packet, "changes")
        .map(|change| bench_string_field(change, "change_id").to_owned())
        .collect();
    change_ids.sort();
    for change_id in &change_ids {
        hasher.update(b"change\0");
        hasher.update(change_id.as_bytes());
        hasher.update(b"\0");
    }

    let mut oracle_tuples: Vec<(String, String, String, String)> =
        bench_packet_array(packet, "oracles")
            .map(|oracle| {
                (
                    bench_string_field(oracle, "oracle_id").to_owned(),
                    bench_string_field(oracle, "target_owner_id").to_owned(),
                    bench_string_field(oracle, "observed_sink").to_owned(),
                    bench_string_field(oracle, "expected_expression").to_owned(),
                )
            })
            .collect();
    oracle_tuples.sort();
    for (oracle_id, target, sink, expected) in &oracle_tuples {
        hasher.update(b"oracle\0");
        hasher.update(oracle_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(target.as_bytes());
        hasher.update(b"\0");
        hasher.update(sink.as_bytes());
        hasher.update(b"\0");
        hasher.update(expected.as_bytes());
        hasher.update(b"\0");
    }

    let mut relation_tuples: Vec<(String, String, String, String, String)> =
        bench_packet_array(packet, "relations")
            .map(|relation| {
                (
                    bench_string_field(relation, "relation_id").to_owned(),
                    bench_string_field(relation, "change_id").to_owned(),
                    bench_string_field(relation, "owner_id").to_owned(),
                    bench_string_field(relation, "test_id").to_owned(),
                    bench_string_field(relation, "oracle_id").to_owned(),
                )
            })
            .collect();
    relation_tuples.sort();
    for (relation_id, change_id, owner_id, test_id, oracle_id) in &relation_tuples {
        hasher.update(b"relation\0");
        hasher.update(relation_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(change_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(owner_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(test_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(oracle_id.as_bytes());
        hasher.update(b"\0");
    }

    let digest = hasher.finalize();
    format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

/// Fail unless the P2 mirror reproduces the packet's embedded fingerprint.
fn check_fingerprint_mirror(packet: &serde_json::Value) -> Result<(), String> {
    let embedded = bench_string_field(packet, "packet_fingerprint");
    let derived = derive_fingerprint(packet);
    if embedded == derived {
        Ok(())
    } else {
        Err(format!("P2 fingerprint mirror drifted: embedded `{embedded}` != derived `{derived}`"))
    }
}

/// P2: consumer-side fingerprint re-derivation over one large packet.
fn bench_packet_fingerprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_fingerprint");
    let root = stage_fixture("large", LARGE_MODULES);
    let packet = must(build_ripr_facts_packet(&packet_request(&root)));
    must_with(
        check_fingerprint_mirror(&packet),
        "P2 mirror must match the embedded packet_fingerprint",
    );
    let packet_len = must_with(serde_json::to_string(&packet), "serialize large packet").len();
    group.throughput(Throughput::Bytes(packet_len as u64));
    group.bench_function(BenchmarkId::from_parameter("large"), |b| {
        b.iter(|| {
            let fingerprint = derive_fingerprint(black_box(&packet));
            black_box(fingerprint);
        });
    });
    group.finish();
}

criterion_group!(packet_build_group, bench_packet_build);
criterion_group!(packet_fingerprint_group, bench_packet_fingerprint);
criterion_main!(packet_build_group, packet_fingerprint_group);
