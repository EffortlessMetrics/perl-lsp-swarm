# Checklist: ParseDiagnosticAnchor — Issue #11616

## Pre-implementation

- [x] Spec files created (context.md, acceptance.md, checklist.md)
- [x] Existing caller inventory: no existing ParseDiagnosticAnchor callers (type is new)
- [x] Dependency analysis: sha2 added as direct dep to perl-diagnostics (not perl-source-identity,
      to avoid unconditionally enabling serde in perl-diagnostics)

## Implementation

- [ ] `crates/perl-diagnostics/src/anchor.rs` created with:
  - [ ] `SourceDigest` type (SHA-256 domain-separated, `perl-lsp:anchor-source-digest:v1\0`)
  - [ ] `ParseDiagnosticAnchor` struct (`span: ByteSpan`, `minted_digest: SourceDigest`)
  - [ ] `AnchorResolution` enum (`Current(ByteSpan)`, `Stale{..}`, `NotProven`)
  - [ ] `BatchFreshnessChecker` struct (cached `Option<SourceDigest>`)
  - [ ] serde derives behind `#[cfg(feature = "serde")]`
- [ ] `crates/perl-diagnostics/src/lib.rs` updated to declare `pub mod anchor`
- [ ] `crates/perl-diagnostics/src/api.rs` updated to re-export new types
- [ ] `crates/perl-diagnostics/Cargo.toml` updated to add `sha2 = { workspace = true }`

## Tests

- [ ] `crates/perl-diagnostics/tests/parse_diagnostic_anchor_tests.rs` created with:
  - [ ] All shift-left falsifiers from acceptance.md covered
  - [ ] Serde round-trip test (cfg-gated)
  - [ ] BatchFreshnessChecker caching behavior
  - [ ] NotProven when source None
  - [ ] AnchorResolution variants exhaustive

## Verification

- [ ] `cargo fmt -p perl-diagnostics -- --check` passes
- [ ] `cargo clippy -p perl-diagnostics --all-targets --locked -- -D warnings` passes
- [ ] `cargo test -p perl-diagnostics --all-targets --locked` passes

## Review-forward

- [ ] No unconditional serde dep added (serde still optional in perl-diagnostics)
- [ ] sha2 version matches workspace pin (sha2 = "0.11.0")
- [ ] No panic/unwrap/expect in production code paths
