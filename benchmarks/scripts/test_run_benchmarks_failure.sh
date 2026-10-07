#!/bin/bash
# Failure-surfacing tests for run-benchmarks.sh (#17218).
#
# A category whose cargo run fails must be marked explicitly failed in the
# JSON and must fail the process -- never a silent empty stub with exit 0.
# Failure records quote cargo's stderr, and the index bench is invoked with
# its required features (#17425).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$SCRIPT_DIR/run-benchmarks.sh"
PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "ok: $1"; }
bad() { FAIL=$((FAIL + 1)); echo "FAIL: $1"; }

# Stub cargo: fails for bench targets listed in $FAIL_BENCHES (space-separated
# substring match), else prints one parseable criterion line and succeeds.
make_stub() {
    local dir=$1
    cat > "$dir/cargo" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${CARGO_ARGS_LOG:-}" ]]; then
    printf '%s\n' "$*" >>"$CARGO_ARGS_LOG"
fi
for want in ${FAIL_BENCHES:-}; do
    for arg in "$@"; do
        if [[ "$arg" == *"$want"* ]]; then
            echo "error: stub cargo failing for $want" >&2
            exit 101
        fi
    done
done
echo "stub_bench  time:   [45.123 us 45.234 us 45.345 us]"
exit 0
EOF
    chmod +x "$dir/cargo"
}

json_get() {
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["results"][sys.argv[2]].get(sys.argv[3], ""))' "$1" "$2" "$3"
}

# Case 1: total cargo failure -> exit nonzero, every category marked failed.
STUB=$(mktemp -d)
make_stub "$STUB"
OUT=$(mktemp)
ERR=$(mktemp)
export FAIL_BENCHES="benchmark"
if PATH="$STUB:$PATH" bash "$RUNNER" --category ripr >"$OUT" 2>"$ERR"; then
    bad "all-fail run exited 0; expected nonzero"
else
    ok "all-fail run exits nonzero"
fi
if python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$OUT" 2>/dev/null; then
    ok "all-fail output is valid JSON"
else
    bad "all-fail output is not valid JSON"
fi
[[ "$(json_get "$OUT" ripr _status)" == "failed" ]] \
    && ok "failed category carries _status=failed" \
    || bad "failed category missing _status=failed"
json_get "$OUT" ripr _error | grep -q "ripr_facts_benchmark" \
    && ok "_error names the failed bench target" \
    || bad "_error does not name the failed bench target"
json_get "$OUT" ripr _error | grep -q "stub cargo failing" \
    && ok "_error quotes cargo stderr" \
    || bad "_error does not quote cargo stderr"
grep -q "benchmark categories failed: ripr" "$ERR" \
    && ok "stderr names the failed category" \
    || bad "stderr does not name the failed category"

# Case 1b: unfiltered all-fail run -> every category marked, process fails.
if PATH="$STUB:$PATH" bash "$RUNNER" >"$OUT" 2>"$ERR"; then
    bad "unfiltered all-fail run exited 0; expected nonzero"
else
    ok "unfiltered all-fail run exits nonzero"
fi
ALL_MARKED=1
for cat in parser lexer lsp index ripr; do
    if [[ "$(json_get "$OUT" "$cat" _status)" != "failed" ]]; then
        ALL_MARKED=0
    fi
done
[[ "$ALL_MARKED" -eq 1 ]] \
    && ok "all five categories marked failed in unfiltered run" \
    || bad "unfiltered run left a category unmarked"
grep -q "benchmark categories failed: parser lexer lsp index ripr" "$ERR" \
    && ok "stderr names all failed categories in order" \
    || bad "stderr does not name all failed categories"

# Case 2: selective failure -> partial data preserved, process still fails.
export FAIL_BENCHES="ripr_facts_benchmark"
if PATH="$STUB:$PATH" bash "$RUNNER" >"$OUT" 2>"$ERR"; then
    bad "partial-fail run exited 0; expected nonzero"
else
    ok "partial-fail run exits nonzero"
fi
[[ "$(json_get "$OUT" ripr _status)" == "failed" ]] \
    && ok "failed category marked in partial run" \
    || bad "failed category not marked in partial run"
[[ -n "$(json_get "$OUT" parser stub_bench)" ]] \
    && ok "successful category keeps its entries in partial run" \
    || bad "successful category lost its entries in partial run"

# Case 3: positive control -> unchanged green behavior, no failure markers.
export FAIL_BENCHES=""
if PATH="$STUB:$PATH" bash "$RUNNER" --category ripr >"$OUT" 2>"$ERR"; then
    ok "all-success run exits 0"
else
    bad "all-success run exited nonzero"
fi
[[ -z "$(json_get "$OUT" ripr _status)" ]] \
    && ok "successful category has no _status marker" \
    || bad "successful category carries a spurious _status marker"
[[ -n "$(json_get "$OUT" ripr stub_bench)" ]] \
    && ok "successful category entries unchanged" \
    || bad "successful category entries missing"

# Case 4: the index bench gets its required features; others unchanged (#17425).
ARGS_LOG=$(mktemp)
export CARGO_ARGS_LOG="$ARGS_LOG"
export FAIL_BENCHES=""
if PATH="$STUB:$PATH" bash "$RUNNER" --category index >"$OUT" 2>"$ERR"; then
    ok "index stub run exits 0"
else
    bad "index stub run exited nonzero"
fi
grep -q "workspace_index_benchmark.*--features workspace" "$ARGS_LOG" \
    && ok "index bench invoked with --features workspace" \
    || bad "index bench missing --features workspace"
: >"$ARGS_LOG"
if PATH="$STUB:$PATH" bash "$RUNNER" --category parser >"$OUT" 2>"$ERR"; then
    ok "parser stub run exits 0"
else
    bad "parser stub run exited nonzero"
fi
if grep "parser_benchmark" "$ARGS_LOG" | grep -q -- "--features"; then
    bad "non-index bench gained --features"
else
    ok "non-index bench keeps default features"
fi
unset CARGO_ARGS_LOG

rm -rf "$STUB" "$OUT" "$ERR" "$ARGS_LOG"
echo "---"
echo "pass=$PASS fail=$FAIL"
[[ "$FAIL" -eq 0 ]]
