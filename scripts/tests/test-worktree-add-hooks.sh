#!/usr/bin/env bash
# Test suite for hook provisioning on the worktree-creation path (issue #17406).
#
# Covers both provisioning paths:
#   - scripts/worktree-add.sh (the documented `git worktree add` wrapper), and
#   - scripts/worktree-manager.py allocate (the managed-slot path).
#
# Cases:
#   A. wrapper provisions on drift: fresh worktree gets an executable
#      installed pre-push matching hooks/pre-push (modulo the installer's
#      trailing newline) in its OWN hooks dir, the installer runs exactly
#      once, core.hooksPath is set, and the shared common dir is untouched;
#   B. wrapper provisions each fresh worktree independently: a second tree
#      provisions its own hooks (installer runs again) and the same-revision
#      fan-out keeps the first tree guarded and byte-identical;
#   B2. mixed-mode fast path: core.hooksPath unset with current shared
#      common-dir hooks → installer NOT invoked (no cargo build);
#   C. wrapper loud failure: installer fails → non-zero exit naming the
#      installer, worktree left in place for retry;
#   D. manager allocate provisions on drift (same assertions as A);
#   E. manager allocate loud failure: non-zero exit naming the installer,
#      worktree left in place, slot NOT recorded active;
#   F. no hooks/pre-push authority in the new worktree → both paths skip
#      provisioning and exit 0 without invoking the installer;
#   G. wrapper with a failing `git worktree add` → non-zero exit, installer
#      never invoked.
#   E2. manager retries a provisioning-failed slot: same allocate resumes
#      (provision + record) without resetting the branch (recovery commit
#      survives).
#   H. wrapper with partial shared state (mixed mode: current common
#      pre-push, missing pre-commit) runs the installer instead of the fast
#      path, and lands per-tree hooks.
#   I. manager allocate does the same from partial shared state.
#   J. wrapper resolves the destination from tricky but valid flag salads
#      (combined shorts, attached -b value); guards the arg parser against
#      breaking valid invocations.
#
# Fully hermetic: a throwaway bare "origin" plus a clone under a tmpdir. The
# fixture commits its OWN scripts/install-githooks.sh, which both
# provisioning paths prefer (installer and authority stay at the same
# revision); the fixture installer emulates the real one (fan the running
# authority out to every tree's own .githooks dir, then set the repo-local
# relative core.hooksPath — writes before the flip, exactly like the real
# installer since the #17426 sibling-guarding repair)
# and honors STUB_INSTALL_FAIL / STUB_INSTALL_LOG. The real installer's byte
# contract is pinned by the perl-ci-hygiene Rust tests; this suite pins the
# provisioning wiring around it. No cargo, no network, no interaction with
# the real repo's hooks or worktree state.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
WRAPPER="${REPO_ROOT}/scripts/worktree-add.sh"
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

if [[ ! -f "$WRAPPER" ]]; then
  echo "ERROR: worktree-add.sh not found at ${WRAPPER}"
  exit 1
fi
if [[ ! -f "$MANAGER_SRC" ]]; then
  echo "ERROR: worktree-manager.py not found at ${MANAGER_SRC}"
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: python3 not found on PATH"
  exit 1
fi

# Same normalization the provisioning paths use: trailing newlines ignored.
installed_matches_authority() {
  [[ "$(tr -d '\r' < "$1")" == "$(tr -d '\r' < "$2")" ]]
}

# Write the fixture-owned installer. Both provisioning paths invoke it via an
# explicit `bash <script>` (never through PATH/cargo resolution), so this is
# hermetic on every platform: no cargo stub, no extension/PATHEXT games.
write_fixture_installer() {
  mkdir -p "$(dirname "$1")"
  cat > "$1" <<'FIXTURE'
#!/usr/bin/env bash
# Fixture installer for the #17406 provisioning suite (committed into the
# fixture repo). Emulates scripts/install-githooks.sh; STUB_INSTALL_FAIL
# makes it refuse, and every invocation appends to STUB_INSTALL_LOG.
set -u
if [ -n "${STUB_INSTALL_FAIL:-}" ]; then echo "stub: install refused" >&2; exit "${STUB_INSTALL_FAIL}"; fi
printf 'install %s\n' "$(pwd)" >> "${STUB_INSTALL_LOG:?}"
# Fan out like the real installer (#17426 review): the running tree's
# authority bytes go to EVERY tree's own .githooks dir first (spelled
# directly — --git-path still resolves to the common dir before the flip),
# then the shared config flips so no sibling is left unguarded.
top="$(git rev-parse --show-toplevel)"
dest=""
commit_dest=""
while IFS= read -r wt; do
  [ -n "$wt" ] && [ -d "$wt" ] || continue
  dest="$wt/.githooks/pre-push"
  mkdir -p "$(dirname "$dest")"
  cp "$top/hooks/pre-push" "$dest"
  printf '\n' >> "$dest"
  chmod +x "$dest"
  commit_dest="$wt/.githooks/pre-commit"
  printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$commit_dest"
  printf '\n' >> "$commit_dest"
  chmod +x "$commit_dest"
done < <(git worktree list --porcelain | sed -n 's/^worktree //p')
git config --local core.hooksPath .githooks
echo "stub installed pre-push to $dest and pre-commit to $commit_dest"
FIXTURE
  chmod +x "$1"
}

