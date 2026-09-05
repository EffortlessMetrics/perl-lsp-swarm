# SEM-ID-01 acceptance evidence

- `cargo test -p perl-semantic-facts --lib semantic_identity` — 15 passed, 0 failed.
- `cargo test -p perl-semantic-facts --lib` — full crate green (recorded in PR).
- `cargo fmt -p perl-semantic-facts -- --check` clean on changed regions.
- `cargo clippy -p perl-semantic-facts --all-targets --locked -- -D warnings` clean.
- No production `unwrap`/`expect`/`panic!` added; constructors return
  `Result<_, SemanticIdentityContractError>`.

Accepted limitation: same-anchor sibling disambiguation is a parent-local
ordinal (see context.md); reordering identical-anchor siblings yields distinct
identities, which conservatively forces recompute rather than false reuse.
