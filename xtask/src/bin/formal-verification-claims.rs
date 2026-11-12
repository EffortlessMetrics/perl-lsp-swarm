//! Truthful claim registry for the repository's formal-verification program.
//!
//! The registry distinguishes evidence that is already enforced from checkers
//! that exist but are not merge authority and from proof work that is only
//! planned. It validates every named production target against the checked-out
//! commit and can emit JSON or a reviewer-oriented Markdown projection.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SCHEMA_VERSION: &str = "formal_verification_claims.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Enforced,
    ImplementedUnwired,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    BuildCheck,
    StructuralInvariant,
    PropertyTest,
    BoundedModel,
    MemorySafetyCheck,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct SourceRef {
    path: &'static str,
    marker: &'static str,
}

const fn source(path: &'static str, marker: &'static str) -> SourceRef {
    SourceRef { path, marker }
}

#[derive(Debug, Serialize)]
struct Claim {
    id: &'static str,
    status: Status,
    evidence: Evidence,
    subject: &'static str,
    assertion: &'static str,
    target: SourceRef,
    runner: Option<&'static str>,
    authority: Option<SourceRef>,
    bound: &'static str,
    assumptions: &'static str,
    limitations: &'static str,
    counterexample: &'static str,
    issue: &'static str,
}

const CLAIMS: &[Claim] = &[
    Claim {
        id: "FV-INFRA-001",
        status: Status::ImplementedUnwired,
        evidence: Evidence::StructuralInvariant,
        subject: "formal-verification claim registry",
        assertion: "Every row has a unique ID, live target, explicit evidence class, scope, assumptions, limitations, replay contract, and truthful enforcement state.",
        target: source("xtask/src/bin/formal-verification-claims.rs", "const CLAIMS: &[Claim]"),
        runner: Some("cargo run -p xtask --bin formal-verification-claims --locked -- --check"),
        authority: None,
        bound: "The typed rows in this binary and repository files at the evaluated commit.",
        assumptions: "Source markers identify the intended live implementation surface.",
        limitations: "The registry describes evidence but does not prove a planned claim; it is not merge authority until #19297 wires it.",
        counterexample: "The checker exits non-zero and names the malformed row or stale binding.",
        issue: "#19297",
    },
    Claim {
        id: "FV-BUILD-001",
        status: Status::Enforced,
        evidence: Evidence::BuildCheck,
        subject: "workspace compilation surface",
        assertion: "The locked workspace compiles for the target and feature surface selected by the canonical all-target command.",
        target: source("Cargo.toml", "rust-version = \"1.95\""),
        runner: Some("just check-all-targets"),
        authority: Some(source(".github/workflows/ci.yml", "just check-all-targets")),
        bound: "The targets, features, hosts, and Rust toolchain selected by current CI.",
        assumptions: "Cargo.lock and the checked-out sources are authoritative inputs.",
        limitations: "Compilation is not semantic correctness, memory safety, race freedom, or liveness.",
        counterexample: "The required job fails with the compiler diagnostic for the evaluated commit.",
        issue: "#19297",
    },
    Claim {
        id: "FV-WORKSPACE-OWNERSHIP-001",
        status: Status::Enforced,
        evidence: Evidence::StructuralInvariant,
        subject: "workspace-index lifecycle ownership",
        assertion: "Every inventoried lifecycle proposition has one selected owner and lifecycle-adjacent declarations cannot drift outside the checked denominator.",
        target: source(
            "policy/workspace-index-lifecycle-propositions.v1.tsv",
            "# schema: workspace_index_lifecycle_propositions.v1",
        ),
        runner: Some(
            "cargo test -p xtask --test workspace_index_lifecycle_ledger --locked -- --nocapture",
        ),
        authority: Some(source(
            "scripts/ci/required-merge-surface.sh",
            "cargo test -p xtask --test workspace_index_lifecycle_ledger --locked -- --nocapture",
        )),
        bound: "The proposition, type, member, source-marker, and generated-projection ledgers.",
        assumptions: "Ledger source markers continue to bind the live declarations.",
        limitations: "This proves ownership and drift control, not concurrent currentness, safety, or liveness.",
        counterexample: "The ledger test reports the duplicate owner, missing declaration, stale marker, or stale projection.",
        issue: "#19298",
    },
    Claim {
        id: "FV-UNSAFE-RATCHET-001",
        status: Status::Enforced,
        evidence: Evidence::StructuralInvariant,
        subject: "production unsafe syntax ratchet",
        assertion: "Counted production unsafe syntax does not increase above the executable baseline and every counted occurrence carries adjacent SAFETY reasoning.",
        target: source("ci/check_unsafe_prod.sh", "ci_exec_hygiene check-unsafe-prod"),
        runner: Some("bash ci/check_unsafe_prod.sh"),
        authority: Some(source(".ci/gate-policy.yaml", "check-unsafe-prod")),
        bound: "The production/test selection and numeric syntax baseline owned by perl-ci-hygiene and ci/unsafe_prod_baseline.txt.",
        assumptions: "The checker completely classifies the Rust files in its policy scope.",
        limitations: "The syntax baseline is 12 while policy describes ten semantic sites; the ratchet does not prove soundness or reconcile that mapping.",
        counterexample: "The checker identifies an over-ceiling, unclassified, or undocumented unsafe site.",
        issue: "#19307",
    },
    Claim {
        id: "FV-WORKSPACE-CURRENTNESS-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "workspace generation and settlement protocol",
        assertion: "Only an admitted operation for the accepted workspace generation may publish; stale and duplicate settlements cannot become current.",
        target: source(
            "crates/perl-workspace/src/workspace/workspace_index.rs",
            "pub struct IndexCoordinator",
        ),
        runner: None,
        authority: None,
        bound: "Initial bound: 2 overlapping operations, 3 generations, 1 replacement, and every explored interleaving.",
        assumptions: "Production uses a checked transition kernel or has a tested trace-equivalence bridge to it.",
        limitations: "The initial model excludes filesystem, parser, and executor liveness.",
        counterexample: "A replayable transition trace records admission, generation, settlement, publication, and the first violation.",
        issue: "#19298",
    },
    Claim {
        id: "FV-SCHEDULER-FRESHNESS-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "LSP read freshness and cancellation",
        assertion: "A position-sensitive read is delivered only while its document instance and generation remain valid through final delivery.",
        target: source(
            "crates/perl-lsp-rs/src/runtime/scheduler.rs",
            "pub(crate) fn is_read_stale",
        ),
        runner: None,
        authority: None,
        bound: "Initial bound: 2 document instances, 3 generations, 2 reads, 2 mutations, 1 close/reopen, and every explored schedule.",
        assumptions: "The harness uses production freshness and extracted production queue transitions.",
        limitations: "The model excludes provider correctness and executor fairness.",
        counterexample: "A minimized schedule records ingress, mutation barriers, instance identity, and the incorrect delivery decision.",
        issue: "#19299",
    },
    Claim {
        id: "FV-SCHEDULER-SHUTDOWN-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "LSP scheduler ordering and shutdown",
        assertion: "Mutation barriers, bounded queues, cancellation, and cooperative shutdown do not strand accepted work or deliver after terminal cancellation within the model.",
        target: source(
            "crates/perl-lsp-rs/src/runtime/scheduler.rs",
            "pub(crate) struct Scheduler",
        ),
        runner: None,
        authority: None,
        bound: "Initial bound: queue capacity 2, 2 reads, 2 mutations, 1 cancellation, 1 shutdown, and every explored schedule.",
        assumptions: "Blocking work is represented by an explicit completion event.",
        limitations: "Cooperative shutdown cannot abort arbitrary blocking work and gives no wall-clock bound.",
        counterexample: "A replayable scheduler trace records queue, barrier, cancellation, worker, and response states.",
        issue: "#19299",
    },
    Claim {
        id: "FV-POSITION-LAWS-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "byte, UTF-16, range, and edit laws",
        assertion: "Within the recorded symbolic domain, production position conversion preserves its byte and UTF-16 contracts, rejects invalid coordinates, and cannot overflow or escape source bounds.",
        target: source("crates/perl-line-index/src/lib.rs", "pub struct LineIndex"),
        runner: None,
        authority: None,
        bound: "Initial bound proposal: at most 8 Unicode scalars, 32 UTF-8 bytes, 4 lines, and every candidate byte and UTF-16 position.",
        assumptions: "Each harness uses the same source snapshot and independently specified byte/UTF-16 oracle required by #19300.",
        limitations: "The first slice covers perl-line-index only; ranges and edits require their actual owners and live consumers before extending the claim.",
        counterexample: "The minimal source, coordinate, expected typed result, and observed result are retained as a regression.",
        issue: "#19300",
    },
    Claim {
        id: "FV-PARSER-ROBUSTNESS-001",
        status: Status::Planned,
        evidence: Evidence::PropertyTest,
        subject: "parser progress and span validity",
        assertion: "Within declared generation budgets, parsing terminates, recovery progresses, and every emitted span remains inside its source.",
        target: source("crates/perl-parser/src/lib.rs", "pub use core::{"),
        runner: None,
        authority: None,
        bound: "Deterministic PR corpus, declared proptest cases and depth budgets, plus scheduled fuzz time budgets.",
        assumptions: "The harness drives the public production Parser facade and production recovery code.",
        limitations: "Generated evidence is not exhaustive over Perl programs or resource sizes and says nothing about semantic correctness.",
        counterexample: "The minimized source and seed are retained with the violated invariant.",
        issue: "#19306",
    },
    Claim {
        id: "FV-TRANSPORT-FRAMING-001",
        status: Status::Planned,
        evidence: Evidence::PropertyTest,
        subject: "shared Content-Length framing",
        assertion: "Valid streams are invariant under bounded chunking; malformed or oversized input fails without over-read, byte loss, overflow, or non-progress.",
        target: source(
            "crates/perl-lsp-rs-core/src/transport/framing.rs",
            "pub struct ContentLengthFramer",
        ),
        runner: None,
        authority: None,
        bound: "All chunk partitions in the deterministic bounded suite, generated malformed streams, and any separately recorded Kani bound.",
        assumptions: "LSP and DAP use the shared framer and the oracle is independent of its transition code.",
        limitations: "Framing evidence does not prove JSON-RPC semantics or async transport liveness.",
        counterexample: "A retained byte stream and chunk partition reproduce the differing output or invalid transition.",
        issue: "#19302",
    },
    Claim {
        id: "FV-COMPILER-IR-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "HIR/PIR identity, anchor, and graph invariants",
        assertion: "Within recorded graph bounds, production HIR/PIR IDs, references, ranges, anchors, places, effects, and CFG edges satisfy the independently stated structural predicate and lowering preserves it.",
        target: source("crates/perl-parser-core/src/pir/model.rs", "pub struct PirGraph"),
        runner: None,
        authority: None,
        bound: "Initial bound proposal: 4 items, 8 scopes, 16 nodes, 8 places, 24 edges, 8 blocks, 64 source bytes, nesting 4, and fan-out 4.",
        assumptions: "Validator soundness, producer preservation, and production-consumer correspondence are checked as separate obligations.",
        limitations: "The claim excludes complete Perl semantics, parser completeness, runtime equivalence, unbounded graphs, and cryptographic collision absence.",
        counterexample: "A minimized malformed or mis-lowered graph records the violated structural predicate and production path.",
        issue: "#19301",
    },
    Claim {
        id: "FV-SOURCE-IDENTITY-001",
        status: Status::Planned,
        evidence: Evidence::PropertyTest,
        subject: "durable source identity encoding",
        assertion: "Canonical inputs produce deterministic domain-separated IDs and length-prefix encoding rejects ambiguity and oversized fields before hashing.",
        target: source(
            "crates/perl-source-identity/src/lib.rs",
            "SOURCE_IDENTITY_V1_MAX_FIELD_BYTES",
        ),
        runner: None,
        authority: None,
        bound: "Independent reference-encoder properties, exhaustive short-field tuples, and any explicit Kani field-length bound.",
        assumptions: "SHA-256 is collision resistant; the proof concerns encoding and domain separation, not cryptographic injectivity.",
        limitations: "No finite test proves collision impossibility and downstream callers can misuse exposed strings.",
        counterexample: "The minimal canonical tuple, domain tag, and observed encodings or validation outcomes are retained.",
        issue: "#19303",
    },
    Claim {
        id: "FV-EVIDENCE-FINGERPRINT-001",
        status: Status::Planned,
        evidence: Evidence::PropertyTest,
        subject: "evidence-envelope fingerprint",
        assertion: "Fingerprints include declared identity fields, exclude non-identifying fields, and are stable under schema-declared order-insensitive insertion.",
        target: source(
            "crates/perl-evidence-envelope/src/lib.rs",
            "pub use fingerprint::EnvelopeFingerprint",
        ),
        runner: None,
        authority: None,
        bound: "Independent reference-encoder properties over bounded envelopes, collections, lineage edges, limitations, and schema versions.",
        assumptions: "The oracle does not reuse the production fingerprint implementation.",
        limitations: "A matching fingerprint does not prove that represented evidence is true or fresh.",
        counterexample: "A minimized pair of envelopes and canonical encodings demonstrates the unexpected equality or inequality.",
        issue: "#19303",
    },
    Claim {
        id: "FV-OPERATION-TRACE-001",
        status: Status::Planned,
        evidence: Evidence::BoundedModel,
        subject: "operation-trace allocator and recorder",
        assertion: "Within declared bounds, IDs remain unique per allocator, caps hold, terminal outcomes cannot be rewritten, and protected field material is not projected.",
        target: source("crates/perl-operation-trace/src/lib.rs", "pub use recorder::{"),
        runner: None,
        authority: None,
        bound: "Initial bound: 2 operations, 4 events each, 3 fields, 2 identity references, and every transition sequence.",
        assumptions: "The harness imports production allocation, recording, registry, and serialization paths.",
        limitations: "The model does not prove downstream context propagation or unbounded liveness.",
        counterexample: "A replayable event sequence records contexts, events, bounds, and the first invalid state or privacy projection.",
        issue: "#19304",
    },
    Claim {
        id: "FV-UNSAFE-SEMANTICS-001",
        status: Status::Planned,
        evidence: Evidence::MemorySafetyCheck,
        subject: "unsafe and FFI semantic obligations",
        assertion: "Every admitted production unsafe boundary has target-appropriate executable evidence for pointer, layout, allocator, environment, handle, and ABI obligations.",
        target: source("ci/unsafe_prod_policy.md", "aggregate ceiling is ten"),
        runner: None,
        authority: None,
        bound: "Every site in the production unsafe inventory, using Miri where supported and target-specific ABI or platform tests elsewhere.",
        assumptions: "The unsafe inventory is complete and external APIs satisfy their documented platform contracts.",
        limitations: "Miri cannot validate every OS or C FFI behavior and tested targets do not imply untested architectures.",
        counterexample: "The exact site, target, input, and Miri, sanitizer, or platform failure are retained.",
        issue: "#19307",
    },
];

