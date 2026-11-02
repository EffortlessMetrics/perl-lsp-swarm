#!/usr/bin/env bash
# scripts/tests/test-clean-tmp-targets.sh
#
# Hermetic controls for scripts/clean-tmp-targets.sh (#17594).
#
# INVARIANTS:
#   * advisory default never deletes;
#   * --prune and APPLY=1 refuse with exit 78 before discovery (even with --help/--dry-run);
#   * live worktree basename matches are not reported as age/name candidates;
#   * failed git worktree observation refuses before listing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMPL="$SCRIPT_DIR/../clean-tmp-targets.sh"
PASS_COUNT=0
FAIL_COUNT=0

if [[ ! -f "$IMPL" ]]; then
    echo "ERROR: clean-tmp-targets.sh not found at $IMPL"
    exit 1
fi

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

FAKE_TMP="$(mktemp -d)"
trap 'rm -rf "$FAKE_TMP"' EXIT

make_mock_git() {
    local mock_dir porcelain_output worktree_rc
    mock_dir="$(mktemp -d "$FAKE_TMP/mock-git-XXXX")"
    porcelain_output="$1"
    worktree_rc="${2:-0}"
    cat > "$mock_dir/git" <<'MOCK_HEADER'
#!/usr/bin/env bash
MOCK_HEADER
    printf 'CANNED=%q\n' "$porcelain_output" >> "$mock_dir/git"
    printf 'WORKTREE_RC=%q\n' "$worktree_rc" >> "$mock_dir/git"
    cat >> "$mock_dir/git" <<'MOCK_BODY'
if [[ "${*}" == *"worktree list --porcelain"* ]]; then
    printf '%s\n' "$CANNED"
    exit "$WORKTREE_RC"
fi
if [[ "${*}" == *"rev-parse --show-toplevel"* ]]; then
    echo "/fake/repo"
    exit 0
fi
exec git "$@"
MOCK_BODY
    chmod +x "$mock_dir/git"
    echo "$mock_dir"
}

TEST_PREFIX="clean-tmp-test-$$"

make_live_target() {
    local wt_id="$1"
    local full="/tmp/agent-${wt_id}-target"
    mkdir -p "$full"
    touch -t 200001010000.00 "$full"
    echo "$full"
}

make_orphan_target() {
    local suffix="$1"
    local full="/tmp/agent-orphan-${TEST_PREFIX}-${suffix}-target"
    mkdir -p "$full"
    touch -t 200001010000.00 "$full"
    echo "$full"
}

remove_fake_targets() {
    rm -rf /tmp/"${TEST_PREFIX}"-* 2>/dev/null || true
}

run_reaper_advisory() {
    local mock_git_dir="$1"
    PATH="$mock_git_dir:$PATH" bash "$IMPL" 2>&1
}

test_live_target_never_reported_as_candidate() {
    local wt_id="livetest-${TEST_PREFIX}"
    local live_dir orphan_dir output

    live_dir="$(make_live_target "$wt_id")"
    orphan_dir="$(make_orphan_target "safety1")"

    local porcelain
    porcelain="$(printf 'worktree /some/base/.claude/worktrees/agent-%s\nHEAD abc123\nbranch refs/heads/test\n' "$wt_id")"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    output="$(run_reaper_advisory "$mock_git" 2>&1)"

    rm -rf "$live_dir" "$orphan_dir" "$mock_git"

    if echo "$output" | grep "CANDIDATE" | grep -q "agent-${wt_id}-target"; then
        fail "live target incorrectly reported as CANDIDATE: $output"
    else
        pass "live target is NOT reported as a candidate"
    fi
}

test_orphan_target_is_reported() {
    local orphan_dir output

    orphan_dir="$(make_orphan_target "report1")"

    local porcelain
    porcelain="$(printf 'worktree /some/repo\nHEAD abc123\nbranch refs/heads/main\n')"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    output="$(run_reaper_advisory "$mock_git" 2>&1)"

    rm -rf "$orphan_dir" "$mock_git"

    if echo "$output" | grep -q "CANDIDATE.*agent-orphan-${TEST_PREFIX}-report1-target"; then
        pass "age/name candidate correctly reported"
    else
        fail "candidate not found in output — got: $output"
    fi
}

test_advisory_does_not_delete() {
    local orphan_dir output

    orphan_dir="$(make_orphan_target "dryrun1")"

    local porcelain
    porcelain="$(printf 'worktree /some/repo\nHEAD abc123\nbranch refs/heads/main\n')"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    output="$(run_reaper_advisory "$mock_git" 2>&1)"

    local still_exists=0
    [[ -d "$orphan_dir" ]] && still_exists=1

    rm -rf "$orphan_dir" "$mock_git"

    if [[ "$still_exists" -eq 1 ]]; then
        pass "advisory run does NOT delete dirs"
    else
        fail "advisory run deleted a dir (should not)"
    fi
}

test_mixed_dirs_only_candidates_reported() {
    local wt_id="mixedlive-${TEST_PREFIX}"
    local live_dir orphan1 orphan2 output

    live_dir="$(make_live_target "$wt_id")"
    orphan1="$(make_orphan_target "mix1")"
    orphan2="$(make_orphan_target "mix2")"

    local porcelain
    porcelain="$(printf 'worktree /repo/.claude/worktrees/agent-%s\nHEAD abc\nbranch refs/heads/x\n' "$wt_id")"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    output="$(run_reaper_advisory "$mock_git" 2>&1)"

    rm -rf "$live_dir" "$orphan1" "$orphan2" "$mock_git"

    local ok=1

    if echo "$output" | grep "CANDIDATE" | grep -q "agent-${wt_id}-target"; then
        fail "live target incorrectly in CANDIDATE list"
        ok=0
    fi

    if ! echo "$output" | grep -q "CANDIDATE.*agent-orphan-${TEST_PREFIX}-mix1-target"; then
        fail "candidate1 not found in CANDIDATE list"
        ok=0
    fi

    if ! echo "$output" | grep -q "CANDIDATE.*agent-orphan-${TEST_PREFIX}-mix2-target"; then
        fail "candidate2 not found in CANDIDATE list"
        ok=0
    fi

    [[ "$ok" -eq 1 ]] && pass "mixed dirs: only age/name candidates reported"
}

