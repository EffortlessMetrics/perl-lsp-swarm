#!/usr/bin/env bash
set -euo pipefail

# Fixture proof for scripts/fuzz-bounded target discovery (#15059) and for
# nightly fuzz-matrix ↔ fuzz/Cargo.toml [[bin]] parity (#17135).
#
# Discovery cases stub cargo. python3 is a PATH wrapper around the real
# interpreter so the script's JSON extractor still runs. LF vs CRLF is applied
# to that extractor's stdout. Fuzz binaries are never compiled.
#
# Matrix parity reads the real workflow and manifest through the real
# interpreter (not the PATH stub) and does not invoke cargo.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMPL="${REPO_ROOT}/scripts/fuzz-bounded"

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

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$label"
  else
    fail "$label (expected ${expected@Q}, got ${actual@Q})"
  fi
}

assert_contains() {
  local label="$1" needle="$2" haystack_file="$3"
  if grep -Fq -- "$needle" "$haystack_file"; then
    pass "$label"
  else
    fail "$label (missing: ${needle@Q})"
  fi
}

assert_not_contains() {
  local label="$1" needle="$2" haystack_file="$3"
  if grep -Fq -- "$needle" "$haystack_file"; then
    fail "$label (unexpectedly found: ${needle@Q})"
  else
    pass "$label"
  fi
}

assert_nonzero() {
  local label="$1" code="$2"
  if ((code == 0)); then
    fail "$label (got 0)"
  else
    pass "$label (got ${code})"
  fi
}

assert_file_has_no_cr() {
  local label="$1" path="$2"
  if grep -q $'\r' "$path"; then
    fail "$label (CR present)"
  else
    pass "$label"
  fi
}

echo "=== fuzz-bounded discovery fixture suite ==="
echo ""

if [[ ! -f "$IMPL" ]]; then
  echo "ERROR: fuzz-bounded not found at ${IMPL}"
  exit 1
fi

if bash -n "$IMPL"; then
  pass "bash -n scripts/fuzz-bounded"
else
  fail "bash -n scripts/fuzz-bounded"
fi

REAL_PYTHON3="$(command -v python3 || true)"
if [[ -z "$REAL_PYTHON3" ]]; then
  echo "ERROR: real python3 is required to exercise the script extractor"
  exit 1
fi
export REAL_PYTHON3

TMPDIR_BASE="$(mktemp -d)"
STUB_BIN="${TMPDIR_BASE}/bin"
mkdir -p "$STUB_BIN"
CARGO_INVOCATION_LOG="${TMPDIR_BASE}/cargo.log"
export CARGO_INVOCATION_LOG

# Two bin targets plus a lib decoy. The production extractor keeps bins only
# and sorts names, so list/all-target order is other_target then parser_integration.
BIN_METADATA='{"packages":[{"name":"fuzz-pkg","targets":[{"name":"parser_integration","kind":["bin"]},{"name":"other_target","kind":["bin"]},{"name":"not_a_bin","kind":["lib"]}]}]}'
EMPTY_METADATA='{"packages":[{"name":"fuzz-pkg","targets":[{"name":"not_a_bin","kind":["lib"]}]}]}'
export BIN_METADATA EMPTY_METADATA

cat > "${STUB_BIN}/cargo" <<'STUB_CARGO'
#!/usr/bin/env bash
set -euo pipefail
: "${CARGO_INVOCATION_LOG:?}"

{
  printf 'CARGO_ARGV'
  printf ' %q' "$@"
  printf '\n'
} >> "$CARGO_INVOCATION_LOG"

if [[ "${1:-}" == "--version" ]]; then
  printf 'cargo 1.95.0 (stub 000000000 2026-01-01)\n'
  exit 0
fi

if [[ "${1:-}" == +* ]]; then
  shift
fi

if [[ "${1:-}" == "metadata" ]]; then
  if [[ "${CARGO_METADATA_EXIT:-0}" != 0 ]]; then
    printf 'error: stub cargo metadata failed\n' >&2
    exit "${CARGO_METADATA_EXIT}"
  fi
  if [[ "${CARGO_METADATA_KIND:-bins}" == "empty" ]]; then
    printf '%s\n' "${EMPTY_METADATA}"
  else
    printf '%s\n' "${BIN_METADATA}"
  fi
  exit 0
