# Acceptance Criteria: #11763 — durable issue-controller architecture and evidence-boundary contract

This is a checked, declarative contract. It implements no role schema, registry,
label, navigation, directory, metadata writer, drift observer, packet, router,
proof or dogfood mechanism. Those remain owned by the nodes named in
`plan.md`.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| An issue's role is questioned | Role is established only by a reviewed registry row with exactly one primary role | Title, body or label signal is not a role |
| A controller issue is considered for implementation | `controller` is `assignable = false`; work routes to its child leaves | Metadata/navigation reconciliation may edit the issue without making it a product PR |
| A proof or fan-in leaf observes a product defect | The defect returns to its exact owning leaf; the proof PR does not repair it | Proof/fan-in validates, never repairs |
| Implementation state of a node is questioned | Derived only from proposition-specific probes on one exact committed tree | Issue closure, PR merge or green checks are not tree currency |
| A controller is needed by two programmes | Exactly one home programme; the other links via an explicit import relation | Import consumes the exact result; no rebuild or widen |
| Labels or navigation blocks are observed | They are deterministic projections of the reviewed registry | Applied projection is not authority and not leaf readiness |
| Live metadata mutation is proposed | Only reviewed expected-old-state tooling with plan/apply/verify/rollback receipt, bounded to labels and the generated navigation block | Semantic prose outside a generated block is never rewritten |
| Live metadata is observed | Drift report is read-only; the registry never rewrites to match GitHub | Drift clean is not product/support truth |
| A train changes semantically | T02R classifies the change and invalidates affected probes/specs/packets/candidates/reviews/metadata plans | Stale derived artifacts are re-derived, not patched valid |
| A fresh agent must execute a node | Bounded exact-tree context plus derived shared-contract packets, no controller archaeology | Packet generated is not work assigned or delivered |
| Evidence is optional or unavailable | Recorded as `not_proven`; never disappears, never becomes pass | Missing/instrument-failed evidence is non-success |
| Work entry prepares an existing issue | Routes through the checked directory to the home train, numbered leaf, proof/fan-in route or external boundary | Title/body/label heuristics are retired for routing; no global DAG or scheduler |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Role authority | Only reviewed adjudication creates roles | `context.md` role law | falsifiers 1, 2, 3 |
| Repair authority | Proof/fan-in never repair product behavior | role table; P01/P02 rows | falsifier 4 |
| Tree/live separation | Closure and green PR are not implementation currency | truth planes 2 vs 3 | falsifier 5 |
| Single home | One home programme per semantic controller; imports explicit | relationship vocabulary | falsifier 6 |
| Projection direction | Labels/navigation project from the registry; never the reverse | registry-vs-projection law | falsifiers 7, 11 |
| Durable-byte hygiene | Live/ephemeral state never enters durable bytes | AGENTS.md compatibility | falsifier 8 |
| No scheduler | Directory is navigation only | directory law; I01 row | falsifier 9 |
| Mutation bounds | Metadata writes touch labels and generated blocks only, expected-old-state guarded | mutation law | falsifier 10 |
| Readiness honesty | Structural readiness is not candidate vacancy or review currentness | T04/T06 boundaries | falsifier 12 |
| Packet separation | Builder and reviewer criterion sets stay distinct shared contracts | T07 adapter boundary | falsifier 13 |
| Entry discipline | Generic entry consumes the directory; heuristics retire | I01/I02 laws | falsifier 14 |
| Evidence semantics | Optional/unavailable evidence stays visible as `not_proven` | evidence law | falsifier 15 |
| Revision hygiene | Material revisions invalidate affected downstream artifacts | T02R law | falsifier 16 |
| Determinism | Same tree produces same ordered check output twice | `checklist.md` proof | second run is byte-clean |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) | Provides the canonical files and acceptance sections; adds `plan.md` as the explicit node-map file requested by #11763 |
| Programme controller architecture | #11681 | Compiles its corrected four-plane architecture without implementing it |
| Functional rail decisions | #11682 / #11683 / #11684 / #11685 / #11686 / #11687 | Preserves each as a distinct one-PR proposition with exact boundaries |
| Execution train decisions | #11764–#11785 (see `plan.md`) | Records propositions, consumes/never columns and ordering |
| Typed dependency/evidence semantics | #10858 | Consumed; dependency classes referenced per node, not redefined |
| Shared builder/reviewer packets | #10872 / #10881 | Consumed by T07 as adapter targets; never cloned or merged |
| Existing-work and writer admission | #4177 / #3982 / #3957 | Preserved as generic authority; no lease/writer registry created |
| Review/currentness/closeout | #3693 / #10168 | Consumed; exact-head closeout extends the existing checker |
| CI route/result/fan-in | #3390 / #1848 / #4787 / #4789 and successors | Extended by T08C; no second router |
| Train-mechanics extraction gate | #10554 | Respected: shared mechanics extracted only after its concrete-duplication gate |
| Fresh-agent evaluation | #11114 | D01/D02 consume its result vocabulary; no local dogfood schema |
| Spec method | #3983 and current `.spec` tooling | Method authority preserved; no new repository-wide spec schema; #3586 historical only |
| Bundle precedent | `.spec/10894-editor-host-reliability/` (PR #11811) | Same checked three-plus discipline: structural proof, second-run determinism, honest NOT_PROVEN boundary |

## §API-Shape

No Rust or public API is introduced. The names below are semantic contract
terms owned by later nodes; they bind future implementation, they do not exist
yet:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `issue_role_contract.v1` | semantic schema | five primary roles, assignability, relationship vocabulary | C01 #11682 |
| `issue_controller_candidate_inventory.v1` | evidence model | non-authoritative offline candidate discovery with sources | C01 #11682 |
| `issue_controller_registry.v1` | stable authority | reviewed roles, homes, routes, dispositions; no live state | C02 #11683 |
| `issue_role_labels.v1` | generated projection | label vocabulary derived from the registry | C03 #11684 |
| `issue_controller_metadata_projection.v1` | generated projection | per-issue navigation block, regenerate-cleanly | C03 #11684 |
| `issue_controller_directory.v1` | derived surface | navigation from any issue to role/chain/entry/closeout | C04 #11685 |
| `issue_controller_live_snapshot.v1` / `issue_controller_drift.v1` | observation model | immutable snapshot; deterministic read-only drift | C06 #11687 |
| `issue_controller_train.v1` | stable graph | node/dependency/writer topology and contracts | T01 #11764 |
| Bounded packets | ephemeral outputs | #10872/#10881 instances; content-addressed, never tracked | T07 #11775 |

## §Test-Grid

All sixteen programme falsifiers, fixed order, as they bind this bundle's
compiled decisions. Verdict semantics: every mutation must be rejected by the
compiled architecture — a later concrete implementation is conformant only if
each mutation fails deterministically in that node's negative controls.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---|---|---|---|---|
| 1 | `controller(...)` title prefix is accepted as establishing controller role | wrong-subject | rejected: title is signal, not adjudication | Candidate carries source `title-prefix`; only a reviewed registry row yields a role |
| 2 | Domain noun `controller` (MVC/host/process) makes an issue non-assignable metadata | wrong-subject | rejected: domain subject stays `implementation` | Denominator excludes body-mention-only issues; false-positive disposition recorded |
| 3 | A semantic controller is delivered as a normal builder leaf | opposite | rejected: `controller` is `assignable = false` | Route from directory returns child leaves, not a one-PR claim on the controller |
| 4 | A proof or fan-in PR repairs product behavior | partial | rejected: defect returns to exact owner | Proof PR diff touches product path outside proof scope → not-proven/failed, not pass |
| 5 | Issue closure or a green PR is read as current-tree implementation | stale | rejected: only exact-tree probes establish tree state | Closed issue with no probe result remains `unknown`, not `implemented` |
| 6 | One semantic controller gets two home programmes with no import relation | opposite | rejected: exactly one home; second is an import | Registry check fails duplicate-home without an import edge |
| 7 | Role labels are treated as registry authority | wrong-subject | rejected: labels are generated projection | Adjudication reads registry only; label-only mutation cannot change a role |
| 8 | Current SHA/PR/check/model/writer state is written into durable bytes | instrument | rejected: durable bytes carry stable content only | Registry/projection byte diff after live-state change is empty |
| 9 | The repository directory grows scheduling/DAG powers | opposite | rejected: directory is navigation only | Directory output contains no ordering, assignment, liveness or writer claims |
| 10 | Metadata migration rewrites semantic issue prose outside a generated block | partial | rejected: mutation bounded to labels + generated block | Apply plan diff touching prose outside the generated block fails validation |
| 11 | Read-only drift rewrites the registry to match GitHub | opposite | rejected: drift observes; registry changes via review only | Drift output is a report + correction route; no write path exists |
| 12 | Local/current-tree proof is read as live candidate vacancy | partial | rejected: readiness is not vacancy or review currentness | Frontier marks node ready while T06 still reports an active candidate/writer |
| 13 | Builder and reviewer packets collapse into one mirrored criterion set | opposite | rejected: #10872 and #10881 stay distinct contracts | Packet adapter emits both schemas unchanged; shared criteria are references, not copies |
| 14 | Generic work preparation bypasses the directory via title/body/label heuristics | stale | rejected: entry consumes the checked directory | Old-heuristic route produces a different route than the directory → failure |
| 15 | Optional or unavailable evidence disappears or is recorded as pass | instrument | rejected: `not_proven` semantics | Missing/instrument-failed evidence renders as `not_proven`, never pass |
| 16 | A material train revision leaves stale probes/specs/packets/candidates/reviews/metadata plans valid | stale | rejected: revision invalidates affected downstream artifacts | T02R impact projection marks every affected artifact stale; consumers must re-derive |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, PR, review or metadata mutation |
| Later train nodes | T01–T08C, I01–I02, P01–P02, D01–D02, R05B consume this bundle as semantic input; their issue bodies remain the per-node authority |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes the programme's stable architecture durable: role schema,
assignability, relationships, discovery-vs-adjudication, registry-vs-projection,
mutation and drift bounds, truth planes, revision/invalidation, exact-tree and
packet boundaries, generic-entry adoption, closeout/proof/dogfood contracts,
node propositions and open decisions. It does not prove that any tooling works,
that roles are correctly adjudicated, that a migration is safe live, or that
fresh agents succeed — those are owned by the consuming nodes and remain
`not_proven` here.

## Non-goals

No issue-role implementation, stable DAG command, current-tree probe, live
candidate observer, source-context resolver, packet schema or instance, GitHub
issue/label write, global scheduler, product proof, support registry, issue
closure, release, publication, or external submission.
