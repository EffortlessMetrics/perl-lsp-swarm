# perl-sync-convergence

Canonical `perl_lsp.convergence_transaction.v1` state and event model for
continuous source synchronization (#11282): durable transaction identity,
immutable generation receipts, writer leases with takeover, the invalidation
graph, and journal-based resumability.

Tier-1 leaf crate. Depends only on `serde` (stable serialization) and `sha2`
(domain-separated generation digests); no parser, workspace, LSP/DAP/editor,
async-runtime, Git-subprocess, or network dependencies. The closure is
asserted exactly by `tests/dependency_contract.rs`.

## Wire formats

- Generation IDs: `gen:sha256:<64 lowercase hex digits>`, derived via
  domain-separated SHA-256 over length-prefixed exact inputs (direction,
  release mode, source repo/parent SHA/tree, swarm repo/parent SHA/tree,
  prior accepted generation). Wrong prefixes or uppercase hex are rejected.
- Receipts: `GenerationReceiptFile { schema_version, receipt }` with
  `schema_version = 1`; unsupported versions fail closed on read.
- Journals: newline-delimited JSON, each line
  `{ "schema_version": 1, ...event }`; replay is deterministic and
  fail-closed.

## Persistence layout

```text
<root>/index.v1.json
<root>/transactions/<transaction_id>/events.v1.jsonl
<root>/transactions/<transaction_id>/generations/<generation_id>.json
```

Small canonical state only: no secrets, credentials, raw host paths, or
unbounded logs. Large artifacts are represented by digests plus retention
class and durable-copy state; an expiring artifact URL is never the sole
retained authority.

## Fail-closed guarantees

- Unsupported schema versions (index, journal, receipts) abort loading.
- Malformed JSON or unknown enum spellings abort loading; nothing degrades.
- Existing generation receipts are immutable: identical canonical rewrites
  succeed idempotently, different bytes are refused. A moved input produces a
  successor generation instead.
- Two active generations claiming the same direction and source parent
  without supersession are refused at append time.
- An expired lease is reclaimable only through a recorded takeover carrying
  reconciliation observations; a live lease grants no merge authority.
- Rejected evidence stays rejected: terminal generations accept no further
  transitions, so later green checks cannot rewrite prior rejection.
- `not_proven` and `instrument_failure` are distinct terminal outcomes that
  can never fold into a passing state.

## Focused validation

```bash
cargo test -p perl-sync-convergence --all-targets --locked
```
