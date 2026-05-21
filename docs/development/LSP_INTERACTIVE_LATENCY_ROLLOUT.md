# LSP Interactive Latency Rollout

> **Substrate (already built)**: cancellation registry typed on `JsonRpcId` (#223); bounded-i32 outbound allocator and typed server-request IDs (#221).
> **Connector gap**: the live editor still pays the full editor-loop tax — `didOpen` → full parse → all diagnostics → semantic tokens → file-watcher registration → eager workspace indexing — before first useful hover/completion. Latency harnesses measure that whole tax, not the user-visible request.
> **0.15.x upside**: a clean Neovim/LSP4IJ/VS Code harness path where first-useful hover and completion latency are isolated from avoidable background work, and stale background work no longer defines worst-case latency.

## Doctrine

This rail fixes **avoidable** live-editor latency. It is the IDE-quality-lift companion to the JSON-RPC migration ([#224](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/224)).

It does **NOT**:

- Implement true incremental AST reuse.
- Change `TextDocumentSyncKind`.
- Touch parser grammar, Tree-sitter, receiver facts, Rails, or release prep.

If a PR in this rail expands into incremental parsing, parser-grammar changes, or `TextDocumentSyncKind` changes, **the PR is wrong-scope** and gets bounced back. This doc is the bouncer.

## Status

| Phase | Issue / PR | Scope | Stack on |
|---|---|---|---|
| 1. Scope-lock doc | this doc — [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | docs only | main |
| 2. Timing probes | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | `PERL_LSP_TIMING=1` opt-in instrumentation | main |
| 3. E2E runtime mode | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | `--runtime-mode e2e`, workload profile for harnesses | main |
| 4. Syntax-only diagnostics | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | `--diagnostic-mode syntax-only` | main |
| 5. didOpen diagnostic defer | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | fast parse errors + debounced full diags | main |
| 6. Pull-diagnostics short-circuit | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | skip push computation for pull-only clients | main |
| 7. Eager-indexing off in e2e | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | startup workspace scan gated by mode | main |
| 8. Latest-only diagnostics | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | discard stale full-diag computations | #223 |
| 9. Generation-aware stale cancellation | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | cancel reads where `req.generation < doc.generation` | #223 + Phase 4 |
| 10. Semantic-token contract cleanup | [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229) | stop advertising `delta` until cache exists | main |

Phases 2–7 and 10 can build in parallel — they touch disjoint subsystems.
Phases 8 and 9 stack on [#223](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/223) (cancellation-registry typed migration) because they live on the cancellation surface.

## Exit criteria

- [ ] Every phase lands or is explicitly deferred with a successor.
- [ ] Receipt command in this doc reproduces the closeout proof.
- [ ] A Neovim harness can run `perllsp --runtime-mode e2e --diagnostic-mode syntax-only --diagnostic-debounce-ms 0` and exercise hover/completion in isolation from background work.
- [ ] `PERL_LSP_TIMING=1` writes per-phase latency receipts to a non-stdout sink.
- [ ] Status doc updated (`docs/project/status/lsp.md` regenerated post-merge).

## Claim boundary

This rail proves that **first-useful hover and completion latency in interactive mode is isolated from avoidable background work**: workload profiles, deferrals, and stale-work cancellation remove the editor-loop tax from latency measurement.

This rail does **NOT** prove:

- That the parser can reuse AST across `didChange` edits (no incremental parse).
- That diagnostics never recompute (only stale recomputations are discarded; valid recomputations on settled state still run).
- That `TextDocumentSyncKind` changes — the rail explicitly does not modify sync kind.
- That LSP4IJ and Neovim have identical latency profiles — both clients benefit, but per-client tuning is out of rail.

## Hard rules

- **Timing logs never go to LSP stdout.** Timing probes write to `stderr` or a configured file sink. `LSP_TIMING` accidentally sent to stdout would corrupt the JSON-RPC stream. This is the single non-negotiable contract from PR 2 forward.
- **E2E mode is a workload profile, not a feature profile.** It changes runtime defaults for tests/harnesses; it does not change advertised LSP capabilities (with the documented exception of semantic-token de-advertise in Phase 10, which is independent of e2e mode).
- **Latest-only is generation-aware, not position-aware.** Position-based dedupe is insufficient because typing moves the cursor; document generation is the right discriminator.

## Receipts

```bash
# Phase 2 (timing probes)
PERL_LSP_TIMING=1 cargo run -p perl-lsp-rs --release -- --stdio 2>timing.log
# Inspect timing.log for didOpen / didChange / parse / diagnostics / queue-wait phases.

# Phase 3 (e2e mode)
perllsp --runtime-mode e2e  # confirm e2e defaults applied via /tmp/lsp4ij* or harness

# Phase 4 (syntax-only)
perllsp --diagnostic-mode syntax-only  # confirm only parser errors published

# Phase 5 (didOpen defer)
# Time between didOpen send and first publishDiagnostics with non-parser errors
# should be ≥ debounce interval; parser-error publish should be < 50ms.

# Phase 7 (eager-indexing off in e2e)
perllsp --runtime-mode e2e  # confirm workspace scan does not start before first request

# Phase 8 (latest-only diagnostics)
# Send didChange v2 mid-diag-computation for v1. Assert only v2 publishes.

# Phase 9 (stale read cancellation)
# Send hover v1 then didChange v2 immediately. Assert hover v1 returns cancelled,
# not stale results computed against v1.

# Phase 10 (semantic-token de-advertise)
# Inspect initialize response: semanticTokensProvider does not advertise delta.
```

## Neovim harness reference

Once Phases 2–7 land:

```lua
local caps = vim.lsp.protocol.make_client_capabilities()

if caps.workspace then
  caps.workspace.didChangeWatchedFiles = nil
end

vim.lsp.config('perl_lsp', {
  cmd = {
    '/absolute/path/to/target/release/perllsp',
    '--stdio',
    '--runtime-mode', 'e2e',
    '--diagnostic-mode', 'syntax-only',
    '--diagnostic-debounce-ms', '0',
  },
  capabilities = caps,
  on_attach = function(client, bufnr)
    vim.lsp.semantic_tokens.enable(false, {
      client_id = client.id,
      bufnr = bufnr,
    })
  end,
})
```

The harness should assert against the **latest request/version**, not global editor idle after every keystroke. After Phase 9, stale-request cancellation makes that assertion mechanical instead of timing-dependent.

## Out-of-rail follow-up

The real long-term architecture lives in a separate **incremental parse rail** (not yet filed):

```
didChange
  -> apply text
  -> enqueue latest parse job
  -> return

parse worker
  -> parse latest version
  -> discard stale result
  -> commit current AST
```

That rail does change `TextDocumentSyncKind` semantics and the parser entry point. It is **explicitly out of scope here**. The latency rail is the cheap, contained set of wins that ship before incremental parse is even designed.

## Related

- Umbrella issue: [#229](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/229)
- JSON-RPC migration: [#224](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/224) (#221 merged, #223 in flight)
- Rails index: [`docs/project/RAILS_INDEX.md`](../project/RAILS_INDEX.md)
- Rail template: [`docs/project/RAIL_TEMPLATE.md`](../project/RAIL_TEMPLATE.md)
