# SEM-ID-01 context

- Issue: #12121 (parent controller #7303, epic #2508).
- Basis: main@e1b03b943 (2026-08-24).
- One PR; no edit-impact classifier and no semantic analysis behavior change.

## Problem

Fresh-full semantic construction (#7306 line: #12135/#12136/#12138), typed AST
effect migration (#8448), generation-owned snapshots (#8557/#12150), and the
incremental successor (#12122/#7308) all need the same durable scope/
contribution/owner/dependency identity vocabulary. Before this PR no such
contract exists: the crate's `ScopeId` is a traversal-order newtype, and no
contribution-owner or fact-family identity exists anywhere.

## Decision

Add a transport-neutral `semantic_identity` module to `perl-semantic-facts`
(the crate both the analyzer and LSP layers already depend on):

- `SemanticSubjectGeneration` binds logical document instance, accepted source
  generation, parser snapshot/configuration, and semantic profile. Distinct on
  source-identical later generations, close/reopen, and multi-root subjects.
- `SemanticScopeIdentity` composes kind, owning declaration key, parent logical
  fingerprint, logical source anchor (digest + sibling ordinal, not offsets/
  lines/paths/names alone), package/source-order context, and recovery
  disposition. Deterministic FNV-1a fingerprint over canonical ordered fields.
- `SemanticContributionOwner` / `SemanticOwnershipDisposition` give every
  future contribution exactly one typed owner (scope / file-global /
  source-order context / external canonical producer / compatibility
  projection with exit / unsupported-not-proven).
- `SemanticFactFamily` (16 closed families), `SemanticDependencyIdentity`,
  `SemanticContributionId`, shared `SemanticSubjectStatus` (9 states), and the
  common work-subject fields (`SemanticWorkSubjectIdentity`) that later #12122
  receipts bind to — without defining retained/rebased/recomputed semantics.

## Proof strategy

Fifteen unit fixtures enforce the identity law: unrelated-earlier-insertion
stability, source-identical-generation distinctness, close/reopen and
multi-root distinctness, same-name sibling distinctness, order-permutation
fingerprint determinism, fail-closed empty fields, owner/status contradiction
rules, JSON round-trip, and a mechanical architecture fence (no LSP/parser/
traversal-order `ScopeId(` tokens in the non-test module sources).

## Boundaries

- No AST traversal, no semantic output change, no providers, no incremental
  policy (owned by #12122).
- Same-anchor sibling disambiguation uses a sibling ordinal scoped to one
  parent and anchor digest; this is stable under unrelated earlier insertion
  with a different anchor, and same-anchor reorderings intentionally produce
  distinct identities (conservative, fails toward recompute, never toward
  false reuse).
