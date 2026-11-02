#!/usr/bin/env bash
# Run: bash scripts/tests/test-run-cargo-deny-check.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ASSERT="${REPO_ROOT}/scripts/ci/assert-no-cargo-deny-unmatched-skip.sh"
RUN="${REPO_ROOT}/scripts/ci/run-cargo-deny-check.sh"
WORK="$(mktemp -d)"
FAKEBIN="$WORK/bin"
trap 'rm -rf "$WORK"' EXIT

require_eq() {
  local label=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    printf 'FAIL %s: expected exit %s got %s\n' "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

mkdir -p "$FAKEBIN"
cat >"$FAKEBIN/cargo-deny" <<'EOF'
#!/usr/bin/env bash
mode="${FAKE_DENY_MODE:-clean}"
if [ "${1:-}" = "--version" ]; then
  echo "cargo-deny 0.20.2 (fake)"
  exit 0
fi
case "$mode" in
  clean)
    printf 'bans ok\n' >&2
    exit 0
    ;;
  unmatched)
    printf 'warning[unmatched-skip]: skipped crate was not encountered\n' >&2
    printf 'bans ok\n' >&2
    exit 0
    ;;
  deny-fail)
    printf 'error: advisory failed\n' >&2
    exit 1
    ;;
  *)
    printf 'unknown FAKE_DENY_MODE=%s\n' "$mode" >&2
    exit 2
    ;;
esac
EOF
chmod +x "$FAKEBIN/cargo-deny"

printf 'warning[unmatched-skip]: example\n' >"$WORK/unmatched.stderr"
printf 'bans ok\n' >"$WORK/clean.stderr"

if bash "$ASSERT" "$WORK/clean.stderr"; then
  :
else
  printf 'FAIL assert should accept clean stderr\n' >&2
  exit 1
fi

if bash "$ASSERT" "$WORK/unmatched.stderr"; then
  printf 'FAIL assert must reject unmatched-skip stderr\n' >&2
  exit 1
fi

set +e
FAKE_DENY_MODE=clean CARGO_DENY="$FAKEBIN/cargo-deny" bash "$RUN" --locked check bans >/dev/null
rc=$?
set -e
require_eq "run wrapper clean fake" 0 "$rc"

set +e
FAKE_DENY_MODE=unmatched CARGO_DENY="$FAKEBIN/cargo-deny" bash "$RUN" --locked check bans >/dev/null
rc=$?
set -e
require_eq "run wrapper unmatched fake" 1 "$rc"

set +e
FAKE_DENY_MODE=deny-fail CARGO_DENY="$FAKEBIN/cargo-deny" bash "$RUN" --locked check bans >/dev/null
rc=$?
set -e
require_eq "run wrapper preserves cargo-deny failure" 1 "$rc"

if command -v cargo-deny >/dev/null 2>&1; then
  stderr_log="$WORK/live.stderr"
  set +e
  cargo-deny --locked check advisories licenses bans sources 2>"$stderr_log" >/dev/null
  deny_rc=$?
  set -e
  set +e
  bash "$ASSERT" "$stderr_log"
  assert_rc=$?
  set -e
  if [ "$assert_rc" -ne 0 ] && [ "$deny_rc" -eq 0 ]; then
    printf 'FAIL live policy: cargo-deny exited 0 but stderr has unmatched-skip (deny.toml rot)\n' >&2
    exit 1
  fi
  if [ "$assert_rc" -ne 0 ] && [ "$deny_rc" -ne 0 ]; then
    printf 'SKIP live policy: cargo-deny failed for unrelated reasons (rc=%s)\n' "$deny_rc" >&2
  fi
fi

printf 'ok: run-cargo-deny-check\n'