stub_install_count() {
  if [[ -f "$STUB_INSTALL_LOG" ]]; then
    grep -c '^install ' "$STUB_INSTALL_LOG" || true
  else
    echo 0
  fi
}

# Resolve a tree's installed hooks dir the way the provisioning paths do.
hooks_dir_of() {
  git -C "$1" rev-parse --path-format=absolute --git-path hooks
}

# True when the shared common hooks dir carries no real hooks (only the
# *.sample files git init creates). Rule C: the toolchain never writes there.
common_hooks_clean() {
  [[ -z "$(find "$1" -type f ! -name '*.sample' 2>/dev/null)" ]]
}

echo "=== worktree hook-provisioning test suite (#17406) ==="
echo ""

TMPDIR_BASE="$(mktemp -d)"
STUB_INSTALL_LOG="${TMPDIR_BASE}/stub-installs.log"
export STUB_INSTALL_LOG

# ── Fixture setup ─────────────────────────────────────────────────────────
# origin.git: bare "remote". agent-one: clone acting as the coordination
# checkout. The pre-hook commit SHA lets case F allocate a worktree whose
# checkout carries no hooks/ authority at all.
ORIGIN_BARE="${TMPDIR_BASE}/origin.git"
git -c init.defaultBranch=main init -q --bare "$ORIGIN_BARE"

AGENT_ONE="${TMPDIR_BASE}/agent-one"
git -c user.name="Fixture" -c user.email="fixture@example.com" clone -q "$ORIGIN_BARE" "$AGENT_ONE" >/dev/null 2>&1
(
  cd "$AGENT_ONE"
  git checkout -B main -q
  git config user.email "fixture@example.com"
  git config user.name "Fixture"
  echo "init" > file.txt
  git add file.txt
  git commit -q -m "init"
  mkdir -p hooks
  printf '#!/usr/bin/env bash\necho fixture-hook\n' > hooks/pre-push
  git add hooks/pre-push
  git commit -q -m "hook authority"
  git push -q origin main
)
write_fixture_installer "${AGENT_ONE}/scripts/install-githooks.sh"
(
  cd "$AGENT_ONE"
  git add scripts/install-githooks.sh
  git commit -q -m "fixture installer"
  git push -q origin main
)
PRE_HOOK_SHA="$(git -C "$AGENT_ONE" rev-parse HEAD~2)"
COMMON_HOOKS="$(git -C "$AGENT_ONE" rev-parse --path-format=absolute --git-common-dir)/hooks"
rm -f "$COMMON_HOOKS/pre-push" "$COMMON_HOOKS/pre-commit" # belt-and-braces: the clone starts with zero hooks

# Place the manager under test where REPO_ROOT resolution expects it.
mkdir -p "${AGENT_ONE}/scripts"
cp "$MANAGER_SRC" "${AGENT_ONE}/scripts/worktree-manager.py"

run_manager() {
  # state/managed-root flags go after the subcommand (subparser defaults
  # would otherwise overwrite top-level values — see test-worktree-manager.sh).
  local state_file="$1" managed_root="$2" subcommand="$3"
  shift 3
  (
    cd "$AGENT_ONE"
    python3 scripts/worktree-manager.py "$subcommand" \
      --state-file "$state_file" --managed-root "$managed_root" "$@"
  )
}

