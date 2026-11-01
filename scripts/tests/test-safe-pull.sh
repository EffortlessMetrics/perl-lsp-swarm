#!/usr/bin/env bash
# scripts/tests/test-safe-pull.sh
#
# Regression tests for issues #17403/#17409: safe-pull.sh must never
# irrecoverably delete colliding untracked files. Colliding content must
# survive byte-identical (in place or in the named salvage packet), packets
# must live under the git common dir so they survive linked-worktree
# removal, and collision-free pulls must keep working (no-collision control).
#
# Usage:
#   bash scripts/tests/test-safe-pull.sh
#
# The script under test can be overridden (falsification seam — point it at
# the pre-fix script to prove this suite fails without the repair):
#   SAFE_PULL_SCRIPT=/tmp/safe-pull-prefix.sh bash scripts/tests/test-safe-pull.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SAFE_PULL="${SAFE_PULL_SCRIPT:-${REPO_ROOT}/scripts/safe-pull.sh}"
# run_safe_pull cds into the fixture before invoking the script, so a relative
# override would resolve against the fixture, not the invocation directory.
# Absolutize once, here, while the cwd is still the invocation directory.
case "${SAFE_PULL}" in
  /*|[A-Za-z]:*) ;;
  *) SAFE_PULL="$(pwd)/${SAFE_PULL}" ;;
esac

PASS=0
FAIL=0
TMPDIR_BASE=""
ORIGIN=""
WORK=""
ADV=""

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

assert_byte_identical() {
  local label="$1"
  local actual="$2"
  local expected="$3"

  if [[ -f "$actual" ]] && cmp -s "$actual" "$expected"; then
    pass "$label"
  else
    fail "$label"
    printf 'not byte-identical: %s vs %s\n' "$actual" "$expected"
  fi
}

# Fixture: bare origin with a base commit on main, plus two clones.
# Sets ORIGIN, WORK (the pull target), and ADV (the origin advancer).
setup_repos() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
  TMPDIR_BASE="$(mktemp -d)"
  ORIGIN="${TMPDIR_BASE}/origin.git"
  WORK="${TMPDIR_BASE}/work"
  ADV="${TMPDIR_BASE}/advancer"

  git init -q -b main --bare "$ORIGIN"
  git clone -q "$ORIGIN" "$ADV" 2>/dev/null
  git -C "$ADV" config user.email "safe-pull-test@example.com"
  git -C "$ADV" config user.name "safe-pull-test"
  git -C "$ADV" config commit.gpgsign false
  echo "base" > "${ADV}/base.txt"
  git -C "$ADV" add base.txt
  git -C "$ADV" commit -qm "base"
  git -C "$ADV" push -q -u origin main

  git clone -q "$ORIGIN" "$WORK" 2>/dev/null
  git -C "$WORK" config user.email "safe-pull-test@example.com"
  git -C "$WORK" config user.name "safe-pull-test"
  git -C "$WORK" config commit.gpgsign false
}

run_safe_pull() {
  # $1 = directory to pull in; $2 = cwd to invoke from (default $1);
  # prints output, returns script exit status
  local dir="$1"
  local cwd="${2:-$1}"
  local out rc
  rc=0
  out="$(cd "$cwd" && bash "$SAFE_PULL" 2>&1)" || rc=$?
  printf '%s' "$out"
  return "$rc"
}

extract_salvage_dir() {
  grep -F 'salvage -> ' <<<"$1" | head -n 1 | sed 's/.*salvage -> //' || true
}

advance_origin_notes() {
  local content="${1:-REMOTE-NOTES}"
  printf '%s\n' "$content" > "${ADV}/notes.txt"
  git -C "$ADV" add notes.txt
  git -C "$ADV" commit -qm "add notes"
  git -C "$ADV" push -q origin main
}

test_colliding_untracked_file_survives_with_content() {
  setup_repos

  # Untracked files in $WORK holding known content (multi-line + trailing
  # newline so the byte-identity check is meaningful), including one nested
  # path to cover salvage-dir structure preservation.
  printf 'KNOWN-CONTENT-17403\nsecond line\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected.txt"
  mkdir -p "${WORK}/docs"
  printf 'NESTED-KNOWN-17403\n' > "${WORK}/docs/plan.txt"
  cp "${WORK}/docs/plan.txt" "${TMPDIR_BASE}/expected-nested.txt"
  # Leading-hyphen name: must be treated as an operand, not an option.
  printf 'DASH-KNOWN-17403\n' > "${WORK}/-draft.txt"
  cp "${WORK}/-draft.txt" "${TMPDIR_BASE}/expected-dash.txt"

  # Origin advances the same paths so the pull reports
  # "would be overwritten by merge".
  printf 'REMOTE-CONTENT\n' > "${ADV}/notes.txt"
  mkdir -p "${ADV}/docs"
  printf 'REMOTE-NESTED\n' > "${ADV}/docs/plan.txt"
  printf 'REMOTE-DASH\n' > "${ADV}/-draft.txt"
  git -C "$ADV" add notes.txt docs/plan.txt ./-draft.txt
  git -C "$ADV" commit -qm "add notes"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "collision: safe-pull exits 0"
  else
    fail "collision: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "collision: output names the salvage packet" "$out" "salvage"

  local salvage_dir
  salvage_dir="$(grep -F 'salvage -> ' <<<"$out" | head -n 1 | sed 's/.*salvage -> //' || true)"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "collision: salvage directory is printed"
    return
  fi
  pass "collision: salvage directory is printed"

  local common_dir
  common_dir="$(git -C "$WORK" rev-parse --path-format=absolute --git-common-dir)"
  case "${salvage_dir}" in
    "${common_dir}/safe-pull-salvage/"*)
      pass "collision: packet lives under the git common dir (retry cannot collide)"
      ;;
    *)
      fail "collision: packet lives under the git common dir (retry cannot collide)"
      printf 'salvage_dir=%s common_dir=%s\n' "$salvage_dir" "$common_dir"
      ;;
  esac

  # Content must be byte-identical either in place or in the named salvage
  # packet (per the Builder Spec: in place wins when the helper keeps it).
  if [[ -f "${WORK}/notes.txt" ]] && cmp -s "${WORK}/notes.txt" "${TMPDIR_BASE}/expected.txt"; then
    pass "collision: known content survives byte-identical (in place)"
  elif [[ -f "${salvage_dir}/notes.txt" ]] && cmp -s "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected.txt"; then
    pass "collision: known content survives byte-identical (salvaged)"
  else
    fail "collision: known content survives byte-identical"
    printf 'work notes.txt:\n'
    cat "${WORK}/notes.txt" 2>/dev/null || printf '(missing)\n'
    printf 'salvage packet listing (%s):\n' "$salvage_dir"
    find "$salvage_dir" -type f 2>/dev/null || printf '(missing)\n'
  fi

  # Nested colliding path must also survive byte-identical with structure.
  assert_byte_identical "collision: nested content salvaged byte-identical" \
    "${salvage_dir}/docs/plan.txt" "${TMPDIR_BASE}/expected-nested.txt"

  # Leading-hyphen colliding path must survive byte-identical (operand, not
  # option, through dirname/mkdir/mv).
  assert_byte_identical "collision: leading-hyphen content salvaged byte-identical" \
    "${salvage_dir}/-draft.txt" "${TMPDIR_BASE}/expected-dash.txt"

  # The merge itself must have completed: work now tracks origin's version.
  assert_byte_identical "collision: pull delivers remote content" \
    "${WORK}/notes.txt" "${ADV}/notes.txt"
}

test_clean_pull_still_succeeds() {
  setup_repos

  # No-collision control: origin advances an unrelated path; the pull must
  # succeed without touching the salvage path.
  printf 'REMOTE-ONLY\n' > "${ADV}/other.txt"
  git -C "$ADV" add other.txt
  git -C "$ADV" commit -qm "add other"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "control: safe-pull exits 0"
  else
    fail "control: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "control: pull reports success" "$out" "Pull succeeded"
  assert_not_contains "control: no salvage on a clean pull" "$out" "salvage"
  assert_byte_identical "control: pull delivers remote content" \
    "${WORK}/other.txt" "${ADV}/other.txt"
}

test_tracked_blockage_aborts_without_moving() {
  setup_repos

  # Mixed refusal: git lists the tracked dirty file AND the untracked
  # collider in one merge output. The pull must abort before moving anything.
  printf 'LOCAL-TRACKED-EDIT\n' > "${WORK}/base.txt"
  cp "${WORK}/base.txt" "${TMPDIR_BASE}/expected-tracked.txt"
  printf 'LOCAL-UNTRACKED\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-untracked.txt"
  printf 'REMOTE-NOTES\n' > "${ADV}/notes.txt"
  printf 'REMOTE-BASE\n' > "${ADV}/base.txt"
  git -C "$ADV" add notes.txt base.txt
  git -C "$ADV" commit -qm "advance both"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    pass "tracked: safe-pull refuses with nonzero exit"
  else
    fail "tracked: safe-pull refuses with nonzero exit (got 0)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "tracked: output names the tracked offender" "$out" "base.txt"
  assert_contains "tracked: output says nothing was moved" "$out" "nothing was moved"
  assert_byte_identical "tracked: tracked edits stay in place" \
    "${WORK}/base.txt" "${TMPDIR_BASE}/expected-tracked.txt"
  assert_byte_identical "tracked: untracked collider stays in place" \
    "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-untracked.txt"
  if [[ -d "${WORK}/.git/safe-pull-salvage" ]]; then
    fail "tracked: no salvage packet is created on refusal"
  else
    pass "tracked: no salvage packet is created on refusal"
  fi
}

test_nested_invocation_salvages_from_root() {
  setup_repos

  printf 'NESTED-INVOCATION\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-nested-inv.txt"
  printf 'REMOTE-NOTES\n' > "${ADV}/notes.txt"
  git -C "$ADV" add notes.txt
  git -C "$ADV" commit -qm "add notes"
  git -C "$ADV" push -q origin main
  mkdir -p "${WORK}/sub/inner"

  # Invoke from a nested directory: git names root-relative paths, so the
  # salvage must still resolve the worktree root.
  local out rc
  rc=0
  out="$(run_safe_pull "$WORK" "${WORK}/sub/inner")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "nested: safe-pull exits 0"
  else
    fail "nested: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi

  local salvage_dir
  salvage_dir="$(grep -F 'salvage -> ' <<<"$out" | head -n 1 | sed 's/.*salvage -> //' || true)"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "nested: salvage directory is printed"
    return
  fi
  pass "nested: salvage directory is printed"
  assert_byte_identical "nested: content salvaged byte-identical" \
    "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected-nested-inv.txt"
  assert_byte_identical "nested: pull delivers remote content" \
    "${WORK}/notes.txt" "${ADV}/notes.txt"
}

test_staged_delete_collision_refuses_without_moving() {
  setup_repos

  # Staged-delete collision (#17418): path is tracked at HEAD, deletion is
  # staged (so `git ls-files --error-unmatch` fails open), valuable untracked
  # content sits at the same path, and the remote modifies it. The guard
  # must refuse before moving anything.
  git -C "$WORK" rm -q --cached base.txt
  printf 'LOCAL-UNTRACKED-AFTER-STAGED-DELETE\n' > "${WORK}/base.txt"
  cp "${WORK}/base.txt" "${TMPDIR_BASE}/expected-staged-delete.txt"
  printf 'REMOTE-BASE\n' > "${ADV}/base.txt"
  git -C "$ADV" add base.txt
  git -C "$ADV" commit -qm "modify base"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    pass "staged-delete: safe-pull refuses with nonzero exit"
  else
    fail "staged-delete: safe-pull refuses with nonzero exit (got 0)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "staged-delete: output refuses salvage" "$out" "Refusing to salvage"
  assert_contains "staged-delete: output names the staged offender" "$out" "base.txt"
  assert_contains "staged-delete: output says nothing was moved" "$out" "nothing was moved"
  assert_byte_identical "staged-delete: untracked content stays in place" \
    "${WORK}/base.txt" "${TMPDIR_BASE}/expected-staged-delete.txt"
  if [[ -d "${WORK}/.git/safe-pull-salvage" ]]; then
    fail "staged-delete: no salvage packet is created on refusal"
  else
    pass "staged-delete: no salvage packet is created on refusal"
  fi
}

test_linked_worktree_packet_survives_removal() {
  setup_repos

  local linked="${TMPDIR_BASE}/linked-wt"
  git -C "$WORK" worktree add -q -b linked-wt "$linked"
  git -C "$linked" config user.email "safe-pull-test@example.com"
  git -C "$linked" config user.name "safe-pull-test"
  git -C "$linked" config commit.gpgsign false

  printf 'LINKED-KNOWN-17409\n' > "${linked}/notes.txt"
  cp "${linked}/notes.txt" "${TMPDIR_BASE}/expected-linked.txt"
  advance_origin_notes "REMOTE-LINKED"

  local out rc
  rc=0
  out="$(run_safe_pull "$linked")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "linked: safe-pull exits 0"
  else
    fail "linked: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    git -C "$WORK" worktree remove --force "$linked" >/dev/null 2>&1 || true
    return
  fi

  local salvage_dir
  salvage_dir="$(extract_salvage_dir "$out")"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "linked: salvage directory is printed"
    git -C "$WORK" worktree remove --force "$linked" >/dev/null 2>&1 || true
    return
  fi
  pass "linked: salvage directory is printed"

  local common_dir wt_git_dir
  common_dir="$(git -C "$linked" rev-parse --path-format=absolute --git-common-dir)"
  wt_git_dir="$(git -C "$linked" rev-parse --absolute-git-dir)"
  case "${salvage_dir}" in
    "${common_dir}/safe-pull-salvage/"*)
      pass "linked: packet lives under the git common dir"
      ;;
    *)
      fail "linked: packet lives under the git common dir"
      printf 'salvage_dir=%s common_dir=%s\n' "$salvage_dir" "$common_dir"
      ;;
  esac
  case "${salvage_dir}" in
    "${wt_git_dir}/"*)
      fail "linked: packet does not live under the per-worktree git dir"
      printf 'salvage_dir=%s wt_git_dir=%s\n' "$salvage_dir" "$wt_git_dir"
      ;;
    *)
      pass "linked: packet does not live under the per-worktree git dir"
      ;;
  esac

  assert_byte_identical "linked: content salvaged byte-identical" \
    "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected-linked.txt"
  if [[ -f "${salvage_dir}/.safe-pull-origin" ]] \
    && grep -qF "worktree=${linked}" "${salvage_dir}/.safe-pull-origin" \
    && grep -qF "branch=linked-wt" "${salvage_dir}/.safe-pull-origin" \
    && grep -qE '^utc=[0-9]{4}-' "${salvage_dir}/.safe-pull-origin"; then
    pass "linked: provenance names the originating worktree"
  else
    fail "linked: provenance names the originating worktree"
    printf 'provenance:\n'
    cat "${salvage_dir}/.safe-pull-origin" 2>/dev/null || printf '(missing)\n'
  fi

  git -C "$WORK" worktree remove --force "$linked"
  if [[ -e "$linked" ]]; then
    fail "linked: worktree remove deleted the worktree checkout"
  else
    pass "linked: worktree remove deleted the worktree checkout"
  fi
  assert_byte_identical "linked: packet survives worktree removal" \
    "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected-linked.txt"
  if [[ -f "${salvage_dir}/.safe-pull-origin" ]] \
    && grep -qF "worktree=${linked}" "${salvage_dir}/.safe-pull-origin"; then
    pass "linked: provenance survives worktree removal"
  else
    fail "linked: provenance survives worktree removal"
  fi
}

test_main_worktree_salvage_uses_common_dir() {
  setup_repos

  printf 'MAIN-KNOWN-17409\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-main.txt"
  advance_origin_notes "REMOTE-MAIN"

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "main-wt: safe-pull exits 0"
  else
    fail "main-wt: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi

  local salvage_dir common_dir git_dir
  salvage_dir="$(extract_salvage_dir "$out")"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "main-wt: salvage directory is printed"
    return
  fi
  pass "main-wt: salvage directory is printed"

  common_dir="$(git -C "$WORK" rev-parse --path-format=absolute --git-common-dir)"
  git_dir="$(git -C "$WORK" rev-parse --absolute-git-dir)"
  if [[ "$common_dir" == "$git_dir" ]]; then
    pass "main-wt: common dir equals git dir"
  else
    fail "main-wt: common dir equals git dir"
    printf 'common_dir=%s git_dir=%s\n' "$common_dir" "$git_dir"
  fi
  case "${salvage_dir}" in
    "${common_dir}/safe-pull-salvage/"*)
      pass "main-wt: packet lives under the common dir (unchanged in practice)"
      ;;
    *)
      fail "main-wt: packet lives under the common dir (unchanged in practice)"
      printf 'salvage_dir=%s common_dir=%s\n' "$salvage_dir" "$common_dir"
      ;;
  esac
  assert_byte_identical "main-wt: content salvaged byte-identical" \
    "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected-main.txt"
  assert_byte_identical "main-wt: pull delivers remote content" \
    "${WORK}/notes.txt" "${ADV}/notes.txt"
}

test_retention_prunes_only_expired_matching_packets() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  mkdir -p "$parent"
  local old="${parent}/2020-01-01T00-00-00Z-aaaaaa"
  mkdir -p "$old"
  printf 'old-payload\n' > "${old}/stale.txt"
  local nonmatch="${parent}/not-a-packet"
  mkdir -p "$nonmatch"
  printf 'nonmatch\n' > "${nonmatch}/stay.txt"
  local target="${TMPDIR_BASE}/symlink-target"
  mkdir -p "$target"
  printf 'symlink-payload\n' > "${target}/precious.txt"
  ln -s "$target" "${parent}/2020-01-01T00-00-00Z-bbbbbb"

  printf 'RETENTION-KNOWN-17409\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-retention.txt"
  advance_origin_notes "REMOTE-RETENTION"

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention: safe-pull exits 0"
  else
    fail "retention: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi

  local salvage_dir
  salvage_dir="$(extract_salvage_dir "$out")"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "retention: salvage directory is printed"
    return
  fi
  pass "retention: salvage directory is printed"
  assert_contains "retention: prune prints the expired packet path" "$out" "$old"
  if [[ -e "$old" ]]; then
    fail "retention: expired matching packet is pruned"
  else
    pass "retention: expired matching packet is pruned"
  fi
  if [[ -d "$salvage_dir" ]] && [[ -f "${salvage_dir}/notes.txt" ]]; then
    pass "retention: fresh packet survives"
  else
    fail "retention: fresh packet survives"
  fi
  if [[ -f "${nonmatch}/stay.txt" ]]; then
    pass "retention: non-matching directory survives"
  else
    fail "retention: non-matching directory survives"
  fi
  if [[ -L "${parent}/2020-01-01T00-00-00Z-bbbbbb" ]]; then
    pass "retention: matching-name symlink is not removed"
  else
    fail "retention: matching-name symlink is not removed"
  fi
  if [[ -f "${target}/precious.txt" ]] && grep -qF 'symlink-payload' "${target}/precious.txt"; then
    pass "retention: symlink target survives"
  else
    fail "retention: symlink target survives"
  fi
}

test_retention_zero_prunes_nothing() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/2020-01-01T00-00-00Z-cccccc"
  mkdir -p "$old"
  printf 'keep-me\n' > "${old}/stay.txt"

  printf 'RETENTION-ZERO-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-ZERO"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=0 run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-0: safe-pull exits 0"
  else
    fail "retention-0: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  local salvage_dir
  salvage_dir="$(extract_salvage_dir "$out")"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "retention-0: salvage directory is printed"
    return
  fi
  pass "retention-0: salvage directory is printed"
  assert_not_contains "retention-0: no prune output" "$out" "prune "
  if [[ -f "${old}/stay.txt" ]]; then
    pass "retention-0: expired matching packet is kept"
  else
    fail "retention-0: expired matching packet is kept"
  fi
}

test_retention_off_prunes_nothing() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/2020-01-01T00-00-00Z-eeeeee"
  mkdir -p "$old"
  printf 'keep-off\n' > "${old}/stay.txt"
  printf 'RETENTION-OFF-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-OFF"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=off run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-off: safe-pull exits 0"
  else
    fail "retention-off: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  local salvage_dir
  salvage_dir="$(extract_salvage_dir "$out")"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "retention-off: salvage directory is printed"
    return
  fi
  pass "retention-off: salvage directory is printed"
  assert_not_contains "retention-off: no prune output" "$out" "prune "
  if [[ -f "${old}/stay.txt" ]]; then
    pass "retention-off: expired matching packet is kept"
  else
    fail "retention-off: expired matching packet is kept"
  fi
}

test_retention_invalid_skips_prune_without_aborting() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/2020-01-01T00-00-00Z-ffffff"
  mkdir -p "$old"
  printf 'keep-invalid\n' > "${old}/stay.txt"
  printf 'RETENTION-ABC-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-ABC"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=abc run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-abc: safe-pull exits 0"
  else
    fail "retention-abc: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "retention-abc: warns and skips prune" "$out" "not a non-negative integer or 'off'"
  if [[ -f "${old}/stay.txt" ]]; then
    pass "retention-abc: expired packet is kept when prune is skipped"
  else
    fail "retention-abc: expired packet is kept when prune is skipped"
  fi
}

nine_day_packet_basename() {
  printf '%s-aaaaaa' "$(date -u -d '9 days ago' +%Y-%m-%dT%H-%M-%SZ)"
}

test_retention_leading_zero_ten_is_decimal() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/$(nine_day_packet_basename)"
  mkdir -p "$old"
  printf 'keep-under-ten\n' > "${old}/stay.txt"
  printf 'OCTAL-TEN-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-OCTAL-TEN"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=010 run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-010: safe-pull exits 0"
  else
    fail "retention-010: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  if [[ -f "${old}/stay.txt" ]]; then
    pass "retention-010: 9-day-old packet kept under decimal 10-day retention"
  else
    fail "retention-010: 9-day-old packet kept under decimal 10-day retention"
  fi
}

test_retention_leading_zero_eight_does_not_abort() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/$(nine_day_packet_basename)"
  mkdir -p "$old"
  printf 'prune-under-eight\n' > "${old}/stale.txt"
  printf 'OCTAL-EIGHT-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-OCTAL-EIGHT"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=08 run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-08: safe-pull exits 0"
  else
    fail "retention-08: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  if [[ -e "$old" ]]; then
    fail "retention-08: 9-day-old packet pruned under decimal 8-day retention"
  else
    pass "retention-08: 9-day-old packet pruned under decimal 8-day retention"
  fi
}

test_retention_oversized_skips_prune_without_aborting() {
  setup_repos

  local parent="${WORK}/.git/safe-pull-salvage"
  local old="${parent}/2020-01-01T00-00-00Z-dddddd"
  mkdir -p "$old"
  printf 'keep-when-skipping\n' > "${old}/stay.txt"
  printf 'OVERSIZE-17409\n' > "${WORK}/notes.txt"
  advance_origin_notes "REMOTE-OVERSIZE"

  local out rc
  rc=0
  out="$(SAFE_PULL_SALVAGE_RETENTION_DAYS=999999999999 run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "retention-oversize: safe-pull exits 0"
  else
    fail "retention-oversize: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "retention-oversize: warns and skips prune" "$out" "exceeds 36500 days"
  if [[ -f "${old}/stay.txt" ]]; then
    pass "retention-oversize: expired packet is kept when prune is skipped"
  else
    fail "retention-oversize: expired packet is kept when prune is skipped"
  fi
}

test_reserved_origin_path_is_refused() {
  setup_repos

  printf 'LOCAL-ORIGIN-17409\n' > "${WORK}/.safe-pull-origin"
  cp "${WORK}/.safe-pull-origin" "${TMPDIR_BASE}/expected-origin.txt"
  printf 'LOCAL-NOTES-17409\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-notes-reserved.txt"
  printf 'REMOTE-ORIGIN\n' > "${ADV}/.safe-pull-origin"
  printf 'REMOTE-NOTES\n' > "${ADV}/notes.txt"
  git -C "$ADV" add .safe-pull-origin notes.txt
  git -C "$ADV" commit -qm "add reserved and notes"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    pass "reserved: safe-pull refuses with nonzero exit"
  else
    fail "reserved: safe-pull refuses with nonzero exit (got 0)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "reserved: output names the reserved path" "$out" ".safe-pull-origin"
  assert_contains "reserved: output says nothing was moved" "$out" "nothing was moved"
  assert_byte_identical "reserved: .safe-pull-origin stays in place" \
    "${WORK}/.safe-pull-origin" "${TMPDIR_BASE}/expected-origin.txt"
  assert_byte_identical "reserved: other collider stays in place" \
    "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-notes-reserved.txt"
  if [[ -d "${WORK}/.git/safe-pull-salvage" ]]; then
    fail "reserved: no salvage packet is created on refusal"
  else
    pass "reserved: no salvage packet is created on refusal"
  fi
}

test_reserved_origin_prefix_is_refused() {
  setup_repos

  mkdir -p "${WORK}/.safe-pull-origin"
  printf 'LOCAL-NESTED-ORIGIN-17409\n' > "${WORK}/.safe-pull-origin/nested.txt"
  cp "${WORK}/.safe-pull-origin/nested.txt" "${TMPDIR_BASE}/expected-nested-origin.txt"
  printf 'LOCAL-NOTES-PREFIX-17409\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-notes-prefix.txt"
  mkdir -p "${ADV}/.safe-pull-origin"
  printf 'REMOTE-NESTED-ORIGIN\n' > "${ADV}/.safe-pull-origin/nested.txt"
  printf 'REMOTE-NOTES\n' > "${ADV}/notes.txt"
  git -C "$ADV" add .safe-pull-origin/nested.txt notes.txt
  git -C "$ADV" commit -qm "add reserved prefix and notes"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    pass "reserved-prefix: safe-pull refuses with nonzero exit"
  else
    fail "reserved-prefix: safe-pull refuses with nonzero exit (got 0)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "reserved-prefix: output names the reserved path" "$out" ".safe-pull-origin/nested.txt"
  assert_contains "reserved-prefix: output says nothing was moved" "$out" "nothing was moved"
  assert_byte_identical "reserved-prefix: nested collider stays in place" \
    "${WORK}/.safe-pull-origin/nested.txt" "${TMPDIR_BASE}/expected-nested-origin.txt"
  assert_byte_identical "reserved-prefix: other collider stays in place" \
    "${WORK}/notes.txt" "${TMPDIR_BASE}/expected-notes-prefix.txt"
  if [[ -d "${WORK}/.git/safe-pull-salvage" ]]; then
    fail "reserved-prefix: no salvage packet is created on refusal"
  else
    pass "reserved-prefix: no salvage packet is created on refusal"
  fi
}

test_colliding_untracked_file_survives_with_content
test_clean_pull_still_succeeds
test_tracked_blockage_aborts_without_moving
test_nested_invocation_salvages_from_root
test_staged_delete_collision_refuses_without_moving
test_linked_worktree_packet_survives_removal
test_main_worktree_salvage_uses_common_dir
test_retention_prunes_only_expired_matching_packets
test_retention_zero_prunes_nothing
test_retention_off_prunes_nothing
test_retention_invalid_skips_prune_without_aborting
test_retention_leading_zero_ten_is_decimal
test_retention_leading_zero_eight_does_not_abort
test_retention_oversized_skips_prune_without_aborting
test_reserved_origin_path_is_refused
test_reserved_origin_prefix_is_refused

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
