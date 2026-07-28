# Selected upstream evidence

This directory stores immutable normalized evidence packets for the selected
`base`, `comp`, and `run` comparison series. Each packet is keyed by the
measurement commit and retains its own series manifests, preparation receipt,
reports, boundary inventory, gap map, baselines, reproduction descriptor, and
bundle indexes.

The `f5fda210bd37e4d00e9ccb273367b4ddda4a6ae8` packet was measured by Linux
workflow run [30408910747](https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/30408910747)
with Rust 1.97 and Perl SHA
`b62845c7186b0b6a8e4e83419e6b5ef64ceef3ed`.

It establishes selected parse/compile evidence only. It does not claim
profile-wide execution, general semantic support, or zero semantic debt.

Validate a packet locally with:

```text
xtask perl-core-harness bundle --check --bundle-dir <profile-bundle>
```