test_recent_dir_is_skipped() {
    local orphan_dir output

    orphan_dir="$(make_orphan_target "recent1")"
    touch "$orphan_dir"

    local porcelain
    porcelain="$(printf 'worktree /some/repo\nHEAD abc123\nbranch refs/heads/main\n')"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    output="$(run_reaper_advisory "$mock_git" 2>&1)"

    rm -rf "$orphan_dir" "$mock_git"

    if echo "$output" | grep -q "RECENT.*agent-orphan-${TEST_PREFIX}-recent1-target"; then
        pass "recently-modified dir listed as RECENT (informational)"
    elif echo "$output" | grep "CANDIDATE" | grep -q "agent-orphan-${TEST_PREFIX}-recent1-target"; then
        fail "recently-modified dir incorrectly listed as candidate"
    else
        pass "recently-modified dir not listed as candidate"
    fi
}

test_prune_flag_refuses_exit_78() {
    local orphan_dir rc output

    orphan_dir="$(make_orphan_target "prune1")"

    local porcelain
    porcelain="$(printf 'worktree /some/repo\nHEAD abc123\nbranch refs/heads/main\n')"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    set +e
    output="$(PATH="$mock_git:$PATH" bash "$IMPL" --prune 2>&1)"
    rc=$?
    set -e

    local still_exists=0
    [[ -d "$orphan_dir" ]] && still_exists=1

    rm -rf "$orphan_dir" "$mock_git" 2>/dev/null || true

    if [[ "$rc" -eq 78 && "$still_exists" -eq 1 ]]; then
        pass "--prune refuses with exit 78 and does not delete"
    else
        fail "--prune expected exit 78 and no delete; rc=$rc exists=$still_exists output=$output"
    fi
}

test_apply_env_refuses_exit_78() {
    local orphan_dir rc output

    orphan_dir="$(make_orphan_target "apply1")"

    local porcelain
    porcelain="$(printf 'worktree /some/repo\nHEAD abc123\nbranch refs/heads/main\n')"

    local mock_git
    mock_git="$(make_mock_git "$porcelain")"

    set +e
    output="$(APPLY=1 PATH="$mock_git:$PATH" bash "$IMPL" 2>&1)"
    rc=$?
    set -e

    local still_exists=0
    [[ -d "$orphan_dir" ]] && still_exists=1

    rm -rf "$orphan_dir" "$mock_git" 2>/dev/null || true

    if [[ "$rc" -eq 78 && "$still_exists" -eq 1 ]]; then
        pass "APPLY=1 refuses with exit 78 and does not delete"
    else
        fail "APPLY=1 expected exit 78 and no delete; rc=$rc exists=$still_exists output=$output"
    fi
}

test_prune_before_help_still_refuses() {
    local rc output

    set +e
    output="$(bash "$IMPL" --prune --help 2>&1)"
    rc=$?
    set -e

    if [[ "$rc" -eq 78 ]] && ! echo "$output" | grep -q '^Usage:'; then
        pass "--prune --help refuses before help output"
    else
        fail "--prune --help should exit 78 before help; rc=$rc output=$output"
    fi
}

test_apply_with_dry_run_flag_still_refuses() {
    local rc output

    set +e
    output="$(APPLY=1 bash "$IMPL" --dry-run 2>&1)"
    rc=$?
    set -e

    if [[ "$rc" -eq 78 ]]; then
        pass "APPLY=1 with --dry-run still refuses with exit 78"
    else
        fail "APPLY=1 --dry-run expected exit 78; rc=$rc output=$output"
    fi
}

test_git_worktree_failure_refuses() {
    local orphan_dir rc output

    orphan_dir="$(make_orphan_target "gitfail1")"

    local mock_git
    mock_git="$(make_mock_git "" 1)"

    set +e
    output="$(PATH="$mock_git:$PATH" bash "$IMPL" 2>&1)"
    rc=$?
    set -e

    local still_exists=0
    [[ -d "$orphan_dir" ]] && still_exists=1

    rm -rf "$orphan_dir" "$mock_git" 2>/dev/null || true

    if [[ "$rc" -eq 75 && "$still_exists" -eq 1 ]] && echo "$output" | grep -qi 'REFUSING.*worktree'; then
        pass "git worktree observation failure refuses before listing"
    else
        fail "expected exit 75 on git failure; rc=$rc output=$output"
    fi
}

test_advisory_disclaimer_present() {
    local output
    output="$(bash "$IMPL" 2>&1)" || true

    if echo "$output" | grep -q 'no ownership or reclaimability proof'; then
        pass "advisory disclaimer is present"
    else
        fail "missing advisory disclaimer in output: $output"
    fi
}

echo "=== clean-tmp-targets safety test suite ==="
echo ""

test_live_target_never_reported_as_candidate
test_orphan_target_is_reported
test_advisory_does_not_delete
test_mixed_dirs_only_candidates_reported
test_recent_dir_is_skipped
test_prune_flag_refuses_exit_78
test_apply_env_refuses_exit_78
test_prune_before_help_still_refuses
test_apply_with_dry_run_flag_still_refuses
test_git_worktree_failure_refuses
test_advisory_disclaimer_present

remove_fake_targets

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
    exit 1
fi
exit 0
