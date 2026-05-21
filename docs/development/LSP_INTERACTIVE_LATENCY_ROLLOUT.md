# LSP Interactive Latency Burndown

> **Substrate (already built)**: async scheduler, mutation/read separation,
> request priority, exact-position stale-request cancellation, diagnostic
> debouncer, parse-error fast path, generation counters, parse cancellation
> tokens, workspace-index coordinator, and UX harness substrate.
>
> **Connector gap**: live editor sessions still pay too much synchronous
> whole-document work on `didOpen` / `didChange`, and the repo has no
> first-class low-noise runtime mode for live Neovim/e2e harnesses.
>
> **0.14.0 upside**: live Neovim and raw JSON-RPC harnesses can measure first
> useful hover/completion/diagnostic behavior without being dominated by
> discarded diagnostics, startup indexing, fixed debounce delay, or stale
> intermediate work.

## Problem statement

Live Neovim e2e exposes real latency. It is not only a harness problem.

A raw JSON-RPC harness can time one provider call. A live editor exercises the
whole loop: document open/change traffic, full-document parse, diagnostics,
semantic-token refresh, workspace file watching, client-side settling, and read
requests waiting behind earlier mutations.

The current server is structurally correct but too eager:

```text
keystroke
  -> textDocument/didChange
  -> full parser run
  -> parent map / symbols / cache invalidation
  -> parse-error diagnostics
  -> debounced full diagnostics
  -> queued read requests become useful
```

The durable direction is to make the mutation path cheap and move non-essential
work to versioned, latest-only background lanes. This rail implements the
low-risk connector steps first. It does **not** implement true incremental AST
reuse.

## Current behavior to preserve

Normal mode must preserve:

- current advertised capabilities unless a phase explicitly changes one;
- push diagnostics for push clients;
- pull diagnostics for pull clients;
- OpenCode push-diagnostic exception;
- eager workspace indexing in normal mode;
- 250 ms diagnostic debounce in normal mode;
- normal full diagnostics behavior outside syntax-only/e2e mode.

## Non-goals

This rail does **not**:

- implement true incremental AST reuse;
- switch the server back to incremental text sync;
- weaken normal-mode diagnostics;
- make e2e mode the default user mode;
- change parser grammar behavior;
- change tree-sitter integration;
- merge semantic-token cleanup with parser/corpus work;
- prove CPAN-wide latency behavior.

## Requirements

### R0 — Timing visibility

When `PERL_LSP_TIMING=1` is set, the server records timing spans for:

- initialize;
- initialized;
- `didOpen`;
- `didChange`;
- `publish_parse_errors_fast`;
- `publish_diagnostics`;
- `publish_diagnostics_debounced`;
- workspace indexing start/end;
- read queue wait after mutation;
- stale request cancellation.

Timing output must go to stderr, logs, or artifact files. It must never write
non-LSP bytes to stdout.

### R1 — Pull clients must not compute discarded push diagnostics

When `client_supports_pull_diags` is true, `publish_diagnostics` must return
before:

- document snapshotting;
- diagnostic provider construction;
- semantic diagnostics;
- module-resolution diagnostics;
- native critic;
- external Perl::Critic;
- workspace dead-code diagnostics;
- LSP diagnostic JSON conversion.

OpenCode-style clients that intentionally keep push diagnostics enabled must
keep current push behavior.

### R2 — `didOpen` must not run full diagnostics synchronously

On successful `didOpen`, the server should:

```text
parse/store document state
publish parse errors fast if applicable
schedule full diagnostics through the diagnostic debouncer
return
```

It must not synchronously call the full `publish_diagnostics` pipeline from the
successful open path.

### R3 — Diagnostic debounce must be configurable

Default remains 250 ms.

Add:

```bash
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0
```

and, if launch plumbing allows cleanly:

```bash
perl-lsp --diagnostic-debounce-ms 0
```

A value of `0` means “fire immediately through the debouncer worker,” not
“disable diagnostics.”

Invalid values fall back to the default.

### R4 — Runtime tuning / e2e mode

Add:

```bash
perl-lsp --stdio --runtime-mode e2e
```

