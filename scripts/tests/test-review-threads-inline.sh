#!/usr/bin/env bash
# Offline self-test for scripts/reviews/threads and scripts/reviews/inline (#6178).
#
# Both scripts are thin, agent-facing wrappers over GitHub. The parts that can
# silently go wrong are NOT the happy path — they are:
#
#   1. Pagination. A PR in this repo routinely carries more review threads than
#      one GraphQL page. A wrapper that reads only the first page emits a
#      truncated thread list, and the agent dispositions a subset while the PR
#      stays BLOCKED on the threads it never saw. This suite serves a TWO-PAGE
#      connection through a stub `gh` and asserts every page's threads survive.
#   2. Silent finding loss. GitHub 422s an inline comment whose (path, line)
#      is not an addressable line in the PR diff, and its error body does not
#      say WHICH comment was bad. A wrapper that just forwards the 422 leaves
#      the agent with a finding it believes it published and nobody can read.
#      This suite asserts the offending finding is named and that NOTHING is
#      posted when any finding is unaddressable.
#   3. --dry-run that is not dry. Asserted by recording every stub invocation
#      and requiring zero review POSTs.
#
# Everything runs offline against a stub `gh` prepended to PATH; no network, no
# real PR touched.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
THREADS="$SCRIPT_DIR/../reviews/threads"
INLINE="$SCRIPT_DIR/../reviews/inline"
DISPOSITION="$SCRIPT_DIR/../reviews/disposition"
PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

for required in "$THREADS" "$INLINE" "$DISPOSITION"; do
    [[ -f "$required" ]] || { echo "ERROR: missing $required"; exit 1; }
done
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq required"; exit 1; }

TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT
STUB_BIN="$TMP_ROOT/bin"
STUB_DIR="$TMP_ROOT/data"
mkdir -p "$STUB_BIN" "$STUB_DIR"

# ── stub gh ─────────────────────────────────────────────────────────────────
# Serves page 1 of reviewThreads when the query carries no cursor and page 2
# when it carries `after:`, so the pagination loop is genuinely exercised
# rather than stubbed away. Records every invocation to GH_STUB_LOG.
cat > "$STUB_BIN/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_STUB_LOG"

if [[ "${1:-}" == "api" && "${2:-}" == "graphql" ]]; then
    # The cursor must match the one page 1 handed out. A stub that serves page 2
    # for any query containing `after:` would let a paginator that forwards a
    # stale or hard-coded cursor pass test_threads_paginates.
    if printf '%s' "$*" | grep -qF 'after: "CURSOR-PAGE-1"'; then
        cat "$GH_STUB_DIR/threads_page2.json"
    elif printf '%s' "$*" | grep -q 'after:'; then
        printf '%s\n' '{"errors":[{"message":"gh stub: pagination requested with an unexpected cursor"}]}' >&2
        exit 1
    else
        cat "$GH_STUB_DIR/threads_page1.json"
    fi
    exit 0
fi

if [[ "${1:-}" == "pr" && "${2:-}" == "view" ]]; then
    printf '%s\n' "feedfacedeadbeeffeedfacedeadbeeffeedface"
    exit 0
fi

if [[ "${1:-}" == "repo" && "${2:-}" == "view" ]]; then
    printf '%s\n' "test-owner/test-repo"
    exit 0
fi

case "$*" in
    *"/files"*)
        cat "$GH_STUB_DIR/pr_files.json"
        exit 0
        ;;
    *"/reviews"*)
        cat > "$GH_STUB_DIR/posted_payload.json"
        if [[ -n "${GH_STUB_REVIEWS_FAIL:-}" ]]; then
            printf '%s\n' '{"message":"Validation Failed","errors":[{"resource":"PullRequestReviewComment","field":"pull_request_review_thread.line","code":"invalid"}]}' >&2
            exit 1
        fi
        printf '%s\n' '{"id":12345,"state":"COMMENTED"}'
        exit 0
        ;;
esac

