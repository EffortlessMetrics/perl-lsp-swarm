# Plan: #11763 — durable architecture handoff to the issue-controller train

This is the checked programme map that later nodes consume instead of re-deriving
from controller archaeology. It records, for every existing and planned node of
the issue-controller programme, the exact one-PR proposition, the authority it
owns, what it consumes, and what it must not do. It changes no implementation,
registry row, label, issue body, GitHub metadata, or external state.

Current paths, SHAs, PRs, checks, candidates, models, assignments and support
verdicts are not durable semantic input; nothing in this plan records them.

## Programme shape

```text
#11681 programme controller (assignable = false)
├── functional rail (initial train)
│   C01 #11682 → C02 #11683 → C03 #11684 → C04 #11685
│   C05/R05A #11686 (tooling) → C06 #11687 (read-only drift)
└── modern execution train
    S00 #11763 (this bundle)
    → T01 #11764 → T02 #11765 → T02R #11767 → T02S #11774
    → T03 #11769 → T04 #11771 → T05 #11772 → T06 #11773
    → T07 #11775 → T08 #11776 → T08C #11784
    → I01 #11777 / I02 #11778
    → P01 #11779 → D01 #11781 → D02 #11782 → P02 #11783
    R05B #11785 (privileged operation, explicit authorization only)
```

The two rails are complementary: the functional rail builds the role/registry/
projection/directory/mutation/drift substrate; the execution train builds the
stable graph, exact-tree/live interpretation, packets, closeout, integration,
proof and dogfood. Neither replaces the other; the directory (C04) routes
between them.

## Node propositions

Every row is one proposition / one reviewable PR result. "Consumes" names the
canonical authority; a consumer may not rebuild or silently widen it.

| Node | Issue | One-PR proposition | Consumes | Never |
| --- | --- | --- | --- | --- |
| C01 | #11682 | Role contract `issue_role_contract.v1` + non-authoritative candidate inventory `issue_controller_candidate_inventory.v1` with offline deterministic operations | #3983 conventions; programme manifests | Decide the registry, create/apply labels, rewrite bodies, compute readiness, inspect live candidates, schedule |
| C02 | #11683 | Reviewed active-controller registry population `issue_controller_registry.v1` covering the full denominator with false-positive dispositions | C01 inventory | Re-adjudicate during planning/applying; duplicate programme leaf DAGs |
| C03 | #11684 | `issue_role_labels.v1` + `issue_controller_metadata_projection.v1` with dry-run plan/render/check | C02 registry | Broad live mutation (owned by R05B) |
| C04 | #11685 | `issue_controller_directory.v1` deterministic offline directory/router + one derived document | C02 + C03 | Compute current-tree state, inspect GitHub, select candidates, schedule |
| C05 | #11686 | Metadata refresh/plan/apply/verify/rollback tooling proven on fixtures/fakes | C02 + C03 + C04 + T02R identity | Execute the reviewed live migration |
| C06 | #11687 | `issue_controller_live_snapshot.v1` + `issue_controller_drift.v1` read-only observer | C02/C03 expectations | Any GitHub mutation; registry rewrite from live state |
| S00 | #11763 | This bundle: durable architecture and evidence-boundary compilation | #11681–#11687 bodies; generic authorities | Implementation, registry population, metadata changes |
| T01 | #11764 | Stable `issue_controller_train.v1` topology and node contracts | S00 bundle | Readiness command, probe, observation, packet, mutation, scheduler |
| T02 | #11765 | Independent static validator + sole checked graph projection | T01 manifest | Rewriting the manifest to pass |
| T02R | #11767 | Semantic change classification + impact/invalidation projection | T02 | Manifest rewrite, issue edits, label mutation, candidate selection |
| T03 | #11769 | Exact immutable-tree implementation observation per node | T01/T02 | Readiness decision, GitHub inspection, proof execution |
| T04 | #11771 | Offline status/blocker/safe-frontier projection | T02 + T03 | Writer-freedom, vacancy, review-currentness or authorization claims |
| T05 | #11772 | Exact-tree bounded context per node | T01/T03 | Scope definition, readiness change, another source index |
| T06 | #11773 | Read-only live candidate/collaboration reconciliation | T04 + generic live authorities | Any mutation, assignment, scheduling, merge |
| T02S | #11774 | Checked per-node `.spec` compilation or explicit reviewed disposition | S00 + T01/T02 (changes governed by T02R) | Issue archaeology as builder input |
| T07 | #11775 | Derived builder/reviewer/reconciliation packets | #10872/#10881 + T02S/T03/T05/T06 | New packet schema, independent readiness/search/GitHub inspection, model invocation |
| T08 | #11776 | Exact-head structural closeout extension for declared nodes | Existing checker + train/spec/packet | Parallel checker, review approval, merge authorization |
| T08C | #11784 | Sufficient-proof routing into the existing CI router | Existing router + component semantics | Second router, second result/fan-in schema |
| I01 | #11777 | Generic existing-issue entry through the checked directory; heuristic retirement | C04 directory | Global DAG, readiness decisions, writer claims, GitHub mutation |
| I02 | #11778 | Shift-left reviewed role/route admission for new issues | Existing preparation method + C01/C02 | Auto-creating issues, unreviewed adjudication, self-updating registry |
| P01 | #11779 | Independent composed product/control proof | Implemented components | Repairing product behavior inside the proof PR |
| D01 | #11781 | Deterministic packet-sufficiency/routing scenario suite | #11114 vocabulary + #10872/#10881 | Real model cohort, local dogfood schema, scheduler |
| D02 | #11782 | Bounded fresh-agent/lower-cost/independent-review dogfood cohorts | Implemented interface after D01 | Packet/spec/product repair inside the evaluation PR; persistent agent state, transcript archives |
| P02 | #11783 | Final exact-current fan-in, maintenance projection, controller closeout | Full selected denominator current | Executing missing work; unauthorized metadata mutation |
| R05B | #11785 | Privileged live metadata operation + immutable migration receipt | Landed C05 tooling + C02/C03 projection | Authorization inferred from readiness/labels/green PR/tool availability |

