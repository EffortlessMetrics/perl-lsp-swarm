# NOW / NEXT / LATER

This file is the short planning snapshot for sequencing work. Use
[docs/project/ROADMAP.md](docs/project/ROADMAP.md) for the canonical milestone
plan and [docs/project/CURRENT_STATUS.md](docs/project/CURRENT_STATUS.md) for
evidence-backed status and release facts.

## DONE — public-alpha foundation through the v0.14.x line

- The 0.12.x and 0.13.x lines built confidence across parser corpus, diagnostics, refactoring, distribution, packaging, and announcement polish
- The 0.14.x line closed the public-alpha channel work and preserved release-channel discipline and evidence-backed status docs
- Earlier release facts are historical; verify current workspace version, crate surface, and channel state against the truth sources before quoting them

## NOW — v0.17.0 public beta shipped; v0.18.0 public-beta train

- `v0.17.0` shipped on 2026-06-28 and is the current **public beta** release line; `v0.18.0` is the next public beta, and public beta does not mean GA or stable
- The live sequencing is the 0.18.0 release chain under [#4343](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4343):
  - **Readiness** — [#4346](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4346) proves installed daily-driver behavior from packaged artifacts rather than workspace binaries, with zero false-exact, stale-exact, unsafe-edit, and unexplained-empty outcomes
  - **Preparation** — [#4347](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4347) moves version surfaces, changelog, curated notes, release-history row, and release status together in one generated PR
  - **Sync** — [#4348](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4348) promotes the frozen swarm tree into `perl-lsp/master` release lineage as a history-preserving complete-tree merge
  - **Rehearsal** — [#4350](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4350) builds the exact candidate artifacts, installs them in clean environments, and emits a fail-closed receipt without publishing anything
  - **Publish and closeout** — [#4351](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4351) tags once, publishes the primary channels, verifies them as an ordinary user, and reconciles notes, status, history, and repository lineage
- Scope classification and the freeze decision ([#4345](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4345)) are closed
- Primary channels are GitHub Releases, crates.io, VS Code Marketplace, and Open VSX; Docker and the owned Homebrew tap are secondary and must be verified or explicitly deferred rather than assumed
- Keep package-version language separate from product-posture language: SemVer package version, public-beta product promise
- Keep DAP claims at preview unless packaged-session receipts support more, and treat missing or stale evidence as `NOT_PROVEN` rather than green by omission
- Keep parser corpus lanes, compiler-backed provider dashboards, and install-surface receipts linked rather than duplicated in this short snapshot

## NEXT — post-v0.18.0

- Close channel receipts before broad cleanup
- Resume parser, corpus, semantic, DAP, and editor-trust hardening after release proof is complete
- Continue compiler-backed provider cutovers through provenance-backed, live-with-fallback slices
- Burn down deferred `v0.18.0` successor issues by ledger rather than by undocumented cleanup

## LATER — beyond v0.18.0

- Stability contract for APIs and advertised wire behavior
- Performance hardening for larger workspaces
- Security and supply-chain posture hardening
- Path to `v1.0.0`

## Working Rules

- Last updated: `2026-08-01`
- Keep “current release line” separate from “next milestone”.
- Put receipts and computed metrics in [docs/project/CURRENT_STATUS.md](docs/project/CURRENT_STATUS.md), not here.
- Put detailed milestone criteria in [docs/project/ROADMAP.md](docs/project/ROADMAP.md), not here.