printf 'gh stub: unhandled invocation: %s\n' "$*" >&2
exit 1
STUB
chmod +x "$STUB_BIN/gh"

GH_STUB_LOG="$TMP_ROOT/gh-calls.log"
export GH_STUB_LOG GH_STUB_DIR="$STUB_DIR"

reset_stub() { : > "$GH_STUB_LOG"; rm -f "$STUB_DIR/posted_payload.json"; unset GH_STUB_REVIEWS_FAIL; }

# ── fixtures: a two-page reviewThreads connection ───────────────────────────
thread_node() {
    # $1 id, $2 isResolved, $3 isOutdated, $4 path, $5 line (or "null"),
    # $6 originalLine, $7 author, $8 body
    jq -n --arg id "$1" --argjson resolved "$2" --argjson outdated "$3" \
        --arg path "$4" --argjson line "$5" --argjson orig "$6" \
        --arg author "$7" --arg body "$8" '{
            id: $id, isResolved: $resolved, isOutdated: $outdated,
            path: $path, line: $line, originalLine: $orig,
            startLine: null, originalStartLine: null, diffSide: "RIGHT",
            comments: { nodes: [ { author: { login: $author }, body: $body,
                                   url: ("https://example.invalid/" + $id) } ] }
        }'
}

jq -n \
    --argjson a "$(thread_node PRRT_page1_active false false 'crates/a/src/lib.rs' 3 3 coderabbitai 'Consider bounding this loop.
It can run forever on malformed input.')" \
    --argjson b "$(thread_node PRRT_page1_resolved true false 'crates/b/src/main.rs' 11 11 alice 'Nit: rename this.')" \
    '{data:{repository:{pullRequest:{reviewThreads:{
        nodes: [$a, $b],
        pageInfo: {hasNextPage: true, endCursor: "CURSOR-PAGE-1"}}}}}}' \
    > "$STUB_DIR/threads_page1.json"

jq -n \
    --argjson c "$(thread_node PRRT_page2_outdated false true 'crates/c/src/x.rs' null 44 gemini 'This hunk moved.')" \
    '{data:{repository:{pullRequest:{reviewThreads:{
        nodes: [$c],
        pageInfo: {hasNextPage: false, endCursor: null}}}}}}' \
    > "$STUB_DIR/threads_page2.json"

# ── fixtures: PR files with real unified-diff patches ───────────────────────
# crates/a/src/lib.rs addressable RIGHT lines: 1,2,3,4,5
# crates/b/src/main.rs addressable RIGHT lines: 10,11,12,13
PATCH_A=' fn a() {
-    let x = 1;
+    let x = 2;
+    let y = 3;
     println!("{}", x);
 }'
PATCH_B=' fn main() {
+    init();
     run();
 }'
jq -n --arg pa "@@ -1,4 +1,6 @@
$PATCH_A" --arg pb "@@ -10,3 +10,4 @@
$PATCH_B" '[
    {filename: "crates/a/src/lib.rs", status: "modified", patch: $pa},
    {filename: "crates/b/src/main.rs", status: "modified", patch: $pb}
]' > "$STUB_DIR/pr_files.json"

run_threads() {
    RUN_EXIT=0
    RUN_OUT="$(PATH="$STUB_BIN:$PATH" bash "$THREADS" "$@" 2>&1)" || RUN_EXIT=$?
}
run_inline() {
    RUN_EXIT=0
    RUN_OUT="$(PATH="$STUB_BIN:$PATH" bash "$INLINE" "$@" 2>&1)" || RUN_EXIT=$?
}
graphql_calls() { grep -c 'api graphql' "$GH_STUB_LOG" || true; }
review_posts()  { grep -c '/reviews' "$GH_STUB_LOG" || true; }

# ═══ threads ════════════════════════════════════════════════════════════════

