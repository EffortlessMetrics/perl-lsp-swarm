#!/usr/bin/env bash
# Offline tests for review leases and evidence-backed thread dispositions.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LEASE="$SCRIPT_DIR/../reviews/lease"
DISPOSITION="$SCRIPT_DIR/../reviews/disposition"
PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

for required in "$LEASE" "$DISPOSITION"; do
    [[ -f "$required" ]] || { echo "ERROR: review script not found at $required"; exit 1; }
done
if ! command -v jq >/dev/null 2>&1; then echo "ERROR: jq not found on PATH"; exit 1; fi

TMPDIR_REVIEW="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_REVIEW"' EXIT
export REVIEW_LEASES_DIR="$TMPDIR_REVIEW/review-leases"

run() { local e=0; RUN_OUT="$(REVIEW_LEASES_DIR="$REVIEW_LEASES_DIR" bash "$LEASE" "$@" 2>&1)" || e=$?; RUN_EXIT=$e; }

# ── acquire → verify passes ─────────────────────────────────────────────────
test_acquire_then_verify() {
    run acquire --branch feat/3693-x --owner alice --pr 42
    local a=$RUN_EXIT
    run verify --branch feat/3693-x
    if [[ "$a" -eq 0 && "$RUN_EXIT" -eq 0 ]]; then
        pass "acquire then verify: unexpired lease verifies (exit 0)"
    else
        fail "acquire/verify — acquire exit=$a verify exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── verify on an absent branch fails (exit 1) ──────────────────────────────
test_verify_absent_fails() {
    run verify --branch never-leased
    if [[ "$RUN_EXIT" -eq 1 ]]; then
        pass "verify on an absent lease fails (exit 1)"
    else
        fail "verify absent — expected exit 1, got $RUN_EXIT out=$RUN_OUT"
    fi
}

# ── expired lease: verify fails + audit emits a takeover-candidate line ─────
# THIS is fixture 11: an expired lease is the takeover trigger, surfaced by
# the lease suite (not the convergence closeout).
test_expired_lease_blocks_and_audits() {
    # Acquire with a 0-minute TTL so it is already expired.
    run acquire --branch stale-branch --owner bob --ttl-min 0
    # Force the epoch clearly into the past to avoid same-second flakiness.
    local path="$REVIEW_LEASES_DIR/stale-branch.json"
    local past=$(( $(date -u +%s) - 3600 ))
    jq --argjson e "$past" '.expires_at_epoch = $e' "$path" > "$path.tmp" && mv "$path.tmp" "$path"

    run verify --branch stale-branch
    local v=$RUN_EXIT
    run audit
    if [[ "$v" -eq 1 && "$RUN_EXIT" -eq 0 ]] && echo "$RUN_OUT" | grep -q "TAKEOVER-CANDIDATE.*stale-branch"; then
        pass "expired lease: verify fails (exit 1) AND audit emits a takeover-candidate line (route-takeover)"
    else
        fail "expired lease — verify exit=$v audit exit=$RUN_EXIT audit out=$RUN_OUT"
    fi
}

# ── a different owner cannot steal an unexpired lease ───────────────────────
test_acquire_refuses_other_owner() {
    run acquire --branch owned-branch --owner alice --ttl-min 120
    local a=$RUN_EXIT
    run acquire --branch owned-branch --owner mallory --ttl-min 120
    if [[ "$a" -eq 0 && "$RUN_EXIT" -eq 1 ]]; then
        pass "acquire refuses a different owner while the lease is unexpired (exit 1)"
    else
        fail "acquire-steal — first exit=$a second exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── same owner may refresh its own lease ────────────────────────────────────
test_same_owner_refreshes() {
    run acquire --branch refresh-branch --owner alice --ttl-min 120
    local a=$RUN_EXIT
    run acquire --branch refresh-branch --owner alice --ttl-min 120
    if [[ "$a" -eq 0 && "$RUN_EXIT" -eq 0 ]]; then
        pass "same owner may refresh its own unexpired lease (exit 0)"
    else
        fail "same-owner-refresh — first exit=$a second exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── release by the holder, then verify fails ────────────────────────────────
test_release_then_verify_fails() {
    run acquire --branch rel-branch --owner alice --ttl-min 120
    run release --branch rel-branch --owner alice
    local r=$RUN_EXIT
    run verify --branch rel-branch
    if [[ "$r" -eq 0 && "$RUN_EXIT" -eq 1 ]]; then
        pass "release by holder then verify fails (release exit 0, verify exit 1)"
    else
        fail "release/verify — release exit=$r verify exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── release by a non-holder is refused ──────────────────────────────────────
test_release_non_holder_refused() {
    run acquire --branch held2 --owner alice --ttl-min 120
    run release --branch held2 --owner mallory
    if [[ "$RUN_EXIT" -eq 1 ]]; then
        pass "release by a non-holder is refused (exit 1)"
    else
        fail "release-non-holder — expected exit 1, got $RUN_EXIT out=$RUN_OUT"
    fi
}

# ── written lease conforms to the review-lease schema shape ────────────────
test_lease_json_shape() {
    run acquire --branch shape-branch --owner alice --pr 7 --base-sha abc123
    local path="$REVIEW_LEASES_DIR/shape-branch.json"
    if jq -e '.v == 1 and .branch == "shape-branch" and .owner == "alice" and .pr == 7 and (.expires_at_epoch > .acquired_at_epoch) and (.base_sha == "abc123")' "$path" >/dev/null; then
        pass "written lease has the expected schema shape (v/branch/owner/pr/epochs/base_sha)"
    else
        fail "lease shape — $(cat "$path")"
    fi
}

# ── disposition fake-GitHub seam ───────────────────────────────────────────
FAKE_BIN="$TMPDIR_REVIEW/fake-bin"
FAKE_LOG="$TMPDIR_REVIEW/gh-mutations.log"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
args="$*"
log="${FAKE_GH_LOG:?}"
mode="${FAKE_GH_MODE:-ok}"

if [[ "$args" == *'query($threadId: ID!)'* ]]; then
    [[ "$mode" != "query-fail" ]] || exit 41
    jq -cn --arg body "${FAKE_EXISTING_BODY:-}" '{
      data:{node:{isResolved:false,comments:{nodes:[{body:$body}]}}}
    }'
    exit 0
fi
if [[ "$args" == *'addPullRequestReviewThreadReply'* ]]; then
    printf 'reply\n' >>"$log"
    printf '%s\n' '{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"C1"}}}}'
    exit 0
fi
if [[ "$args" == *'resolveReviewThread'* ]]; then
    printf 'resolve\n' >>"$log"
    printf '%s\n' '{"data":{"resolveReviewThread":{"thread":{"id":"THREAD","isResolved":true}}}}'
    exit 0
fi

echo "unexpected fake gh invocation: $args" >&2
exit 42
EOF
chmod +x "$FAKE_BIN/gh"

run_disposition() {
    local mode="$1" existing_body="$2" commit="$3"
    : >"$FAKE_LOG"
    DISPOSITION_EXIT=0
    DISPOSITION_OUT="$(
      PATH="$FAKE_BIN:$PATH" \
      FAKE_GH_LOG="$FAKE_LOG" \
      FAKE_GH_MODE="$mode" \
      FAKE_EXISTING_BODY="$existing_body" \
      bash "$DISPOSITION" \
        --pr 42 \
        --thread THREAD \
        --class fixed \
        --reply "Disposition: fixed" \
        --commit "$commit" \
        --repo owner/repo \
        --head h2 \
        --by tester 2>&1
    )" || DISPOSITION_EXIT=$?
}

existing_h1=$'Prior repair.\n\n<!-- disposition:v1 {"v":1,"class":"fixed","thread_id":"THREAD","by":"tester","head":"h1","evidence":{"commit":"abc"}} -->'

# ── unrelated head movement reuses the stable disposition ──────────────────
# @risk: an unrelated candidate SHA duplicates a previously supported disposition reply.
# @return_path: a matching class and evidence marker is reused regardless of observed head.
# @side_effect: re-resolution is allowed; no new review reply is posted.
test_disposition_reuses_h1_at_h2() {
    run_disposition ok "$existing_h1" abc
    local mutations
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 0 && "$mutations" == "resolve" && "$DISPOSITION_OUT" == *"without duplicate reply"* ]]; then
        pass "matching H1 disposition is reused at H2 without duplicate reply"
    else
        fail "stable disposition reuse — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
    fi
}

