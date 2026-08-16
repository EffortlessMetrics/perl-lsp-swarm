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
#   Case 5: a recorded owner rejects missing or different owners; the correct
#           owner succeeds; an explicit --force bypasses the guard; and
#           --dry-run predicts the real outcome for both owners rather than
#           reporting "would release" for a slot the guard would reject.
#   Case 6: state mutations serialize on the lock file — proven by holding
#           that lock externally and asserting the manager blocks, fails at
#           its bound, and proceeds once released; plus an end-to-end check
#           that two concurrent allocations both survive.
#   Case 7: an injected write failure leaves the previous JSON readable
#           (atomic temp-file + rename write).
#   Case 8: the module imports cleanly on a platform without fcntl, and
#           _make_lock reports the absence of any backend as None rather than
#           substituting a silent no-op.
#   Case 8b/8c: with neither fcntl nor msvcrt, state mutation fails closed by
#           default and proceeds only under WORKTREE_MANAGER_ALLOW_UNLOCKED.
#   Case 9: Windows locking behavior — NOT_PROVEN on this platform; must be
#           exercised by Windows release-preparation CI.  The POSIX backend is
#           proven by Case 6; the accepted claim is narrowed accordingly.
#   Case 10: the atomic write preserves the destination file's permission
#           mode instead of tightening it to mkstemp's 0600.
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

  # Assert against the canonical primary checkout, not merely that the two
  # invocations agree: an implementation returning one consistently WRONG
  # shared path would satisfy a self-comparison.
  CANONICAL_ROOT="$(cd "${AGENT_ONE}" && pwd -P)"

  if [[ "${PRIMARY_ROOT}" == "${CANONICAL_ROOT}" && "${LINKED_ROOT}" == "${CANONICAL_ROOT}" ]]; then
    pass "linked worktree invocation resolves the canonical primary repository root (${CANONICAL_ROOT})"
  else
    fail "linked worktree root: expected=${CANONICAL_ROOT} primary=${PRIMARY_ROOT} linked=${LINKED_ROOT}"
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

  # 5d: --force must actually bypass the guard.  Without this case the guard
  # could reject or ignore --force entirely and 5a-5c would still all pass.
  CASE5D_SETUP_EXIT=0
  run_manager5 allocate --slot case5-forced --branch test/case5-forced --owner alice \
    >/dev/null 2>&1 || CASE5D_SETUP_EXIT=$?
  if [[ "${CASE5D_SETUP_EXIT}" -ne 0 ]]; then
    fail "Case 5d setup: allocate case5-forced with --owner alice failed (exit ${CASE5D_SETUP_EXIT})"
  else
    CASE5D_EXIT=0
    CASE5D_OUT="$(run_manager5 release --slot case5-forced --owner bob --force 2>&1)" || CASE5D_EXIT=$?
    if [[ "${CASE5D_EXIT}" -eq 0 ]]; then
      pass "owner guard: wrong-owner release with --force is permitted"
    else
      fail "owner guard: --force did not bypass the owner check: ${CASE5D_OUT}"
    fi
  fi

  # 5e: --dry-run must predict the *real* outcome.  If the guard runs after
  # the dry-run return, --dry-run prints "would release" for another owner's
  # slot while the real command raises — wrong in exactly the case the guard
  # protects.  Callers use --dry-run to decide whether to release.
  CASE5E_SETUP_EXIT=0
  run_manager5 allocate --slot case5-dryrun --branch test/case5-dryrun --owner alice \
    >/dev/null 2>&1 || CASE5E_SETUP_EXIT=$?
  if [[ "${CASE5E_SETUP_EXIT}" -ne 0 ]]; then
    fail "Case 5e setup: allocate case5-dryrun with --owner alice failed (exit ${CASE5E_SETUP_EXIT})"
  else
    CASE5E_EXIT=0
    CASE5E_OUT="$(run_manager5 release --slot case5-dryrun --owner bob --dry-run 2>&1)" \
      || CASE5E_EXIT=$?
    if [[ "${CASE5E_EXIT}" -ne 0 && "${CASE5E_OUT}" != *"would release"* ]]; then
      pass "owner guard: --dry-run release predicts the real wrong-owner rejection"
    else
      fail "owner guard: --dry-run predicted success for a wrong-owner release: ${CASE5E_OUT}"
    fi

    # And it must still predict success for the owner who may actually
    # release, without mutating the slot.
    CASE5F_EXIT=0
    CASE5F_OUT="$(run_manager5 release --slot case5-dryrun --owner alice --dry-run 2>&1)" \
      || CASE5F_EXIT=$?
    CASE5F_STATUS="$(python3 -c "