# The load-bearing case: a thread that only exists on page 2 must appear.
test_threads_paginates() {
    reset_stub
    run_threads 9999 test-owner/test-repo --json
    local calls; calls="$(graphql_calls)"
    if [[ "$RUN_EXIT" -eq 0 ]] \
        && jq -e '(.threads | length) == 3
                  and ([.threads[].id] | index("PRRT_page1_active")) != null
                  and ([.threads[].id] | index("PRRT_page2_outdated")) != null' \
             >/dev/null <<<"$RUN_OUT" \
        && [[ "$calls" -eq 2 ]]; then
        pass "threads: pages through a two-page connection (2 graphql calls, no thread dropped)"
    else
        fail "threads pagination — exit=$RUN_EXIT graphql_calls=$calls out=$RUN_OUT"
    fi
}

test_threads_unresolved_only() {
    reset_stub
    run_threads 9999 test-owner/test-repo --unresolved-only --json
    if [[ "$RUN_EXIT" -eq 0 ]] \
        && jq -e '(.threads | length) == 2
                  and ([.threads[].isResolved] | all(. == false))
                  and ([.threads[].id] | index("PRRT_page1_resolved")) == null
                  and .unresolved_count == 2 and .thread_count == 2' \
             >/dev/null <<<"$RUN_OUT"; then
        pass "threads: --unresolved-only drops resolved threads (including the page-2 outdated one)"
    else
        fail "threads --unresolved-only — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# The whole point of the script: the emitted id must be the thing disposition
# takes, and the outdated thread must still carry a usable line (originalLine).
test_threads_json_shape() {
    reset_stub
    run_threads 9999 test-owner/test-repo --json
    if jq -e '(.threads[] | has("id") and has("isResolved") and has("isOutdated")
                            and has("path") and has("line") and has("author")
                            and has("excerpt"))
              and (.threads[] | select(.id == "PRRT_page2_outdated")
                   | .isOutdated == true and .line == 44 and .line_is_original == true)
              and (.threads[] | select(.id == "PRRT_page1_active")
                   | .author == "coderabbitai"
                   and .excerpt == "Consider bounding this loop.")' \
             >/dev/null <<<"$RUN_OUT"; then
        pass "threads: --json carries id/state/path/line/author/excerpt; outdated thread falls back to originalLine"
    else
        fail "threads json shape — out=$RUN_OUT"
    fi
}

# An id from `threads` must be accepted by `disposition --thread` unchanged.
# --dry-run keeps this offline; it proves the handoff contract, not the network.
test_threads_id_feeds_disposition() {
    reset_stub
    run_threads 9999 test-owner/test-repo --unresolved-only --json
    local tid; tid="$(jq -r '.threads[0].id' <<<"$RUN_OUT")"
    local d_exit=0 d_out
    d_out="$(bash "$DISPOSITION" --pr 9999 --thread "$tid" --class fixed \
        --commit abc1234 --reply 'Disposition: fixed' --dry-run 2>&1)" || d_exit=$?
    if [[ "$d_exit" -eq 0 && -n "$tid" ]] && grep -q "$tid" <<<"$d_out"; then
        pass "threads → disposition: an emitted thread id is accepted by disposition --thread ($tid)"
    else
        fail "threads→disposition handoff — tid=$tid exit=$d_exit out=$d_out"
    fi
}

