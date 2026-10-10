#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# fake cargo stubs answer with the workspace-required version and do not log it.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
INSTALL_GITHOOKS_SCRIPT="${REPO_ROOT}/scripts/install-githooks.sh"

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

echo "=== install-githooks wrapper test suite ==="
echo ""

if [[ ! -f "$INSTALL_GITHOOKS_SCRIPT" ]]; then
  echo "ERROR: install-githooks.sh not found at ${INSTALL_GITHOOKS_SCRIPT}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/cargo-args.txt"
write_fake_cargo "$FAKE_BIN" "$FAKE_LOG"
# Exercise the real first-mile wrappers; stub only the admission/build boundary.
# The production admission guard has its own focused suite. No fake free-space
# observation is passed into the actual wrapper on this host.
FIXTURE_ROOT="${TMPDIR_BASE}/repo with spaces"
mkdir -p "$FIXTURE_ROOT/scripts"
cp "$REPO_ROOT/scripts/"{install-githooks.sh,check-githooks.sh,githooks-bootstrap.sh} "$FIXTURE_ROOT/scripts/"
cat > "$FIXTURE_ROOT/scripts/cargo-admitted" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$PWD" > "$FAKE_CWD_LOG"
exec cargo "$@"
STUB
chmod +x "$FIXTURE_ROOT/scripts/cargo-admitted"
INSTALL_GITHOOKS_SCRIPT="$FIXTURE_ROOT/scripts/install-githooks.sh"
export FAKE_CWD_LOG="${TMPDIR_BASE}/cwd.txt"

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_PASS_ARGS="${PASS_DIR}/expected-args.txt"
cat > "$EXPECTED_PASS_ARGS" <<'ARGS'
run
--locked
-p
perl-ci-hygiene
--
install-githooks
--check
ARGS

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$INSTALL_GITHOOKS_SCRIPT" --check
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_zero "delegates to admitted lightweight hygiene package" "$code"
assert_args_equal "forwards install-githooks arguments unchanged" "$EXPECTED_PASS_ARGS" "$FAKE_LOG"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_EXIT=37 bash "$INSTALL_GITHOOKS_SCRIPT" --check
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates cargo failure from delegated command" "$code"

if [[ "$(cat "$FAKE_CWD_LOG")" == "$FIXTURE_ROOT" ]]; then
  pass "bootstrap selects its own revision from an unrelated working directory"
else
  fail "bootstrap cwd must be its own repository"
fi
EXPECTED_CHECK_ARGS="${TMPDIR_BASE}/check-args.txt"
sed 's/install-githooks/check-githooks/; /--check/d' "$EXPECTED_PASS_ARGS" > "$EXPECTED_CHECK_ARGS"
code=0
PATH="${FAKE_BIN}:$PATH" bash "$FIXTURE_ROOT/scripts/check-githooks.sh" || code=$?
assert_exit_zero "check uses the read-only owning command" "$code"
assert_args_equal "check routes to the same lightweight package" "$EXPECTED_CHECK_ARGS" "$FAKE_LOG"
# A stale same-name executable on PATH must not satisfy bootstrap failure.
printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_BIN/perl-ci-hygiene"
chmod +x "$FAKE_BIN/perl-ci-hygiene"
code=0
PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_EXIT=37 bash "$INSTALL_GITHOOKS_SCRIPT" || code=$?
if [[ "$code" -eq 37 ]]; then
  pass "bootstrap failure propagates exactly despite stale PATH executable"
else
  fail "bootstrap failure must preserve status 37"
fi
rm "$FIXTURE_ROOT/scripts/cargo-admitted"
code=0
PATH="${FAKE_BIN}:$PATH" bash "$INSTALL_GITHOOKS_SCRIPT" > "$TMPDIR_BASE/missing-admission.log" 2>&1 || code=$?
assert_exit_nonzero "missing admission executable cannot report installation success" "$code"
TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