fi

if [[ "${1:-}" == "fuzz" && "${2:-}" == "run" ]]; then
  target="${3:-}"
  printf 'FUZZ_RUN_TARGET=%s\n' "$target" >> "$CARGO_INVOCATION_LOG"
  printf 'FUZZ_RUN_TARGET_HEX=' >> "$CARGO_INVOCATION_LOG"
  printf '%s' "$target" | od -An -tx1 | tr -d ' \n' >> "$CARGO_INVOCATION_LOG"
  printf '\n' >> "$CARGO_INVOCATION_LOG"
  exit "${CARGO_FUZZ_EXIT:-0}"
fi

printf 'unexpected cargo invocation:' >&2
printf ' %q' "$@" >&2
printf '\n' >&2
exit 99
STUB_CARGO
chmod +x "${STUB_BIN}/cargo"

cat > "${STUB_BIN}/python3" <<'STUB_PYTHON'
#!/usr/bin/env bash
set -euo pipefail
: "${REAL_PYTHON3:?}"

exit_code="${FUZZ_BOUNDED_PYTHON_EXIT:-0}"
if ((exit_code != 0)); then
  printf 'stub python failing\n' >&2
  exit "$exit_code"
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
set +e
"$REAL_PYTHON3" "$@" >"$tmp"
py_code=$?
set -e
if ((py_code != 0)); then
  cat "$tmp"
  exit "$py_code"
fi

mode="${FUZZ_BOUNDED_PYTHON_MODE:-lf}"
case "$mode" in
  lf)
    cat "$tmp"
    ;;
  crlf)
    sed 's/$/\r/' "$tmp"
    ;;
  crlf-blank)
    printf '\r\n'
    sed 's/$/\r/' "$tmp"
    printf '\r\n'
    ;;
  drop)
    ;;
  *)
    printf 'unknown stub python mode: %s\n' "$mode" >&2
    exit 99
    ;;
esac
STUB_PYTHON
chmod +x "${STUB_BIN}/python3"

run_fuzz_bounded() {
  local out_file="$1"
  local err_file="$2"
  shift 2
  : > "$CARGO_INVOCATION_LOG"
  set +e
  (
    cd "$REPO_ROOT"
    PATH="${STUB_BIN}:${PATH}"
    bash "$IMPL" "$@"
  ) >"$out_file" 2>"$err_file"
  RUN_CODE=$?
  set -e
}

hex_of() {
  printf '%s' "$1" | od -An -tx1 | tr -d ' \n'
}

PARSER_HEX="$(hex_of parser_integration)"
export FUZZ_BOUNDED_PYTHON_EXIT=0
export FUZZ_BOUNDED_PYTHON_MODE=lf
export CARGO_METADATA_EXIT=0
export CARGO_METADATA_KIND=bins

# ── LF discovery accepts a declared named target and forwards it unmodified ──

run_fuzz_bounded "${TMPDIR_BASE}/lf.out" "${TMPDIR_BASE}/lf.err" \
  --duration 30 -- parser_integration
assert_eq "LF named target exits 0" 0 "$RUN_CODE"
assert_contains "LF named target invokes cargo-fuzz" \
  "FUZZ_RUN_TARGET=parser_integration" "$CARGO_INVOCATION_LOG"
assert_contains "LF named target forwards unmodified bytes" \
  "FUZZ_RUN_TARGET_HEX=${PARSER_HEX}" "$CARGO_INVOCATION_LOG"
assert_not_contains "LF named target does not invoke undeclared names" \
  "FUZZ_RUN_TARGET=other_target" "$CARGO_INVOCATION_LOG"
assert_contains "LF named target prints success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/lf.out"
assert_contains "LF named target declares libFuzzer RSS ceiling" \
  "-rss_limit_mb=2048" "${TMPDIR_BASE}/lf.out"
assert_contains "LF named target forwards rss_limit_mb to libFuzzer" \
  "-rss_limit_mb=2048" "$CARGO_INVOCATION_LOG"

# ── CRLF discovery (Git Bash + native Windows Python) ─────────────────────────

export FUZZ_BOUNDED_PYTHON_MODE=crlf
run_fuzz_bounded "${TMPDIR_BASE}/crlf.out" "${TMPDIR_BASE}/crlf.err" \
  --duration 30 -- parser_integration
