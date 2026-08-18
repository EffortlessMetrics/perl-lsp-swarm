# Acceptance: ParseDiagnosticAnchor — Issue #11616

## Core type API

- [ ] `ParseDiagnosticAnchor::mint(span: ByteSpan, source: &[u8]) -> Self` constructs
      an anchor for a given span and source text.
- [ ] `ParseDiagnosticAnchor::span() -> ByteSpan` returns the minted span.
- [ ] `ParseDiagnosticAnchor::resolve_for_current(source: &[u8]) -> AnchorResolution`
      returns:
      - `AnchorResolution::Current(span)` when the source digest matches the
        minted digest;
      - `AnchorResolution::Stale { minted_digest, current_digest }` when they differ;
      - (no `NotProven` from `resolve_for_current` directly — the caller may produce
        `NotProven` from a batch checker when source is unavailable).

## AnchorResolution variants

- [ ] `AnchorResolution::Current(ByteSpan)` — anchor valid for current source.
- [ ] `AnchorResolution::Stale { minted_digest: SourceDigest, current_digest: SourceDigest }`
      — source changed; includes both digests for traceability.
- [ ] `AnchorResolution::NotProven` — freshness cannot be established (source
      unavailable, digest unknown, or `BatchFreshnessChecker` constructed without
      a current source).

## SourceDigest

- [ ] `SourceDigest` is a domain-separated SHA-256 of source bytes.
- [ ] `SourceDigest::of_bytes(source: &[u8]) -> Self` is deterministic.
- [ ] `SourceDigest` implements `PartialEq`, `Eq`, `Hash`, `Clone`, `Debug`.
- [ ] Wire representation (via `Display`) is `anchor-digest:<64 lowercase hex>`.
- [ ] `SourceDigest` equality is based on the raw hash bytes, not the wire string.

## BatchFreshnessChecker

- [ ] `BatchFreshnessChecker::new() -> Self` constructs an uncached checker.
- [ ] `BatchFreshnessChecker::for_source(source: &[u8]) -> Self` constructs with
      a pre-computed current digest.
- [ ] `BatchFreshnessChecker::check(&mut self, anchor: &ParseDiagnosticAnchor, source: Option<&[u8]>) -> AnchorResolution`
      computes the current digest at most once per source snapshot.
- [ ] Passing `None` as source returns `NotProven`.
- [ ] Passing `Some(source)` on a second call with the same source does NOT
      recompute the SHA-256 — the cached digest is reused.

## Shift-left falsifiers (must be tested)

- [ ] An anchor minted from `source_a` returns `Stale` when resolved against `source_b`.
- [ ] An anchor minted from `source_a` returns `Current` when resolved against the same bytes.
- [ ] `BatchFreshnessChecker` with no current source returns `NotProven`.
- [ ] `BatchFreshnessChecker::check` called N times with the same source computes
      the digest only once (verified by counting calls / or using a deterministic
      test-double approach).
- [ ] Two `SourceDigest` values for the same bytes are equal across separate computations.
- [ ] Two `SourceDigest` values for different bytes are not equal.
- [ ] An empty source `b""` produces a valid, deterministic `SourceDigest`.

## Serde support

- [ ] When the `serde` feature of `perl-diagnostics` is enabled, `ParseDiagnosticAnchor`,
      `AnchorResolution`, and `SourceDigest` all derive `Serialize`/`Deserialize`.
- [ ] Round-trip: serialize then deserialize produces an equal value.
- [ ] The serde feature is still optional (no unconditional serde dep added).

## Non-goals

- [ ] Does NOT mint anchors at the parser call site (parser scope, separate issue).
- [ ] Does NOT consume anchors in LSP providers (LSP scope, separate issue).
- [ ] Does NOT replace `ContentDigest` from `perl-source-identity` (different scope).
- [ ] Does NOT add a process-wide anchor registry or cache.
