# Context: #11763 — compile the durable issue-controller architecture

This is a checked, declarative programme-architecture contract. It changes no
role registry, label, issue body, GitHub metadata, train readiness, candidate
state, product behavior, support claim, or external state.

## Problem

The repository has many programme, semantic, routing, proof, release, train and
meta controllers (parent #11681 is one). Their roles, child routes and closeout
boundaries are encoded across title prefixes, opening prose, labels, checked
programme manifests, comments and human memory. A controller can be mistaken
for a build leaf; a real leaf can require several levels of issue archaeology
before a coding agent finds its scope, current source, candidate, tests and
stop point. The accepted architecture that fixes this is distributed across
issue bodies and generic agent-method issues, so every later agent re-derives
it and may derive it differently.

Before the stable train (#11764 onward) becomes the execution authority, the
lasting decisions must be compiled into one checked programme-level bundle that
later nodes consume instead of rebuilding.

## Why this approach

The architecture is projected as a spec-only bundle first, exactly as the
sibling precedent `.spec/10894-editor-host-reliability/` (PR #11811) did for
the editor-host rail: stable decisions are made durable and reviewable before
any tooling exists. A tooling-first train would freeze accidental mechanics as
authority; an issue-prose-only train would leave every leaf re-reading
controller archaeology. The controlling issue #11763 fixes the method authority:
#3983 and the current `.spec` tooling conventions (`docs/reference/SPEC_TEMPLATE.md`);
no new repository-wide spec schema is invented, and the closed historical
proposal #3586 is context only.

## Current state (honest, as of this bundle)

Nothing in this section is a claim about the future; it records what exists on
the exact tree this bundle lands on:

- Issue control today is **manual**. Roles are signaled by title prefixes
  (`spec(...)`, `tooling(...)`, `test(...)`, `program(...)`, `ci(...)`,
  `ops(...)`, `integration(...)`), status/priority labels (`status:ready`,
  `status:blocked`, `P1-high`), issue prose, and the routing prose in
  `AGENTS.md` (the `$deliver-goal` / `$deliver-pr` / `$prepare-issue` /
  `$prepare-proof` / `$build-candidate` / `$finish-pr` route selection).
- `.claude/workflows/spec-builder.js` is a **markdown fan-out workflow only**:
  six parallel haiku passes that populate `§Hazards`, `§Contracts`,
  `§API-Shape`, `§Test-Grid` and `§Blast-Radius` sections plus a prior-art
  block. It contains no issue-role, registry, navigation, metadata or drift
  machinery.
- The repository has **no issue-controller tooling**: no role schema, no
  reviewed registry, no `issue_controller_*` contracts, no directory/router,
  no label projection, no metadata planner/applicator, no drift observer.
- Sixty-eight checked `.spec/` bundles existed at this bundle's compilation
  base under the three-file `SPEC_TEMPLATE.md` convention (a compilation-time
  observation, not a durable invariant); none covered the issue-controller
  programme.
- The programme issues #11681–#11789 are open and **unstarted**: the six
  functional-rail children #11682–#11687 sit under #11681, and the eighteen
  execution leaves #11764–#11785 carry `status:blocked` behind this node (S00).

## Authority and ownership

### Programme controller: #11681

Owns the programme meaning: make controller roles, labels and navigation
explicit over existing programme authorities. It is a controller
(`assignable = false`); no implementation PR lands against it. Its corrected
architecture introduced the four truth planes and the execution train that
this bundle compiles.

### Functional rail authority (initial #11681 train)

Six distinct one-PR propositions, preserved independently:

| Node | Issue | Durable authority |
| --- | --- | --- |
| C01 | #11682 | Role vocabulary (`controller`, `implementation`, `proof`, `fan_in`, `external_gate`), assignability, and non-authoritative candidate discovery |
| C02 | #11683 | Reviewed active controller registry: home/import/route adjudication and the complete controller denominator |
| C03 | #11684 | Role-label vocabulary and generated issue-navigation projection contract (dry-run only) |
| C04 | #11685 | Stable repository controller directory and deterministic offline router (`route`/`explain`/`docs`) |
| C05/R05A | #11686 | Bounded metadata planner/applicator/verifier/rollback tooling; no broad live migration |
| C06 | #11687 | Read-only live metadata drift observer and projection; never mutates GitHub |

### Execution train authority (S00 and the eighteen blocked leaves)

| Node | Issue | Durable authority |
| --- | --- | --- |
| S00 | #11763 | This bundle: programme-wide durable architecture and evidence-boundary compilation |
| T01 | #11764 | Stable `issue_controller_train.v1` topology and node contracts |
| T02 | #11765 | Independent static validation and the sole checked graph projection |
| T02R | #11767 | Semantic train revision governance and impact invalidation |
| T03 | #11769 | Exact immutable-tree implementation observation |
| T04 | #11771 | Deterministic offline frontier and blocker chains (planning projection only) |
| T05 | #11772 | Exact-tree component/instruction/test/generated-output context per node |
| T06 | #11773 | Explicit read-only live candidate and collaboration reconciliation |
| T02S | #11774 | Checked per-node leaf specification compilation and exact dispositions |
| T07 | #11775 | Derived bounded builder/reviewer/reconciliation packets (adapter into #10872/#10881) |
| T08 | #11776 | Exact-head structural closeout for declared train nodes |
| T08C | #11784 | Cross-cutting sufficient-proof routing into the existing CI router |
| I01 | #11777 | Generic existing-issue work-entry cutover; duplicate heuristic retirement |
| I02 | #11778 | Shift-left reviewed role and route during new-issue preparation |
| P01 | #11779 | Independent composed role/registry/directory/metadata/work-entry proof |
| P02 | #11783 | Final exact-current fan-in, maintenance projection and controller closeout |
| D01 | #11781 | Deterministic packet-sufficiency and routing scenario suite (no real models) |
| D02 | #11782 | Bounded fresh-agent, lower-cost and independent-review dogfood cohorts |
| R05B | #11785 | Privileged live metadata operation and immutable migration receipt |

### Generic authorities consumed, never cloned

`#3983`/`.spec` tooling (spec method), `#3949` (development method), `#10858`
(typed dependency/evidence semantics), `#10872` (model-neutral bounded builder
packets), `#10881` (adversarial review/finding/closure packets),
`#4177`/`#3982`/`#3957` (existing-work and writer admission),
`#3693`/`#10168` (review/currentness and semantic closeout), `#3390`/`#1848`/
`#4787`/`#4789` and successors (generic CI route/result/fan-in), `#10554`
(shared train-mechanics extraction gate), `#11114` (generic fresh/lower-cost
agent evaluation). A child may consume another layer's exact result; it may
not rebuild or silently widen it. Runtime packet instances remain ephemeral,
content-addressed outputs; no tracked active-goal pointer, task database,
model roster, lease table or current packet archive is created.

## Durable laws

### Primary issue roles and assignability

Every reviewed registry row has exactly one primary role:

| Role | Owns | Assignable |
| --- | --- | --- |
| `controller` | A conjunction, route, programme, semantic boundary, closure or evidence denominator decomposed into child leaves | `assignable = false` for ordinary implementation; metadata/navigation reconciliation may edit the issue without making it a product PR |
| `implementation` | One assignable one-PR repository proposition (product, tooling, spec, containment, cutover, migration, retirement, generated contract) | `true` |
| `proof` | Assignable evidence/acceptance work; may expose product defects but may not repair product behavior inside the proof PR | `true` |
| `fan_in` | Aggregation/closeout projection; validates current child evidence; cannot execute missing work or make non-pass evidence green | `true` |
| `external_gate` | Separately authorized external/privileged operation (for example the live metadata migration R05B); authorization is never inferred from tool availability, readiness, labels or a green PR | `true` only with explicit authorization |

Title, body or label signal is not a reviewed role. A domain noun such as an
MVC, host or process `controller` in an issue's subject matter does not make
the issue a controller role.

### Relationship vocabulary

- **home programme**: every semantic controller has exactly one home
  programme; a second home requires an explicit import relation.
- **import**: cross-programme consumption reference; the importing programme
  consumes the exact result and may not rebuild or widen it.
- **parent/child**: the controller chain; the controller owns decomposition,
  the child owns one proposition.
- **closeout**: the fan-in boundary and its denominator; controller
  open/closed is not child-denominator completeness.
- **supersession**: transferred/superseded/historical controller routes
  retained for navigation with an exact destination.

### Candidate discovery versus reviewed role adjudication

Discovery (C01) computes a non-authoritative candidate inventory from offline
signals: title prefixes, explicit role markers, checked train manifests,
controller/parent links, and manual exceptions with source and reason. Only
reviewed adjudication (C02) turns a candidate into a registry row with a
primary role, home and route. A candidate is not a role; an inventory row is
not an adjudication. The denominator is not limited to title prefixes, and a
body mentioning a domain controller does not add the issue to the denominator.

### Stable registry versus generated labels, navigation and directory

The reviewed registry (C02) is the stable authority for roles and navigation
relationships. Labels, per-issue navigation blocks, and the repository
directory (C03/C04) are deterministic generated projections of it. A registry
row does not mean the projection is applied live; an applied label does not
create authority. The directory records navigation-bearing relationships only;
it is not a global implementation DAG, scheduler or task database, and it does
not duplicate complete programme leaf DAGs, which remain programme-local.

### Bounded expected-old-state GitHub metadata mutation

The only broad metadata writer is the reviewed migration (R05A tooling #11686,
R05B operation #11785). It plans from immutable snapshots, validates
expected-old-state before applying, batches, verifies after, and produces an
immutable rollback-planning receipt. Mutation is bounded to role/disposition
labels and the generated navigation block; it never rewrites semantic issue
prose outside a generated block, and it never mutates registry semantics,
programme graphs, product behavior, support state, assignees, milestones, PRs,
reviews, merges or releases. Authorization for R05B is explicit and external to
any builder packet.

### Read-only metadata drift observation

Drift observation (C06 #11687) compares exact live GitHub issue/label metadata
against the checked registry and generated projections, and emits one
deterministic drift report and a focused correction route. It performs no
GitHub mutation of any kind. Drift never rewrites the registry to match
GitHub: registry changes flow only through reviewed adjudication. Metadata
drift clean is not product/support truth.

### Durable truth planes (load-bearing invariant)

1. stable issue-controller architecture and train topology;
2. exact current-tree implementation state;
3. live branch/worktree/PR/check/review/writer and GitHub metadata state;
4. behavior/evidence/support/release/external truth.

No plane substitutes for another:

```text
title/body/label signal       != reviewed issue role
issue closed                  != implementation current on tree
registry row                  != label/navigation applied live
label/navigation applied      != programme leaf readiness
current-tree implementation   != candidate vacancy or review currentness
packet generated              != work assigned or delivered
proof producer exists         != required proof executed
metadata drift clean          != product/support truth
controller open/closed        != child denominator complete
```

### Semantic train revision and invalidation

The programme will change: controllers split, leaves become fan-ins, new
programme-local trains appear, labels evolve, migrations begin, superseded
routes are discovered. Revision governance (T02R #11767) owns deterministic
semantic change classification and impact projection. A material train
revision invalidates affected downstream artifacts — probes, specs, packets,
candidates, reviews and current metadata plans become stale and must be
re-derived, not patched into apparent validity. Revision never rewrites the
manifest to make it pass and never mutates GitHub.

### Exact-tree context and shared actor-packet boundaries

Exact current-tree implementation state (T03 #11769) is derived from
proposition-specific repository probes on one exact committed tree; issue,
PR, label and check state are irrelevant to it. Bounded per-node context
(T05 #11772) maps each node onto that exact tree so a fresh agent needs no
controller archaeology. Live candidate reconciliation (T06 #11773) is a
separate read-only plane. Builder, reviewer and reconciliation packets
(T07 #11775) are derived by adapting into the shared #10872/#10881 packet
contracts — the builder and reviewer criterion sets remain distinct
contracts, never one mirrored set — and packet instances stay ephemeral.

### Generic work-entry adoption and old-heuristic retirement

Generic work preparation (I01 #11777) routes existing-issue entry through the
checked directory and then hands off to the issue's home programme-local
train, exact numbered leaves, proof/fan-in route or external/manual boundary.
New-issue preparation (I02 #11778) shift-lefts role/route admission so no new
untyped controller prose enters the repository. Adoption retires the old
title/body/label heuristics for routing; it does not remove title prefixes as
human-readable signal, does not bypass the named public-route skills defined
by `AGENTS.md`, and does not decide programme-local readiness, claim a writer
or mutate GitHub.

### Exact-head closeout, composed proof and fresh-agent dogfood

Closeout (T08 #11776) binds one exact node, base, head, diff and current
evidence set to the checked train/spec/context/packet and requires every owned
closure obligation to be current; focused green tests or a detailed issue body
alone cannot claim completion. Composed proof (P01 #11779, fan-in P02 #11783)
proves the composed system without granting any proof PR product-repair
authority. Deterministic routing scenarios (D01 #11781) prove contract
sufficiency without real models; bounded real dogfood cohorts (D02 #11782)
prove fresh agents can start, resume, review, refuse and hand off safely.

## Compatibility with the repository operating contract (`AGENTS.md`)

This bundle is deliberately constrained to remain compatible with the
repository's operating contract:

- **GitHub owns durable live transaction state.** The four truth planes keep
  live state (plane 3) out of stable bytes. The checked registry, labels
  vocabulary, navigation projection and directory hold **stable reviewed
  decisions** — the same authority class as `.spec/` bundles and generated
  contracts — never runtime frontier, task order, liveness, retries, leases or
  temporary plans. Current paths, SHAs, PRs, checks, candidates, models,
  assignments and support verdicts are not durable semantic input and never
  enter durable bytes.
- **No scheduler, no parallel lifecycle.** The directory is a navigation
  surface; it does not sequence work, own liveness or replace `$deliver-*`
  route selection. Issue-route skills and GitHub surfaces keep their existing
  meanings; I01 informs route preparation with reviewed roles, it does not
  invent a parallel lifecycle.
- **One writer, claim-local lanes.** Nothing in this architecture creates a
  writer registry or lease table; writer admission stays with the existing
  generic authorities (#4177/#3982/#3957).
- **Proof and currentness discipline.** Material candidate change re-runs
  affected proof; revision governance (T02R) is the train-level expression of
  the same rule. Missing, partial, stale, contradictory or instrument-failed
  evidence remains `not_proven`; optional or unavailable evidence never
  disappears or becomes pass.

Where the train's original vision and current repository policy remain in
tension, this bundle surfaces the tension as an open decision below instead of
silently picking a side.

## Open decisions

These are surfaced conflicts between the train vision and current repository
policy. They are recorded, not decided; each names its owning node.

1. **Checked registry bytes versus tracked-state prohibition.** `AGENTS.md`
   forbids tracked runtime topology/frontier/task state. The boundary this
   bundle states — stable reviewed decisions are checked; live transaction
   state is not — permits the registry, but the exact schema, file placement,
   review cadence and the rule for when a registry row stops being "stable
   decision" and becomes "runtime plan" are owned by C01/C02 (#11682/#11683)
   and must be re-ratified there before population.
2. **Bulk live metadata application versus minimal useful handoffs.**
   `AGENTS.md` posts GitHub updates only when information remains useful after
   the current context disappears. Applying generated navigation blocks and
   role labels across the full active denominator is bulk mutation with a
   maintenance tail. The scope, frequency, initial-cohort boundary and
   rollback policy for R05B (#11785) remain an open decision; this bundle
   fixes only the generated-block bound and the expected-old-state safety law.
3. **Generated navigation blocks inside issue bodies.** Issue prose is
   human-authored semantic content; a generated block inside it must remain
   identifiably generated, regenerate cleanly and never swallow authorial
   prose. The exact block format, placement and regeneration trigger are owned
   by C03 (#11684).
4. **Directory-informed entry versus skill-based routing.** `AGENTS.md` route
   selection is prose over named skills. I01 (#11777) must couple the checked
   directory to issue preparation without making the directory a routing
   authority that bypasses or duplicates the named `$deliver-*`/`$prepare-*`
   skills; the exact coupling seam is owned by I01.
5. **Proof-depth routing seam.** T08C (#11784) extends the existing risk and
   proof-depth router; whether that extension is configuration, adapter or new
   route class is owned by T08C and must not create a second router.

## Adoption, rollback, transfer and stop

**Adoption.** Later train nodes consume this bundle as semantic input: T01
(#11764) encodes the stable topology; C01/C02 consume the role and
adjudication laws; T02S (#11774) compiles per-node specs against these
boundaries. No node rebuilds them.

**Rollback.** Revert the single commit or remove the bundle; no runtime,
product, CI, support or GitHub state depends on it. Programme issue authority
(#11681–#11789) is unchanged by rollback.

**Transfer.** If the architecture is superseded, a successor bundle supersedes
this one by explicit link and T02R governs invalidation of derived artifacts.

**Stop.** This bundle stops before: stable-train implementation, role-registry
population, GitHub metadata changes, current-tree/live observation, actor
packet generation, model execution, merge, release or publication. If a
material decision listed under "Open decisions" is needed as a decision rather
than a boundary, stop and route it to its owning node — do not decide it here.

## Links

- Controlling issue: #11763; parent controller: #11681.
- Functional rail: #11682, #11683, #11684, #11685, #11686, #11687.
- Execution train: #11764, #11765, #11767, #11769, #11771, #11772, #11773,
  #11774, #11775, #11776, #11777, #11778, #11779, #11781, #11782, #11783,
  #11784, #11785.
- Method and spec authority: #3949, #3983, `docs/reference/SPEC_TEMPLATE.md`,
  `.claude/workflows/spec-builder.js` (current markdown fan-out only).
- Generic contracts: #10858, #10872, #10881, #4177, #3982, #3957, #3693,
  #10168, #3390, #1848, #4787, #4789, #10554, #11114.
- Bundle precedent: `.spec/10894-editor-host-reliability/` (PR #11811,
  commit `55b91651a`).