and environment fallback:

```bash
PERL_LSP_E2E=1
```

E2E mode defaults:

```text
diagnostic_debounce_ms = 0
diagnostic_mode = syntax-only
eager_workspace_indexing = false
file_watchers = false unless explicitly enabled
quiet startup = true
```

E2E mode changes runtime workload, not protocol identity.

### R5 — Syntax-only diagnostics mode

Add diagnostic mode:

```text
normal
syntax-only
```

Syntax-only mode reports parser errors and clean clears.

Syntax-only mode must skip:

- semantic diagnostics;
- module-resolution diagnostics;
- native critic;
- external Perl::Critic;
- workspace dead-code diagnostics.

Push and pull diagnostics must both respect the mode.

### R6 — Workspace indexing is gated

Normal mode preserves eager startup indexing.

E2E mode must not call `start_workspace_indexing()` during `initialized`.

A later lazy-indexing phase is allowed but is not required for this rail.

### R7 — Full diagnostics are latest-only

Full diagnostics must be generation-aware.

If a newer document generation appears while diagnostics are pending or
computing, stale diagnostics must not publish.

The fast parse-error path must continue to publish promptly.

### R8 — Read cancellation is generation-aware

Exact-position dedup is not enough for typing because the cursor moves.

For these methods:

- hover;
- completion;
- definition;
- declaration;
- typeDefinition;
- implementation;
- references;
- semantic tokens, if practical;

the scheduler should cancel before execution when the request was created
against an older open-document generation than the current generation.

### R9 — Semantic-token capability contract is honest

The server must either:

1. implement `textDocument/semanticTokens/full/delta` correctly, including
   `resultId`, previous-result cache, and delta response; or
2. stop advertising semantic-token delta support.

Pick the smaller safe change for this rail. If delta implementation is not
small and obvious, de-advertise delta first.

## Design

### Runtime tuning model

