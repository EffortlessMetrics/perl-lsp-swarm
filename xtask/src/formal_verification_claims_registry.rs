//! Strict source data for the formal-verification claim registry.
//!
//! Kept in a Rust-owned file so adding or changing the denominator does not
//! create a parallel non-Rust policy surface. The validator parses this as
//! JSON with `deny_unknown_fields` at every schema layer.

pub const REGISTRY_JSON: &str = r##"{
  "schema_version": "formal_verification_claims.v1",
  "claims": [
    {
      "id": "FV-INFRA-001",
      "status": "enforced",
      "evidence": "structural_invariant",
      "subject": "formal-verification claim registry",
      "assertion": "Every row has a unique ID, live target, explicit evidence class, scope, assumptions, limitations, replay contract, and truthful enforcement state.",
      "target": {
        "path": "xtask/src/formal_verification_claims_registry.rs",
        "marker": "\"schema_version\": \"formal_verification_claims.v1\""
      },
      "runner": "cargo run -p xtask --bin formal-verification-claims --locked -- --check",
      "authority": {
        "kind": "source_marker",
        "source": {
          "path": "scripts/ci/required-merge-surface.sh",
          "marker": "cargo run -p xtask --bin formal-verification-claims --locked -- --check"
        }
      },
      "bound": "The strict registry rows, validator, and consulted Git blobs at the evaluated commit; all 16 inventory controls are selected by the Linux canonical merge surface.",
      "assumptions": "Source markers are structural bindings; the existing CI admission and execution contracts remain dependencies.",
      "limitations": "Inventory validation is not a native proof result. Enforced records the configured canonical route, not a successful run on the current candidate. Complete native proof closure and receipt admission remain #19297 work.",
      "counterexample": "The checker exits non-zero and names the malformed row or stale binding; canonical CI refuses zero, skipped, filtered, or missing named controls.",
      "issue": "#19297"
    },
    {
      "id": "FV-BUILD-001",
      "status": "enforced",
      "evidence": "build_check",
      "subject": "workspace compilation surface",
      "assertion": "The locked workspace compiles for the target and feature surface selected by the canonical all-target command.",
      "target": {
        "path": "Cargo.toml",
        "marker": "rust-version = \"1.95\""
      },
      "runner": "just check-all-targets",
      "authority": {
        "kind": "source_marker",
        "source": {
          "path": ".github/workflows/ci.yml",
          "marker": "      - name: Compile all targets (catches integration-test + bench bit-rot)\n        run: just check-all-targets"
        }
      },
      "bound": "The targets, features, hosts, and Rust toolchain selected by current CI.",
      "assumptions": "Cargo.lock and the checked-out sources are authoritative inputs.",
      "limitations": "Compilation is not semantic correctness, memory safety, race freedom, or liveness.",
      "counterexample": "The required job fails with the compiler diagnostic for the evaluated commit.",
      "issue": "#19297"
    },
    {
      "id": "FV-WORKSPACE-OWNERSHIP-001",
      "status": "enforced",
      "evidence": "structural_invariant",
      "subject": "workspace-index lifecycle ownership",
      "assertion": "Every inventoried lifecycle proposition has one selected owner and lifecycle-adjacent declarations cannot drift outside the checked denominator.",
      "target": {
        "path": "policy/workspace-index-lifecycle-propositions.v1.tsv",
        "marker": "# schema: workspace_index_lifecycle_propositions.v1"
      },
      "runner": "cargo test -p xtask --test workspace_index_lifecycle_ledger --locked -- --nocapture",
      "authority": {
        "kind": "source_marker",
        "source": {
          "path": "scripts/ci/required-merge-surface.sh",
          "marker": "cargo test -p xtask --test workspace_index_lifecycle_ledger --locked -- --nocapture"
        }
      },
      "bound": "The proposition, type, member, source-marker, and generated-projection ledgers.",
      "assumptions": "Ledger source markers continue to bind the live declarations.",
      "limitations": "This proves ownership and drift control, not concurrent currentness, safety, or liveness.",
      "counterexample": "The ledger test reports the duplicate owner, missing declaration, stale marker, or stale projection.",
      "issue": "#19298"
    },
    {
      "id": "FV-UNSAFE-RATCHET-001",
      "status": "enforced",
      "evidence": "structural_invariant",
      "subject": "production unsafe syntax ratchet",
      "assertion": "Counted production unsafe syntax does not increase above the executable baseline and every counted occurrence carries adjacent SAFETY reasoning.",
      "target": {
        "path": "ci/check_unsafe_prod.sh",
        "marker": "ci_exec_hygiene check-unsafe-prod"
      },
      "runner": "bash ci/check_unsafe_prod.sh",
      "authority": {
        "kind": "required_gate",
        "policy": {
          "path": ".ci/gate-policy.yaml",
          "marker": "  - name: unsafe_prod_check\n    tier: merge_gate\n    description: \"Fail when a production unsafe block lacks // SAFETY: reasoning or the unsafe-prod baseline rises\"\n    required: true\n    command: bash ci/check_unsafe_prod.sh"
        },
        "workflow": {
          "path": ".github/workflows/ci.yml",
          "marker": "gates: docs_agents_contract_workflows unsafe_prod_check msrv_authority_sync"
        }
      },
      "bound": "The production/test selection and numeric syntax baseline owned by perl-ci-hygiene and ci/unsafe_prod_baseline.txt.",
      "assumptions": "The checker completely classifies the Rust files in its policy scope.",
      "limitations": "The syntax baseline is 12 while policy describes ten semantic sites; the ratchet does not prove soundness or reconcile that mapping.",
      "counterexample": "The checker identifies an over-ceiling, unclassified, or undocumented unsafe site.",
      "issue": "#19307"
    },
    {
      "id": "FV-WORKSPACE-CURRENTNESS-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "workspace generation and settlement protocol",
      "assertion": "Only an admitted operation for the accepted workspace generation may publish; stale and duplicate settlements cannot become current.",
      "target": {
        "path": "crates/perl-workspace/src/workspace/workspace_index.rs",
        "marker": "pub struct IndexCoordinator"
      },
      "bound": "Initial bound: 2 overlapping operations, 3 generations, 1 replacement, and every explored interleaving.",
      "assumptions": "Production uses a checked transition kernel or has a tested trace-equivalence bridge to it.",
      "limitations": "The initial model excludes filesystem, parser, and executor liveness.",
      "counterexample": "A replayable transition trace records admission, generation, settlement, publication, and the first violation.",
      "issue": "#19298"
    },
    {
      "id": "FV-SCHEDULER-FRESHNESS-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "LSP read freshness and cancellation",
      "assertion": "A position-sensitive read is delivered only while its document instance and generation remain valid through final delivery.",
      "target": {
        "path": "crates/perl-lsp-rs/src/runtime/scheduler.rs",
        "marker": "pub(crate) fn is_read_stale"
      },
      "bound": "Initial bound: 2 document instances, 3 generations, 2 reads, 2 mutations, 1 close/reopen, and every explored schedule.",
      "assumptions": "The harness uses production freshness and extracted production queue transitions.",
      "limitations": "The model excludes provider correctness and executor fairness.",
      "counterexample": "A minimized schedule records ingress, mutation barriers, instance identity, and the incorrect delivery decision.",
      "issue": "#19299"
    },
    {
      "id": "FV-SCHEDULER-SHUTDOWN-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "LSP scheduler ordering and shutdown",
      "assertion": "Mutation barriers, bounded queues, cancellation, and cooperative shutdown do not strand accepted work or deliver after terminal cancellation within the model.",
      "target": {
        "path": "crates/perl-lsp-rs/src/runtime/scheduler.rs",
        "marker": "pub(crate) struct Scheduler"
      },
      "bound": "Initial bound: queue capacity 2, 2 reads, 2 mutations, 1 cancellation, 1 shutdown, and every explored schedule.",
      "assumptions": "Blocking work is represented by an explicit completion event.",
      "limitations": "Cooperative shutdown cannot abort arbitrary blocking work and gives no wall-clock bound.",
      "counterexample": "A replayable scheduler trace records queue, barrier, cancellation, worker, and response states.",
      "issue": "#19299"
    },
    {
      "id": "FV-POSITION-LAWS-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "byte, UTF-16, range, and edit laws",
      "assertion": "Within the recorded symbolic domain, production position conversion preserves its declared byte and UTF-16 clamping and rejection contracts and cannot overflow or escape source bounds.",
      "target": {
        "path": "crates/perl-line-index/src/lib.rs",
        "marker": "pub struct LineIndex"
      },
      "bound": "Initial bound proposal: at most 8 Unicode scalars, 32 UTF-8 bytes, 4 lines, and every candidate byte and UTF-16 position.",
      "assumptions": "Each harness uses the same source snapshot and independently specified byte/UTF-16 oracle required by #19300.",
      "limitations": "The first slice covers perl-line-index only; ranges and edits require their actual owners and live consumers before extending the claim.",
      "counterexample": "The minimal source, coordinate, expected typed result, and observed result are retained as a regression.",
      "issue": "#19300"
    },
    {
      "id": "FV-PARSER-ROBUSTNESS-001",
      "status": "planned",
      "evidence": "property_test",
      "subject": "parser progress and span validity",
      "assertion": "Within declared generation budgets, parsing terminates, recovery progresses, and every emitted span remains inside its source.",
      "target": {
        "path": "crates/perl-parser/src/lib.rs",
        "marker": "pub use core::{"
      },
      "bound": "Deterministic PR corpus, declared proptest cases and depth budgets, plus scheduled fuzz time budgets.",
      "assumptions": "The harness drives the public production Parser facade and production recovery code.",
      "limitations": "Generated evidence is not exhaustive over Perl programs or resource sizes and says nothing about semantic correctness.",
      "counterexample": "The minimized source and seed are retained with the violated invariant.",
      "issue": "#19306"
    },
    {
      "id": "FV-TRANSPORT-FRAMING-001",
      "status": "planned",
      "evidence": "property_test",
      "subject": "shared Content-Length framing",
      "assertion": "Valid streams are invariant under bounded chunking; malformed or oversized input fails without over-read, byte loss, overflow, or non-progress.",
      "target": {
        "path": "crates/perl-lsp-rs-core/src/transport/framing.rs",
        "marker": "pub struct ContentLengthFramer"
      },
      "bound": "All chunk partitions in the deterministic bounded suite, generated malformed streams, and any separately recorded Kani bound.",
      "assumptions": "LSP and DAP use the shared framer and the oracle is independent of its transition code.",
      "limitations": "Framing evidence does not prove JSON-RPC semantics or async transport liveness.",
      "counterexample": "A retained byte stream and chunk partition reproduce the differing output or invalid transition.",
      "issue": "#19302"
    },
    {
      "id": "FV-COMPILER-IR-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "HIR/PIR identity, anchor, and graph invariants",
      "assertion": "Within recorded graph bounds, production HIR/PIR IDs, references, ranges, anchors, places, effects, and CFG edges satisfy the independently stated structural predicate and lowering preserves it.",
      "target": {
        "path": "crates/perl-parser-core/src/pir/model.rs",
        "marker": "pub struct PirGraph"
      },
      "bound": "Initial bound proposal: 4 items, 8 scopes, 16 nodes, 8 places, 24 edges, 8 blocks, 64 source bytes, nesting 4, and fan-out 4.",
      "assumptions": "Validator soundness, producer preservation, and production-consumer correspondence are checked as separate obligations.",
      "limitations": "The claim excludes complete Perl semantics, parser completeness, runtime equivalence, unbounded graphs, and cryptographic collision absence.",
      "counterexample": "A minimized malformed or mis-lowered graph records the violated structural predicate and production path.",
      "issue": "#19301"
    },
    {
      "id": "FV-SOURCE-IDENTITY-001",
      "status": "planned",
      "evidence": "property_test",
      "subject": "durable source identity encoding",
      "assertion": "Canonical inputs produce deterministic domain-separated IDs and length-prefix encoding rejects ambiguity and oversized fields before hashing.",
      "target": {
        "path": "crates/perl-source-identity/src/lib.rs",
        "marker": "SOURCE_IDENTITY_V1_MAX_FIELD_BYTES"
      },
      "bound": "Independent reference-encoder properties, exhaustive short-field tuples, and any explicit Kani field-length bound.",
      "assumptions": "SHA-256 is collision resistant; the proof concerns encoding and domain separation, not cryptographic injectivity.",
      "limitations": "No finite test proves collision impossibility and downstream callers can misuse exposed strings.",
      "counterexample": "The minimal canonical tuple, domain tag, and observed encodings or validation outcomes are retained.",
      "issue": "#19303"
    },
    {
      "id": "FV-EVIDENCE-FINGERPRINT-001",
      "status": "planned",
      "evidence": "property_test",
      "subject": "evidence-envelope fingerprint",
      "assertion": "Fingerprints include declared identity fields, exclude non-identifying fields, and are stable under schema-declared order-insensitive insertion.",
      "target": {
        "path": "crates/perl-evidence-envelope/src/lib.rs",
        "marker": "pub use fingerprint::EnvelopeFingerprint"
      },
      "bound": "Independent reference-encoder properties over bounded envelopes, collections, lineage edges, limitations, and schema versions.",
      "assumptions": "The oracle does not reuse the production fingerprint implementation.",
      "limitations": "A matching fingerprint does not prove that represented evidence is true or fresh.",
      "counterexample": "A minimized pair of envelopes and canonical encodings demonstrates the unexpected equality or inequality.",
      "issue": "#19303"
    },
    {
      "id": "FV-OPERATION-TRACE-001",
      "status": "planned",
      "evidence": "bounded_model",
      "subject": "operation-trace allocator and recorder",
      "assertion": "Within declared bounds, IDs remain unique per allocator, caps hold, terminal outcomes cannot be rewritten, and protected field material is not projected.",
      "target": {
        "path": "crates/perl-operation-trace/src/lib.rs",
        "marker": "pub use recorder::{"
      },
      "bound": "Initial bound: 2 operations, 4 events each, 3 fields, 2 identity references, and every transition sequence.",
      "assumptions": "The harness imports production allocation, recording, registry, and serialization paths.",
      "limitations": "The model does not prove downstream context propagation or unbounded liveness.",
      "counterexample": "A replayable event sequence records contexts, events, bounds, and the first invalid state or privacy projection.",
      "issue": "#19304"
    },
    {
      "id": "FV-UNSAFE-SEMANTICS-001",
      "status": "planned",
      "evidence": "memory_safety_check",
      "subject": "unsafe and FFI semantic obligations",
      "assertion": "Every admitted production unsafe boundary has target-appropriate executable evidence for pointer, layout, allocator, environment, handle, and ABI obligations.",
      "target": {
        "path": "ci/unsafe_prod_policy.md",
        "marker": "aggregate ceiling is ten"
      },
      "bound": "Every site in the production unsafe inventory, using Miri where supported and target-specific ABI or platform tests elsewhere.",
      "assumptions": "The unsafe inventory is complete and external APIs satisfy their documented platform contracts.",
      "limitations": "Miri cannot validate every OS or C FFI behavior and tested targets do not imply untested architectures.",
      "counterexample": "The exact site, target, input, and Miri, sanitizer, or platform failure are retained.",
      "issue": "#19307"
    }
  ]
}"##;
