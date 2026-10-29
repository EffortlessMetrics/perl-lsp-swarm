#!/usr/bin/env bash
# Guard the existing RIPR receipt inputs; no branch name is semantic authority.
set -euo pipefail

not_proven() {
  printf 'RIPR subject NOT_PROVEN: %s\n' "$1" >&2
  exit 1
}

for name in RIPR_BASE_SHA RIPR_EVENT_HEAD_SHA RIPR_EVALUATED_SHA; do
  value="${!name:-}"
  [[ "$value" =~ ^[0-9a-f]{40}$ && "$value" != 0000000000000000000000000000000000000000 ]] ||
    not_proven "$name must supply an exact nonzero commit SHA"
  if ! git cat-file -e "$value^{commit}" 2>/dev/null; then
    # Fetch only the declared object, with a bounded refusal on unavailability.
    # In particular, never repair a missing event base by fetching moving main.
    timeout --signal=TERM --kill-after=5s 30s git fetch --no-tags --depth=1 origin "$value" ||
      not_proven "$name object unavailable after exact-SHA fetch"
  fi
  actual="$(git rev-parse --verify "$value^{commit}")" ||
    not_proven "$name commit unavailable"
  [[ "$actual" == "$value" ]] || not_proven "$name does not name a commit object"
done

checkout="$(git rev-parse --verify HEAD^{commit})" || not_proven "checkout commit unavailable"
[[ "$checkout" == "$RIPR_EVALUATED_SHA" ]] ||
  not_proven "checkout differs from the event integration commit"

case "${GITHUB_EVENT_NAME:-}" in
  pull_request)
    [[ "${PR_HEAD_SHA:-}" == "$RIPR_EVENT_HEAD_SHA" ]] ||
      not_proven "raw PR head differs from the event head"
    # Read the commit header itself: shallow traversal can hide existing parents.
    # Drain the message too so a large body cannot SIGPIPE git under pipefail.
    parents="$(git cat-file commit "$RIPR_EVALUATED_SHA" |
      awk 'BEGIN { header=1 } /^$/ { header=0 } header && /^parent / { printf "%s%s", separator, $2; separator=" " }')" ||
      not_proven "integration parents unavailable"
    read -r -a parent_shas <<< "$parents"
    [[ "${#parent_shas[@]}" -eq 2 ]] ||
      not_proven "integration commit must have exactly two parents"
    [[ "${parent_shas[1]}" == "$RIPR_EVENT_HEAD_SHA" ]] ||
      not_proven "integration second parent differs from the event head"
    # Retain event A as provenance. The PR producer authenticates parent P
    # separately for the P...E integration comparison.
    git merge-base --is-ancestor "$RIPR_BASE_SHA" "${parent_shas[0]}" ||
      not_proven "integration base ancestry unavailable or contradictory"
    ;;
  push|merge_group|workflow_dispatch)
    [[ "$RIPR_EVALUATED_SHA" == "$RIPR_EVENT_HEAD_SHA" && -z "${PR_HEAD_SHA:-}" ]] ||
      not_proven "evaluated commit differs from the declared event head"
    ;;
  *) not_proven "unsupported or missing event kind" ;;
esac

printf 'RIPR subject verified: base=%s event_head=%s evaluated=%s\n' \
  "$RIPR_BASE_SHA" "$RIPR_EVENT_HEAD_SHA" "$RIPR_EVALUATED_SHA"
