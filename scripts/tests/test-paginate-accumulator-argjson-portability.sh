#!/usr/bin/env bash
# Regression: scripts/ci/check-pr-review-convergence-core accumulates large
# paged GraphQL results in two places:
#
#   1. `paginate_pr_connection` (the `all_nodes=$(...)` accumulator) folds
#      page results into a growing JSON array.
#   2. The `UNREACHABLE_FIX_COMMITS` loop (around line 640) accumulates one
#      entry per disposition marker that fails the reachability check.
#
# Both were originally implemented as
#
#   jq -n --argjson a "$a" --argjson b "$b" '$a + $b'
#
# which passes both inputs through `argv` on every iteration. Once the
# accumulator grows past the platform's `argv` length cap, `execvp()` returns
# E2BIG and the page is silently dropped or the helper aborts. On Windows
# that cap is ~8-32 KB, so a PR with ~30+ review threads (each thread
# embeds `comments(first: 20) { ... }`) hits the limit at page ~3 and the
# verdict silently degrades to `state: NOT_PROVEN` /
# `classification: NOT_PROVEN / reason: github_facts_unavailable`.
#
# The fix pipes the accumulator and the new page through stdin via
# `jq -s 'add'`, so `argv` stays empty and the loop runs to completion on
# every platform.
#
# This test pins (a) the stdin-accumulator form returns the same canonical
# result as `$a + $b`, (b) both known accumulator sites use stdin rather
# than the original `jq -n --argjson` form, so a future contributor cannot
# silently re-introduce the bad pattern without breaking this gate.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE="$SCRIPT_DIR/../ci/check-pr-review-convergence-core"

PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

command -v jq >/dev/null 2>&1 || { echo "ERROR: jq required"; exit 1; }
[[ -f "$CORE" ]] || { echo "ERROR: $CORE missing"; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "ERROR: python3 required"; exit 1; }

# ---- Data pins ---------------------------------------------------------
SMALL_A=$(jq -nc '[range(0; 150) | {page: 0, id: (. | tostring), body: "thread body for node"}]')
SMALL_B=$(jq -nc '[range(0; 50)  | {page: 1, id: (. | tostring), body: "thread body for node"}]')
SMALL_MARKER='{"page":99,"id":"u","body":"unreachable"}'

canonical() { jq -S . <<<"$1"; }

EXPECTED=$(jq -nc --argjson a "$SMALL_A" --argjson b "$SMALL_B" '$a + $b')
NEW_FORM=$(printf '%s\n%s\n' "$SMALL_A" "$SMALL_B" | jq -s 'add')
if [[ "$(canonical "$NEW_FORM")" == "$(canonical "$EXPECTED")" ]]; then
    pass "stdin-accumulator form equals direct concatenation at 200 nodes"
else
    fail "stdin-accumulator form diverged from direct concat at 200 nodes"
fi

EXPECTED2=$(jq -nc --argjson a "$SMALL_A" --argjson m "$SMALL_MARKER" '$a + [$m]')
PIPE2=$(printf '%s\n[%s]\n' "$SMALL_A" "$SMALL_MARKER" | jq -s '.[0] + .[1]')
if [[ "$(canonical "$PIPE2")" == "$(canonical "$EXPECTED2")" ]]; then
    pass "stdin-accumulator wrapper form equals '\$a + [\$m]'"
else
    fail "stdin-accumulator wrapper form diverged"
fi

# ---- Static guards -----------------------------------------------------
# Run the python check on each site; capture stdout and split into per-line
# pass/fail accounting.

OUT1=$(mktemp)
OUT2=$(mktemp)
trap 'rm -f "$OUT1" "$OUT2"' EXIT

python3 - "$CORE" "all_nodes" "jq -s 'add'" "paginate_pr_connection accumulator" <<'PY' >"$OUT1"
import re
import sys

path, var, good_pattern, label = sys.argv[1:5]

with open(path, 'r', encoding='utf-8') as f:
    lines = f.readlines()

# Drop everything from " # ... end of line" onward. Bash comments can sit
# mid-line, so this is a conservative truncation.
def strip_comment(line):
    return re.sub(r'\s*#.*$', '', line.rstrip('\n'))

code_lines = [(n, strip_comment(raw)) for n, raw in enumerate(lines, 1)
              if strip_comment(raw).strip()]

assignment = re.compile(rf'\s*{re.escape(var)}\s*=')
bad = re.compile(r'jq\s+-n\s+--argjson')

found = False
for n, code in code_lines:
    if assignment.match(code) and 'jq' in code:
        if bad.search(code):
            print(f"FAIL {label}: line {n} still uses jq -n --argjson")
        elif good_pattern not in code:
            print(f"FAIL {label}: line {n} does not contain {good_pattern!r}")
        else:
            print(f"PASS {label}: line {n} uses stdin pipe ({good_pattern!r})")
        found = True
        break

if not found:
    print(f"FAIL {label}: could not locate {var}= jq assignment in {path}")
PY

python3 - "$CORE" "UNREACHABLE_FIX_COMMITS" "jq -s '.[0] + .[1]'" "unreachable-fix-commits accumulator" <<'PY' >"$OUT2"
import re
import sys

path, var, good_pattern, label = sys.argv[1:5]

with open(path, 'r', encoding='utf-8') as f:
    lines = f.readlines()

def strip_comment(line):
    return re.sub(r'\s*#.*$', '', line.rstrip('\n'))

code_lines = [(n, strip_comment(raw)) for n, raw in enumerate(lines, 1)
              if strip_comment(raw).strip()]

assignment = re.compile(rf'\s*{re.escape(var)}\s*=')
bad = re.compile(r'jq\s+-n\s+--argjson')

found = False
for n, code in code_lines:
    if assignment.match(code) and 'jq' in code:
        if bad.search(code):
            print(f"FAIL {label}: line {n} still uses jq -n --argjson")
        elif good_pattern not in code:
            print(f"FAIL {label}: line {n} does not contain {good_pattern!r}")
        else:
            print(f"PASS {label}: line {n} uses stdin pipe ({good_pattern!r})")
        found = True
        break

if not found:
    print(f"FAIL {label}: could not locate {var}= jq assignment in {path}")
PY

while read -r line; do
    if [[ "$line" == PASS* ]]; then
        pass "${line#PASS }"
    elif [[ "$line" == FAIL* ]]; then
        fail "${line#FAIL }"
    fi
done <"$OUT1"

while read -r line; do
    if [[ "$line" == PASS* ]]; then
        pass "${line#PASS }"
    elif [[ "$line" == FAIL* ]]; then
        fail "${line#FAIL }"
    fi
done <"$OUT2"

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="
[[ "$FAIL_COUNT" -eq 0 ]]