# ── changed evidence is a new disposition ──────────────────────────────────
# @risk: stable idempotency suppresses a materially different repair disposition.
# @return_path: changed evidence is treated as a new supported disposition.
# @side_effect: one evidence-bearing reply is posted before the thread is resolved.
test_disposition_posts_changed_evidence() {
    run_disposition ok "$existing_h1" def
    local mutations
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 0 && "$mutations" == "reply,resolve" ]]; then
        pass "changed evidence posts one new reply before resolution"
    else
        fail "changed evidence — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
    fi
}

# ── unavailable provider state causes zero mutation ─────────────────────────
# @risk: an unavailable GitHub observation is misclassified as a proven zero match.
# @return_path: provider failure returns exit 2 and preserves unknown disposition state.
# @side_effect: neither a review reply nor a thread-resolution mutation is permitted.
test_disposition_provider_failure_is_inert() {
    run_disposition query-fail "" abc
    local mutations
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 2 && -z "$mutations" ]]; then
        pass "provider failure exits 2 with no reply or resolution mutation"
    else
        fail "provider failure — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
    fi
}

# ── malformed historical marker causes zero mutation ───────────────────────
# @risk: malformed historical evidence is silently discarded and replaced with a duplicate reply.
# @return_path: marker parse failure returns exit 2 for explicit repair or adjudication.
# @side_effect: neither a review reply nor a thread-resolution mutation is permitted.
test_disposition_malformed_marker_is_inert() {
    local malformed
    malformed=$'Broken marker.\n\n<!-- disposition:v1 {not-json} -->\n\n'
    malformed+="$existing_h1"
    run_disposition ok "$malformed" abc
    local mutations
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 2 && -z "$mutations" && "$DISPOSITION_OUT" == *"malformed disposition marker"* ]]; then
        pass "malformed marker exits 2 with no reply or resolution mutation"
    else
        fail "malformed marker — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
    fi
}

echo "=== review lease + disposition test suite ==="
echo ""
test_acquire_then_verify
test_verify_absent_fails
test_expired_lease_blocks_and_audits
test_acquire_refuses_other_owner
test_same_owner_refreshes
test_release_then_verify_fails
test_release_non_holder_refused
test_lease_json_shape
test_disposition_reuses_h1_at_h2
test_disposition_posts_changed_evidence
test_disposition_provider_failure_is_inert
test_disposition_malformed_marker_is_inert
echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then exit 1; fi
exit 0
