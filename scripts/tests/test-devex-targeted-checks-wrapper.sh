#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# fake cargo stubs answer with the workspace-required version and do not log it.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
WRAPPER="${REPO_ROOT}/scripts/devex-targeted-checks.sh"

# Evidence contract: run the public wrapper from outside the repository; each
# case must delegate exactly once with literal argv, repository cwd, and the
# stub's exact exit status. Append to fresh per-case receipts so missing/repeated
# executions cannot reuse or overwrite earlier evidence. Cargo is stubbed here;
# this suite does not prove xtask's check selection or execution.

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

assert_exit_equals() {
  local label="$1"
  local expected="$2"
  local code="$3"
  if [[ "$code" -eq "$expected" ]]; then
    pass "$label (exit ${code} as expected)"
  else
    fail "$label (expected exit ${expected}, got ${code})"
  fi
}

write_fake_cargo() {
  local fake_bin="$1"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
printf '%s\0' "$@" >> "${FAKE_CARGO_LOG_DIR:?}/args.bin"
pwd >> "${FAKE_CARGO_LOG_DIR}/cwd.txt"
exit "${FAKE_CARGO_EXIT:-0}"
FAKE
  chmod +x "${fake_bin}/cargo"
}

assert_file_equals() {
  local label="$1"
  local expected="$2"
  local actual="$3"

  if cmp -s "$expected" "$actual"; then
    pass "$label"
  else
    fail "$label"
    printf 'expected bytes:\n'
    od -An -tc "$expected"
    if [[ -f "$actual" ]]; then
      printf 'actual bytes:\n'
      od -An -tc "$actual"
    else
      printf 'missing receipt: %s\n' "$actual"
    fi
  fi
}

echo "=== devex-targeted-checks wrapper test suite ==="
echo ""

if [[ ! -f "$WRAPPER" ]]; then
  echo "ERROR: devex-targeted-checks.sh not found at ${WRAPPER}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
write_fake_cargo "$FAKE_BIN"

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_PASS_ARGS="${PASS_DIR}/expected-args.bin"
printf '%s\0' xtask targeted-checks --base origin/main --mode tests > "$EXPECTED_PASS_ARGS"
EXPECTED_PWD="${TMPDIR_BASE}/expected-cwd.txt"
printf '%s\n' "$REPO_ROOT" > "$EXPECTED_PWD"

code=0
(
  cd "$TMPDIR_BASE"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_LOG_DIR="$PASS_DIR" FAKE_CARGO_EXIT=0 bash "$WRAPPER" --base origin/main --mode tests
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_equals "delegates to cargo xtask targeted-checks" 0 "$code"
assert_file_equals "forwards success arguments exactly once" "$EXPECTED_PASS_ARGS" "${PASS_DIR}/args.bin"
assert_file_equals "success command runs from the wrapper repo root" "$EXPECTED_PWD" "${PASS_DIR}/cwd.txt"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
EXPECTED_FAIL_ARGS="${FAIL_DIR}/expected-args.bin"
printf '%s\0' xtask targeted-checks --mode all > "$EXPECTED_FAIL_ARGS"
code=0
(
  cd "$TMPDIR_BASE"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_LOG_DIR="$FAIL_DIR" FAKE_CARGO_EXIT=37 bash "$WRAPPER" --mode all
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_equals "propagates exact cargo failure from delegated command" 37 "$code"
assert_file_equals "forwards failure arguments exactly once" "$EXPECTED_FAIL_ARGS" "${FAIL_DIR}/args.bin"
assert_file_equals "failing command runs from the wrapper repo root" "$EXPECTED_PWD" "${FAIL_DIR}/cwd.txt"

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