assert_eq "CRLF named target exits 0" 0 "$RUN_CODE"
assert_contains "CRLF named target invokes cargo-fuzz" \
  "FUZZ_RUN_TARGET=parser_integration" "$CARGO_INVOCATION_LOG"
assert_contains "CRLF named target forwards unmodified bytes" \
  "FUZZ_RUN_TARGET_HEX=${PARSER_HEX}" "$CARGO_INVOCATION_LOG"
if grep '^FUZZ_RUN_TARGET_HEX=' "$CARGO_INVOCATION_LOG" | grep -q '0d'; then
  fail "CRLF named target must not pass a CR suffix to cargo"
else
  pass "CRLF named target does not pass a CR suffix to cargo"
fi
assert_not_contains "CRLF named target is not rejected as unknown" \
  "unknown fuzz target" "${TMPDIR_BASE}/crlf.err"
assert_contains "CRLF named target prints success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/crlf.out"

# ── CRLF list output is CR-free and keeps both declared names ────────────────

run_fuzz_bounded "${TMPDIR_BASE}/crlf-list.out" "${TMPDIR_BASE}/crlf-list.err" --list
assert_eq "CRLF --list exits 0" 0 "$RUN_CODE"
assert_contains "CRLF --list includes parser_integration" \
  "parser_integration" "${TMPDIR_BASE}/crlf-list.out"
assert_contains "CRLF --list includes other_target" \
  "other_target" "${TMPDIR_BASE}/crlf-list.out"
assert_file_has_no_cr "CRLF --list stdout has no CR" "${TMPDIR_BASE}/crlf-list.out"
assert_eq "CRLF --list prints sorted discovered names" \
  $'other_target\nparser_integration' \
  "$(tr -d '\r' < "${TMPDIR_BASE}/crlf-list.out")"
assert_not_contains "CRLF --list does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"

# ── CRLF all-target mode uses normalized discovered names ─────────────────────

run_fuzz_bounded "${TMPDIR_BASE}/crlf-all.out" "${TMPDIR_BASE}/crlf-all.err" --duration 1
assert_eq "CRLF all-target exits 0" 0 "$RUN_CODE"
assert_contains "CRLF all-target runs other_target" \
  "FUZZ_RUN_TARGET=other_target" "$CARGO_INVOCATION_LOG"
assert_contains "CRLF all-target runs parser_integration" \
  "FUZZ_RUN_TARGET=parser_integration" "$CARGO_INVOCATION_LOG"
if grep -q 'FUZZ_RUN_TARGET_HEX=.*0d' "$CARGO_INVOCATION_LOG"; then
  fail "CRLF all-target must not forward CR in cargo target names"
else
  pass "CRLF all-target forwards CR-free cargo target names"
fi

# ── Blank CRLF lines are not invented targets ─────────────────────────────────

export FUZZ_BOUNDED_PYTHON_MODE=crlf-blank
run_fuzz_bounded "${TMPDIR_BASE}/crlf-blank.out" "${TMPDIR_BASE}/crlf-blank.err" \
  --duration 1 -- parser_integration
assert_eq "CRLF blank lines still accept parser_integration" 0 "$RUN_CODE"
assert_contains "CRLF blank-line discovery invokes parser_integration" \
  "FUZZ_RUN_TARGET=parser_integration" "$CARGO_INVOCATION_LOG"
blank_extra="$(grep '^FUZZ_RUN_TARGET=' "$CARGO_INVOCATION_LOG" | grep -v 'FUZZ_RUN_TARGET=parser_integration' || true)"
if [[ -n "$blank_extra" ]]; then
  fail "CRLF blank-line discovery does not invent an empty target (${blank_extra})"
else
  pass "CRLF blank-line discovery does not invent an empty target"
fi

# ── Genuinely unknown target fails before fuzz execution ─────────────────────

export FUZZ_BOUNDED_PYTHON_MODE=lf
run_fuzz_bounded "${TMPDIR_BASE}/unknown.out" "${TMPDIR_BASE}/unknown.err" \
  --duration 30 -- not_a_real_target
