#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# fake cargo stubs answer with the workspace-required version and do not log it.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
GENERATE_BADGES_SCRIPT="${REPO_ROOT}/scripts/generate-badges.sh"

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

assert_exit_zero() {
  local label="$1"
  local code="$2"
  if [[ "$code" -eq 0 ]]; then
    pass "$label"
  else
    fail "$label (expected exit 0, got ${code})"
  fi
}

assert_exit_nonzero() {
  local label="$1"
  local code="$2"
  if [[ "$code" -ne 0 ]]; then
    pass "$label (exit ${code} as expected)"
  else
    fail "$label (expected non-zero exit, got 0)"
  fi
}

write_fake_cargo() {
  local fake_bin="$1"
  local log_path="$2"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "\${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
printf '%s\n' "\$@" > "${log_path}"
exit "\${FAKE_CARGO_EXIT:-0}"
FAKE
  chmod +x "${fake_bin}/cargo"
}

write_interpreter_sentinel() {
  local sentinel_bin="$1"
  local log_path="$2"
  local name

  mkdir -p "$sentinel_bin"
  # Cover the spellings a re-embedded owner proof could resolve through PATH.
  for name in python python3 py; do
    cat > "${sentinel_bin}/${name}" <<SENTINEL
#!/usr/bin/env bash
printf '%s %s\n' "${name}" "\$*" >> "${log_path}"
exit 97
SENTINEL
    chmod +x "${sentinel_bin}/${name}"
  done
}

assert_args_equal() {
  local label="$1"
  local expected="$2"
  local actual="$3"

  if cmp -s "$expected" "$actual"; then
    pass "$label"
  else
    fail "$label"
    printf 'expected:\n'
    cat "$expected"
    printf 'actual:\n'
    cat "$actual"
  fi
}

echo "=== generate-badges wrapper test suite ==="
echo ""

if [[ ! -f "$GENERATE_BADGES_SCRIPT" ]]; then
  echo "ERROR: generate-badges.sh not found at ${GENERATE_BADGES_SCRIPT}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/cargo-args.txt"
write_fake_cargo "$FAKE_BIN" "$FAKE_LOG"

# Routing-ownership sentinel (#14184), armed for the whole run: any interpreter
# this suite resolves through PATH is recorded instead of executed. See the
# guard assertion at the end of the file for what it proves.
SENTINEL_BIN="${TMPDIR_BASE}/sentinel-bin"
SENTINEL_LOG="${TMPDIR_BASE}/interpreter-invocations.txt"
write_interpreter_sentinel "$SENTINEL_BIN" "$SENTINEL_LOG"
PATH="${SENTINEL_BIN}:$PATH"
export PATH

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_PASS_ARGS="${PASS_DIR}/expected-args.txt"
cat > "$EXPECTED_PASS_ARGS" <<'ARGS'
xtask
ci-hygiene
generate-badges
--check
ARGS

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$GENERATE_BADGES_SCRIPT" --check
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_zero "delegates to cargo xtask ci-hygiene generate-badges" "$code"
assert_args_equal "forwards badge generator arguments unchanged" "$EXPECTED_PASS_ARGS" "$FAKE_LOG"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_EXIT=37 bash "$GENERATE_BADGES_SCRIPT" --check
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates cargo failure from delegated command" "$code"

# Routing-ownership guard (#14184). Direct proof of the Python badge generator
# belongs to the `ripr-badge-endpoints` pack
# (`scripts/tests/test-generate-badges.py`), which CI selects for generator
# edits. This wrapper pack is selected only by shell-wrapper edits, so proof
# embedded here would stop running exactly when the generator changes.
#
# The check is behavioral, not textual: the sentinel armed at the top of this
# run shadows `python`/`python3`/`py` on PATH, so any interpreter this suite
# actually launches is recorded no matter how its name is spelled in source
# (a runtime-assembled string defeats a grep, not a PATH lookup). A stub also
# exits 97, so re-embedded proof fails loudly rather than silently passing.
# Residual gap, stated rather than papered over: an invocation by absolute
# path (`/usr/bin/python3 ...`) bypasses PATH and would not be recorded.
if [[ -s "$SENTINEL_LOG" ]]; then
  fail "wrapper proof stays shell-scoped and never runs the Python owner"
  printf 'interpreter invocations recorded during this run:\n'
  cat "$SENTINEL_LOG"
else
  pass "wrapper proof stays shell-scoped and never runs the Python owner"
fi

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
