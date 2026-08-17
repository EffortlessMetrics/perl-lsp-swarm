# Zed Codex implementation train v2

> **Public state:** EffortlessMetrics `perllsp` support in Zed remains **planned / not proven** until the official-registry host receipt and exact support projection pass.
>
> **Programme:** [#7759](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7759)
>
> **Stable train:** `.ci/fixtures/zed-perl-upstream/train-v2/manifest.json`

## Ruling

The train has two inputs with different authority:

```text
stable topology
  stages, dependencies, evidence boundaries, actors, stop points

live observation
  current main, issue and PR state, checks, external publication state
```

The stable topology changes only when the architecture changes. Live GitHub state belongs to a separate typed observation and generates the current frontier. A merge, issue closure, green check, or elapsed time cannot rewrite product evidence.

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

Use **Advances** until the issue's actual acceptance is earned. A local branch, unpushed commit, checked template, or green static build is not a delivered stage.

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
C00 stable train/live observation
P00 static convergence
  ├─ P01 public-asset producer ──────────────┐
  ├─ P02 exact-source host driver ───────┐   │
  └─ P09 support projection substrate    │   │
                                         ├─ I00 canonical perllsp identity
                                         │
P02 ─ P03 deterministic fixture          │
      ├─ P04 settings authority           │
      └─ P05 defaults authority           │
P01 + I00 ─ P06 public-asset workflow    │
P01 + P02 + P03 + I00 ─ P07 managed authority
P02 + P03 + I00 ─ P08 public host driver
C00 + P01 + P02 + I00 ─ C01 currentness
C00 ─ C02 historical reconciliation

P01 + P06 + I00 + C01 ─ P10 public-asset evidence
P02 + P03 + I00 + C01 ─ P11 exact-source host evidence
P04 + P11 ─ P12 settings evidence
P05 + P11 ─ P13 defaults/order evidence
P07 + P10 + P11 + I00 + C01 ─ P14 managed-route evidence
P10..P14 + I00 + C01 ─ P15 exact-source submission authority
P15 + P13 + I00 ─ P16 current upstream candidate
P16 + P15 + P13 + P10 + I00 ─ P17 extension packet
P13 ─ P18 defaults packet
P17 + P18 ─ M01 ─ U01 ─ P19 registry packet ─ M02 ─ U02
P08 + P10 + P13 + P14 + I00 + C01 + U02 ─ P20 public host evidence
P09 + P20 + C01 ─ P21 exact support projection
P21 + C02 ─ P22 programme closeout
```

## Core authority stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| C00 | #10338 | codex | — | Separate the stable Zed implementation DAG from live GitHub observation and derive the current Codex frontier. |
| P00 | #7975 | codex | — | Converge the truthful static Zed integration contracts on one current mainline tree. |
| P01 | #7980 | codex | P00 | Provide the read-only public perllsp asset producer and receipt authority without treating templates as executed evidence. |
| P02 | #7984 | codex | P00 | Provide the operator-assisted exact-source Zed prepare, launch, finalize, and fail-closed receipt authority. |
| I00 | #10340 | codex | P01, P02 | Bind canonical perl_lsp.binary_identity.v1 packets and external file identity across every Zed perllsp route. |
| P03 | #8647 | codex | P02 | Add one deterministic fixture and machine-readable expectation contract shared by every Zed host lane. |
| P04 | #7990 | codex | P02, P03, I00 | Provide the canonical Zed settings behavior experiment and validator. |
| P05 | #7992 | codex | P02, P03, I00 | Provide the four-cell provider/default compatibility experiment and derived publication-order validator. |
| P06 | #8661 | codex | P01, I00 | Run the public perllsp asset producer through a read-only Windows, Linux, and macOS workflow matrix. |
| P07 | #8753 | codex | P01, P02, P03, I00 | Provide the exact-source managed route, cache reuse, and known-good recovery authority. |
| P08 | #9467 | codex | P02, P03, I00 | Provide the official-registry public-host prepare, launch, finalize, and public-stage validation authority. |
| P09 | #9468 | codex | P00 | Provide the fail-closed Zed support-registry importer and generated-documentation substrate. |
| C01 | #9483 | codex | C00, P01, P02, I00 | Add subject-aware Zed CI selection, exact invalidation, fail-closed fan-in, and stable final checks. |
| C02 | #10352 | codex | C00 | Reconcile superseded internal Zed PRs and branches through a unique-work and successor ledger. |

## Core evidence stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| P10 | #8678 | codex | P01, P06, I00, C01 | Execute, validate, and freeze current public perllsp asset receipts and their aggregate. |
| P11 | #8695 | codex | P02, P03, I00, C01 | Execute and freeze one exact-source development-extension Zed core receipt. |
| P12 | #8714 | codex | P04, P11 | Execute and freeze the settings precedence, typed-consumption, and live/restart receipts. |
| P13 | #8733 | codex | P05, P11 | Execute and freeze provider/default compatibility receipts and derive the safe publication order. |
| P14 | #8772 | codex | P07, P10, P11, I00, C01 | Execute and freeze the real-Zed managed first-mile, reuse, disable, and recovery receipts. |
| P15 | #10343 | codex | P10, P11, P12, P13, P14, I00, C01 | Aggregate all exact-source child receipts into one current submission authority and claim set. |

## Delivery and public-proof stages

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| P16 | #10345 | codex | P15, P13, I00 | Reconstruct the smallest accepted perllsp extension candidate on the exact current upstream source. |
| P17 | #10347 | codex | P16, P15, P13, P10, I00 | Freeze the evidence-bound, copy-ready upstream extension submission packet. |
| P18 | #7908 | codex | P13 | Freeze the evidence-derived, exact-current Zed-core dormant-provider defaults packet. |
| M01 | manual | maintainer | P17, P18 | Manually submit the extension and defaults packets in the evidence-derived order. |
| U01 | #10350 | read_only_acceptance | M01 | Bind and classify the actually merged upstream extension and defaults subjects. |
| P19 | #7910 | codex | U01 | Freeze the exact existing-perl official registry update packet from the accepted upstream subject. |
| M02 | manual | maintainer | P19 | Manually submit the official registry packet and record the external submission identity. |
| U02 | #10351 | read_only_acceptance | M02, U01 | Bind official registry publication and the released Zed defaults subject required for public execution. |
| P20 | #7912 | codex | P08, P10, P13, P14, I00, C01, U02 | Execute the clean ordinary official-registry installation and exact public Zed host journey. |
| P21 | #8000 | codex | P09, P20, C01 | Project only exact public Zed receipt cells into #7122 and deterministic generated documentation. |
| P22 | #7759 | codex | P21, C02 | Reconcile the programme ledger, preserve limitations, and close only the proven Zed LSP programme. |

## Non-blocking DAP sidecar

| Stage | Issue | Actor | Depends on | One-PR objective |
|---|---:|---|---|---|
| D01 | #9485 | codex | P00 | Add the static perl-dap adapter authority to the existing Perl extension candidate. |
| DA01 | #9516 | codex | D01, C01 | Execute and freeze public perl-dap asset receipts independently of Zed. |
| D02 | #9486 | codex | D01, P02, P03, C01 | Execute and freeze one exact-source real-Zed perl-dap debug session. |
| D03 | #9490 | codex | D01, D02 | Freeze the evidence-bound upstream Perl-extension packet for perl-dap. |
| DM01 | manual | maintainer | D03 | Manually submit the DAP extension packet upstream. |
| DU01 | #10353 | read_only_acceptance | DM01 | Bind the actually merged and released upstream perl-dap extension subject. |
| D04 | #9491 | codex | DU01 | Freeze the official existing-perl registry packet for the accepted DAP extension subject. |
| DM02 | manual | maintainer | D04 | Manually submit the DAP registry packet. |
| D05 | #9487 | codex | DA01, D02, DM02, C01 | Execute the clean official-registry managed perl-dap journey in real Zed. |
| D06 | #9489 | codex | D05, P09 | Project proven Zed debugger cells separately into #7122 and generated documentation. |
| D07 | #9484 | codex | D06 | Reconcile and close the non-blocking Zed DAP programme without changing the Zed LSP verdict. |

The DAP sidecar may run when its own prerequisites are met. No core LSP stage depends on it.

## Evidence boundaries

These substitutions are forbidden:

```text
manifest or WASM build          != actual Zed behavior
version output                  != canonical binary identity
canonical packet                != observed file SHA-256
release metadata                != downloaded executable bytes
public executable smoke         != real Zed host behavior
development extension           != official registry distribution
registry merge                  != released Zed defaults
merged source                   != public availability
PATH route                      != managed route
one platform                    != another platform
server capability               != observed Zed method
static defaults patch           != quiet provider behavior
receipt template                != executed receipt
submitted external PR           != accepted upstream subject
LSP evidence                    != DAP evidence
```

## Canonical binary identity

Every Zed `perllsp` process subject must consume the canonical `perl_lsp.binary_identity.v1` packet and independently bind the observed file SHA-256. Managed routes additionally consume the artifact-aware staged verifier from #6853 before selection. A same-version binary from another source revision, wrong target, partial identity, copied file, or wrong role cannot pass.

## External acceptance

The train contains two read-only gates after manual writes:

- **U01 / #10350** compares the actual merged extension and Zed defaults source against the prepared packets. It classifies exact acceptance, compatible delta requiring revalidation, semantic drift, partial acceptance, or not merged.
- **U02 / #10351** binds the actual official `perl` registry publication and one released Zed build containing the accepted provider order. Both must exist before public execution.

Neither gate writes externally.

## Current frontier

Do not hand-edit a current frontier into this document. Generate it from:

```text
stable train fragments
+
one typed live GitHub/external observation
```

Unknown, missing, ambiguous, stale, or instrument-failed observation stays fail closed. The generated frontier must distinguish `ready`, `in_progress`, `blocked_internal`, `blocked_external`, `manual_checkpoint_pending`, `external_acceptance_pending`, `evidence_pending`, `superseded`, and `landed_current_tree`.

## Codex goal prompt

Work only the named Zed train stage and its owning issue. Re-fetch current `main`, issue comments, canonical PRs and branches, and every named external read-only subject before planning; treat stale branches as evidence rather than authority. Write falsifiers first, reconstruct the smallest current-base increment, run focused and policy checks, commit and push intentionally, open one draft PR, and update the handoff. Preserve provider, binary, route, platform, evidence-stage, and LSP/DAP boundaries; stop before merge or any external submission, registry mutation, or release unless separately instructed.
