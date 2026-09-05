# Builder packet: N_service_marker_probe

## Work identity

- packet: example-train/P_service_marker_probe/2026-08-24T00:00:00Z
- repository: perl-lsp-swarm @ a277266c5
- programme: example-train example_train.v1 v3
- node/proposition/profile: N_service_marker_probe / P_service_marker_probe / coding_agent_bounded
- owning issue: #10858
- actor: coding_agent_bounded (write boundary: repository_candidate_branch)
- frontier: ready (frontier:example-train:3:ready:P_service_marker_probe:sha256:1c0f)
- candidate state: not_observed
- current-tree probe: xtask/src/tasks/service_marker_probe.rs -> absent-on-observed-tree

## Result and claim ceiling

The service-marker probe emits one typed mismatch finding per stale marker with a focused failing-first test.

Claim ceiling: Establishes only the service-marker probe and its focused test; no frontier, scheduler, packet-selection, or live-observation semantics.

- remains unproven: end-to-end marker freshness across multiple programmes
- remains unproven: performance under a ten-thousand-marker workspace
- non-goal: no readiness evaluation
- non-goal: no GitHub observation
- non-goal: no scheduler or agent assignment

## Authorities

- [must_be_current] #10858 — shared train edge and claim-profile contract
- [must_be_current] .spec/10858-train-edge-contract/acceptance.md — checked acceptance criteria for the shared edge contract
- [may_be_mined] refs/heads/archived/marker-spike — historical marker-spike branch
- [must_not_be_reimplemented] xtask/src/tasks/train_edge_contract.rs — shared edge-contract validator
- [consumer_fan_in] #10872 — shared builder-packet contract consumer
- [external_manual_owner] maintainer/reviewer — review and merge authority

## Bounded repository surfaces

- implementation: xtask/src/tasks/service_marker_probe.rs
- tests/fixtures: fixtures/service_marker_probe/stale_marker.v1.json
- generated: docs/policy/NON_RUST_INVENTORY.md
- forbidden adjacent: xtask/src/tasks/train_edge_contract.rs
- forbidden adjacent: schemas/train_edge_contract.v1.schema.json
- writer slot service-marker-probe-module: xtask/src/tasks/service_marker_probe.rs
- writer slot service-marker-fixture: fixtures/service_marker_probe/stale_marker.v1.json

## Shift-left proof

- falsifier F_marker_missing_fails [unit]: A stale service marker that the probe accepts is a defect: the focused test feeds one stale marker and asserts the typed mismatch is emitted.
- falsifier F_fresh_marker_passes [unit]: A fresh marker must not emit a finding; the positive control feeds a current marker and asserts no mismatch.
- positive discriminator: The probe distinguishes a stale marker from a fresh one by comparing the marker's recorded tree against the observed tree, not by marker presence alone.
- mutation control: Inverting the tree comparison in the probe must flip both focused tests red.
- mutation control: Deleting the emission path must fail the stale-marker falsifier.
- terminal outcome: DELIVERED_REVIEWABLE_PR
- terminal outcome: BLOCKED_MISSING_INPUT
- terminal outcome: NOT_PROVEN

## Verification route

1. [focused_proof] xtask.test.service_marker_probe — `cargo test -p xtask --bin xtask service_marker_probe --locked`
2. [generation] xtask.non_rust_inventory_write — `cargo xtask non-rust inventory --write` (second run: no diff)
3. [file_policy] xtask.check_file_policy — `cargo xtask check-file-policy`
4. [format] cargo.fmt_check — `cargo fmt -p xtask -- --check`
5. [clippy] cargo.clippy_xtask — `cargo clippy -p xtask --all-targets --locked -- -D warnings`
6. [diff_check] git.diff_check — `git diff --check`

## Delivery and handoff

Delivered means: reviewable_draft_pr_and_handoff

- branch: tooling/example-service-marker-probe
- PR title: tooling(train): add the service-marker probe focused falsifier
- PR body field: change
- PR body field: proof
- PR body field: boundaries
- PR body field: rollback
- limitation: probe covers one programme's marker namespace
- next: after merge, P_service_marker_rollout may compose over this probe

## Stop conditions

- stop: stop at the reviewable draft PR and handoff comment
- stop: do not merge, release, or perform any external action
- stop: do not mutate the programme manifest or frontier
- no terminal action beyond the reviewable draft PR is permitted
