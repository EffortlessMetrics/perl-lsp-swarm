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

# ── lease v1 — happy-path regression: audit reports v1 leases correctly ────
# @risk: validate_lease_file rejects an otherwise-valid v1 lease because of a
#        schema-string drift.
# @return_path: a v1 lease with valid expires_at_epoch is read by audit and
#        counted as ACTIVE.
# @side_effect: no exit, no overwrite; audit prints the v1 summary line and
#        the new branch appears in the active tally (delta +1 vs the prior
#        state).
test_audit_v1_lease_reports_active() {
    # Snapshot the active count before this test.
    run audit
    local before
    before="$(printf '%s\n' "$RUN_OUT" | sed -nE 's/.*[^0-9]([0-9]+) active.*/\1/p')"
    [[ -n "$before" ]] || before=0
    run acquire --branch v1-active-branch --owner alice --ttl-min 120
    local a=$RUN_EXIT
    run audit
    local after
    after="$(printf '%s\n' "$RUN_OUT" | sed -nE 's/.*[^0-9]([0-9]+) active.*/\1/p')"
    if [[ "$a" -eq 0 && "$RUN_EXIT" -eq 0 && "$after" -eq "$((before + 1))" ]]; then
        pass "audit reports a v1 unexpired lease as active (active count $before -> $after)"
    else
        fail "v1-audit-active — acquire exit=$a audit exit=$RUN_EXIT before=$before after=$after out=$RUN_OUT"
    fi
}

# ── lease v2 (future schema) — verify fails loud (exit 2), not silent ───────
# @risk: an unrecognized v2 lease silently parses with // 0 / // "?" defaults
#        and verify classifies it as EXPIRED — the wrong-action path that
#        #15283 was filed against.
# @return_path: validate_lease_file refuses with exit 2 and a clear
#        "unsupported version" error; verify never runs.
# @side_effect: the file is unchanged; the audit summary line is not printed.
test_verify_v2_lease_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/v2-verify-branch.json"
    # A v2 envelope with renamed fields (owner → held_by, expires_at_epoch →
    # lease_expires_at) — the exact case the issue body describes.
    jq -n '{v:2, branch:"v2-verify-branch", held_by:"alice",
            lease_expires_at:2147483647, pr:42, base_sha:"abc"}' > "$path"
    run verify --branch v2-verify-branch
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version"; then
        pass "verify refuses a v2 lease with a clear unsupported-version error (exit 2)"
    else
        fail "verify-v2 — expected exit 2 with 'unsupported version', got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── lease v2 — release fails loud (exit 2) ──────────────────────────────────
# @risk: release silently reads .owner from a v2 lease and REFUSEs a release
#        the user should not have been blocked from making, OR accepts a
#        release for a lease that does not actually exist under the v1 shape.
# @return_path: release refuses with exit 2 and an unsupported-version error
#        before the .owner field is read.
test_release_v2_lease_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/v2-release-branch.json"
    jq -n '{v:2, branch:"v2-release-branch", held_by:"alice",
            lease_expires_at:2147483647, pr:42, base_sha:"abc"}' > "$path"
    run release --branch v2-release-branch --owner alice
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version"; then
        pass "release refuses a v2 lease with a clear unsupported-version error (exit 2)"
    else
        fail "release-v2 — expected exit 2 with 'unsupported version', got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── lease v2 — audit fails loud (exit 2) ────────────────────────────────────
# @risk: audit iterates every .json in the leases dir and silently emits
#        TAKEOVER-CANDIDATE for a v2 lease with the // 0 / // "?" defaults —
#        the operational defect the issue body documents.
# @return_path: validate_lease_file refuses the v2 lease inside the audit
#        loop with exit 2 and an unsupported-version error; the wrong-action
#        TAKEOVER-CANDIDATE line is not emitted.
test_audit_v2_lease_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/v2-audit-branch.json"
    jq -n '{v:2, branch:"v2-audit-branch", held_by:"alice",
            lease_expires_at:2147483647, pr:42, base_sha:"abc"}' > "$path"
    run audit
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version" && ! echo "$RUN_OUT" | grep -q "TAKEOVER-CANDIDATE.*v2-audit-branch"; then
        pass "audit refuses a v2 lease with a clear unsupported-version error (exit 2, no wrong-action TAKEOVER-CANDIDATE)"
    else
        fail "audit-v2 — expected exit 2 + 'unsupported version' + no TAKEOVER-CANDIDATE line, got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── acquire against a v2 lease refuses with exit 2 — no silent overwrite ────
