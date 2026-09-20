# Acceptance Criteria: #11709 — durable Zed integration architecture contract

This is a checked, declarative contract. It implements no extension source,
server launch, settings surface, asset workflow, cache mutation, host driver,
packet schema, DAG, or support projection. Those remain owned by the issues
named in `plan.md`.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| The Zed server identity is questioned | Exactly one active server ID: `perl-lsp-rs`, display "Perl LSP (EffortlessMetrics)", launching exact `perllsp --stdio` | Title prose or upstream fixture state is not the durable identity; #10842 owns migration/classification |
| A launch route is proposed that reaches `perl-lsp` or another executable | Rejected: no alias, wrap, or fall-through exists | Exact argv authority is #11304's; behavior claims need plane-4 evidence |
| An extension install path is selected | Materialization (#10395), execution source (#11041), and registry distribution (#9467/#7912) stay distinct authorities | One stage's receipt never proves another stage |
| A public asset is published | Asset bytes/archive/process follow the public contract with retained receipts | Public asset evidence != Zed host behavior (plane 4 vs 4, distinct classes) |
| Host behavior is claimed | Claimed only from exact-source or managed-route host evidence on a real Zed host | Issue closure, green PR, or asset download is not host truth |
| Settings/defaults are questioned | Keys, default order, and status identities each have one owner (#10392/#10393/#11043) | Prose agreement is not behavior evidence |
| Optional breadth (platform/activation/remote/DAP) is proposed | Non-blocking sidecar unless a selected bounded claim requires it | Breadth never hardens by issue order or enthusiasm |
| Support docs are produced | Only through the #10168 projection owner | Packet, merge, or host receipts never promote support text |
| DAP work is planned | Separate rail, separate evidence; never inside the LSP support row or blocking #7759 | LSP evidence != DAP evidence |
| A leaf finishes its bounded work | It advances its declared successor; only S15 completes #7759 | Bounded authority/evidence never closes the programme |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Identity authority | One server ID; legacy ID cannot survive as active | identity law (`context.md`), `zed.server.identity` | F1, F2 |
| Executable confusion | Product/package/upstream names are never launch executables | identity law, `zed.launch.argv` | F2 |
| Stage promotion | Each stage's evidence class promotes only itself | stage ladder S01–S15 | F3, F4 |
| Support promotion | Support docs require the #10168 projection owner | stage ladder S14 | F5 |
| Rail separation | DAP stays out of LSP rows and out of the critical path | `zed.dap.sidecar` | F6 |
| Role discipline | Controllers/fan-ins/manual stops/external gates are not builder leaves | execution law; S07–S12 roles | F7 |
| Closeout honesty | No bounded leaf closes #7759 | `zed.claim.ceiling`; S15 | F8 |
| Durable-byte hygiene | Mutable live state never enters these files | stable-vs-mutable law | F9 |
| Shared-authority consumption | No Zed-local packet/probe/evidence ontology | compatibility section | F10 |
| Proof executability | Placeholders/unresolved commands are never treated as executable | agentic execution law; this checklist's proof | F11 |
| Breadth discipline | Optional breadth stays optional without a selected claim | `zed.activation.platform` | F12 |
| Determinism | Same tree produces same ordered check output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) | Canonical three files plus `plan.md`, the explicit decision-map file requested by #11709 |
| Programme controller architecture | #7759 | Compiles its settled Zed decisions without implementing them |
| Stable train authority | #10338 | Decision IDs match its conflict-key namespace so Z01 consumes rows directly |
| Server-ID migration and occurrence classification | #10842 | `zed.server.identity`; legacy `perllsp` rejected as active ID (F1) |
| Reproducible materialization | #10395 | `zed.extension.materialization`; sequencing pending there, boundary compiled |
| Execution source selection | #11041 | `zed.extension.execution_source`; outcome pending there, distinctness compiled |
| Binary provenance/process identity | #10340 / #10530 | `zed.binary.provenance` |
| Exact command/argument authority | #11304 | `zed.launch.argv`: `perllsp --stdio`, no alias/wrap/fall-through (F2) |
| Settings/defaults/status identities | #10392 / #10393 / #11043 | `zed.settings.defaults.status` |
| Deterministic fixtures/expectations | #8647 | `zed.fixture.expectations` |
| Activation/platform breadth | #11046 / #10991 | `zed.activation.platform`; optional-breadth law (F12) |
| Public asset workflow/receipts | #8661 / #8678 | `zed.assets.public_contract`; S02 |
| Cache integrity/known-good preservation | #10396 / #8753 / #8772 | `zed.cache.integrity`; S06 |
| Mutation safety/rollback | #11316 | `zed.mutation.safety` |
| Offline/update network authority | #11308 | `zed.network.update` |
| Official-registry host authority | #9467 / #7912 | `zed.registry.host_authority`; S13 |
| Operation/evidence profiles | #10858 | Dependency and evidence classes consumed, not redefined |
| Builder/reviewer packets | #10872 / #10881 | Consumed downstream; no Zed-local packet ontology (F10) |
| Semantic close/support projection | #10168 | S14 promotion requires its owner (F5) |
| Extraction gate | #10554 | Respected: shared mechanics extracted only at its gate |
| Spec method | #3983 and current `.spec` conventions | No new spec schema; #3586 historical context only |
| Bundle precedent | `.spec/11763-issue-controller-architecture/` (PR #12006); `.spec/10894-editor-host-reliability/` (PR #11811) | Same checked discipline: structural proof, second-run determinism, honest NOT_PROVEN boundary |

## §API-Shape

No Rust or public API is introduced. The names below are semantic contract
terms owned by later nodes; they bind future implementation, they do not exist
yet:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `perl-lsp-rs` | Zed server ID | single active language-server key for this programme | #10842 |
| `perllsp --stdio` | exact launch argv | the only permitted command/argument form | #11304 |
| `zed.*` decision IDs | semantic conflict keys | namespace defined by #10338; rows here are the payload | #10338 |
| S01–S15 | stage vocabulary | evidence-class handoffs with owning roles | #10338 encodes; owners per row |
| F1–F12 | falsifier grid | fixed-order negative controls bound to decision IDs | consuming nodes' negative suites |

## §Test-Grid

All twelve programme falsifiers, fixed order, as they bind this bundle's
compiled decisions. Verdict semantics: every mutation must be rejected by the
compiled architecture — a later concrete node is conformant only if each
mutation fails deterministically in that node's negative controls.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---|---|---|---|---|
| 1 | `perllsp` retained as an active Zed server ID | stale | rejected: exactly one active ID, `perl-lsp-rs` | Any manifest/settings row keying `perllsp` as active fails validation against `zed.server.identity` |
| 2 | `perl-lsp-rs` routed through `perl-lsp` or another executable | opposite | rejected: launch argv is exactly `perllsp --stdio` | Launch-config diff whose resolved executable differs from `perllsp` fails against `zed.launch.argv` |
| 3 | A public asset receipt presented as real Zed behavior | wrong-subject | rejected: asset bytes/process are not host behavior | Stage-S02 receipt offered as S03/S06/S13 evidence fails stage-promotion validation |
| 4 | An exact-source receipt presented as official-registry proof | wrong-subject | rejected: exact-source behavior != registry distribution | S03/S06 evidence class satisfying an S13 requirement fails validation |
| 5 | Support docs promoted from a packet, merge, or host receipt without the projection owner | partial | rejected: S14 requires the #10168 owner | Support-surface change citing non-S14 provenance fails validation |
| 6 | DAP inserted into the LSP controller or support row | opposite | rejected: DAP is a separate rail/sidecar | Node graph edge placing `perl-dap` in the LSP support row fails rail-separation validation |
| 7 | A controller/fan-in/manual checkpoint rendered as builder work | wrong-subject | rejected: those roles are not assignable leaves | Node with role fan_in/packet_freeze/manual_checkpoint/external_gate carrying builder-writer obligations fails role validation |
| 8 | A leaf allowed to close #7759 from bounded authority or evidence work | partial | rejected: only S15 denominator completion closes the programme | Advances/Closes relation claiming the programme from a bounded leaf fails closeout validation |
| 9 | Current SHA/PR/check/model state embedded in the durable contract | instrument | rejected: durable bytes carry stable content only | Durable-file diff after any live-state change is empty; 40-hex digest scan of the four files finds nothing |
| 10 | A duplicate Zed-local packet/probe/evidence ontology created instead of consuming shared authority | opposite | rejected: #10858/#10872/#10881/#10168 are consumed, never cloned | New Zed-local schema duplicating a shared profile fails shared-authority validation |
| 11 | Placeholders or unresolved proof commands treated as executable | instrument | rejected: proof commands must be resolved and runnable | Leaf contract carrying TODO/placeholder proof fails executability validation |
| 12 | Optional breadth made a hard dependency for the bounded core without a selected claim requiring it | partial | rejected: breadth stays optional by default | Edge typing platform/activation/DAP breadth as hard without a selected-claim citation fails dependency-class validation |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, extension source, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, PR, review or metadata mutation |
| Downstream nodes | #10338 consumes decision IDs/stage vocabulary; #11710/#11711/#10479/#9483 consume their planes; packets derive via #10872/#10881 adapters |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes the Zed programme's stable architecture durable: identities,
decision ownership, evidence/publication stages, truth planes, claim ceilings,
agentic execution law, and explicit non-claims. It does not prove that any
tooling works, that any server launches, that any stage's evidence exists, or
that any release/support claim holds — those are owned by the consuming nodes
and remain `not_proven` here.

## Non-goals

No product implementation, extension source change, stable-DAG implementation,
current-tree classification, live observer, scheduler/lease/worktree manager,
agent launcher, packet schema or instance, exact-head closeout, support or
release claim, external submission, or external repository mutation.
