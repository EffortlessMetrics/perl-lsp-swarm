# PERL-LSP-SWARM-PROP-0001: Source-of-truth stack

Status: proposed
Owner: repo-infra
Created: 2026-05-20
Target milestone: 0.1.0
Linked specs:
- PERL-LSP-SWARM-SPEC-0001-source-of-truth-contract
Linked ADRs:
- none
Linked plan:
- plans/0.1.0/implementation-plan.md
Support-tier impact: yes
Policy impact: yes

## Problem

The repository has no enforceable linkage between motivation, behavior contracts, execution sequencing, and proof obligations.

## Users and surfaces

Contributors, maintainers, and agents that rely on predictable proposal/spec/plan and policy workflows.

## Success criteria

A linked document and policy stack exists with stable IDs and proof-oriented artifacts.

## Proposed shape

Adopt proposal/spec/ADR/plan/goal/status/policy taxonomy with templates and ledgers.

## Alternatives considered

Single-document strategy was rejected because it merges why/what/how/proof and blocks validation.

## Specs to create or update

- PERL-LSP-SWARM-SPEC-0001-source-of-truth-contract

## Architecture decisions needed

- none currently

## Implementation campaign shape

Scaffold first, then ledger and validators, then CI enforcement.

## Evidence plan

`git diff --check`; future `cargo xtask` policy validators.

## Risks

Over-scoping into implementation code before contracts are in place.

## Non-goals

Runtime feature changes.

## Exit criteria

All contract layers exist, link forward, and are ready for checker enforcement.
