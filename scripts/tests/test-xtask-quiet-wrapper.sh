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

# Fake cargo: answers the guard probe and logs any other invocation so tests
# can prove the wrapper never falls back to raw `cargo build`.
write_fake_cargo() {
  local fake_bin="$1" cargo_log="$2"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "\${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
rendered=""
for arg in "\$@"; do
  rendered+="[\${arg}]"
done
printf 'cargo args=%s\n' "\${rendered}" >> "${cargo_log}"
printf 'error[E0583]: unexpected raw cargo invocation (stub)\n' >&2
exit 97
FAKE
  chmod +x "${fake_bin}/cargo"
}

# Fake cargo-admitted: logs argv, optionally fails, and emits the admitted
# resource descriptor the wrapper uses to locate the binary.
write_fake_admitted() {
  local fake_bin="$1" fake_target_dir="$2" admitted_log="$3"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo-admitted" <<FAKE
#!/usr/bin/env bash
rendered=""
for arg in "\$@"; do
  rendered+="[\${arg}]"
done
printf 'admitted args=%s\n' "\${rendered}" >> "${admitted_log}"
if [[ -n "\${FAKE_ADMITTED_FAIL_PATTERN:-}" && "\${rendered}" == *"\${FAKE_ADMITTED_FAIL_PATTERN}"* ]]; then
  printf 'cargo-admitted: DENY: fake admission failure (stub)\n' >&2
  printf 'error[E0583]: fake build failure (stub)\n' >&2
  exit "\${FAKE_ADMITTED_EXIT:-37}"
fi
if [[ "\${FAKE_ADMITTED_OMIT_RESOURCES:-}" != "1" ]]; then
  printf 'cargo-admitted resources: {"resources":{"target":"%s"}}\n' "${fake_target_dir}" >&2
fi
# A successful build still emits compiler noise on stderr; the wrapper must
# discard it, so the success-path test asserts this marker is never replayed.
printf 'warning: replayed-noise-marker (stub)\n' >&2
exit 0
FAKE
  chmod +x "${fake_bin}/cargo-admitted"
}

# Fake xtask binary: echoes distinguishable markers on both streams, logs its
# argv, and exits with FAKE_XTASK_EXIT. Both spellings are provided because
# the wrapper prefers `xtask.exe` on Windows.
write_fake_xtask() {
  local fake_binary_dir="$1" argv_log="$2" stdout_marker="${3:-xtask-stdout-marker}"

  mkdir -p "$fake_binary_dir"
  for name in xtask xtask.exe; do
    cat > "${fake_binary_dir}/${name}" <<FAKE
#!/usr/bin/env bash
rendered=""
for arg in "\$@"; do
  rendered+="[\${arg}]"
done
printf '%s\n' "\${rendered}" >> "${argv_log}"
pwd > "${argv_log}.cwd"
printf '%s\n' "${stdout_marker}"
printf 'xtask-stderr-marker\n' >&2
exit "\${FAKE_XTASK_EXIT:-0}"
FAKE
    chmod +x "${fake_binary_dir}/${name}"
  done
}

make_scenario_dir() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "${dir}/bin"
  write_fake_cargo "${dir}/bin" "${dir}/cargo.log"
  write_fake_admitted "${dir}/bin" "${dir}/target" "${dir}/admitted.log"
  write_fake_xtask "${dir}/target/debug" "${dir}/argv.log"
  printf '%s' "$dir"
}

# Register a scenario directory for cleanup in the PARENT shell. Appending to
# TMPDIR_BASES inside make_scenario_dir would mutate a command-substitution
# subshell's copy and leak every fixture directory.
register_scenario_dir() {
  TMPDIR_BASES+=("$1")
}

run_wrapper() {
  local dir="$1"
  shift
  PATH="${dir}/bin:${PATH}" \
    XTASK_QUIET_CARGO_ADMITTED="${dir}/bin/cargo-admitted" \
    bash "$XTASK_QUIET_SCRIPT" "$@"
}

