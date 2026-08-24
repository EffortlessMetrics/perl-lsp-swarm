# Plan: #11709 — durable Zed integration leaf contracts

This is the checked decision map that later nodes consume instead of
re-deriving the Zed programme from controller archaeology. For every durable
decision it records the stable decision ID, the one-proposition contract, the
canonical owning authority, the falsifiers that guard it, and its claim
ceiling. It changes no implementation, workflow, receipt, label, issue body,
GitHub metadata, or external state.

Current paths, SHAs, PRs, checks, candidates, models, assignments, release
subjects, and support verdicts are not durable semantic input; nothing in this
plan records them.

## Decision vocabulary

Decision IDs deliberately match the semantic conflict-key namespace that
#10338 defines (`zed.spec.architecture`, `zed.extension.*`, `zed.binary.*`,
`zed.process.*`, `zed.fixture.expectations`, `zed.assets.*`, `zed.cache.*`,
`zed.network.update`, `zed.managed_route.*`, `zed.dap.*`), so the stable graph
consumes these rows without translation. A row's owner may not be rebuilt or
silently widened by a consumer.

Row status semantics: `compiled` means the owning authority has landed the
decision this bundle records; `pending(<issue>)` means the decision's final
form is owned by the named issue and is recorded here as a boundary, with the
reason stated in the row. No row invents authority.

## Decision inventory

### Product authority

| Decision ID | Proposition | Owning authority | Falsifiers | Status |
| --- | --- | --- | --- | --- |
| `zed.server.identity` | The Zed server ID is `perl-lsp-rs` ("Perl LSP (EffortlessMetrics)"); legacy-ID occurrence classification migrates every launch path to this single identity; `perllsp` is never retained as an active server ID | #10842 | F1 | compiled |
| `zed.launch.argv` | The launched executable is exactly `perllsp --stdio`; no aliasing to, wrapping of, or fall-through to `perl-lsp` or any other executable | #11304 | F2 | compiled |
| `zed.product.package` | Product/package identity is `perl-lsp`; the existing upstream server `perl-lsp` and existing default `perlnavigator-server` remain distinct named identities, never launch targets for this server | #10842 + product-identity policy on tree | F1, F2 | compiled |
| `zed.binary.provenance` | Downloaded/bundled binaries carry exact provenance; the running process identity is attributable to a proven binary subject | #10340 / #10530 | F3 | compiled |
| `zed.extension.materialization` | Extension source, package, and WASM materialize reproducibly from pinned inputs | #10395 | F11 | pending(#10395 leaves) — implementation sequencing owned there |
| `zed.extension.execution_source` | Exactly one selected execution source and route serves development installs; distinct from public-asset and registry distribution | #11041 | F4 | pending(#11041) — selection outcome owned there |
| `zed.settings.defaults.status` | Settings keys, default-provider order, and status identities have exactly one authority each | #10392 / #10393 / #11043 | F12 | compiled |
| `zed.fixture.expectations` | Deterministic fixtures pin expected behavior for all host-independent proof | #8647 | F11 | compiled |
| `zed.activation.platform` | Activation and platform breadth is an optional-breadth matrix; non-blocking unless a selected bounded claim requires it | #11046 / #10991 | F12 | compiled |

### Managed artifact and mutation authority

| Decision ID | Proposition | Owning authority | Falsifiers | Status |
| --- | --- | --- | --- | --- |
| `zed.assets.public_contract` | Public asset bytes/archive/process follow one published contract with retained asset evidence | #8661 / #8678 | F3 | compiled |
| `zed.cache.integrity` | Managed cache integrity preserves known-good binaries through update/recovery | #10396 / #8753 / #8772 | F3 | compiled |
| `zed.mutation.safety` | Cache/asset mutation is safe-by-construction with explicit rollback | #11316 | F7 | compiled |
| `zed.network.update` | Offline behavior never mutates cached state; updates flow only through the network authority | #11308 | F3 | compiled |
| `zed.managed_route.authority` | The managed route is proven inside actual Zed, with recovery evidence distinct from asset receipts | #8753 / #8772 | F3 | compiled |
| `zed.registry.host_authority` | Official-registry distribution truth comes only from clean public-host proof | #9467 / #7912 | F4 | compiled |

