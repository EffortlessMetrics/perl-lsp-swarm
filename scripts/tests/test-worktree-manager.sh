#!/usr/bin/env bash
# Test suite for scripts/worktree-manager.py — issue #3749 regression tests
# and issue #5444 hardening tests.
#
# Original cases (#3749 — allocate branch-freshness):
#   Case 1: re-allocating a slot for a branch that exists on origin checks out
#           that branch's content, not local/base main.
#   Case 2: allocating a genuinely new branch cuts from freshly-fetched
#           origin/main, not a stale local tracking ref.
#   Case 3: a fetch failure for an existing-on-origin branch fails closed;
#           no worktree is created and the error message cites #3749.
#
# Additional cases (#5444 — root, owner, concurrency, atomicity):
#   Case 4: invocation from a linked worktree resolves the same primary
#           repository root as the main checkout.
#   Case 5: a recorded owner rejects missing or different owners unless
#           --force is explicit; the correct owner succeeds.
#   Case 6: two concurrent state mutations retain both updates (serialization).
#   Case 7: an injected write failure leaves the previous JSON readable
#           (atomic temp-file + rename write).
#   Case 8: the module imports cleanly on a platform without fcntl.
#   Case 9: Windows locking behavior — NOT_PROVEN on this platform; must be
#           exercised by Windows release-preparation CI.
#
# This suite is fully hermetic: it builds a throwaway bare "origin" repo plus
# a clone under a tmpdir, copies the worktree-manager script under test into
# that fixture tree (REPO_ROOT is derived from the script's own file location),
# and asserts against real git state. No network access, no interaction with
# the real repo's .ops-perl-lsp state.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
MANAGER_SRC="${REPO_ROOT}/scripts/worktree-manager.py"

PASS_COUNT=0
FAIL_COUNT=0
TMPDIR_BASE=""

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

cleanup() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
}
trap cleanup EXIT

if [[ ! -f "$MANAGER_SRC" ]]; then
  echo "ERROR: worktree-manager.py not found at ${MANAGER_SRC}"
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: python3 not found on PATH"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"

git_q() { git -c user.name="Fixture" -c user.email="fixture@example.com" "$@" >/dev/null 2>&1; }

echo "=== worktree-manager self-test (#3749 branch-freshness, #5444 hardening) ==="
echo ""

# ── Fixture setup ───────────────────────────────────────────────────────────
# origin.git: bare "remote" repo.
ORIGIN_BARE="${TMPDIR_BASE}/origin.git"
git -c init.defaultBranch=main init -q --bare "$ORIGIN_BARE"

# agent-one: the clone that will act as REPO_ROOT for the script under test.
AGENT_ONE="${TMPDIR_BASE}/agent-one"
git_q clone -q "$ORIGIN_BARE" "$AGENT_ONE"
(
  cd "$AGENT_ONE"
  git checkout -B main -q
  echo "init" > file.txt
  git -c user.name="Fixture" -c user.email="fixture@example.com" add file.txt
  git -c user.name="Fixture" -c user.email="fixture@example.com" commit -q -m "init"
  git push -q origin main
)
MAIN_A_SHA="$(git -C "$AGENT_ONE" rev-parse main)"

# From a throwaway third clone, push an already-existing remote branch with
# content that diverges from main — this is the branch a slot re-allocation
# will target. Using a separate clone (rather than switching agent-one's own
# branch back and forth) keeps agent-one's working branch untouched.
AGENT_THREE="${TMPDIR_BASE}/agent-three"
git_q clone -q "$ORIGIN_BARE" "$AGENT_THREE"
(
  cd "$AGENT_THREE"
  git checkout -b existing/feature -q
  echo "feature work" >> file.txt
  git -c user.name="Fixture" -c user.email="fixture@example.com" commit -q -am "feature commit"
  git push -q origin existing/feature
)
EXISTING_FEATURE_SHA="$(git -C "$ORIGIN_BARE" rev-parse refs/heads/existing/feature)"

