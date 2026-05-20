# PERL-LSP-SWARM-SPEC-0001: Source-of-truth stack contract

Status: accepted
Owner: repo-infra
Created: 2026-05-20
Linked proposal: PERL-LSP-SWARM-PROP-0001-source-of-truth-stack
Linked ADRs:
- none
Linked plan:
- plans/0.1.0/implementation-plan.md
Linked issues:
- none
Linked PRs:
- pending
Support-tier impact: docs/status/SUPPORT_TIERS.md
Policy impact:
- policy/doc-artifacts.toml
- policy/package-boundary.toml
- policy/ci-lane-whitelist.toml

## Problem

Contract layers are not currently explicit or enforceable.

## Behavior

The repository must maintain linked proposal/spec/ADR/plan/goal/support-tier/policy artifacts with stable IDs and required headers.

## Non-goals

Feature implementation and runtime behavior changes.

## Required evidence

Successful policy and goal checks once validators are implemented.

## Acceptance examples

`PERL-LSP-SWARM-PROP-*` links to `PERL-LSP-SWARM-SPEC-*`; plan and active goal references resolve.

## Test mapping

Future: `cargo xtask check-doc-artifacts`, `cargo xtask check-goals`.

## Implementation mapping

`docs/`, `plans/`, `.codex/goals/`, `policy/`, and CI workflow policy lane.

## CI proof

`Policy Contracts` workflow lane (advisory initially).

## Metrics / promotion rule

Promote claim tiers only after proof commands are wired and passing.

## Failure modes

Missing linked IDs, invalid headers, or unproven stable claims must fail checks.