assert_eq "unknown target exits 2" 2 "$RUN_CODE"
assert_contains "unknown target names the rejected argv" \
  "unknown fuzz target(s): not_a_real_target" "${TMPDIR_BASE}/unknown.err"
assert_contains "unknown target still lists known names" \
  "parser_integration" "${TMPDIR_BASE}/unknown.err"
assert_not_contains "unknown target does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"
assert_not_contains "unknown target does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/unknown.out"

# Opposite-direction: discovery is normalized; the Cargo argv is not rewritten.
run_fuzz_bounded "${TMPDIR_BASE}/argv-cr.out" "${TMPDIR_BASE}/argv-cr.err" \
  --duration 30 -- $'parser_integration\r'
assert_eq "CR-suffixed argv is not silently rewritten to a known target" 2 "$RUN_CODE"
assert_not_contains "CR-suffixed argv does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"

# Empty argv used to crash on `known[""]` (bad array subscript) before fuzzing.
run_fuzz_bounded "${TMPDIR_BASE}/empty-argv.out" "${TMPDIR_BASE}/empty-argv.err" \
  --duration 30 -- ""
assert_eq "empty argv exits 2" 2 "$RUN_CODE"
assert_contains "empty argv is a typed refusal" \
  "empty fuzz target name" "${TMPDIR_BASE}/empty-argv.err"
assert_not_contains "empty argv does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"
assert_not_contains "empty argv does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/empty-argv.out"
assert_not_contains "empty argv is not a bash subscript crash" \
  "bad array subscript" "${TMPDIR_BASE}/empty-argv.err"

# ── Failed metadata cannot report successful fuzzing ──────────────────────────
# All-target is the fail-open path: named targets can look like "unknown".

export CARGO_METADATA_EXIT=1
run_fuzz_bounded "${TMPDIR_BASE}/meta-fail.out" "${TMPDIR_BASE}/meta-fail.err" --duration 30
assert_nonzero "failed cargo metadata all-target exits non-zero" "$RUN_CODE"
assert_not_contains "failed cargo metadata does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"
assert_not_contains "failed cargo metadata does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/meta-fail.out"

export CARGO_METADATA_EXIT=0
export FUZZ_BOUNDED_PYTHON_EXIT=1
run_fuzz_bounded "${TMPDIR_BASE}/py-fail.out" "${TMPDIR_BASE}/py-fail.err" --duration 30
assert_nonzero "failed python metadata all-target exits non-zero" "$RUN_CODE"
assert_not_contains "failed python metadata does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"
assert_not_contains "failed python metadata does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/py-fail.out"

# ── Empty metadata cannot report successful fuzzing ───────────────────────────

export FUZZ_BOUNDED_PYTHON_EXIT=0
export FUZZ_BOUNDED_PYTHON_MODE=lf
export CARGO_METADATA_KIND=empty
run_fuzz_bounded "${TMPDIR_BASE}/empty.out" "${TMPDIR_BASE}/empty.err" --duration 30
assert_nonzero "empty bin list all-target exits non-zero" "$RUN_CODE"
assert_not_contains "empty bin list does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"
assert_not_contains "empty bin list does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/empty.out"

run_fuzz_bounded "${TMPDIR_BASE}/empty-list.out" "${TMPDIR_BASE}/empty-list.err" --list
assert_nonzero "empty bin list --list exits non-zero" "$RUN_CODE"

export CARGO_METADATA_KIND=bins
export FUZZ_BOUNDED_PYTHON_MODE=drop
run_fuzz_bounded "${TMPDIR_BASE}/drop.out" "${TMPDIR_BASE}/drop.err" --duration 30
assert_nonzero "dropped python stdout all-target exits non-zero" "$RUN_CODE"
assert_not_contains "dropped python stdout does not print success banner" \
  "Bounded fuzz testing complete" "${TMPDIR_BASE}/drop.out"

# ── LF --list and duration validation stay intact ─────────────────────────────

export FUZZ_BOUNDED_PYTHON_MODE=lf
run_fuzz_bounded "${TMPDIR_BASE}/lf-list.out" "${TMPDIR_BASE}/lf-list.err" --list
assert_eq "LF --list exits 0" 0 "$RUN_CODE"
assert_eq "LF --list prints sorted discovered names" \
  $'other_target\nparser_integration' \
  "$(cat "${TMPDIR_BASE}/lf-list.out")"

