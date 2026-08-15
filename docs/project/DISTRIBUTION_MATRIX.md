# Distribution Matrix

This page separates the places where perl-lsp can be obtained from the evidence
needed to claim that each channel is current. A workspace version or a GitHub
Release does not prove that another channel has published the same build.

## Current release line

- Workspace line: `v0.17.0`.
- Product posture: public beta, not stable or GA.
- GitHub Release: `v0.17.0` shipped on 2026-06-28 with platform archives, VSIX,
  `SHA256SUMS`, and an SPDX SBOM.
- Next planned train: `v0.18.0`; no candidate is staged in this development
  repository.

The narrative release status and channel receipts are authoritative:
[release readiness](status/release.md). The matrix below is a routing guide,
not a second generated status ledger.

## Channel matrix

| Channel | User path | What is supported by the current repository evidence | Boundary |
| --- | --- | --- | --- |
| GitHub Releases | Download a platform archive from the release page | `v0.17.0` archives and checksums are recorded as shipped | Verify the archive and checksum before use |
| VS Code Marketplace | Install `EffortlessMetrics.perl-lsp-rs` | Extension publication is independently versioned | Marketplace state is not proved by the GitHub Release receipt alone |
| Open VSX | Install `EffortlessMetrics.perl-lsp-rs` | Separate registry path for VSCodium and compatible clients | Check the displayed extension version |
| crates.io | Install `perllsp` or consume published crates | Registry publication is independently versioned | Do not infer all 32 crate versions from the workspace manifest |
| Homebrew | `brew install effortlessmetrics/tap/perllsp` | Owned tap path | Formula freshness and archive checksums require their own receipt |
| Docker | Use the project image when a current image receipt exists | Channel remains pending/not proven in the current release ledger | Do not present an image tag as current without a receipt |
| Build from source | Build the selected branch or tag locally | Always available as a contributor path | A source build is not a published-release receipt |

## Installation guidance by need

- For the most directly evidenced public binary, start with the GitHub Release
  archive and verify its checksum.
- For VS Code or VSCodium, use the matching marketplace path and inspect the
  installed extension version before relying on managed binary download.
- For crate consumers, read the release notes and stability policy before
  updating dependencies; published support crates can move independently from
  the workspace development tree.
- For offline or enterprise environments, use a pre-approved archive or an
  internal mirror and set the extension's `serverPath` or download base URL as
  documented in the installation guides.

## Support tiers

| Surface | Tier | Interpretation |
| --- | --- | --- |
| `perllsp` binary and core editor workflows | Public beta | Usable and actively hardened; not a GA promise |
| `perl-lsp-rs`, `perl-parser`, `perl-dap`, `perl-uri` facades | Public beta with the highest compatibility expectation | Patch releases preserve the API; pre-1.0 minor breaks require migration guidance |
| Other published support crates | Public beta, faster-moving | Check release notes and SemVer receipts before depending directly |
| DAP native and bridge paths | Preview within public beta | Do not infer native debugger parity from presence of the adapter |
| Internal/non-allowlisted crates | Development surface | No external compatibility contract until published |

## Before reporting a channel problem

Record the channel, installed version, operating system and architecture, exact
asset or registry URL, and the output of `perllsp --version` and `perllsp --health`.
`--health` proves that the executable starts; it does not prove editor feature
or workspace-index correctness. Use the channel-specific receipt in
`docs/project/status/release.md` when filing the issue.

## Related sources

- [Release readiness](status/release.md)
- [API stability policy](../reference/STABILITY.md)
- [Installation guide](../how-to/INSTALLATION.md)
- [Upgrade guide](../how-to/UPGRADING.md)
- [Repository README](../../README.md)
