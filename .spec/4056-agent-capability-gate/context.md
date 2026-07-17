# Context: #4056 - Agent capability gate runner routing

## Problem

`.github/workflows/agent-capability-gate.yml` currently executes the M4b
review/audit-agent capability check on `ubuntu-latest`. The check is small, but
the workflow is part of the repository's front-door trust surface and should
use the same trusted `workflow-nano` capacity as the other control-plane
workflows. Fork and bot pull requests must never execute untrusted checkout
content on an organization self-hosted runner.

## Design

Keep the existing triggers, permissions, concurrency, pinned checkout/toolchain,
and `cargo xtask check-agent-capabilities` command. Add a hosted router job on
`ubuntu-24.04` with static downstream jobs:

- `agent-capability-gate-self-hosted` runs on
  `[self-hosted, linux, x64, em-ci, trusted-pr, workflow-nano]` only for a
  trusted same-repository event when an online, idle runner with those labels
  is observed in the `em-ci-nano` runner group.
- `agent-capability-gate-hosted` runs on `ubuntu-24.04` for fork/bot events and
  for explicit infrastructure fallback (missing token, runner API failure, or
  no idle runner).

The router emits `target`, `reason`, `error`, and `fallback_allowed` in both the
step output and job summary. It treats runner-group API failure or a missing
`em-ci-nano` group as infrastructure fallback. A capability-policy failure in
either execution job remains a real failure; only routing/capacity failures
select the hosted fallback.

## Alternatives rejected

- A dynamic `runs-on` expression is rejected because GitHub runner selection
  must remain statically auditable and cannot safely encode trust boundaries.
- `pull_request_target` is rejected because it would expose a privileged
  workflow to untrusted pull-request content.
- Silent hosted fallback on a policy-check failure is rejected because it would
  hide a real M4b violation.
- Reusing the Rust Small router wholesale is rejected because its runner labels,
  required result semantics, and failure policy differ from this hygiene gate.

## Claim boundary

This slice proves the workflow is statically routed, fork/bot-safe, explicit
about infrastructure fallback, group-aware, and still fails when the
capability checker fails. It does not prove live runner capacity, Rust/Cargo
availability on `workflow-nano`, or the permission scope of
`EM_RUNNER_READ_TOKEN`; those are external prerequisites and remain
`NOT_PROVEN` until a live run exercises them.

## Cargo-allow policy

No new Rust source exception is authorized. The policy test uses fallible
`Result`/`anyhow` checks rather than panic-shaped assertions. Run
`rtk cargo allow diff --base origin/main` and record any repository-wide
baseline findings from `rtk cargo allow check` separately.
