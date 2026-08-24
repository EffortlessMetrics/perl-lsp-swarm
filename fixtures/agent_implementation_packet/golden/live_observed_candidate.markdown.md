# Builder packet: N_service_marker_probe

## Work identity

- packet: example-train/P_service_marker_probe/2026-08-24T06:00:00Z
- repository: perl-lsp-swarm @ a277266c5
- programme: example-train example_train.v1 v3
- node/proposition/profile: N_service_marker_probe / P_service_marker_probe / coding_agent_bounded
- owning issue: #10858
- actor: coding_agent_bounded (write boundary: repository_candidate_branch)
- frontier: ready (frontier:example-train:3:ready:P_service_marker_probe:sha256:1c0f)
- candidate state: observed
  - candidate: PR #12150 branch tooling/service-marker-probe head 44d2c9e10
  - collision: one-writer-active-no-collision
- current-tree probe: xtask/src/tasks/service_marker_probe.rs -> branch-candidate-open

## Result and claim ceiling

The service-marker probe emits one typed mismatch finding per stale marker with a focused failing-first test.

Claim ceiling: Establishes only the service-marker probe and its focused test; no frontier, scheduler, packet-selection, or live-observation semantics.

- remains unproven: end-to-end marker freshness across multiple programmes
- non-goal: no readiness evaluation
- non-goal: no GitHub observation

## Authorities

- [must_be_current] #10858 — shared train edge and claim-profile contract
- [may_be_mined] refs/heads/archived/marker-spike — historical marker-spike branch
- [must_not_be_reimplemented] xtask/src/tasks/train_edge_contract.rs — shared edge-contract validator
- [consumer_fan_in] #10872 — shared builder-packet contract consumer
- [external_manual_owner] maintainer/merger — review and merge authority

## Bounded repository surfaces

- implementation: xtask/src/tasks/service_marker_probe.rs
- tests/fixtures: fixtures/service_marker_probe/stale_marker.v1.json
- forbidden adjacent: xtask/src/tasks/train_edge_contract.rs
- writer slot service-marker-probe-module: xtask/src/tasks/service_marker_probe.rs

## Shift-left proof

- falsifier F_candidate_resume_repairs_probe [integration]: Resuming PR #12150 must repair the probe so the stale-marker focused test passes; discarding the observed candidate and rewriting from scratch is a defect.
- falsifier F_marker_missing_fails [unit]: A stale service marker that the probe accepts is a defect: the focused test feeds one stale marker and asserts the typed mismatch is emitted.
- positive discriminator: The probe distinguishes a stale marker from a fresh one by comparing the marker's recorded tree against the observed tree, not by marker presence alone.
- mutation control: Inverting the tree comparison in the probe must flip both focused tests red.
- terminal outcome: DELIVERED_REVIEWABLE_PR
- terminal outcome: RESUMED_CANDIDATE
- terminal outcome: NOT_PROVEN

## Verification route

1. [focused_proof] xtask.test.service_marker_probe — `cargo test -p xtask --bin xtask service_marker_probe --locked`
2. [diff_check] git.diff_check — `git diff --check`

## Delivery and handoff

Delivered means: reviewable_draft_pr_and_handoff

- branch: tooling/example-service-marker-probe
- PR title: tooling(train): add the service-marker probe focused falsifier
- PR body field: change
- PR body field: proof
- PR body field: candidate-resume
- limitation: candidate observation is a snapshot; re-observe before acting
- next: repair the observed candidate and hand the PR back to review

## Stop conditions

- stop: stop at the reviewable draft PR and handoff comment
- stop: do not merge or release
- stop: re-observe the candidate if resumption is delayed
- no terminal action beyond the reviewable draft PR is permitted