# ── Case A: wrapper provisions on drift ───────────────────────────────────
WT_A="${TMPDIR_BASE}/wt-a"
CASE_A_EXIT=0
CASE_A_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-a" "$WT_A" 2>&1)" || CASE_A_EXIT=$?
HOOKS_A="$(hooks_dir_of "$WT_A")"
INSTALLED_A="$HOOKS_A/pre-push"
INSTALLED_COMMIT_A="$HOOKS_A/pre-commit"

if [[ "$CASE_A_EXIT" -ne 0 ]]; then
  fail "wrapper provisions on drift: exited $CASE_A_EXIT: $CASE_A_OUT"
elif [[ "$HOOKS_A" != */wt-a/.githooks ]]; then
  fail "wrapper provisions on drift: hooks dir is $HOOKS_A, expected the tree's own .githooks"
elif [[ ! -f "$INSTALLED_A" ]]; then
  fail "wrapper provisions on drift: no installed pre-push at $INSTALLED_A"
elif [[ ! -x "$INSTALLED_A" ]]; then
  fail "wrapper provisions on drift: installed pre-push is not executable"
elif ! installed_matches_authority "$INSTALLED_A" "$WT_A/hooks/pre-push"; then
  fail "wrapper provisions on drift: installed pre-push does not match hooks/pre-push"
elif [[ ! -f "$INSTALLED_COMMIT_A" ]]; then
  fail "wrapper provisions on drift: no installed pre-commit at $INSTALLED_COMMIT_A"
elif [[ ! -x "$INSTALLED_COMMIT_A" ]]; then
  fail "wrapper provisions on drift: installed pre-commit is not executable"
elif [[ "$(git -C "$WT_A" config --get core.hooksPath)" != ".githooks" ]]; then
  fail "wrapper provisions on drift: core.hooksPath is not the installer-managed relative value"
elif ! common_hooks_clean "$COMMON_HOOKS"; then
  fail "wrapper provisions on drift: the shared common hooks dir was written"
elif [[ "$(stub_install_count)" -ne 1 ]]; then
  fail "wrapper provisions on drift: installer ran $(stub_install_count) times, expected 1"
else
  pass "wrapper provisions on drift: fresh worktree gets its own current executable pre-push, installer ran once"
fi
cp "$INSTALLED_A" "${TMPDIR_BASE}/a-pre-push.snapshot" 2>/dev/null || true

# ── Case B: wrapper provisions each fresh worktree independently ──────────
# The second tree provisions its own hooks (the installer runs again); the
# same-revision fan-out rewrites the first tree's file with identical bytes,
# so it stays guarded and byte-identical (cross-revision skew is covered by
# the isolation fixture's case 3/7, not here).
WT_B="${TMPDIR_BASE}/wt-b"
CASE_B_EXIT=0
CASE_B_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-b" "$WT_B" 2>&1)" || CASE_B_EXIT=$?
HOOKS_B="$(hooks_dir_of "$WT_B")"
INSTALLED_B="$HOOKS_B/pre-push"
INSTALLED_COMMIT_B="$HOOKS_B/pre-commit"

if [[ "$CASE_B_EXIT" -ne 0 ]]; then
  fail "wrapper independent provision: exited $CASE_B_EXIT: $CASE_B_OUT"
elif [[ ! -f "$INSTALLED_B" ]]; then
  fail "wrapper independent provision: no installed pre-push at $INSTALLED_B"
elif [[ ! -x "$INSTALLED_B" ]]; then
  fail "wrapper independent provision: installed pre-push is not executable"
elif ! installed_matches_authority "$INSTALLED_B" "$WT_B/hooks/pre-push"; then
  fail "wrapper independent provision: installed pre-push does not match hooks/pre-push"
elif [[ ! -x "$INSTALLED_COMMIT_B" ]]; then
  fail "wrapper independent provision: installed pre-commit is missing or not executable"
elif [[ "$(stub_install_count)" -ne 2 ]]; then
  fail "wrapper independent provision: installer ran $(stub_install_count) times, expected 2 (fresh tree, own dir)"
elif ! cmp -s "${TMPDIR_BASE}/a-pre-push.snapshot" "$INSTALLED_A"; then
  fail "wrapper independent provision: provisioning wt-b altered wt-a's installed bytes"
elif ! common_hooks_clean "$COMMON_HOOKS"; then
  fail "wrapper independent provision: the shared common hooks dir was written"
else
  pass "wrapper independent provision: second tree gets its own hooks, first tree guarded and byte-identical"
fi