test_threads_human_output_is_actionable() {
    reset_stub
    run_threads 9999 test-owner/test-repo
    if [[ "$RUN_EXIT" -eq 0 ]] \
        && grep -q 'PRRT_page2_outdated' <<<"$RUN_OUT" \
        && grep -q 'scripts/reviews/disposition' <<<"$RUN_OUT"; then
        pass "threads: default output lists every thread id and names the sanctioned resolve path"
    else
        fail "threads human output — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

test_threads_usage_error() {
    reset_stub
    run_threads
    local a=$RUN_EXIT
    run_threads 9999 test-owner/test-repo --bogus-flag
    if [[ "$a" -eq 2 && "$RUN_EXIT" -eq 2 ]]; then
        pass "threads: missing PR and unknown flag both exit 2"
    else
        fail "threads usage — no-arg exit=$a bad-flag exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# `threads` is read-only. Nothing it does may mutate the PR.
test_threads_is_read_only() {
    reset_stub
    run_threads 9999 test-owner/test-repo --json >/dev/null
    if ! grep -qE 'method (POST|PATCH|PUT|DELETE)|mutation' "$GH_STUB_LOG"; then
        pass "threads: makes no mutating call (no POST/PATCH/PUT/DELETE, no mutation)"
    else
        fail "threads read-only — log=$(cat "$GH_STUB_LOG")"
    fi
}

# A truncated list that exits 0 is worse than an error: the agent dispositions
# what it sees and the PR stays BLOCKED on the rest. "More pages exist" with no
# cursor to fetch them must be a hard failure, not a quiet stop.
test_threads_refuses_truncation() {
    reset_stub
    jq -n '{data:{repository:{pullRequest:{reviewThreads:{
        nodes: [{id: "PRRT_only", isResolved: false, isOutdated: false,
                 path: "a.rs", line: 1, originalLine: 1, startLine: null,
                 originalStartLine: null, diffSide: "RIGHT",
                 comments: {nodes: [{author: {login: "bot"}, body: "x", url: null}]}}],
        pageInfo: {hasNextPage: true, endCursor: null}}}}}}' \
        > "$STUB_DIR/threads_page1.json"

    run_threads 9999 test-owner/test-repo --json
    local ok=0
    if [[ "$RUN_EXIT" -eq 2 ]] && grep -qi 'truncated' <<<"$RUN_OUT"; then ok=1; fi

    jq -n \
        --argjson a "$(thread_node PRRT_page1_active false false 'crates/a/src/lib.rs' 3 3 coderabbitai 'Consider bounding this loop.')" \
        --argjson b "$(thread_node PRRT_page1_resolved true false 'crates/b/src/main.rs' 11 11 alice 'Nit: rename this.')" \
        '{data:{repository:{pullRequest:{reviewThreads:{
            nodes: [$a, $b],
            pageInfo: {hasNextPage: true, endCursor: "CURSOR-PAGE-1"}}}}}}' \
        > "$STUB_DIR/threads_page1.json"

    if [[ "$ok" -eq 1 ]]; then
        pass "threads: hasNextPage with a null cursor exits 2 rather than reporting a truncated list"
    else
        fail "threads truncation guard — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# Regression: bot review bodies are long, and an accumulator passed to jq
# through argv (`jq --argjson acc "$SO_FAR"`) dies with "Argument list too
# long" — observed live on a real 12-thread PR before the fix, and it would hit
# hardest on exactly the large PRs this script exists for. Serve one page whose
# payload comfortably exceeds any argv limit and require every thread to
# survive.
test_threads_survives_large_payload() {
    reset_stub
    # The fixture itself must never travel through argv — doing so would
    # reproduce the very failure this case guards against instead of testing
    # for it. Build it into a file and slurp it.
    jq -n '[range(0; 40) as $i | {
        id: ("PRRT_big_" + ($i | tostring)),
        isResolved: false, isOutdated: false,
        path: ("crates/big/src/f" + ($i | tostring) + ".rs"), line: 1, originalLine: 1,
        startLine: null, originalStartLine: null, diffSide: "RIGHT",
        comments: { nodes: [ { author: { login: "coderabbitai" },
                               body: ("Finding " + ($i | tostring) + ".\n" + ("x" * 60000)),
                               url: ("https://example.invalid/" + ($i | tostring)) } ] }
    }]' > "$STUB_DIR/big_nodes.json"
    jq -n --slurpfile n "$STUB_DIR/big_nodes.json" '{data:{repository:{pullRequest:{reviewThreads:{
        nodes: $n[0], pageInfo: {hasNextPage: false, endCursor: null}}}}}}' \
        > "$STUB_DIR/threads_page1.json"

    run_threads 9999 test-owner/test-repo --json
    local ok=0
    if [[ "$RUN_EXIT" -eq 0 ]] \
        && jq -e '(.threads | length) == 40
                  and (.threads[0].excerpt == "Finding 0.")' >/dev/null <<<"$RUN_OUT"; then
        ok=1
    fi

    # Restore the two-page fixtures for any later case.
    jq -n \
        --argjson a "$(thread_node PRRT_page1_active false false 'crates/a/src/lib.rs' 3 3 coderabbitai 'Consider bounding this loop.')" \
        --argjson b "$(thread_node PRRT_page1_resolved true false 'crates/b/src/main.rs' 11 11 alice 'Nit: rename this.')" \
        '{data:{repository:{pullRequest:{reviewThreads:{
            nodes: [$a, $b],
            pageInfo: {hasNextPage: true, endCursor: "CURSOR-PAGE-1"}}}}}}' \
        > "$STUB_DIR/threads_page1.json"

    if [[ "$ok" -eq 1 ]]; then
        pass "threads: a multi-megabyte thread payload survives (no argv-limit truncation)"
    else
        fail "threads large payload — exit=$RUN_EXIT out=$(head -c 400 <<<"$RUN_OUT")"
    fi
}

