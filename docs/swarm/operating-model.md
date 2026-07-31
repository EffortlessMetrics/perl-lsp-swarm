# perl-lsp-swarm operating model

`perl-lsp-swarm` is the active development repository. `perl-lsp` remains the
curated release, history, and package-lineage repository until a separate
history-preserving repository decision changes that relationship.

This document defines repository roles and live-state ownership. It does not
create a scheduler, active lane, work queue, or provider runtime topology.

## Repository roles

| Repository | Role | Normal work |
| --- | --- | --- |
| `perl-lsp-swarm` | Active development and proof | Product/compiler/LSP/DAP changes, tests, specs, evidence, cleanup, and current PR integration |
| `perl-lsp` | Curated release lineage | Release syncs, history preservation, package lineage, and emergency release fixes mirrored back to swarm |

Default routing:

```text
new development → perl-lsp-swarm
curated release sync → perl-lsp
emergency release fix → perl-lsp, then mirror/reconcile into swarm
```

See [sync-protocol.md](sync-protocol.md) for exact history-preserving sync and
release mechanics.

## State ownership

```text
repository
  durable product, architecture, method, specification, and proof contracts

GitHub
  live issues, PRs, reviews, threads, checks, rulesets, merges, and remaining work

runtime
  selected claim, agent/model choices, task lists, worktrees, retries, and liveness
```

No tracked file appoints the repository's current program, lane, queue, or next
work item. Multi-PR outcomes live as ordinary GitHub umbrella issues, linked
specifications/ADRs, and current-main evidence. Several outcomes and claim lanes
may be active simultaneously.

The retired `.perl-lsp/goals/` manifests remain recoverable through Git history.
The compatibility commands `cargo xtask goals next`, `cargo xtask goals
reconcile`, and `cargo xtask check-active-goal-manifest` report retirement and
select no work.

## Development method

The active provider-neutral method is defined by:

- [Development method](../agents/DEVELOPMENT_METHOD.md)
- [GitHub surfaces](../agents/GITHUB_SURFACES.md)
- [Review and proof currentness](../agents/REVIEW_CURRENTNESS.md)
- [Skill contract](../agents/SKILL_CONTRACT.md)

The six public flows are:

```text
deliver-goal
deliver-pr
prepare-issue
prepare-proof
build-candidate
finish-pr
```

A fresh Claude or Codex session reconstructs only the selected outcome or claim,
enters at the earliest absent or stale useful judgment, follows locally named
skill routes, and continues until the requested result is reconciled, explicitly
left in flight under GitHub authority, or bounded by a real blocker or
`NOT_PROVEN` evidence.

## Concurrency

One writer mutates each current candidate branch/worktree at a time. Distinct
claim lanes use ordinary optimistic Git concurrency and may touch the same files,
crates, or nearby semantics. Do not create file reservations, semantic-surface
ownership, overlap maps, or sibling-lane surveillance.

When Git reports a conflict, an explicit prerequisite changes, or actual
merge-group/combined-tree proof fails, the affected lane owns the smallest
coherent repair and refreshes only affected proof and review.

## High-scrutiny surfaces

These surfaces require explicit behavior, authority, failure, proof, and claim
boundaries rather than routine-cleanup treatment:

- rename, safe delete, and edit-producing code actions;
- subprocess and DAP process state;
- URI/path/module resolution;
- LSP runtime and workspace-currentness state;
- published APIs, schemas, packages, and support claims;
- provider promotion and fallback decisions;
- compiler fact provenance and dynamic boundaries.

## Current durable control-plane contracts

Real Perl Editor Trust remains bounded by the accepted specifications and
current generated evidence, including:

- [Real Perl Editor Trust v1 boundary](../specs/PLSP-SPEC-0015-real-perl-editor-trust-v1-boundary.md)
- [Provider decision receipt v1](../specs/PLSP-SPEC-0016-provider-decision-receipt-v1.md)
- [Fact provenance and source backing](../specs/PLSP-SPEC-0017-fact-provenance-and-source-backing.md)
- [Edit authorization contract](../specs/PLSP-SPEC-0018-edit-authorization-contract.md)
- [Semantic token class promotion contract](../specs/PLSP-SPEC-0019-semantic-token-class-promotion-contract.md)
- [Workspace symbol generated-label contract](../specs/PLSP-SPEC-0020-workspace-symbol-generated-label-contract.md)
- [Diagnostic explanation v1](../specs/PLSP-SPEC-0021-diagnostic-explanation-v1.md)
- [Module path authority](../specs/PLSP-SPEC-0022-module-path-authority.md)
- [Ambient inputs](../specs/PLSP-SPEC-0023-ambient-inputs.md)
- [Framework fact adapters](../specs/PLSP-SPEC-0024-framework-fact-adapters.md)
- [PIR v0](../specs/PLSP-SPEC-0025-pir-v0.md)
- [Real Perl Editor Trust dashboard](../project/status/real_perl_editor_trust_v1.md)
- [Provider promotion ledger](../project/status/provider_promotion_ledger.md)

Generated status is evidence for its exact generation and inputs. It does not
select work or create a repository-global current lane.