import json, sys
state = json.load(open('${CASE5_STATE}'))
slot = next(s for s in state['slots'] if s.get('slot_id') == 'case5-dryrun')
print(slot.get('status', ''))
" 2>/dev/null || echo "unreadable")"
    if [[ "${CASE5F_EXIT}" -eq 0 && "${CASE5F_OUT}" == *"would release"* \
          && "${CASE5F_STATUS}" != "idle" && "${CASE5F_STATUS}" != "retired" ]]; then
      pass "owner guard: --dry-run release for the correct owner predicts success without mutating"
    else
      fail "owner guard: correct-owner --dry-run wrong (exit ${CASE5F_EXIT}, status '${CASE5F_STATUS}'): ${CASE5F_OUT}"
    fi
  fi
fi

# ── Case 6: state mutations serialize on the lock file (issue #5444
#    defect 3 — serialization). ─────────────────────────────────────────────
#
# Racing two background allocations does NOT prove serialization: the OS is
# free to schedule them sequentially, so an unlocked implementation passes
# that check too.  Instead we create *deterministic* contention — an external
# holder takes the very lock file the manager uses and keeps it — and assert
# the manager genuinely blocks on it, gives up at its bound, and proceeds once
# the holder lets go.
CASE6_STATE="${TMPDIR_BASE}/case6-state.json"
CASE6_MANAGED="${TMPDIR_BASE}/case6-worktrees"
CASE6_LOCK="${TMPDIR_BASE}/case6-state.lock"   # state_path.with_suffix(".lock")
CASE6_READY="${TMPDIR_BASE}/case6-holder-ready"
CASE6_RELEASE="${TMPDIR_BASE}/case6-holder-release"

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

# Seed the state file so `query` has something to read, and so the lock path
# the holder grabs is exactly the one the manager will open.
run_manager6 query >/dev/null 2>&1 || true

# External holder: takes an exclusive flock on the manager's lock file and
# holds it until told to let go.
CASE6_HOLDER_SCRIPT="${TMPDIR_BASE}/case6_hold_lock.py"
cat > "${CASE6_HOLDER_SCRIPT}" << PYEOF
import fcntl, pathlib, time

lock_path = pathlib.Path('${CASE6_LOCK}')
lock_path.touch(exist_ok=True)
with open(lock_path, 'r+b') as fh:
    fcntl.flock(fh, fcntl.LOCK_EX)
    pathlib.Path('${CASE6_READY}').write_text('held\n')
    deadline = time.monotonic() + 60
    while not pathlib.Path('${CASE6_RELEASE}').exists() and time.monotonic() < deadline:
        time.sleep(0.05)
    fcntl.flock(fh, fcntl.LOCK_UN)
PYEOF

rm -f "${CASE6_READY}" "${CASE6_RELEASE}"
python3 "${CASE6_HOLDER_SCRIPT}" &
CASE6_HOLDER_PID=$!

# Wait for the holder to actually own the lock before probing.
CASE6_WAITED=0
while [[ ! -f "${CASE6_READY}" && "${CASE6_WAITED}" -lt 100 ]]; do
  sleep 0.05
  CASE6_WAITED=$((CASE6_WAITED + 1))
done

