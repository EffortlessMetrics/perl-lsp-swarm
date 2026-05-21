# LSP Interactive Latency Rollout

> **Substrate (already built)**: async scheduler, mutation/read separation,
> request prioritization, exact-position stale-read cancellation, parse
> cancellation tokens, document generations, diagnostic debouncer, parse-error
> fast path, workspace-index coordinator, semantic-token provider, and UX test
> harness.
>
> **Connector gap**: live editor sessions still pay too much synchronous
> whole-document and follow-on work on `didOpen` / `didChange`; the repo does
> not yet have a first-class latency mode, stable timing receipts, or a
> regression budget that keeps interactive performance from drifting.
>
> **User-visible upside**: live Neovim and other LSP clients get faster
> first-useful hover, completion, and diagnostic feedback, while normal-mode
> diagnostics and workspace features remain intact.

## Problem statement

Live Neovim is exposing real latency, not just harness noise.

A raw JSON-RPC test can time one request. A live editor exercises the whole loop:
open/change notifications, full document reparse, diagnostics, optional
semantic-token refresh, file-watcher/workspace activity, and read requests that
must wait behind earlier document mutations.

The current server contract is honest but expensive: it advertises full document
sync because the runtime still reparses the full document and does not maintain
incremental AST state between edits.

The live `didChange` path still calls `Parser::new(...).parse()` over the code
slice, rebuilds derived state, and then schedules additional analysis work. The
incremental helper state can speed lexer/checkpoint work, but the AST still comes
from the full parser call.

The scheduler is structurally correct: reads are prioritized, but they still
wait for prior mutations. That means hover/completion inherit expensive
`didOpen` / `didChange` work in front of them.

## Non-goals

This rail does **not**:

- implement true incremental AST reuse;
- switch advertised text sync back to incremental;
- weaken normal-mode diagnostics;
- make e2e mode the default;
- remove workspace indexing in normal mode;
- change parser grammar behavior;
- change tree-sitter behavior;
- combine with PR comments, PR gates, ripr, tokmd, Clippy, Codecov, file-policy, or release-prep work.

## Guiding principle

The live edit/open path should pay only for work needed to make the latest
document state coherent.

Everything else should be:

```text
versioned
latest-only
deferred
cancellable
measured
budgeted
```

## Requirements

### R0 — Timing visibility

Add opt-in timing behind:

```bash
PERL_LSP_TIMING=1
```

Timing must cover:

- initialize;
- initialized;
- `didOpen`;
- `didChange`;
- parse;
- parent-map build;
- document symbol extraction;
- `publish_parse_errors_fast`;
- `publish_diagnostics`;
- diagnostic debounce schedule/fire;
- workspace indexing start/end;
- read queue wait after mutation;
- stale read cancellation;
- semantic-token full/range/delta request shape.

Timing output must go to stderr, logs, or receipt artifacts. It must never write
non-LSP bytes to stdout.

### R1 — Pull diagnostics must not compute discarded push diagnostics

When a client uses pull diagnostics, `publish_diagnostics` must return before:

- document snapshotting;
- diagnostic provider construction;
- semantic diagnostics;
- module-resolution diagnostics;
- native critic;
- external Perl::Critic;
- workspace dead-code diagnostics;
- LSP diagnostic JSON conversion.

OpenCode-style clients that intentionally keep push diagnostics must retain
current behavior.

### R2 — `didOpen` must not run full diagnostics synchronously

On successful `didOpen`, replace synchronous full diagnostics with:

```rust
self.publish_parse_errors_fast(uri);
self.publish_diagnostics_debounced(uri);
```

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

`0` means immediate scheduling through the debouncer, not “disable diagnostics.”

### R4 — E2E/light runtime mode

Add:

```bash
perl-lsp --stdio --runtime-mode e2e
PERL_LSP_E2E=1
```

E2E mode defaults:

```text
diagnostic_debounce_ms = 0
diagnostic_mode = syntax-only
eager_workspace_indexing = false
file_watchers = false unless explicitly enabled
startup noise = quiet
```

This is runtime workload tuning, not advertised feature-profile tuning.

### R5 — Syntax-only diagnostics mode

Add diagnostic modes:

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

Both push and pull diagnostics must respect the mode.

### R6 — Workspace indexing must be gated

Normal mode preserves current eager indexing.

E2E mode must not call `start_workspace_indexing()` during `initialized`.

### R7 — Full diagnostics must be latest-only

Full diagnostics must be generation-aware.

If a newer document generation appears while full diagnostics are pending or
computing, the stale result must not publish.

The fast parse-error path must still publish promptly.

### R8 — Read cancellation must be generation-aware

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

### R9 — Semantic-token capability contract must be honest

Either implement:

```text
textDocument/semanticTokens/full/delta
resultId
previous result cache
delta response
```

or stop advertising semantic-token delta support.

### R10 — Latency budgets must be repo-owned

Add:

```text
policy/lsp-latency-budgets.toml
docs/status/LSP_INTERACTIVE_LATENCY_BUDGETS.md
```

Budgets begin advisory. They become blocking only after burn-in.

