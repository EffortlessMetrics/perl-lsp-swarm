#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMPL="${REPO_ROOT}/scripts/cleanup-completed-worktrees.sh"

PASS=0
FAIL=0
TMPDIR_BASE=""

cleanup() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
}
trap cleanup EXIT

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

assert_contains() {
  local label="$1"
  local haystack="$2"
  local needle="$3"

  if grep -qF "$needle" <<<"$haystack"; then
    pass "$label"
  else
    fail "$label"
    printf 'missing: %s\noutput:\n%s\n' "$needle" "$haystack"
  fi
}

assert_not_contains() {
  local label="$1"
  local haystack="$2"
  local needle="$3"

  if grep -qF "$needle" <<<"$haystack"; then
    fail "$label"
    printf 'unexpected: %s\noutput:\n%s\n' "$needle" "$haystack"
  else
    pass "$label"
  fi
}

write_mock_git() {
  local mock_dir="$1"
  cat > "${mock_dir}/git" <<'MOCK_GIT'
#!/usr/bin/env bash
set -euo pipefail

printf 'git %s\n' "$*" >> "${MOCK_STATE}/git.log"

handle_rev_parse() {
  if [[ "$*" == *"--path-format=absolute --git-common-dir"* ]]; then
    printf '%s/.git\n' "${MOCK_REPO_ROOT}"
    exit 0
  fi
  if [[ "$*" == *"--show-toplevel"* ]]; then
    if [[ -n "${_worktree_path:-}" ]]; then
      printf '%s\n' "${_worktree_path}"
    else
      printf '%s\n' "${MOCK_REPO_ROOT}"
    fi
    exit 0
  fi
  if [[ "$*" == *"--abbrev-ref HEAD"* ]]; then
    cat "${MOCK_STATE}/current-branch"
    exit 0
  fi
  if [[ "$*" == *"HEAD"* && "$*" != *"--verify"* ]]; then
    printf 'abc123\n'
    exit 0
  fi
  if [[ "$*" == *"--verify"* && "$*" == *"origin/"* ]]; then
    if [[ -s "${MOCK_STATE}/remote-branch" ]]; then
      exit 0
    fi
    exit 1
  fi
  if [[ "$*" == *"--verify"* ]]; then
    exit 0
  fi
}

if [[ "${1:-}" == "-C" ]]; then
  shift
  _worktree_path="${1:-}"
  shift || true
  case "${1:-}" in
    status)
      if [[ -f "${MOCK_STATE}/dirty" ]]; then
        cat "${MOCK_STATE}/dirty"
      fi
      exit 0
      ;;
    rev-parse)
      shift
      handle_rev_parse "$*"
      ;;
    fetch)
      if [[ -s "${MOCK_STATE}/fetch-fail" ]]; then
        exit 1
      fi
      exit 0
      ;;
    merge-base)
      if [[ "${2:-}" == "--is-ancestor" ]]; then
        if grep -qxF "${3:-}" "${MOCK_STATE}/merged-heads" 2>/dev/null; then
          exit 0
        fi
        exit 1
      fi
      ;;
    rev-list)
      if [[ "${2:-}" == "--count" ]]; then
        if [[ -s "${MOCK_STATE}/ahead-count" ]]; then
          cat "${MOCK_STATE}/ahead-count"
        else
          printf '0\n'
        fi
        exit 0
      fi
      ;;
    worktree)
      case "${2:-}" in
        prune|list|remove)
          case "${2:-}" in
            list)
              cat "${MOCK_STATE}/worktree-list"
              ;;
        remove)
          if grep -q -- '--force' <<<"$*"; then
            printf 'unexpected --force in worktree remove\n' >&2
            exit 99
          fi
          if [[ -s "${MOCK_STATE}/remove-fails" ]]; then
            exit 1
          fi
          printf 'DESTRUCTIVE git worktree remove %s\n' "$*" >> "${MOCK_STATE}/destructive.log"
          ;;
          esac
          exit 0
          ;;
      esac
      ;;
    branch)
      if [[ "${2:-}" == "-D" ]]; then
        printf 'DESTRUCTIVE git branch -D %s\n' "${3:-}" >> "${MOCK_STATE}/destructive.log"
        exit 0
      fi
      ;;
  esac
fi

