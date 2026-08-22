# Acceptance: StaleSourceAnchor (parse-diagnostic stale-source disposition) — Issue #11616

## Core type API

- [x] `StaleSourceAnchor::mint(span: ByteSpan, source: &[u8]) -> Self` constructs
      an anchor for a given span and source text.
- [x] `StaleSourceAnchor::span() -> ByteSpan` returns the minted span.
- [x] `StaleSourceAnchor::resolve_for_current(source: &[u8]) -> AnchorResolution`
      returns:
      - `AnchorResolution::Current(span)` when the source digest matches the
        minted digest;
      - `AnchorResolution::Stale { minted_digest, current_digest }` when they differ;
      - (no `NotProven` from `resolve_for_current` directly — the caller may produce
        `NotProven` from a batch checker when source is unavailable).

## AnchorResolution variants

- [x] `AnchorResolution::Current(ByteSpan)` — anchor valid for current source.
- [x] `AnchorResolution::Stale { minted_digest: SourceDigest, current_digest: SourceDigest }`
      — source changed; includes both digests for traceability.
- [x] `AnchorResolution::NotProven` — freshness cannot be established (source
      unavailable, digest unknown, or `BatchFreshnessChecker` constructed without
      a current source).

## SourceDigest

- [x] `SourceDigest` is a domain-separated SHA-256 of source bytes.
- [x] `SourceDigest::of_bytes(source: &[u8]) -> Self` is deterministic.
- [x] `SourceDigest` implements `PartialEq`, `Eq`, `Hash`, `Clone`, `Debug`.
- [x] Wire representation (via `Display`) is `anchor-digest:<64 lowercase hex>`.
- [x] `SourceDigest` equality is based on the raw hash bytes, not the wire string.

## BatchFreshnessChecker

- [x] `BatchFreshnessChecker::new() -> Self` constructs an uncached checker.
- [x] `BatchFreshnessChecker::for_source(source: &[u8]) -> Self` constructs with
      a pre-computed current digest.
- [x] `BatchFreshnessChecker::check(&mut self, anchor: &StaleSourceAnchor, source: Option<&[u8]>) -> AnchorResolution`
      computes the current digest at most once per source snapshot.
- [x] Passing `None` as source returns `NotProven`.
- [x] Passing `Some(source)` on a second call with the same source does NOT
      recompute the SHA-256 — the cached digest is reused.

## Shift-left falsifiers (must be tested)

- [x] An anchor minted from `source_a` returns `Stale` when resolved against `source_b`.
- [x] An anchor minted from `source_a` returns `Current` when resolved against the same bytes.
- [x] `BatchFreshnessChecker` with no current source returns `NotProven`.
- [x] `BatchFreshnessChecker::check` computes the digest only once — proven by the
      discriminating test `batch_checker_resolves_later_calls_against_first_snapshot`,
      which passes *different* source bytes on later calls and asserts results still
      resolve against the first snapshot's digest (a per-call recomputation would flip
      them to `Stale`). No instrumented test-double needed.
- [x] Two `SourceDigest` values for the same bytes are equal across separate computations.
- [x] Two `SourceDigest` values for different bytes are not equal.
- [x] An empty source `b""` produces a valid, deterministic `SourceDigest`.

## Serde support

- [x] When the `serde` feature of `perl-diagnostics` is enabled, `StaleSourceAnchor`,
      `AnchorResolution`, and `SourceDigest` all derive `Serialize`/`Deserialize`.
- [x] Round-trip: serialize then deserialize produces an equal value.
- [x] The serde feature is still optional (no unconditional serde dep added).
- [x] The hand-written `SourceDigest` Deserialize impl qualifies trait method calls
      explicitly (`<String as serde::Deserialize>::deserialize`) so the optional
      feature compiles without relying on trait-import side effects (`--all-features`
      proof).

## Non-goals

- [x] Does NOT mint anchors at the parser call site (parser scope, separate issue).
- [x] Does NOT consume anchors in LSP providers (LSP scope, separate issue).
- [x] Does NOT replace `ContentDigest` from `perl-source-identity` (different scope).
- [x] Does NOT add a process-wide anchor registry or cache.
- [x] Does NOT reopen or extend `perl_parser_core::ParseDiagnosticAnchor` (#6941):
      that type owns diagnostic *position* resolution; this contract owns *freshness*.
