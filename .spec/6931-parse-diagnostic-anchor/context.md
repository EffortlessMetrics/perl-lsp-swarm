# Context: StaleSourceAnchor (parse-diagnostic stale-source disposition) — Issue #6931 / #11616

## Background

`StaleSourceAnchor` is a lightweight pair of a [`ByteSpan`] and a source-text
digest, emitted together with each parse diagnostic. Its purpose is to
allow downstream consumers (LSP handlers, incremental-parse pipelines, workspace
indexers) to ask a single question:

> "Is the diagnostic I received still valid for the source text I have now?"

Without an anchor, a consumer holding a `ByteSpan` from a previous parse must
re-run the full parse to discover whether the diagnostic is still accurate.
With an anchor, it can check freshness in O(1) per batch.

## Ownership boundary (resolved 2026-08-22 repair)

Merged #6941 already owns `perl_parser_core::ParseDiagnosticAnchor`, a *position*
resolver (`Exact`/`EndOfInput`/`NoSource`/`InvalidOffset`/`InvalidUtf8Boundary`
resolved against source length). This contract deliberately does NOT duplicate
that name or reopen that contract: it is the complementary *freshness* layer.
The perl-diagnostics type was renamed from `ParseDiagnosticAnchor` to
`StaleSourceAnchor` so the two authorities cannot be confused at import time.

## Related issues

| Issue | Role |
|-------|------|
| #6941 | Lands the `ParseDiagnosticAnchor` base contract (source-text resolution, `InvalidUtf8Boundary`) — **merged**; owns position resolution |
| #6931 | Closed as superseded by #6941; carried one genuine P2 remainder: stale-source disposition |
| #11616 | This work — implements `resolve_for_current` with once-per-batch freshness constraint |

## Authority split

- **This issue** owns: `StaleSourceAnchor` type, `AnchorResolution` enum,
  `SourceDigest` internal type, `BatchFreshnessChecker`, and tests in
  `crates/perl-diagnostics/`.
- **Parser** owns: minting anchors at the call site where diagnostics are emitted.
  (Not in scope here.)
- **LSP providers** own: consuming `BatchFreshnessChecker` per on-disk or
  in-memory source snapshot. (Not in scope here.)
- **`perl-source-identity`** owns: durable cross-repository source identity for
  workspace indexing. `SourceDigest` here is a lighter parser-scoped concept;
  it is not a `ContentRevision` or `ContentDigest` in the source-identity sense.

## Source-digest design

The anchor captures a SHA-256 digest of the exact bytes of the source text at
parse time. Two source texts that differ in at least one byte produce different
digests; two identical source texts always produce the same digest, even across
processes and machines.

The digest uses domain separation (`perl-lsp:anchor-source-digest:v1\0`) to
ensure it cannot be confused with other SHA-256 digests in the codebase.

## Once-per-batch freshness

A batch is a logical unit of work against one source snapshot (for example,
processing all diagnostics from a single `parse_with_recovery` call). Within a
batch, the source text does not change, so comparing each anchor's `SourceDigest`
against the current source on every call is redundant.

`BatchFreshnessChecker` solves this by caching the current `SourceDigest` on the
first call within a batch, then reusing it for all subsequent calls. Callers must
construct a new `BatchFreshnessChecker` for each new source snapshot.

## Freshness contract invariants

- A missing or unknown source digest → `NotProven`, never `Current`.
- A stale result always identifies both the minted and current digests.
- `BatchFreshnessChecker` never reuses a digest from a previous batch; it must be
  re-constructed per snapshot.
- `SourceDigest` equality is byte-by-byte; no fuzzy comparison.
