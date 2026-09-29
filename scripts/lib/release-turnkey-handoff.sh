#!/usr/bin/env bash
# scripts/lib/release-turnkey-handoff.sh
#
# T02 (#16798) consumer of the `release_turnkey_transaction.v1` handoff states.
# This slice writes and validates the two manual/partial-merge records; it does
# not own the full #16797 validator, workflow-run correlation, or resume engine.
#
# Sourced by scripts/release-turnkey-pr.sh. Helpers take explicit arguments so
# tests can exercise them without GitHub mutation.

TURNKEY_SCHEMA_VERSION="release_turnkey_transaction.v1"
TURNKEY_STAGE_MANUAL_MERGE="manual_merge_required"
TURNKEY_STAGE_MERGE_REQUESTED="merge_requested_waiting_for_landing"
TURNKEY_EXIT_MANUAL_MERGE=2
TURNKEY_EXIT_MERGE_REQUESTED=4

turnkey_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
    return 0
  fi
  printf 'turnkey-handoff: no sha256sum or shasum available\n' >&2
  return 1
}

# Canonical semantic bytes exclude observation metadata (timestamps, paths).
turnkey_canonical_bytes() {
  jq -S '{
    schema_version,
    stage,
    authoritative,
    repository,
    target_branch,
    requested_version,
    initial_base_commit,
    pr,
    next_safe_action,
    wake_event,
    invalidators
  }'
}

turnkey_digest_of_record() {
  turnkey_canonical_bytes | turnkey_sha256
}

turnkey_attach_digest() {
  local record digest
  record="$(cat)"
  digest="$(printf '%s' "$record" | turnkey_digest_of_record)" || return 1
  printf '%s' "$record" | jq --arg digest "sha256:${digest}" '.digest = $digest'
}

# Build one handoff record. Digest is derived from canonical semantic bytes.
# Usage: turnkey_build_handoff <stage> <authoritative:true|false> \
#   <repository> <target_branch> <version> <base_sha> \
#   <pr_number> <pr_url> <pr_head> <pr_base> \
#   <next_safe_action> <wake_event> <invalidators_json_array>
turnkey_build_handoff() {
  local stage="$1"
  local authoritative="$2"
  local repository="$3"
  local target_branch="$4"
  local version="$5"
  local base_sha="$6"
  local pr_number="$7"
  local pr_url="$8"
  local pr_head="$9"
  local pr_base="${10}"
  local next_action="${11}"
  local wake_event="${12}"
  local invalidators="${13}"

  if [[ -z "$stage" || -z "$repository" || -z "$target_branch" || -z "$version" ]]; then
    printf 'turnkey-handoff: stage, repository, target_branch, and version are required\n' >&2
    return 1
  fi
  if [[ -z "$pr_number" || -z "$pr_head" || -z "$pr_base" ]]; then
    printf 'turnkey-handoff: PR number, head, and base are required\n' >&2
    return 1
  fi
  if [[ "$authoritative" != "true" && "$authoritative" != "false" ]]; then
    printf 'turnkey-handoff: authoritative must be true or false\n' >&2
    return 1
  fi

  jq -n \
    --arg schema "$TURNKEY_SCHEMA_VERSION" \
    --arg stage "$stage" \
    --argjson authoritative "$authoritative" \
    --arg repository "$repository" \
    --arg target_branch "$target_branch" \
    --arg version "$version" \
    --arg base_sha "$base_sha" \
    --argjson pr_number "$pr_number" \
    --arg pr_url "$pr_url" \
    --arg pr_head "$pr_head" \
    --arg pr_base "$pr_base" \
    --arg next_action "$next_action" \
    --arg wake_event "$wake_event" \
    --argjson invalidators "$invalidators" \
    '{
      schema_version: $schema,
      stage: $stage,
      authoritative: $authoritative,
      repository: $repository,
      target_branch: $target_branch,
      requested_version: $version,
      initial_base_commit: $base_sha,
      pr: {
        number: $pr_number,
        url: $pr_url,
        head: $pr_head,
        base: $pr_base
      },
      next_safe_action: $next_action,
      wake_event: $wake_event,
      invalidators: $invalidators
    }' | turnkey_attach_digest
}

# Write authoritative bytes only. Dry-run callers must not invoke this.
turnkey_write_authoritative() {
  local path="$1"
  local record="$2"
  local parent
  if [[ -z "$path" ]]; then
    printf 'turnkey-handoff: transaction path is required\n' >&2
    return 1
  fi
  if ! printf '%s' "$record" | jq -e '.authoritative == true' >/dev/null; then
    printf 'turnkey-handoff: refusing to persist a non-authoritative record at %s\n' "$path" >&2
    return 1
  fi
  parent="$(dirname -- "$path")"
  mkdir -p "$parent"
  printf '%s\n' "$record" >"$path"
}