Budget dimensions:

- raw JSON-RPC;
- live Neovim lean mode;
- live Neovim normal smoke;
- file size class;
- runtime mode;
- release/debug build profile;
- semantic tokens enabled/disabled;
- file watching enabled/disabled;
- diagnostics mode.

### R11 — Timing receipts must be stable and reviewable

Add schema:

```text
.ci/receipts/schemas/lsp-latency.schema.json
```

Receipt must include:

```json
{
  "schema_version": 1,
  "mode": "raw-rpc | neovim",
  "runtime_mode": "normal | e2e",
  "build_profile": "release | debug",
  "server_sha": "...",
  "base_sha": "...",
  "workspace_shape": "...",
  "semantic_tokens": "enabled | disabled",
  "file_watchers": "enabled | disabled",
  "diagnostics_mode": "normal | syntax-only",
  "measurements": [],
  "budget_verdict": "pass | warn | fail | not_applicable",
  "claim_boundary": "..."
}
```

### R12 — Slow-path admission policy

Any PR that adds synchronous work to `didOpen`, `didChange`, diagnostics, semantic tokens, or scheduler read gating must answer:

```text
Why must this work be synchronous?
Can it be latest-only?
Can it be deferred?
Can it be cancelled?
What timing receipt proves the change?
```

Codex should encode this in the rail and later in docs/reference.

### R13 — Documentation drift guard

Docs must not claim “incremental parsing” or “incremental AST” for the live LSP path unless true AST reuse exists.

Allowed wording:

```text
incremental text application
incremental lexer/checkpoint helper
full AST parse on live LSP edits
```

Forbidden wording without qualification:

```text
incremental parsing on every edit
incremental AST reuse
subtree reuse on didChange
```

### R14 — Follow-up rail boundary

When this rail closes, open the next rail:

```text
LSP Incremental Parse Architecture
```

That rail owns:

- moving parse off the mutation worker;
- latest parse jobs per URI;
- AST freshness states;
- fallback provider behavior while AST catches up;
- true incremental AST reuse, if implemented.

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

### Server helper methods

Do not scatter environment reads through handlers.

Add helpers:

```rust
fn runtime_mode(&self) -> RuntimeMode;
fn diagnostic_mode(&self) -> DiagnosticMode;
fn diagnostic_debounce_interval(&self) -> Duration;
fn should_start_workspace_indexing(&self) -> bool;
fn should_register_file_watchers(&self) -> bool;
fn should_compute_push_diagnostics(&self) -> bool;
fn should_run_full_diagnostics(&self) -> bool;
```

### `didOpen` target flow

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

### `didChange` target flow for this rail

This rail keeps full parse on `didChange`, but makes the surrounding work cheaper:

```text
didChange
  apply edit
  full parse for now
  update state
  publish parse errors fast
  schedule latest-only full diagnostics
  cancel stale read work by generation
```

True parse-off-mutation comes later.

### Budget policy file

Add:

```toml
# policy/lsp-latency-budgets.toml
schema_version = 1
policy = "lsp-interactive-latency"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-05-21"

[defaults]
sample_count = 20
warmup_count = 3
build_profile = "release"
stability = "advisory"

[[scenario]]
id = "raw_rpc_open_hover_e2e"
mode = "raw-rpc"
runtime_mode = "e2e"
semantic_tokens = false
file_watchers = false
diagnostics_mode = "syntax-only"
p95_warn_ms = 500
p95_fail_ms = 1000
claim = "First-useful hover in e2e mode should not wait on workspace indexing or full diagnostics."

[[scenario]]
id = "raw_rpc_edit_diagnostics_clear_e2e"
mode = "raw-rpc"
runtime_mode = "e2e"
semantic_tokens = false
file_watchers = false
diagnostics_mode = "syntax-only"
p95_warn_ms = 500
p95_fail_ms = 1000
claim = "Syntax-only diagnostic clear should avoid fixed debounce and full diagnostics."

[[scenario]]
id = "neovim_open_completion_lean"
mode = "neovim"
runtime_mode = "e2e"
semantic_tokens = false
file_watchers = false
diagnostics_mode = "syntax-only"
p95_warn_ms = 750
p95_fail_ms = 1500
claim = "Live Neovim lean mode should produce first-useful completion without full IDE background work."

[[scenario]]
id = "neovim_normal_smoke"
mode = "neovim"
runtime_mode = "normal"
semantic_tokens = true
file_watchers = true
diagnostics_mode = "normal"
p95_warn_ms = 2000
p95_fail_ms = 4000
claim = "Normal mode remains usable; this is smoke/advisory, not a hard SLO."
```

### Latency command surface

Add later:

