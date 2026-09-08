# clippy::map_err_ignore — widest honest deny plan (#11246)

Toolchain pin: **1.95.0** (rust-toolchain.toml). Lint behavior is version-dependent; every
statement below is observed on that channel, Windows x86_64 MSVC host, default features.

## Ruling

The lint proposition is sound: "Error conversion must preserve diagnostic context" (#11335
catalog row, `tracked`). The census found **zero rows** where a payload-bearing cause is
silently discarded at a place with no boundary authority, and **zero rows** justifying plan D.
High volume (295) did not and cannot select warning or no-activation: the denominator
decomposes into small honest classes, not noise.

Selected plan: **C — staged cohorts converging to A** (global deny + exact local expectations),
because whole-crate activation requires each crate's *all-targets* denominator to be zero or
exactly excepted (workspace lints cannot scope lib-only), and 237 test-context rows are queued
behind #11736/#12000 tranche mechanics rather than blocking production enforcement.

## Denominator shape (295 unique sites)

| boundary class | rows | meaning | disposition |
|---|---:|---|---|
| `retain_cause` | 145 | cfg(test)/tests setup conversions discard the real setup cause into a String-typed test error (144), plus the slice's own deliberate lossy contrast fixture at `map_err_boundary_contract.rs:94` | repair in tests tranche T2; fixture = exact exception with removal condition |
| `classification_only` | 120 | source error carries no diagnostic payload beyond what the mapped value already states: `StripPrefixError`, `TryFromIntError`, thread-join panic fact, lock-poison class, CAS race loss, documented-impossible rejections, env-contract absence | exact exception |
| `stable_protocol_mapping` | 12 | JSON-RPC/LSP public code+message must remain stable (`invalid_params`, `JsonRpcError`); serde/ParseError internals withheld by protocol authority | exact exception |
| `redact_deliberately` | 10 | authenticated resolve-envelope surface: serde/authenticator internals must not leak into rejections or issues (trust boundary) | exact exception |
| `retain_class_not_details` | 5 | class retained, byte-level detail deliberately withheld at client/subprocess trust boundaries (DAP structured-value offsets, perltidy output encoding) or superseded by richer typed variants (`DocumentVersionDecodeError::OutOfRange`, `CorpusTopologyError::PathOutsideRoot`) | exact exception |
| `independent_error_model_defect` | 3 | mapped model lacks a payload carrier for real diagnostic content (see leaves) | separate leaves |

Strongest falsifier checks against dishonest repairs (issue negative controls):

- No row was classified by count. The 144-row test block is one mechanism (String error type
  in test modules), not 144 judgments.
- No proposed repair binds `|error|` without using it (falsifier 6): classification_only rows
  get reasoned `#[expect]`s or owned adapters, not renamed closures.
- No row adds unconditional logging or boxed generic sources.
- Redaction rows keep messages stable and non-leaking; the contrast tests pin this.
- The slice's own footprint is inside its own denominator: the retain-cause control's
  deliberate lossy form is row 295 (`cohort CTRL`) and must gain an exact reasoned
  `#[expect(clippy::map_err_ignore, reason = ...)]` when perl-lsp-rs-core activates.

## Cohorts

### C0 — activate deny now (26 crates, zero findings on all targets)

perl-ast-v2, perl-token, perl-source-identity, perl-pragma, perl-regex, perl-parser-bench,
perl-core-harness-types, perl-core-test-runner, perl-parser-pest, perl-parser-comparison,
perl-semantic-facts, perl-tdd-support, perl-test-must, perl-test-generators, perl-test-facts,
perl-lsp-perltidy, perl-diagnostics, perl-symbol, perl-line-index, perl-pod, perl-ripr-facts,
perllsp, tree-sitter-perl-c, perl-ci-hygiene, perl-kwalitee, perl-workspace-core.

Currentness condition before activation: rerun the corrected-instrument census on current
main; any new finding moves the crate to C1 instead of weakening the lint.

### C1 — production residual (58 rows: P1=45, T1=8, TS=2, LEAF=3)

Each crate joins when its rows are resolved as either mechanical retain-cause repairs or
exact reasoned `#[expect(..., reason = ...)]` rows under CLIPPY_POLICY.md suppression law:

- protocol/redaction/classification rows (P1): expectation per row or owning adapter;
  owner: restriction train #11337 through packet compiler #11257.
- LEAF rows are blocked on separately filed error-model leaves below; their crates join only
  after those land.

### C2 — test-context tranches (237 rows, cohort T2)

Ride the #11736 kernel-cohort admission mechanics and #12000's perl-lsp-rs-core debt tranches:
a crate's test-context `map_err_ignore` rows repair alongside its existing all-targets work so
no crate pays two admission passes. Owner: #11736/#12000 trains + #11337.

### Global deny (plan A completion)

When C0+C1+C2 are consumed, flip `[workspace.lints.clippy] map_err_ignore = "deny"` and move
the catalog row from `tracked` to `active`; accepted boundary rows become exact local
expectations. That activation PR owns the ledger transition through #11335/#11404 coherence
(`cargo xtask check-lint-policy`).

Plan D rejected: no accepted #11335 semantic rejection exists or is warranted — the lint's
false-positive surface (payload-free sources) is representable through exact expectations.

## Filed leaves (through #11257)

1. Activation leaf: [#12598](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12598)
   — staged `clippy::map_err_ignore` deny cohorts C0→C1→C2→A.
2. Error-model defect: [#12600](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12600)
   — `perl-corpus` sidecar `relative_identity` drops path/root identity that sibling
   `asset_from_path` retains.
3. Error-model defect: [#12601](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12601)
   — `ProviderAdapterError::MalformedEnvelope(fact_id)` drops the envelope validator's
   invariant reason.
4. Error-model defect: [#12602](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12602)
   — `TreeError::ParseFailed` carries no parser error position.

## NOT_PROVEN rows

- Linux x86_64 GNU and macOS ARM64 required-host observations (#11225 owns hosted subjects):
  not runnable from this host; recorded unresolved_non_green at platform level, not per-finding.
- Feature-superset / GA-lock subjects (#11222 projection): unlanded; default-features census
  only, per this issue's subject availability stop-condition.

## Contrast controls landed here

`crates/perl-lsp-rs-core/tests/map_err_boundary_contract.rs` pins the strongest cases:

- deliberate redaction: malformed resolve-envelope tokens reject as coarse
  `ResolveEnvelopeRejection` variants without echoing wire bytes or serde internals; JSON-RPC
  overflow mapping keeps its exact stable INVALID_PARAMS message.
- honest classification: `decode_version_value` maps an out-of-range value into sign-aware
  typed variants (richer than the discarded `TryFromIntError`), demonstrating the correct
  non-theater form the exceptions protect.
- retain-cause repair shape: binding-and-preserving versus discarding contrast, the reference
  repair for every cohort row marked `retain_cause`; #12600 records a live lossy instance.

Removal/review condition for every non-enforced row: consumed when its cohort activates; the
denominator.csv spans go stale on any edit to cited lines — rerun the census command from
context.md before consuming any row.