case "${1:-}" in
  rev-parse)
    shift
    handle_rev_parse "$*"
    ;;
  worktree)
    case "${2:-}" in
      prune)
        exit 0
        ;;
      list)
        cat "${MOCK_STATE}/worktree-list"
        exit 0
        ;;
      remove)
        if grep -q -- '--force' <<<"$*"; then
          printf 'unexpected --force in worktree remove\n' >&2
          exit 99
        fi
        if [[ -s "${MOCK_STATE}/remove-fails" ]]; then
          exit 1
        fi
        printf 'DESTRUCTIVE git worktree remove %s\n' "$*" >> "${MOCK_STATE}/destructive.log"
        exit 0
        ;;
    esac
    ;;
  branch)
    if [[ "${2:-}" == "--merged" ]]; then
      printf '%s\n' "${3:-}" >> "${MOCK_STATE}/merged-queries.log"
      cat "${MOCK_STATE}/merged-branches"
      exit 0
    fi
    if [[ "${2:-}" == "-D" ]]; then
      printf 'DESTRUCTIVE git branch -D %s\n' "${3:-}" >> "${MOCK_STATE}/destructive.log"
      exit 0
    fi
    ;;
  config)
    if [[ "${2:-}" == "--get" ]]; then
      if [[ -s "${MOCK_STATE}/remote-merge" ]]; then
        cat "${MOCK_STATE}/remote-merge"
        exit 0
      fi
      exit 1
    fi
    ;;
  rev-list)
    if [[ -s "${MOCK_STATE}/ahead-count" ]]; then
      cat "${MOCK_STATE}/ahead-count"
    else
      printf '0\n'
    fi
    exit 0
    ;;
esac

printf 'unexpected git command: %s\n' "$*" >&2
exit 97
MOCK_GIT
  chmod +x "${mock_dir}/git"
}

write_mock_gh() {
  local mock_dir="$1"
  cat > "${mock_dir}/gh" <<'MOCK_GH'
#!/usr/bin/env bash
set -euo pipefail

printf 'gh %s\n' "$*" >> "${MOCK_STATE}/gh.log"

if [[ "$*" == pr\ list* ]]; then
  if [[ "$*" == *"--state merged"* ]]; then
    if [[ -s "${MOCK_STATE}/merged-pr" ]]; then
      cat "${MOCK_STATE}/merged-pr"
    else
      printf '[]\n'
    fi
    exit 0
  fi
  if [[ -s "${MOCK_STATE}/pr-number" ]]; then
    cat "${MOCK_STATE}/pr-number"
  fi
  exit 0
fi

printf 'unexpected gh command: %s\n' "$*" >&2
exit 98
MOCK_GH
  chmod +x "${mock_dir}/gh"
}

new_case() {
  local name="$1"
  local case_dir="${TMPDIR_BASE}/${name}"
  mkdir -p "${case_dir}/bin" "${case_dir}/repo/.git" "${case_dir}/repo/.claude/worktrees"
  : > "${case_dir}/merged-branches"
  : > "${case_dir}/merged-heads"
  : > "${case_dir}/remote-branch"
  : > "${case_dir}/remote-merge"
  : > "${case_dir}/ahead-count"
  : > "${case_dir}/pr-number"
  : > "${case_dir}/destructive.log"
  : > "${case_dir}/git.log"
  : > "${case_dir}/gh.log"
  printf 'main\n' > "${case_dir}/current-branch"
  write_mock_git "${case_dir}/bin"
  write_mock_gh "${case_dir}/bin"
  printf '%s\n' "$case_dir"
}

write_worktree_list() {
  local case_dir="$1"
  local branch="$2"
  local worktree_path="${case_dir}/repo/.claude/worktrees/${branch//\//-}"
  mkdir -p "$worktree_path"
  cat > "${case_dir}/worktree-list" <<EOF
worktree ${case_dir}/repo
HEAD abc123
branch refs/heads/main

worktree ${worktree_path}
HEAD abc123
branch refs/heads/${branch}

EOF
}

run_cleanup_dry_run() {
  local case_dir="$1"
  PATH="${case_dir}/bin:${PATH}" \
    MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" \
    bash "$IMPL" --dry-run 2>&1
}

assert_no_destructive_commands() {
  local label="$1"
  local case_dir="$2"

  if [[ -s "${case_dir}/destructive.log" ]]; then
    fail "$label"
    cat "${case_dir}/destructive.log"
  else
    pass "$label"
  fi
}

test_merged_branch_uses_main_by_default() {
  local case_dir output
  case_dir="$(new_case merged-main)"
  write_worktree_list "$case_dir" "feature/merged"
  printf 'abc123\n' > "${case_dir}/merged-heads"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "dry-run announces the cleanup base" "$output" "Base:"
  assert_contains "merged branch is marked for removal in dry-run" "$output" "feature/merged"
  assert_contains "merged branch removal is reported" "$output" "landed"
  assert_contains "merged branch action is REMOVE" "$output" "REMOVE"
  assert_no_destructive_commands "dry-run does not remove merged worktrees" "$case_dir"
}

test_pushed_branch_is_removed_even_with_open_pr() {
  local case_dir output
  case_dir="$(new_case open-pr)"
  write_worktree_list "$case_dir" "feature/open-pr"
  printf '123\n' > "${case_dir}/pr-number"
  printf '1\n' > "${case_dir}/remote-branch"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "fully pushed branch is removed even when a PR is open" "$output" "pushed"
  assert_contains "pushed branch action is REMOVE" "$output" "REMOVE"
  assert_no_destructive_commands "dry-run does not remove pushed worktrees" "$case_dir"
}

