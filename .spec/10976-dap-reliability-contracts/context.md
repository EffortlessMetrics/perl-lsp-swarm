# Context: #10976 — compile the DAP reliability programme into linked contracts, concise builder views, and one deterministic stable index

This bundle is a checked, declarative compilation artifact (train item C00 of the
DAP programme controller #9415, feeding stable train consumer #10558). It adds no
readiness command, current-tree probe, GitHub observation, product behavior,
CI routing, support claim, release action, validator tooling, scheduler, or
review state. Those remain owned by their existing authorities.

## One-PR result

One versioned `dap_reliability_contracts.v1` stable index
(`reliability.manifest.json`) plus the concise builder/reviewer views in this
bundle, giving every enumerated DAP reliability-programme decision:

```text
stable semantic ID  = DAPREL-<issue>
family view         = one of seven compiled durability areas
role                = closed train-role vocabulary reused from #10558 planning
one disposition     = exactly one of the #10976 disposition enum
compiled invariants = normative one-line contracts per family
hard dependencies   = distinct integer issue references resolving to the accepted train order
consumers           = closed issue-number vocabulary (#10558 family)
```

## Why this approach

The programme's lasting decisions lived only as prose spread across the #9415
controller body, controlling-issue bodies, and reconciliation comments; no
enumerated decision had an in-tree durable owner before this bundle (verified by
a mechanical scan of `.spec/`, `schemas/`, and `docs/specs` on the candidate
base — zero hits for every referenced number). Later consumers (#10558 stable
manifest population, #10982 static validation, #10997 probes) would otherwise
re-parse thousands of lines of prose and could derive different graphs.

The manifest follows the accepted sibling precedent
`.spec/11764-controller-train-graph/train.manifest.json`:
machine-checked data compiled inside a `.spec` bundle, with the strict checker
embedded in `checklist.md`, two-run determinism proof, fail-closed negative
controls, and honest boundaries. JSON was chosen so the embedded checker needs
no external parser. No xtask command, schema registry entry, generated receipt,
or CI surface is added: #10982 owns the independent static validator and #10558
owns the `dap_train.v1` DAG/consumer semantics; duplicating either here would
create a second authority.

## Authorities consumed, never re-derived

- Programme decomposition, slot mapping, dependency order, optional-breadth
  floors: #9415 (`main`-current reconciliation at candidate time) and #4754.
- Disposition vocabulary and spec-disposition rules: #10976.
- Stable-node role vocabulary: #10558 planning section (reused, not copied into
  a new system).
- Per-decision identity and settled semantics: each owning issue listed under
  `contract_nodes`. This projection records their authority summaries; it does
  not re-adjudicate outcomes or currentness.
- Bundle/checker discipline precedents: `.spec/11763-issue-controller-architecture/`,
  `.spec/11764-controller-train-graph/`, `.spec/10894-editor-host-reliability/`.
- Method: #3983; repository protocol: #3949; linked spec graph registration:
  #3586.

## Derivation method (deterministic, reviewable)

1. Every concrete issue number enumerated by the seven durable-area families of
   #10976 was expanded (including ranges `#9527–#9538` and `#7337–#7348`) into
   the deduplicated 107-number set.
2. Each number was joined against the repository tracker catalog on the
   candidate base. Five cited numbers do not resolve to issues; they carry
   `NOT_PROVEN` nodes and a `scope_corrections` record rather than invented
   authority.
3. Titles were used only as classification evidence for roles; titles themselves
   are deliberately not stored here (they are mutable surface state).
4. Roles use the closed vocabulary with the rule that issue-family controllers
   named by #9415 rule 1 / #10976 (`#8591` mutation subtrain, position epics
   `#4973`/`#8687`/`#8707`, release closeout `#7278`) cannot be builder leaves;
   fan-ins `#6684`/`#6688` aggregate evidence instead of implementing.
5. Family assignments follow the semantics of each decision (for example
   `#7339` sits inside the cited convergence range but is the A05 native
   capability-floor decision, so it compiles under capability truth);
   range-membership anomalies that could not be placed honestly are explicit
   `RETURN_TO_ISSUE` rows.
6. Dispositions: `SPEC_COMPILED` where this bundle's family invariants own the
   durable statement (98 nodes), `RETURN_TO_ISSUE` where the cited member falls
   outside its swept family (4 nodes), `NOT_PROVEN` where reference provenance
   is unresolvable (5 nodes). No `EXISTING_CONTRACT_SUFFICIENT` row exists
   because the owner scan found none — that emptiness is itself recorded
   evidence, not an omission.
7. Invariant statements are one-line recombinations of the accepted decisions'
   public summaries above; they introduce no new semantics and intentionally do
   not quote issue bodies.

## Compiled summary

Seven family views, 42 compiled invariants, 107 contract nodes. Downstream
current-tree truth stays with #10997 probes; graph/readiness/consumer semantics
stay with #10558; executable drift validation stays with #10982.

## Prerequisites

None hard for this bundle: it is data plus embedded checks. Its consumers
(#10558, #10982, #3986, #3693) read the landed file when they run.

## What this PR establishes and cannot establish

Establishes: one deterministic, schema-stamped, hygiene-checked linked-contract
index; per-decision dispositions for everything #10976 enumerates; four
evidence-backed corrections to #10976's own citation ranges; concise
builder/reviewer views exposing falsifiers, ceilings, rollback, and stop
conditions before any builder consumes these nodes.

Cannot establish: currentness or completion of any node (probe layer #10997),
executable conformance of files against these invariants (#10982), stable-DAG
writer/conflict/readiness semantics (#10558), product behavior truth (#9415
proof trains), release or support claims (#7278 consumers).

## Rollback / transfer / stop

Rollback = revert the single commit adding this directory and the regenerated
inventory snapshot; nothing else consumes it until #10558 lands its consumer.
Transfer = a later lane may rewrite projections wholesale through one semantic
revision recorded against #10976; silent per-field edits are forbidden by the
checker laws. Stop/return conditions live in `acceptance.md` §Claim boundary.