# Scenario 1 (success path): the admitted build succeeds, the wrapper must not
# replay any build noise, must pass the command's stdout/stderr through
# untouched, must forward arguments verbatim, and must exit with the xtask
# exit status.
scenario_success_path() {
  local dir out err status argv admitted_args cargo_log_size
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  out="$(run_wrapper "$dir" ci doctor --help 2>"${dir}/err.txt")" || status=$?
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
  assert_contains "success: args forwarded verbatim" "$argv" "[ci][doctor][--help]"

  admitted_args="$(cat "${dir}/admitted.log")"
  assert_contains "success: build uses admitted cargo-admitted build -p xtask --locked" \
    "$admitted_args" "[build][-p][xtask][--locked]"

  if [[ -f "${dir}/cargo.log" ]]; then
    cargo_log_size="$(wc -c <"${dir}/cargo.log")"
  else
    cargo_log_size=0
  fi
  if [[ "$cargo_log_size" -eq 0 ]]; then
    pass "success: raw cargo is not invoked for the build"
  else
    fail "success: raw cargo was invoked (${cargo_log_size} bytes)"
  fi
}

# Scenario 2 (failure path): when the admitted build fails, the captured
# output must be shown verbatim, the producer exit status must propagate, and
# the xtask binary must not run at all.
scenario_failure_path() {
  local dir out err status argv_log_size
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  out="$(FAKE_ADMITTED_FAIL_PATTERN='xtask' FAKE_ADMITTED_EXIT=37 \
    run_wrapper "$dir" ci doctor --help 2>"${dir}/err.txt")" || status=$?
  err="$(cat "${dir}/err.txt")"

  if [[ "$status" -eq 37 ]]; then
    pass "failure: cargo-admitted exit status propagates (37)"
  else
    fail "failure: wrapper exit (expected 37, got ${status})"
  fi
  assert_contains "failure: build error output is shown verbatim" "$err" "error[E0583]"
  assert_contains "failure: admission denial is shown" "$err" "cargo-admitted: DENY:"
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
# admitted build invocation, and only the build invocation. Fixture-only feature
# names contain selector text; validating feature names belongs to the producer.
scenario_build_flags_passthrough() {
  local dir status admitted_args
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  XTASK_QUIET_BUILD_FLAGS="--features parser-tasks --features=fixture--release,fixture--profile --message-format=json-render-diagnostics" \
    run_wrapper "$dir" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -ne 0 ]]; then
    fail "build flags passthrough: wrapper failed (exit ${status})"
    return
  fi
  admitted_args="$(cat "${dir}/admitted.log")"
  assert_contains "build flags passthrough: XTASK_QUIET_BUILD_FLAGS forwarded" \
    "$admitted_args" "[--features][parser-tasks]"
  assert_contains "build flags passthrough: profile text inside feature values is preserved" \
    "$admitted_args" "[--features=fixture--release,fixture--profile]"
  assert_contains "build flags passthrough: -r inside a value is not a profile selector" \
    "$admitted_args" "[--message-format=json-render-diagnostics]"
  assert_contains "build flags passthrough: default xtask executes" \
    "$(cat "${dir}/out.txt")" "xtask-stdout-marker"
}

# Scenario 4 (xtask exit status): a nonzero xtask exit must survive the exec
# handoff unchanged.
scenario_xtask_exit_status() {
  local dir status
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  FAKE_XTASK_EXIT=9 \
    run_wrapper "$dir" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -eq 9 ]]; then
    pass "xtask exit status propagates (9)"
  else
    fail "xtask exit status (expected 9, got ${status})"
  fi
}

# Scenario 5: a successful admitted producer that omits the resource
# descriptor must not execute a pre-existing binary.
scenario_missing_resources_does_not_exec() {
  local dir status out err argv_log_size
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  out="$(FAKE_ADMITTED_OMIT_RESOURCES=1 \
    run_wrapper "$dir" ci doctor --help 2>"${dir}/err.txt")" || status=$?
  err="$(cat "${dir}/err.txt")"

  if [[ "$status" -eq 70 ]]; then
    pass "missing resources: wrapper exits 70"
  else
    fail "missing resources: wrapper exit (expected 70, got ${status})"
  fi
  assert_contains "missing resources: diagnostic names the descriptor" \
    "$err" "cannot resolve the admitted target directory"
  assert_not_contains "missing resources: xtask is not executed" \
    "${out}${err}" "xtask-stdout-marker"

  if [[ -f "${dir}/argv.log" ]]; then
    argv_log_size="$(wc -c <"${dir}/argv.log")"
  else
    argv_log_size=0
  fi
  if [[ "$argv_log_size" -eq 0 ]]; then
    pass "missing resources: fake xtask argv log is empty"
  else
    fail "missing resources: fake xtask argv log is not empty (${argv_log_size} bytes)"
  fi
}