test_unpushed_branch_is_kept() {
  local case_dir output
  case_dir="$(new_case unpushed)"
  write_worktree_list "$case_dir" "feature/unpushed"
  printf '1\n' > "${case_dir}/remote-branch"
  printf '2\n' > "${case_dir}/ahead-count"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "unpushed branch is kept" "$output" "unpushed:2"
  assert_contains "unpushed action is KEEP" "$output" "KEEP"
  assert_no_destructive_commands "dry-run does not remove unpushed worktrees" "$case_dir"
}

test_branch_without_remote_is_kept() {
  local case_dir output
  case_dir="$(new_case abandoned)"
  write_worktree_list "$case_dir" "feature/abandoned"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "branch without a remote tracking ref is kept" "$output" "no-remote"
  assert_contains "no-remote action is KEEP" "$output" "KEEP"
  assert_not_contains "no-remote dry-run does not claim actual deletion" "$output" "git branch -D"
  assert_no_destructive_commands "dry-run does not remove no-remote worktrees" "$case_dir"
}

write_managed_owner_state() {
  local case_dir="$1"
  local worktree_path="$2"
  local owner="$3"
  local rel_path="${worktree_path#${case_dir}/repo/}"
  mkdir -p "${case_dir}/repo/.ops-perl-lsp/worktree-manager"
  cat > "${case_dir}/repo/.ops-perl-lsp/worktree-manager/state.json" <<EOF
{"slots":[{"path":"${rel_path}","owner":"${owner}","status":"active"}]}
EOF
}

test_managed_owner_keeps_pushed_worktree() {
  local case_dir output worktree_path
  case_dir="$(new_case managed-owner)"
  write_worktree_list "$case_dir" "feature/owned"
  worktree_path="${case_dir}/repo/.claude/worktrees/feature-owned"
  printf '1\n' > "${case_dir}/remote-branch"
  write_managed_owner_state "$case_dir" "$worktree_path" "lane-a"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "managed owner keeps pushed worktree" "$output" "owned:lane-a"
  assert_contains "managed owner action is KEEP" "$output" "KEEP"
  assert_no_destructive_commands "managed owner dry-run does not remove worktree" "$case_dir"
}

run_cleanup_json() {
  local case_dir="$1"
  PATH="${case_dir}/bin:${PATH}" \
    MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" \
    bash "$IMPL" --json 2>/dev/null
}

test_json_escapes_special_characters() {
  local case_dir output
  if ! command -v jq >/dev/null 2>&1; then
    printf 'SKIP json output parses with quoted branch name (jq not installed)\n'
    return 0
  fi
  case_dir="$(new_case json-quote)"
  write_worktree_list "$case_dir" 'feature/wt"quote'
  printf '1\n' > "${case_dir}/remote-branch"

  output="$(run_cleanup_json "$case_dir")"

  if echo "$output" | jq -e . >/dev/null 2>&1; then
    pass "json output parses with quoted branch name"
  else
    fail "json output parses with quoted branch name"
    printf 'invalid json:\n%s\n' "$output"
  fi
}

test_squash_merged_branch_without_remote_is_removed() {
  local case_dir output
  case_dir="$(new_case squash-merged)"
  write_worktree_list "$case_dir" "feature/squash-merged"
  printf '[{"number":99}]\n' > "${case_dir}/merged-pr"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "squash-merged branch is marked landed" "$output" "landed"
  assert_contains "squash-merged branch action is REMOVE" "$output" "REMOVE"
  assert_no_destructive_commands "dry-run does not remove squash-merged worktrees" "$case_dir"
}

test_fetch_failure_keeps_ambiguous_remote_branch() {
  local case_dir output
  case_dir="$(new_case fetch-fail)"
  write_worktree_list "$case_dir" "feature/fetch-fail"
  printf '1\n' > "${case_dir}/remote-branch"
  printf 'fetch-fail\n' > "${case_dir}/fetch-fail"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "fetch failure downgrades to not-proven" "$output" "not-proven"
  assert_contains "not-proven action is KEEP" "$output" "KEEP"
  assert_no_destructive_commands "fetch failure does not remove worktrees" "$case_dir"
}

echo "=== cleanup-completed-worktrees dry-run test suite ==="
echo ""

if [[ ! -f "$IMPL" ]]; then
  echo "ERROR: cleanup-completed-worktrees.sh not found at ${IMPL}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"

test_merged_branch_uses_main_by_default
test_pushed_branch_is_removed_even_with_open_pr
test_unpushed_branch_is_kept
test_branch_without_remote_is_kept
test_managed_owner_keeps_pushed_worktree
test_json_escapes_special_characters
test_squash_merged_branch_without_remote_is_removed
test_fetch_failure_keeps_ambiguous_remote_branch

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
