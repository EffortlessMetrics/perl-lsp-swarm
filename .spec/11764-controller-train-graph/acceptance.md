# Acceptance Criteria: #11764 — canonical stable issue_controller_train.v1 topology graph

This is a checked, declarative contract. It implements no validator command,
semantic revision evaluator, current-tree probe, readiness/frontier, source
context, live observer, packet rendering, GitHub metadata work, exact-head
checker, dogfood, scheduler, support claim, release or publication. Those
remain owned by the nodes this graph declares.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| The complete issue-controller train is needed as data | `train.manifest.json` covers all 26 nodes (programme controller, S00, functional rail C01–C06/R05B, T-series, I-series, P/D-series) with exact issue IDs from #11681 | Node set is checked against the exact expected node/issue pairs |
| A node's contract is queried | Every node carries ID, issue, title fingerprint, train role, lane, chain, one-PR outcome, authority before/after, dependency classes, claim ceiling, writer/conflict identity, spec disposition, first falsifier, controls, proof, review questions, obligations, exits, rollback, successors, identity fields, limitations | Required-field completeness is structurally checked |
| An edge's basis is questioned | Every dependency records the statement it traces to: leaf body references, S00 plan rows/ordering, or #11681's dependency graph | Provenance strings name the source |
| Graph-law ordering is questioned | The S00/#11681 ordering laws are frozen edge-by-edge with their declared classes; weakening or removing one fails | Checker law-edge table |
| A controller, fan-in or external gate is routed to a builder | Rejected: controllers are never buildable; R05B requires an explicit external authorization dependency | Controller/external-gate laws |
| Two nodes claim the same authority-after proposition or conflict key | Rejected: uniqueness is structural | Checker uniqueness laws |
| A semantic graph change is proposed | T02R #11767 owns classification and invalidation; the manifest records revision governance and never rewrites itself to pass | Revision-governance block |
| Optional or unavailable evidence rows | Remain explicit; missing/instrument-failed evidence is `not_proven`, never pass | Evidence-semantics block |
| The manifest is serialized twice | Canonical semantic digest is identical across input order; two checker runs print byte-identical output | Order-invariance control and two-run proof |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Controller authority | Controllers never enter ordinary builder frontiers | node law | falsifier 1 |
| Role separation | Train roles never collapse into GitHub issue roles | train_role vocabulary | falsifier 2 |
| Proposition uniqueness | One authority-after proposition and one conflict key per node | uniqueness laws | falsifiers 3, 4 |
| Dependency typing | hard/evidence/optional/external never collapse; graph-law edges frozen | law-edge table | falsifier 5 |
| Durable-byte hygiene | No live SHA/PR/check/model/writer state in stable bytes | value scan | falsifier 6 |
| Plane separation | Label/navigation application is not product readiness | plane law | falsifier 7 |
| Repair authority | Proof/fan-in nodes bound against product repair | claim-ceiling law | falsifier 8 |
| Cutover discipline | Generic entry keeps its old-heuristic retirement exit | I01 exit law | falsifier 9 |
| Contract completeness | Every node keeps falsifier, review question, rollback, stop | required-field laws | falsifier 10 |
| Supersession safety | Superseded work keeps unique identity and exact successor | supersession law | falsifier 11 |
| Evidence visibility | Optional/unavailable rows never disappear | evidence-semantics law | falsifier 12 |
| Path neutrality | No source paths; path order is never semantic order | schema strictness | falsifier 13 |
| Determinism | Canonical serialization invariant under input order | canonical digest | falsifier 14 |
| Revision ownership | Every future semantic change has an invalidation owner | T02R node law | falsifier 15 |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Durable architecture | #11763 / `.spec/11763-issue-controller-architecture/` | Consumed as semantic input; its proposition map, ordering, planes, open decisions and checker discipline are encoded, not re-derived |
| Programme controller child links | #11681 | Exact child issue IDs only; no placeholders |
| Leaf dependency statements | #11682–#11687, #11765–#11785 | Every edge traces to a leaf-body reference, an S00 plan row/ordering boundary, or #11681's dependency graph |
| Typed dependency/evidence semantics | #10858 | hard/evidence/optional/external classes consumed, not redefined |
| Shared packet contracts | #10872 / #10881 | Declared as T07's adapter targets; never cloned or merged |
| Train-mechanics extraction gate | #10554 | Respected: the manifest is programme-local; no shared mechanics extracted |
| Writer admission | #4177 / #3982 / #3957 | Conflict keys are identities, not reservations; no writer registry created |
| Spec method | #3983 and current `.spec` tooling | Bundle shape follows `SPEC_TEMPLATE.md` plus the S00 four-file precedent |
| Bundle precedents | `.spec/11763-issue-controller-architecture/` (PR #12006), `.spec/10894-editor-host-reliability/` (PR #11811) | Same discipline: embedded structural checker, fail-closed negative controls, two-run determinism, honest NOT_PROVEN |

## §API-Shape

No Rust or public API is introduced. The manifest is data; the names below
are the stable contract surfaces it declares for later nodes:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `issue_controller_train.v1` (`train.manifest.json`) | stable graph | 26 nodes, typed edges with provenance, conflict keys, dispositions, controls; deterministic canonical digest | T01 #11764 (this bundle) |
| Canonical semantic digest | deterministic function | SHA-256 over order-canonical content; invariant under input order | T01; consumed by T02 #11765 |
| Independent validator | executable | none here; T02 owns validation and the checked projection | T02 #11765 |
| Semantic revision governance | executable | none here; T02R owns classification/invalidation | T02R #11767 |

## §Test-Grid

All fifteen issue falsifiers, fixed order, as they bind this manifest. Every
mutation is executed as an in-memory negative control by the embedded checker
in `checklist.md`; a conformant checker must reject each one deterministically.
Falsifier 5 carries two controls (class reclassification and a duplicate edge
under a conflicting class); falsifier 14 is an order-invariance control whose
rejected subject is an order-sensitive canonicalization, not the shuffled
input itself.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---|---|---|---|---|
| 1 | #11681 or another controller is emitted as ordinary implementation | opposite | rejected | Set controller node buildable; checker must fail |
| 2 | Issue role and train node role collapse | wrong-subject | rejected | Inject an `issue_role` field; strict schema must fail |
| 3 | Two active nodes own the same authority-after proposition | partial | rejected | Duplicate authority_after; uniqueness must fail |
| 4 | One node has two incompatible writer/conflict identities | partial | rejected | Duplicate conflict key; uniqueness must fail |
| 5 | Hard/evidence/optional/external dependencies collapse | partial | rejected | Reclassify a graph-law edge; class freeze must fail |
| 6 | Current SHA/PR/check/model/writer state enters stable bytes | instrument | rejected | Inject a live-state token; value scan must fail |
| 7 | Label/navigation application is treated as product readiness | wrong-subject | rejected | Inject applied-readiness state; strict schema must fail |
| 8 | Proof/fan-in can repair missing product work | partial | rejected | Strip the repair bound; ceiling law must fail |
| 9 | Generic entry cutover has no old-heuristic exit | partial | rejected | Empty the exit; cutover law must fail |
| 10 | A node lacks first falsifier, review question, rollback or stop boundary | partial | rejected | Blank a required field; completeness must fail |
| 11 | A superseded/transferred node loses unique work or exact successor | partial | rejected | Add successorless supersession; registry law must fail |
| 12 | Optional/unavailable/instrument-failed rows disappear | instrument | rejected | Remove evidence semantics; schema must fail |
| 13 | Path order becomes semantic dependency order | wrong-subject | rejected | Inject source paths; strict schema must fail |
| 14 | Canonical serialization changes with map/input order | stale | rejected: an order-sensitive canonicalization is what the control rejects; the shuffle itself is a valid input | Shuffle nodes/edges; digest must stay identical |
| 15 | A future semantic graph change has no revision/invalidation owner | stale | rejected | Remove T02R; required node set must fail |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, PR, review or metadata mutation |
| Later train nodes | T02/T02R/T02S and later consume `train.manifest.json` as the stable topology input; their issue bodies remain the per-node authority |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes the train's stable topology durable: the complete 26-node
graph with typed, provenance-traced edges, writer/conflict identities,
dispositions, controls and revision ownership, plus a deterministic canonical
digest and fifteen fail-closed negative controls. It does not prove that the
topology is the semantically correct reading of every leaf body (that is
review's job here and T02R's later), that any later tooling works (unbuilt),
that the graph stays current as issues evolve (T02R owns invalidation), or
that an independent validator accepts it (T02 is unbuilt). Those remain
`not_proven` here.

## Non-goals

No static validator command, semantic revision evaluator, current-tree probe,
readiness/frontier, source context, live observer, packet rendering, GitHub
metadata work, exact-head checker, dogfood, scheduler, support claim, release
or publication.
