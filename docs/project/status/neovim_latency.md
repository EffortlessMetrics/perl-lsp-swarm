# Neovim Lean Latency Status

> Human-owned. Update when the Neovim lean profile, raw-RPC receipts, actual
> Neovim receipts, trace receipts, or benchmark evidence changes.

## Current claim

The lean profile is a bounded server-work profile for sessions where parser
diagnostics and responsiveness matter more than the full
semantic/module/native-critic/workspace diagnostic stack. Normal mode remains
the richer default.

Keep four propositions separate:

```text
lean server profile exists
synthetic Neovim-shaped protocol profile passes
actual Neovim client journey passes for an exact host/version
stable-runner latency/work budget is measured
```

No earlier proposition substitutes for a later one.

The lean profile also does **not** define text-sync or parser-reuse semantics.
Ranged client synchronization, full parser fallback, parser-input reuse, and
incremental AST work are separate evidence classes.

## Lean profile

The profile uses the existing runtime dials:

```text
PERL_LSP_E2E=1
PERL_LSP_DIAGNOSTIC_MODE=syntax-only
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0
PERL_LSP_EAGER_WORKSPACE_INDEXING=false
PERL_LSP_FILE_WATCHERS=false
```

Equivalent CLI flags are:

```bash
perllsp --stdio \
  --runtime-mode e2e \
  --diagnostic-mode syntax-only \
  --diagnostic-debounce-ms 0 \
  --eager-workspace-indexing=false \
  --file-watchers=false
```

## Existing protocol and smoke evidence

Raw-RPC wiring receipts live in
[`crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs`](../../../crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs).
They exercise server/process behavior such as:

- open → completion;
- open → hover;
- edit → parse-error diagnostic;
- edit → diagnostic clear/update;
- rapid typing → latest completion;
- inline-completion and watcher-registration behavior under the lean flags.

The lightweight actual-Neovim wiring smoke lives in
[`scripts/ux/neovim_lean_smoke.sh`](../../../scripts/ux/neovim_lean_smoke.sh).
It launches real `nvim --headless`, but intentionally suppresses some client
capabilities/features. Treat it as a wiring smoke, not the complete Neovim
support verdict.

The richer startup trace lives in
[`crates/perl-lsp-ux-tests/tests/ux_neovim_lean_startup_trace.rs`](../../../crates/perl-lsp-ux-tests/tests/ux_neovim_lean_startup_trace.rs).
Despite the name, this is a **Neovim-shaped synthetic protocol profile** run
through the repository harness; it does not launch the Neovim host. It is useful
for fast capability/registration regression checks but cannot promote an actual
client support row.

Current server behavior includes a semantic-token result-ID/delta path. Older
status text saying semantic-token delta remains unimplemented/full-only is
stale. Synthetic capability proof still remains separate from whether an exact
Neovim release advertises and consumes delta end to end.

Likewise, `perllsp` advertises/implements LSP 3.18
`workspace/textDocumentContent` for `perldoc://` content, but that server fact
is not stock-Neovim virtual-document proof. Actual client support is tracked
separately and may remain an upstream dependency for a tested Neovim row.

## Actual-host support evidence

The actual-host programme is intentionally layered:

```text
canonical activation/root/filetype contract
→ deep actual-Neovim lifecycle
→ bounded support-floor/current-stable compatibility rows
→ public-artifact/package-manager first-mile rows
```

The deep actual-host journey owns semantic/currentness correctness under edits,
recovery, rapid supersession, lifecycle races, and shutdown. The bounded version
matrix owns the cheaper host/version/capability replay, including diagnostics,
formatting, semantic-token delta, opt-in feature state, workspace configuration,
watcher capability, and virtual-document state.

Do not derive a broad `Neovim 0.11+` claim from one host receipt. Exact tested
versions/platforms belong in the bounded supported-version matrix (#7716) once
that lane lands; invalidate only the affected receipt rows when a host or
protocol subject changes.

## Rapid-edit scheduling evidence

The stale-read pressure receipt lives in
[`crates/perl-lsp-rs/src/runtime/scheduler.rs`](../../../crates/perl-lsp-rs/src/runtime/scheduler.rs)
as `rapid_typing_stale_reads_cancel_before_worker_permit_receipt`. It proves
that older generation reads cancel before taking a worker permit while the
latest generation request reaches a worker.

That is server scheduling evidence. Actual editor usefulness after a burst still
requires a current-answer client receipt.

## Focused protocol receipt bundle

A narrow server/process refresh can include:

```bash
cargo build -p perllsp --profile agent --locked
cargo xtask inline-completion-smoke --binary <agent-target>/agent/perllsp

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc -- --test-threads=1 --nocapture

PERL_LSP_E2E=1 \
PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
cargo test -p perl-lsp-ux-tests --test ux_neovim_lean_startup_trace -- --test-threads=1 --nocapture

cargo test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
cargo test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
cargo test -p perl-lsp-rs --test lsp_text_document_content_tests --profile agent --locked
cargo test -p perl-lsp-rs --test lsp_318_negative_claims --profile agent --locked
cargo xtask check-lsp-318-claims
```

On Windows, use the `.exe` binary path where applicable. When using an external
`CARGO_TARGET_DIR`, replace `<agent-target>` with that target directory.

This bundle does not replace the actual-host Neovim receipts.

## What this proves

The raw-RPC and synthetic trace receipts prove bounded server/process/capability
wiring. The lightweight headless smoke proves that a real Neovim process can
launch and communicate with the selected binary under its reduced profile.

## What this does not prove

- No incremental AST reuse is implied by the lean profile.
- A full parser fallback does not require full-document client synchronization;
  the two contracts are independent.
- CI wall-clock timing is not a stable benchmark receipt.
- Syntax-only mode does not provide the full semantic/module/native-critic/dead-code diagnostic stack.
- Server `workspace/textDocumentContent` tests do not prove stock Neovim can open
  or refresh `perldoc://` virtual buffers.
- A synthetic Neovim-shaped capability profile does not prove the actual editor.
- Exact-source actual-host proof does not prove a public release/Cargo/Homebrew/Mason install path.

## Next evidence

- Complete the exact actual-Neovim lifecycle and bounded host-version rows.
- Consume public-artifact/package-manager first-mile receipts separately from
  exact-source client proof.
- Calibrate stable-runner timing/work before publishing numeric latency budgets.
