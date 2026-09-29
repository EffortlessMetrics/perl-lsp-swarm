# Context: #10980 — encode the stable perl-corpus authority DAG, conflict keys, and legacy exits

## Problem

The complete `perl-corpus` programme (epic #6696, execution controller #8826) had its
dependency, role, conflict, and exit semantics only in prose: the #8826 train body, its
three controller comments (leaf-id resolution 2026-08-14, graph refinement 2026-08-18,
current-main reconciliation 2026-08-29), and every leaf header. Every later consumer —
the current-tree frontier (#10992), the live candidate observer (#11001), the frontier
spec compiler (#11010), and the packet generator (#11017) — would re-parse that prose
and could derive a different graph. Issue labels, open/closed state, and PR state were
the only machine-readable signals, and none of them is a stable semantic fact.

## Why this approach

One human-edited, versioned JSON manifest (`perl_corpus_train.v1`) inside a `.spec`
bundle, validated by an xtask command with named diagnostics, exactly as the landed
train precedents did: `native_neovim_train.v1` (#11392: closed JSON Schema, named
reason codes, shuffled determinism control, invalid fixtures) and `module_train.v1`
(#11625/#11626: strict manifest laws, order-invariant canonical digest, deterministic
projections). The stable manifest owns what each node establishes, which exact result
depends on which, which authority moves, which writers conflict, which legacy path exits
when, and which action needs authorization. It never owns current main SHA, issue or PR
state, the active writer, or what an agent may start today; #10992/#11001/#11017 own
those.

Operations landed here, per the issue's checked commands:

```text
cargo xtask perl-corpus-train check
cargo xtask perl-corpus-train graph [--check]
cargo xtask perl-corpus-train explain-static <node>
```

## Current state (honest, as of this bundle)

- Current `main` carried no `perl_corpus_train` artifact or command; this bundle is the
  first. `CorpusRoot` (#7705) is on main; `CorpusAssetPath` (#10555) and
  `OpenedCorpusAsset` (#7693) are not — the manifest records that only as topology
  (which result depends on which), never as current-tree state.
- The manifest encodes 106 nodes: 22 controllers/umbrellas, 47 implementation, 20
  cutover, 6 proof, 2 external actions, and 9 historical inputs (closed #6699/#7700/
  #7705/#9140/#13072/#13807, merged PR #6750, superseded PR #7188, transferred
  PR #7200), joined by 256 typed edges (225 hard, 29 evidence, 2 authorization).
- Leaf headers that name an umbrella as a dependency (#6980, #6985, #6989, #6716,
  #7009) are routed to the leaf that owns the result; the edge provenance records the
  original statement. Controllers therefore carry no dependency edges and are never
  dependency targets.
- Cross-programme prerequisites (#6261, #7745 CI reachability; #8056, #8081 query
  execution) are declared external authorities, not nodes.

## Authority and ownership

- Controlling issue: #10980. Parent execution controller: #8826. Architecture epic:
  #6696. Programme controllers: #6703, #6721, #6706, #6708, #6712, #6725, #8091, #7086,
  #7405, #7407, #7408; sub-umbrellas #6985, #6989, #6716, #6718, #6980; later
  controllers #11030, #11032, #1377, #7009.
- Canonical semantic input: the #8826 body and its three controller comments, plus
  every leaf body's header group (`Depends on`, `Hard dependencies`, `Blocks`,
  `Successor`, `Consumes`, `May proceed in parallel with`). Leaf bodies remain the
  per-node authority; the manifest is the stable index over them.
- Generic authorities consumed, never cloned: #10858 (typed edge and claim-profile
  contract, via declared adaptations), #10554 (shared-mechanics extraction gate), #5205
  (anti-lifecycle-mirror ruling), #3982/#3983/#3957 (admission and method).

## Encoding decisions

- **Roles**: `controller | implementation | proof | cutover | decision | external_action
  | historical`; only implementation/proof/cutover are selectable. `decision` is
  declared but unused by the current node set.
- **Dependency classes**: `hard | evidence | authorization`. Authorization edges may
  only target the symbolic `#EXPLICIT-AUTHORIZATION` authority and may only be carried
  by `external_action` nodes, so publication can never become an ordinary coding
  dependency. Hard/evidence edges target nodes or numeric external authorities.
- **Release horizons**: the nine horizons from the issue, ranked. A node ranked before
  `package_externalization` may not hard/evidence-depend on a package or publication
  node (`PUBLICATION_PROMOTED_INTO_FOUNDATION`).
- **Conflict keys**: the issue's key list plus the narrow additions needed so that
  genuinely parallel siblings (the three CI routes, the three consumer families, the
  three parser-accuracy strata, the two #11030 consumer leaves) never share one
  exclusive key; two selectable nodes may share a key only when a hard path orders them
  (an evidence edge lets its source land first, so it orders nothing) (`CONFLICT_KEY_PARALLEL_COLLISION`).
- **Legacy exits**: every implementation/cutover leaf names an exit owner and removal
  condition; proof leaves are exempt because they move no authority.
- **Candidate lineages**: PR references live in a registry with reuse policy only;
  lineage rows and stable values are scanned for status words, branch/pull coordinates,
  and commit hashes (`MUTABLE_STATE_EMBEDDED`).
- **Phases** follow the #8826 train slots (`P`, `C0`, `A0`–`A6`, `B0`–`B2`, `H`).
- Node ids are `pc_<slug>_<issue>`; titles are fingerprinted (first 16 uppercase hex of
  SHA-256, the shared law) so a retitled issue cannot silently keep an old fingerprint.

## Shared-mechanics disposition (#10554)

The checker consumes `module_train::canonical_digest` (order-invariant SHA-256) and
`native_neovim_train::canonical_form` (canonical serialization) rather than copying
them. The overlap that remains programme-local — the title-fingerprint law, the
banned-key walk, and the hard-cycle search — is small and recorded here for the
#10554 gate; no extraction begins in this PR and #10554 is not a prerequisite.

## Alternatives rejected

- **A `.spec` data bundle with an embedded PowerShell checker only** (the #11625 shape):
  rejected because #10980 names the xtask commands as the one-PR result and the
  repository now has two landed xtask train validators to mirror.
- **A universal train schema shared with the other programmes**: rejected by #10554's
  gate and by #10980's "no repository-wide train manifest" rule; the shared contract is
  consumed through `.spec/10858-train-edge-contract/adaptations.json` instead.
- **Encoding controller membership as dependency edges**: rejected; controller
  membership is not a semantic edge, and a leaf depending on an umbrella hides the leaf
  that owns the result.

## Prior art / duplicates

`xtask/src/tasks/native_neovim_train.rs`, `xtask/src/tasks/module_train.rs`,
`xtask/src/tasks/train_edge_contract.rs`, `xtask/src/tasks/emacs_train_specs.rs`. No
existing corpus train manifest, command, schema, or projection was found on `main`.

## Links

- Issue: #10980; parent #8826; epic #6696.
- Successors: #10992 (current-tree frontier), #11001 (live candidates), #11010
  (frontier specs), #11017 (packets).
- Shared contracts: #10858 adaptations; #10554 gate.
- Precedent bundles: `.spec/11392-native-neovim-train-graph/`,
  `.spec/11625-module-train-graph/`.
