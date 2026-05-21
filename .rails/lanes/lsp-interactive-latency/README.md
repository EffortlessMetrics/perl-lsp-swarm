# LSP Interactive Latency Lane (Track D)

Track D improves live editor responsiveness (especially Neovim) for perl-lsp runtime behavior.

## Ownership boundaries
- Track A: parser-target fairness
- Track B: production parser edge-gap closure
- Track C: semantic receiver intelligence
- Track D: runtime/editor latency

## Scope
Live LSP critical path: didOpen/didChange, diagnostics/debounce, startup indexing behavior, scheduler wait, stale-read cancellation, semantic-token capability honesty, and latency evidence/budgets.

## First-phase constraint
No true incremental AST reuse in this lane phase; text sync remains Full until proven incremental AST reuse exists.