if [[ ! -f "${CASE6_READY}" ]]; then
  fail "Case 6 setup: external lock holder never acquired ${CASE6_LOCK}"
  kill "${CASE6_HOLDER_PID}" 2>/dev/null || true
else
  # 6a: with the lock held, the manager must NOT proceed. It must wait and
  # then fail at its bound with an actionable message naming the lock file.
  # An implementation that skips locking would return 0 immediately here.
  CASE6A_EXIT=0
  CASE6A_OUT="$(WORKTREE_MANAGER_LOCK_TIMEOUT=1 run_manager6 query 2>&1)" || CASE6A_EXIT=$?
  if [[ "${CASE6A_EXIT}" -eq 0 ]]; then
    fail "serialization: manager completed while the state lock was held by another process — not serialized"
  elif [[ "${CASE6A_OUT}" == *"${CASE6_LOCK}"* ]]; then
    pass "serialization: manager blocks on a held state lock and fails at its bound naming the lock file"
  else
    fail "serialization: manager failed while lock was held, but without citing ${CASE6_LOCK}: ${CASE6A_OUT}"
  fi

  # 6b: once the holder releases, the same command must succeed — the bound
  # must not leave the manager permanently wedged.
  touch "${CASE6_RELEASE}"
  wait "${CASE6_HOLDER_PID}" 2>/dev/null || true

  CASE6B_EXIT=0
  CASE6B_OUT="$(run_manager6 query 2>&1)" || CASE6B_EXIT=$?
  if [[ "${CASE6B_EXIT}" -eq 0 ]]; then
    pass "serialization: manager proceeds once the state lock is released"
  else
    fail "serialization: manager still blocked after the lock was released: ${CASE6B_OUT}"
  fi
fi

# 6c: end-to-end outcome check — two concurrent allocations must both survive.
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
# succeed without ImportError; _make_lock then selects msvcrt, or reports
# None when this platform has no locking backend at all.
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

# _make_lock must report the *absence* of a backend as None rather than
# substituting a no-op that satisfies the type while dropping serialization.
# Use a temporary file rather than '/dev/null', which does not exist on the
# very platforms this case simulates.
import tempfile
dummy_fh = tempfile.TemporaryFile(mode='w+b')
lock = wm._make_lock(dummy_fh)
dummy_fh.close()
if isinstance(lock, wm._MsvcrtLock) or lock is None:
    print('PASS: module imports cleanly without fcntl; _make_lock reports msvcrt or None')
    sys.exit(0)
else:
    print(f'FAIL: expected _MsvcrtLock or None, got {type(lock).__name__}')
    sys.exit(1)
PYEOF

CASE8_EXIT=0
CASE8_OUT="$(python3 "${CASE8_SCRIPT}" 2>&1)" || CASE8_EXIT=$?

if [[ "${CASE8_EXIT}" -eq 0 && "${CASE8_OUT}" == *"PASS"* ]]; then
  pass "module imports cleanly on a platform without fcntl; _make_lock reports the backend honestly"
else
  fail "fcntl-absent import: ${CASE8_OUT}"
fi

# ── Case 8b/8c: with NO locking backend, state mutation fails closed by
#    default and proceeds only under the explicit opt-in.  This is the
#    control for the review finding that a claimed-portable lock must not
#    silently degrade to no lock on an unproven platform. ─────────────────
CASE8B_SCRIPT="${TMPDIR_BASE}/case8b_no_backend.py"

cat > "${CASE8B_SCRIPT}" << PYEOF
import os, sys, importlib.util, pathlib

# Poison BOTH backends: this platform can take no lock at all.
sys.modules['fcntl'] = None   # type: ignore[assignment]
sys.modules['msvcrt'] = None  # type: ignore[assignment]

spec = importlib.util.spec_from_file_location(
    'worktree_manager_no_backend',
    '${AGENT_ONE}/scripts/worktree-manager.py',
)
wm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wm)

state_path = pathlib.Path('${TMPDIR_BASE}/case8b-state.json')