run_fuzz_bounded "${TMPDIR_BASE}/bad-duration.out" "${TMPDIR_BASE}/bad-duration.err" \
  --duration 0 -- parser_integration
assert_eq "duration 0 is rejected" 2 "$RUN_CODE"
assert_not_contains "invalid duration does not invoke fuzz run" \
  "FUZZ_RUN_TARGET=" "$CARGO_INVOCATION_LOG"

# ── Nightly matrix ↔ fuzz/Cargo.toml [[bin]] parity (#17135) ──────────────────
# Uses REAL_PYTHON3 so a leftover FUZZ_BOUNDED_PYTHON_MODE=drop stub cannot
# swallow the comparison. The discovery self-test job executes this script
# when this test or either authority file changes. Parse [[bin]] names by
# scanning tables so Python 3.10 (no tomllib) still runs the gate.

compare_fuzz_matrix() {
  local nightly="$1"
  local cargo="$2"
  "$REAL_PYTHON3" - "$nightly" "$cargo" <<'PY'
import re
import sys
from pathlib import Path


class ParseError(Exception):
    pass


def block_after(text, header_pattern, sibling_pattern):
    lines = text.splitlines(keepends=True)
    start = None
    for index, line in enumerate(lines):
        if re.match(header_pattern, line):
            start = index
            break
    if start is None:
        return ""
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if re.match(sibling_pattern, lines[index]):
            end = index
            break
    return "".join(lines[start:end])


def fuzz_job_text(workflow):
    job = block_after(workflow, r"^  fuzz:\s*(#.*)?$", r"^  [A-Za-z0-9_-]+:")
    if not job:
        raise ParseError("workflow has no jobs.fuzz")
    return job


def parse_flow_names(body):
    names = []
    for raw in body.split(","):
        token = raw.split("#", 1)[0].strip().strip("'\"")
        if token:
            names.append(token)
    return names


def parse_block_names(body):
    names = []
    for line in body.splitlines():
        match = re.match(r"^\s+-\s+(\S+)", line)
        if match is None:
            continue
        token = match.group(1).split("#", 1)[0].strip().strip("'\"").rstrip(",")
        if token:
            names.append(token)
    return names


def matrix_targets(job):
    strategy = block_after(job, r"^    strategy:\s*(#.*)?$", r"^    \S")
    if not strategy:
        raise ParseError("jobs.fuzz.strategy not found")
    matrix = block_after(strategy, r"^      matrix:\s*(#.*)?$", r"^      \S")
    if not matrix:
        raise ParseError("jobs.fuzz.strategy.matrix not found")
    flow = re.search(r"(?m)^\s+target:\s*\[(.*?)\]", matrix, re.S)
    if flow is not None:
        names = parse_flow_names(flow.group(1))
    else:
        block = re.search(r"(?m)^\s+target:\s*\n((?:\s+-\s+\S.*\n?)*)", matrix)
        if block is None:
            raise ParseError("jobs.fuzz.strategy.matrix.target not found")
        names = parse_block_names(block.group(1))
    if not names:
        raise ParseError("jobs.fuzz.strategy.matrix.target is empty")
    return names


def manifest_bin_targets(path):
    text = path.read_text(encoding="utf-8")
    names = []
    in_bin = False
    for raw in text.splitlines():
        stripped = raw.strip()
        if stripped.startswith("["):
            in_bin = stripped == "[[bin]]"
            continue
        if not in_bin:
            continue
        match = re.match(r"^name\s*=\s*(['\"])([^'\"]+)\1", stripped)
        if match:
            names.append(match.group(2))
    if not names:
        raise ParseError("%s has no [[bin]] name entries" % (path,))
    return names


def duplicates(names):
    seen = set()
    dupes = []
    for name in names:
        if name in seen and name not in dupes:
            dupes.append(name)
        seen.add(name)
    return dupes


def main() -> int:
    if len(sys.argv) != 3:
        raise ParseError("usage: compare NIGHTLY.yml CARGO.toml")
    nightly_path = Path(sys.argv[1])
    cargo_path = Path(sys.argv[2])
    job = fuzz_job_text(nightly_path.read_text(encoding="utf-8"))
    matrix = matrix_targets(job)
    manifest = manifest_bin_targets(cargo_path)
    matrix_dupes = duplicates(matrix)
    manifest_dupes = duplicates(manifest)
    if matrix_dupes or manifest_dupes:
        print("PARSE_ERROR: duplicate target names")
        if matrix_dupes:
            print("duplicates_matrix: " + ",".join(matrix_dupes))
        if manifest_dupes:
            print("duplicates_manifest: " + ",".join(manifest_dupes))
        return 2
    matrix_set = set(matrix)
    manifest_set = set(manifest)
    if matrix_set == manifest_set:
        print("EQUAL")
        print("count=" + str(len(matrix_set)))
        print("names=" + ",".join(sorted(matrix_set)))
        return 0
    print("MISMATCH")
    print("count_matrix=" + str(len(matrix_set)))
    print("count_manifest=" + str(len(manifest_set)))
    only_matrix = ",".join(sorted(matrix_set - manifest_set))
    only_manifest = ",".join(sorted(manifest_set - matrix_set))
    print("only_in_matrix:" + ((" " + only_matrix) if only_matrix else ""))
    print("only_in_manifest:" + ((" " + only_manifest) if only_manifest else ""))
    return 1


try:
    sys.exit(main())
except ParseError as exc:
    print("PARSE_ERROR: %s" % (exc,))
    sys.exit(2)
except (OSError, UnicodeError) as exc:
    print("PARSE_ERROR: %s" % (exc,))
    sys.exit(2)
PY
}