turnkey_print_handoff() {
  local record="$1"
  local path="$2"
  local stage digest pr_number pr_head pr_base version next_action wake_event
  stage="$(printf '%s' "$record" | jq -r '.stage')"
  digest="$(printf '%s' "$record" | jq -r '.digest')"
  pr_number="$(printf '%s' "$record" | jq -r '.pr.number')"
  pr_head="$(printf '%s' "$record" | jq -r '.pr.head')"
  pr_base="$(printf '%s' "$record" | jq -r '.pr.base')"
  version="$(printf '%s' "$record" | jq -r '.requested_version')"
  next_action="$(printf '%s' "$record" | jq -r '.next_safe_action')"
  wake_event="$(printf '%s' "$record" | jq -r '.wake_event')"

  printf '[release] typed handoff: %s\n' "$stage"
  printf '[release] this is not a release failure and not completed preparation\n'
  printf '[release] requested version: %s\n' "$version"
  printf '[release] PR: #%s (head %s, base %s)\n' "$pr_number" "$pr_head" "$pr_base"
  printf '[release] transaction digest: %s\n' "$digest"
  printf '[release] transaction file: %s\n' "$path"
  printf '[release] next safe action: %s\n' "$next_action"
  printf '[release] wake event: %s\n' "$wake_event"
  printf '[release] invalidators: %s\n' "$(printf '%s' "$record" | jq -r '.invalidators | join("; ")')"
}

# Fail closed when a supplied record is missing, malformed, or its PR head moved.
turnkey_validate_existing_record() {
  local path="$1"
  local live_pr_head="$2"
  local schema stage recorded_head authoritative
  if [[ ! -f "$path" ]]; then
    printf 'turnkey-handoff: transaction record not found at %s; refusing to rediscover a partial transaction\n' "$path" >&2
    return 1
  fi
  if ! jq -e . "$path" >/dev/null 2>&1; then
    printf 'turnkey-handoff: transaction record at %s is not valid JSON\n' "$path" >&2
    return 1
  fi
  schema="$(jq -r '.schema_version // empty' "$path")"
  if [[ "$schema" != "$TURNKEY_SCHEMA_VERSION" ]]; then
    printf 'turnkey-handoff: unsupported schema_version %s at %s\n' "${schema:-<missing>}" "$path" >&2
    return 1
  fi
  stage="$(jq -r '.stage // empty' "$path")"
  if [[ "$stage" != "$TURNKEY_STAGE_MANUAL_MERGE" && "$stage" != "$TURNKEY_STAGE_MERGE_REQUESTED" ]]; then
    printf 'turnkey-handoff: record at %s is not a T02 handoff stage (%s)\n' "$path" "${stage:-<missing>}" >&2
    return 1
  fi
  authoritative="$(jq -r '.authoritative // false' "$path")"
  if [[ "$authoritative" != "true" ]]; then
    printf 'turnkey-handoff: record at %s is not marked authoritative\n' "$path" >&2
    return 1
  fi
  recorded_head="$(jq -r '.pr.head // empty' "$path")"
  if [[ -z "$recorded_head" ]]; then
    printf 'turnkey-handoff: record at %s omits PR head identity\n' "$path" >&2
    return 1
  fi
  if [[ -n "$live_pr_head" && "$recorded_head" != "$live_pr_head" ]]; then
    printf 'turnkey-handoff: record at %s invalidated: recorded PR head %s != live %s\n' \
      "$path" "$recorded_head" "$live_pr_head" >&2
    return 1
  fi
  return 0
}

turnkey_exit_code_for_stage() {
  case "$1" in
    "$TURNKEY_STAGE_MANUAL_MERGE") printf '%s' "$TURNKEY_EXIT_MANUAL_MERGE" ;;
    "$TURNKEY_STAGE_MERGE_REQUESTED") printf '%s' "$TURNKEY_EXIT_MERGE_REQUESTED" ;;
    *) return 1 ;;
  esac
}

turnkey_pr_merged_now() {
  local pr_number="$1"
  local merged
  merged="$(gh pr view "$pr_number" --json mergedAt -q '.mergedAt // empty')"
  [[ -n "$merged" ]]
}

turnkey_default_invalidators() {
  printf '%s' '["pr head movement","missing or non-authoritative transaction record","target branch still at initial base commit","transaction digest mismatch"]'
}

turnkey_manual_merge_next_action() {
  local version="$1"
  local pr_number="$2"
  local pr_head="$3"
  local tx_path="$4"
  printf 'Merge PR #%s at expected head %s, then re-run: cargo xtask release-turnkey --version %s --transaction %s' \
    "$pr_number" "$pr_head" "$version" "$tx_path"
}

turnkey_manual_merge_wake() {
  local pr_number="$1"
  local branch="$2"
  local base_sha="$3"
  printf 'PR #%s is MERGED and origin/%s is no longer %s' "$pr_number" "$branch" "$base_sha"
}

turnkey_merge_requested_next_action() {
  local version="$1"
  local pr_number="$2"
  local tx_path="$3"
  printf 'Wait until PR #%s lands on the target branch, then re-run: cargo xtask release-turnkey --version %s --transaction %s' \
    "$pr_number" "$version" "$tx_path"
}

turnkey_merge_requested_wake() {
  local pr_number="$1"
  local branch="$2"
  local base_sha="$3"
  printf 'PR #%s reports mergedAt and origin/%s has moved off %s; merge-queue admission or auto-merge arming is not landing' \
    "$pr_number" "$branch" "$base_sha"
}
