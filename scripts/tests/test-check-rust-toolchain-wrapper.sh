#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
WRAPPER="${REPO_ROOT}/scripts/check-rust-toolchain.sh"

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

write_fake_rustup() {
  local fake_bin="$1"
  local log_path="$2"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/rustup" <<FAKE
#!/usr/bin/env bash
printf '%s\n' "\$@" > "${log_path}"
exit "\${FAKE_RUSTUP_EXIT:-0}"
FAKE
  chmod +x "${fake_bin}/rustup"
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

echo "=== check-rust-toolchain wrapper test suite ==="
echo ""

if [[ ! -f "$WRAPPER" ]]; then
  echo "ERROR: check-rust-toolchain.sh not found at ${WRAPPER}"
  exit 1
fi

TOOLCHAIN="$(awk -F'"' '/channel/{print $2; exit}' "${REPO_ROOT}/rust-toolchain.toml")"
if [[ -z "${TOOLCHAIN:-}" ]]; then
  echo "ERROR: rust-toolchain.toml channel not found"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/rustup-args.txt"
write_fake_rustup "$FAKE_BIN" "$FAKE_LOG"

CHECK_DIR="${TMPDIR_BASE}/check"
mkdir -p "$CHECK_DIR"
EXPECTED_CHECK_ARGS="${CHECK_DIR}/expected-args.txt"
cat > "$EXPECTED_CHECK_ARGS" <<ARGS
run
${TOOLCHAIN}
cargo
xtask
check-toolchain
ARGS

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER"
) > "${CHECK_DIR}/out.txt" 2> "${CHECK_DIR}/err.txt" || code=$?
assert_exit_zero "delegates default check through pinned rustup toolchain" "$code"
assert_args_equal "defaults to check-toolchain" "$EXPECTED_CHECK_ARGS" "$FAKE_LOG"

DOCTOR_DIR="${TMPDIR_BASE}/doctor"
mkdir -p "$DOCTOR_DIR"
EXPECTED_DOCTOR_ARGS="${DOCTOR_DIR}/expected-args.txt"
cat > "$EXPECTED_DOCTOR_ARGS" <<ARGS
run
${TOOLCHAIN}
cargo
xtask
check-toolchain
--doctor
ARGS

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" doctor
) > "${DOCTOR_DIR}/out.txt" 2> "${DOCTOR_DIR}/err.txt" || code=$?
assert_exit_zero "delegates doctor mode through pinned rustup toolchain" "$code"
assert_args_equal "adds doctor flag" "$EXPECTED_DOCTOR_ARGS" "$FAKE_LOG"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" FAKE_RUSTUP_EXIT=37 bash "$WRAPPER" check
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates rustup/cargo failure from delegated command" "$code"

USAGE_DIR="${TMPDIR_BASE}/usage"
mkdir -p "$USAGE_DIR"
code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" unknown
) > "${USAGE_DIR}/out.txt" 2> "${USAGE_DIR}/err.txt" || code=$?
assert_exit_nonzero "rejects unknown mode" "$code"
if grep -Fq "Usage:" "${USAGE_DIR}/err.txt"; then
  pass "prints usage for unknown mode"
else
  fail "prints usage for unknown mode"
fi

# Toolchain guard (#12593): when rustup is absent the wrapper falls back to
# the plain PATH cargo, and that fallback must refuse a stale cargo. Simulate
# a rustup-less machine by fronting a stale cargo stub on PATH and hiding the
# real rustup (empty HOME/CARGO_HOME without .cargo/bin/rustup).
GUARD_BIN="${TMPDIR_BASE}/guard-bin"
mkdir -p "$GUARD_BIN" "${TMPDIR_BASE}/guard-home"
cat > "${GUARD_BIN}/cargo" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  printf 'cargo 1.75.0 (apt stub 2023-11-01)\n'
  exit 0
fi
exit 0
STUB
chmod +x "${GUARD_BIN}/cargo"

GUARD_DIR="${TMPDIR_BASE}/guard"
mkdir -p "$GUARD_DIR"
SYS_BIN="$(dirname "$(command -v bash)")"
code=0
(
  cd "$REPO_ROOT"
  PATH="${GUARD_BIN}:${SYS_BIN}:/usr/bin:/bin" HOME="${TMPDIR_BASE}/guard-home" CARGO_HOME="" bash "$WRAPPER" check
) > "${GUARD_DIR}/out.txt" 2> "${GUARD_DIR}/err.txt" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "refuses stale PATH cargo when rustup is absent (exit ${code})"
else
  fail "rustup-less fallback must refuse stale cargo with exit 78, got ${code}"
fi
if grep -Fq "cargo-toolchain-guard: REFUSED" "${GUARD_DIR}/err.txt"; then
  pass "stale fallback refusal prints the guard message"
else
  fail "stale fallback refusal prints the guard message"
fi

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
