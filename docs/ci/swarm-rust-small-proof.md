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

Any receipt left by an earlier run is destroyed before the first fallible step.
`target/` is reused across runs, so without that a failed rerun of the same
candidate could leave the previous run's green receipt in place, still
verifying — an artifact describing a run that did not happen.

`--verify-receipt <path>` re-reads a receipt against the current checkout and
runs no proof steps, so it is the cheap consumer seam for asking whether an
artifact actually certifies this candidate. It exits nonzero on:

- a malformed or stale schema version;
- a missing, extra, renamed, or reordered step, or argv that is not the pinned
  lane argv;
- a success claimed over a non-`ok` step, or over a zero/absent census;
- a failure result over steps that all recorded `ok`, a terminal result that
  contradicts the first failing step, `not_run` steps that do not form a
  suffix, a census count from a step that never ran, or an outcome and exit
  code that cannot co-occur;
- a subject that is not this candidate, toolchain, or scorecard profile.

**Trust boundary.** The receipt certifies its subject *as observed at capture
time*. Subject capture is the trust root: the producer's self-check and
`--verify-receipt` both call the same capture code, so a capture that reported
the wrong identity would be agreed on by both sides. Detecting that would mean
recording raw command output and re-executing it to byte-compare at
verification time — a design change, not a tightening of the current check.
