# P7 — consume producer-native staged identity and retire downstream workarounds

Issue: #9119  
Parent: #9112  
Depends on: #9118 / PR #9126  
Upstream: ripr-swarm #3212, #3213, #3237 in a published RIPR release  
Branch: `agent/ripr-native-cleanup`

## End goal

Remove consumer-owned compensation only after released RIPR owns the corresponding truth. This PR is deliberately blocked on published producer capabilities, not on a version number alone. The three cleanup authorities are independent: immutable candidate input may ship before deleted-side currentness or test-role classification, and each workaround must remain until its own reproducer passes without it.

## Start condition

Keep this PR draft until a published RIPR release demonstrably contains at least one of:

- #3237 immutable Git-tree candidate input;
- #3212 deleted/base-side currentness disposition;
- #3213 test/evidence source-role classification.

If capabilities land in separate releases, split this draft into multiple narrow PRs rather than combining unrelated version bumps and workaround removals.

## Slice A — producer-native immutable subject

Replace the 0.10/0.11 bridge:

```text
planner TREE_OID
→ consumer frozen-tree sandbox
→ consumer base->candidate diff
→ ripr check --root sandbox --diff diff
```

with the released producer-native equivalent:

```text
planner base identity + exact planner TREE_OID
→ RIPR immutable candidate analysis
```

Requirements:

- outer `GatePlan.staged_tree_oid` remains transaction authority;
- RIPR consumes that exact identity rather than rereading the live index;
- candidate source/tests/config remain tree-bound;
- `draft` + unchanged-test semantics remain unchanged;
- staged/committed parity remains green;
- later index/worktree mutations remain falsifiers;
- remove consumer materialization/diff code only after equivalence proof.

A bare producer `--staged` mode is insufficient if it captures a new mutable index snapshot.

## Slice B — retire deleted-side containment only after #3212

Remove `HeadLineExtents`-style filtering only when the old downstream reproduction passes producer-natively with no consumer filter.

Mandatory controls:

```text
deleted function tail
whole-file delete
reused candidate line number with different expression
rename/move
real candidate-side actionable gap in same diff
unresolved/malformed currentness state
```

The producer must distinguish candidate-current from base-deleted/moved/unresolved subjects strongly enough that impossible repair obligations cannot become blocking. A real current candidate gap must remain blocking.

## Slice C — remove test-harness suppressions only after #3213

Inventory suppressions whose **sole** reason is test/evidence plumbing being treated as production. For each candidate suppression:

1. remove only that suppression in the reproducer;
2. rerun the released producer;
3. prove test/receipt/contract harness code remains usable as evidence without creating production obligations;
4. prove an inadequately-tested production control still produces the expected gap;
5. delete the suppression only if its original rationale is gone.

Do not remove suppressions that also cover independent activation/infection/call-presence/static-tracing limits.

## Version discipline

If consuming these producer capabilities requires a version bump beyond 0.11.0:

- update the single reviewed version authority;
- capture real before/after producer output;
- fail closed on new partial/limited/ineligible states;
- keep badge/main and precommit aligned;
- remeasure latency;
- separate the version bump from large workaround deletions when reviewability suffers.

## Mandatory negative controls

- producer-native subject reads unstaged repair → fail;
- producer-native subject rereads later index snapshot → fail;
- removing currentness filter reintroduces deleted-line blocker → fail;
- removing test-role suppression makes `Ok(())`, `?`, `map_err`, assertion/receipt scaffolding a production obligation → fail;
- suppression cleanup hides a real production gap → fail;
- producer partial/ineligible state becomes PASS → fail.

## Expected cleanup targets

Only when their authority is earned:

```text
consumer frozen-tree materialization bridge
consumer-generated staged diff bridge
HeadLineExtents/currentness containment
#3213-only test/receipt/contract-harness suppressions
bridge-only fixtures/tests superseded by producer-native identity tests
```

Retain generic staged-subject falsifiers permanently; they protect the integration even after the producer improves.

## Guardrails

- No forcing upstream rolling changes into a release.
- No mass suppression purge.
- No weakening actionable-gap policy.
- No reintroduction of required PR RIPR CI.
- No compile/Clippy/test proof-tier changes.

## Acceptance before merge

- every removed workaround has a released producer authority and a discriminating reproducer;
- exact planner TREE_OID remains the staged transaction identity;
- staged/committed parity and mutation/index falsifiers remain green;
- deleted/base-side impossible targets cannot block;
- test harnesses stop generating production obligations while real production controls retain teeth;
- unrelated suppressions remain intact;
- commit-tier latency remains within accepted bounds.

## Suggested review map

Review each workaround removal as an independent claim: upstream release evidence, old reproducer without the workaround, positive production control, then deletion. Do not accept “newer RIPR version” as evidence by itself.