# ── Case B2: mixed-mode fast path (hooks already current, shared) ─────────
# core.hooksPath unset + current common-dir hooks (pre-migration leftovers):
# the installer is NOT invoked. Seeds are removed and hooksPath restored at
# the end so later cases start from the migrated state.
git -C "$AGENT_ONE" config --local --unset core.hooksPath
cp "$WT_A/hooks/pre-push" "$COMMON_HOOKS/pre-push"
printf '\n' >> "$COMMON_HOOKS/pre-push"
chmod +x "$COMMON_HOOKS/pre-push"
printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$COMMON_HOOKS/pre-commit"
printf '\n' >> "$COMMON_HOOKS/pre-commit"
chmod +x "$COMMON_HOOKS/pre-commit"
WT_B2="${TMPDIR_BASE}/wt-b2"
BEFORE_B2="$(stub_install_count)"
CASE_B2_EXIT=0
CASE_B2_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-b2" "$WT_B2" 2>&1)" || CASE_B2_EXIT=$?

if [[ "$CASE_B2_EXIT" -ne 0 ]]; then
  fail "mixed-mode fast path: exited $CASE_B2_EXIT: $CASE_B2_OUT"
elif [[ "$(stub_install_count)" -ne "$BEFORE_B2" ]]; then
  fail "mixed-mode fast path: installer ran (count $(stub_install_count)) instead of skipping the build"
elif [[ "$CASE_B2_OUT" != *"already current"* ]]; then
  fail "mixed-mode fast path: output does not report the skip: $CASE_B2_OUT"
else
  pass "mixed-mode fast path: current shared hooks skip the installer (no build)"
fi
rm -f "$COMMON_HOOKS/pre-push" "$COMMON_HOOKS/pre-commit"
git -C "$AGENT_ONE" config --local core.hooksPath .githooks

# ── Case C: wrapper loud failure ──────────────────────────────────────────
# No drift-forcing needed: a fresh tree's own hooks dir starts empty.
WT_C="${TMPDIR_BASE}/wt-c"
CASE_C_EXIT=0
CASE_C_OUT="$(cd "$AGENT_ONE" && STUB_INSTALL_FAIL=42 bash "$WRAPPER" -b "feature/wt-c" "$WT_C" 2>&1)" || CASE_C_EXIT=$?

if [[ "$CASE_C_EXIT" -eq 0 ]]; then
  fail "wrapper loud failure: exited 0 despite installer failure"
elif [[ "$CASE_C_OUT" != *"install-githooks"* ]]; then
  fail "wrapper loud failure: output does not name the installer: $CASE_C_OUT"
elif [[ "$CASE_C_OUT" != *"stub: install refused"* ]]; then
  fail "wrapper loud failure: failure did not come from the fixture installer: $CASE_C_OUT"
elif [[ ! -d "$WT_C" ]]; then
  fail "wrapper loud failure: worktree was rolled back (contract: left in place for retry)"
else
  pass "wrapper loud failure: non-zero exit naming the installer, worktree left in place"
fi

# ── Case D: manager allocate provisions on drift ──────────────────────────
STATE_D="${TMPDIR_BASE}/state-d.json"
MANAGED_D="${TMPDIR_BASE}/managed-d"
BEFORE_D="$(stub_install_count)"
CASE_D_EXIT=0
CASE_D_OUT="$(run_manager "$STATE_D" "$MANAGED_D" allocate --slot slot-d --branch feature/slot-d 2>&1)" || CASE_D_EXIT=$?
HOOKS_D="$(hooks_dir_of "${MANAGED_D}/slot-d")"
INSTALLED_D="$HOOKS_D/pre-push"
INSTALLED_COMMIT_D="$HOOKS_D/pre-commit"

if [[ "$CASE_D_EXIT" -ne 0 ]]; then
  fail "manager allocate provisions on drift: exited $CASE_D_EXIT: $CASE_D_OUT"
elif [[ "$HOOKS_D" != */slot-d/.githooks ]]; then
  fail "manager allocate provisions on drift: hooks dir is $HOOKS_D, expected the slot's own .githooks"
elif [[ ! -f "$INSTALLED_D" ]]; then
  fail "manager allocate provisions on drift: no installed pre-push at $INSTALLED_D"
elif [[ ! -x "$INSTALLED_D" ]]; then
  fail "manager allocate provisions on drift: installed pre-push is not executable"
elif ! installed_matches_authority "$INSTALLED_D" "${MANAGED_D}/slot-d/hooks/pre-push"; then
  fail "manager allocate provisions on drift: installed pre-push does not match hooks/pre-push"