#[derive(Serialize)]
struct Registry<'a> {
    schema_version: &'static str,
    source_commit: String,
    claims: &'a [Claim],
}

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live directly beneath the repository root")
}

fn read(root: &Path, source: SourceRef) -> Result<String> {
    let path = root.join(source.path);
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    ensure!(
        content.contains(source.marker),
        "marker {:?} missing from {}",
        source.marker,
        source.path
    );
    Ok(content)
}

fn source_commit(root: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn validate(root: &Path) -> Result<()> {
    ensure!(!CLAIMS.is_empty(), "formal claim denominator is empty");
    let mut ids = BTreeSet::new();

    for claim in CLAIMS {
        ensure!(
            claim.id.starts_with("FV-")
                && claim.id.chars().last().is_some_and(|ch| ch.is_ascii_digit()),
            "{}: invalid claim ID",
            claim.id
        );
        ensure!(ids.insert(claim.id), "{}: duplicate claim ID", claim.id);
        ensure!(
            claim.issue.len() > 1
                && claim.issue.starts_with('#')
                && claim.issue[1..].chars().all(|ch| ch.is_ascii_digit()),
            "{}: issue must be a #number",
            claim.id
        );
        ensure!(!claim.target.path.starts_with("archive/"), "{}: archive target", claim.id);
        read(root, claim.target).with_context(|| format!("{}: stale target", claim.id))?;

        for (name, value) in [
            ("subject", claim.subject),
            ("assertion", claim.assertion),
            ("bound", claim.bound),
            ("assumptions", claim.assumptions),
            ("limitations", claim.limitations),
            ("counterexample", claim.counterexample),
        ] {
            ensure!(!value.trim().is_empty(), "{}: empty {name}", claim.id);
            ensure!(!value.contains('\n'), "{}: multiline {name}", claim.id);
        }

        if claim.evidence == Evidence::BoundedModel {
            ensure!(
                claim.bound.chars().any(|ch| ch.is_ascii_digit()),
                "{}: bounded model has no numeric bound",
                claim.id
            );
        }

        let assertion = claim.assertion.to_ascii_lowercase();
        for forbidden in ["all possible", "fully proven", "unconditionally correct"] {
            ensure!(
                !assertion.contains(forbidden),
                "{}: unsupported universal wording {forbidden:?}",
                claim.id
            );
        }

        match claim.status {
            Status::Enforced => {
                ensure!(claim.runner.is_some(), "{}: enforced claim has no runner", claim.id);
                let authority = claim
                    .authority
                    .context(format!("{}: enforced claim has no authority", claim.id))?;
                read(root, authority).with_context(|| format!("{}: stale authority", claim.id))?;
            }
            Status::ImplementedUnwired => {
                ensure!(
                    claim.runner.is_some() && claim.authority.is_none(),
                    "{}: implemented-unwired metadata is inconsistent",
                    claim.id
                );
            }
            Status::Planned => {
                ensure!(
                    claim.runner.is_none() && claim.authority.is_none(),
                    "{}: planned claim advertises unearned evidence",
                    claim.id
                );
            }
        }
    }

    Ok(())
}

fn markdown() -> String {
    let mut output = String::from(
        "# Formal verification claims\n\n\
         `enforced` names a required authority; `implemented_unwired` has a checker but no merge authority; `planned` is not evidence.\n\n\
         | ID | Status | Evidence | Subject | Assertion | Bound | Issue |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for claim in CLAIMS {
        output.push_str(&format!(
            "| `{}` | `{:?}` | `{:?}` | {} | {} | {} | {} |\n",
            claim.id,
            claim.status,
            claim.evidence,
            escape(claim.subject),
            escape(claim.assertion),
            escape(claim.bound),
            claim.issue
        ));
    }
    output
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|")
}

fn usage() {
    eprintln!("usage: formal-verification-claims [--check | --json | --markdown]");
}

fn main() -> Result<()> {
    let root = repo_root()?;
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [] => "--check",
        [mode] if mode == "--check" => "--check",
        [mode] if mode == "--json" => "--json",
        [mode] if mode == "--markdown" => "--markdown",
        [mode] if mode == "--help" || mode == "-h" => {
            usage();
            return Ok(());
        }
        _ => {
            usage();
            bail!("expected exactly one supported mode");
        }
    };

    validate(&root)?;
    match mode {
        "--check" => println!(
            "{SCHEMA_VERSION}: valid; claims={}; head={}",
            CLAIMS.len(),
            source_commit(&root)
        ),
        "--json" => println!(
            "{}",
            serde_json::to_string_pretty(&Registry {
                schema_version: SCHEMA_VERSION,
                source_commit: source_commit(&root),
                claims: CLAIMS,
            })?
        ),
        "--markdown" => print!("{}", markdown()),
        _ => unreachable!("validated mode"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid() -> Result<()> {
        validate(&repo_root()?)
    }

    #[test]
    fn projections_cover_the_denominator() {
        let rendered = markdown();
        let json = serde_json::to_value(Registry {
            schema_version: SCHEMA_VERSION,
            source_commit: "test".to_owned(),
            claims: CLAIMS,
        })
        .expect("registry serializes");

        assert_eq!(json["claims"].as_array().map(Vec::len), Some(CLAIMS.len()));
        for claim in CLAIMS {
            assert!(rendered.contains(claim.id), "Markdown omits {}", claim.id);
        }
    }

    #[test]
    fn only_enforced_claims_name_authority() {
        for claim in CLAIMS {
            assert_eq!(
                claim.status == Status::Enforced,
                claim.authority.is_some(),
                "{} has inconsistent authority metadata",
                claim.id
            );
        }
    }
}
