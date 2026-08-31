# Acceptance: #10976 — DAP reliability-programme linked contracts and builder views

## §Behavior

This bundle changes no runtime, editor, debugger-peer, VS Code, CI-routing,
support, or release behavior. Its only "behavior" is the deterministic
structural contract of the bundle itself:

1. `reliability.manifest.json` parses as strict `dap_reliability_contracts.v1`
   with exact key sets at every level and closed vocabularies.
2. Every concrete decision enumerated by #10976 resolves to exactly one node
   with exactly one disposition.
3. Compiled family invariants exist, are referenced by at least one node each,
   and never leak command spellings, live state, or generated scope.

Typed non-pass outcomes: any failed check below is `NOT_PROVEN` for this
bundle; there is no partial pass.

## §Hazards

- Encoding a controller (#8591/#7278/position epics) as an implementation leaf
  it does not own — rejected structurally by role law.
- Creating a second lifecycle/broker/capability/evidence authority — prevented:
  invariants are projections of existing authorities; consumer/validation
  layers stay owned by #10558/#10982.
- Copying stale issue prose contradicted by later synthesis — mitigated: no
  titles/bodies are stored; invariant statements are recombinations of current
  accepted summaries; stale dispositions are corrected through revision
  governance, never silently.
- Embedding live GitHub/tree state (open/closed, labels, PR numbers, checks,
  SHAs) — rejected byte- and value-wise by the live-state token control.
- Nondeterministic ordering / second-run diff — rejected by order laws plus a
  two-run byte-identical digest proof including an order-invariance control.

## §Contracts

- Identity/currentness split: stable semantic identity lives here; tree
  currentness deliberately does not (probe layer #10997 owns it).
- Role law: `controller` and `fan_in` nodes carry authority summaries and are
  never selectable builder leaves.
- Disposition exclusivity: exactly one disposition per node, values closed to
  the seven-value enum of #10976.
- Referential closure: every `covered_invariants` entry resolves inside a
  compiled family view (a decision may cite invariants across views); every compiled invariant is covered by at least one node; every
  hard dependency is an integer decision reference distinct from itself.
- Ownership boundary: spec dispositions name this bundle as owner only for the
  durable statement level; per-issue behavioral truth remains with each owning
  issue until #10558 compiles node-level DAG semantics.
- Consumer vocabulary stays closed (`10558`, `10982`, `7278`, `4346`, `6056`);
  nothing else may quietly gain consumption claims (negative control on
  connective prose widening).

## §API-Shape

Stable surface = the four files of
`.spec/10976-dap-reliability-contracts/`. The machine-readable artifact schema:

```text
dap_reliability_contracts.v1
  programme{...}                  closed key set, issue integers only
  role_vocabulary[7]              closed train roles
  disposition_vocabulary[7]       #10976 enum, fixed order
  consumer_vocabulary[5]          closed consumer issues
  index_law[6]                    normative checker-enforced statements
  family_views[7]{view_id,title,compiled_invariants[],first_falsifier,...}
  contract_nodes[107]{stable_semantic_id,family_view,issue,train_slot,role,
                      semantic_authority,disposition,disposition_basis,
                      hard_dependency_issues,covered_invariants,consumers}
  scope_corrections[3]            evidence-backed citation-range findings
  limitations[3]                  honest NOT_PROVEN cells
```

Future versions must classify semantic revision against #10976 and re-derive
consumers; unknown keys fail closed.

## §Test-Grid

The embedded checker in `checklist.md` must reject every falsifier mutation
below. Order is fixed; each names its kind and expected verdict.

| ID | Kind | Mutation | Verdict |
|---|---|---|---|
| T01 | structure | delete required node key `disposition_basis` from one node | reject |
| T02 | structure | add unknown top-level key `next_ready_slot` | reject |
| T03 | identity | duplicate one `stable_semantic_id` onto another node | reject |
| T04 | role-law | reclassify controller `#8591` to `evidence_leaf` while its family keeps builder leaves | reject |
| T05 | second-authority | duplicate invariant id `INV-LC-01` into another view | reject |
| T06 | orphan-authority | remove every node covering `INV-EV-03` | reject |
| T07 | omission | set `first_falsifier` of `FAM-BREAKPOINT` empty | reject |
| T08 | omission | give a `SPEC_COMPILED` node zero `covered_invariants` | reject |
| T09 | live-state | embed a 20-hex-char token in `semantic_authority` | reject |
| T10 | prose-scope | make `consumers` contain an out-of-vocabulary value | reject |
| T11 | command-spelling | put `cargo xtask check` text into a `disposition_basis` | reject |
| T12 | determinism | rotate `contract_nodes` array; canonical semantic digest must stay identical while file-order law rejects unsorted storage | digest-stable + reject |
| T13 | population | drop the final contract node from the manifest | reject |
| T14 | scope-law | changed/untracked path outside the four-file bundle set (or a required bundle path missing from it) | reject |
| T15 | referential-closure | set one `hard_dependency_issues` entry to an integer absent from the 107 compiled decisions | reject |

## Claim boundary

One compilation claim: accepted decisions -> linked durable statements +
per-decision dispositions + deterministic index. NOT claimed: product truth,
currentness, executable conformance, readiness, writer/conflict semantics,
review or support outcomes. Review-forward fields exposed here intentionally
feed #3986/#3693 without creating review state.

Rollback meaning: revert of this bundle restores the pre-compilation prose-only
surface; consumers (#10558) are not yet landed, so blast radius is exactly this
directory plus the regenerated non-Rust inventory snapshot.

Stop / RETURN_TO_ISSUE conditions: a reviewer shows a compiled invariant
contradicts a later accepted synthesis (fix via semantic revision, not patch);
the citation-range corrections turn out wrong (re-derive from tracker);
maintainers prefer a different index location/schema (transfer wholesale).

## Non-goals

No xtask validator command, schemas/ registry entry, Gherkin/BDD acceptance
data, feature_status regeneration trigger, generated receipts, CI gates, review
submission, release/promotion actions, or per-node current-tree probes.