# From a second, independent clone, advance origin/main further. agent-one's
# local `main` (MAIN_A_SHA) does NOT see this commit until it fetches —
# reproducing "stale local main" for the genuinely-new-branch case.
AGENT_TWO="${TMPDIR_BASE}/agent-two"
git_q clone -q "$ORIGIN_BARE" "$AGENT_TWO"
(
  cd "$AGENT_TWO"
  git checkout -B main -q
  echo "advanced by someone else" >> file.txt
  git -c user.name="Fixture" -c user.email="fixture@example.com" commit -q -am "advance main"
  git push -q origin main
)
ORIGIN_MAIN_TIP_SHA="$(git -C "$ORIGIN_BARE" rev-parse refs/heads/main)"

if [[ "$ORIGIN_MAIN_TIP_SHA" == "$MAIN_A_SHA" ]]; then
  echo "ERROR: fixture setup failed to advance origin/main past agent-one's local main"
  exit 1
fi

# A third existing-on-origin branch, used by Case 3 to prove that a fetch
# failure while resolving an existing-on-origin branch fails CLOSED — never
# silently checking out a stale/unverified ref.
AGENT_FOUR="${TMPDIR_BASE}/agent-four"
git_q clone -q "$ORIGIN_BARE" "$AGENT_FOUR"
(
  cd "$AGENT_FOUR"
  git checkout -b flaky/branch -q
  echo "flaky branch work" >> file.txt
  git -c user.name="Fixture" -c user.email="fixture@example.com" commit -q -am "flaky branch commit"
  git push -q origin flaky/branch
)
FLAKY_BRANCH_SHA="$(git -C "$ORIGIN_BARE" rev-parse refs/heads/flaky/branch)"

# Place the script under test where REPO_ROOT resolution expects it:
# `<repo>/scripts/worktree-manager.py` inside agent-one's checkout.
mkdir -p "${AGENT_ONE}/scripts"
cp "$MANAGER_SRC" "${AGENT_ONE}/scripts/worktree-manager.py"

STATE_FILE="${TMPDIR_BASE}/state.json"
MANAGED_ROOT="${TMPDIR_BASE}/managed-worktrees"

run_manager() {
  # --state-file/--managed-root must come after the subcommand: the script's
  # argparse defines them on both the top-level parser and each subparser
  # (via a shared `parents=[common]`), and the subparser's own defaults
  # silently overwrite values set at the top level if passed before the
  # subcommand name.
  local subcommand="$1"
  shift
  (
    cd "$AGENT_ONE"
    python3 scripts/worktree-manager.py "$subcommand" --state-file "$STATE_FILE" --managed-root "$MANAGED_ROOT" "$@"
  )
}

# ── Case 3: a fetch failure while resolving an existing-on-origin branch
#    must fail CLOSED — never silently check out a stale/unverified ref.
#    (Review follow-up on #3749: `ls-remote` confirming existence does not
#    guarantee the subsequent fetch actually landed the ref.)
#
# Reproduced by corrupting (deleting) the flaky/branch tip commit's loose
# object on the bare origin repo: `ls-remote` only reads ref metadata (still
# succeeds, still reports the SHA), but `fetch` needs the actual object and
# fails -- an OS-independent stand-in for a dropped connection mid-transfer,
# without relying on filesystem permissions or PATH-shimmed binaries (which
# don't reproduce reliably across platforms for a Python-spawned `git`).
#
# MUST run before Case 1/2: Case 2's genuinely-new-branch path does a plain
# `git fetch origin` (all branches, no refspec) in agent-one's own clone. If
# that ran first, agent-one would already have flaky/branch's object cached
# locally before we corrupt origin's copy, and the later "fetch" for Case 3
# would silently succeed from agent-one's own object store instead of
# hitting (and failing against) origin — masking the exact bug this case
# exists to catch. ───────────────────────────────────────────────────────
FLAKY_OBJ_PATH="${ORIGIN_BARE}/objects/${FLAKY_BRANCH_SHA:0:2}/${FLAKY_BRANCH_SHA:2}"
if [[ ! -f "$FLAKY_OBJ_PATH" ]]; then
  echo "ERROR: expected loose object for flaky/branch tip at ${FLAKY_OBJ_PATH} (got auto-packed?) — Case 3 fixture assumption broken"
  exit 1
