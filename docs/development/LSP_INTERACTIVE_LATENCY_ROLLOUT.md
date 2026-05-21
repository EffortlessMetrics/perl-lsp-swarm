# LSP Interactive Latency Burndown

> **Substrate (already built)**: perl-lsp currently serves live editors through full text sync and whole-document parse/reanalysis on edit/open paths; read requests are prioritized but still sequenced behind prior document mutations for consistency.
> **Connector gap**: remove avoidable synchronous work on `didOpen`/`didChange`, make diagnostics and startup workload runtime-tunable for latency harnesses, and align semantic-token capability advertisement with actual behavior.
> **0.14.0 upside**: first-useful Neovim/live-editor interactions (hover/completion/diagnostics) stop paying avoidable background work before user-visible feedback.

## Why live Neovim E2E is slow today

Live-editor timing is not a narrow provider microbenchmark: it measures the full event loop envelope (`didOpen`, `didChange`, parse, diagnostics, semantic tokens, workspace/file-watch effects, then read requests). In the current architecture, full-document parse/analysis and follow-on work triggered by mutations can occupy the critical path before first-useful read responses are visible.

This is expected from the existing consistency contract, but it over-exposes avoidable costs in interactive harnesses. The rail therefore targets latency waste and determinism first, and explicitly does **not** attempt true incremental AST reuse in this sequence.

## Critical-path costs and known waste

- **Whole-document mutation cost on edit/open path**: current full text sync / full reparse behavior means mutation handlers carry parse and related state-update costs for each change.
- **Read requests waiting behind earlier mutations**: request prioritization still preserves mutation ordering, so expensive mutation work blocks user-visible read responses.
- **Pull-diagnostics push-computation waste**: diagnostics can be computed before discovering push publication is skipped for pull clients.
- **Fixed diagnostic debounce**: a hard default debounce is reasonable for normal interactive use, but it reduces harness determinism.
- **Eager workspace indexing on startup**: automatic indexing at initialization can compete with first interactive operations.
- **Semantic-token contract mismatch**: advertise/implement mismatch around delta behavior creates extra ambiguity and potential overhead.

## Requirements (R0–R9)

- **R0 — scope boundary**: this rail covers live-editor latency in perl-lsp only; no PR comments/control-plane/ripr/tokmd/parser-grammar/release-prep work.
- **R1 — timing observability**: add opt-in timing probes (`PERL_LSP_TIMING=1`) for `didOpen`, `didChange`, diagnostics, workspace indexing, queue wait, and stale cancellations.
- **R2 — pull diagnostics short-circuit**: skip diagnostic computation on push path when client mode is pull-only.
- **R3 — debounced open-path diagnostics**: replace eager full diagnostics on `didOpen` with fast parse-error publication + debounced full diagnostics.
- **R4 — configurable debounce**: keep default debounce for normal mode, but allow `PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0` for deterministic harnesses.
- **R5 — e2e runtime mode**: add `--runtime-mode e2e` / `PERL_LSP_E2E=1` as runtime workload tuning (not feature-profile advertisement changes).
- **R6 — syntax-only diagnostics in e2e mode**: default e2e diagnostics to syntax-only while preserving normal-mode diagnostics.
- **R7 — startup indexing gate in e2e mode**: disable eager workspace indexing in e2e mode while keeping normal startup behavior.
- **R8 — latest-only generation collapse**: full diagnostics and read-side cancellation must prefer latest document generation; stale results must not publish.
- **R9 — semantic-token capability cleanup + receipts**: either implement `full/delta` with `resultId` correctly or stop advertising delta; prove outcomes with raw RPC and Neovim receipts.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---|---|---|
| 1. Rail doc + index seed | #0000 | yes | — | `git diff --check` clean |
| 2. Timing probes (`PERL_LSP_TIMING`) | #0000 | yes | — | timing logs include didOpen/didChange/diag/index/queue/stale |
| 3. Pull diagnostics short-circuit | #0000 | yes | — | pull client run shows no discarded push diagnostic computation |
| 4. Debounced `didOpen` diagnostics | #0000 | yes | — | open-path latency receipt + parse-error-first behavior |
| 5. Configurable diagnostic debounce | #0000 | yes | — | `PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0` deterministic receipt |
| 6. E2E runtime mode plumbing | #0000 | yes | — | e2e mode startup receipt (runtime knobs active) |
| 7. E2E syntax-only diagnostics default | #0000 | yes | — | e2e diagnostics lane excludes semantic/module/critic/dead-code by default |
| 8. E2E workspace indexing startup gate | #0000 | yes | — | e2e startup receipt shows no eager indexing kickoff |
| 9. Latest-only diagnostics publication | #0000 | yes | — | stale generation diagnostics never publish |
| 10. Generation-aware stale read cancellation | #0000 | yes | — | moving-cursor edit stream cancels superseded reads |
| 11. Semantic-token contract cleanup | #0000 | yes | — | advertised capability matches implemented delta/full behavior |
| 12. Raw RPC + Neovim latency receipts | #0000 | yes | — | two-lane latency receipts committed |

## PR-by-PR implementation plan

1. **Timing probes** only.
2. **Pull diagnostics short-circuit** only.
3. **`didOpen` full-diagnostics deferral** only.
4. **Configurable diagnostic debounce** only.
5. **E2E runtime mode** only.
6. **E2E syntax-only diagnostics default** only.
7. **E2E startup indexing gate** only.
8. **Latest-only diagnostics publication** only.
9. **Generation-aware stale read cancellation** only.
10. **Semantic-token capability cleanup** only.
11. **Receipt PR** for raw RPC + Neovim proof.

Each PR in this rail is one semantic change, no bundling.

## Receipts and acceptance checks

Baseline structural receipt for this seed PR:

```bash
git diff --check
```

Implementation-phase acceptance receipts (to be executed in later PRs):

```bash
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --profile agent --locked
just agent-pr-fast
```

Performance proof receipts for final rail closure:

- **Raw JSON-RPC lane**: reproducible latency captures isolating protocol-path costs.
- **Neovim live-editor lane**: reproducible latency captures in lean runtime config and normal runtime config, showing avoidable-latency reduction without weakening normal-mode correctness.

## Exit criteria

A rail is closed when ALL of:

- [ ] R0–R9 are satisfied via landed PRs or explicit deferred successor issues.
- [ ] Raw RPC and Neovim receipts are reproducible by another contributor.
- [ ] `docs/project/RAILS_INDEX.md` and this rail doc reflect landed state.
- [ ] Claim boundary remains intact and unexpanded.

## Claim boundary

This rail proves perl-lsp removed avoidable live-editor latency on the edit/open critical path and made latency harnessing deterministic enough to attribute remaining cost.

This rail does **not** prove true incremental AST reuse, does **not** change the advertised full text sync contract yet, and does **not** prove broader control-plane quality or unrelated subsystem performance.

## Do not combine

Do not combine rail PRs with:

- PR comments or PR gate control-plane work;
- ripr or tokmd lanes;
- parser grammar/AST architecture rewrites (including true incremental AST reuse);
- Clippy/codecov/file-policy/release-prep lanes;
- unrelated diagnostics feature expansion.

## Lane assignment

**Lane**: builder (perl-lsp live-editor runtime behavior and receipts).