```bash
cargo xtask lsp-latency run \
  --scenario raw_rpc_open_hover_e2e \
  --receipt target/receipts/lsp-latency/raw_rpc_open_hover_e2e.json

cargo xtask lsp-latency verify \
  --policy policy/lsp-latency-budgets.toml \
  --receipt target/receipts/lsp-latency/raw_rpc_open_hover_e2e.json

cargo xtask lsp-latency report \
  --receipts target/receipts/lsp-latency \
  --output target/receipts/lsp-latency/report.md
```

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---:|---|---|
| 1. Rail doc + index row | file after doc PR | yes | — | `git diff --check` |
| 2. Timing probes | file after phase 1 | yes | — | `PERL_LSP_TIMING=1 ...` |
| 3. Pull diagnostics short-circuit | file after phase 1 | yes | — | pull-client provider-not-invoked test |
| 4. Debounced `didOpen` diagnostics | file after phase 1 | yes | — | didOpen scheduling test |
| 5. Configurable diagnostic debounce | file after phase 1 | yes | — | debounce env tests |
| 6. Runtime tuning / e2e mode | file after phase 1 | yes | — | e2e defaults receipt |
| 7. Syntax-only diagnostics | file after phase 1 | yes | — | syntax-only push/pull tests |
| 8. Workspace indexing gate | file after phase 1 | yes | — | e2e no-index test |
| 9. Latest-only diagnostics | file after phase 1 | yes | — | stale diagnostics discarded test |
| 10. Generation-aware read cancellation | file after phase 1 | yes | — | rapid typing cancellation test |
| 11. Semantic-token contract cleanup | file after phase 1 | yes | — | advertised methods match router |
| 12. Raw RPC latency receipts | file after phase 1 | yes | — | lsp-latency JSON receipt |
| 13. Neovim lean latency receipt | file after phase 1 | maybe | — | label-gated if CI lacks Neovim |
| 14. Latency budget ledger | file after phase 1 | yes | — | `policy/lsp-latency-budgets.toml` |
| 15. Latency verifier | file after phase 14 | yes | — | `cargo xtask lsp-latency verify` |
| 16. Regression gate advisory | file after phase 15 | yes | — | CI artifact/report |
| 17. Slow-path admission docs | file after phase 1 | yes | — | docs/reference update |
| 18. Documentation drift guard | file after phase 1 | yes | — | grep/xtask doc guard |
| 19. Follow-up incremental parse rail | after phase 12 | yes | — | new rail doc |

## Exit criteria

This rail is closed only when:

- [ ] The rail doc exists and is listed in `docs/project/RAILS_INDEX.md`.
- [ ] Timing probes exist and never write to stdout.
- [ ] Pull clients do not compute discarded push diagnostics.
- [ ] `didOpen` no longer runs full diagnostics synchronously.
- [ ] Diagnostic debounce is configurable and can be set to `0`.
- [ ] E2E mode exists.
- [ ] E2E mode disables eager workspace indexing.
- [ ] E2E mode defaults to syntax-only diagnostics.
- [ ] Syntax-only diagnostics work for push and pull.
- [ ] Full diagnostics are latest-only by generation.
- [ ] Stale read requests cancel by document generation.
- [ ] Semantic-token advertised capability matches implemented handlers.
- [ ] Raw JSON-RPC latency receipt exists.
- [ ] Live Neovim lean receipt exists or is explicitly label-gated.
- [ ] Latency budget ledger exists.
- [ ] Latency verifier exists at least advisory.
- [ ] Slow-path admission policy is documented.
- [ ] Docs cannot imply live incremental AST parsing unless true AST reuse exists.
- [ ] Follow-up incremental-parse rail is created or explicitly deferred.

## Claim boundary

This rail proves:

- live-editor latency has a repo-owned implementation and maintenance contract;
- obvious open/edit-path waste is removed;
- latency-focused harnesses can run deterministic, low-noise sessions;
- pull clients avoid discarded push-diagnostic computation;
- startup indexing and full diagnostics no longer define e2e first-useful results;
- regression budgets and receipts exist to keep the fix from drifting.

This rail does **not** prove:

- true incremental AST reuse;
- CPAN-wide latency behavior;
- all Neovim plugin configurations;
- all semantic-token client behavior;
- full workspace-index readiness in e2e mode;
- parser correctness improvements;
- grammar improvements.

## Receipts

Core local receipts:

```bash
git diff --check

PERL_LSP_TIMING=1 \
PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture
```

Budget receipts:

```bash
cargo xtask lsp-latency run \
  --scenario raw_rpc_open_hover_e2e \
  --receipt target/receipts/lsp-latency/raw_rpc_open_hover_e2e.json

cargo xtask lsp-latency verify \
  --policy policy/lsp-latency-budgets.toml \
  --receipt target/receipts/lsp-latency/raw_rpc_open_hover_e2e.json
```

Manual Neovim receipt:

```bash
PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
target/release/perl-lsp --stdio --runtime-mode e2e
```

## Do not combine

Do not combine this rail with:

- true incremental AST reuse;
- parser grammar changes;
- tree-sitter work;
- corpus correctness work;
- PR comments;
- PR gate control-plane;
- ripr;
- tokmd;
- Clippy cleanup;
- Codecov;
- file-policy rollout;
- release prep.

## Lane assignment

Primary lane: **codex**.

Builder lane may be needed for:

- semantic-token delta implementation if de-advertising is not enough;
- live Neovim harness integration if CI dependencies are nontrivial;
- true incremental AST follow-up rail.