# ═══ inline ═════════════════════════════════════════════════════════════════

findings_file() { printf '%s' "$1" > "$TMP_ROOT/findings.json"; printf '%s' "$TMP_ROOT/findings.json"; }

VALID_FINDINGS='[
  {"path":"crates/a/src/lib.rs","line":3,"body":"`y` is never read."},
  {"path":"crates/b/src/main.rs","line":11,"body":"`init()` can fail; the result is dropped."}
]'

test_inline_posts_one_review() {
    reset_stub
    local f; f="$(findings_file "$VALID_FINDINGS")"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'Two findings.'
    local posts; posts="$(review_posts)"
    if [[ "$RUN_EXIT" -eq 0 && "$posts" -eq 1 ]] \
        && jq -e '(.comments | length) == 2 and .event == "COMMENT"
                  and (.body | length) > 0
                  and (.comments[0].path == "crates/a/src/lib.rs")
                  and (.comments[0].line == 3) and (.comments[0].side == "RIGHT")' \
             >/dev/null "$STUB_DIR/posted_payload.json"; then
        pass "inline: two findings become ONE submitted review carrying two inline comments"
    else
        fail "inline single review — exit=$RUN_EXIT posts=$posts out=$RUN_OUT payload=$(cat "$STUB_DIR/posted_payload.json" 2>/dev/null)"
    fi
}

test_inline_reads_stdin() {
    reset_stub
    RUN_EXIT=0
    RUN_OUT="$(printf '%s' "$VALID_FINDINGS" | PATH="$STUB_BIN:$PATH" bash "$INLINE" \
        --pr 9999 --repo test-owner/test-repo --body 'stdin path' 2>&1)" || RUN_EXIT=$?
    if [[ "$RUN_EXIT" -eq 0 ]] && jq -e '(.comments | length) == 2' >/dev/null "$STUB_DIR/posted_payload.json"; then
        pass "inline: findings may arrive on stdin instead of --findings"
    else
        fail "inline stdin — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# The core rejection path: a line outside the diff must be NAMED, and because
# a review is atomic, nothing at all may be posted.
test_inline_rejects_unaddressable_line() {
    reset_stub
    local f; f="$(findings_file '[
      {"path":"crates/a/src/lib.rs","line":3,"body":"fine"},
      {"path":"crates/a/src/lib.rs","line":99,"body":"line is outside the diff"}
    ]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    local posts; posts="$(review_posts)"
    if [[ "$RUN_EXIT" -eq 2 && "$posts" -eq 0 ]] \
        && grep -q 'findings\[1\]' <<<"$RUN_OUT" \
        && grep -q 'crates/a/src/lib.rs:99' <<<"$RUN_OUT"; then
        pass "inline: an unaddressable line is reported by index and location, and NO review is posted"
    else
        fail "inline unaddressable line — exit=$RUN_EXIT posts=$posts out=$RUN_OUT"
    fi
}

