# Context: #10918 — canonical stable emacs_train.v1 topology graph

This is a checked, declarative stable contract. It adds no xtask validation
command, current-tree probe, GitHub observation, packet instance, readiness
computation, host execution, scheduler, support result or GitHub mutation.
Those remain owned by the nodes this graph declares.

## Problem

The Emacs support programme's execution topology lives in prose: the E00 bundle
(`.spec/11716-emacs-support-architecture/`) records the architecture planes,
per-leaf ceilings and a dependency sketch as markdown; the controlling issue
#10918 carries a canonical seed graph and a corrected functional DAG in ASCII;
two #10918 review comments add the decomposed stable-graph corrections (the
#8755/#8834/#8838/#11717/#11718/#10936 fan-in directions, the #11766, #11768
and #11770 additions); the functional controller #8706 and every leaf issue
carry their own reference blocks. Every later consumer — the spec plane
(E02 #11717 and #11751–#11755), the context plane (E04 #11718, #11756–#11758),
the packet adapter (E06 #11719), the dogfood chain (#10936, #11759, #11760),
revision governance (E01R #11770), and the subject, adapter, profile,
actual-host, root, public and projection lanes — would re-parse that prose and
could derive a different graph. #10918's acceptance forbids that: the manifest
must encode issue nodes rather than pull requests, adopt the existing
candidate only as a rule, and keep the plane boundaries distinct.

## Why this approach

A machine-readable, versioned, deterministic manifest is the stable contract
artifact the E00 bundle names (`emacs_train.v1`, §API-Shape). It is compiled
inside a `.spec` bundle exactly as the sibling precedents compiled theirs
(`.spec/11716-emacs-support-architecture/` issue #11716, merged as E00;
`.spec/11764-controller-train-graph/` issue #11764, the controller train's
T01 stable graph; `.spec/10894-editor-host-reliability/` issue #11766):
canonical bundle files, an embedded PowerShell 7 structural checker with
fail-closed negative controls, a two-run determinism proof, and an honest
`not_proven` boundary. JSON is used so the checker needs no external parser,
following the machine-readable precedent
`.spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml`.

The controlling issue also names offline xtask operations
(`cargo xtask integration emacs train check` / `graph`). Those are executable
repository tooling and are deliberately **not** built here: this bundle lands
the topology as checked data plus its embedded checker, and records the absent
xtask validator as `not_proven` rather than papering over it. Building the
validator is a separate tooling claim against the same seam, exactly as the
controller train deferred its independent validator from T01 to T02.

## Current state (honest, as of this bundle)

- E00 is merged: `.spec/11716-emacs-support-architecture/` with its
  architecture planes, per-leaf contract table and embedded checker.
- The controller-train sibling T01 (`.spec/11764-controller-train-graph/`,
  26 nodes) is merged and is the shape precedent for this bundle.
- `#11716` (E00) and `#11766` (shared host-run reliability projection) are
  closed; `#11366` is closed as landed substrate; `#7777` is closed with the
  receipt contract landed through merged pull 7874; the `#7778` runner core
  landed through merged pull 8024 while the issue remains open beyond that
  core; pull 8026 (project fixtures) is merged, so the canonical
  existing-candidate rule for `#11366` is recorded as stable law whose live
  confirmation still belongs to `#10930`.
- The repository has no Emacs train tooling of any kind; the only validators
  for `.spec` bundles are the embedded structural checkers in each bundle's
  checklist.
- This bundle lands the manifest as data plus its embedded checker. It does
  not prove the topology is the semantically correct reading of every leaf
  body — that is this PR's review job here and E01R's job after.

## Authority and ownership

- Controlling issue: #10918 (E01). Parent programme: #7979. Functional
  controller: #8706.
- Durable architecture consumed: the E00 bundle — its architecture planes,
  identity decisions, authority split, per-leaf ceilings and dependency
  ordering are semantic input; this manifest encodes them, it does not
  re-derive, widen or replace them. A graph byte never amends an E00 decision.
- Stable-graph corrections consumed: the two #10918 review comments that fix
  the fan-in directions (`#11744/#11745/#11746 → #8755`,
  `#11747/#11748 → #8834`, `#11749/#11750 → #8838`,
  `#11751 → populations → #11717`, `#11756 → populations → #11718`,
  `#11759 → #11760 → #10936`) and add `#11766`, `#11768` and `#11770` with
  their role and gating semantics, including the four negative topology
  fixtures (ungoverned cells, bypassed shared cleanup, un-mapped authority
  change, metadata-only invalidation).
- Leaf issue bodies are the per-node authority; every edge records the
  statement it traces to (leaf body references, the #10918 canonical seed
  graph or corrected DAG, a #10918 comment correction, or an E00 section).
- Generic authorities consumed, never cloned: #10527 (receipt integrity),
  #10894 (editor-host reliability), #10858 (typed dependency/evidence
  classes), #10872/#10881 (packet contracts), #10554 (train-mechanics
  extraction gate), #11114 (evaluation vocabulary), #7956 (proof routing),
  #3983/#3949 (method), #4177/#3982/#3957 (writer admission),
  #3390/#3693/#6275 (CI, review, integration), #7122 (support registry),
  #5903/#6990 (Linux install inputs), #9310/#9374/#9413 (already-decomposed
  child trains referenced, not flattened), #10923/#10930 (current-tree and
  live planes).

## Durable laws consumed

The manifest encodes, as data plus checker law:

- **Eight authority planes** from the #10918 architecture-planes section, in
  fixed order: the E00 durable architecture, `emacs_train.v1` itself, the
  #11717 spec plane, the #10923 exact current-tree plane, the #11718
  navigation plane, the #10930 live plane, the #11719 packet plane, and the
  `editor_client_compat.v1` + #7122 behavior/support plane. No plane
  satisfies another.
- **Eleven train roles** — controller, specification, stable_contract,
  semantic_revision, historical, implementation, fan_in, packet_adapter,
  dogfood, evidence_policy, external_gate — kept distinct from any GitHub
  issue-role vocabulary. `historical` encodes #10918's completed-evidence
  class; `evidence_policy` encodes #9375's typed routing authority without
  receipt or support power.
- **Typed dependencies** with classes hard / evidence / optional / external
  per #10858, one writer slot and conflict key per node, the A/B/C/D writer
  capacity classes from #10918, parallel groups and stack relations, claim
  ceilings, spec dispositions, first falsifiers, control sets, proof
  obligations, review questions, obligations, exits, rollback quartets,
  successors, identity fields and limitations for all 55 nodes.
- **Graph laws**: the corrected functional DAG plus the comment corrections —
  E00 → E01 → E01R; the E02/E04 planes as engine plus parallel sibling
  populations fanning into their parents; E06 joining the spec and context
  planes; the dogfood chain fanning into #10936; the subject lane
  (#11744 → #11745/#11746 → #8755 fan-in, with the corrected-DAG #8734 edge
  preserved); observation/producer separation (#8755 → #11360 → #11361, with
  adapters parallel after #11360 and #8755); #11768 as a hard predecessor for
  promotion-eligible actual-host, root-semantics and public journey leaves
  and an evidence input to #11360/#11361/#8776/#8795; the root lanes as
  independent client-family chains fanning into #8834 (observation) and
  #8838 (semantics); #8842 owning install/fresh-process substrate without
  waiting for semantic journeys; public replays requiring their exact local
  hosts, required root cells, the substrate and the producer; and
  #8858/#8862/#8865 as strict fan-ins over complete denominators. The
  checker freezes 121 law edges with their exact classes and 24 forbidden
  edges; the manifest cannot silently weaken or add one.
- **Evidence semantics**: missing, partial, stale, contradictory or
  instrument-failed evidence is `not_proven`, never pass; optional and
  unavailable rows remain explicit; metadata-only movement invalidates
  nothing.
- **Stable-byte hygiene**: no current SHA, PR, check, review, model, writer,
  landing or live metadata state enters the manifest; the existing candidate
  for #11366 appears only as the canonical adoption rule (pull 8026,
  confirmed live by #10930, never a node); source paths appear only when
  CTX_SUB/CTX_PUB resolve them on one exact tree.

## Encoding decisions and traceability

- Node IDs follow the E00 topology vocabulary (E00/E01/E01R/E02/E04/E06) plus
  stable leaf identifiers; issue numbers are the identity, titles are
  fingerprinted, and pull requests never appear as nodes.
- `#7778` is encoded as the landed runner-core proposition (merged pull 8024)
  per the #10918 canonical seed graph; the issue's residual openness is
  current state, not topology, and is not encoded.
- `#11366` is landed substrate per E00; its canonical existing-candidate rule
  is encoded as stable law in a dedicated block, and regression case 1 (a
  pull request posing as a stable node) is a checker law.
- E00's vertical E02/E04 listing is encoded as one engine plus parallel
  sibling populations with the parent retained as fan-in, per the comment
  correction that population leaves are siblings after their engines.
- `#8755` carries both the comment's subject-lane fan-in edges and the
  corrected-DAG hard edge from `#8734`; both provenances are recorded.
- `#11718`, `#11719` and the context/packet currentness carry evidence-class
  edges to E01R per the comment placing #11770 as their predecessor and
  evidence authority.
- `#9375` holds no edge of the corrected functional DAG; it is encoded as an
  isolated typed evidence authority consumed through proof routing, never as
  a receipt, support or host authority.
- Optional breadth (`#9310`, referenced optionally from certification) and
  the sibling child trains `#9374`/`#9413` are referenced as external
  authorities, never flattened into the Linux chain.
- Every dependency records its provenance string from a closed vocabulary
  (`#10918 body corrected functional DAG`, `#10918 body canonical seed
  graph`, `#10918 body observation/receipt separation`, `#10918 body
  CI/proof-routing policy`, the two comment corrections, the `#7979/#8706`
  programme header, and three E00 sections), so a reader can trace each edge
  to its statement.

## Compatibility with the repository operating contract (`AGENTS.md`)

- The manifest holds stable reviewed topology only — the same authority class
  as `.spec` bundles and generated contracts. Runtime topology, frontier,
  task order, liveness, retries and temporary plans remain runtime-local and
  never enter durable bytes.
- The manifest is a navigation and contract surface: it sequences nothing,
  owns no liveness, and replaces no `$deliver-*` route selection. It adds no
  readiness command, no scheduler and no parallel lifecycle.
- One writer built this candidate; no writer registry or lease table is
  created. Writer admission stays with #4177/#3982/#3957.
- The node set is programme-local to the Emacs support train; #9310, #9374
  and #9413 remain independent child trains referenced from the architecture.

## Open decisions respected, not decided

Five open decisions are recorded with their owners and are not decided here:
OD1 (shared checked-train mechanics extraction, gated on #10554's
concrete-reuse condition → #10554, consumed by E01R #11770), OD2 (canonical
existing-candidate adoption for #11366, confirmed live by → #10930), OD3
(exact subject pin selection for the five client generations → #11744), OD4
(optional, upstream and platform breadth entry timing → #9310), OD5 (Emacs
lifecycle-proof routing details under #7956 → #9375). The checker requires
exactly these five decisions with exactly these owners; a manifest revision
that decided one here would have to reclassify it, which is E01R's authority.

## Adoption, rollback, transfer and stop

**Adoption.** E01R (#11770) governs semantic revisions of this manifest; the
E02/E04/E06 and dogfood chains and every implementation lane consume the
topology without re-parsing controller prose; #11717 compiles per-node specs
against it; #11718 maps exact-tree context per node.

**Rollback.** Revert the single commit or remove this bundle directory; no
runtime, product, CI, support or GitHub state depends on it. The E00 bundle
and the issue bodies remain authoritative.

**Transfer.** A successor manifest version supersedes this one only through
an E01R-classified revision with an exact successor recorded; stale derived
artifacts are re-derived, never patched valid.

**Stop.** Stop before validator commands, current-tree probes, frontier,
source-context resolution, live observation, packet rendering, GitHub
metadata work, exact-head checkers, dogfood, scheduling, host execution,
support claims, release or publication. If an open decision OD1–OD5 is needed
as a decision rather than a boundary, stop and route it to its owning issue.

## Links

- Controlling issue: #10918 (E01); parents: #7979 / #8706.
- Durable architecture: #11716, `.spec/11716-emacs-support-architecture/`.
- Shape precedent: #11764, `.spec/11764-controller-train-graph/`.
- Shared reliability projection: #11766, `.spec/10894-editor-host-reliability/`.
- Control planes: #11717 (+ #11751–#11755), #11718 (+ #11756–#11758),
  #11719, #10936 (+ #11759, #11760); revision governance #11770; governed
  journeys #11768.
- Foundation and lanes: #7777, #7778, #11366, #8734, #9375, #11744–#11746,
  #8755, #11360, #11361, #8776, #8795, #8819, #8821, #8822–#8825, #8828,
  #8830, #11747–#11750, #8834, #8838, #8842, #8846, #8849, #8853, #8858,
  #8862, #8865.
- Generic authorities: #10527, #10894, #10858, #10872, #10881, #10554,
  #11114, #7956, #3983, #3949, #4177, #3982, #3957, #3390, #3693, #6275,
  #7122, #5903, #6990, #9310, #9374, #9413, #10923, #10930.
