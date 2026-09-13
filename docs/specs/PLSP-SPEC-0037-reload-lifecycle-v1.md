# PLSP-SPEC-0037: Reload lifecycle and observation-route contract v1

Status: current
Owner: perl-lsp maintainers
Linked proposal: [#10874](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10874)
Linked ADRs: [ADR-0046](../adr/0046-loaded-module-reload-semantics.md) (adjacent, out of scope — see
[Non-authority boundary](#non-authority-boundary))
Linked plan: #9926 (semantic sequencing), #11205 / #11208 (machine topology), #9621 (downstream projection)
Status impact: none. This contract changes no runtime behavior, no public API, and no support claim.

RELOAD-LIFECYCLE-AUTHORITY: v1

This document is the single current in-repo architecture authority for the generation-bound
reload and observation-route lifecycle. Exactly one document in this repository may carry the
`RELOAD-LIFECYCLE-AUTHORITY` marker above; `tests/test_reload_lifecycle_authority.py` enforces
that, so a second spec or ADR cannot silently become a competing authority.

## Purpose

The repository has a coherent reload architecture spread across an issue graph, and no in-repo
document that owns it. An implementer reading current source plus older watcher and index prose
can independently derive any of the designs listed in
[Forbidden architecture patterns](#forbidden-architecture-patterns) and be locally consistent
while being architecturally wrong.

This spec fixes the durable part: which identities exist, which subsystem owns each state
transition, which transitions are legal, and which designs are rejected. It is deliberately not a
runtime state database, an implementation-readiness tracker, or a copy of the changing issue
train.

## Scope

```text
owns       durable identities, authority boundaries, state laws, forbidden designs
does not   runtime types, handler code, scheduling, issue status, performance targets,
           support promotion, release or publication claims
```

### Non-authority boundary

| Concern | Authority | This spec |
|---|---|---|
| Semantic implementation sequence and controller rulings | #9926 | consumes |
| Machine reload-train topology and one-PR node contracts | #11208 (validated by #11212, observed by #11216, packetised by #11220, under programme #11205) | consumes |
| Mutable exact-tree implementation observation | #11216 | consumes |
| Editor-intelligence implementation DAG | #9621 | downstream projection only; not the canonical reload DAG |
| DAP loaded-module reload semantics (live debuggee `%INC` replacement) | [ADR-0046](../adr/0046-loaded-module-reload-semantics.md) | **out of scope** — a different subject, a different process, and a different generation family |
| Canonical lifecycle scenario corpus | #11522 | seeds, does not own |
| Deterministic lifecycle barriers and controlled ports | #11526 | seeds, does not own |

ADR-0046 governs replacing code inside a live Perl debuggee under `perl-dap`. This spec governs
the language-server workspace reload lifecycle. They share the word "reload" and share no
identity, generation, or publication rule. Neither document may be cited as authority for the
other.

## Implementation reality

This contract is written against current `main`, not against an imagined tree. Each identity below
carries a today-status drawn from a closed vocabulary:

```text
implemented  a real type exists and already carries this meaning
partial      a real type exists but is narrower than this contract
contracted   no type exists today; the named owner must create it
```

`contracted` is a statement about current source, not a licence to invent the design elsewhere.
An implementer meeting a `contracted` row builds it under the named owner issue and this contract.

## Contract

### C1. Independent identities

Each identity below is independent. No identity may be substituted for, derived from, or compared
against another by string equality.

| Identity | Canonical owner | Today | Current realisation |
|---|---|---|---|
| RL-I01 client/original URI or display path | #8198 | partial | `lsp_types::Uri` is used directly; typed file authority, UNC, and Windows verbatim projection are #8198 work |
| RL-I02 canonical logical source identity | #8198 / #7088 | implemented | `LogicalSourceId` in `crates/perl-source-identity` |
| RL-I03 process-relative resource locality and host relation | #11582 | contracted | no type today |
| RL-I04 open document instance and document generation | #8041 (landed) | implemented | `DocumentState` / `ParsedSnapshot` generation in `crates/perl-lsp-rs` |
| RL-I05 closed-source input generation | #8052 | partial | stable-read and transactional closed-source load is #8052 work |
| RL-I06 process/application session identity | #10226 (landed) | implemented | `WorkspaceRuntimeSessionId` in `crates/perl-workspace` |
| RL-I07 workspace runtime (root) generation | #10013 (landed) | implemented | `WorkspaceRuntimeGeneration` as `(session_id, root_id, sequence)` |
| RL-I08 configuration generation | #7057 | implemented | `WorkspaceConfigurationGeneration` |
| RL-I09 trust generation | #7057 / #10016 | implemented | `WorkspaceTrustGeneration` |
| RL-I10 watch set generation | #8217 | partial | `WorkspaceFolderSetGeneration` tracks folder-set mutation; a watch-set generation distinct from the folder set is #8217 work |
| RL-I11 observation route plan generation | #11227 | contracted | no type today |
| RL-I12 observation route generation | #10770 | contracted | no type today |
| RL-I13 reload subject id | #7893 | partial | `perl-dap` has `LoadedModuleSubject` for its own out-of-scope subject; the workspace reload subject is #7893 work |
| RL-I14 reload operation id | #7893 | partial | `WorkspaceRuntimeOperationId` exists as an opaque caller-scoped id |
| RL-I15 operation budget and deadline identity | #7893 | contracted | no type today |
| RL-I16 private source/workspace/config/adapter candidate identity | #8594 | contracted | no type today |
| RL-I17 accepted workspace snapshot generation | #8619 | partial | `WorkspaceRuntimeView` snapshots exist; atomic accepted-generation publication is #8619 work |
| RL-I18 manual command request and result version/identity | #11533 | contracted | no LSP manual reload command exists today |
| RL-I19 client request/refresh operation identity | #7021 | contracted | no type today |
| RL-I20 proof/receipt subject identity | #11522 / #11526 | contracted | no type today |

Durable rules:

- **RL-R01.** Path or URI equality never substitutes for root, source, session, route, or reload
  identity. Two observations of the same path string in different sessions, roots, or route
  generations are different subjects.
- **RL-R02.** A displayed remote URI is not proof that the `perllsp` process can observe the
  resource through its local filesystem.
- **RL-R03.** Every load-bearing generation an operation read at admission must be revalidated
  before that operation publishes.
- **RL-R04.** A missing or instrument-failed identity is `NotProven`. It is never guessed and
  never defaulted to the previous value.

### C2. Authority table

| Authority | Owner | Consumed by | Must not own |
|---|---|---|---|
| file/root transition | #7088 and the canonical URI owners | #7893, domain ingress | watcher backend, publication |
| strict URI/file authority and target projection | #8198 | #11582, #7088 | locality, trust grant, route policy |
| process/application session identity | #10226 (landed) | #10013, #11582 | root composition, route policy |
| root runtime generation | #10013 (landed) | reload, routes, publication | configuration or source semantics |
| process-relative resource locality/access context | #11582 | workspace services, #11227, #10777, #10016 | URI parsing/projection, trust grant, route policy, watcher backend, source read, support proof |
| open source | #8041 (landed) | source subject | disk watcher truth |
| closed stable input | #8052 | source subject | open editor state |
| configuration generation | #7057 | reload, configuration effects | route choice |
| configuration precedence and transactional effects | #6736 | #7057 | route choice, reload ordering |
| source watch set | #8217 | #11227 | generic backend, current route |
| adapter watch set | #7894 | #11227 | generic backend, evaluation |
| project/profile subjects | #7900 / #7123 | #11227, #7893 | generic route, backend |
| desired observation-route plan and fallback policy | #11227 | #10770 | request outcomes, watcher backend, reconciliation, semantics |
| observation-route lifecycle and currentness | #10770 | #10016, #7933, #9858, watch subjects, status | route policy, backend implementation, semantic current input |
| internal watcher backend | #10777 | #10770 | desired membership, route choice, semantic reload |
| reload operation | #7893 | coalescer, subjects, status | filesystem backend, workspace snapshot |
| coalescing | #8064 (landed) | #7893 | semantic mutation |
| manual wire contract | #11533 | #10788, #11537 | reload semantics, client presentation |
| manual server adapter | #10788 | #7893, #10770 | semantic reload implementation, editor UX |
| first-party VS Code repair | #11537 | #11533, #10788 | server semantics, route or support authority |
| workspace candidate | #8594 | #8619 | live swap, readiness |
| workspace publication | #8619 | readers, effects | source/root/config identity creation |
| snapshot reads | #8642 | providers | lifecycle mutation |
| index lifecycle and readiness | #10434 / #3099 | providers, status | pending-count authority |
| client refresh outcomes | #7021 / #9858 | clients | internal accepted-state rollback |
| scenario semantics | #11522 | implementation and proof leaves | product behavior, execution scheduler |
| deterministic test ports | #11526 | product tests | scenario semantics, exact-client proof |
| external topology evidence | #4261 / #9849 | support claims | runtime authority |

Four rows above name owners whose authority has **landed** (#8041, #8064, #10013, #10226). Landed
means the authority exists in current source and is no longer a pending prerequisite. Downstream
issue prose that still cites them as blocking predecessors is stale relative to `main`; this table
is the current reading.

### C3. Reload operation state machine

```text
Observed
→ Invalidating
→ PendingCoalesce
→ LoadingCurrentInput
→ BuildingCandidate
→ Validating
→ Publishing
→ Current
```

Terminal outcomes, all distinct:

```text
NoOpCurrent                  observation resolved to no semantic change
RemovedCurrent               subject is authoritatively absent
InvalidOrUnavailableCurrent  current input exists but is not usable under its domain policy
BudgetExhausted              operation exceeded its own budget or deadline
Cancelled                    a caller withdrew the operation
Superseded                   a later operation for the same subject took ownership
DetachedRoot                 the operation's root generation is no longer current
Shutdown                     the runtime is terminating
ProductFailure               the product under test behaved wrongly
InstrumentFailure            the observation or harness failed; the product is unproven
```

Rules:

- **RL-R05.** A terminal stale operation never becomes publishable again.
- **RL-R06.** Invalid current input is not prior-success current. See [C8](#c8-domain-invalidity-versus-transactional-rejection).
- **RL-R07.** `Cancelled`, `Superseded`, and `BudgetExhausted` are three different outcomes and may
  not be collapsed into one failure class.
- **RL-R08.** Accepted invalidation happens before expensive debounce for semantic changes. A
  subject is marked stale at observation, not at dispatch.
- **RL-R09.** `ProductFailure` and `InstrumentFailure` are distinct. An instrument failure leaves
  the product claim `NotProven` and never reports a passing subject.

### C4. Desired observation-route plan

The #11227 boundary is pure: inputs to one immutable ordered plan, with no transport, no I/O, and
no semantics.

```text
current desired subject and spec
+ exact root, configuration, trust, watch, locality, capability and backend policy
→ one immutable ordered ReloadObservationRoutePlan
→ one selected route class and its typed fallback successors
```

Rules:

- **RL-R10.** Equivalent capability surfaces are client-name invariant. A client rename, or a
  different client reporting the same capabilities, yields the same plan.
- **RL-R11.** Workspace-local, explicit user or global, remote-host-local, and virtual or non-file
  resources remain four distinct classes.
- **RL-R12.** External or remote resources never gain local or workspace watcher authority by path
  coercion.
- **RL-R13.** At most one automatic route is selected for one logical subject at a time.
- **RL-R14.** Fallback creates a successor plan and route identity after a typed terminal outcome.
  It never mutates the failed plan in place.
- **RL-R15.** A route may degrade to `ManualOnly` or `Unavailable`. It never degrades to broad
  polling.

### C5. Observation route lifecycle

```text
DesiredPlan
→ Installing
→ ActiveNeedsReconciliation
→ ActiveCurrent
→ Replacing | ManualOnly | Unavailable | Detached | Shutdown
```

Rules:

- **RL-R16.** Emitting a registration request is not `Active`.
- **RL-R17.** Transport activation is not `ActiveCurrent`.
- **RL-R18.** `ActiveCurrent` requires same-generation reconciliation across the plan, root,
  configuration, trust, and watch generations the route was installed under.
- **RL-R19.** Callbacks from a superseded route never affect a successor route, root, configuration,
  or watch generation.
- **RL-R20.** `ManualOnly` is a valid availability and support state. It is not automatic reload,
  and completing a manual reload never promotes it to automatic.
- **RL-R21.** #10770 executes #11227 policy. It never chooses a route locally.
- **RL-R22.** `ActiveCurrent` means the observation route is current, not that the subject is
  semantically valid. A complete reconciliation may establish that the current subject input is
  invalid; the route is current and the domain status is invalid or action-required. A failed,
  incomplete, or instrument-failed reconciliation cannot make the route current.

### C6. Publication

```text
private candidate
→ revalidate source, root, configuration, trust, reload, budget and invariants
→ one accepted generation swap or commit
→ separately owned effects and readiness
```

Rules:

- **RL-R23.** Publication is one accepted-generation swap. Partial mutation of an accepted workspace
  map with a rollback path is rejected as final architecture.
- **RL-R24.** Effects and readiness are owned separately from the swap (#10434, #9858). Publication
  does not itself declare readiness.
- **RL-R25.** `pending == 0`, a non-empty index, or any pending count is never readiness authority.

### C7. Manual reload request and result

```text
versioned all-roots or exact-current-root request
→ bounded server adapter request admission
→ canonical #7893 / #10770 reconciliation
→ one terminal result
```

Terminal results:

```text
current | no_op_current | accepted_pending | limited | rejected |
superseded | cancelled | timed_out | detached | shutdown |
product_failure | instrument_failure
```

Rules:

- **RL-R26.** Request emission, request acceptance, pending work, and semantic currentness are four
  different facts.
- **RL-R27.** Manual completion never promotes automatic observation-route support.
- **RL-R28.** Manual reload is not a server restart and its result is never a bare boolean.

### C8. Domain invalidity versus transactional rejection

Two cases look similar and carry different authority semantics.

**Authoritative current input became invalid.** Closed source bytes are now unreadable or invalid
under a source policy; an adapter definition changed to bytes its contract treats as invalidating.
The prior semantic output cannot silently remain `exact_current`. The domain exposes invalid,
unavailable, reloading, or current-with-limitation according to its owner.

**A new candidate layer is rejected transactionally.** Some configuration and profile domains
(#6736, #7123) define an invalid attempted update as rejected before it becomes the accepted
semantic generation, preserving the previously accepted effective configuration while separately
exposing the invalid or action-required input state:

```text
observed input/candidate   = invalid_rejected
accepted semantic generation = prior accepted generation (unchanged)
input/source status        = invalid / action_required
claim about the invalid candidate = none
```

- **RL-R29.** Transactional rejection is not last-known-good pretending an invalid input succeeded.
  The rejected candidate is never described as current accepted configuration.
- **RL-R30.** Subject owners choose which policy applies. #7893 orders and supersedes operations; it
  does not impose one invalidity policy on every domain.

### C9. Process-relative resource locality

```text
#8198 strict URI/file authority and target projection
+ #10226 process/application session identity
+ #10013 current root generation
+ supplied path/trust authority provenance
→ #11582 ResourceAccessContext
→ #11227 route plan
→ #10770 route lifecycle
→ #10777 backend where selected
```

- **RL-R31.** `file:` or path-like text, a client name, and extension-host prose do not establish
  locality.
- **RL-R32.** A remote product may be server-local when exact session, root, and URI projection
  prove `perllsp` runs on the workspace host.
- **RL-R33.** A local process never native-watches a remote or non-file resource by path coercion.
- **RL-R34.** The same URI or path after a restart or root re-add receives a new access context.
- **RL-R35.** External-path locality and external-path authorization are separate decisions.
- **RL-R36.** A missing or instrument-failed host relation is `NotProven`, not a guess.
- **RL-R37.** #4261 and #9849 prove external topology for support claims; they never become runtime
  authority.

## Worked sequences

Each sequence states the identities before, the evidence observed, who may mutate what, which
generation becomes stale, what may keep running privately, what may publish, the terminal result,
and the external claim that is and is not earned. These rows seed the #11522 corpus; #11522 owns
stable scenario identity thereafter.

### RL-S01 closed file normal modify

Before: root `Rn` current, config `Cn`, route `RT1` `ActiveCurrent`, source `S` closed at input
generation `In`. Evidence: watched-file change for `S`. Mutation: #7893 owns the operation; #8064
owns coalescing; #8052 owns the stable read. Stale: `In`. Private: candidate build under `In+1`.
Publishes: one accepted workspace generation. Terminal: `Current`. Earned: `S` facts are current at
`In+1`. Not earned: any claim about unobserved siblings.

### RL-S02 open unsaved buffer plus external disk modify

Before: `S` open with document generation `Dk` (#8041), disk copy changed. Evidence: watched-file
change for a path whose logical source is open. Mutation: the open-source owner only. Stale:
nothing in the open document. Publishes: nothing that replaces buffer content. Terminal:
`NoOpCurrent` for the disk observation. Earned: the disk change is recorded as external divergence.
Not earned: disk bytes becoming the semantic input for an open buffer. This is the direct falsifier
for [RL-F07](#forbidden-architecture-patterns).

### RL-S03 atomic temp-file replace

Before: `S` closed at `In`. Evidence: create, rename, and delete events for temporary siblings plus
a final rename onto `S`. Mutation: #7893 after #8064 coalescing. Stale: `In`. Terminal: `Current`
at `In+1`, never an interleaved `RemovedCurrent`. Earned: one logical modify. Not earned: a delete
claim from the intermediate window.

### RL-S04 rename with watcher plus explicit file-operation evidence

Before: `S` at logical identity `L1`. Evidence: a watcher rename pair and a client file-operation
notification. Mutation: #7088 owns the transition. Stale: `L1` bindings. Terminal: `RemovedCurrent`
for `L1` and `Current` for `L2`. Earned: one rename. Not earned: treating the two evidence planes
as two independent renames.

### RL-S05 root remove and re-add at the same URI

Before: root `Rn` at URI `U`. Evidence: workspace folder removed, then added at `U`. Mutation:
#10013 mints `Rn+1`; #11582 mints a new access context (RL-R34). Stale: every `Rn`-bound route,
operation, and candidate. Terminal: `DetachedRoot` for in-flight `Rn` work. Earned: a fresh root.
Not earned: reuse of an `Rn` callback, route, or cached locality. This is the ABA falsifier.

### RL-S06 configuration Cn to Cn+1 while reload N is held

Before: operation `N` admitted under `Cn`, currently in `PendingCoalesce`. Evidence: accepted
configuration generation advances to `Cn+1`. Mutation: #7057 publishes `Cn+1`; #7893 revalidates.
Stale: `N`'s admission basis. Publishes: nothing under `Cn`. Terminal: `Superseded` for `N`; a
successor is admitted under `Cn+1`. Earned: current answers under `Cn+1`. Not earned: publishing
`Cn` work as current (RL-R03).

### RL-S07 route plan for workspace-local, explicit external, and remote subjects

Before: three desired subjects — one inside a workspace folder, one an explicit user or global path
outside every folder, one on a remote host. Evidence: identical capability surface. Mutation:
#11227 only. Terminal: three plans in three distinct classes (RL-R11). Earned: per-class routes.
Not earned: coercing the explicit external path into a workspace-relative pattern, or granting the
remote subject a local native watcher (RL-R12, RL-R33).

### RL-S08 LSP watcher activation with a scan-to-registration blind window

Before: route `RT1` `Installing`; an initial scan is in flight. Evidence: registration succeeds
while the scan has not completed. Mutation: #10770. Stale: nothing yet. Terminal:
`ActiveNeedsReconciliation`, then `ActiveCurrent` only after same-generation reconciliation
(RL-R18). Earned: coverage from reconciliation onward. Not earned: treating registration success as
coverage of the blind window (RL-F10), or treating scan absence as a delete.

### RL-S09 route replacement with a late old callback

Before: `RT1` `Replacing`, `RT2` `Installing`. Evidence: a callback arrives bearing `RT1`'s route
generation. Mutation: none. Terminal: the callback is discarded (RL-R19). Earned: nothing. Not
earned: advancing `RT2` or any root or configuration generation from `RT1` evidence.

### RL-S10 internal watcher overflow

Before: `RT2` is an internal backend route (#10777) and `ActiveCurrent`. Evidence: the backend
reports a bounded-queue overflow. Mutation: #10777 reports; #10770 demotes. Stale: `RT2`'s
`ActiveCurrent` claim. Terminal: `ActiveNeedsReconciliation`. Earned: honest degradation. Not
earned: silently dropping subjects while still claiming currentness, or switching to broad polling
(RL-R15).

### RL-S11 no watcher available

Before: every automatic class in the plan has reached a typed terminal outcome. Evidence: exhausted
fallback successors. Mutation: #10770. Terminal: `ManualOnly`. Earned: manual reload availability
and an honest support statement. Not earned: an automatic-reload support claim (RL-R20).

### RL-S12 accepted workspace generation to readiness, effects, and refresh

Before: a validated candidate at `Wn+1`. Evidence: publication commits. Mutation: #8619 swaps;
#10434 owns readiness; #7021 and #9858 own refresh. Terminal: `Current`, then separately owned
readiness and outcome-aware refresh. Earned: providers read `Wn+1` coherently. Not earned:
publication declaring readiness itself (RL-R24), or a refresh request emission counting as a
delivered refresh.

### RL-S13 budget expiry immediately before publication

Before: operation `N` in `Validating` with its deadline (RL-I15) about to expire. Evidence: the
deadline passes during revalidation. Mutation: none. Terminal: `BudgetExhausted`; the private
candidate is discarded. Earned: an honest incomplete state. Not earned: publishing an unvalidated
candidate, or reporting `Cancelled` or `Superseded` instead (RL-R07).

### RL-S14 manual command accepted-pending versus terminal current

Before: `ManualOnly` after RL-S11. Evidence: a versioned manual reload request for the current root.
Mutation: #10788 admits; #7893 reconciles. Terminal: `accepted_pending` at admission, then exactly
one of the C7 terminal results. Earned: the terminal result only. Not earned: reporting
`accepted_pending` as `current` (RL-R26), or promoting the route to automatic (RL-R27).

### RL-S15 remote host-local positive

Before: a `vscode-remote`-shaped client URI. Evidence: exact session (#10226), root (#10013), and
URI projection (#8198) prove `perllsp` executes on the workspace host. Mutation: #11582 mints a
server-local access context. Terminal: a workspace-local route class is permitted. Earned:
native observation for that subject. Not earned: generalising to every remote-shaped URI (RL-R32).

### RL-S16 local process with a remote subject, negative

Before: `perllsp` runs on the client host. Evidence: the subject resolves to a remote host
relation. Mutation: #11582. Terminal: no native local route; the plan degrades within its class.
Earned: an honest unavailable or manual state. Not earned: native watching by path coercion
(RL-R33).

### RL-S17 path-like non-file resource

Before: a virtual or non-file scheme whose text resembles a path. Evidence: URI projection (#8198)
reports non-file. Terminal: the virtual class; no filesystem route. Earned: nothing beyond the
virtual class. Not earned: locality from path-like text (RL-R31).

### RL-S18 external path authority

Before: an explicit user or global path outside every workspace folder, and locality established.
Evidence: trust and authority provenance is absent. Terminal: locality is established and
authorization is not; the subject is not observed. Earned: a precise reason. Not earned: treating
established locality as an authorization grant (RL-R35).

### RL-S19 instrument failure during reconciliation

Before: `RT1` `ActiveNeedsReconciliation`. Evidence: the reconciliation probe itself fails. Mutation:
none. Terminal: `InstrumentFailure`; the route stays `ActiveNeedsReconciliation`. Earned: nothing.
Not earned: `ActiveCurrent`, a `ProductFailure` verdict, or a passing subject claim (RL-R09,
RL-R22, RL-R36).

## Forbidden architecture patterns

A design matching any row below is rejected regardless of local test results.

| Rule | Forbidden design |
|---|---|
| RL-F01 | URI or path strings used as lifecycle generation authority |
| RL-F02 | Client-name route selection |
| RL-F03 | Separate source, configuration, and adapter watcher backends |
| RL-F04 | Two current automatic routes for one logical subject, deduplicated by path |
| RL-F05 | Unbounded watcher or debounce queues |
| RL-F06 | Semantic mutation performed on watcher or debounce callback threads |
| RL-F07 | Last-known-good becoming current after current input is invalid |
| RL-F08 | Direct accepted `WorkspaceIndex` mutation before candidate validation, as final architecture |
| RL-F09 | Per-handler generation counters duplicating #10013, #7057, or #7893 |
| RL-F10 | LSP request emission treated as terminal success |
| RL-F11 | Watcher handle creation treated as semantic currentness |
| RL-F12 | Sleep-based race proof |
| RL-F13 | `pending == 0` or a non-empty index treated as readiness authority |
| RL-F14 | Manual reload implemented as a restart or returning a bare boolean result |
| RL-F15 | Broad polling used as watcher fallback |
| RL-F16 | A cache or database changefeed driving live reload |
| RL-F17 | Controlled-port, exact-process, client, or installed claims inferred across evidence planes |
| RL-F18 | Route policy and route lifecycle collapsed into one owner |
| RL-F19 | A successful manual reload promoting automatic observation support |
| RL-F20 | Locality inferred from path-like text, client name, or extension-host prose |

## Negative controls

A reviewer challenges an implementation of this contract by attempting each wrong design below and
confirming the contract forbids it.

```text
URI-only debouncer                                         RL-R01, RL-F01
watcher callback indexes directly                          RL-R08, RL-F06
client name chooses the route                              RL-R10, RL-F02
LSP and internal watchers both current for one subject     RL-R13, RL-F04
registration success marks the route current               RL-R16, RL-R17, RL-F10
internal watcher handle marks currentness                  RL-R17, RL-F11
manual reload restarts the server or returns a bool        RL-R28, RL-F14
partial accepted-map mutation with rollback                RL-R23, RL-F08
configuration request id used as configuration generation  RL-R01, RL-F09
same root URI reuses a stale callback                      RL-R19, RL-S05
stale facts stay current while invalid input rebuilds      RL-R06, RL-F07
controlled-port pass promoted to installed support         RL-R09, RL-F17
explicit external path becomes a relative pattern          RL-R12, RL-S07
backend failure authorises polling                         RL-R15, RL-F15
successful manual reload promotes automatic support        RL-R27, RL-F19
```

## Single-authority rule

- **RL-R38.** Exactly one document in this repository carries the `RELOAD-LIFECYCLE-AUTHORITY`
  marker, and it is this file. A second spec or ADR adding the marker fails
  `tests/test_reload_lifecycle_authority.py`.
- **RL-R39.** This spec is listed in the `docs/specs/README.md` catalog and in `docs/INDEX.md`. An
  unindexed architecture authority is not discoverable and is treated as a contract defect.
- **RL-R40.** Conflicting active watcher or reload doctrine must carry a disposition in
  [Disposition of adjacent doctrine](#disposition-of-adjacent-doctrine) before it can be read as
  current.

### Disposition of adjacent doctrine

An inventory of in-repo documents that describe watcher, file-change, index-refresh, or reload
behavior, and how each relates to this contract.

| Document | Disposition | Reason |
|---|---|---|
| [ADR-0046](../adr/0046-loaded-module-reload-semantics.md) | current, different subject | Accepted DAP loaded-module reload contract. Does not describe workspace observation routes or workspace publication, and is not superseded by this spec. |
| [PLSP-SPEC-0022](PLSP-SPEC-0022-module-path-authority.md) | current, upstream input | Module path authority. Feeds URI and source identity; owns no reload transition. |
| [PLSP-SPEC-0023](PLSP-SPEC-0023-ambient-inputs.md) | current, upstream input | Ambient inputs. Feeds configuration and environment identity; owns no route or publication rule. |

No in-repo document currently claims workspace reload lifecycle or observation-route authority, so
this spec supersedes nothing. A document later found to conflict is added here with an explicit
disposition rather than silently outranked.

## Valid PR shapes

- An implementation PR under a named owner issue that creates a `contracted` identity, cites the
  `RL-I*` row, and keeps every `RL-R*` rule it touches.
- A PR that promotes a `partial` row to `implemented` and updates that row's today-status.
- A PR that adds a worked sequence when a new lifecycle case is discovered, seeding #11522.
- A PR that adds a forbidden row after a real wrong design was caught in review.

## Invalid PR shapes

- A PR that introduces a second reload-lifecycle authority document.
- A PR that deletes or weakens an `RL-R*` or `RL-F*` rule to make an implementation pass.
- A PR that marks an identity `implemented` without a real type in current source.
- A PR that cites this spec as authority for DAP loaded-module reload, or cites ADR-0046 as
  authority for workspace reload.
- A PR that copies the changing issue train into this spec.

## Acceptance

- [x] One current indexed reload lifecycle spec exists in-repo.
- [x] Load-bearing identities are separated and their canonical owners named (C1).
- [x] Route planning, route activation, reload operation, publication, and manual command state
      machines are explicit (C3–C7).
- [x] Nineteen representative sequences make currentness, supersession, and authority concrete.
- [x] Remote and locality rules, client-name invariance, and the single-automatic-owner rule are
      explicit (C4, C9).
- [x] Forbidden duplicate-authority patterns are explicit and mechanically guarded (RL-F*, RL-R38).
- [x] Adjacent watcher and reload doctrine carries an explicit disposition.
- [x] #9926 is semantic order authority; #11205 and #11208 are machine reload-train authorities;
      #9621 is a downstream projection.
- [x] #11522 can compile stable scenario rows without becoming another architecture authority.
- [x] Identity, authority, and state rules are derivable without issue archaeology.
- [x] No runtime behavior changes.

## Proof Commands

```bash
python3 -m unittest tests/test_reload_lifecycle_authority.py
cargo xtask ci-hygiene check-doc-paths docs/specs
git diff --check
```

## Non-goals

- Runtime implementation of any `contracted` identity.
- A new general documentation or specification framework.
- A machine scheduler, issue-status mirror, or readiness graph.
- Editor-specific guide rewrites, performance target selection, support promotion, or release and
  publication claims.

## Claim Boundaries

- This spec constrains architecture. It proves no runtime behavior and earns no support claim.
- `implemented` in C1 means a type carrying that meaning exists in current source. It does not mean
  the surrounding lifecycle is complete.
- `contracted` means no type exists today. Citing a `contracted` row is not evidence that the
  behavior works.
- Landed-authority annotations in C2 describe `main` at the time this spec landed. Current source
  and live GitHub state remain authoritative for the underlying facts.
