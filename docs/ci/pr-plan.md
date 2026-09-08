# PR Plan

Advisory CI economics forecast. Runs once per PR via
[`.github/workflows/pr-plan.yml`](../../.github/workflows/pr-plan.yml) and writes
`target/ci/ci-plan.json` + a step summary.

> Companion: [lem-budgeting.md](lem-budgeting.md), [labels.md](labels.md),
> [trigger model](#trigger-model-and-sha-like-head-branch-suppression).

---

## Trigger model and SHA-like head-branch suppression

PR Plan triggers **solely** on `pull_request_target`
(`.github/workflows/pr-plan.yml`). Two properties of that trigger are load
bearing here:

- The `branches: [master, main]` filter selects the pull request **base**
  branch. It plays no role in the suppression described below.
- GitHub documents for `pull_request_target`: *"Branches with names that match
  certain patterns (such as those which look similar to SHAs) may not trigger
  workflows."* A suppressed event never starts a run, so the workflow cannot
  detect or signal its own absence — there is no in-workflow fallback.

The suppression matcher itself is not published, so whether any concrete head
name is actually suppressed is externally unverifiable (`NOT_PROVEN`);
everything on this page fails closed rather than claiming the gap is proven
reachable or unreachable.

Suppression detection therefore lives **outside** the suppressed event, in
[`.github/workflows/pr-plan-head-name-guard.yml`](../../.github/workflows/pr-plan-head-name-guard.yml)
(#6238):

- It runs on the `pull_request` event, so it fires wherever GitHub actually
  emits that event for `pull_request_target`-suppressed head names — with one
  documented exception below.
- It classifies only the event payload string `pull_request.head.ref`, reached
  through an env-indirect expression (never inline `github.event.*` inside run
  syntax). No checkout of any ref, no actions, no scripts executed from any
  ref, no secrets, no write permissions.
- It holds single-producer discipline: it never writes plan artifacts or
  conclusions; PR Plan remains the only `ci-plan.json` producer.
- Its own `branches:` filter also selects the base branch and does not rescue
  anything by itself — the guard works because its *event* fires in place of
  the suppressed one.
- Accepted silence boundary: `pull_request` workflows do not run while a pull
  request has a merge conflict. In the combined case (SHA-like head name and a
  conflicted pull request) both PR Plan and this guard stay silent; this residue
  stands until the branch-naming ruleset closes the class (#6238 follow-up).
- The classifier over-approximates the class with
  `^[0-9a-fA-F]{7,40}$` (7–40 hex characters). Since the real matcher is
  unknown, the sentinel fails closed loud: an over-approximated hit produces a
  red check with rename guidance even when GitHub would in fact have run PR
  Plan. Prefixed names like `agent/parser-fix` are unaffected.
- Rename-only repairs fire no events (branch renames produce no event), and a
  push of an unchanged tip is a `synchronize`-less no-op, so the guidance
  explicitly says **rename, then land a new commit** — an empty
  `git commit --allow-empty` suffices — or close and reopen the pull request;
  that event re-runs both workflows.
- Like PR Plan, the guard is advisory: it owns no required check and gates no
  merge. Per-PR concurrency mirrors `pr-plan.yml`'s grouping semantics.

Residual boundary: live suppression of real SHA-like head names has not been
exercised with a fixture event on this repository (proof would require opening
a throwaway SHA-like-named PR); the live-fire verification procedure and its
current status are tracked on #6238. The planned durable closure is a
maintainer-gated branch-naming ruleset making SHA-like heads unreachable by
construction (#6238 follow-up); running planner logic from PR-supplied
definitions was rejected on #6003 grounds and is out of scope here.

---

## What it does

1. Reads `policy/ci-budget.toml`, `policy/ci-lanes.toml`,
   `policy/ci-risk-packs.toml`, and `policy/trust-lanes.toml`.
2. Computes changed files via `git diff --name-only $BASE...$HEAD`.
3. Classifies the diff into risk packs (parser, LSP, retained-state, etc).
4. Selects lanes from three independent sources, recording the **origin** of
   each selection:
   - `default-pr` — every lane with `default_pr = true` is selected for
     non-docs PRs (`docs_gate` is selected only for docs-only PRs).
   - `risk-pack:<id>` — lanes pulled in by a matched risk pack.
   - `label:<name>` — lanes pulled in by labels (`ci:*`, `full-ci`, etc).
   - `deep-lane:full-ci` — `deep_lanes` from matched risk packs when
     `full-ci` is set.
5. **Honors lane `paths:` filters.** A lane that has a `paths` field is only
   counted toward LEM when at least one changed file matches. Lanes that
   would have been selected but are skipped by paths-filter are reported in
   the `skipped_lanes` section so contributors can see they were considered.
6. Sums `base_lem` per selected lane.
7. Classifies the diff against advisory trust-lane classes from
   `policy/trust-lanes.toml`, recording the strongest class, required proof,
   skipped-by-policy checks, widening triggers, support-claim impact, and
   hosted-CI estimate.
8. Emits the band: `default` / `elevated` / `high` / `over_ceiling`.

---

## Output schema

```json
{
  "schema_version": 1,
  "repo": "perl-lsp",
  "base_sha": "abc",
  "head_sha": "def",
  "labels": ["full-ci"],
  "posture": "rust",
  "budget": {
    "estimated_lem": 42.0,
    "band": "elevated",
    "default_limit_lem": 35,
    "elevated_limit_lem": 75,
    "hard_limit_lem": 125,
    "estimated_usd": 0.336
  },
  "changed": {
    "files": ["..."],
    "areas": ["parser"],
    "docs_only": false
  },
  "selection": {
    "risk_packs": ["parser"],
    "lanes": [
      {"id": "pr_smoke", "intent": "frontdoor mechanical proof",
       "runner": "ubuntu_24_04", "base_lem": 4,
       "default_pr": true, "blocking": true}
    ]
  },
  "trust_lanes": {
    "schema_version": 1,
    "policy": "trust-lanes",
    "status": "advisory",
    "spec": "docs/specs/PLSP-SPEC-0011-trust-lane-ci-routing.md",
    "strongest_class": {
      "id": "parser_runtime_fix",
      "risk_rank": 50,
      "claim_boundary": "Changes parser, lexer, AST, token, POD, regex, or source-position runtime behavior.",
      "required_checks": ["focused parser runtime tests for the changed grammar family"],
      "skipped_by_policy_checks": ["release proof unless release files changed"],
      "widening_triggers": ["AST shape changes"],
      "support_claim_impact": "Parser bucket or compatibility claims require fresh generated status evidence."
    },
    "changed_surface": ["parser, lexer, or parser-core runtime source"],
    "hosted_ci_estimate": {
      "estimated_lem": 42.0,
      "band": "elevated",
      "selected_lanes": 5
    }
  },
  "warnings": []
}
```

---

## What it is not

- **Not blocking.** PR 04 lands the planner advisory-only. Hard ceiling guard
  arrives in PR 13.
- **Not a billing source.** `estimated_usd` is display only.
- **Not a runtime gate.** It does not trigger or skip downstream workflows. PR 12
  adds `xtask ci plan` which downstream lanes can read for `if:` conditions.
- **Not support proof.** Trust-lane classification explains which proof a PR
  should buy for its claim class; it does not prove provider behavior or
  promote support tiers.

---

## Running locally

```bash
python3 scripts/ci/pr_plan.py \
  --base origin/master --head HEAD \
  --json-out target/ci/ci-plan.json
cat target/ci/ci-plan.json | jq .budget
```

---

## Roadmap

| PR | Change | Status |
|---:|---|---|
| 04 | Python prototype, advisory only. | landed |
| 07 | Lane origins + paths-filter + ripr summary section. | landed |
| 12 | Replace prototype with `cargo xtask ci plan`. | deferred |
| 13 | Hard-ceiling guard above 125 LEM. | landed |
| 16 | Aggregator + consumer scripts; PR Plan reads `.ci/metrics/ci-lane-history.json` when present. | landed |
| — | Trigger-model correction (base-side `branches` filter, documented SHA-like `pull_request_target` suppression) plus the advisory head-name sentinel; completes the Option-3 documentation deferred by #6286. (#6238) | landed |

The planner now consumes learned LEM estimates whenever the history file has
`learned: true` for a lane (≥ 5 samples in the rolling window). Sampled lanes
substitute `p50 × 1.15` (clamped to the static floor) for the static `base_lem`,
and the plan's `learned` block reports `lanes_using_learned` and
`delta_lem_vs_static`.