test_inline_rejects_unknown_path() {
    reset_stub
    local f; f="$(findings_file '[{"path":"crates/z/not-touched.rs","line":1,"body":"nope"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    local posts; posts="$(review_posts)"
    if [[ "$RUN_EXIT" -eq 2 && "$posts" -eq 0 ]] \
        && grep -q 'crates/z/not-touched.rs' <<<"$RUN_OUT" \
        && grep -qi 'not in the pull request diff' <<<"$RUN_OUT"; then
        pass "inline: a path absent from the PR diff is reported distinctly, and NO review is posted"
    else
        fail "inline unknown path — exit=$RUN_EXIT posts=$posts out=$RUN_OUT"
    fi
}

test_inline_rejects_malformed_finding() {
    reset_stub
    local f; f="$(findings_file '[{"line":3,"body":"no path"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    local a=$RUN_EXIT
    local f2; f2="$(findings_file '{"path":"crates/a/src/lib.rs","line":3,"body":"not an array"}')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f2" --body 'x'
    if [[ "$a" -eq 2 && "$RUN_EXIT" -eq 2 && "$(review_posts)" -eq 0 ]]; then
        pass "inline: a finding missing \`path\` and a non-array payload both exit 2 with nothing posted"
    else
        fail "inline malformed — missing-path exit=$a non-array exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# A numeric `path` must fail here with a useful message, not pass a coercing
# check and come back as an anonymous GitHub rejection.
test_inline_rejects_wrong_types() {
    reset_stub
    local f; f="$(findings_file '[{"path":123,"line":3,"body":"numeric path"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    local a=$RUN_EXIT a_out="$RUN_OUT"
    local f2; f2="$(findings_file '[{"path":"crates/a/src/lib.rs","line":"3","body":"string line"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f2" --body 'x'
    if [[ "$a" -eq 2 && "$RUN_EXIT" -eq 2 && "$(review_posts)" -eq 0 ]] \
        && grep -q 'must be a string' <<<"$a_out" \
        && grep -q 'must be a number' <<<"$RUN_OUT"; then
        pass "inline: a numeric \`path\` and a string \`line\` are rejected by type, not coerced"
    else
        fail "inline type checks — path exit=$a out=$a_out / line exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# A file present in the diff but with no `patch` (binary, or a patch GitHub
# truncated) must be reported differently from a path that is simply absent.
test_inline_distinguishes_missing_patch() {
    reset_stub
    cp "$STUB_DIR/pr_files.json" "$STUB_DIR/pr_files.bak"
    jq '. + [{filename: "assets/logo.png", status: "modified"}]' \
        "$STUB_DIR/pr_files.bak" > "$STUB_DIR/pr_files.json"
    local f; f="$(findings_file '[{"path":"assets/logo.png","line":1,"body":"binary"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    local ok=0
    if [[ "$RUN_EXIT" -eq 2 && "$(review_posts)" -eq 0 ]] \
        && grep -q 'IS in the pull request diff' <<<"$RUN_OUT" \
        && grep -q 'no patch' <<<"$RUN_OUT"; then
        ok=1
    fi
    mv "$STUB_DIR/pr_files.bak" "$STUB_DIR/pr_files.json"
    if [[ "$ok" -eq 1 ]]; then
        pass "inline: a diffed file with no patch is reported distinctly from an absent path"
    else
        fail "inline missing-patch — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

test_inline_multiline_range() {
    reset_stub
    local f; f="$(findings_file '[{"path":"crates/a/src/lib.rs","line":3,"start_line":2,"body":"both new lines"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'range'
    if [[ "$RUN_EXIT" -eq 0 ]] \
        && jq -e '.comments[0].start_line == 2 and .comments[0].line == 3' >/dev/null "$STUB_DIR/posted_payload.json"; then
        pass "inline: start_line/line multi-line range is forwarded when both ends are addressable"
    else
        fail "inline range — exit=$RUN_EXIT out=$RUN_OUT payload=$(cat "$STUB_DIR/posted_payload.json" 2>/dev/null)"
    fi
}

test_inline_rejects_unaddressable_start_line() {
    reset_stub
    local f; f="$(findings_file '[{"path":"crates/a/src/lib.rs","line":3,"start_line":90,"body":"bad range"}]')"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x'
    if [[ "$RUN_EXIT" -eq 2 && "$(review_posts)" -eq 0 ]] && grep -q 'start_line' <<<"$RUN_OUT"; then
        pass "inline: an unaddressable start_line is rejected by name"
    else
        fail "inline bad start_line — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

test_inline_dry_run_posts_nothing() {
    reset_stub
    local f; f="$(findings_file "$VALID_FINDINGS")"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'planned' --dry-run
    local posts; posts="$(review_posts)"
    if [[ "$RUN_EXIT" -eq 0 && "$posts" -eq 0 ]] \
        && grep -q 'DRY-RUN' <<<"$RUN_OUT" \
        && jq -e '(.comments | length) == 2' >/dev/null <<<"$(sed -n '/^{/,$p' <<<"$RUN_OUT")"; then
        pass "inline: --dry-run prints the exact request payload and posts nothing"
    else
        fail "inline dry-run — exit=$RUN_EXIT posts=$posts out=$RUN_OUT"
    fi
}

# Local validation cannot cover every GitHub rejection. When GitHub 422s
# anyway, the error body must reach the operator instead of being swallowed.
test_inline_surfaces_github_error() {
    reset_stub
    local f; f="$(findings_file "$VALID_FINDINGS")"
    RUN_EXIT=0
    RUN_OUT="$(GH_STUB_REVIEWS_FAIL=1 PATH="$STUB_BIN:$PATH" bash "$INLINE" \
        --pr 9999 --repo test-owner/test-repo --findings "$f" --body 'x' 2>&1)" || RUN_EXIT=$?
    if [[ "$RUN_EXIT" -eq 2 ]] && grep -q 'Validation Failed' <<<"$RUN_OUT"; then
        pass "inline: a GitHub rejection surfaces the API error body and exits 2"
    else
        fail "inline github error — exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

test_inline_requires_body() {
    reset_stub
    local f; f="$(findings_file "$VALID_FINDINGS")"
    run_inline --pr 9999 --repo test-owner/test-repo --findings "$f"
    local a=$RUN_EXIT
    run_inline --repo test-owner/test-repo --findings "$f" --body 'x'
    if [[ "$a" -eq 2 && "$RUN_EXIT" -eq 2 && "$(review_posts)" -eq 0 ]]; then
        pass "inline: missing --body and missing --pr both exit 2"
    else
        fail "inline usage — no-body exit=$a no-pr exit=$RUN_EXIT out=$RUN_OUT"
    fi
}

# ═══ boundary: resolution stays in disposition ══════════════════════════════
# Neither new script may carry the raw resolve mutation; scripts/reviews/
# disposition is the only sanctioned resolve path (#3693 R1 FILE 3).
test_no_raw_resolve_token() {
    local token
    token="resolve""ReviewThread"
    if ! grep -q "$token" "$THREADS" && ! grep -q "$token" "$INLINE"; then
        pass "boundary: neither threads nor inline contains the raw thread-resolve mutation"
    else
        fail "boundary — a new script contains the raw resolve mutation"
    fi
}

echo "=== review threads/inline test suite ==="
echo ""
test_threads_paginates
test_threads_unresolved_only
test_threads_json_shape
test_threads_id_feeds_disposition
test_threads_human_output_is_actionable
test_threads_usage_error
test_threads_is_read_only
test_threads_refuses_truncation
test_threads_survives_large_payload
test_inline_posts_one_review
test_inline_reads_stdin
test_inline_rejects_unaddressable_line
test_inline_rejects_unknown_path
test_inline_rejects_malformed_finding
test_inline_rejects_wrong_types
test_inline_distinguishes_missing_patch
test_inline_multiline_range
test_inline_rejects_unaddressable_start_line
test_inline_dry_run_posts_nothing
test_inline_surfaces_github_error
test_inline_requires_body
test_no_raw_resolve_token
echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then exit 1; fi
exit 0