fi
# Back up rather than permanently destroy: a later plain `git fetch origin`
# (Case 2's genuinely-new-branch fallback, which fetches ALL branches with no
# refspec) would otherwise keep failing on this same still-missing object for
# the rest of the suite, since git's default-refspec fetch can fail the whole
# operation if any one ref can't be resolved.
cp "$FLAKY_OBJ_PATH" "${FLAKY_OBJ_PATH}.bak"
rm -f "$FLAKY_OBJ_PATH"

CASE3_OUT=""
CASE3_EXIT=0
CASE3_OUT="$(run_manager allocate --slot slot-flaky --branch flaky/branch 2>&1)" || CASE3_EXIT=$?

mv "${FLAKY_OBJ_PATH}.bak" "$FLAKY_OBJ_PATH"

if [[ "$CASE3_EXIT" -eq 0 ]]; then
  fail "allocate fails closed on fetch failure: manager exited 0 (expected non-zero); out=$CASE3_OUT"
elif [[ -e "${MANAGED_ROOT}/slot-flaky" ]]; then
  fail "allocate fails closed on fetch failure: a worktree was created at slot-flaky despite the fetch failure — stale-checkout risk"
elif [[ "$CASE3_OUT" != *"3749"* ]]; then
  fail "allocate fails closed on fetch failure: exited non-zero as expected, but error message doesn't cite the #3749 fail-closed rationale — out=$CASE3_OUT"
else
  pass "allocate fails closed (no worktree created) when the origin fetch fails after ls-remote confirms the branch exists"
fi

# ── Case 1: re-allocating a slot for a branch that exists on origin must
#    check out THAT branch's content, not local/base main. ─────────────────
CASE1_OUT=""
CASE1_EXIT=0
CASE1_OUT="$(run_manager allocate --slot slot-existing --branch existing/feature 2>&1)" || CASE1_EXIT=$?

if [[ "$CASE1_EXIT" -ne 0 ]]; then
  fail "allocate for existing origin branch: manager exited non-zero ($CASE1_EXIT): $CASE1_OUT"
else
  SLOT1_HEAD="$(git -C "${MANAGED_ROOT}/slot-existing" rev-parse HEAD 2>/dev/null || echo "MISSING")"
  if [[ "$SLOT1_HEAD" == "$EXISTING_FEATURE_SHA" ]]; then
    pass "allocate for existing origin branch checks out origin/existing/feature content"
  elif [[ "$SLOT1_HEAD" == "$MAIN_A_SHA" ]]; then
    fail "allocate for existing origin branch: worktree HEAD == local main (${MAIN_A_SHA}), expected origin branch tip (${EXISTING_FEATURE_SHA}) — the #3749 bug"
  else
    fail "allocate for existing origin branch: unexpected worktree HEAD ${SLOT1_HEAD}, expected ${EXISTING_FEATURE_SHA}"
  fi
fi

# ── Case 2: allocating a genuinely NEW branch must cut from a freshly
#    fetched origin/main, not a stale local tracking ref. ─────────────────
CASE2_OUT=""
CASE2_EXIT=0
CASE2_OUT="$(run_manager allocate --slot slot-new --branch brand-new/feature 2>&1)" || CASE2_EXIT=$?

if [[ "$CASE2_EXIT" -ne 0 ]]; then
  fail "allocate for new branch: manager exited non-zero ($CASE2_EXIT): $CASE2_OUT"
else
  SLOT2_HEAD="$(git -C "${MANAGED_ROOT}/slot-new" rev-parse HEAD 2>/dev/null || echo "MISSING")"
  if [[ "$SLOT2_HEAD" == "$ORIGIN_MAIN_TIP_SHA" ]]; then
    pass "allocate for new branch cuts from freshly-fetched origin/main"
  elif [[ "$SLOT2_HEAD" == "$MAIN_A_SHA" ]]; then
    fail "allocate for new branch: worktree HEAD == stale local main (${MAIN_A_SHA}), expected current origin/main tip (${ORIGIN_MAIN_TIP_SHA}) — base_ref() never fetched"
  else
    fail "allocate for new branch: unexpected worktree HEAD ${SLOT2_HEAD}, expected ${ORIGIN_MAIN_TIP_SHA}"
  fi
