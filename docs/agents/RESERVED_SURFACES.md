# Reserved-surface register

Status: current ownership-boundary index
Owner: perl-lsp maintainers
Machine registry: [`reserved_surfaces.toml`](reserved_surfaces.toml)
Tracking issue: [#12562](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12562)

This page answers one bounded question:

> When I am about to write to a surface, which durable claim already owns that
> surface's next change?

Each row records an **ownership boundary** — surface, owning issue(s), status —
for seams where concurrent lanes have actually risked (or produced) same-seam
writes. Rows are seeded from observed collisions and near-misses, not
hypotheticals.

## What this register is not

- It is not a scheduler, lane registry, or mandatory reservation gate. The
  current method explicitly does not require file reservations or a central
  scheduler (see [AUTHORITY_STATUS.md](AUTHORITY_STATUS.md)). Writer exclusivity
  still comes from the one-writer/one-candidate rule and live GitHub state.
- It never records runtime state. Lane liveness, wake events, queue order,
  retries, reservations lifted, and topology stay in campaign runtime memory;
  root [AGENTS.md](../../AGENTS.md) forbids writing them to tracked files.
  A lane announces or yields a claim in issue/PR comments at brief time; this
  register carries only the durable boundary behind such announcements.
- Absence of a row is not permission to duplicate a claim.

## Reading rule

Before the first write to a surface listed here:

1. Read the surface's row in [`reserved_surfaces.toml`](reserved_surfaces.toml).
2. Verify the owning issues' live state on GitHub — the register records
   boundaries as of its `last_verified` date, and live issue/PR state remains
   authoritative when they diverge.
3. If your claim falls inside a row's boundary, coordinate with the owning
   claim at brief time instead of starting a rival candidate.
4. If your surface has no row, proceed normally — but absence of a row never
   overrides the reconcile-before-write rule. Check live GitHub state first.

## Current rows

Verified against main `296ae9bb4141` on 2026-08-26. Path-level detail lives in
the machine registry; this table is the fast index.

| Surface | Owning claims | Status | Boundary |
| --- | --- | --- | --- |
| `references-navigation-pir-shadow` | #12155, #12329 | open | exact request subject / PIR-quarantine residuals own references navigation next change |
| `dap-module-reload-family` | #10095, #10138 | open | reload directory work coordinates through the programme family |
| `dap-perl-identity-prerequisites` | #12594, #12595, #12748 | open | DAP interpreter identity / pipe-capability semantics are one decision |
| `session-warning-dedup-runtime` | #9769 | landed | extends merged cfg-gate strategy (#12367); removal fork (#12374) was superseded |
| `xtask-gate-disposition-route-profile` | #10176, #10178 | landed | gate additions ride typed disposition and route-profile contracts (#12415, #12541) |
| `vim-lsp-cell-catalog` | #11374 | landed | new cells extend the checked catalog API; no per-cell forks |
| `vim-host-leaves` | #11396, #11372 | open | leaves share xtask trunk wiring; second leaf rebases onto first |
| `framework-adapters-canonical-facts` | #8928 | open | adapter additions cut to canonical facts under the closed parent #8910 |
| `collapsible-if-suppression-sweep` | #12734, #12731, #12732 | open | deny policy (closed authority #6305) and clippy-debt ratchet are shared across per-crate sweep lanes |
| `parser-consumers-facade-imports` | #11377, #11382, #11389 | open | three sweeps overlap on coverage packs, ci.yml, and perl-lsp-rs import sites |
| `devex-shared-build-cache` | #12596 | open | one claim, candidate strategies reconcile on justfile/docs before landing |
| `non-rust-inventory-manifest` | #12775 | open | regenerate from tooling; hand edits only outside contested sections |
| `emacs-journey-host-runner` | #8824, #8825, #8830, #11719 | open | journey proofs vs packet tooling vs landed runner substrate (#7778) name their layer |

## Maintenance rule

- **Adding or updating a row:** the lane that establishes a surface family adds
  the row in the same PR that lands the family's first slice — reservation-only
  PRs do not exist here. The registration PR for already-landed families seeds
  rows from verified evidence only.
- **Status transitions:** at closeout the owning lane flips the row's status
  (`open` → `landed`) in its closeout PR or the next reconciliation pass.
- **Rows retire** when the boundary no longer helps anyone: a fully absorbed
  family whose contract is enforced by tooling can be dropped like any other
  stale documentation.
- Machine and human halves move together: a path-row edit touches both
  [`reserved_surfaces.toml`](reserved_surfaces.toml) and this index in one PR.

## Claim boundary

This register proves only which durable claims owned each surface's next
change as of its `last_verified` date. It does not prove that any candidate is
current, green, mergeable, or safe to write — live GitHub evidence owns those
decisions, and the closest document-status question remains governed by
[AUTHORITY_STATUS.md](AUTHORITY_STATUS.md), which classifies documents rather
than code surfaces.
