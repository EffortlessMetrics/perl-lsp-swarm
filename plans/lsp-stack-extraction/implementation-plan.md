# lsp-stack Extraction Implementation Plan

Status: planned
Owner: perl-lsp maintainers
Linked ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Linked spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Static seam audit: [PR 2 audit](static-seam-audit.md)

## Purpose

Define the PR sequence for a future `lsp-stack` extraction without starting the
extraction in this PR.

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
- semantic-token delta is advertised only with result-id state and parity proof
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

Status: landed on `main`

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
just ci-docs-check
cargo xtask check-support-claims
```

If an xtask command is unavailable or unstable, report that separately.

Rollback:

Revert the docs-only PR. No runtime rollback is needed because no code moved.

### PR 2: Static seam audit

Status: candidate in [#13084](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13084), tracked by [#13054](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13054)

Audit artifact: [static seam audit](static-seam-audit.md)

Goal:

Classify candidate protocol/runtime files as language-neutral, Perl-specific,
mixed, or not extractable.

Allowed changes:

- docs or generated audit report only
- no code movement

Acceptance:

- every candidate names its current tests
- every mixed file names the Perl dependency that blocks extraction
- every proposed first move is low-risk and language-neutral

Proof:

```bash
git diff --check
just ci-docs-check
```

Rollback:

Revert the audit doc. Do not move code based on a reverted audit.

### PR 3: Dependency boundary audit

Goal:

Determine whether the first candidate extraction set is dependency-clean.
Record a blocker when the preferred candidate remains mixed; do not convert an
audit result into an independent-compilation claim.

Audit input:

- record JSON-RPC error classification as a blocker; do not treat
  `protocol/jsonrpc.rs` as dependency-clean until a later dependency-boundary
  PR reconciles the application classification seam
- record the `$/perl-lsp/clientResponse` compatibility shim outside low-level
  framing
- verify the source again at the PR's own base; the PR 2 audit is a reviewed
  classification, not build proof

Allowed changes:

- docs or build metadata experiments only when they do not create
  `crates/lsp-stack`
- no production code movement

Acceptance:

- the exact candidate source set and its direct external references are
  recorded
- a clean candidate records its language-neutral reference set; a mixed
  candidate records every blocker and the intended post-preparation set
- provider, parser, DAP, release, package, and compatibility-policy references
  are classified rather than silently omitted
- the intended post-preparation dependency closure is recorded as a target, not
  as a current fact
- no claim of independent compilation or Perl-free closure is made while a
  blocker remains

Proof:

```bash
git diff --check
just ci-docs-check
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
```

The compile check is supporting evidence for the current application. It does
not prove that a mixed candidate compiles independently.

Rollback:

Revert the audit. Keep candidates in the current app until the dependency
boundary is clean.

### Pre-scaffold dependency preparation

Status: required only when PR 3 proves a preferred candidate is still mixed

Goal:

Remove the specific Perl dependency documented by PR 3 without moving the
candidate or creating `crates/lsp-stack`.

Allowed changes:

- one bounded no-behavior-change cleanup in current application/core code
- affected tests and error-classification inventory
- no file movement into a new crate
- no capability, provider, routing, editor, DAP, release, or package change

Acceptance:

- the candidate no longer imports the documented Perl dependency
- application-owned classification behavior remains proven
- the intended language-neutral dependency closure is re-checked at the
  candidate head

Proof:

```bash
git diff --check
./scripts/cargo-safe test -p perl-lsp-rs-core protocol::jsonrpc --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
```

Rollback:

Revert the dependency preparation and keep the candidate in the current app.
Do not create the scaffold while the documented blocker remains.

### PR 4: Current-app test baseline

Goal:

Re-prove the current application's protocol, runtime, scheduling, lifecycle,
raw-RPC, and lean-editor baseline after the audits and any required dependency
preparation, before a new crate boundary exists.

Allowed changes:

- test-only changes, fixtures, and receipt documentation that make existing
  behavior explicit
- no new product behavior
- no `crates/lsp-stack`
- no production file movement
- any behavior defect exposed by the baseline is repaired in a separate bounded
  PR before this gate is declared green

Acceptance:

- protocol, capability, registration, lifecycle, and scheduler proof is green
- generation-aware stale-read cancellation remains proven
- raw-RPC and synthetic Neovim-shaped lean receipts are green
- the exact current-app commit used as the extraction baseline is recorded
- synthetic receipts do not promote an actual-host editor support claim

Proof:

```bash
git diff --check
./scripts/cargo-safe test -p perl-lsp-rs --lib --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_3_17_lifecycle_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cap_snap --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_neovim_lean_startup_trace -- --test-threads=1 --nocapture
```

Receipt ownership and claim limits are documented in
[`docs/project/status/neovim_latency.md`](../../docs/project/status/neovim_latency.md).

Rollback:

Revert only the test/fixture/receipt changes from this step. Do not create the
scaffold while any required current-app baseline proof is red or not proven.

### PR 5: Crate scaffold

Goal:

Create `crates/lsp-stack` only after PRs 1-4 and any required dependency
preparation land.

Allowed changes:

- workspace metadata for the new crate
- minimal crate files
- no moved production behavior

Acceptance:

- the crate has no Perl dependencies
- the crate compiles independently
- no runtime behavior changes

Proof:

```bash
git diff --check
./scripts/cargo-safe check -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
```

Rollback:

Revert the scaffold PR. Current app behavior must remain unchanged.

### PR 6: First protocol primitive move

Goal:

Move one language-neutral protocol primitive with no behavior change.

Candidate class:

- JSON-RPC ID parsing or request envelope utilities
- only if the dependency audit proves the type has no Perl dependency

Acceptance:

- existing current-app tests pass
- moved tests prove the same parse/serialize behavior
- no capability or provider behavior changes

Proof:

```bash
./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
git diff --check
```

Rollback:

Revert the move and restore imports. Keep any added regression test if it
captures a real bug.

### PR 7: Transport/framing move

Goal:

Move language-neutral message framing only after protocol primitive parity is
proven.

Acceptance:

- no Perl provider imports
- request/response framing tests pass
- raw RPC receipt remains green

Proof:

```bash
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
git diff --check
```

Rollback:

Revert the transport move and restore current-app framing paths.

### PR 8: Runtime primitive move

Goal:

Move cancellation, request-id allocation, or scheduling primitives only when
they are language-neutral.

Acceptance:

- file-watcher tuning still gates only file watchers
- inline-completion dynamic registration is not suppressed by watcher tuning
- generation-aware stale-read cancellation still passes existing receipts

Proof:

```bash
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
PERL_LSP_E2E=1 PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 PERL_LSP_DIAGNOSTIC_MODE=syntax-only cargo test -p perl-lsp-ux-tests --test ux_neovim_lean_startup_trace -- --test-threads=1 --nocapture
git diff --check
```

Rollback:

Revert the runtime primitive move. Do not weaken cancellation or watcher tests.

### PR 9: Current-app integration

Goal:

Wire `perl-lsp-rs` to the extracted primitives while keeping it as the product
crate and preserving current-app behavior and compatibility paths.

Acceptance:

- current app remains the product surface
- temporary re-exports or compatibility paths are explicit and tested
- capability JSON, dynamic registration, and editor behavior remain unchanged
- duplicate-wrapper cleanup is deferred to PR 10
- no release, package, signing, or marketplace behavior changes

Proof:

```bash
./scripts/cargo-safe check -p lsp-stack --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cap_snap --profile agent --locked
git diff --check
```

Rollback:

Revert the current-app wiring and restore the previous imports. Keep the
extracted crate and its independent tests unless the defect is in the extracted
primitive itself.

### PR 10: Post-extraction cleanup

Goal:

Remove duplicate wrappers and temporary compatibility paths only after current-
app integration, behavior parity, and dependency boundaries are proven.

Acceptance:

- no current consumer requires the removed wrapper
- current app remains the product surface
- docs still describe current editor behavior accurately
- no release, package, signing, or marketplace behavior changes

Proof:

```bash
./scripts/cargo-safe check -p lsp-stack --all-targets --profile agent --locked
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

- creating `crates/lsp-stack` before the boundary, audits, any required dependency preparation, and current-app test baseline are green
- moving code in the boundary-docs PR
- adding Perl dependencies to future `lsp-stack`
- changing inline-completion behavior while extracting infrastructure
- changing DAP while extracting LSP infrastructure
- changing release, publish, signing, marketplace, or package behavior
- claiming release readiness from extraction work
- deleting current-app proof commands to make extraction pass

## Reporting Requirements

Every future extraction PR must say:

- what moved or changed
- what did not move
- whether runtime behavior changed
- whether capability JSON changed
- whether dynamic registration changed
- whether dependencies changed
- whether release or package surfaces changed
- which proof commands passed
- which unrelated warnings were pre-existing
