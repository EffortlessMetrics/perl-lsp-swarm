# Perl LSP Swarm Rust Small Proof

This repository is the high-volume same-repo PR workspace for
`EffortlessMetrics/perl-lsp`.

The first protected swarm lane is `Perl LSP Rust Small Result`. Branch
protection must require that normalized result, not the conditional
implementation jobs for CX53, CX43, or GitHub-hosted fallback.

Initial proof captured:

- same-repo PR fallback route: `26146166886`;
- forced CX43 backfill route: `26146635076`;
- forced CX53 primary route: `26147069092`.

Release, publish, signing, extension, and secrets-heavy workflows remain owned
by the source repository until a separate deliberate migration.

## Lane receipt

Every `cargo run -p xtask --locked -- rust-small-proof` run emits one versioned
receipt (`rust_small_proof.v1`) to `target/receipts/rust-small-proof.json`, or
to `--receipt <path>`. It binds the exact subject the proof ran against — the
candidate SHA, `rustc`/`cargo` versions, and the scorecard profile/features
read out of the pinned argv — to a typed outcome for every selected step
(`ok`, `product_failure`, `not_completed`, `instrument_failure`, `not_run`).

A failed lane still emits a complete receipt: the failing step keeps its
classification and every step the lane never reached is recorded `not_run`, so
an omitted step and an unreached step stay distinguishable.

`--verify-receipt <path>` re-reads a receipt against the current checkout and
exits nonzero on a malformed or stale schema, a missing/extra/reordered step,
argv that is not the pinned lane argv, a success claimed over a non-`ok` step
or a zero census, or a subject that is not this candidate/toolchain/profile.
It runs no proof steps, so it is the cheap consumer seam for asking whether an
artifact actually certifies this candidate.