# @risk: acquire silently reads a v2 lease's // "?" .owner and proceeds to
#        overwrite the file with a fresh v1 lease, losing the historical
#        record without surfacing that the prior envelope was unreadable.
# @return_path: validate_lease_file refuses with exit 2 before the existing
#        .owner field is read; the original v2 file is unchanged on disk
#        (acquire does not get the chance to clobber it).
test_acquire_v2_lease_refused_no_overwrite() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/v2-acquire-branch.json"
    local original
    original="$(jq -c '.' <<<'{"v":2,"branch":"v2-acquire-branch","held_by":"alice","lease_expires_at":2147483647}')"
    echo "$original" > "$path"
    run acquire --branch v2-acquire-branch --owner bob --ttl-min 120
    local after
    after="$(cat "$path")"
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version" && [[ "$after" == "$original" ]]; then
        pass "acquire refuses a v2 lease without overwriting it (exit 2, file unchanged)"
    else
        fail "acquire-v2 — exit=$RUN_EXIT out=$RUN_OUT after=$after"
    fi
}

# ── malformed (non-JSON) lease — verify fails loud (exit 2) ────────────────
# @risk: a non-JSON file in the leases dir parses through `jq` with a
#        non-zero exit, and validate_lease_file's jq guard rejects it.
# @return_path: validate_lease_file's `jq` guard fails with exit 2 and a
#        clear "not valid JSON" error rather than falling through to the
#        // "?" / // 0 default-read path.
test_verify_malformed_lease_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/malformed-verify-branch.json"
    printf 'this is not JSON at all\n' > "$path"
    run verify --branch malformed-verify-branch
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "not valid JSON"; then
        pass "verify refuses a malformed lease with a clear not-valid-JSON error (exit 2)"
    else
        fail "verify-malformed — expected exit 2 + 'not valid JSON', got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── missing-v lease (valid JSON, but no `v` field) — verify fails loud ─────
# @risk: a lease missing the `v` field silently parses through validate_lease_file
#        and reaches the // 0 / // "?" default-read path.
# @return_path: validate_lease_file's jq guard fails (v defaults to "missing")
#        with exit 2 and the unsupported-version error.
test_verify_missing_v_field_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/missing-v-branch.json"
    jq -n '{branch:"missing-v-branch", owner:"alice", expires_at_epoch:2147483647,
            acquired_at_epoch:1, acquired_at:"1970-01-01T00:00:01Z",
            expires_at:"2038-01-19T03:14:07Z"}' > "$path"
    run verify --branch missing-v-branch
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version"; then
        pass "verify refuses a missing-v lease with a clear unsupported-version error (exit 2)"
    else
        fail "verify-missing-v — expected exit 2 + 'unsupported version', got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── string-v lease (valid JSON, but `v` is the string "1") — refused ─────────
# @risk: a string "1" renders identically to numeric 1 under `jq -r`, so a
#        string comparison lets a mistyped envelope pass the version gate.
# @return_path: validate_lease_file compares numerically (`.v == 1`) and
#        refuses with exit 2 and the unsupported-version error.
test_verify_string_v_field_refused() {
    mkdir -p "$REVIEW_LEASES_DIR"
    local path="$REVIEW_LEASES_DIR/string-v-branch.json"
    jq -n '{v:"1", branch:"string-v-branch", owner:"alice", expires_at_epoch:2147483647,
            acquired_at_epoch:1, acquired_at:"1970-01-01T00:00:01Z",
            expires_at:"2038-01-19T03:14:07Z"}' > "$path"
    run verify --branch string-v-branch
    if [[ "$RUN_EXIT" -eq 2 ]] && echo "$RUN_OUT" | grep -q "unsupported version"; then
        pass "verify refuses a string-v lease with a clear unsupported-version error (exit 2)"
    else
        fail "verify-string-v — expected exit 2 + 'unsupported version', got exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ── swap-after-validation — verify acts on the validated snapshot ──────────
