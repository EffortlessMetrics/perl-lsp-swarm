# Checklist: StaleSourceAnchor — Issue #11616

## Pre-implementation

- [x] Spec files created (context.md, acceptance.md, checklist.md)
- [x] Existing caller inventory: no existing `StaleSourceAnchor` callers (type is new);
      `perl_parser_core::ParseDiagnosticAnchor` (#6941) owns the position contract and
      is deliberately NOT duplicated — this is the freshness layer (see context.md)
- [x] Dependency analysis: sha2 added as direct dep to perl-diagnostics (not perl-source-identity,
      to avoid unconditionally enabling serde in perl-diagnostics)

## Implementation

- [x] `crates/perl-diagnostics/src/anchor.rs` created with:
  - [x] `SourceDigest` type (SHA-256 domain-separated, `perl-lsp:anchor-source-digest:v1\0`)
  - [x] `StaleSourceAnchor` struct (`span: ByteSpan`, `minted_digest: SourceDigest`)
  - [x] `AnchorResolution` enum (`Current(ByteSpan)`, `Stale{..}`, `NotProven`)
  - [x] `BatchFreshnessChecker` struct (cached `Option<SourceDigest>`)
  - [x] serde derives behind `#[cfg(feature = "serde")]`, trait calls fully qualified
        for optional-feature compilation
- [x] `crates/perl-diagnostics/src/lib.rs` updated to declare `pub mod anchor`
- [x] `crates/perl-diagnostics/src/api.rs` updated to re-export new types
- [x] `crates/perl-diagnostics/Cargo.toml` updated to add `sha2 = { workspace = true }`

## Tests

Deviation from original plan: tests live inline in `anchor.rs`
(`#[cfg(test)] mod tests`) rather than a separate
`tests/parse_diagnostic_anchor_tests.rs` integration file; same falsifiers,
cheaper layer.

- [x] All shift-left falsifiers from acceptance.md covered
- [x] Serde round-trip tests (feature-gated)
- [x] BatchFreshnessChecker caching behavior, including discriminating
      once-per-snapshot proof (`batch_checker_resolves_later_calls_against_first_snapshot`)
- [x] NotProven when source None
- [x] AnchorResolution variants exhaustive

## Verification

- [x] `cargo fmt -p perl-diagnostics -- --check` passes
- [x] `cargo clippy -p perl-diagnostics --all-targets --locked -- -D warnings` passes
- [x] `cargo test -p perl-diagnostics --all-targets --locked`: anchor surface fully green
      (lib 43/43 incl. all 26 anchor tests + discriminating cache proof; catalog/codes
      unit binaries green). The 2 failures in `codes_comprehensive_unit_tests.rs`
      (`from_message_parse_error`, `from_message_syntax_error_matches_parse_error`)
      are the pre-existing stale assertions documented at branch point and fixed on
      origin/main by #10549 (commit 095ade8b9) — files this candidate does not touch.
- [x] `cargo test -p perl-diagnostics --all-features --all-targets --locked`: serde
      path compiles and round-trips (51/51 lib tests incl. rejection tests)

## Review-forward

- [x] No unconditional serde dep added (serde still optional in perl-diagnostics)
- [x] sha2 version matches workspace pin (sha2 = "0.11.0")
- [x] No panic/unwrap/expect in production code paths