fi

# ── Case 4: invocation from a linked worktree resolves the same primary
#    repository root as the main checkout (issue #5444 defect 1). ──────────
echo ""
echo "=== #5444 hardening cases ==="
echo ""

LINKED_WT="${TMPDIR_BASE}/linked-worktree"
LINKED_BRANCH="test/case4-linked-wt"
CASE4_EXIT=0

# Create a linked worktree from agent-one, copy the script into it, then
# compare the roots each invocation resolves.
git_q -C "${AGENT_ONE}" worktree add -b "${LINKED_BRANCH}" "${LINKED_WT}" || CASE4_EXIT=$?

if [[ "${CASE4_EXIT}" -ne 0 ]]; then
  fail "Case 4 setup: failed to create linked worktree"
else
  mkdir -p "${LINKED_WT}/scripts"
  cp "${AGENT_ONE}/scripts/worktree-manager.py" "${LINKED_WT}/scripts/worktree-manager.py"

  CASE4_SCRIPT="${TMPDIR_BASE}/case4_root_probe.py"
  cat > "${CASE4_SCRIPT}" << PYEOF
import sys, importlib.util, pathlib

script_path = sys.argv[1]
spec = importlib.util.spec_from_file_location('worktree_manager', script_path)
wm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wm)
print(str(wm._resolve_primary_repo_root()))
PYEOF

  # Root resolved from primary checkout
  PRIMARY_ROOT="$(cd "${AGENT_ONE}" && python3 "${CASE4_SCRIPT}" "${AGENT_ONE}/scripts/worktree-manager.py" 2>&1)"
  # Root resolved from linked worktree
  LINKED_ROOT="$(cd "${LINKED_WT}" && python3 "${CASE4_SCRIPT}" "${LINKED_WT}/scripts/worktree-manager.py" 2>&1)"

  if [[ "${PRIMARY_ROOT}" == "${LINKED_ROOT}" ]]; then
    pass "linked worktree invocation resolves same primary repository root as main checkout (${PRIMARY_ROOT})"
  else
    fail "linked worktree root: primary=${PRIMARY_ROOT} linked=${LINKED_ROOT} — linked invocation uses wrong root"
  fi

  # Clean up
  git_q -C "${AGENT_ONE}" worktree remove --force "${LINKED_WT}" 2>/dev/null || true
  git_q -C "${AGENT_ONE}" branch -D "${LINKED_BRANCH}" 2>/dev/null || true
fi

# ── Case 5: owner guard — ownerless and wrong-owner release both fail;
#    correct owner succeeds (issue #5444 defect 2). ────────────────────────
CASE5_STATE="${TMPDIR_BASE}/case5-state.json"
CASE5_MANAGED="${TMPDIR_BASE}/case5-worktrees"

run_manager5() {
  local subcommand="$1"
  shift
  (
    cd "$AGENT_ONE"
    python3 scripts/worktree-manager.py "$subcommand" \
      --state-file "${CASE5_STATE}" \
      --managed-root "${CASE5_MANAGED}" "$@"
  )
}

# Allocate with a recorded owner.
CASE5_SETUP_EXIT=0
run_manager5 allocate --slot case5-slot --branch test/case5-owned --owner alice \
  >/dev/null 2>&1 || CASE5_SETUP_EXIT=$?

if [[ "${CASE5_SETUP_EXIT}" -ne 0 ]]; then
  fail "Case 5 setup: allocate with --owner alice failed (exit ${CASE5_SETUP_EXIT})"
else
  # 5a: ownerless release must be rejected.
  CASE5A_EXIT=0
  run_manager5 release --slot case5-slot >/dev/null 2>&1 || CASE5A_EXIT=$?
  if [[ "${CASE5A_EXIT}" -ne 0 ]]; then
    pass "owner guard: ownerless release of an owned slot is rejected"
  else
    fail "owner guard: ownerless release of an owned slot succeeded — defect 2 not fixed"
  fi

  # 5b: wrong-owner release must be rejected.
  CASE5B_EXIT=0
  run_manager5 release --slot case5-slot --owner bob >/dev/null 2>&1 || CASE5B_EXIT=$?
  if [[ "${CASE5B_EXIT}" -ne 0 ]]; then
    pass "owner guard: wrong-owner release of an owned slot is rejected"
  else
    fail "owner guard: wrong-owner release succeeded — defect 2 not fixed"
  fi

  # 5c: correct owner release must succeed.
  CASE5C_EXIT=0
  CASE5C_OUT="$(run_manager5 release --slot case5-slot --owner alice 2>&1)" || CASE5C_EXIT=$?
  if [[ "${CASE5C_EXIT}" -eq 0 ]]; then
    pass "owner guard: correct-owner release succeeds"
  else
    fail "owner guard: correct-owner release failed: ${CASE5C_OUT}"
  fi