Add runtime tuning near launch config and copy it into `LspServer`.

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RuntimeMode {
    Normal,
    E2e,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DiagnosticMode {
    Normal,
    SyntaxOnly,
}

#[derive(Debug, Clone)]
pub struct RuntimeTuning {
    pub runtime_mode: RuntimeMode,
    pub diagnostic_mode: DiagnosticMode,
    pub diagnostic_debounce_ms: u64,
    pub eager_workspace_indexing: bool,
    pub file_watchers: bool,
}
```

Extend launch config:

```rust
pub struct LaunchConfig {
    pub transport: TransportMode,
    pub enable_logging: bool,
    pub feature_profile: FeatureProfile,
    pub runtime_tuning: RuntimeTuning,
}
```

Precedence:

```text
CLI flag > environment variable > runtime-mode default > normal default
```

Examples:

```bash
perl-lsp --runtime-mode e2e
PERL_LSP_E2E=1

perl-lsp --diagnostic-mode syntax-only
PERL_LSP_DIAGNOSTIC_MODE=syntax-only

perl-lsp --diagnostic-debounce-ms 0
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0
```

### Server helper methods

Avoid scattering env reads through handlers.

Add helpers on `LspServer`:

```rust
fn runtime_mode(&self) -> RuntimeMode;
fn diagnostic_mode(&self) -> DiagnosticMode;
fn diagnostic_debounce_interval(&self) -> Duration;
fn should_start_workspace_indexing(&self) -> bool;
fn should_register_file_watchers(&self) -> bool;
fn should_compute_push_diagnostics(&self) -> bool;
fn should_run_full_diagnostics(&self) -> bool;
```

### Diagnostic flow

`publish_diagnostics` starts with:

```rust
if !self.should_compute_push_diagnostics() {
    return;
}
```

Then:

```rust
if self.diagnostic_mode() == DiagnosticMode::SyntaxOnly {
    return self.publish_syntax_only_diagnostics(uri);
}
```

### `didOpen` flow

Normal push client:

```text
didOpen
  parse/store document
  publish parse errors fast
  schedule full diagnostics debounced
  return
```

Pull client:

```text
didOpen
  parse/store document
  no push diagnostic computation
  return
```

E2E mode:

```text
didOpen
  parse/store document
  syntax-only diagnostics
  no eager workspace indexing
  return
```

### Workspace indexing flow

Normal mode:

```text
initialized
  register file watchers if supported
  start workspace indexing
```

E2E mode:

```text
initialized
  skip file watchers unless explicitly enabled
  do not start workspace indexing
```

### Semantic-token flow

Codex must inspect the advertised capability and router before editing.

If delta is advertised but there is no delta route/handler/resultId support,
either implement the missing protocol path or de-advertise delta. Prefer the
smaller safe change.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---:|---|---|
| 1. Rail doc + index row | file after doc PR | yes | — | `git diff --check` |
| 2. Timing probes | file after phase 1 | yes | — | `PERL_LSP_TIMING=1 cargo test -p perl-lsp-rs timing` |
| 3. Pull diagnostics short-circuit | file after phase 1 | yes | — | `cargo test -p perl-lsp-rs pull_diagnostics` |
| 4. Debounced `didOpen` diagnostics | file after phase 1 | yes | — | `cargo test -p perl-lsp-rs did_open_diagnostics` |
| 5. Configurable diagnostic debounce | file after phase 1 | yes | — | `PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 cargo test -p perl-lsp-rs diagnostic_debounce` |
| 6. Runtime tuning / e2e mode | file after phase 1 | yes | — | `perl-lsp --info --runtime-mode e2e` |
| 7. Syntax-only diagnostics | file after phase 1 | yes | — | `cargo test -p perl-lsp-rs syntax_only_diagnostics` |
| 8. Workspace indexing gate | file after phase 1 | yes | — | `cargo test -p perl-lsp-rs e2e_mode_does_not_start_indexing` |
| 9. Latest-only diagnostics | file after phase 1 | yes | — | stale-generation diagnostics test |
| 10. Generation-aware read cancellation | file after phase 1 | yes | — | rapid typing stale-read cancellation tests |
| 11. Semantic-token contract cleanup | file after phase 1 | yes | — | advertised-methods-match-router test |
| 12. Raw RPC / Neovim latency receipts | file after phase 1 | maybe | — | latency receipt JSON/MD artifacts |

## Phase implementation plan

### Phase 1 — Documentation rail

Files:

```text
docs/development/LSP_INTERACTIVE_LATENCY_ROLLOUT.md
docs/project/RAILS_INDEX.md
```

Acceptance:

```bash
git diff --check
```

No code changes.

### Phase 2 — Timing probes

Add opt-in timing behind:

```bash
PERL_LSP_TIMING=1
```

Probe:

- initialize;
- initialized;
- didOpen;
- didChange;
- fast parse diagnostics;
- full diagnostics;
- diagnostic debounce scheduling/firing;
- workspace indexing;
- read queue wait;
- stale cancellation.

Acceptance:

```bash
PERL_LSP_TIMING=1 cargo test -p perl-lsp-rs timing
cargo test -p perl-lsp-rs runtime_pressure
git diff --check
```

Timing output must never touch stdout.

### Phase 3 — Pull diagnostics short-circuit

Change:

```rust
pub(crate) fn publish_diagnostics(&self, uri: &str) {
    if !self.should_compute_push_diagnostics() {
        return;
    }

    // existing expensive implementation
}
```

Acceptance:

```bash
cargo test -p perl-lsp-rs pull_diagnostic_clients_do_not_compute_push_diagnostics
cargo test -p perl-lsp-rs push_diagnostic_clients_still_receive_publish_diagnostics
cargo test -p perl-lsp-rs opencode_exception_keeps_push_diagnostics
git diff --check
```

### Phase 4 — Defer full diagnostics on `didOpen`

Replace successful `didOpen` calls to full diagnostics with:

```rust
self.publish_parse_errors_fast(uri);
self.publish_diagnostics_debounced(uri);
```

Acceptance:

```bash
cargo test -p perl-lsp-rs did_open_publishes_parse_errors_fast
cargo test -p perl-lsp-rs did_open_schedules_full_diagnostics
cargo test -p perl-lsp-rs did_open_does_not_block_on_full_diagnostics
git diff --check
```

### Phase 5 — Configurable diagnostic debounce

Add:

```bash
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0
```

and, if clean:

```bash
--diagnostic-debounce-ms 0
```

Acceptance:

```bash
cargo test -p perl-lsp-rs diagnostic_debounce_default_is_250ms
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 cargo test -p perl-lsp-rs diagnostic_debounce_zero_is_immediate
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=garbage cargo test -p perl-lsp-rs diagnostic_debounce_invalid_falls_back
git diff --check
```

### Phase 6 — Runtime tuning / e2e mode

Add:

```bash
--runtime-mode normal|e2e
PERL_LSP_E2E=1
```

E2E defaults:

```text
diagnostic_debounce_ms = 0
diagnostic_mode = syntax-only
eager_workspace_indexing = false
file_watchers = false
quiet startup = true
```

Acceptance:

```bash
perl-lsp --info --runtime-mode e2e
cargo test -p perl-lsp-rs runtime_mode_e2e_defaults
cargo test -p perl-lsp-rs runtime_mode_normal_defaults_unchanged
git diff --check
```

### Phase 7 — Syntax-only diagnostics

Add:

```bash
--diagnostic-mode normal|syntax-only
PERL_LSP_DIAGNOSTIC_MODE=syntax-only
```

Acceptance:

```bash
cargo test -p perl-lsp-rs syntax_only_reports_parse_errors
cargo test -p perl-lsp-rs syntax_only_clears_when_parse_errors_clear
cargo test -p perl-lsp-rs syntax_only_skips_critic_dead_code_and_module_resolution
cargo test -p perl-lsp-rs pull_diagnostics_respect_syntax_only_mode
git diff --check
```

### Phase 8 — Workspace indexing gate

Change initialized path to:

```rust
if self.should_start_workspace_indexing() {
    self.start_workspace_indexing();
}
```

Acceptance:

```bash
cargo test -p perl-lsp-rs normal_mode_starts_workspace_indexing
cargo test -p perl-lsp-rs e2e_mode_does_not_start_workspace_indexing
cargo test -p perl-lsp-rs workspace_symbol_normal_mode_unchanged
git diff --check
```

### Phase 9 — Latest-only full diagnostics

Implement one pending/computing full diagnostic generation per URI.

Acceptance:

```bash
cargo test -p perl-lsp-rs stale_full_diagnostics_are_discarded
cargo test -p perl-lsp-rs latest_full_diagnostics_publish
cargo test -p perl-lsp-rs parse_error_fast_path_still_publishes
git diff --check
```

### Phase 10 — Generation-aware read cancellation

Apply to:

- hover;
- completion;
- definition;
- declaration;
- typeDefinition;
- implementation;
- references.

Acceptance:

```bash
cargo test -p perl-lsp-rs stale_hover_cancelled_after_newer_generation
cargo test -p perl-lsp-rs stale_completion_cancelled_after_newer_generation
cargo test -p perl-lsp-rs newest_request_for_generation_runs
git diff --check
```

### Phase 11 — Semantic-token capability contract

Decision rule:

- If delta implementation is small and safe, implement it.
- Otherwise de-advertise delta.

Acceptance:

```bash
cargo test -p perl-lsp-rs semantic_tokens_advertised_methods_are_implemented
cargo test -p perl-lsp-rs semantic_tokens_delta_contract_or_not_advertised
git diff --check
```

### Phase 12 — Latency receipts

Add raw RPC receipt first. Add live Neovim receipt if CI environment can support it; otherwise make Neovim label-gated and document the reason.

Scenarios:

- open → hover;
- open → completion;
- edit → parse-error diagnostic;
- edit → diagnostic clear;
- rapid typing → latest completion wins;
- e2e mode → no eager startup indexing;
- semantic tokens off → no token traffic.

Acceptance:

```bash
cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture

PERL_LSP_E2E=1 \nPERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \ncargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture

git diff --check
```

## Exit criteria

This rail is closed only when all are true:

- [ ] Rail doc exists and is listed in `docs/project/RAILS_INDEX.md`.
- [ ] Pull clients do not compute discarded push diagnostics.
- [ ] `didOpen` no longer runs full diagnostics synchronously.
- [ ] Diagnostic debounce is runtime-configurable.
- [ ] `--runtime-mode e2e` or `PERL_LSP_E2E=1` exists.
- [ ] E2E mode disables eager workspace indexing.
- [ ] E2E mode defaults to syntax-only diagnostics.
- [ ] Syntax-only diagnostics work for push and pull diagnostics.
- [ ] Full diagnostics are latest-only by document generation.
- [ ] Stale read requests can cancel by document generation.
- [ ] Semantic-token advertised capability matches implemented handlers.
- [ ] Raw RPC latency receipt exists.
- [ ] Live Neovim latency receipt exists or is explicitly label-gated with a documented reason.
- [ ] Normal mode remains covered and unchanged where this rail says it must remain unchanged.
- [ ] Claim boundary is recorded.

## Receipts

Core local receipts:

```bash
git diff --check

cargo test -p perl-lsp-rs diagnostic_debounce
cargo test -p perl-lsp-rs did_open_diagnostics
cargo test -p perl-lsp-rs pull_diagnostics
cargo test -p perl-lsp-rs syntax_only_diagnostics
cargo test -p perl-lsp-rs runtime_mode_e2e_defaults
cargo test -p perl-lsp-rs e2e_mode_does_not_start_indexing
```

Latency receipts:

```bash
PERL_LSP_TIMING=1 \nPERL_LSP_E2E=1 \nPERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \ncargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
```

Manual live Neovim receipt, if available:

```bash
PERL_LSP_E2E=1 \nPERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \ntarget/release/perl-lsp --stdio --runtime-mode e2e
```

## Claim boundary

This rail proves:

- latency-focused editor harnesses have a supported low-noise runtime mode;
- pull clients avoid discarded push-diagnostic computation;
- `didOpen` no longer pays synchronous full diagnostics;
- fixed diagnostic debounce can be removed for deterministic tests;
- eager workspace indexing can be suppressed in e2e mode;
- syntax-only diagnostics are available and tested;
- stale full diagnostics/read requests can be dropped or canceled by generation;
- semantic-token advertised capabilities match implementation.

This rail does **not** prove:

- true incremental AST reuse;
- all Neovim plugin configurations;
- all CPAN/workspace shapes;
- full workspace-index readiness in e2e mode;
- parser correctness improvements;
- grammar improvements;
- semantic-token delta correctness unless delta is explicitly implemented in phase 11.

## Related

- Umbrella issue: file after doc PR.
- Architecture: `crates/perl-lsp-rs/src/runtime/scheduler.rs`
- Runtime launch config: `crates/perl-lsp-rs-core/src/runtime/launcher/mod.rs`
- Text sync: `crates/perl-lsp-rs/src/runtime/text_sync.rs`
- Diagnostics: `crates/perl-lsp-rs/src/runtime/diagnostics.rs`
- Diagnostic debounce: `crates/perl-lsp-rs/src/runtime/diagnostic_debounce.rs`
- Workspace indexing: `crates/perl-lsp-rs/src/runtime/workspace.rs`
- Semantic-token routing/capabilities: inspect before phase 11.

## Do not combine

Do not combine this rail with:

- true incremental AST reuse;
- parser grammar changes;
- tree-sitter parser work;
- corpus correctness changes;
- Clippy cleanup rails;
- Codecov / coverage rails;
- file-policy rollout;
- release prep;
- ripr / mutation testing changes;
- VS Code packaging;
- broad docs cleanup.

Each phase must be a single-purpose PR.

## Lane assignment

Primary lane: **codex**.

Codex owns:

- docs rail;
- timing probes;
- diagnostic short-circuit;
- `didOpen` diagnostic deferral;
- debounce config;
- runtime tuning;
- syntax-only diagnostics;
- workspace indexing gate;
- latest-only diagnostics;
- generation-aware stale read cancellation;
- semantic-token capability cleanup if de-advertising is enough;
- raw RPC latency receipts.

Builder lane may be needed for:

- full semantic-token delta implementation;
- true incremental AST follow-up;
- live Neovim harness integration if CI dependencies are nontrivial.
