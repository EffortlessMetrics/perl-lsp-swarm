# LSP Interactive Latency Rollout

## Follow-up specs

This rail intentionally does not solve every editor-performance problem. When this rail closes, open these follow-up rails as needed.

### 1) `LSP_INCREMENTAL_PARSE_ARCHITECTURE.md`

- **Owns:** moving parse off the mutation worker, latest parse jobs per URI, AST freshness state, and eventual true incremental AST reuse.
- **Out of scope for first rail:** the first rail removes avoidable work and adds receipts without changing core parser architecture.
- **Open when:** immediately after the first latency rail lands; this is the highest-priority follow-up.
- **Must not combine with:** semantic-token delta implementation or broad diagnostics lane restructuring.

### 2) `LSP_PROVIDER_FRESHNESS_AND_FALLBACK.md`

- **Owns:** provider behavior contracts for `Current`/`Stale`/`Missing` analysis states and stale-result claim boundaries.
- **Out of scope for first rail:** the first rail reduces latency waste but does not redefine provider correctness semantics during AST catch-up.
- **Open when:** as soon as async/latest-only parse behavior is planned or starts landing.
- **Must not combine with:** parser implementation changes that move parse off `didChange`; this spec is contract-first.

### 3) `LSP_DIAGNOSTIC_PIPELINE_ARCHITECTURE.md`

- **Owns:** diagnostics lane split (syntax, semantic, workspace, external), scheduling, cancellation, freshness, and pull/push contract alignment.
- **Out of scope for first rail:** the first rail adds latest-only and deterministic behavior, not full multi-lane pipeline architecture.
- **Open when:** after first-rail measurements validate where diagnostic contention remains.
- **Must not combine with:** incremental parse architecture changes in the same PR/spec rail.

### 4) `LSP_WORKSPACE_INDEX_LIFECYCLE.md`

- **Owns:** lazy/progressive indexing lifecycle, partial-index semantics, degraded states, and file-watcher storm handling.
- **Out of scope for first rail:** the first rail only gates eager indexing for e2e determinism; it does not define normal-mode lifecycle policy.
- **Open when:** if startup or first-useful-response remains noisy in normal editor mode.
- **Must not combine with:** e2e-mode behavior changes already owned by the first latency rail.

### 5) `LSP_SEMANTIC_TOKENS_DELTA.md`

- **Owns:** real semantic-token delta (`resultId`, token cache, delta calculation, invalidation, memory limits, client compatibility tests).
- **Out of scope for first rail:** the first rail may make capability advertising honest without implementing full delta machinery.
- **Open when:** only if the first rail de-advertises semantic-token delta, or when delta is explicitly prioritized next.
- **Must not combine with:** parser architecture changes; keep semantic-token delta isolated.

### 6) `LSP_NEOVIM_HARNESS_CONTRACT.md`

- **Owns:** live Neovim harness profiles, assertion boundaries, and measurement discipline (request-scoped vs global idle).
- **Out of scope for first rail:** the first rail adds receipts but does not fully define harness product contracts.
- **Open when:** if harness runs still measure the wrong path after first-rail fixes.
- **Must not combine with:** runtime LSP behavior rewrites; this rail is test/harness contract only.

### 7) `LSP_EXTERNAL_TOOL_LATENCY.md`

- **Owns:** subprocess/filesystem latency policy (timeouts, caching, on-save vs on-change, scan budgets, failure messaging).
- **Out of scope for first rail:** the first rail addresses immediate interaction latency, not all external-tool scheduling policies.
- **Open when:** once core interaction path is stabilized and external-tool costs become the dominant residual source.
- **Must not combine with:** broad diagnostic scheduling refactors except minimal rules needed to keep external tools off live edit paths.

Do not combine these follow-up rails with the first latency rail. The first rail removes avoidable work and adds receipts; follow-up rails change architecture.
