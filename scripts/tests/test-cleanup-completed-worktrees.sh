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

# Logged verbatim above so assertions can see it, then stripped so dispatch below
# stays keyed on the subcommand.
if [[ "${1:-}" == "--no-optional-locks" ]]; then
  shift
fi

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

# `set -e` is active and these capture output in a command substitution, so a
# non-zero exit from the script would abort the whole suite instead of failing
# one assertion — measured: 22 assertions, no FAIL line, no summary. Exit status
# is asserted separately by test_successful_sweeps_exit_zero.
run_cleanup_dry_run() {
  local case_dir="$1"
  PATH="${case_dir}/bin:${PATH}" \
    MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" \
    bash "$IMPL" --dry-run 2>&1 || true
}

# The mutating front door. --dry-run is the read-only one; this exists so the
# read-only assertions below cannot pass by disabling the sweep everywhere.
run_cleanup_real() {
  local case_dir="$1"
  PATH="${case_dir}/bin:${PATH}" \
    MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" \
    bash "$IMPL" 2>&1 || true
}

exit_status_of() {
  local case_dir="$1"; shift
  local status=0
  # `set -e` is active: capture the status explicitly, or a non-zero exit aborts
  # the whole suite instead of reporting one legible failure.
  PATH="${case_dir}/bin:${PATH}" \
    MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" \
    bash "$IMPL" "$@" >/dev/null 2>&1 || status=$?
  printf '%s\n' "$status"
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

# Any git invocation that can write refs, administrative metadata, config, or the
# working tree. `worktree prune` and `fetch` belong here: both mutate, neither was
# ever recorded in destructive.log, so the previous spy reported a clean dry-run
# while metadata mutation was attempted.
MUTATING_GIT_PATTERNS=(
  'worktree prune'
  'worktree remove'
  'fetch'
  'branch -D'
  'branch -d'
  'update-ref'
  'config --add'
  'config --unset'
  'reset'
  'checkout'
)

assert_no_mutating_git_commands() {
  local label="$1"
  local case_dir="$2"
  local pattern found=""

  for pattern in "${MUTATING_GIT_PATTERNS[@]}"; do
    if grep -qF -- "$pattern" "${case_dir}/git.log" 2>/dev/null; then
      found+="${pattern}"$'\n'
    fi
  done

  if [[ -n "$found" ]]; then
    fail "$label"
    printf 'mutating git commands observed:\n%s\nfull git log:\n' "$found"
    cat "${case_dir}/git.log"
  else
    pass "$label"
  fi
}

# `git status` is read-shaped but not read-only: it opportunistically refreshes
# the index and rewrites .git/worktrees/<id>/index when a tracked file's cached
# stat data is stale. `--no-optional-locks` suppresses that, so every observation
# must carry it — a plain `git status` on the inspection path is a metadata write
# the mutating-command patterns above cannot detect.
assert_reads_are_lock_free() {
  local label="$1"
  local case_dir="$2"
  local offenders

  offenders="$(grep -E '^git (status|-C [^ ]+ status)' "${case_dir}/git.log" 2>/dev/null || true)"

  if [[ -n "$offenders" ]]; then
    fail "$label"
    printf 'observation ran without --no-optional-locks:\n%s\n' "$offenders"
  else
    pass "$label"
  fi
}

assert_git_log_contains() {
  local label="$1"
  local case_dir="$2"
  local needle="$3"

  if grep -qF -- "$needle" "${case_dir}/git.log" 2>/dev/null; then
    pass "$label"
  else
    fail "$label"
    printf 'missing git invocation: %s\ngit log:\n' "$needle"
    cat "${case_dir}/git.log"
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
    bash "$IMPL" --json 2>/dev/null || true
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

# The NOT_PROVEN downgrade guards a fetch that was attempted and failed, leaving
# refs at unknown staleness. Only the mutating run fetches, so that is where the
# contract is proven.
test_fetch_failure_keeps_ambiguous_remote_branch() {
  local case_dir output
  case_dir="$(new_case fetch-fail)"
  write_worktree_list "$case_dir" "feature/fetch-fail"
  printf '1\n' > "${case_dir}/remote-branch"
  printf 'fetch-fail\n' > "${case_dir}/fetch-fail"

  output="$(run_cleanup_real "$case_dir")"

  assert_contains "fetch failure downgrades to not-proven" "$output" "not-proven"
  assert_contains "not-proven action is KEEP" "$output" "KEEP"
  assert_contains "fetch failure is reported as a remote-ref state" "$output" "Remote refs: failed"
  assert_no_destructive_commands "fetch failure does not remove worktrees" "$case_dir"
}

# --- read-only inspection contract (#10256) --------------------------------
#
# The incident: `--dry-run` through WSL against a Windows-hosted repository pruned
# the administrative registrations of two still-existing worktrees. The rows did
# not merely get misclassified, they vanished from the report, because the global
# prune ran before classification and erased the evidence being observed.

test_dry_run_performs_no_mutating_git_commands() {
  local case_dir output
  case_dir="$(new_case readonly-spy)"
  write_worktree_list "$case_dir" "feature/landed"
  printf 'abc123\n' > "${case_dir}/merged-heads"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "dry-run still classifies the landed worktree" "$output" "landed"
  assert_no_mutating_git_commands "dry-run runs no fetch, prune, remove, or branch delete" "$case_dir"
  assert_reads_are_lock_free "dry-run observations decline optional index locks" "$case_dir"
}

# Negative control for the assertion above: it must fail on a mutating run, or it
# would also pass against a script that simply never sweeps.
test_real_run_still_fetches_and_prunes() {
  local case_dir output
  case_dir="$(new_case mutating-control)"
  write_worktree_list "$case_dir" "feature/landed"
  printf 'abc123\n' > "${case_dir}/merged-heads"

  output="$(run_cleanup_real "$case_dir")"

  assert_git_log_contains "real run refreshes remote refs" "$case_dir" "fetch"
  assert_git_log_contains "real run prunes administrative metadata" "$case_dir" "worktree prune"
  assert_contains "real run reports fresh remote refs" "$output" "Remote refs: fresh"
  assert_contains "real run reports removals as performed" "$output" "Removed:"
}

test_dry_run_reports_stale_refs_without_refreshing_them() {
  local case_dir output
  case_dir="$(new_case stale-refs)"
  write_worktree_list "$case_dir" "feature/pushed"
  printf '1\n' > "${case_dir}/remote-branch"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "dry-run names the remote-ref state" "$output" "Remote refs: stale"
  # Stale refs are not monotonic — a forced update or a deleted branch can move a
  # remote-tracking ref backward — so the header must not promise conservatism.
  assert_contains "dry-run calls stale-ref verdicts provisional" "$output" "provisional"
  assert_contains "dry-run says the sweep re-fetches before acting" "$output" "re-fetches before it acts"
  assert_not_contains "dry-run does not claim stale refs only over-KEEP" "$output" "over-KEEP"
  # Containment in an already-observed origin ref still proves the remote holds
  # the commits, so a REMOVE verdict survives the missing fetch.
  assert_contains "stale refs still prove a fully pushed branch" "$output" "pushed"
  assert_no_mutating_git_commands "stale-ref inspection mutates nothing" "$case_dir"
}

# A registration whose path this OS view cannot resolve is not evidence that the
# registration is stale. It is preserved as a REVIEW row and never disappears.
write_unreachable_worktree_list() {
  local case_dir="$1"
  local unreachable_path="$2"
  cat > "${case_dir}/worktree-list" <<EOF
worktree ${case_dir}/repo
HEAD abc123
branch refs/heads/main

worktree ${unreachable_path}
HEAD abc123
branch refs/heads/feature/elsewhere

EOF
}

test_foreign_os_registration_is_preserved_for_review() {
  local case_dir output
  case_dir="$(new_case foreign-path)"
  write_unreachable_worktree_list "$case_dir" 'F:\code\Opencode\Rust\wt-mainred'

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "cross-OS registration is classified as a foreign path" "$output" "foreign-path"
  assert_contains "cross-OS registration is routed to REVIEW" "$output" "REVIEW"
  assert_contains "cross-OS row is counted, not dropped" "$output" "Total:   2"
  assert_contains "review rows are counted distinguishably" "$output" "Review:  1"
  assert_no_mutating_git_commands "cross-OS inspection mutates nothing" "$case_dir"
}

test_unreachable_native_path_is_preserved_for_review() {
  local case_dir output
  case_dir="$(new_case missing-path)"
  write_unreachable_worktree_list "$case_dir" "${case_dir}/repo/.claude/worktrees/never-created"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "an absent path is reported as missing" "$output" "missing"
  assert_contains "an absent path is routed to REVIEW, never cleanup" "$output" "REVIEW"
  assert_contains "the absent row survives inspection" "$output" "Total:   2"
  assert_no_mutating_git_commands "absent-path inspection mutates nothing" "$case_dir"
}

test_dry_run_message_claims_only_what_it_proves() {
  local case_dir output
  case_dir="$(new_case dry-run-claim)"
  write_worktree_list "$case_dir" "feature/landed"
  printf 'abc123\n' > "${case_dir}/merged-heads"

  output="$(run_cleanup_dry_run "$case_dir")"

  assert_contains "dry-run states the read-only guarantee" "$output" "read-only: no fetch, prune, removal, or branch deletion"
  # A proposed action must never be summarised as one already taken.
  assert_contains "dry-run summarises removals as proposed" "$output" "removal proposed, not performed"
  assert_not_contains "dry-run does not claim it removed anything" "$output" "Removed:"
}

test_successful_sweeps_exit_zero() {
  local case_dir status
  case_dir="$(new_case exit-status)"
  write_worktree_list "$case_dir" "feature/landed"
  printf 'abc123\n' > "${case_dir}/merged-heads"

  status="$(exit_status_of "$case_dir" --dry-run)"
  if [[ "$status" == "0" ]]; then
    pass "successful dry run exits zero"
  else
    fail "successful dry run exits zero"
    printf 'exit status: %s\n' "$status"
  fi

  # The summary tail was `$DRY_RUN && echo ...`, whose status is the last command
  # of the script, so every successful real sweep exited 1.
  status="$(exit_status_of "$case_dir")"
  if [[ "$status" == "0" ]]; then
    pass "successful real sweep exits zero"
  else
    fail "successful real sweep exits zero"
    printf 'exit status: %s\n' "$status"
  fi
}

# --- real-repository non-mutation proof ------------------------------------
#
# Everything above proves command *shape* against a fake git. Shape spying is
# blind to a read-shaped command that writes: `git status` refreshes the index
# and rewrites `.git/worktrees/<id>/index` when a tracked file's cached stat data
# is stale. Only a real repository can prove no file under .git/worktrees/**
# changed, so that claim is proven here rather than asserted.

real_git_snapshot() {
  local repo="$1"
  (
    cd "$repo" || exit 1
    find .git/worktrees -type f -exec sha256sum {} \; 2>/dev/null | sort
    # --no-optional-locks so taking the snapshot cannot perturb its own subject.
    git --no-optional-locks worktree list --porcelain
    git --no-optional-locks for-each-ref --format='%(refname) %(objectname)'
  )
}

test_real_repository_dry_run_writes_nothing() {
  local label="real-repository dry run leaves .git metadata and refs byte-identical"
  local root before after
  root="${TMPDIR_BASE}/real-git"

  if ! command -v git >/dev/null 2>&1; then
    printf 'SKIP %s (git not installed)\n' "$label"
    return 0
  fi

  mkdir -p "$root"
  (
    set -e
    cd "$root"
    git init -q -b main repo
    cd repo
    git config user.email cleanup-test@example.invalid
    git config user.name "cleanup test"
    mkdir -p sub
    printf 'content\n' > sub/file.txt
    git add .
    git commit -qm init
    git worktree add -q ../wt-live -b live
    # A past mtime, not `touch`: a just-touched file is "racily clean" and git
    # deliberately declines to cache its stat data, so the write never fires and
    # the fixture would silently prove nothing.
    touch -d '2020-01-01 00:00:00' ../wt-live/sub/file.txt
  ) >/dev/null 2>&1 || {
    printf 'SKIP %s (fixture repository could not be built)\n' "$label"
    return 0
  }

  before="$(real_git_snapshot "${root}/repo")"
  ( cd "${root}/repo" && CLEANUP_BASE_BRANCH=main bash "$IMPL" --dry-run ) >/dev/null 2>&1 || true
  after="$(real_git_snapshot "${root}/repo")"

  if [[ "$before" == "$after" ]]; then
    pass "$label"
  else
    fail "$label"
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") || true
  fi
}

test_json_projection_carries_the_inspection_axes() {
  local case_dir output
  if ! command -v jq >/dev/null 2>&1; then
    printf 'SKIP json projection carries the inspection axes (jq not installed)\n'
    return 0
  fi
  case_dir="$(new_case json-axes)"
  write_unreachable_worktree_list "$case_dir" 'F:\code\Opencode\Rust\wt-pr11859'

  output="$(PATH="${case_dir}/bin:${PATH}" MOCK_STATE="$case_dir" \
    MOCK_REPO_ROOT="${case_dir}/repo" bash "$IMPL" --json --dry-run 2>/dev/null || true)"

  # Human and JSON projections must agree on the same semantic row.
  assert_contains "json reports the dry-run axis" "$output" '"dry_run":true'
  assert_contains "json reports the remote-ref state" "$output" '"remote_state":"stale"'
  assert_contains "json counts the review row" "$output" '"review":1'
  assert_contains "json preserves the foreign-path row" "$output" '"state":"foreign-path"'
  assert_contains "json preserves the review action" "$output" '"action":"REVIEW"'
  assert_no_mutating_git_commands "json inspection mutates nothing" "$case_dir"
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
test_dry_run_performs_no_mutating_git_commands
test_real_run_still_fetches_and_prunes
test_dry_run_reports_stale_refs_without_refreshing_them
test_foreign_os_registration_is_preserved_for_review
test_unreachable_native_path_is_preserved_for_review
test_dry_run_message_claims_only_what_it_proves
test_successful_sweeps_exit_zero
test_json_projection_carries_the_inspection_axes
test_real_repository_dry_run_writes_nothing

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
