# Context: #11764 — canonical stable issue_controller_train.v1 topology graph

This is a checked, declarative stable contract. It adds no readiness command,
current-tree probe, GitHub observation, packet instance, metadata mutation,
proof execution, support result, or scheduler. Those remain owned by the nodes
this graph declares.

## Problem

The issue-controller programme's execution topology lives in prose: the S00
bundle (`.spec/11763-issue-controller-architecture/`) records the node map as
markdown tables and ASCII sketches, the controller #11681 carries a dependency
graph in its body, and each leaf issue carries its own reference block. Every
later consumer — the independent validator (T02 #11765), revision governance
(T02R #11767), current-tree probes (T03 #11769), frontier (T04 #11771),
context (T05 #11772), live observation (T06 #11773), leaf specs (T02S #11774),
packets (T07 #11775), closeout (T08 #11776), routing (T08C #11784), entry
(I01 #11777 / I02 #11778), proof (P01 #11779), dogfood (D01 #11781 / D02
#11782) and the fan-in (P02 #11783) — would re-parse that prose and could
derive a different graph. #11764's acceptance forbids that: the final manifest
must use the exact child issue IDs linked from #11681, and prose-only edges
are invalid.

## Why this approach

A machine-readable, versioned, deterministic manifest is the stable contract
artifact the S00 bundle names (`issue_controller_train.v1`, §API-Shape). It is
compiled inside a `.spec` bundle exactly as the sibling precedents
(`.spec/11763-issue-controller-architecture/`, PR #12006; `.spec/10894-editor-host-reliability/`,
PR #11811) compiled their contracts: canonical bundle files, an embedded
structural checker with fail-closed negative controls, two-run determinism
proof, and an honest NOT_PROVEN boundary. The manifest is TOML/JSON-style
checked data following the machine-readable precedent
`.spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml`, using
JSON so the checker needs no external parser. No validator command, generated
repository artifact or CI surface is added: T02 owns the independent
validator, and T02R owns revision governance.

## Current state (honest, as of this bundle)

- The S00 bundle is the accepted durable architecture: `.spec/11763-issue-controller-architecture/`
  with its proposition map (25 nodes), ordering boundaries, five open decisions
  routed to owning leaves, and its own structural checker discipline.
- The execution-train leaves #11764–#11785 and the functional rail
  #11682–#11687 are open; their dependency statements live in their bodies and
  in #11681's dependency graph section.
- The repository has no executable issue-controller tooling of any kind; the
  only validators for `.spec` bundles are the embedded structural checkers in
  each bundle's checklist.
- This bundle lands the manifest as data plus its embedded checker. It does
  not prove the topology is semantically correct against every future issue
  edit — that is review's job here and T02R's job after.

## Authority and ownership

- Controlling issue: #11764 (T01). Parent controller: #11681.
- Durable architecture consumed: the S00 bundle at
  `.spec/11763-issue-controller-architecture/` (issue #11763) — its
  proposition map, ordering boundaries, truth-plane law, open decisions and
  checker discipline are semantic input; this manifest encodes them, it does
  not re-derive or widen them.
- Leaf issue bodies #11682–#11687 and #11765–#11785 are the per-node
  authority; every edge in the manifest records the statement it traces to
  (leaf body references, the S00 plan's rows/ordering, or #11681's dependency
  graph).
- Generic authorities consumed, never cloned: #10858 (typed dependency and
  evidence classes), #10872/#10881 (packet contracts T07 adapts into), #10554
  (train-mechanics extraction gate), #11114 (D-series evaluation vocabulary),
  #3983/#3949 (method), #4177/#3982/#3957 (writer admission), #3693/#10168
  (review/closeout), #3390/#1848/#4787/#4789 (CI route/result/fan-in).

## Durable laws consumed

The manifest encodes, as data plus checker law:

- **Nine authority planes**, non-transferable: stable train contract, semantic
  train revision, current-tree implementation state, offline readiness/frontier,
  exact-tree context, live collaboration/candidate state, exact-head
  proof/review closeout, live GitHub metadata state, and behavior/proof/
  support/external truth. No plane satisfies another; role registry landed
  does not mean labels applied; labels applied do not make a leaf ready;
  issue/PR closed does not prove implementation on the evaluated tree;
  candidate green does not prove review current; composed proof does not
  authorize metadata mutation or merge; dogfood does not create routing
  policy.
- **Fifteen train roles** — controller, specification, stable_contract,
  validator, current_tree_probe, offline_frontier, context_projection,
  live_observer, packet_adapter, implementation, proof, fan_in, integration,
  external_gate, dogfood — kept independent from #11682's issue-role product
  vocabulary. The manifest carries `train_role` only; it never emits GitHub
  issue roles.
- **Typed dependencies** with classes hard / evidence / optional / external
  per #10858, one writer slot / conflict key per node, parallel groups and
  stack relations, claim ceilings, spec dispositions, first falsifiers,
  control sets, proof obligations, review questions, generated/docs/changelog/
  receipt obligations, exits, rollback quartets, successors, identity fields
  and limitations for all 26 nodes.
- **Graph laws**: the S00/#11681 ordering — S00 → T01 → T02 → T02R →
  {T02S, T03} → T04 → {T05, T06, T08C} → T07 (+T08C) → T08; C01 → (C02 || C03)
  → C04 → C05 → C06 with R05B as the separate authorization-gated privileged
  stage; (C04, T07, T08) → I01 → I02; functional rail + entry + closeout →
  P01 → D01 → D02 → P02 → #11681 closeout. The checker freezes these edges
  and their classes; the manifest cannot silently weaken one.
- **Evidence semantics**: missing, partial, stale, contradictory or
  instrument-failed evidence is `not_proven`, never pass; optional and
  unavailable rows remain explicit and never disappear.
- **Stable-byte hygiene**: no current SHA, PR, check, model, writer, landing
  or live metadata state enters the manifest; the external authorization gate
  is represented only as the `#EXPLICIT-AUTHORIZATION` authority with its
  never-inferred law; likely source paths appear only later when T05 resolves
  them on one exact tree.

## Encoding decisions and traceability

- Node IDs follow the S00 bundle's proposition map (C01–C06 for the
  functional rail #11682–#11687, R05B for #11785); #11764's law vocabulary
  R01–R06/R05A is carried as aliases so both vocabularies resolve to the same
  unique node.
- Edges are encoded exactly where a leaf body states them. Where the #11681
  controller graph and a leaf body differ, the difference is recorded in edge
  provenance and node limitations rather than silently resolved: T04's T02S
  edge is evidence-class per the controller graph while the T04 body is
  silent; T06's body lists no S00/T02 direct input; R05B's participation in
  P01/P02 is optional because the stage is authorization-gated and selected.
- #11779's body cites live observation as "#11787", which is an unrelated
  closed issue; the drift-observer edge is encoded against C06 #11687
  following #11681's exact child links.
- #11681's prose sketch chains R05B before R06; per the S00 bundle's
  programme shape and #11687's own references, the hard chain is C05 → C06
  with R05B a sibling privileged stage, and that is what the manifest encodes.

## Compatibility with the repository operating contract (`AGENTS.md`)

- The manifest holds stable reviewed topology only — the same authority class
  as `.spec` bundles and generated contracts. Runtime topology, frontier,
  task order, liveness, retries and temporary plans remain runtime-local and
  never enter durable bytes.
- The manifest is a navigation and contract surface: it sequences nothing,
  owns no liveness, and replaces no `$deliver-*` route selection. It adds
  no readiness command, no scheduler, and no parallel lifecycle.
- One writer built this candidate; no writer registry or lease table is
  created. Writer admission stays with #4177/#3982/#3957.
- The train manifest's node set is deliberately not a global implementation
  DAG: it is programme-local to the issue-controllers home, and the directory
  (C04) remains the navigation surface for other programmes.

## Open decisions respected, not decided

The five open decisions compiled by the S00 bundle are recorded in the
manifest with their owning nodes and are not decided here: OD1 (checked
registry bytes boundary → C02 #11683, with C01 #11682), OD2 (bulk live
metadata application scope → R05B #11785), OD3 (generated navigation-block
format → C03 #11684), OD4 (directory/skill coupling seam → I01 #11777), OD5
(proof-depth routing seam → T08C #11784). The checker requires exactly these
five decisions with exactly these owners; a manifest revision that decided
one here would have to reclassify it, which is T02R's authority.

## Adoption, rollback, transfer and stop

**Adoption.** T02 (#11765) validates this manifest and derives the sole
checked graph projection from it; T02R (#11767) governs its semantic
revisions; T02S (#11774) compiles per-node specs from it; later nodes consume
the topology without re-parsing controller prose.

**Rollback.** Revert the single commit or remove this bundle directory; no
runtime, product, CI, support or GitHub state depends on it. The S00 bundle
and the issue bodies remain authoritative.

**Transfer.** A successor manifest version supersedes this one only through a
T02R-classified revision with an exact successor recorded; stale derived
artifacts are re-derived, never patched valid.

**Stop.** Stop before validator commands, current-tree probes, frontier,
source-context resolution, live observation, packet rendering, GitHub
metadata work, exact-head checkers, dogfood, scheduling, support claims,
release or publication. If an open decision OD1–OD5 is needed as a decision
rather than a boundary, stop and route it to its owning node.

## Links

- Controlling issue: #11764 (T01); parent controller: #11681.
- Durable architecture: S00 #11763, `.spec/11763-issue-controller-architecture/`.
- Functional rail: C01 #11682, C02 #11683, C03 #11684, C04 #11685, C05 #11686,
  C06 #11687; privileged stage R05B #11785.
- Execution train: T02 #11765, T02R #11767, T03 #11769, T04 #11771, T05 #11772,
  T06 #11773, T02S #11774, T07 #11775, T08 #11776, T08C #11784.
- Integration/proof/closeout: I01 #11777, I02 #11778, P01 #11779, D01 #11781,
  D02 #11782, P02 #11783.
- Generic authorities: #10858, #10872, #10881, #10554, #11114, #3983, #3949,
  #4177, #3982, #3957, #3693, #10168, #3390, #1848, #4787, #4789.
- Bundle precedents: `.spec/11763-issue-controller-architecture/` (PR #12006),
  `.spec/10894-editor-host-reliability/` (PR #11811),
  `.spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml`.
