#!/usr/bin/env bash
set -euo pipefail

OPS_DIR="${OPS_DIR:-.ops-perl-lsp}"
METRICS_FILE="${OPS_DIR}/swarm-metrics.jsonl"

mkdir -p "${OPS_DIR}"

INPUT="$(cat)"

payload_field() {
  local query="$1"
  echo "${INPUT}" | jq -r "${query}" | tr -d '\r'
}

TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
AGENT_NAME="$(payload_field '.subagent_name // .agent_name // .teammate_name // "unknown"')"
AGENT_TYPE="$(payload_field '.subagent_type // .agent_type // .matcher // "unknown"')"
# Prefer cwd (platform-provided) over worktree_path (not in platform payload)
WORKTREE_PATH="$(payload_field '.cwd // .worktree_path // .path // empty')"
SESSION_ID="$(payload_field '.session_id // empty')"

jq -nc \
  --arg ts "${TIMESTAMP}" \
  --arg event "subagent_stop" \
  --arg agent_name "${AGENT_NAME}" \
  --arg agent_type "${AGENT_TYPE}" \
  --arg worktree_path "${WORKTREE_PATH}" \
  --arg session_id "${SESSION_ID}" \
  '{ts:$ts,event:$event,agent_name:$agent_name,agent_type:$agent_type,worktree_path:$worktree_path,session_id:$session_id}' >> "${METRICS_FILE}"

# Resolve the issue number for plan-reviewer agents without guessing from a
# generic worktree name. Explicit environment/payload values take precedence.
resolve_plan_review_issue_num() {
  local resolved="${ISSUE_NUMBER:-}"
  if [[ -n "${resolved}" ]]; then
    printf '%s\n' "${resolved}"
    return
  fi

  resolved="$(payload_field '.issue_number // empty' 2>/dev/null || true)"
  if [[ -n "${resolved}" ]]; then
    printf '%s\n' "${resolved}"
    return
  fi

  if [[ "${AGENT_NAME}" =~ ^plan-review-([1-9][0-9]*)$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return
  fi

  if [[ "${AGENT_NAME}" =~ ^plan-reviewer-([1-9][0-9]*)$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
  fi
}

# Plan-reviewer completions must leave a terminal issue state. The gate is
# intentionally limited to plan-reviewer agents and accepts either label used
# by the workflow: builder-ready or already-fixed.
if [[ "${AGENT_TYPE}" == *"plan-reviewer"* ]]; then
  RESOLVED_ISSUE_NUM="$(resolve_plan_review_issue_num)"

  if [[ -z "${RESOLVED_ISSUE_NUM}" ]] || ! [[ "${RESOLVED_ISSUE_NUM}" =~ ^[1-9][0-9]*$ ]]; then
    echo "subagent-stop: plan-reviewer completed but no valid ISSUE_NUMBER is set." >&2
    echo "  Set ISSUE_NUMBER=<n>, include issue_number in the agent JSON payload, or use the canonical plan-review-NNN agent name." >&2
    exit 3
  fi

  LABELS="$(gh issue view "${RESOLVED_ISSUE_NUM}" --json labels --jq '[.labels[].name] | join(",")' 2>/dev/null || true)"
  if [[ -n "${LABELS}" ]] && [[ "${LABELS}" != *"builder-ready"* ]] && [[ "${LABELS}" != *"already-fixed"* ]]; then
    echo "Plan review incomplete: issue #${RESOLVED_ISSUE_NUM} does not have builder-ready or already-fixed label." >&2
    echo "Add the label before completing: gh issue edit ${RESOLVED_ISSUE_NUM} --add-label builder-ready" >&2
    exit 2
  fi
fi

exit 0
