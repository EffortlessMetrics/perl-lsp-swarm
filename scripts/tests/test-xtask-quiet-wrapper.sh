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
  local fake_target_dir="$1" argv_log="$2"

  mkdir -p "${fake_target_dir}/debug"
  for name in xtask xtask.exe; do
    cat > "${fake_target_dir}/debug/${name}" <<FAKE
#!/usr/bin/env bash
rendered=""
for arg in "\$@"; do
  rendered+="[\${arg}]"
done
printf '%s\n' "\${rendered}" >> "${argv_log}"
printf '%s\0' "\$@" > "${argv_log}.nul"
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
  write_fake_cargo "${dir}/bin" "${dir}/cargo.log"
  write_fake_admitted "${dir}/bin" "${dir}/target" "${dir}/admitted.log"
  write_fake_xtask "${dir}/target" "${dir}/argv.log"
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
# admitted build invocation, and only the build invocation.
scenario_build_flags_passthrough() {
  local dir status admitted_args
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  XTASK_QUIET_BUILD_FLAGS="--features parser-tasks" \
    run_wrapper "$dir" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -ne 0 ]]; then
    fail "build flags passthrough: wrapper failed (exit ${status})"
    return
  fi
  admitted_args="$(cat "${dir}/admitted.log")"
  assert_contains "build flags passthrough: XTASK_QUIET_BUILD_FLAGS forwarded" \
    "$admitted_args" "[--features][parser-tasks]"
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

# Scenario 6: argv with spaces and Unicode must reach the tool unchanged.
scenario_argv_spaces_and_unicode() {
  local dir status argv
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"

  status=0
  run_wrapper "$dir" "arg with space" "unicodé-Δ" \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?

  if [[ "$status" -ne 0 ]]; then
    fail "argv spaces/unicode: wrapper failed (exit ${status})"
    return
  fi
  argv="$(cat "${dir}/argv.log")"
  assert_contains "argv spaces/unicode: exact tokens reach xtask" \
    "$argv" "[arg with space][unicodé-Δ]"
}

# Each accepted form must preserve its exact build argv. The glob names an
# existing source file so an unquoted expansion cannot pass this fixture by
# leaving an unmatched pattern unchanged.
scenario_supported_build_flags() {
  local flags expected dir status admitted_args
  local cases=(
    $' \t\n'
    '--features=parser-tasks,other-feature'
    '-F parser-tasks'
    '--all-features --no-default-features'
    $' \t--features\nparser-tasks\t--no-default-features\n'
    '--features parser-tasks -F other-feature --features=third-feature'
    '--features scripts/xtask-quiet.s?'
    '--features feature--target-name'
  )
  local expected_args=(
    ''
    '[--features=parser-tasks,other-feature]'
    '[-F][parser-tasks]'
    '[--all-features][--no-default-features]'
    '[--features][parser-tasks][--no-default-features]'
    '[--features][parser-tasks][-F][other-feature][--features=third-feature]'
    '[--features][scripts/xtask-quiet.s?]'
    '[--features][feature--target-name]'
  )
  local index
  for ((index = 0; index < ${#cases[@]}; index++)); do
    flags="${cases[index]}"
    expected="admitted args=[build][-p][xtask][--locked]${expected_args[index]}"
    dir="$(make_scenario_dir)"
    register_scenario_dir "$dir"
    status=0
    XTASK_QUIET_BUILD_FLAGS="$flags" run_wrapper "$dir" ci doctor --help \
      >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?
    admitted_args="$(cat "${dir}/admitted.log" 2>/dev/null || true)"
    if [[ "$status" -eq 0 && "$admitted_args" == "$expected" && -s "${dir}/argv.log" ]]; then
      pass "supported build flags: exact argv case ${index}"
    else
      fail "supported build flags: case ${index} (exit ${status}, argv ${admitted_args})"
    fi
  done
}

# A pre-existing debug binary is present in every scenario. A producer that
# accepts a different artifact/profile or a no-work option must never let the
# wrapper dispatch that stale binary. Refuse before invoking the producer.
scenario_unsupported_build_flags() {
  local flags label dir status err
  local cases=(
    '--release'
    '-r'
    '--profile agent'
    '--profile=release'
    '--bin other'
    '--bin=other'
    '--lib'
    '--example other'
    '--tests'
    '--all-targets'
    '--package other'
    '--package=other'
    '-p other'
    '--workspace'
    '--exclude xtask'
    '--help'
    '-h'
    '--version'
    '-V'
    '--message-format=json'
    '--message-format json'
    '--target example-triple'
    '--target=example-triple'
    '--target-dir other'
    '--'
    '--features parser-tasks --release'
    $'--features parser-tasks\n--release'
    '--features'
    '--features='
    '-F'
    '--features --release'
    '--features=--release'
    '-F --release'
    '--features= --all-features'
  )
  for flags in "${cases[@]}"; do
    printf -v label '%q' "$flags"
    dir="$(make_scenario_dir)"
    register_scenario_dir "$dir"
    status=0
    XTASK_QUIET_BUILD_FLAGS="$flags" run_wrapper "$dir" ci doctor --help \
      >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?
    err="$(cat "${dir}/err.txt")"
    if [[ "$status" -eq 70 && -n "$err" && ! -s "${dir}/out.txt" && \
      ! -s "${dir}/admitted.log" && ! -s "${dir}/argv.log" ]]; then
      pass "unsupported build flags: ${label} refused before build/dispatch"
    else
      fail "unsupported build flags: ${label} reached build/dispatch or returned ${status}"
    fi
  done
}

scenario_build_target_environment() {
  local dir status err
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  status=0
  CARGO_BUILD_TARGET=example-triple run_wrapper "$dir" ci doctor --help \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?
  err="$(cat "${dir}/err.txt")"
  if [[ "$status" -eq 70 && "$err" == *CARGO_BUILD_TARGET* && \
    ! -s "${dir}/out.txt" && ! -s "${dir}/admitted.log" && ! -s "${dir}/argv.log" ]]; then
    pass "build target environment: refused before build/dispatch"
  else
    fail "build target environment: reached build/dispatch or returned ${status}"
  fi
}

# These are xtask arguments, not wrapper build options. Compare NUL-delimited
# records so empty arguments, newlines and metacharacters cannot disappear or
# be confused with the human-readable bracket log.
scenario_exact_command_argv() {
  local dir status
  local args=(
    '' 'arg with space' 'unicodé-Δ' '*' '[?*]' 'semi;colon'
    '$(printf unexpected)' '`printf unexpected`' '$HOME' $'line\nbreak'
    '--release' '--features=' '--'
  )
  dir="$(make_scenario_dir)"
  register_scenario_dir "$dir"
  printf '%s\0' "${args[@]}" >"${dir}/expected-argv.nul"
  status=0
  run_wrapper "$dir" "${args[@]}" \
    >"${dir}/out.txt" 2>"${dir}/err.txt" || status=$?
  if [[ "$status" -eq 0 ]] && cmp -s "${dir}/expected-argv.nul" "${dir}/argv.log.nul"; then
    pass "command argv: empty arguments and metacharacters pass through byte-for-byte"
  else
    fail "command argv: arguments changed or wrapper returned ${status}"
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

scenario_success_path
scenario_failure_path
scenario_build_flags_passthrough
scenario_xtask_exit_status
scenario_missing_resources_does_not_exec
scenario_argv_spaces_and_unicode
scenario_supported_build_flags
scenario_unsupported_build_flags
scenario_build_target_environment
scenario_exact_command_argv
scenario_source_pins_admitted_producer

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
