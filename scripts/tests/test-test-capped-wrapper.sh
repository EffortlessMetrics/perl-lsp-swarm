#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# fake cargo stubs answer with the workspace-required version and do not log it.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
WRAPPER="${REPO_ROOT}/scripts/test-capped.sh"

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
  local args_log="$2"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<FAKE
#!/usr/bin/env bash
printf '%s\n' "\$PWD" >> "${args_log}.cwd"
if [ "\${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "\${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
printf '%s\n' "\$@" > "${args_log}"
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

write_expected_cwd_log() {
  local dest="$1"
  local count="${2:-2}"
  : >"$dest"
  local i
  for ((i = 0; i < count; i++)); do
    printf '%s\n' "$REPO_ROOT" >>"$dest"
  done
}

assert_cwd_log() {
  local label="$1"
  local expected="$2"
  local actual="$3"

  assert_args_equal "$label" "$expected" "$actual"

  if [[ ! -f "$actual" ]]; then
    return
  fi

  local first=""
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ -z "$first" ]]; then
      first="$line"
      continue
    fi
    if [[ "$line" != "$first" ]]; then
      fail "${label}: guard probe and delegation used different cwd values"
      return
    fi
  done <"$actual"
  pass "${label}: guard probe and delegation share cwd"
}

echo "=== test-capped wrapper test suite ==="
echo ""

if [[ ! -f "$WRAPPER" ]]; then
  echo "ERROR: test-capped.sh not found at ${WRAPPER}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/cargo-args.txt"
write_fake_cargo "$FAKE_BIN" "$FAKE_LOG"

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_PASS_ARGS="${PASS_DIR}/expected-args.txt"
cat > "$EXPECTED_PASS_ARGS" <<'ARGS'
xtask
ci-hygiene
test-capped
--package
perl-lsp-rs-core
ARGS
EXPECTED_CWD_LOG="${PASS_DIR}/expected-cwd.txt"
write_expected_cwd_log "$EXPECTED_CWD_LOG" 2

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" --package perl-lsp-rs-core
) > "${PASS_DIR}/root-out.txt" 2> "${PASS_DIR}/root-err.txt" || code=$?
assert_exit_zero "delegates from the repository root" "$code"
assert_args_equal "forwards test-capped arguments from repository root" "$EXPECTED_PASS_ARGS" "$FAKE_LOG"
assert_cwd_log "probes and delegates from the repository root" "$EXPECTED_CWD_LOG" "${FAKE_LOG}.cwd"

rm -f "${FAKE_LOG}" "${FAKE_LOG}.cwd"
code=0
(
  cd "$TMPDIR_BASE"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" --package perl-lsp-rs-core
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_zero "delegates to cargo xtask ci-hygiene test-capped" "$code"
assert_args_equal "forwards test-capped arguments unchanged" "$EXPECTED_PASS_ARGS" "$FAKE_LOG"
assert_cwd_log "probes and delegates from the owning repository outside the checkout" \
  "$EXPECTED_CWD_LOG" "${FAKE_LOG}.cwd"

rm -f "${FAKE_LOG}" "${FAKE_LOG}.cwd"
code=0
(
  cd "$REPO_ROOT/scripts/tests"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" --package perl-lsp-rs-core
) > "${PASS_DIR}/nested-out.txt" 2> "${PASS_DIR}/nested-err.txt" || code=$?
assert_exit_zero "delegates from a nested repository directory" "$code"
assert_cwd_log "probes and delegates from the owning repository for nested callers" \
  "$EXPECTED_CWD_LOG" "${FAKE_LOG}.cwd"

rm -f "${FAKE_LOG}" "${FAKE_LOG}.cwd"
code=0
(
  cd "$REPO_ROOT/scripts/tests"
  PATH="${FAKE_BIN}:$PATH" bash ../test-capped.sh --package perl-lsp-rs-core
) > "${PASS_DIR}/relative-out.txt" 2> "${PASS_DIR}/relative-err.txt" || code=$?
assert_exit_zero "delegates through a relative nested invocation" "$code"
assert_cwd_log "relative invocation still probes before delegation" \
  "$EXPECTED_CWD_LOG" "${FAKE_LOG}.cwd"

EXPECTED_SPACE_ARGS="${PASS_DIR}/expected-space-args.txt"
cat > "$EXPECTED_SPACE_ARGS" <<'ARGS'
xtask
ci-hygiene
test-capped
--package
hello world
ARGS
rm -f "${FAKE_LOG}" "${FAKE_LOG}.cwd"
code=0
(
  cd "$TMPDIR_BASE"
  PATH="${FAKE_BIN}:$PATH" bash "$WRAPPER" --package "hello world"
) > "${PASS_DIR}/space-out.txt" 2> "${PASS_DIR}/space-err.txt" || code=$?
assert_exit_zero "delegates arguments containing spaces" "$code"
assert_args_equal "preserves arguments with spaces verbatim" "$EXPECTED_SPACE_ARGS" "$FAKE_LOG"
assert_cwd_log "runs guard and delegation in repo root when args contain spaces" \
  "$EXPECTED_CWD_LOG" "${FAKE_LOG}.cwd"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
rm -f "${FAKE_LOG}" "${FAKE_LOG}.cwd"
code=0
(
  cd "$TMPDIR_BASE"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_EXIT=37 bash "$WRAPPER" --package perl-lsp-rs-core
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates cargo failure from delegated command" "$code"

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