fi

# ── Case 6: concurrent state mutations retain both updates (issue #5444
#    defect 3 — serialization). ─────────────────────────────────────────────
#
# Two background allocations target different slots on the same state file.
# With the file lock they serialize; both must survive in the final state.
# Without the lock, one could overwrite the other's slot record.
CASE6_STATE="${TMPDIR_BASE}/case6-state.json"
CASE6_MANAGED="${TMPDIR_BASE}/case6-worktrees"

run_manager6() {
  local subcommand="$1"
  shift
  (
    cd "$AGENT_ONE"
    python3 scripts/worktree-manager.py "$subcommand" \
      --state-file "${CASE6_STATE}" \
      --managed-root "${CASE6_MANAGED}" "$@"
  )
}

(run_manager6 allocate --slot conc-slot-a --branch concurrent/alpha 2>/dev/null) &
PID_A=$!
(run_manager6 allocate --slot conc-slot-b --branch concurrent/beta 2>/dev/null) &
PID_B=$!

CASE6_EXIT_A=0; wait "${PID_A}" || CASE6_EXIT_A=$?
CASE6_EXIT_B=0; wait "${PID_B}" || CASE6_EXIT_B=$?

if [[ "${CASE6_EXIT_A}" -ne 0 || "${CASE6_EXIT_B}" -ne 0 ]]; then
  fail "concurrent mutations: one or both allocations failed (exit_a=${CASE6_EXIT_A} exit_b=${CASE6_EXIT_B})"
else
  CASE6_SLOT_COUNT="$(python3 -c "
import json, pathlib
state = json.loads(pathlib.Path('${CASE6_STATE}').read_text())
print(len(state.get('slots', [])))
" 2>/dev/null || echo "PARSE_ERROR")"
  if [[ "${CASE6_SLOT_COUNT}" == "2" ]]; then
    pass "concurrent mutations: both allocations retained in state (2 slots)"
  else
    fail "concurrent mutations: expected 2 slots, got '${CASE6_SLOT_COUNT:-0}' — possible state corruption"
  fi
fi

# ── Case 7: injected write failure leaves previous JSON readable (issue #5444
#    defect 3 — atomic temp-file + rename). ────────────────────────────────
#
# Patches pathlib.Path.replace to raise OSError at the rename step, simulating
# a process crash between write and rename.  With atomic write the original
# file is untouched; with the old direct-write approach it would be corrupted.
CASE7_STATE="${TMPDIR_BASE}/case7-state.json"
CASE7_SCRIPT="${TMPDIR_BASE}/case7_atomic_write.py"

cat > "${CASE7_SCRIPT}" << PYEOF
import sys, json, pathlib, importlib.util, os, contextlib

spec = importlib.util.spec_from_file_location(
    'worktree_manager',
    '${AGENT_ONE}/scripts/worktree-manager.py',
)
wm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wm)

state_file = pathlib.Path('${CASE7_STATE}')
initial = {'version': 1, 'slots': [], 'updated_at': None, 'managed_root': '.'}
state_file.parent.mkdir(parents=True, exist_ok=True)
state_file.write_text(json.dumps(initial) + '\n', encoding='utf-8')
initial_text = state_file.read_text(encoding='utf-8')

# Patch the atomic rename step to raise, simulating a crash mid-rename.
_original_replace = pathlib.Path.replace
patched_once = False

