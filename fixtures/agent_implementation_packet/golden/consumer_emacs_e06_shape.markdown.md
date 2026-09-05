# Builder packet: E06

## Work identity

- packet: emacs-train/E06-P_hover_fixture_dogfood/2026-08-24T00:00:00Z
- repository: perl-lsp-swarm @ fe2e8bb0d
- programme: emacs-train emacs_train.v1 v55
- node/proposition/profile: E06 / P_hover_fixture_dogfood / coding_agent_bounded
- owning issue: #11719
- actor: coding_agent_bounded (write boundary: repository_candidate_branch)
- frontier: ready (frontier:emacs-train:55:ready:P_hover_fixture_dogfood:sha256:9a41)
- candidate state: not_observed (no live observation supplied)
- current-tree probe: emacs/train/packet_adapter.el -> absent-on-observed-tree

## Result and claim ceiling

The Emacs hover fixture dogfood renders one shared builder packet without any Emacs-local packet schema.

Claim ceiling: Adapter projection only: joins manifest, ledger, spec disposition, tree context, and supplied observation into agent_implementation_packet.v1; no Emacs packet schema, no model invocation, no GitHub mutation, no scheduling.

- remains unproven: live candidate observation beyond the supplied snapshot
- remains unproven: strong-agent execution quality
- non-goal: no Emacs-local packet ontology
- non-goal: no readiness evaluation inside the adapter
- non-goal: no label/body mutation

## Authorities

- [must_be_current] #11716 — E00 architecture row fixing the adapter boundary
- [must_be_current] #10918 — E01 stable emacs_train.v1 graph (55 nodes)
- [must_be_current] #11770 — E01R emacs_train_revision.v1 ledger
- [must_not_be_reimplemented] #10872 — shared builder-packet contract this adapter projects into
- [consumer_fan_in] #11114 — generic dogfood consumer
- [external_manual_owner] maintainer/merger — review and merge authority

## Bounded repository surfaces

- implementation: emacs/train/packet_adapter.el
- tests/fixtures: tests/emacs/packet_adapter_spec.el
- forbidden adjacent: schemas/agent_implementation_packet.v1.schema.json
- forbidden adjacent: xtask/src/tasks/agent_implementation_packet.rs
- writer slot emacs-packet-adapter: emacs/train/packet_adapter.el, tests/emacs/packet_adapter_spec.el

## Shift-left proof

- falsifier F_no_emacs_local_schema [unit]: Any Emacs-local packet schema emitted by the adapter is a defect: the focused test asserts every rendered payload carries schema agent_implementation_packet.v1 and no Emacs-local schema id.
- falsifier F_missing_input_fails_closed [unit]: Composing a packet with a missing spec disposition must fail with the missing-input reason rather than rendering plausible prose.
- positive discriminator: The adapter projects manifest+ledger+spec+tree joins into the shared schema fields; Emacs supplies fields only.
- mutation control: Renaming the shared schema const in the adapter must fail the no-local-schema falsifier.
- mutation control: Dropping the spec-disposition lookup must fail the fail-closed falsifier.
- terminal outcome: DELIVERED_REVIEWABLE_PR
- terminal outcome: BLOCKED_MISSING_INPUT
- terminal outcome: NOT_PROVEN

## Verification route

1. [focused_proof] emacs.ert.packet_adapter — `make -C emacs test PACKET_ADAPTER=1`
2. [diff_check] git.diff_check — `git diff --check`

## Delivery and handoff

Delivered means: reviewable_draft_pr_and_handoff

- branch: tooling/emacs-packet-adapter-e06
- PR title: tooling(emacs-train): project E06 into the shared builder packet
- PR body field: change
- PR body field: proof
- PR body field: boundaries
- limitation: offline composition only until #10930 supplies live observation
- next: wait for E02 #11717 and E04 #11718 before enabling generation for all nodes

## Stop conditions

- stop: stop at the reviewable draft PR and handoff comment
- stop: no model invocation, GitHub mutation, or scheduling
- stop: candidate/collision state remains not observed until #10930 lands
- no terminal action beyond the reviewable draft PR is permitted
