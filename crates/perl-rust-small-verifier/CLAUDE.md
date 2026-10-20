# CLAUDE.md (perl-rust-small-verifier)

## Role

Lightweight executable entrypoints for the canonical Rust Small proof and receipt
verifier. The package imports the owning task source rather than maintaining a
separate receipt schema or validation implementation.

## Owns

- Verification-only default CLI argument handling.
- Explicit no-argument producer entrypoint.
- CLI boundary tests for argument refusal and producer behavior.

## Does not own

- Canonical proof steps, receipt semantics or artifact-admission rules, which
  belong to the shared task modules.
- Native CI admission, artifact transport, required-result policy or runner access.
- A successful product result merely because a diagnostic receipt verifies.

## Read first

- `Cargo.toml`
- `src/lib.rs`
- `src/main.rs`
- `src/bin/perl-rust-small-proof.rs`
- `xtask/src/tasks/rust_small_proof.rs`
- `xtask/src/tasks/rust_small_proof/artifact_admission.rs`

## Focused validation

- `cargo test -p perl-rust-small-verifier --locked`
- `cargo clippy -p perl-rust-small-verifier --all-targets --locked -- -D warnings -A missing_docs`
- `tests/verify.rs`
- `tests/produce.rs`

Producer tests use controlled command fixtures. Their success does not establish
cold product execution, authenticated native transport or required-check acceptance.
