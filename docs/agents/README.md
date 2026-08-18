# Agent method and worked lanes

This directory contains current cross-provider method contracts plus optional worked
examples. It is not a second workflow engine.

## Read first

- [Agent and maintainer authority status](AUTHORITY_STATUS.md) — classifies current,
  transitional, historical, and superseded agent/control-plane documents. Use this when
  search or an internal old status banner conflicts with current provider-native method.
- [Development method](DEVELOPMENT_METHOD.md) — shared orchestration and delivery method.
- [Review and proof currentness](REVIEW_CURRENTNESS.md) — semantic review currentness and
  exact-subject machine-evidence boundary.
- [GitHub surfaces](GITHUB_SURFACES.md) — durable issue, PR, review, check, thread, label,
  and merge authority.
- [Skill contract](SKILL_CONTRACT.md) — public flows and atomic skill composition.

The machine-readable document inventory is
[`authority_status.toml`](authority_status.toml).

## Worked lanes

Worked lanes are optional calibration examples drawn from durable repository and GitHub
artifacts. They show proportion, evidence boundaries, and routing decisions; they are
not phrase templates or a replacement for the active skills.

- [Integration trigger and bounded proof caller](examples/integration-trigger-and-proof-caller.md)
  — a pure trigger authority followed by a bounded synthetic-proof caller, including a
  later instrument failure that remained `NOT_PROVEN`.

## Reading rule

Each current contract and example states what it establishes and what it does not.
Do not promote a local pass, a merge, a historical banner, or a bot result into a broader
product, review, release, or orchestration claim without the relevant current authority
and evidence.
