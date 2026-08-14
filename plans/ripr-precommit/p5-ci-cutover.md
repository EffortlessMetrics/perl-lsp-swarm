# P5 — retire dedicated RIPR PR CI after staged-gate cutover

Issue: #9117  
Parent: #9112  
Depends on: #9116 / PR #9124  
Branch: `agent/ripr-ci-cutover`

## End goal

Remove `.github/workflows/ripr.yml` and its required `ripr+ New Gap Gate` status only after P4 has made the exact staged RIPR check authoritative and blocking. Preserve remote RIPR only where it performs a different job, especially main-branch repo-wide badge/measurement generation. Move reviewed RIPR version authority out of the workflow being deleted so precommit, badge automation, docs, and compatibility tests cannot drift.

## Start condition

Do not execute the cutover until:

- P4 is landed and the canonical precommit path blocks new actionable staged gaps;
- P3 parity/falsifier/performance evidence is accepted;
- required tool/instrument failures are `NOT_PROVEN`, not warnings;
- the live GitHub required-context/ruleset state is known and can be changed deliberately.

Keep this PR draft if any condition is missing.

## Codex implementation order

1. Inventory every file, test, doc, and policy record whose authority is currently `.github/workflows/ripr.yml` or `ripr+ New Gap Gate`.
2. Identify whether the repo already has a reviewed external-tool-version authority suitable for RIPR. Reuse it if appropriate. If not, introduce the smallest RIPR-specific authority; do not build a generic package manager.
3. Make precommit expected version, badge/main RIPR installation, setup guidance, and version-contract tests consume/check that single authority.
4. Replace `badge_ripr_version_contract.rs`'s dependency on the soon-to-be-deleted PR workflow with the new reviewed authority.
5. Replace workflow-presence/new-gap-workflow tests with recurrence guards for the staged authority.
6. Remove `.github/workflows/ripr.yml` and code/tests/docs used only by that dedicated routed PR lane.
7. Remove `ripr+ New Gap Gate` from `.ci/policies/required-checks.toml` and generated/checked required-context inventory.
8. Update docs to state the intentional claim boundary: precommit owns RIPR new-gap construction control; ordinary CI owns compile/test/integration; badge/main automation may still run repo-wide RIPR.
9. Coordinate the live GitHub ruleset/branch-protection change before the workflow deletion becomes merge-critical. The repo must not require a status context that no workflow can produce.
10. Record concrete removed runtime/complexity surface without inventing unmeasured dollar savings.

## Required cutover order

```text
P4 blocking staged gate proven
→ prepare repo-side P5 changes
→ inspect live required-context authority
→ remove `ripr+ New Gap Gate` from live settings
→ verify live settings == repo policy
→ merge workflow/policy/test/docs deletion
```

Do not reverse the middle steps.

## Version authority requirements

One reviewed RIPR version identity must govern/check:

```text
precommit expected binary version
badge/main workflow installation version
docs/setup guidance
real-output compatibility fixtures/tests
```

The badge workflow must not become a second version authority after the PR workflow disappears.

## Expected repository changes

```text
.github/workflows/ripr.yml                         delete
.ci/policies/required-checks.toml                  remove RIPR context
.github/workflows/badge-endpoints.yml              consume reviewed version authority
xtask/tests/badge_ripr_version_contract.rs         rewrite authority
xtask/tests/ripr_new_gap_gate_workflow.rs          retire/replace
quality/CI wiring recurrence tests                 update
PRE_COMMIT + RIPR/CI architecture docs             update
minimal reviewed RIPR version authority            add/reuse
workflow-only routing/artifact helpers if orphaned remove
this plan file
```

## Recurrence guards

Prove after cutover that:

- `cargo xtask precommit` selects staged RIPR for relevant staged Rust changes;
- expected RIPR version is shared by precommit and badge/main consumers;
- required-check inventory does not name a deleted RIPR status;
- no dedicated required PR RIPR workflow is silently reintroduced;
- badge automation remains main/push scoped according to its separate contract.

## Bypass claim boundary

Document explicitly that `git commit --no-verify` can bypass the local RIPR check. It is a checkpoint escape, not proof. The project intentionally no longer reconstructs the same RIPR new-gap predicate as a required PR status after this cutover. Do not hide that tradeoff by moving the same gate into another required workflow.

## Cost/complexity closeout

Record the concrete surface removed:

```text
runner router
CX53/CX43/GitHub fallback lanes
PR-time cargo install/version cache
60/75 minute execution envelopes
PR RIPR artifact uploads
required aggregate status context
workflow-specific recurrence tests
```

Do not state a universal dollar saving without measurements.

## Guardrails

- No 0.11 bump in P5.
- No removal of badge/main repo-wide RIPR measurement.
- No `HeadLineExtents` or test-harness suppression cleanup.
- No disguised replacement required RIPR workflow.
- No unrelated required-check changes.

## Acceptance before merge

- staged RIPR is already blocking;
- dedicated `ripr.yml` is gone;
- repo policy and live GitHub settings no longer require `ripr+ New Gap Gate`;
- remaining required checks are producible/current;
- one reviewed RIPR version authority drives/checks precommit + badge/docs;
- old workflow tests are replaced by staged-authority recurrence guards;
- bypass and remote-proof boundaries are documented;
- badge/main measurement remains functional.

## Suggested review map

Review live-required-context sequencing first, version-authority migration second, deleted workflow/runtime surface third, and recurrence tests/docs last. Any hidden remote reimplementation of the same new-gap gate defeats the purpose of P5.
