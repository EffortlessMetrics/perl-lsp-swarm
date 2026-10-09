#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# the fake cargo stub answers with the workspace-required version.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
XTASK_QUIET_SCRIPT="${REPO_ROOT}/scripts/xtask-quiet.sh"

PASS=0
FAIL=0
TMPDIR_BASES=()

cleanup() {
  local dir
  for dir in "${TMPDIR_BASES[@]}"; do
    if [[ -d "$dir" ]]; then
      rm -rf "$dir"
    fi
  done
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

assert_contains() {
  local label="$1" haystack="$2" needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    pass "$label"
  else
    fail "$label (missing: ${needle})"
  fi
}

assert_not_contains() {
  local label="$1" haystack="$2" needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    pass "$label"
  else
    fail "$label (unexpectedly present: ${needle})"
  fi
}

# Fake cargo: answers the guard probe, serves `metadata` with a fixture
# target directory, and logs (optionally fails) `build` invocations.
write_fake_cargo() {
  local fake_bin="$1" fake_target_dir="$2" build_log="$3"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "\${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
if [ "\${1:-}" = "metadata" ]; then printf '{"target_directory":"%s"}\n' "${fake_target_dir}"; exit 0; fi
rendered=""
for arg in "\$@"; do
  rendered+="[\${arg}]"
done
printf 'build args=%s\n' "\${rendered}" >> "${build_log}"
if [[ -n "\${FAKE_CARGO_FAIL_PATTERN:-}" && "\${rendered}" == *"\${FAKE_CARGO_FAIL_PATTERN}"* ]]; then
  printf 'error[E0583]: fake build failure (stub)\n' >&2
  exit "\${FAKE_CARGO_EXIT:-37}"
fi
# A successful build still emits compiler noise on stderr; the wrapper must
# discard it, so the success-path test asserts this marker is never replayed.
printf 'warning: replayed-noise-marker (stub)\n' >&2
exit 0
FAKE
  chmod +x "${fake_bin}/cargo"
}

# Fake xtask binary: echoes distinguishable markers on both streams, logs its
# argv, and exits with FAKE_XTASK_EXIT. Both spellings are provided because
# the wrapper prefers `xtask.exe` on Windows.
write_fake_xtask() {
  local fake_target_dir="$1" argv_log="$2"

  mkdir -p "${fake_target_dir}/debug"
  for name in xtask xtask.exe; do
    cat > "${fake_target_dir}/debug/${name}" <<FAKE
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "${argv_log}"
printf 'xtask-stdout-marker\n'
printf 'xtask-stderr-marker\n' >&2
exit "\${FAKE_XTASK_EXIT:-0}"
FAKE
    chmod +x "${fake_target_dir}/debug/${name}"
  done
}

make_scenario_dir() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "${dir}/bin"
  write_fake_cargo "${dir}/bin" "${dir}/target" "${dir}/build.log"
  write_fake_xtask "${dir}/target" "${dir}/argv.log"
  printf '%s' "$dir"
}

# Register a scenario directory for cleanup in the PARENT shell. Appending to
# TMPDIR_BASES inside make_scenario_dir would mutate a command-substitution
# subshell's copy and leak every fixture directory.
register_scenario_dir() {
  TMPDIR_BASES+=("$1")
}

# Scenario 1 (success path): the build succeeds, the wrapper must not replay
# any build noise, must pass the command's stdout/stderr through untouched,
# must forward arguments verbatim, and must exit with the xtask exit status.
scenario_success_path() {
  local dir out err status argv build_args
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  local fake_bin="${dir}/bin"

  status=0
  out="$(PATH="${fake_bin}:${PATH}" bash "$XTASK_QUIET_SCRIPT" ci doctor --help 2>"${dir}/err.txt")" || status=$?
  err="$(cat "${dir}/err.txt")"

  if [[ "$status" -eq 0 ]]; then
    pass "success: wrapper exits with the xtask exit status (0)"
  else
    fail "success: wrapper exit (expected 0, got ${status})"
  fi
  assert_contains "success: xtask stdout passes through" "$out" "xtask-stdout-marker"
  assert_contains "success: xtask stderr passes through" "$err" "xtask-stderr-marker"
  assert_not_contains "success: build noise is not replayed" "$err${out}" "replayed-noise-marker"

  argv="$(cat "${dir}/argv.log")"
  assert_contains "success: args forwarded verbatim" "$argv" "ci doctor --help"

  build_args="$(cat "${dir}/build.log")"
  assert_contains "success: build uses --package xtask --locked" \
    "$build_args" "[build][--package][xtask][--locked]"
}

# Scenario 2 (failure path): when the build fails, the captured build output
# must be shown verbatim, the cargo exit status must propagate, and the xtask
# binary must not run at all.
scenario_failure_path() {
  local dir out err status argv_log_size
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  local fake_bin="${dir}/bin"

  status=0
  out="$(FAKE_CARGO_FAIL_PATTERN='package' FAKE_CARGO_EXIT=37 \
    PATH="${fake_bin}:${PATH}" bash "$XTASK_QUIET_SCRIPT" ci doctor --help 2>"${dir}/err.txt")" || status=$?
  err="$(cat "${dir}/err.txt")"

  if [[ "$status" -eq 37 ]]; then
    pass "failure: cargo build exit status propagates (37)"
  else
    fail "failure: wrapper exit (expected 37, got ${status})"
  fi
  assert_contains "failure: build error output is shown verbatim" "$err" "error[E0583]"
  assert_not_contains "failure: xtask is not executed" "${out}${err}" "xtask-stdout-marker"

  if [[ -f "${dir}/argv.log" ]]; then
    argv_log_size="$(wc -c <"${dir}/argv.log")"
  else
    argv_log_size=0
  fi
  if [[ "$argv_log_size" -eq 0 ]]; then
    pass "failure: fake xtask argv log is empty"
  else
    fail "failure: fake xtask argv log is not empty (${argv_log_size} bytes)"
  fi
}

# Scenario 3 (build-flag passthrough): XTASK_QUIET_BUILD_FLAGS reaches the
# cargo build invocation, and only the build invocation.
scenario_build_flags_passthrough() {
  local dir status build_args
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  local fake_bin="${dir}/bin"

  status=0
  XTASK_QUIET_BUILD_FLAGS="--features parser-tasks" \
    PATH="${fake_bin}:${PATH}" bash "$XTASK_QUIET_SCRIPT" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -ne 0 ]]; then
    fail "build flags passthrough: wrapper failed (exit ${status})"
    return
  fi
  build_args="$(cat "${dir}/build.log")"
  assert_contains "build flags passthrough: XTASK_QUIET_BUILD_FLAGS forwarded" \
    "$build_args" "[--features][parser-tasks]"
}

# Scenario 4 (xtask exit status): a nonzero xtask exit must survive the exec
# handoff unchanged.
scenario_xtask_exit_status() {
  local dir status
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  local fake_bin="${dir}/bin"

  status=0
  FAKE_XTASK_EXIT=9 \
    PATH="${fake_bin}:${PATH}" bash "$XTASK_QUIET_SCRIPT" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -eq 9 ]]; then
    pass "xtask exit status propagates (9)"
  else
    fail "xtask exit status (expected 9, got ${status})"
  fi
}

scenario_success_path
scenario_failure_path
scenario_build_flags_passthrough
scenario_xtask_exit_status

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
