#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
WRAPPER_SOURCE="${REPO_ROOT}/scripts/devex-doctor.sh"

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

write_fake_cargo_safe() {
  local script_dir="$1"
  local args_log="$2"
  local pwd_log="$3"

  mkdir -p "$script_dir"
  cat > "${script_dir}/cargo-safe" <<FAKE
#!/usr/bin/env bash
pwd > "${pwd_log}"
printf '%s\n' "\$@" > "${args_log}"
if [[ "\${FAKE_BOOTSTRAP_FAILURE:-0}" != 1 ]]; then
  echo 'devex-doctor: started'
  if [[ "\${FAKE_CARGO_SAFE_EXIT:-0}" != 0 ]]; then
    echo 'doctor_check_failed: fixture'
  fi
fi
if [[ -n "\${FAKE_CARGO_ERROR:-}" ]]; then
  echo "\$FAKE_CARGO_ERROR" >&2
fi
if [[ -n "\${FAKE_CARGO_ERROR_FILE:-}" ]]; then
  cat "\$FAKE_CARGO_ERROR_FILE" >&2
fi
exit "\${FAKE_CARGO_SAFE_EXIT:-0}"
FAKE
  chmod +x "${script_dir}/cargo-safe"
}

assert_file_equals() {
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

echo "=== devex-doctor wrapper test suite ==="
echo ""

if [[ ! -f "$WRAPPER_SOURCE" ]]; then
  echo "ERROR: devex-doctor.sh not found at ${WRAPPER_SOURCE}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
TEST_REPO="${TMPDIR_BASE}/repo"
TEST_SCRIPTS="${TEST_REPO}/scripts"
mkdir -p "$TEST_SCRIPTS"
cp "$WRAPPER_SOURCE" "${TEST_SCRIPTS}/devex-doctor.sh"

FAKE_ARGS_LOG="${TMPDIR_BASE}/cargo-safe-args.txt"
FAKE_PWD_LOG="${TMPDIR_BASE}/cargo-safe-pwd.txt"
write_fake_cargo_safe "$TEST_SCRIPTS" "$FAKE_ARGS_LOG" "$FAKE_PWD_LOG"

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_ARGS="${PASS_DIR}/expected-args.txt"
cat > "$EXPECTED_ARGS" <<'ARGS'
xtask
devex-doctor
ARGS
EXPECTED_PWD="${PASS_DIR}/expected-pwd.txt"
printf '%s\n' "$TEST_REPO" > "$EXPECTED_PWD"

code=0
(
  cd "$REPO_ROOT"
  bash "${TEST_SCRIPTS}/devex-doctor.sh"
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_zero "delegates to scripts/cargo-safe xtask devex-doctor" "$code"
assert_file_equals "runs from the wrapper repo root" "$EXPECTED_PWD" "$FAKE_PWD_LOG"
assert_file_equals "passes only the devex-doctor xtask command" "$EXPECTED_ARGS" "$FAKE_ARGS_LOG"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
code=0
(
  cd "$REPO_ROOT"
  FAKE_CARGO_SAFE_EXIT=37 bash "${TEST_SCRIPTS}/devex-doctor.sh"
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates cargo-safe failure from delegated command" "$code"
if grep -Fq 'doctor_exited (exit 37' "${FAIL_DIR}/err.txt"; then
  pass "doctor failure is distinguished from bootstrap failure"
else
  fail "doctor failure is distinguished from bootstrap failure"
fi

for case_name in locked nocargo toolchain generic; do
  case "$case_name" in
    locked) error='error: failed to replace xtask.exe: The process cannot access the file because it is being used by another process'; expected=bootstrap_artifact_locked_or_in_use ;;
    nocargo) error='cargo-toolchain-guard: REFUSED: no cargo found on PATH; this entrypoint requires a Rust toolchain >= 1.85 (rust-toolchain.toml pin: none).'; expected=bootstrap_tool_missing ;;
    toolchain) error='cargo-toolchain-guard: REFUSED: cargo predates edition-2024 support'; expected=bootstrap_toolchain_rejected ;;
    generic) error='error: failed to compile xtask'; expected=bootstrap_build_failed ;;
  esac
  code=0
  FAKE_BOOTSTRAP_FAILURE=1 FAKE_CARGO_SAFE_EXIT=37 FAKE_CARGO_ERROR="$error" \
    bash "${TEST_SCRIPTS}/devex-doctor.sh" > "${FAIL_DIR}/${case_name}.out" 2> "${FAIL_DIR}/${case_name}.err" || code=$?
  if [[ "$code" -eq 37 ]] && grep -Fq "$expected (exit 37" "${FAIL_DIR}/${case_name}.err" && \
    ! grep -Eq 'doctor_check_failed|Missing required tools|Install Rust' "${FAIL_DIR}/${case_name}.err"; then
    pass "$case_name bootstrap failure keeps exit and avoids false doctor finding"
  else
    fail "$case_name bootstrap failure keeps exit and avoids false doctor finding"
  fi
done

