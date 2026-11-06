# Context: #10712 — the install train as a checked `distribution_train.v1` DAG

## Problem

The #10703 install/distribution programme's dependency and ownership semantics live in
prose: the controlling issue #10712 carries the required graph as ASCII; the public-route
controller #8087 carries the R03 DAG; each leaf issue carries its own reference block.
Every later consumer — spec compilation (#11164), live observation (#10726), the route
contract (#10333), and each adapter/guidance/parity leaf — would re-parse that prose and
could derive a different graph. #10712's acceptance forbids that: the manifest must encode
issue nodes rather than pull requests, and keep stable topology separate from live
observation.

## Why this approach

A machine-readable, versioned, deterministic manifest is the stable contract artifact.
It is compiled inside a `.spec` bundle exactly as the sibling precedents compiled theirs
(`.spec/10918-emacs-train-graph/` emacs_train.v1, `.spec/11625-module-train-graph/`,
`.spec/11084-import-cleanup-train-manifest/` import_cleanup_train.v1): checked data, a
fail-closed loader with closed vocabularies and structural laws, a pinned canonical
digest, and deterministic projections. The Rust loader and its `check | graph | explain |
next` projections live in `xtask/src/tasks/distribution_train.rs`; this bundle is the data.

## Design rule honored

The manifest contains stable architecture facts (node identity, kind, lane, one-PR
proposition, dependency classes, authorities, conflict keys, spec dispositions, proof
ownership, supersession, do-not-build rules). It contains no mutable facts: no PR numbers,
head SHAs, review/CI state, agent identity, or wall-clock ownership. `deny_unknown_fields`
plus the pinned canonical digest make any such field a loud failure, not silent drift.

`current_tree_status` is a reviewed repository-side snapshot (file evidence in the tree
that carries this manifest), explicitly not GitHub issue state; #10726 owns live
read-only observation.

## Current state (honest, as of this bundle)

- Landed substrate with file evidence in this tree: #6067 (release_topology.v4),
  #9103 (install_surface_registry.v1), #6355 (public_release_claims.v1, historical),
  #11550 (standalone conformance vectors).
- This branch lands #10712 (this bundle), #11164 (spec dispositions + route packets +
  `distribution-spec check|explain|index`), and #10333 (public_release_claims.v2 model,
  schema, validator, and historical-v1 disposition).
- #11157 was closed NOT_PLANNED as a duplicate of #11485; the standalone packet rail
  (#11485) and managed packet rail (#11161) remain open, so their leaves stay
  `ISSUE_PLAN_SUFFICIENT` until those packets compile.
- Everything else is `not_started` in the reviewed snapshot; statuses refresh only through
  reviewed manifest revisions that move the pinned digest.