elif [[ ! -x "$INSTALLED_COMMIT_D" ]]; then
  fail "manager allocate provisions on drift: installed pre-commit is missing or not executable"
elif ! common_hooks_clean "$COMMON_HOOKS"; then
  fail "manager allocate provisions on drift: the shared common hooks dir was written"
elif [[ "$(stub_install_count)" -ne $((BEFORE_D + 1)) ]]; then
  fail "manager allocate provisions on drift: installer invocation count did not advance by exactly 1"
else
  pass "manager allocate provisions on drift: slot gets its own current executable pre-push"
fi

# ── Case E: manager allocate loud failure ─────────────────────────────────
STATE_E="${TMPDIR_BASE}/state-e.json"
MANAGED_E="${TMPDIR_BASE}/managed-e"
CASE_E_EXIT=0
# export inside the command-substitution subshell: a VAR=value prefix before a
# shell *function* would leak into later cases (POSIX leaves it unspecified;
# bash persists it), while the $(...) subshell containment cannot leak.
CASE_E_OUT="$(export STUB_INSTALL_FAIL=43; run_manager "$STATE_E" "$MANAGED_E" allocate --slot slot-e --branch feature/slot-e 2>&1)" || CASE_E_EXIT=$?

if [[ "$CASE_E_EXIT" -eq 0 ]]; then
  fail "manager allocate loud failure: exited 0 despite installer failure"
elif [[ "$CASE_E_OUT" != *"install-githooks"* ]]; then
  fail "manager allocate loud failure: output does not name the installer: $CASE_E_OUT"
elif [[ "$CASE_E_OUT" != *"stub: install refused"* ]]; then
  fail "manager allocate loud failure: failure did not come from the fixture installer: $CASE_E_OUT"
elif [[ ! -d "${MANAGED_E}/slot-e" ]]; then
  fail "manager allocate loud failure: worktree was rolled back (contract: left in place for retry)"
elif [[ -f "$STATE_E" ]] && grep -q '"slot-e"' "$STATE_E"; then
  fail "manager allocate loud failure: slot recorded despite provisioning failure"
else
  pass "manager allocate loud failure: non-zero exit naming the installer, worktree kept, slot unrecorded"
fi
# ── Case E2: manager retries the failed slot without resetting ────────────
# The slot-e worktree from case E exists on disk but was never recorded, and
# the hooks are still stale. Add a unique commit (recovery work), leave the
# installer repaired (STUB_INSTALL_FAIL was contained to case E's subshell),
# and retry the identical allocate: it must resume (provision + record)
# without resetting the branch to base.
SLOT_E_WT="${MANAGED_E}/slot-e"
echo "recovery" > "${SLOT_E_WT}/recovery.txt"
git -C "$SLOT_E_WT" -c user.email="fixture@example.com" -c user.name="Fixture" add recovery.txt >/dev/null 2>&1
# Redirected: the commit runs the installed pre-commit hook, whose echo is
# noise here (a commit failure still kills the suite via set -e).
git -C "$SLOT_E_WT" -c user.email="fixture@example.com" -c user.name="Fixture" commit -qm "recovery work" >/dev/null 2>&1
BEFORE_E2="$(stub_install_count)"
CASE_E2_EXIT=0
CASE_E2_OUT="$(run_manager "$STATE_E" "$MANAGED_E" allocate --slot slot-e --branch feature/slot-e 2>&1)" || CASE_E2_EXIT=$?

if [[ "$CASE_E2_EXIT" -ne 0 ]]; then
  fail "manager retry resumes the failed slot: exited $CASE_E2_EXIT: $CASE_E2_OUT"
elif [[ "$CASE_E2_OUT" != *"resumed-tip"* ]]; then
  fail "manager retry resumes the failed slot: output does not report the resume: $CASE_E2_OUT"
elif ! grep -q '"slot-e"' "$STATE_E"; then
  fail "manager retry resumes the failed slot: slot-e not recorded in state"
elif ! git -C "$SLOT_E_WT" log --oneline | grep -q "recovery work"; then
  fail "manager retry resumes the failed slot: recovery commit was reset away"
elif [[ "$(stub_install_count)" -ne $((BEFORE_E2 + 1)) ]]; then
  fail "manager retry resumes the failed slot: installer did not run exactly once on resume"