# An unrelated access error must not be promoted to a locked artifact just
# because the log also mentions xtask somewhere else: the access error has to
# name the executable on the same line (#16413).
MULTI_ERROR=$'   Compiling xtask v0.17.0 (/repo)\nerror: failed to create temporary directory `${TMPDIR}/cargo-build`: Access is denied. (os error 5)'
code=0
FAKE_BOOTSTRAP_FAILURE=1 FAKE_CARGO_SAFE_EXIT=37 FAKE_CARGO_ERROR="$MULTI_ERROR" \
  bash "${TEST_SCRIPTS}/devex-doctor.sh" > "${FAIL_DIR}/uncorrelated.out" 2> "${FAIL_DIR}/uncorrelated.err" || code=$?
if [[ "$code" -eq 37 ]] && grep -Fq 'bootstrap_build_failed (exit 37' "${FAIL_DIR}/uncorrelated.err" && \
  ! grep -Fq 'bootstrap_artifact_locked_or_in_use' "${FAIL_DIR}/uncorrelated.err"; then
  pass "unrelated access error with a distant xtask mention is not a locked artifact"
else
  fail "unrelated access error with a distant xtask mention is not a locked artifact"
fi

# These cases supply all output themselves, including any real startup marker.
# Use a file so the large-output regression does not depend on environment size.
assert_classification() {
  local label="$1"
  local expected="$2"
  local code=0
  FAKE_BOOTSTRAP_FAILURE=1 FAKE_CARGO_SAFE_EXIT=37 \
    FAKE_CARGO_ERROR_FILE="${FAIL_DIR}/classifier.input" \
    bash "${TEST_SCRIPTS}/devex-doctor.sh" > "${FAIL_DIR}/classifier.out" 2> "${FAIL_DIR}/classifier.err" || code=$?
  if [[ "$code" -eq 37 ]] && grep -Fq "devex: $expected (exit 37" "${FAIL_DIR}/classifier.err"; then
    pass "$label"
  else
    fail "$label (exit $code)"
    cat "${FAIL_DIR}/classifier.err"
  fi
}

for artifact in 'C:\repo\xtask\target' 'C:\repo\xtask\src\main.rs' 'C:\repo\target\debug\xtask.exe'; do
  printf 'error: failed to open %s: Access is denied. (os error 5)\n' "$artifact" > "${FAIL_DIR}/classifier.input"
  assert_classification "access denial is not proof of a lock: $artifact" bootstrap_build_failed
done

# Even sharing evidence must identify an executable, not a similarly named file
# or an xtask directory somewhere in the path.
for artifact in 'C:\repo\xtask\src\main.rs' 'C:\repo\target\debug\xtask.exe.d' '/repo/xtask/target' '/repo/not-xtask.exe'; do
  printf 'error: failed to open `%s`: being used by another process (os error 32)\n' "$artifact" > "${FAIL_DIR}/classifier.input"
  assert_classification "sharing evidence names a different artifact: $artifact" bootstrap_build_failed
done

for artifact in 'C:\repo\target\debug\xtask.exe' '/repo/target/debug/xtask' 'xtask.exe'; do
  printf 'error: failed to replace `%s`: being used by another process (os error 32)\n' "$artifact" > "${FAIL_DIR}/classifier.input"
  assert_classification "sharing evidence identifies executable: $artifact" bootstrap_artifact_locked_or_in_use
done

printf 'error: xtask.exe (os error 320)\n' > "${FAIL_DIR}/classifier.input"
assert_classification "error 320 is not sharing error 32" bootstrap_build_failed

cat > "${FAIL_DIR}/classifier.input" <<'ERROR'
error[E0425]: cannot find value `missing` in this scope
  --> xtask/src/tasks/devex_doctor.rs:19:1
19 | println!("devex-doctor: started", missing);
ERROR
assert_classification "compiler source excerpt does not establish doctor execution" bootstrap_build_failed

cat > "${FAIL_DIR}/classifier.input" <<'ERROR'
error[E0425]: cannot find value `missing` in this scope
  --> xtask/src/main.rs:1:69
1 | fn main() { println!("xtask.exe: being used by another process {}", missing); }
ERROR
assert_classification "compiler source excerpt does not establish an executable lock" bootstrap_build_failed

printf 'devex-doctor: started\nerror: mentioned doctor_check_failed in a probe\n' > "${FAIL_DIR}/classifier.input"
assert_classification "incidental finding text does not manufacture a doctor finding" doctor_instrument_failed

for finding in doctor_required_tool_missing doctor_required_component_missing doctor_check_failed; do
  printf 'devex-doctor: started\r\n❌ %s: fixture\r\n' "$finding" > "${FAIL_DIR}/classifier.input"
  assert_classification "CRLF marker and rendered $finding remain recognized" doctor_exited
done

# Enough matching output to exceed the pipe buffer: grep -q in the second
# stage used to close the pipe early and invert the condition under pipefail.
awk 'BEGIN { for (i = 0; i < 10000; i++) print "error: failed to replace xtask.exe: being used by another process (os error 32)" }' > "${FAIL_DIR}/classifier.input"
assert_classification "large matching output preserves lock classification" bootstrap_artifact_locked_or_in_use

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