run_compare_fuzz_matrix() {
  local out_file="$1"
  local err_file="$2"
  local nightly="$3"
  local cargo="$4"
  set +e
  compare_fuzz_matrix "$nightly" "$cargo" >"$out_file" 2>"$err_file"
  COMPARE_CODE=$?
  set -e
}

NIGHTLY_YML="${REPO_ROOT}/.github/workflows/ci-nightly.yml"
FUZZ_CARGO="${REPO_ROOT}/fuzz/Cargo.toml"

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-live.out" "${TMPDIR_BASE}/parity-live.err" \
  "$NIGHTLY_YML" "$FUZZ_CARGO"
assert_eq "live nightly fuzz matrix equals fuzz/Cargo.toml [[bin]] names" \
  0 "$COMPARE_CODE"
assert_contains "live parity reports EQUAL" \
  "EQUAL" "${TMPDIR_BASE}/parity-live.out"
assert_contains "live parity reports a non-empty name set" \
  "count=" "${TMPDIR_BASE}/parity-live.out"

# Fixture workflow: a decoy job matrix must not be compared. The fuzz job is
# the only authority for nightly enrollment.
cat > "${TMPDIR_BASE}/fixture-nightly.yml" <<'YAML'
jobs:
  decoy:
    strategy:
      matrix:
        target: [decoy_only]
  fuzz:
    strategy:
      matrix:
        target: [
          alpha,
          beta
        ]
YAML

cat > "${TMPDIR_BASE}/fixture-cargo.toml" <<'TOML'
[[bin]]
name = "beta"
path = "fuzz_targets/beta.rs"

[[bin]]
name = "alpha"
path = "fuzz_targets/alpha.rs"
TOML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-decoy.out" "${TMPDIR_BASE}/parity-decoy.err" \
  "${TMPDIR_BASE}/fixture-nightly.yml" "${TMPDIR_BASE}/fixture-cargo.toml"
assert_eq "decoy job matrix is ignored when fuzz job matches [[bin]]" \
  0 "$COMPARE_CODE"
assert_contains "decoy fixture reports EQUAL" \
  "EQUAL" "${TMPDIR_BASE}/parity-decoy.out"
assert_not_contains "decoy fixture does not enroll decoy_only" \
  "decoy_only" "${TMPDIR_BASE}/parity-decoy.out"

# A same-job `target:` list outside strategy.matrix must not win.
cat > "${TMPDIR_BASE}/fixture-nightly-same-job.yml" <<'YAML'
jobs:
  fuzz:
    name: Fuzz Target
    extra:
      target: [same_job_decoy]
    strategy:
      fail-fast: false
      matrix:
        target: [
          alpha,
          beta
        ]
