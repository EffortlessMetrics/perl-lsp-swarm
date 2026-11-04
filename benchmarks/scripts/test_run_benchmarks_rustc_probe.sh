#!/usr/bin/env bash
# Regression for an unguarded rustc --version probe (#18427).
#
# Under set -euo pipefail, a failing rustc probe must not abort before
# json_output: stdout stays parseable JSON, rust_version falls back to
# "unknown", and cargo still runs. A succeeding stub must still record the
# probed version so the fallback cannot be a constant.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$SCRIPT_DIR/run-benchmarks.sh"
FORMATTER="$SCRIPT_DIR/format-results.py"
PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "ok: $1"; }
bad() { FAIL=$((FAIL + 1)); echo "FAIL: $1"; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

STUB="$TMP/bin"
mkdir -p "$STUB"
OUT="$TMP/out.json"
ERR="$TMP/err.txt"
CARGO_ARGS_LOG="$TMP/cargo-args.log"
export CARGO_ARGS_LOG

write_rustc() {
    cat > "$STUB/rustc" <<EOF
#!/usr/bin/env bash
$1
EOF
    chmod +x "$STUB/rustc"
}

write_cargo() {
    cat > "$STUB/cargo" <<EOF
#!/usr/bin/env bash
if [[ -n "\${CARGO_ARGS_LOG:-}" ]]; then
    printf '%s\\n' "\$*" >>"\$CARGO_ARGS_LOG"
fi
$1
EOF
    chmod +x "$STUB/cargo"
}

run_runner() {
    : >"$CARGO_ARGS_LOG"
    set +e
    PATH="$STUB:$PATH" bash "$RUNNER" --category ripr >"$OUT" 2>"$ERR"
    status=$?
    set -e
}

json_field() {
    python3 -c 'import json,sys
doc=json.load(open(sys.argv[1], encoding="utf-8"))
cur=doc
for key in sys.argv[2:]:
    if not isinstance(cur, dict):
        print("")
        raise SystemExit(0)
    cur=cur.get(key, "")
print(cur if cur is not None else "")' "$@"
}

write_cargo 'echo "stub_bench  time:   [45.123 us 45.234 us 45.345 us]"; exit 0'

# Case 1: rustc --version fails. Current main aborts with zero-byte stdout
# before json_output; the guarded probe must keep going.
write_rustc 'echo "error: rustc probe failed" >&2; exit 1'
run_runner
if [[ "$status" -eq 0 ]]; then
    ok "failing-probe run exits 0 when cargo succeeds"
else
    bad "failing-probe run exited $status; expected 0 once the probe is guarded"
fi
if [[ -s "$OUT" ]] && python3 -c 'import json,sys; json.load(open(sys.argv[1], encoding="utf-8"))' "$OUT" 2>/dev/null; then
    ok "failing-probe stdout is parseable JSON"
else
    bad "failing-probe stdout is empty or not JSON ($(wc -c <"$OUT") bytes)"
fi
if [[ "$(json_field "$OUT" environment rust_version)" == "unknown" ]]; then
    ok "failing probe records rust_version=unknown"
else
    bad "failing probe recorded rust_version=$(json_field "$OUT" environment rust_version); expected unknown"
fi
if grep -q "ripr_facts_benchmark" "$CARGO_ARGS_LOG"; then
    ok "failing probe still invoked cargo for the selected category"
else
    bad "failing probe aborted before cargo ran"
fi
if python3 "$FORMATTER" "$OUT" --markdown 2>/dev/null | grep -Fq "**Rust Version:** unknown"; then
    ok "format-results.py markdown treats unknown as display text"
else
    bad "format-results.py markdown did not render rust_version unknown"
fi

# Case 2: realistic broken toolchain — rustc and cargo both fail. The receipt
# must still exist with an explicit category failure (#17218), not a zero-byte
# abort.
write_rustc 'echo "error: rustc probe failed" >&2; exit 1'
write_cargo 'echo "error: stub cargo failing" >&2; exit 101'
run_runner
if [[ "$status" -ne 0 ]]; then
    ok "failing-probe+cargo run exits nonzero"
else
    bad "failing-probe+cargo run exited 0; expected nonzero"
fi
if [[ -s "$OUT" ]] && python3 -c 'import json,sys; json.load(open(sys.argv[1], encoding="utf-8"))' "$OUT" 2>/dev/null; then
    ok "failing-probe+cargo stdout is parseable JSON"
else
    bad "failing-probe+cargo stdout is empty or not JSON ($(wc -c <"$OUT") bytes)"
fi
if [[ "$(json_field "$OUT" results ripr _status)" == "failed" ]]; then
    ok "failing-probe+cargo marks the selected category failed"
else
    bad "failing-probe+cargo recorded _status=$(json_field "$OUT" results ripr _status); expected failed"
fi
if [[ "$(json_field "$OUT" environment rust_version)" == "unknown" ]]; then
    ok "failing-probe+cargo records rust_version=unknown"
else
    bad "failing-probe+cargo recorded rust_version=$(json_field "$OUT" environment rust_version); expected unknown"
fi
if grep -q "ripr_facts_benchmark" "$CARGO_ARGS_LOG"; then
    ok "failing-probe+cargo still invoked cargo"
else
    bad "failing-probe+cargo aborted before cargo ran"
fi

# Case 3: opposite-direction control. A working probe must still surface the
# stub version, so the fallback cannot be hard-coded.
write_cargo 'echo "stub_bench  time:   [45.123 us 45.234 us 45.345 us]"; exit 0'
write_rustc 'echo "rustc 9.9.9-test (hash 2026-10-31)"; exit 0'
run_runner
if [[ "$status" -eq 0 ]]; then
    ok "succeeding-probe run exits 0"
else
    bad "succeeding-probe run exited $status; expected 0"
fi
if [[ "$(json_field "$OUT" environment rust_version)" == "9.9.9-test" ]]; then
    ok "succeeding probe records the stub rust_version"
else
    bad "succeeding probe recorded rust_version=$(json_field "$OUT" environment rust_version); expected 9.9.9-test"
fi

echo
echo "pass=$PASS fail=$FAIL"
if [[ "$FAIL" -ne 0 ]]; then
    exit 1
fi
