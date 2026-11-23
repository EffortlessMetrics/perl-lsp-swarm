# Review convergence core output (`convergence_core.v1`)

Machine-readable JSON emitted as the **final line of stdout** by
`scripts/ci/check-pr-review-convergence-core`. Human-readable summary lines precede
the JSON object on stdout; stderr carries `BLOCK` / `WARN` lines.

**Producer:** `scripts/ci/check-pr-review-convergence-core`  
**Schema id:** `convergence_core.v1` (string in `schema_version`)  
**Stability:** additive changes require a new schema id; field renames or removals
require a new schema id. Consumers must refuse unknown ids.

**Related envelopes:**

| Envelope | `schema_version` / field | Notes |
|----------|--------------------------|-------|
| Core facts (this doc) | `schema_version: "convergence_core.v1"` | Paginated GitHub review facts |
| Public wrapper | `wrapper_schema_version: "pr_review_convergence.v2"` | Adds classification; merges core via `$core + {...}` |
| `scripts/reviews/state` | reads core via wrapper | Warns on missing core `schema_version`; refuses unknown |
| `scripts/ci/check-pr-claim-currentness` | — | **Retired** for live PRs; fixture-only historical parser |

---

## Version field

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `schema_version` | string | Must be `"convergence_core.v1"` for this contract. Emitted as the **first** object key so callers can short-circuit before parsing remaining fields. | Immutable for v1; bump the string (e.g. `convergence_core.v2`) for breaking shape changes. |

Override for ad-hoc tests only: `CONVERGENCE_CORE_SCHEMA_VERSION` env var on the core script.

---

## Identity and verdict

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `pr` | number | Pull request number evaluated. | stable |
| `headRefOid` | string | GitHub `headRefOid` observed for the evaluation. | stable |
| `converged` | boolean | Native review-fact convergence at `headRefOid` (threads, requests, stale human reviews, etc.). Does **not** imply semantic review currentness. | stable |

---

## Review requests and change requests

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `pending_reviewers` | string[] | GitHub-native pending review requests (blocking while non-empty). | stable |
| `independent_review_pending` | boolean | Compatibility alias derived from `pending_reviewers`. | stable |
| `current_change_requests` | object[] | Current-head `CHANGES_REQUESTED` human reviews (blocking). | stable |
| `review_decision` | string | GitHub `reviewDecision` when present. | stable |
| `is_draft` | boolean | PR draft flag from GitHub. | stable |

---

## Human review inventory

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `stale_reviews` | object[] | Human reviewers whose latest review predates `headRefOid` (**blocking**). | stable |
| `stale_bot_reviews` | object[] | Bot/auto-app reviewers stale vs HEAD (**advisory**, never blocking). | stable |
| `current_human_reviews` | object[] | Latest human review submissions on current head. | stable |
| `dismissed_human_reviews` | object[] | Dismissed human reviews in the latest-per-user projection. | stable |
| `human_review_count` | number | Count in the human review inventory used by the core derivation. | stable |
| `current_human_review_count` | number | Subset count for current-head human reviews. | stable |
| `dismissed_human_review_count` | number | Dismissed human review count. | stable |

---

## Review threads

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `unresolved_active` | number | Unresolved review threads on current diff (**blocking**). | stable |
| `unresolved_outdated` | number | Unresolved outdated threads (**blocking**). | stable |
| `unresolved_total` | number | Sum of active + outdated unresolved threads. | stable |
| `resolved_threads` | number | Resolved thread count. | stable |
| `resolved_without_disposition` | number | Resolved threads with no disposition reply (**blocking**). | stable |

---

## Protocol, receipts, and follow-ups

| Field | Type | Semantics | Stability |
|-------|------|-----------|-----------|
| `review_protocol_enforce` | boolean | Whether R1 protocol axes were enforced vs advisory for this run. | stable |
| `review_runs_in_flight` | number | Observed in-flight review automation runs. | stable |
| `verification_runs_in_flight` | number | Observed in-flight verification runs. | stable |
| `deep_review_receipt_head_match` | boolean | Legacy receipt head observation (compatibility). | stable |
| `verification_receipt_head_match` | boolean | Legacy verification receipt head observation. | stable |
| `dispositions_missing_marker` | number | Threads missing required disposition markers when protocol enforced. | stable |
| `followups_without_issue` | number | Follow-up comments without linked issues when protocol enforced. | stable |
| `unreachable_fix_commits` | number | Fix commits referenced but not reachable from head when protocol enforced. | stable |

---

## Consumer requirements

Live consumers of the **core** envelope must:

1. Read `schema_version` before interpreting other fields.
2. **Warn** (stderr) and continue when `schema_version` is missing (pre-#15278 collectors).
3. **Refuse** (non-zero exit, structured `NOT_PROVEN`) when `schema_version` is present but not supported.

Implemented today:

- `scripts/reviews/state`
- `scripts/ci/check-pr-review-convergence` (wrapper gate before merging `$core`)

Campaign worker agents should consume the public wrapper or `scripts/reviews/state`, not re-parse GraphQL directly.

---

## Exit codes (core script)

| Code | Meaning |
|------|---------|
| `0` | Converged at head |
| `1` | Not converged (see stderr `BLOCK` lines) |
| `2` | Usage, tooling, or GitHub fetch failure |
