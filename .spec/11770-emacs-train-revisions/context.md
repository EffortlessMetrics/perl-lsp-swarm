# Context: #11770 — Emacs semantic train revision governance ledger

## Problem

The stable `emacs_train.v1` graph landed by E01 (#10918,
`.spec/10918-emacs-train-graph/`) froze a 55-node topology, but a static
manifest cannot explain how an accepted train contract changes after
publication. The Emacs train already underwent ten material movements —
authority insertions, six decompositions, an external-train incorporation and
a stage retarget — and without a revision contract, agents must guess which
old propositions, specs, contexts, packets, candidates, proofs, receipts,
registry rows and docs projections became stale, and which unique work had to
be preserved through each split. Cheap agents will either continue against
obsolete packets or invalidate unrelated work whenever the graph moves.

## Why this approach

The governing issue (#11770) places E01R after the stable manifest and before
current-tree projection treats one revision as current. The reference
revision contract (LSP4IJ #11375) supplies the reviewed vocabulary for
revision classes, change-proposal fields, impact laws and collaboration
handoffs; this bundle composes with that vocabulary instead of duplicating
its mechanics. Following the merged bundle precedents (T01
`.spec/11764-controller-train-graph/`, E01 `.spec/10918-emacs-train-graph/`,
C01 `.spec/11625-module-train-graph/`, S00
`.spec/11763-issue-controller-architecture/`), the contract lands as checked
data plus an embedded fail-closed checker — the same cargo-light bundle
discipline E01 used when its own xtask operations were still unbuilt
tooling.

The deliverable is a versioned append-only revision ledger
(`emacs_train_revision.v1`, `revisions.ledger.json`) that records the ten
frozen material movements with identity preservation and affected-node
invalidation semantics, cross-validated against the consumed manifest as
data. The E01 bytes stay immutable historical evidence; this is a sibling
bundle, not an edit of the merged E01 bundle, because the manifest's own
revision-governance block names E01R as the invalidation owner and its
transfer law requires supersession through an E01R-classified revision —
which this bundle now provides the schema for.

## Current state (honest, as of this bundle)

- E01 is merged (`emacs_train.v1`, 55 nodes, 124 typed edges); its closeout
  routed graph currentness to E01R.
- The ten material movements listed in #11770 have already happened on the
  issue graph; the initial ledger records them as immutable history, exactly
  as the manifest now encodes their outcomes.
- No executable Emacs train validator or revision tooling exists in the
  repository; the offline diff, change-check and impact operations named by
  #11770 remain a separate tooling claim and are recorded as `not_proven`
  here.
- The manifest's `supersessions` list is empty; the `supersede` kind is
  present in the ledger vocabulary for future manifest versions and drains
  every cell and work item by law.

## Authority and ownership

| Plane | Authority | This bundle's stance |
|---|---|---|
| Durable architecture | #11716 / `.spec/11716-emacs-support-architecture/` | consumed; no architecture decision is made here |
| Stable topology | `emacs_train.v1` (E01 #10918) | consumed as data; cross-validated, never rewritten |
| Semantic revision governance | #11770 (E01R) — this bundle | owned: revision kinds, ledger laws, identity preservation, invalidation semantics |
| Reference revision vocabulary | LSP4IJ #11375 | composed with, never duplicated |
| Generic checked-train mechanics gate | #10554 | respected (OD1): no generic framework is extracted here |
| Exact-tree state / live overlay | #10923 / #10930 | untouched; adoption-rule confirmation stays with #10930 |
| Spec method | #3983 and current `.spec` tooling | bundle shape follows `SPEC_TEMPLATE.md` plus the four-file precedent |

This bundle does not decide architecture, edit issue bodies automatically,
inspect or mutate GitHub, run host proof, choose support state, or rewrite
the manifest from live facts.

## Durable laws consumed

From #11770 (which itself composes #11375):

1. A changed one-PR proposition invalidates its old builder/reviewer packet
   and any candidate claiming that proposition.
2. A node split maps every old acceptance cell and unique work item to
   exactly one new owner or explicit retirement.
3. A new dependency blocks readiness without discarding unique existing
   work.
4. Metadata-only changes cannot churn specs, contexts, packets or evidence.
5. Subject/stage/claim-ceiling movement invalidates every stronger receipt
   or presentation projection that depended on the former ceiling.
6. Live PR/review/check collaboration actions are emitted as required
   actions; this offline ledger performs none.
7. Historical manifests and change packets remain immutable evidence of why
   old graphs existed.

From the E01 manifest: the `revision_governance` block (revision never
rewrites the manifest to pass, never mutates GitHub, metadata-only movement
invalidates nothing) and the `existing_candidate_adoption` rule for FIXT.

## Encoding decisions and traceability

- **Sibling bundle, not a `revisions/` subdirectory of E01.** E01's scope
  boundary froze exactly its four files, and its transfer law routes
  supersession through an E01R-classified revision. A sibling
  `.spec/11770-emacs-train-revisions/` keeps merged bytes immutable and
  gives this claim its own rollback unit, matching the one-bundle-per-issue
  convention (10918, 11625, 11763, 11764).
- **Structural kinds plus semantic classes.** Each entry carries one
  structural `revision_kind` (decompose, insert, incorporate, retarget,
  supersede) — the Emacs-local operation vocabulary — and one
  `semantic_class` from the closed 23-value shared vocabulary of #11770 and
  #11375. Kinds say what happened to the graph; classes say what it means
  for obligations.
- **Ledger references resolve against the manifest.** Node ids, issue
  numbers and external authority ids are validated against
  `train.manifest.json` as data (the ledger-vs-manifest cross-validation);
  an entry can never reference a node the accepted graph does not contain.
- **Decomposition wiring is derived, not trusted.** A decompose entry's
  successor set must equal the subject's hard dependency children minus its
  declared retained prerequisites, and every child must reach the parent
  fan-in through its manifest successor set — so a revision can neither
  silently drop a node nor invent a phantom child, and the fan-in
  denominator path survives every split.
- **Cells and work drain.** Every acceptance cell maps to exactly one
  declared owner or an explicit retirement; every unique work item is
  preserved to a declared owner or explicitly retired. Nothing disappears
  during decomposition, supersession or transfer.
- **Append-only by law.** Sequences are contiguous from one, entry
  identifiers match their sequence, the initial coverage is frozen to the
  ten movements of #11770, and the ledger's own rollback text forbids
  editing entries. History is immutable evidence.
- **No automatic execution.** Every entry carries
   `automatic_execution: "forbidden"`; required synchronization is a typed,
   bounded list naming exact controllers — never a free-form
   "update related work" instruction.

## Compatibility with the repository operating contract (`AGENTS.md`)

The bundle is data plus an embedded checker: no Rust, no generated
artifacts, no CI change, no GitHub state. Proof is fail-closed negative
controls plus two-run byte-identical determinism, matching the repository's
checked-contract discipline. The checker code is held to production hygiene
within its PowerShell surface (explicit failure modes, no swallowed errors).
Historical evidence stays reproducible; the ledger never rewrites the
manifest to pass.

## Open decisions respected, not decided

- **OD1 (#10554):** shared checked-train mechanical layer reuse stays gated;
  this bundle builds no generic framework.
- **OD2 (#10930):** whether the FIXT candidate is currently live stays with
  the live overlay plane; the ledger records only the canonical adoption
  rule.
- **OD3 (#11744):** exact subject pin selection binds at subject
  materialization, not here.
- Unbuilt tooling: the xtask diff/change-check/impact operations named by
  #11770 remain a separate claim; this bundle records them as `not_proven`.

## Adoption, rollback, transfer and stop

- **Adoption:** later Emacs revisions append entries to this ledger and
  re-derive affected specs, contexts and packets; nothing is patched valid.
- **Rollback:** revert the single commit; no runtime, product, CI, support
  or GitHub state depends on it.
- **Transfer:** a successor ledger or manifest version supersedes this one
  only through a `supersede` entry with full cell and work drain.
- **Stop:** stop before validator commands, live GitHub observation,
  automatic issue/body/label/PR mutation, host proof, support claims,
  scheduling or publication. If a decision owned by another plane is needed
  as a decision, route it there; do not decide it in a ledger entry.

## Links

- Governing issue: #11770 (E01R)
- Parent programme: #7979 / #8706; durable architecture: #11716
- Consumed stable graph: `.spec/10918-emacs-train-graph/` (#10918)
- Reference revision contract: #11375 (LSP4IJ C02R)
- Generic mechanics gate: #10554; extraction precedent bundles:
  `.spec/11764-controller-train-graph/`, `.spec/11625-module-train-graph/`,
  `.spec/11763-issue-controller-architecture/`
- Exact-tree and overlay planes: #10923, #10930