## Execution contract per node

Every concrete control/product/integration/proof leaf must be representable as:

```text
one proposition / one reviewable PR result
stable node ID and role
hard/evidence/optional/external dependencies
writer slot and conflict key
claim/evidence ceiling
canonical authorities consumed and forbidden substitutes
first realistic false-green discriminator
positive/opposite/stale/wrong-subject/partial/instrument controls
spec/test/schema/generated/docs/receipt obligations
old-path or compatibility exit
rollback / transfer / return-to-issue / not-proven / stop
successors provisionally unblocked
```

T01 encodes this as the stable node contract; T02S compiles it per node; T07
derives bounded packets from it without rereading controller archaeology.

## Ordering boundaries

- C02 and C03 may proceed in parallel after C01.
- C05 (metadata tooling) is the only broad metadata writer; R05B executes only
  after C05 is landed on protected main and explicit authorization exists.
- C06 never mutates GitHub and observes only after C03/C04 expectations exist.
- T02S follows T01/T02; material changes flow through T02R invalidation.
- D01 precedes D02 (deterministic routing proof before real cohorts).
- P02 is the final fan-in; it closes #11681 only when the complete selected
  denominator passes.
- I01/I02 may proceed once C04 and C01/C02 respectively are current; they do
  not gate T-lane nodes.

## Falsifier-first rule

Each node's checked specification (T02S) records its slice of the sixteen
programme falsifiers (see `acceptance.md` §Test-Grid) as its first realistic
false-green discriminators before implementation. Proofs (P01/D01) exercise
them composed; a falsifier that fails deterministically in the negative-control
suite is the success condition.

## Handoff

This plan plus `context.md` (laws and boundaries) and `acceptance.md`
(falsifiers and claim ceiling) is the complete semantic input T01 (#11764)
needs to encode the stable graph. T01 proceeds when this bundle is merged;
nothing else in the train may proceed ahead of its declared dependencies.
