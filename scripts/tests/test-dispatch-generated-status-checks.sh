#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SUBJECT="$ROOT/scripts/ci/dispatch-generated-status-checks.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; exit 1; }

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'STUB'
#!/usr/bin/env bash
{
    printf '%s' "$#"
    printf '|%s' "$@"
    printf '\n'
} >>"$GH_LOG"
if [[ "${3:-}" == "ci.yml" ]]; then
    if [[ "$#" -ne 9 || "${6:-}" != "-f" || ! "${7:-}" =~ ^base_sha=[0-9a-f]{40}$ \
        || "${8:-}" != "-f" || ! "${9:-}" =~ ^head_sha=[0-9a-f]{40}$ ]]; then
        echo "HTTP 422: Required input 'base_sha' not provided" >&2
        exit 1
    fi
elif [[ "$#" -ne 5 ]]; then
    exit 2
fi
if [[ "${FAIL_WORKFLOW:-}" == "${3:-}" ]]; then
    exit 1
fi
STUB
chmod +x "$TMP/bin/gh"

BASE_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
HEAD_SHA=cccccccccccccccccccccccccccccccccccccccc
BRANCH=automation/post-merge-status
EXPECTED=$(cat <<EOF
9|workflow|run|ci.yml|--ref|$BRANCH|-f|base_sha=$BASE_SHA|-f|head_sha=$HEAD_SHA
5|workflow|run|em-ci-routed-rust.yml|--ref|$BRANCH
5|workflow|run|ripr.yml|--ref|$BRANCH
5|workflow|run|pr-title-check.yml|--ref|$BRANCH
EOF
)

run_subject() {
    local log="$1" failure="${2:-}"
    shift 2 || true
    GH_LOG="$log" FAIL_WORKFLOW="$failure" PATH="$TMP/bin:$PATH" \
        bash "$SUBJECT" "$@"
}

legacy_log="$TMP/legacy.log"
legacy_error="$TMP/legacy.err"
if GH_LOG="$legacy_log" PATH="$TMP/bin:$PATH" \
    gh workflow run ci.yml --ref "$BRANCH" 2>"$legacy_error"; then
    fail "legacy ci.yml dispatch without required inputs was accepted"
fi
grep -Fq "HTTP 422: Required input 'base_sha' not provided" "$legacy_error" \
    || fail "legacy omitted-base dispatch did not reproduce the observed 422"
pass "legacy omitted-base ci.yml dispatch reproduces the observed 422"

success_log="$TMP/success.log"
run_subject "$success_log" "" "$BRANCH" "$BASE_SHA" "$BASE_SHA" "$HEAD_SHA" \
    || fail "all-success dispatch returned non-zero"
[[ "$(cat "$success_log")" == "$EXPECTED" ]] \
    || fail "dispatch argv did not bind base_sha only to ci.yml"
pass "exact argv preserves all four dispatch contracts"

failure_log="$TMP/failure.log"
if run_subject "$failure_log" "ci.yml" "$BRANCH" "$BASE_SHA" "$BASE_SHA" "$HEAD_SHA"; then
    fail "one failed dispatch returned success"
fi
[[ "$(cat "$failure_log")" == "$EXPECTED" ]] \
    || fail "one failure prevented later workflow attempts"
pass "one failure still attempts all workflows and fails at the end"

assert_refused_without_dispatch() {
    local name="$1"
    shift
    local log="$TMP/$name.log"
    if run_subject "$log" "" "$@"; then
        fail "$name was accepted"
    fi
    [[ ! -e "$log" ]] || fail "$name reached gh before refusal"
    pass "$name fails closed before dispatch"
}

assert_refused_without_dispatch missing-base "$BRANCH" "" "$BASE_SHA" "$HEAD_SHA"
assert_refused_without_dispatch missing-branch "" "$BASE_SHA" "$BASE_SHA" "$HEAD_SHA"
assert_refused_without_dispatch malformed-base "$BRANCH" abc "$BASE_SHA" "$HEAD_SHA"
assert_refused_without_dispatch uppercase-base "$BRANCH" AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA "$BASE_SHA" "$HEAD_SHA"
assert_refused_without_dispatch mismatched-base "$BRANCH" bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb "$BASE_SHA" "$HEAD_SHA"
assert_refused_without_dispatch malformed-source "$BRANCH" "$BASE_SHA" abc "$HEAD_SHA"
assert_refused_without_dispatch missing-head "$BRANCH" "$BASE_SHA" "$BASE_SHA" ""
assert_refused_without_dispatch malformed-head "$BRANCH" "$BASE_SHA" "$BASE_SHA" abc

printf 'All generated-status dispatch checks passed.\n'