YAML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-same-job.out" "${TMPDIR_BASE}/parity-same-job.err" \
  "${TMPDIR_BASE}/fixture-nightly-same-job.yml" "${TMPDIR_BASE}/fixture-cargo.toml"
assert_eq "same-job extra target list is ignored" \
  0 "$COMPARE_CODE"
assert_contains "same-job fixture reports EQUAL" \
  "EQUAL" "${TMPDIR_BASE}/parity-same-job.out"
assert_not_contains "same-job fixture does not enroll same_job_decoy" \
  "same_job_decoy" "${TMPDIR_BASE}/parity-same-job.out"

# Block-style matrix lists are accepted so a later YAML reformat cannot
# silently disable the gate.
cat > "${TMPDIR_BASE}/fixture-nightly-block.yml" <<'YAML'
jobs:
  fuzz:
    strategy:
      matrix:
        target:
          - alpha
          - beta
YAML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-block.out" "${TMPDIR_BASE}/parity-block.err" \
  "${TMPDIR_BASE}/fixture-nightly-block.yml" "${TMPDIR_BASE}/fixture-cargo.toml"
assert_eq "block-style fuzz matrix equals [[bin]] names" \
  0 "$COMPARE_CODE"
assert_contains "block-style fixture reports EQUAL" \
  "EQUAL" "${TMPDIR_BASE}/parity-block.out"

# Matrix-only name: the silent nightly miss in reverse (name stays enrolled
# after it is removed from the manifest).
cat > "${TMPDIR_BASE}/fixture-cargo-missing.toml" <<'TOML'
[[bin]]
name = "alpha"
path = "fuzz_targets/alpha.rs"
TOML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-matrix-extra.out" "${TMPDIR_BASE}/parity-matrix-extra.err" \
  "${TMPDIR_BASE}/fixture-nightly.yml" "${TMPDIR_BASE}/fixture-cargo-missing.toml"
assert_eq "matrix name absent from [[bin]] is a mismatch" \
  1 "$COMPARE_CODE"
assert_contains "matrix-extra mismatch names the leftover matrix target" \
  "only_in_matrix: beta" "${TMPDIR_BASE}/parity-matrix-extra.out"

# Manifest-only name: adding a [[bin]] without updating the nightly matrix.
cat > "${TMPDIR_BASE}/fixture-cargo-extra.toml" <<'TOML'
[[bin]]
name = "alpha"
path = "fuzz_targets/alpha.rs"

[[bin]]
name = "beta"
path = "fuzz_targets/beta.rs"

[[bin]]
name = "gamma"
path = "fuzz_targets/gamma.rs"
TOML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-manifest-extra.out" "${TMPDIR_BASE}/parity-manifest-extra.err" \
  "${TMPDIR_BASE}/fixture-nightly.yml" "${TMPDIR_BASE}/fixture-cargo-extra.toml"
assert_eq "[[bin]] name absent from the nightly matrix is a mismatch" \
  1 "$COMPARE_CODE"
assert_contains "manifest-extra mismatch names the missing matrix target" \
  "only_in_manifest: gamma" "${TMPDIR_BASE}/parity-manifest-extra.out"

# Missing fuzz job cannot pass: an extractor that sees no matrix must not EQUAL.
cat > "${TMPDIR_BASE}/fixture-no-fuzz.yml" <<'YAML'
jobs:
  decoy:
    strategy:
      matrix:
        target: [alpha, beta]
YAML

run_compare_fuzz_matrix \
  "${TMPDIR_BASE}/parity-no-fuzz.out" "${TMPDIR_BASE}/parity-no-fuzz.err" \
  "${TMPDIR_BASE}/fixture-no-fuzz.yml" "${TMPDIR_BASE}/fixture-cargo.toml"
assert_eq "workflow without jobs.fuzz is a parse error, not EQUAL" \
  2 "$COMPARE_CODE"
assert_contains "missing fuzz job is reported as PARSE_ERROR" \
  "PARSE_ERROR:" "${TMPDIR_BASE}/parity-no-fuzz.out"
assert_not_contains "missing fuzz job does not report EQUAL" \
  "EQUAL" "${TMPDIR_BASE}/parity-no-fuzz.out"

echo ""
echo "=== ${PASS} passed, ${FAIL} failed ==="
if ((FAIL > 0)); then
  exit 1
fi
exit 0
