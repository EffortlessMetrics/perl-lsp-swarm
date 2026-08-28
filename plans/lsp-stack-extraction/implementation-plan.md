# lsp-stack Extraction Implementation Plan

Status: in progress
Owner: perl-lsp maintainers
Linked ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Linked spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Static seam audit: [static-seam-audit.md](static-seam-audit.md)

## Purpose

Define the staged PR sequence for a future `lsp-stack` extraction.

This plan exists so future agents know the boundary before they move code. The
current app remains the shipping implementation until a later extraction PR
proves parity.

## Current-app Hardening Prerequisite

Extraction starts only after current protocol, runtime, and editor-doc
hardening is complete and green on `main`.

The prerequisite baseline is:

- inline-completion registration mode is correct for static, dynamic, and
  disabled clients
- LSP 3.18 inline-completion params coverage exists
- watcher registration honors lean/e2e runtime tuning
- semantic tokens advertise full-only support until delta is implemented
- raw RPC and lean editor receipts exist for the current app
- editor docs describe LSP4IJ, Neovim lean/e2e mode, standard inline
  completion, and `perlInlineCompletionStream`

This baseline is a regression target, not a release-readiness claim.

## Boundary

Future `lsp-stack` work may extract only language-neutral LSP infrastructure:

- JSON-RPC envelopes and request-id handling
- LSP framing and transport helpers
- server-originated request helpers
- capability-shape and dynamic-registration primitives
- lifecycle, tuning, cancellation, and scheduling primitives that do not encode
  Perl behavior
- protocol contract test helpers

Future `lsp-stack` work must not include:

- Perl parser, lexer, semantic analysis, or workspace indexing
- Perl module resolution or provider facts
- inline-completion provider behavior
- `features.toml` Perl feature policy
- DAP or Perl debugger behavior
- perltidy, subprocess runtime, packaging, signing, publishing, marketplace, or
  release automation

Future `lsp-stack` must have no Perl dependencies.

## PR Sequence

### PR 1: Boundary docs

Status: landed as `06329ff7`

Goal:

Add the ADR, spec, and implementation plan.

Allowed files:

- `docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md`
- `docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md`
- `plans/lsp-stack-extraction/implementation-plan.md`

Non-goals:

- no `crates/lsp-stack`
- no code movement
- no generic handler traits
- no routing rewrite
- no inline-completion behavior change
- no DAP change
- no release, publish, signing, or package change

Proof:

```bash
git diff --check
cargo xtask docs-check
cargo xtask check-support-claims
```

If an xtask command is unavailable or unstable, report that separately.

Rollback:

Revert the docs-only commit. No runtime rollback is needed because no code
moved.

### PR 2: Static seam audit