def _failing_replace(self, target):
    global patched_once
    if not patched_once and str(target) == str(state_file):
        patched_once = True
        # Clean up the temp file that was written (save_json suppresses OSError
        # on unlink when the rename fails, so the .tmp is removed cleanly).
        raise OSError('simulated rename failure (crash mid-write)')
    return _original_replace(self, target)

pathlib.Path.replace = _failing_replace

try:
    wm.save_json(state_file, {'version': 1, 'slots': [{'id': 'NEW'}], 'updated_at': None})
except OSError:
    pass  # expected

pathlib.Path.replace = _original_replace

# The original file must still contain valid JSON identical to the initial content.
recovered_text = state_file.read_text(encoding='utf-8')
try:
    recovered = json.loads(recovered_text)
except json.JSONDecodeError as e:
    print(f'FAIL: state file corrupted after simulated write failure: {e}')
    sys.exit(1)

if recovered_text != initial_text:
    print(f'FAIL: state file changed despite simulated failure; got: {recovered_text[:120]}')
    sys.exit(1)

print('PASS: state file unchanged after simulated write failure (atomic rename protects original)')
sys.exit(0)
PYEOF

CASE7_EXIT=0
CASE7_OUT="$(python3 "${CASE7_SCRIPT}" 2>&1)" || CASE7_EXIT=$?

if [[ "${CASE7_EXIT}" -eq 0 && "${CASE7_OUT}" == *"PASS"* ]]; then
  pass "injected write failure leaves previous JSON readable"
else
  fail "atomic write test: ${CASE7_OUT}"
fi

# ── Case 8: module imports cleanly without fcntl (issue #5444 defect 3 —
#    no Unix-only import at module scope). ────────────────────────────────
#
# Simulates a Windows-like environment where fcntl is absent by setting
# sys.modules['fcntl'] = None before importing the module.  Import must
# succeed without ImportError; _make_lock falls back to msvcrt then _NoopLock.
CASE8_SCRIPT="${TMPDIR_BASE}/case8_no_fcntl.py"

cat > "${CASE8_SCRIPT}" << PYEOF
import sys, importlib.util

# Poison the fcntl import (simulates a platform without it).
sys.modules['fcntl'] = None  # type: ignore[assignment]

spec = importlib.util.spec_from_file_location(
    'worktree_manager_no_fcntl',
    '${AGENT_ONE}/scripts/worktree-manager.py',
)
wm = importlib.util.module_from_spec(spec)
try:
    spec.loader.exec_module(wm)
except ImportError as exc:
    print(f'FAIL: module raised ImportError (fcntl imported at module scope): {exc}')
    sys.exit(1)
except Exception as exc:
    print(f'FAIL: unexpected import error: {type(exc).__name__}: {exc}')
    sys.exit(1)

# Verify _make_lock falls back gracefully without fcntl.
import io
dummy_fh = open('/dev/null', 'a+b')
lock = wm._make_lock(dummy_fh)
dummy_fh.close()
if isinstance(lock, (wm._MsvcrtLock, wm._NoopLock)):
    print('PASS: module imports cleanly without fcntl; _make_lock uses fallback backend')
    sys.exit(0)
else:
    print(f'FAIL: expected _MsvcrtLock or _NoopLock fallback, got {type(lock).__name__}')
    sys.exit(1)
PYEOF

CASE8_EXIT=0
CASE8_OUT="$(python3 "${CASE8_SCRIPT}" 2>&1)" || CASE8_EXIT=$?

if [[ "${CASE8_EXIT}" -eq 0 && "${CASE8_OUT}" == *"PASS"* ]]; then
  pass "module imports cleanly on a platform without fcntl; _make_lock falls back gracefully"
else
  fail "fcntl-absent import: ${CASE8_OUT}"
fi

# ── Case 9: Windows locking ─────────────────────────────────────────────────
# NOT_PROVEN: msvcrt.locking behavior must be exercised by Windows
# release-preparation CI.  This runner is Linux; no attempt is made here.
echo "NOT_PROVEN: Case 9 — Windows msvcrt locking (requires Windows CI lane)"

# ── Summary ──────────────────────────────────────────────────────────────────
TOTAL=$((PASS_COUNT + FAIL_COUNT))
echo ""
echo "=== Results: ${PASS_COUNT}/${TOTAL} passed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi

exit 0