# Scenario 6: application arguments, including profile spellings, spaces and
# Unicode, must reach the tool unchanged in the caller's working directory.
scenario_argv_spaces_and_unicode() {
  local dir status argv expected_argv caller_dir
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  mkdir -p "${dir}/caller"
  caller_dir="$(cd "${dir}/caller" && pwd)"

  status=0
  (cd "$caller_dir" && XTASK_QUIET_BUILD_FLAGS="" \
    run_wrapper "$dir" "arg with space" "unicodé-Δ" \
      --release -r --profile agent --profile=dev) \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -ne 0 ]]; then
    fail "argv spaces/unicode: wrapper failed (exit ${status})"
    return
  fi
  argv="$(cat "${dir}/argv.log")"
  expected_argv="[arg with space][unicodé-Δ][--release][-r][--profile][agent][--profile=dev]"
  if [[ "$argv" == "$expected_argv" ]]; then
    pass "argv spaces/unicode/profile flags: exact tokens reach xtask"
  else
    fail "argv spaces/unicode/profile flags: expected ${expected_argv}, got ${argv}"
  fi
  if [[ "$(cat "${dir}/argv.log.cwd")" == "$caller_dir" ]]; then
    pass "argv: xtask runs in the caller's working directory"
  else
    fail "argv: xtask did not run in the caller's working directory"
  fi
}

# Scenario 7: source pin — the production wrapper names cargo-admitted and
# does not contain a raw `cargo build` producer.
scenario_source_pins_admitted_producer() {
  local source
  source="$(cat "$XTASK_QUIET_SCRIPT")"
  assert_contains "source: production producer is scripts/cargo-admitted" \
    "$source" 'scripts/cargo-admitted'
  assert_contains "source: admitted producer is resolved from script_dir" \
    "$source" '${script_dir}/cargo-admitted'
  assert_contains "source: admitted argv is build -p xtask --locked" \
    "$source" '"$admitted" build -p xtask --locked'
  if grep -Eq '^[[:space:]]*cargo[[:space:]]+build([[:space:]]|$)' "$XTASK_QUIET_SCRIPT"; then
    fail "source: raw cargo build producer is still present"
  else
    pass "source: raw cargo build producer is absent"
  fi
}

# Scenario 8: every explicit build-profile selector is unsupported, including
# redundant dev selections. Existing executables make a skipped build visible:
# refusal must occur before either the producer or any xtask binary executes.
scenario_profile_flags_are_refused() {
  local flags dir status err
  for flags in \
    "--release" \
    "-r" \
    "--profile agent" \
    "--profile=agent" \
    "--profile dev" \
    "--profile=dev" \
    "--features parser-tasks --profile=agent"; do
    dir="$(make_scenario_dir)"
    register_scenario_dir "$dir"
    write_fake_xtask "${dir}/target/debug" "${dir}/argv.log" "stale-debug-marker"
    write_fake_xtask "${dir}/target/release" "${dir}/argv.log" "alternate-release-marker"
    write_fake_xtask "${dir}/target/agent" "${dir}/argv.log" "alternate-agent-marker"

    status=0
    XTASK_QUIET_BUILD_FLAGS="$flags" run_wrapper "$dir" ci doctor --help \
      >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?
    err="$(cat "${dir}/err.txt")"

    if [[ "$status" -eq 70 ]]; then
      pass "profile ${flags}: wrapper exits 70"
    else
      fail "profile ${flags}: wrapper exit (expected 70, got ${status}; stdout: $(cat "${dir}/out.txt"))"
    fi
    assert_contains "profile ${flags}: diagnostic explains the supported profile" \
      "$err" "default dev-profile"
    if [[ ! -s "${dir}/admitted.log" ]]; then
      pass "profile ${flags}: admitted producer is not invoked"
    else
      fail "profile ${flags}: admitted producer was invoked"
    fi
    if [[ ! -s "${dir}/argv.log" ]]; then
      pass "profile ${flags}: no xtask executable runs"
    else
      fail "profile ${flags}: an xtask executable ran"
    fi
    if [[ ! -s "${dir}/cargo.log" ]]; then
      pass "profile ${flags}: raw cargo is not invoked for the build"
    else
      fail "profile ${flags}: raw cargo was invoked for the build"
    fi
  done
}

scenario_success_path
scenario_failure_path
scenario_build_flags_passthrough
scenario_xtask_exit_status
scenario_missing_resources_does_not_exec
scenario_argv_spaces_and_unicode
scenario_source_pins_admitted_producer
scenario_profile_flags_are_refused

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