# 8b: default must refuse to mutate rather than run unserialized.
os.environ.pop(wm.ALLOW_UNLOCKED_ENV_VAR, None)
try:
    with wm._state_transaction(state_path):
        print('FAIL: _state_transaction proceeded with no locking backend and no opt-in')
        sys.exit(1)
except RuntimeError as exc:
    if wm.ALLOW_UNLOCKED_ENV_VAR not in str(exc):
        print(f'FAIL: fail-closed error does not name the opt-in variable: {exc}')
        sys.exit(1)

# 8c: the explicit opt-in must still work, so an operator on an exotic
# runtime is warned rather than hard-blocked.
os.environ[wm.ALLOW_UNLOCKED_ENV_VAR] = '1'
try:
    with wm._state_transaction(state_path):
        pass
except RuntimeError as exc:
    print(f'FAIL: opt-in did not permit unlocked operation: {exc}')
    sys.exit(1)

print('PASS: no-backend mutation fails closed by default and honors the opt-in')
sys.exit(0)
PYEOF

CASE8B_EXIT=0
CASE8B_OUT="$(python3 "${CASE8B_SCRIPT}" 2>&1)" || CASE8B_EXIT=$?

if [[ "${CASE8B_EXIT}" -eq 0 && "${CASE8B_OUT}" == *"PASS"* ]]; then
  pass "no locking backend: state mutation fails closed by default, opt-in still available"
else
  fail "no-backend fail-closed: ${CASE8B_OUT}"
fi

# ── Case 9: Windows locking ─────────────────────────────────────────────────
# NOT_PROVEN: msvcrt.locking behavior must be exercised by Windows
# release-preparation CI.  This runner is Linux; no attempt is made here.
echo "NOT_PROVEN: Case 9 — Windows msvcrt locking (requires Windows CI lane)"

# ── Case 10: the atomic write preserves the destination's permission mode. ──
#
# `tempfile.mkstemp` creates files 0600 and `Path.replace` carries the SOURCE
# mode onto the destination, so a naive atomic write silently tightens a
# previously group/world-readable state file to owner-only.
CASE10_STATE="${TMPDIR_BASE}/case10-state.json"
CASE10_SCRIPT="${TMPDIR_BASE}/case10_mode_preserved.py"

cat > "${CASE10_SCRIPT}" << PYEOF
import importlib.util, json, os, pathlib, stat, sys

spec = importlib.util.spec_from_file_location(
    'worktree_manager_mode',
    '${AGENT_ONE}/scripts/worktree-manager.py',
)
wm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wm)

state_file = pathlib.Path('${CASE10_STATE}')
state_file.parent.mkdir(parents=True, exist_ok=True)
state_file.write_text(json.dumps({'version': 1, 'slots': []}) + '\n', encoding='utf-8')
os.chmod(state_file, 0o644)
before = stat.S_IMODE(state_file.stat().st_mode)

wm.save_json(state_file, {'version': 1, 'slots': [{'slot_id': 'after'}]})
after = stat.S_IMODE(state_file.stat().st_mode)

if after != before:
    print(f'FAIL: mode changed across atomic write: {before:04o} -> {after:04o}')
    sys.exit(1)

# The write must still have landed.
payload = json.loads(state_file.read_text(encoding='utf-8'))
if not payload.get('slots'):
    print('FAIL: payload was not written')
    sys.exit(1)

print(f'PASS: mode {before:04o} preserved across atomic write')
sys.exit(0)
PYEOF

CASE10_EXIT=0
CASE10_OUT="$(python3 "${CASE10_SCRIPT}" 2>&1)" || CASE10_EXIT=$?

if [[ "${CASE10_EXIT}" -eq 0 && "${CASE10_OUT}" == *"PASS"* ]]; then
  pass "atomic write preserves the destination state file's permission mode"
else
  fail "atomic write mode preservation: ${CASE10_OUT}"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
TOTAL=$((PASS_COUNT + FAIL_COUNT))
echo ""
echo "=== Results: ${PASS_COUNT}/${TOTAL} passed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi

exit 0
