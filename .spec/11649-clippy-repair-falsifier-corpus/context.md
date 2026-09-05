# Clippy repair-falsifier corpus slice

## Issue

[#11649](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11649)

## Scope

Freeze one versioned falsifier corpus (`ClippyRepairFalsifierCorpusV1`) covering every
required dishonest-repair and verifier-weakening family from #11649 as checked-in
fixtures plus a fail-closed validator (`cargo run -p xtask --bin
check-clippy-repair-corpus` / `cargo test -p xtask --locked clippy_repair_corpus`).
Cases whose rejecting authority is already landed bind to it exactly; cases whose
authority is still open carry an explicit `pending_owner` record with the owning issue
and stay `packet_ready: false`, so downstream packets can never consume them as green.

## Claim boundary

This covers fixture identity, digest integrity, authority binding honesty, positive-
counterpart discrimination, and deterministic validation of the frozen 50-case
denominator (families A–G). It does not execute Cargo or Clippy, decide lint quality,
own suppression admission (#11217/#11345 authorities), project required product
subjects (#11222/#11225), admit automatic suggestions (#11228), generate finding
baselines (#11407), render builder/reviewer packets (#10872/#10881/#11257), change any
lint level, config value, workflow requiredness, product subject, toolchain, or live
setting.