else
  pass "manager retry resumes the failed slot: provisioned, recorded, branch untouched"
fi

# ── Case F: no authority → both paths skip ────────────────────────────────
WT_F="${TMPDIR_BASE}/wt-f"
BEFORE_F="$(stub_install_count)"
CASE_F_EXIT=0
CASE_F_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-f" "$WT_F" "$PRE_HOOK_SHA" 2>&1)" || CASE_F_EXIT=$?

if [[ "$CASE_F_EXIT" -ne 0 ]]; then
  fail "no-authority wrapper skip: exited $CASE_F_EXIT: $CASE_F_OUT"
elif [[ "$(stub_install_count)" -ne "$BEFORE_F" ]]; then
  fail "no-authority wrapper skip: installer ran with no authority present"
elif [[ "$CASE_F_OUT" != *"skipping hook provisioning"* ]]; then
  fail "no-authority wrapper skip: output does not report the skip: $CASE_F_OUT"
else
  pass "no-authority wrapper skip: exit 0 without invoking the installer"
fi

STATE_F="${TMPDIR_BASE}/state-f.json"
MANAGED_F="${TMPDIR_BASE}/managed-f"
BEFORE_FM="$(stub_install_count)"
CASE_FM_EXIT=0
CASE_FM_OUT="$(run_manager "$STATE_F" "$MANAGED_F" allocate --slot slot-f --branch feature/slot-f --ref "$PRE_HOOK_SHA" 2>&1)" || CASE_FM_EXIT=$?

if [[ "$CASE_FM_EXIT" -ne 0 ]]; then
  fail "no-authority manager skip: exited $CASE_FM_EXIT: $CASE_FM_OUT"
elif [[ "$(stub_install_count)" -ne "$BEFORE_FM" ]]; then
  fail "no-authority manager skip: installer ran with no authority present"
elif [[ "$CASE_FM_OUT" != *"skipping hook provisioning"* ]]; then
  fail "no-authority manager skip: output does not report the skip: $CASE_FM_OUT"
else
  pass "no-authority manager skip: exit 0 without invoking the installer"
fi

# ── Case G: wrapper with failing git worktree add ─────────────────────────
BEFORE_G="$(stub_install_count)"
CASE_G_EXIT=0
CASE_G_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-a" "${TMPDIR_BASE}/wt-dupe" 2>&1)" || CASE_G_EXIT=$?

if [[ "$CASE_G_EXIT" -eq 0 ]]; then
  fail "wrapper git failure: exited 0 despite git worktree add failing (branch already checked out)"
elif [[ "$(stub_install_count)" -ne "$BEFORE_G" ]]; then
  fail "wrapper git failure: installer ran even though no worktree was created"
elif [[ "$CASE_G_OUT" != *"no worktree created"* ]]; then
  fail "wrapper git failure: output does not report the git failure: $CASE_G_OUT"
else
  pass "wrapper git failure: non-zero exit, installer never invoked"
fi

# ── Case H: wrapper provisions from partial shared state ──────────────────
# Mixed mode: core.hooksPath unset, common pre-push current but pre-commit
# missing. Partial currency must not trigger the fast path: the installer
# runs and the tree lands its own hooks (and sets hooksPath). Seeds are
# removed at the end; hooksPath stays set (the installer set it).
git -C "$AGENT_ONE" config --local --unset core.hooksPath
cp "$WT_A/hooks/pre-push" "$COMMON_HOOKS/pre-push"
printf '\n' >> "$COMMON_HOOKS/pre-push"
chmod +x "$COMMON_HOOKS/pre-push"
rm -f "$COMMON_HOOKS/pre-commit"
WT_H="${TMPDIR_BASE}/wt-h"
BEFORE_H="$(stub_install_count)"
CASE_H_EXIT=0
CASE_H_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-h" "$WT_H" 2>&1)" || CASE_H_EXIT=$?
HOOKS_H="$(hooks_dir_of "$WT_H")"
INSTALLED_H="$HOOKS_H/pre-push"
INSTALLED_COMMIT_H="$HOOKS_H/pre-commit"

if [[ "$CASE_H_EXIT" -ne 0 ]]; then
  fail "wrapper provisions from partial shared state: exited $CASE_H_EXIT: $CASE_H_OUT"
elif [[ "$HOOKS_H" != */wt-h/.githooks ]]; then
  fail "wrapper provisions from partial shared state: hooks dir is $HOOKS_H, expected the tree's own .githooks"