# @risk: validate_lease_file passes on a v1 read, then a concurrent writer
#        replaces the mutable path with a v2 envelope before owner/expiry are
#        read; the // "?" / // 0 defaults then take the wrong-action EXPIRED
#        path this PR is meant to eliminate.
# @return_path: the command reads the bytes once; fields come from the
#        validated snapshot, so verify still reports the original v1 owner.
# @side_effect: the on-disk file is left as the swapped v2 content (the test
#        simulates the race); the command output reflects the snapshot.
test_verify_uses_snapshot_despite_swap_after_validation() {
    run acquire --branch snapshot-race-branch --owner alice --ttl-min 120
    [[ "$RUN_EXIT" -eq 0 ]] || { fail "snapshot-race setup — acquire exit=$RUN_EXIT out=$RUN_OUT"; return; }
    local real_jq
    real_jq="$(command -v jq)"
    local shim_dir="$TMPDIR_REVIEW/shim-jq"
    mkdir -p "$shim_dir"
    export SNAPSHOT_RACE_TARGET="$REVIEW_LEASES_DIR/snapshot-race-branch.json"
    export SNAPSHOT_RACE_REAL_JQ="$real_jq"
    export SNAPSHOT_RACE_MARKER="$TMPDIR_REVIEW/shim-jq.swapped"
    rm -f "$SNAPSHOT_RACE_MARKER"
    cat >"$shim_dir/jq" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
real="${SNAPSHOT_RACE_REAL_JQ:?}"
target="${SNAPSHOT_RACE_TARGET:?}"
marker="${SNAPSHOT_RACE_MARKER:?}"
is_version_check=0
for a in "$@"; do
    [[ "$a" == *".v"* ]] && is_version_check=1
done
"$real" "$@"
status=$?
if [[ $is_version_check -eq 1 && ! -f "$marker" ]]; then
    : > "$marker"
    "$real" -n '{v:2, branch:"snapshot-race-branch", held_by:"mallory", lease_expires_at:2147483647, pr:42, base_sha:"abc"}' > "$target"
fi
exit $status
EOF
    chmod +x "$shim_dir/jq"
    local out exit_code
    local e=0
    out="$(PATH="$shim_dir:$PATH" REVIEW_LEASES_DIR="$REVIEW_LEASES_DIR" bash "$LEASE" verify --branch snapshot-race-branch 2>&1)" || e=$?
    exit_code=$e
    if [[ "$exit_code" -eq 0 ]] && echo "$out" | grep -q "alice"; then
        pass "verify acts on the validated snapshot despite a swap after validation (still alice, exit 0)"
    else
        fail "snapshot-race — expected exit 0 holding alice, got exit=$exit_code out=$out"
    fi
    rm -rf "$shim_dir" "$SNAPSHOT_RACE_MARKER"
    unset SNAPSHOT_RACE_TARGET SNAPSHOT_RACE_REAL_JQ SNAPSHOT_RACE_MARKER
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

# ── unsupported marker version (v2) causes zero mutation ───────────────────
# @risk: an unrecognized envelope version (e.g. v2) is silently dropped, which
#        previously let the script post duplicate replies and resolve threads
#        that already carried an authoritative v2 disposition (#15282).
# @return_path: any non-v1 disposition marker is counted as malformed and
#        forces the existing fail-closed path (exit 2).
# @side_effect: neither a review reply nor a thread-resolution mutation is permitted.
test_disposition_v2_marker_is_inert() {
    local v2_only
    v2_only=$'Future reply.\n\n<!-- disposition:v2 {"v":2,"class":"fixed","thread_id":"THREAD","by":"alice","head":"abc","evidence":{"commit":"abc"}} -->'
    run_disposition ok "$v2_only" abc
    local mutations
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 2 && -z "$mutations" && "$DISPOSITION_OUT" == *"malformed disposition marker"* ]]; then
        pass "v2-only marker exits 2 with no reply or resolution mutation"
    else
        fail "v2-only marker — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
    fi

    local v1_plus_v2="$existing_h1"$'\n\nFuture reply.\n\n<!-- disposition:v2 {"v":2,"class":"fixed","thread_id":"THREAD","by":"alice","head":"abc","evidence":{"commit":"abc"}} -->'
    run_disposition ok "$v1_plus_v2" abc
    mutations="$(paste -sd, "$FAKE_LOG")"
    if [[ "$DISPOSITION_EXIT" -eq 2 && -z "$mutations" && "$DISPOSITION_OUT" == *"malformed disposition marker"* ]]; then
        pass "mixed v1+v2 fixture exits 2 (the v2 envelope gates mutation)"
    else
        fail "mixed v1+v2 fixture — exit=$DISPOSITION_EXIT mutations=$mutations out=$DISPOSITION_OUT"
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
test_audit_v1_lease_reports_active
test_verify_v2_lease_refused
test_release_v2_lease_refused
test_audit_v2_lease_refused
test_acquire_v2_lease_refused_no_overwrite
test_verify_malformed_lease_refused
test_verify_missing_v_field_refused
test_verify_string_v_field_refused
test_verify_uses_snapshot_despite_swap_after_validation
test_disposition_reuses_h1_at_h2
test_disposition_posts_changed_evidence
test_disposition_provider_failure_is_inert
test_disposition_malformed_marker_is_inert
test_disposition_v2_marker_is_inert
echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then exit 1; fi
exit 0
