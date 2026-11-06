# Gate stack merge order (#17405 family / #17707)

Four open pull requests each add a **required** `merge_gate` entry on the hosted
`policy` shard. They must land in a fixed order so the shared activation
inventory and `xtask/tests/activation.rs` hardcoded counts are re-derived **once**
instead of fighting in `git merge-tree`.

## Required merge order

| Order | PR | Gate id | Notes |
| --- | --- | --- | --- |
| 1 | [#17411](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17411) | `test_deletion_gate` | Closes the T4 deletion half (#17405). |
| 2 | [#17432](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17432) | `secret_scan` | Token-shaped secret refusal (#17428). |
| 3 | [#17433](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17433) | `change_scope_check` | T6 scope admission (#17429); vacuous `Ok(0)` receipts must not serialize as `pass` (#17707). |
| 4 | [#17434](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17434) | `no_verify_recover_check` | `--no-verify` recovery refusal. |

After **all four** are on `main`, run once (writer-owned):

```bash
cargo xtask activation generate --write
```

and commit the resulting `policy/activation-inventory.v1.json` plus
`xtask/tests/activation.rs` count pins in the same integration commit. Do not
bump those integers independently in each gate PR.

## Merge blocking path (not a GitHub-required shard context)

The live ruleset does **not** protect `CI Gate shard (...)` contexts directly.
Required `merge_gate` rows still block integration when they are **hosted on the
`policy` shard** and wired through the four-leg closure checked by
`issue_17707_policy_shard_required_gates_have_hosted_closure` in
`xtask/tests/issue_17707_gate_hosted_closure.rs`:

```text
.ci/gate-policy.yaml (required merge_gate row)
  → ci.yml merge-gate-shards / policy matrix row
  → .ci/gate-shard-execution.json dependency row
  → scripts/ci/validate_gate_lane_mapping.py GATE_TO_LANE_MAP entry
  → CI Gate shard (policy) job failure
  → Perl LSP Rust Small Result (`scripts/ci/main_red_refusal.py`), including
    candidate-side policy shard reds when `main` is green (#17707 routing)
```

Inner `required: true` means the shard runner invocation fails closed; GitHub
mechanical protection remains on `Perl LSP Rust Small Result`, not on the shard
name itself. See `docs/ci/gate-enforcement-contract.md` and
`docs/ci/merge-ready-protocol.md`.

## Out of scope here

- Resolving the four-way `merge-tree` overlap on `xtask/tests/activation.rs` —
  expected until the order above is followed.
- Self-hosted `Validate CI policy ledgers` cancellation (#16832).
- Shared `ripr+ New Gap Gate` infra-no-proof reds (#17451).