### Evidence and publication stages

Each stage S01–S15 is one handoff boundary; promotion requires the owning
stage's evidence class. Roles: fan-ins, packet freezes, manual checkpoints,
external actions, and read-only acceptances are never ordinary builder work.

| Stage | Contract | Role | Owning authority | Falsifiers |
| --- | --- | --- | --- | --- |
| S01 | Static source/package authority | implementation | #10395 | F11 |
| S02 | Public asset bytes/archive/process | implementation + receipt evidence | #8661 / #8678 | F3 |
| S03 | Exact-source development-extension behavior | implementation + host evidence | #11041 + host-execution owners | F3, F4 |
| S04 | Settings behavior | implementation + behavior evidence | #10392 | F5 |
| S05 | Default/provider-order behavior | implementation + behavior evidence | #10393 / #11043 | F5 |
| S06 | Managed route/cache recovery | implementation + recovery evidence | #8753 / #8772 / #10396 | F3 |
| S07 | Exact-source fan-in | fan_in (not builder work) | #7759 train controller | F7 |
| S08 | Upstream packet freeze | packet_freeze (frozen corpus, then stop) | upstream submission owners under #7759 | F7, F9 |
| S09 | Manual external submission | external action (explicit stop; never inferred) | external maintainer gate | F7, F9 |
| S10 | Merged upstream acceptance | read_only_acceptance (merge fact observed, never assumed) | upstream acceptance owners | F9 |
| S11 | Official registry packet freeze | packet_freeze (frozen corpus, then stop) | registry submission owners | F7, F9 |
| S12 | Manual registry submission/released defaults | external_gate (explicit authorization) | registry/release owners | F7, F9 |
| S13 | Clean official-registry public host proof | evidence_execution | #9467 / #7912 | F4 |
| S14 | Support-registry/generated-doc projection | projection (requires #10168 owner) | #10168 close authority | F5 |
| S15 | Programme closeout | closeout (#7759 denominator complete) | #7759 | F8 |

### Rails and cross-cutting boundaries

| Decision ID | Proposition | Owning authority | Falsifiers | Status |
| --- | --- | --- | --- | --- |
| `zed.dap.sidecar` | DAP adapter/binary `perl-dap` is a separate rail; DAP evidence never substitutes LSP evidence; DAP never enters the LSP support row or blocks #7759 | #7759 rail structure + DAP owners | F6 | compiled |
| `zed.currentness.invalidation` | Truth planes 1–4 stay independent; material decision revision invalidates affected downstream artifacts through their owning nodes | this bundle + consumers' own laws | F9 | compiled |
| `zed.claim.ceiling` | Every leaf carries a claim ceiling and explicit non-claims; no bounded authority or evidence closes #7759 | agentic execution law above | F8 | compiled |

## Execution law binding

Every concrete leaf derived from this map must satisfy the agentic execution
law in `context.md`. #10338 encodes it as the stable node contract;
#11710/#11711 observe it on tree and frontier; packets derive from it without
copying controller prose.

## Ordering boundaries

- This bundle precedes everything: #10338 waits for these accepted decisions.
- Stages S01→S15 promote only through their declared evidence classes.
- Optional breadth (`zed.activation.platform`, remote/replay/DAP sidecars)
  stays non-blocking unless a selected bounded claim requires it.
- Manual checkpoints (S09/S12) and packet freezes (S08/S11) are stops with
  frozen outputs, never writer claims.

## Falsifier-first rule

The twelve programme falsifiers (F1–F12, fixed order, defined in
`acceptance.md` §Test-Grid) bind these decisions before any happy-path work.
A later concrete node is conformant only if each mutation fails
deterministically in that node's negative controls.

## Handoff

This plan plus `context.md` (identity, truth planes, stage ladder, execution
law) and `acceptance.md` (falsifiers and claim ceiling) is the complete
semantic input #10338 needs to encode the stable DAG. #10338 proceeds when
this bundle merges; nothing downstream proceeds ahead of its declared
dependencies.
