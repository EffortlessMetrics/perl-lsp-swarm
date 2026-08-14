# P6 — upgrade staged consumer to published RIPR 0.11.0

Issue: #9118  
Parent: #9112  
Depends on: #9117 / PR #9125  
Release dependency: published `EffortlessMetrics/ripr` v0.11.0  
Branch: `agent/ripr-011-upgrade`

## End goal

Make the post-cutover staged RIPR consumer use the published 0.11.0 source release while preserving the exact staged proof contract. This is a producer-version compatibility PR, not another staged-tree architecture change. If 0.11.0 is not yet published, keep this PR draft and do not substitute a ripr-swarm commit, protected candidate ref, Git dependency, or locally built binary as repository policy.

## Expected starting state

```text
staged RIPR precommit = authoritative/blocking
exact subject = planner TREE_OID + frozen candidate bridge
reviewed RIPR version authority = 0.10.0
dedicated PR RIPR workflow = retired
badge/main measurement = independent consumer of reviewed version
```

## Codex implementation order

1. Confirm the actual published `ripr v0.11.0` release identity from the source repository. Do not proceed from the frozen swarm candidate alone.
2. Change the single reviewed RIPR version authority from 0.10.0 to 0.11.0 and let all governed consumers follow it.
3. Capture real 0.10 and real 0.11 output for the compatibility matrix below. Exercise every field the repository consumes rather than trusting `schema_version` alone.
4. Update the parser/normalizer only for producer changes demonstrated by those fixtures. Preserve fail-closed behavior on absent/malformed/unrecognized required state.
5. Check whether 0.11 introduces typed complete/partial/limited/gate-eligibility fields. Any producer-declared incomplete or ineligible result must become `NOT_PROVEN`, never PASS.
6. Re-run same-subject staged-versus-committed parity and the P3 subject-leakage falsifiers against 0.11.
7. Re-run the accepted latency methodology; do not weaken the semantic profile if 0.11 is slower.
8. Inspect the published bits for #3212, #3213, and #3237 capabilities, but do not remove any downstream workaround here. P7/#9119 owns evidence-backed cleanup.
9. Keep the sandbox bridge unless the released version already exposes the complete immutable-subject contract and a separate reviewed P7 slice removes the bridge.

## Real-output compatibility matrix

```text
complete clean staged diff
unsuppressed actionable gap
suppressed gap
deleted/base-side #3212 reproducer
test-harness #3213 reproducer
large/bounded diff state
producer error/limitation state
malformed fixture negative control
```

For each, compare path, line/currentness, classification, canonical gap identity, suppression behavior, completeness, and normalized gate posture.

## Semantic invariants

The staged gate still means:

```text
candidate = exact planner TREE_OID
mode = draft
unchanged tests = included
complete/trustworthy producer state required for PASS
```

Do not adopt a changed producer default if it weakens these semantics.

## Rolling work is not assumed present

The frozen 0.11 W7 candidate predates later rolling work automatically entering the release. Inspect the published release rather than assuming it contains:

- ripr-swarm #3212 — deleted/base-side currentness;
- ripr-swarm #3213 — test/evidence source-role classification;
- ripr-swarm #3237 — immutable Git-tree candidate input.

Even if a behavior appears fixed, workaround deletion is P7 and requires a discriminating removal test.

## Performance

Repeat P3's phase measurements and accepted thresholds. If 0.11 materially violates the commit-tier hard ceiling or changes the default into a partial/ineligible result, keep 0.10 as the reviewed version until the regression is understood. Do not make the gate weaker to force the upgrade.

## Mandatory controls

- after authority says 0.11, a 0.10 binary is `NOT_PROVEN`;
- malformed/new output shape cannot become clean;
- producer partial/ineligible state cannot become PASS;
- staged bad source + unstaged fix remains blocked;
- suppression-positive and unsuppressed-negative controls remain discriminating;
- badge/main and precommit cannot drift to different versions;
- current 0.10 containment stays in place even if a newer version appears to make it redundant.

## Guardrails

- No ripr-swarm Git dependency.
- No forcing rolling changes into 0.11.
- No sandbox/currentness/suppression cleanup in this PR.
- No unrelated CI redesign.

## Acceptance before merge

- published 0.11.0 is the single reviewed consumer version;
- real 0.10→0.11 schema/meaning differences are dispositioned;
- complete/incomplete state maps fail-closed;
- exact staged proof semantics and parity remain green;
- commit-tier latency remains accepted;
- badge/main and precommit version identity remains singular;
- P7 remains the only owner of workaround removal.

## Suggested review map

Review release identity and version authority first, real producer-output diffs second, parser changes third, parity/latency evidence fourth. Treat any cleanup of legacy containment as a scope violation unless moved to P7.