elif ! installed_matches_authority "$INSTALLED_H" "$WT_H/hooks/pre-push"; then
  fail "wrapper provisions from partial shared state: installed pre-push does not match hooks/pre-push"
elif [[ ! -x "$INSTALLED_COMMIT_H" ]]; then
  fail "wrapper provisions from partial shared state: installed pre-commit is missing or not executable"
elif [[ "$(git -C "$WT_H" config --get core.hooksPath)" != ".githooks" ]]; then
  fail "wrapper provisions from partial shared state: core.hooksPath was not set by the installer"
elif [[ "$(stub_install_count)" -ne $((BEFORE_H + 1)) ]]; then
  fail "wrapper provisions from partial shared state: installer did not run exactly once"
else
  pass "wrapper provisions from partial shared state: installer ran, per-tree hooks usable"
fi
rm -f "$COMMON_HOOKS/pre-push" "$COMMON_HOOKS/pre-commit"

# ── Case I: manager provisions from partial shared state ──────────────────
git -C "$AGENT_ONE" config --local --unset core.hooksPath
cp "$WT_A/hooks/pre-push" "$COMMON_HOOKS/pre-push"
printf '\n' >> "$COMMON_HOOKS/pre-push"
chmod +x "$COMMON_HOOKS/pre-push"
rm -f "$COMMON_HOOKS/pre-commit"
STATE_I="${TMPDIR_BASE}/state-i.json"
MANAGED_I="${TMPDIR_BASE}/managed-i"
BEFORE_I="$(stub_install_count)"
CASE_I_EXIT=0
CASE_I_OUT="$(run_manager "$STATE_I" "$MANAGED_I" allocate --slot slot-i --branch feature/slot-i 2>&1)" || CASE_I_EXIT=$?
HOOKS_I="$(hooks_dir_of "${MANAGED_I}/slot-i")"
INSTALLED_I="$HOOKS_I/pre-push"
INSTALLED_COMMIT_I="$HOOKS_I/pre-commit"

if [[ "$CASE_I_EXIT" -ne 0 ]]; then
  fail "manager provisions from partial shared state: exited $CASE_I_EXIT: $CASE_I_OUT"
elif [[ "$HOOKS_I" != */slot-i/.githooks ]]; then
  fail "manager provisions from partial shared state: hooks dir is $HOOKS_I, expected the slot's own .githooks"
elif ! installed_matches_authority "$INSTALLED_I" "${MANAGED_I}/slot-i/hooks/pre-push"; then
  fail "manager provisions from partial shared state: installed pre-push does not match hooks/pre-push"
elif [[ ! -x "$INSTALLED_COMMIT_I" ]]; then
  fail "manager provisions from partial shared state: installed pre-commit is missing or not executable"
elif [[ "$(git -C "${MANAGED_I}/slot-i" config --get core.hooksPath)" != ".githooks" ]]; then
  fail "manager provisions from partial shared state: core.hooksPath was not set by the installer"
elif [[ "$(stub_install_count)" -ne $((BEFORE_I + 1)) ]]; then
  fail "manager provisions from partial shared state: installer did not run exactly once"
else
  pass "manager provisions from partial shared state: installer ran, per-tree hooks usable"
fi
rm -f "$COMMON_HOOKS/pre-push" "$COMMON_HOOKS/pre-commit"

# ── Case J: wrapper resolves tricky flag salads ───────────────────────────
# Combined short flags plus an attached -b value are valid `git worktree
# add` invocations; the arg parser must resolve the destination path from
# them (no shared-list delta, so concurrent adds cannot confuse it).
WT_J="${TMPDIR_BASE}/wt-j"
CASE_J_EXIT=0
CASE_J_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -qf -bfeat/salad "$WT_J" main 2>&1)" || CASE_J_EXIT=$?

if [[ "$CASE_J_EXIT" -ne 0 ]]; then
  fail "wrapper flag salad: exited $CASE_J_EXIT: $CASE_J_OUT"
elif [[ ! -d "$WT_J" ]]; then
  fail "wrapper flag salad: destination worktree was not created at $WT_J"
elif [[ "$(git -C "$WT_J" rev-parse --abbrev-ref HEAD 2>/dev/null)" != "feat/salad" ]]; then
  fail "wrapper flag salad: worktree branch is not feat/salad"
else
  pass "wrapper flag salad: destination resolved from combined/attached flags"
fi

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi
exit 0
