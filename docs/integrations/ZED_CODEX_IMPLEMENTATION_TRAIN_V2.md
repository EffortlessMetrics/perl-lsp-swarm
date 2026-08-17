# Zed Codex implementation train v2

> **Public state:** EffortlessMetrics `perllsp` support in Zed remains **planned / not proven** until the official-registry host receipt, public compatibility row, and exact support projection pass.
>
> **Programme:** [#7759](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7759)
>
> **Stable train:** `.ci/fixtures/zed-perl-upstream/train-v2/manifest.json`

## Ruling

The programme has two inputs with different authority:

```text
stable topology
  stages, dependencies, evidence boundaries, actors, stop points

live observation
  current main, issue and PR state, checks, external publication state
```

The stable topology changes only when the architecture changes. Live GitHub state belongs to a separate typed observation and generates the current frontier. A merge, issue closure, green check, or elapsed time cannot rewrite product evidence.

The first implementation attempt exposed five load-bearing joints that are now explicit stages rather than assumptions:

```text
reviewed extension source -> reproducible attested package
host logs and process state -> derived receipt cells
release topology + reviewed release -> generated managed projection
candidate/current/prior cache -> evidence-backed lifecycle
Zed + extension + perllsp -> exact compatibility rows
```

## Product identities

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp
perl-dap             -> separate debugger executable and sidecar programme
```

The first three language-server identities remain independent. Selecting the EffortlessMetrics provider must launch exact `perllsp --stdio`. `.pod` remains the separate POD language. Zed DAP never blocks the core LSP train and never enters the LSP support row.

## One stage, one PR

Every `codex` or `read_only_acceptance` stage completes through the checked **deliver-pr** contract:

1. Inspect current `main`, the owning issue and comments, canonical PRs and branches, and every named external read-only subject.
2. Claim only the named stage.
3. Write falsifiers first.
4. Reconstruct the narrowest current-base increment. Historical branches are mining subjects, not authority.
5. Run focused and repository-policy checks.
6. Commit and push intentionally.
7. Open one draft PR and update the issue or PR handoff.
8. Stop before merge or any external write unless separately instructed.

Use **Advances** until the issue's actual acceptance is earned. A local branch, unpushed commit, checked template, package build, or green static test is not a delivered behavioral stage.

## Manual and read-only checkpoints

```text
M01  maintainer submits the extension and defaults packets
U01  Codex/read-only acceptance binds the actual merged upstream subjects

M02  maintainer submits the existing-perl registry packet
U02  Codex/read-only acceptance binds registry publication and released defaults
```

The maintainer owns external writes. Codex owns only internal PRs and read-only acceptance records. Submitted is not merged; merged is not released; released metadata is not a successful public host journey.

## Core graph

```text
C00 stable train and live observation
P00 static convergence
  ├─ P01 public-asset producer ─ R00 generated release projection ──────┐
  ├─ P02 exact-source host driver ─ I00 canonical perllsp identity ─┐   │
  └─ P09 support projection substrate                              │   │
                                                                    │   │
I00 + P02 ─ S00 reproducible extension subject                     │   │
P02 ─ P03 deterministic fixture                                    │   │
P02 + P03 + S00 + I00 ─ E00 immutable evidence derivation          │   │
                                                                    │   │
P02 + P03 + I00 ─ P04 settings authority                           │   │
P02 + P03 + I00 ─ P05 defaults authority                           │   │
P01 + R00 + I00 ─ P06 public-asset workflow                        │   │
P01 + R00 + P02 + P03 + I00 ─ P07 managed authority               │   │
P07 + R00 + I00 ─ L00 safe cache lifecycle                         │   │
P02 + P03 + S00 + E00 + I00 ─ P08 public host driver              │   │
R00 + S00 + E00 + P04 + P05 + P07 + L00 + I00 ─ K00 compatibility│   │
C00 + authority subjects ─ C01 currentness                         │   │
C00 ─ C02 historical reconciliation                               │   │
                                                                    │   │
P01 + R00 + P06 + I00 + C01 ─ P10 public-asset evidence           │   │
P02 + P03 + S00 + E00 + I00 + C01 ─ P11 exact-source host evidence│   │
P04 + P11 + E00 ─ P12 settings evidence                            │   │
P05 + P11 + E00 ─ P13 defaults and order evidence                  │   │
P07 + L00 + P10 + P11 + subjects ─ P14 managed-route evidence     │   │
K00 + P10..P14 ─ K01 exact-source compatibility rows               │   │
P10..P14 + K01 + subjects ─ P15 exact-source submission authority  │   │
                                                                    │   │
P15 + P13 + K01 ─ P16 current upstream candidate                   │   │
P16 + P15 + evidence ─ P17 extension packet                        │   │
P13 ─ P18 defaults packet                                          │   │
P17 + P18 ─ M01 ─ U01 ─ P19 registry packet ─ M02 ─ U02           │   │
P08 + evidence + K01 + U02 ─ P20 public host evidence              │   │
K00 + P20 + U02 ─ K02 official-registry compatibility row          │   │
P09 + P20 + K02 + C01 ─ P21 exact support projection               │   │
P21 + C02 ─ P22 programme closeout                                 │   │
```

## Core authority stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| C00 | #10338 | codex | — | Separate stable topology from live observation and derive the current Codex frontier. |
| P00 | #7975 | codex | — | Converge truthful static Zed integration contracts on current `main`. |
| P01 | #7980 | codex | P00 | Provide the read-only public `perllsp` asset producer and receipt authority. |
| R00 | #10395 | codex | P00, P01 | Generate the managed-download projection from canonical topology plus one reviewed release input. |
| P02 | #7984 | codex | P00 | Provide operator-assisted exact-source prepare, launch, finalize, and receipt authority. |
| I00 | #10340 | codex | P01, P02 | Bind canonical `perl_lsp.binary_identity.v1` across every `perllsp` route. |
| S00 | #10392 | codex | P00, P02, I00 | Materialize and attest the exact development-extension source and WASM subject. |
| P03 | #8647 | codex | P02 | Add one deterministic fixture and machine-readable expectation contract. |
| E00 | #10393 | codex | P02, P03, S00, I00 | Derive required host cells from immutable evidence; allow operator evidence only where reviewed. |
| P04 | #7990 | codex | P02, P03, I00 | Provide the canonical settings behavior experiment and validator. |
| P05 | #7992 | codex | P02, P03, I00 | Provide the four-cell defaults/extension matrix and publication-order validator. |
| P06 | #8661 | codex | P01, R00, I00 | Run the asset producer through a read-only Windows, Linux, and macOS workflow matrix. |
| P07 | #8753 | codex | P01, R00, P02, P03, I00 | Provide the exact-source managed route and recovery authority. |
| L00 | #10396 | codex | P07, R00, I00 | Define safe candidate/current/prior-known-good cache promotion, retention, retry, and cleanup. |
| P08 | #9467 | codex | P02, P03, S00, E00, I00 | Provide the official-registry public-host driver and public-stage validation authority. |
| P09 | #9468 | codex | P00 | Provide the fail-closed support-registry importer and generated-doc substrate. |
| K00 | #10394 | codex | R00, S00, E00, P04, P05, P07, L00, I00 | Define exact Zed + extension + `perllsp` compatibility authority. |
| C01 | #9483 | codex | C00 plus subject authorities | Add subject-aware CI selection, invalidation, fan-in, and stable final checks. |
| C02 | #10352 | codex | C00 | Reconcile superseded PRs and branches through a unique-work and successor ledger. |

## Core evidence stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| P10 | #8678 | codex | P01, R00, P06, I00, C01 | Execute, validate, and freeze current public `perllsp` asset receipts. |
| P11 | #8695 | codex | P02, P03, S00, E00, I00, C01 | Execute and freeze one exact-source development-extension Zed core receipt. |
| P12 | #8714 | codex | P04, P11, E00 | Execute settings precedence, typed-consumption, and live/restart receipts. |
| P13 | #8733 | codex | P05, P11, E00 | Execute provider/default compatibility receipts and derive publication order. |
| P14 | #8772 | codex | P07, L00, P10, P11, subject authorities | Execute managed first-mile, reuse, disable, and recovery receipts. |
| K01 | #10401 | codex | K00, P10–P14 | Project exact-source and managed-public receipts into separate compatibility rows. |
| P15 | #10343 | codex | P10–P14, K01, subject authorities | Aggregate exact-source child evidence into one submission authority and claim set. |

## Delivery and public-proof stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| P16 | #10345 | codex | P15, P13, K01, subject authorities | Reconstruct the smallest accepted `perllsp` extension candidate on exact current upstream source. |
| P17 | #10347 | codex | P16, P15, P13, P10, K01, subject authorities | Freeze the evidence-bound copy-ready extension submission packet. |
| P18 | #7908 | codex | P13 | Freeze the evidence-derived exact-current Zed dormant-provider defaults packet. |
| M01 | manual | maintainer | P17, P18 | Manually submit extension and defaults packets in the evidence-derived order. |
| U01 | #10350 | read-only | M01 | Bind and classify the actually merged extension and defaults subjects. |
| P19 | #7910 | codex | U01 | Freeze the exact existing-`perl` official registry update packet. |
| M02 | manual | maintainer | P19 | Manually submit the official registry packet. |
| U02 | #10351 | read-only | M02, U01 | Bind registry publication and a released Zed build containing the accepted defaults. |
| P20 | #7912 | codex | P08, public/exact evidence, K01, U02 | Execute the clean ordinary official-registry installation and public host journey. |
| K02 | public compatibility issue | codex | K00, P20, U02 | Project the official-registry receipt into exact public compatibility rows. |
| P21 | #8000 | codex | P09, P20, K02, C01 | Project only exact public receipt and compatibility cells into #7122 and generated docs. |
| P22 | #10400 | codex | P21, C02 | Reconcile the ledger, retire superseded operational state, and close #7759. |

## Non-blocking DAP sidecar

The DAP sidecar remains in `dap-sidecar.json`. It may run when its own prerequisites are met. No core LSP stage depends on it, and no DAP result can promote or block the Zed LSP row.

## Evidence boundaries

These substitutions are forbidden:

```text
arbitrary extension directory       != attested extension subject
manifest or WASM build              != actual Zed behavior
free-form observation pass          != immutable evidence derivation
version output                      != canonical binary identity
canonical packet                    != observed file SHA-256
release metadata                    != downloaded executable bytes
hand-maintained release snapshot    != generated canonical projection
public executable smoke             != real Zed host behavior
extraction                          != candidate launch success
candidate extraction                != known-good cache promotion
development extension               != official registry distribution
registry submission                 != registry publication
registry merge                      != released Zed defaults
merged source                       != public availability
PATH route                          != managed route
one platform                        != another platform
server capability                   != observed Zed method
static defaults patch               != quiet provider behavior
receipt template                    != executed receipt
version equality                    != compatibility
exact-source compatibility          != public-registry compatibility
submitted external PR               != accepted upstream subject
LSP evidence                        != DAP evidence
```

## Canonical subject and compatibility model

Every Zed `perllsp` process subject consumes the canonical `perl_lsp.binary_identity.v1` packet and independently binds the observed file SHA-256. The development extension additionally consumes the S00 subject manifest; managed routes consume the R00 generated release projection and L00 lifecycle policy; host receipts consume the E00 immutable evidence index.

Compatibility is an exact tuple, not a matching version number:

```text
Zed build
+ extension package/tree/WASM
+ perllsp binary/release/route
+ platform
+ capability/settings/default contract
+ validated host receipt
```

Exact-source compatibility K01 gates submission. Official-registry compatibility K02 gates public support projection.

## External acceptance

The core train contains two read-only gates after manual writes:

- **U01 / #10350** compares the actual merged extension and defaults source against the prepared packets.
- **U02 / #10351** binds the official `perl` registry publication and one released Zed build containing the accepted provider order.

Neither gate writes externally. The maintainer handles every upstream and registry submission directly.

## Current frontier

Do not hand-edit a current frontier into this document. Generate it from:

```text
stable train fragments
+
one typed live GitHub/external observation
```

Unknown, missing, ambiguous, stale, or instrument-failed observation stays fail closed. The generated frontier distinguishes `ready`, `in_progress`, `blocked_internal`, `blocked_external`, `manual_checkpoint_pending`, `external_acceptance_pending`, `evidence_pending`, `superseded`, and `landed_current_tree`.

## Codex goal prompt

Work only the named Zed train stage and its owning issue. Re-fetch current `main`, issue comments, canonical PRs and branches, and every named external read-only subject before planning; treat stale branches as evidence rather than authority. Write falsifiers first, reconstruct the smallest current-base increment, run focused and policy checks, commit and push intentionally, open one draft PR, and update the handoff. Preserve provider, extension-subject, binary, release-projection, cache, compatibility, route, platform, evidence-stage, and LSP/DAP boundaries; stop before merge or any external submission, registry mutation, or release unless separately instructed.
