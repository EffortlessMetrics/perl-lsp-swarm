# P2 — exact staged RIPR shadow evaluator

Issue: #9114  
Parent: #9112  
Depends on: #9113 / PR #9121  
Branch: `agent/ripr-staged-shadow`

## End goal

Teach the canonical commit gate to evaluate RIPR 0.10 against the exact staged candidate captured by `GatePlan.staged_tree_oid`, while keeping the result advisory/shadow-only. The implementation must prove that every authoritative source, test, config, and suppression byte comes from the frozen candidate tree rather than the dirty worktree or a later live-index read. RIPR remains the evidence producer and `xtask` remains the repository policy authority. No commit may be blocked by RIPR in this PR.

## Codex implementation order

1. Read #3786, `xtask/src/tasks/staged.rs`, `xtask/src/tasks/commit_checks.rs`, the Changie staged sandbox, and `xtask/src/tasks/ripr_evidence.rs` before changing code.
2. Identify the single existing commit-gate extension point. Add one staged RIPR check there; do not create a second gate engine or shell hook.
3. Reuse the planner-provided `TREE_OID`. If a helper currently recomputes `git write-tree`, change the call graph so this check accepts the already-bound identity instead of capturing a second subject.
4. Resolve the base using the existing staged substrate, including unborn HEAD. Do not hardcode the SHA-1 empty tree.
5. Build a reusable frozen-tree sandbox helper under the staged/commit-check substrate if one does not already exist. Materialize only Git-tree bytes; never copy authoritative files from the worktree.
6. Generate a unified `base -> TREE_OID` diff from the same immutable identity.
7. Resolve a local `ripr` binary and verify the expected 0.10.0 version. Never install or access the network from precommit.
8. Run `ripr check --root <frozen-root> --diff <diff> --mode draft --format json` with a bounded subprocess.
9. Refactor existing RIPR normalization so staged and CI overlap can consume the same parser, suppression semantics, actionable-gap rules, and deleted-side containment.
10. Emit a typed shadow receipt but preserve the pre-existing commit exit status regardless of RIPR findings/tool availability.

## Exact subject law

```text
base      = HEAD or repository-native empty-tree identity
candidate = GatePlan.staged_tree_oid
diff      = base -> candidate
root      = filesystem reconstructed from candidate only
config    = candidate/ripr.toml
policy    = candidate/policy/ripr-suppressions.toml
mode      = draft
unchanged tests = included
```

A staged diff evaluated against `--root .` is a correctness defect.

## Expected implementation seams

Likely touch points include:

```text
xtask/src/tasks/commit_checks.rs
xtask/src/tasks/staged.rs
xtask/src/tasks/ripr_evidence.rs
new commit-check/staged helper modules where SRP warrants them
focused commit-check tests
focused RIPR fixture(s)
docs/how-to/PRE_COMMIT.md only if shadow behavior needs operator disclosure
```

Prefer small internal modules over making `commit_checks.rs` or `ripr_evidence.rs` materially larger.

## Shadow receipt fields

At minimum retain:

```text
tree_oid
base_identity
diff_digest
expected_ripr_version
observed_ripr_version
mode
config_digest
suppression_digest
applicable
analysis_status
raw_findings
suppressed_findings
actionable_new_gaps
elapsed_ms
```

Use existing typed receipt/result vocabulary where possible instead of inventing an isolated schema.

## Required falsifiers in this PR

- staged bad production source + unstaged source repair: staged defect remains visible;
- staged focused test + unstaged weaker test: staged test controls evidence;
- staged `ripr.toml` differs from worktree config: staged config wins;
- live index changes after planner capture: original tree remains the subject;
- missing/wrong-version binary is reported and never self-installs;
- malformed producer JSON cannot become zero findings;
- deleted candidate path does not require copying that path from the worktree;
- docs-only staged change does not spawn RIPR.

## Guardrails

- Shadow only: no new commit failure from RIPR.
- Keep `.github/workflows/ripr.yml` untouched and required.
- Keep `mode=draft` and unchanged-test evidence.
- Keep current `HeadLineExtents`-equivalent containment and current suppressions.
- No result cache optimization yet beyond existing generic gate behavior.
- Do not depend on ripr-swarm #3237 landing.

## Acceptance before merge

- exact frozen staged candidate is the only authoritative subject;
- no dirty-worktree or later-index leak can influence evidence;
- real RIPR 0.10 output flows through shared repository normalization;
- tool/config/policy identities are recorded;
- shadow findings are visible but cannot fail a commit;
- no network acquisition occurs during precommit;
- current PR RIPR CI continues unchanged for overlap.

## Suggested review map

Review identity binding and sandbox construction first, producer invocation second, normalization reuse third, then fixtures. Any code path that reads `.` or the live index after `TREE_OID` binding should be treated as suspicious until proven non-authoritative.
