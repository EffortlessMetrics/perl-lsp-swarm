# Context: #11625 — canonical stable module_train.v1 implementation and proof graph

This is a checked, declarative stable contract. It adds no xtask validation
command, current-tree probe, frontier computation, live observation, packet
instance, scheduler, product behavior change, support claim or GitHub
mutation. Those remain owned by the nodes this graph declares.

## Problem

The module programme's execution topology lives in prose: the controlling
issue #11625 carries the canonical graph sections (E00, M00S, M01–M10, L09,
provider proof, P11, claims and documentation); two #11625 comments record the
C-series successor graph and the C01 entry-gate clarification; the programme
controllers #8133/#4240, the evidence controller #8479, the functional
controllers #8566/#8701/#7421/#9270, the claim composition controllers
#7430/#4245 and every leaf body carry their own reference blocks and
train-position statements. Every later consumer — the C02 current-tree
frontier (#11626), the C03 live observer (#11627), the durable spec source
(#10592), and every module implementation, cutover, retirement, proof and
claim leaf — would re-parse that prose and could derive a different graph.
#11625's acceptance forbids that: controllers must never be selectable as
implementation, dependency types must stay distinct, safe parallelism must
remain visible, cross-programme nodes must be imported rather than copied,
and the stable bytes must stay deterministic and free of live state.

## Why this approach

A machine-readable, versioned, deterministic manifest is the stable contract
artifact the controlling issue names (`module_train.v1`). It is compiled
inside a `.spec` bundle exactly as the two merged shape precedents compiled
theirs: `.spec/11764-controller-train-graph/` (issue #11764, the controller
train's stable graph) and `.spec/10918-emacs-train-graph/` (issue #10918, the
newest four-file train-graph bundle): canonical bundle files, an embedded
PowerShell 7 structural checker with fail-closed negative controls, a two-run
determinism proof, and an honest `not_proven` boundary. JSON is used so the
checker needs no external parser, following the same precedents.

The controlling issue also names offline xtask operations
(`cargo xtask module-train check`, `graph`, `list`, `explain-static`). Those
are executable repository tooling and are deliberately **not** built here:
this bundle lands the topology as checked data plus its embedded checker, and
records the absent xtask validator as `not_proven` rather than papering over
it, exactly as both precedents deferred their validators to later tooling
claims.

## Current state (honest, as of this bundle)

- Current `main` carries no `module_train` or module-train artifact of any
  kind (verified against origin/main before authoring; nothing in `xtask/`,
  `crates/` or `.spec/` names this train). This bundle is the first artifact.
- The E00 family (#10977, #10981, #10986, #10995, #10999) and #10592 are all
  open and unlanded: no production module-contract schema, row family or
  durable spec packet exists yet. Per the C01 entry-gate clarification, this
  manifest therefore binds case and work-packet references by issue identity
  only and keeps the relation **structurally pending**; it invents no local
  case identifiers and treats no issue number as an executable evidence
  identity.
- The goal owner's census (batch 2) verified #11625 as a STILL_VALID,
  independently landable head, which is the current authority for beginning
  this bundle without waiting for the E00 denominator to land; the older
  `status:blocked` label is metadata, not disposition.
- The two shape precedents are merged; the repository has no module train
  tooling of any kind, and the only validators for `.spec` bundles are the
  embedded structural checkers in each bundle's checklist.
- The manifest covers exactly 52 nodes: nine controllers, fourteen
  implementation, nine cutover, one retirement, five evidence, seven proof,
  one fan_in, two claim and four spec nodes.
- This bundle does not prove the topology is the semantically correct reading
  of every leaf body — that is this review's job here, and the #11625
  revision route's job after.

## Authority and ownership

- Controlling issue: #11625 (C01). Parent programme: #8133. Functional
  controller: #4240. Evidence controller: #8479.
- Canonical semantic input: the #11625 body's canonical graph sections, the
  C-series successor comment, and the C01 entry-gate clarification; every
  manifest edge records the statement it traces to through a closed
  provenance vocabulary.
- Leaf issue bodies are the per-node authority; where a leaf header declares
  a dependency (`Depends on`, hard dependencies, evidence groups, required
  children), the manifest preserves it with leaf-body provenance rather than
  forcing the canonical summary chain.
- Generic authorities consumed, never cloned: #10858 (typed dependency and
  claim-profile contracts), #10554 (shared checked-train mechanics gate),
  #11114 (fresh and lower-cost agent evaluation), #3982/#3983 (preparation
  and method), #3985/#3989 (proof routing and closeout), and the
  cross-programme owners recorded in the manifest's import table (#8131,
  #7582, #7621, #7622, #4851, #7419, #7420, #6736, #7057, #7943, #8112,
  #8199, #8518, #4239, #7584, #8617, #6720, #7249, #8761, #9621).

## Durable laws consumed

The manifest encodes, as data plus checker law:

- **Eight authority planes** in fixed order: durable programme decisions, the
  executable evidence denominator, the durable spec source, this stable
  topology, the C02 current-tree plane, the C03 live plane, behavior
  evidence and claim truth, and the external/support stages. No plane
  satisfies another.
- **Ten train roles** from the #11625 stable node contract — controller,
  spec, evidence, implementation, cutover, retirement, proof, fan_in, claim,
  external_gate — with the controller rejection list (#8133 #4240 #8479
  #8566 #8701 #7421 #9270 #7430 #4245) frozen as exactly the nine
  non-buildable controller nodes.
- **Typed dependencies** hard / evidence / optional / external per #10858,
  one edge per target, closed provenance vocabulary, writer classes A
  (evidence/spec/train/generated authorities), B (request/directive/resolver/
  source contracts), C (live provider-consumer cutovers), D (exact-process
  proof and fan-in), one conflict key per node, claim ceilings, spec
  dispositions, first falsifiers, control sets, proof obligations, exits,
  rollback quartets, successors, identity fields and limitations for all 52
  nodes.
- **Graph laws**: the canonical E00 denominator; M00S consuming the complete
  case identity; the M01→M02 admission chain with imported path mechanics;
  the M03–M06 parser-fact hierarchy with the leaf-declared precision edges;
  the M07 resolver chain with configuration generations and folder ownership;
  M08–M10 geometry and public boundary; the L09 service and consumer cutover
  family with terminal-disposition retirement; provider proof distinct from
  cutover; the P11 substrate/cell/fan-in family; and the claims and
  documentation paths. The checker freezes 177 node law edges with
  their exact classes, 20 cross-programme edges through the import
  table's relation law, and @FORB_COUNT@ forbidden edges; the
  manifest cannot silently weaken or add one.
- **Claim profiles**: the six reviewed profile denominators with frozen
  membership, and the full-closeout superset law that a core resolution pass
  can never hide a non-admitted or not-proven semantic-edit profile. The
  stable graph names membership only; current receipts and status evaluate
  profiles elsewhere.
- **Case and work-packet bindings stay structurally pending** until the E00
  family materializes stable identities; the checker rejects both invented
  local case identifiers and premature binding promotion.
- **Evidence semantics**: missing, partial, stale, contradictory or
  instrument-failed evidence is `not_proven`, never pass; optional and
  unavailable rows remain explicit; metadata-only movement invalidates
  nothing; issue numbers are reviewed propositions, never executable
  evidence identities.
- **Stable-byte hygiene**: no current SHA, branch, pull, check, review,
  model, writer, landing or live metadata state enters the manifest.

## Encoding decisions and traceability

- Node IDs follow the #11625 canonical section vocabulary (E00A–E00E, M00S,
  M01–M10, L09A–L09G, P11A–P11F, C01–C03) plus stable controller and proof
  identifiers; issue numbers are the identity, titles are fingerprinted, and
  pull requests never appear as nodes.
- The bundle path follows the merged four-file train-graph precedent
  (`.spec/11625-module-train-graph/`); the controlling issue's
  `-module-train-stable` three-file naming is satisfied by this equivalent
  shape under current conventions.
- Where a leaf header names an authority without a dependency statement, the
  manifest records it as a consumed authority, not an edge; where a leaf
  header declares a dependency, the edge carries leaf-body provenance
  (e.g. #8521's path-containment hard dependency, #10575's contract and
  specialized-effect dependencies, #8744's source-identity hard dependency,
  #8780/#8810's dependency-direction imports, the L09 headers' hard
  dependency lists, and the P11 headers' product-prerequisite and
  required-children groups).
- Controller-family dependencies named by leaf headers (#8566, #8701,
  #7421) are satisfied transitively through their decomposed leaf edges;
  controllers never gate builders directly.
- The E00 pack-consumption edges among E00D/E00E and the evidence groups
  feeding the L09 and P11 families encode the leaf-declared evidence
  denominators as evidence-class edges.
- The two named provider proof nodes (#1744 completion no-lib, #4243
  navigation shadow promotion) stand for the focused/provider/process
  evidence the claim composition controllers consume; further exact proof
  owners stay with their leaf bodies.
- #9621 is recorded as an imported consumer with its home train
  (`editor_intelligence`) and never as a node; broad editor-intelligence
  closure is never a hard dependency for unrelated module work.

## Shared-mechanics disposition (#10554)

#10554 owns a private shared-mechanics seam with an explicit start gate and
is **not** a prerequisite for a first implementation. Verified against
current `main`: no shared checked-train library exists there; the two landed
train artifacts are `.spec` data bundles with embedded checkers, not
duplicated xtask algorithms, so no condition of the start gate (two landed
train implementations duplicating at least three concrete algorithms, a
reviewed generic module awaiting a second consumer, or a train blocked by an
exact common-mechanics defect) is satisfied. This bundle therefore: reuses
nothing (there is nothing to reuse), creates no second concrete copy of
shared DAG, edge or serializer algorithms (it lands data plus a bundle-local
checker, no cargo code), records the overlap in this section and in the
manifest's import table, and routes the future extraction decision to #10554
as OD1 rather than deciding it here. The same facts answer #11625's
shared-mechanics boundary for this PR: the reuse-vs-extract gate decision
this claim carries is "no extraction begins here; the gate remains
unsatisfied and open at #10554".

## Compatibility with the repository operating contract (`AGENTS.md`)

- The manifest holds stable reviewed topology only — the same authority class
  as `.spec` bundles and generated contracts. Runtime topology, frontier,
  task order, liveness, retries and temporary plans remain runtime-local and
  never enter durable bytes.
- The manifest is a navigation and contract surface: it sequences nothing,
  owns no liveness, and replaces no route selection. It adds no readiness
  command, no scheduler and no parallel lifecycle.
- One writer built this candidate; no writer registry or lease table is
  created. Writer admission stays with the generic authorities.
- The node set is programme-local to the module train; cross-programme
  owners keep their homes and are imported through exact typed edges only.

## Open decisions respected, not decided

Five open decisions are recorded with their owners and are not decided here:
OD1 (shared checked-train mechanics extraction → #10554), OD2 (stable
case-family and owner identity binding → #8479), OD3 (broader
editor-intelligence completion and provider node admission → #9621), OD4
(selection of a bounded module leaf for fresh or lower-cost evaluation →
#11114), OD5 (which environment and configuration generation is accepted for
effective-root composition → #10575). The checker requires exactly these
five decisions with exactly these owners; a manifest revision that decided
one here would have to reclassify it through the #11625 revision route.

## Adoption, rollback, transfer and stop

**Adoption.** C02 (#11626) consumes this topology to derive current-tree
status, the safe offline frontier and bounded agent packets; C03 (#11627)
joins it to live collaboration state; #10592 compiles durable packets
alongside it; every module leaf consumes its node contract as the stable
execution-critical summary without re-parsing the issue corpus.

**Rollback.** Revert the single commit or remove this bundle directory; no
runtime, product, CI, support or GitHub state depends on it. The issue bodies
remain authoritative.

**Transfer.** A successor manifest version supersedes this one only through
a classified #11625 revision with an exact successor recorded; stale derived
artifacts are re-derived, never patched valid.

**Stop.** Stop before validator commands, current-tree probes, frontier,
live observation, packet rendering, GitHub metadata work, product
implementation, scheduling, support claims, release or publication. If an
open decision OD1–OD5 is needed as a decision rather than a boundary, stop
and route it to its owning issue.

## Links

- Controlling issue: #11625 (C01); parents: #8133 / #4240; evidence
  controller: #8479.
- Successors: #11626 (C02), #11627 (C03); generic evaluation: #11114.
- Shape precedents: `.spec/11764-controller-train-graph/` (#11764),
  `.spec/10918-emacs-train-graph/` (#10918).
- Controllers encoded: #8133, #4240, #8479, #8566, #8701, #7421, #9270,
  #7430, #4245.
- Leaf families encoded: #10977–#10999 (E00), #10592 (M00S), #8497, #8521,
  #8542, #10568–#10572, #8634, #8659, #10573, #10575, #10578, #8170,
  #8744, #8780, #8810, #11008–#11026 (L09), #1744, #4243, #11619–#11624
  (P11), #7460, #10599.
- Generic and cross-programme authorities: #10858, #10554, #3982, #3983,
  #3985, #3989, #9621, #11114, #8131, #7582, #7621, #7622, #4851, #7419,
  #7420, #6736, #7057, #7943, #8112, #8199, #8518, #4239, #7584, #8617,
  #6720, #7249, #8761.