Status: candidate in [#13097](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13097)

Goal:

Classify candidate protocol/runtime files as language-neutral, Perl-specific,
mixed, or not extractable and select one exact first extraction unit.

Audit:

- [static-seam-audit.md](static-seam-audit.md)

Allowed changes:

- docs or generated audit report only
- no code movement

Acceptance:

- every candidate names its current tests
- every mixed file names the dependency or policy that blocks extraction
- one low-risk language-neutral first unit is selected
- the next dependency-boundary PR has exact allowed and forbidden dependencies
- the initial external-consumer contract is explicit
- the minimum useful non-Perl consumer slice is defined separately from the
  first mechanical move

Proof:

```bash
git diff --check
cargo xtask docs-check
cargo xtask check-support-claims
```

Rollback:

Revert the audit doc and this plan update. Do not move code based on a reverted
audit.

### First source-change gate

PR 3 is the first planned source-changing extraction-preparation PR. Before it
lands, its review record must confirm the ADR's current-app baseline:

- static, dynamic, and disabled inline-completion clients select exactly the
  intended registration mode
- lean/e2e watcher tuning does not register file watchers or suppress dynamic
  inline-completion registration
- semantic-token capabilities remain full-only without result-id-backed delta
- raw-RPC and lean-editor receipts remain current
- editor docs still distinguish standard inline completion from
  `perlInlineCompletionStream`

Use current, unaffected evidence when it remains valid. Do not rebase, replay
broad CI, or create an empty commit only to make evidence attach to a newer
head. Re-run the proof whose semantic subject changes, and identify the commit
or receipt each reused result actually evaluated.

### PR 3: `JsonRpcId` dependency boundary

Goal:

Prepare `JsonRpcId` as an independently compilable, language-neutral source
unit without creating `crates/lsp-stack` or changing its public path.

Allowed changes:

- split `JsonRpcId` and its focused tests into an in-place protocol submodule
- preserve `perl_lsp_rs_core::protocol::JsonRpcId` as the public path
- add a focused dependency-boundary assertion or compile probe
- no cross-crate production-code movement
- no runtime or wire behavior change

Acceptance:

- the candidate unit depends only on `std`, `serde`, and `serde_json`
- no `perl-*`, provider, parser, DAP, feature-catalog, tracing, runtime,
  release, or package dependency reaches the candidate unit
- all current callers compile through the unchanged public path
- focused tests directly cover `JsonRpcId` serde and helper conversions rather
  than relying only on request/response envelopes
- integer and string IDs retain round-trip behavior
- null, fractional, and out-of-domain numeric IDs remain rejected
- the move/re-export shape for PR 5 is documented
- the first source-change gate is satisfied in the PR review record

Proof:

```bash
git diff --check
./scripts/cargo-safe test -p perl-lsp-rs-core json_rpc_ --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
```

Rollback:

Revert the in-place split and dependency assertion. Keep the candidate in the
current app until the dependency boundary is clean.

### PR 4: Crate scaffold

Goal:

Create `crates/lsp-stack` only after PRs 1-3 and the first source-change gate
land.

Allowed changes:

- workspace metadata for the new crate
- minimal crate files
- no moved production behavior

Acceptance:

- the crate has no Perl dependencies
- the crate compiles independently
- no runtime behavior changes
- no publication or stability claim is introduced

Proof:

```bash
git diff --check
./scripts/cargo-safe check -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
```

Rollback:

Revert the scaffold PR. Current app behavior must remain unchanged.

### PR 5: First protocol primitive move

Goal:

Move `JsonRpcId` and its strict serde boundary with no behavior change.

Candidate unit:

- `JsonRpcId::{Integer, String}`
- strict untagged serialization and deserialization
- `from_value`, `try_from_value`, `to_value`, and display behavior
- focused ID round-trip and rejection tests

Acceptance:

- `lsp_stack::jsonrpc::JsonRpcId` owns the implementation
- `perl_lsp_rs_core::protocol::JsonRpcId` remains a compatibility re-export
- a crate integration test consumes `JsonRpcId` through only the public
  `lsp_stack` path
- existing current-app tests pass
- moved tests prove the same parse/serialize behavior
- no capability, registration, provider, transport, or release behavior changes
- no Perl dependency reaches `lsp-stack`
- this establishes an internal workspace public API only; publication and a
  stability contract remain separate work

Proof:

```bash
./scripts/cargo-safe test -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
git diff --check
```

Rollback:

Revert the move and restore the current implementation behind the same public
path. Keep any added regression test if it captures a real bug.

### PR 6: Transport/framing move

Goal:

Move language-neutral message framing only after protocol primitive parity is
proven.

Acceptance:

- byte framing is separated from JSON decode, response routing, tracing, and
  current malformed-frame recovery policy before movement
- no Perl provider or parser-category import reaches the moved unit
- request/response framing tests pass
- raw RPC receipt remains green

Proof:

```bash
./scripts/cargo-safe test -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
git diff --check
```

Rollback:

Revert the transport move and restore current-app framing paths.

### PR 7: Runtime primitive move

Goal:

Move cancellation, server-request correlation, request-id allocation, or
scheduling primitives only when they are language-neutral.

Acceptance:

- file-watcher tuning still gates only file watchers
- inline-completion dynamic registration is not suppressed by watcher tuning
- generation-aware stale-read cancellation still passes existing receipts
- provider cleanup, document generation, feature priority, and current product
  policy remain in `perl-lsp`

Proof:

```bash
./scripts/cargo-safe test -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_neovim_lean_startup_trace -- --test-threads=1 --nocapture
git diff --check
```

Rollback:

Revert the runtime primitive move. Do not weaken cancellation, request
correlation, watcher, or stale-read tests.

### PR 8: Minimum useful non-Perl consumer

Goal:

Prove that the extracted public surface supports one bounded protocol exchange
without importing Perl product state.

Fixture contract:

- lives outside `perl-lsp-rs` and `perl-lsp-rs-core`
- consumes only public `lsp_stack` APIs
- accepts one bounded Content-Length-framed request
- deserializes the JSON-RPC request and preserves its ID
- emits a success or error response with the same ID
- issues and correlates one server-to-client request
- has no Perl, provider, feature-catalog, DAP, editor, package, or release
  dependency

Acceptance:

- the fixture passes as a crate integration test or dedicated non-Perl fixture
  crate
- the dependency boundary is machine-checked
- the claim is limited to the proven protocol slice
- no crates.io publication, general runtime, or stability claim is made

Proof:

```bash
./scripts/cargo-safe test -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe check -p lsp-stack --all-targets --profile agent --locked
git diff --check
```

Rollback:

Revert the fixture or the smallest extracted primitive that cannot satisfy it.
Do not repair the fixture by importing Perl product code.

### PR 9: Current-app integration cleanup

Goal:

Remove duplicate wrappers only after extracted primitives and the non-Perl
consumer are proven and the current app has parity.

Acceptance:

- current app remains the product surface
- no release, package, signing, or marketplace behavior changes
- docs still describe current editor behavior accurately

Proof:

```bash
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cap_snap --profile agent --locked
git diff --check
```

Rollback:

Revert cleanup only. Do not revert earlier extraction PRs unless the regression
is in the extracted primitive.

## Always Invalid In This Lane

- creating `crates/lsp-stack` before the boundary and audits land
- moving code in the boundary-docs or static-audit PR
- adding Perl dependencies to future `lsp-stack`
- changing inline-completion behavior while extracting infrastructure
- changing DAP while extracting LSP infrastructure
- changing release, publish, signing, marketplace, or package behavior
- claiming release readiness from extraction work
- claiming a generally reusable stack from `JsonRpcId` or scaffold work alone
- deleting current-app proof commands to make extraction pass

## Reporting Requirements

Every future extraction PR must say:

- what moved or changed
- what did not move
- whether runtime behavior changed
- whether JSON-RPC or capability JSON changed
- whether dynamic registration changed
- whether dependencies changed
- whether a public compatibility re-export changed
- whether external-consumer proof changed
- whether publication or stability claims changed
- whether release or package surfaces changed
- which proof commands passed
- which current unaffected evidence was reused and what commit or receipt it
  evaluated
- which unrelated warnings were pre-existing
