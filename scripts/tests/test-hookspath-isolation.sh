#!/usr/bin/env bash
# Per-worktree core.hooksPath isolation fixture (issue #17414 rule C).
#
# Two revisions: worktree A is cut from revision 1 (hook authority v1) and
# worktree B from revision 2 (authority v2, different bytes). Then:
#   1. tracked-never: .githooks is ignored and untracked in this repo;
#   2. provision A via scripts/worktree-add.sh: A's hooks land in A's dir;
#   3. provision B via worktree-manager.py allocate: B's hooks land in B's
#      dir AND A stays guarded (the installer fans out to every tree, so
#      A's file carries the installing revision's bytes, never nothing);
#   4. push-sim from B (execute B's installed pre-push, which carries a
#      #4220-style self-heal like the real hook): only B's own file heals;
#   5. the shared common hooks dir carries no real hooks (uninvolved);
#   6. core.hooksPath is the installer-managed relative value;
#   7. agent-preflight.sh passes in B and fails closed (exit 7) in skewed A
#      (hookspath mode): per-tree staleness is detectable, never silent.
#
# Installer behavior and scripts under test are selectable for red-green proof:
#   HOOKSPATH_FIXTURE_INSTALLER_MODE=hookspath (default): the stub installer
#     emulates the real one (write the running tree's authority bytes into
#     EVERY tree's own .githooks dir, then set the repo-local relative
#     core.hooksPath).
#   HOOKSPATH_FIXTURE_INSTALLER_MODE=legacy: the stub emulates pre-#17414
#     behavior (write the common hooks dir, never touch core.hooksPath).
#   HOOKSPATH_FIXTURE_SCRIPTS_DIR (default: this repo's scripts/): must
#     contain worktree-add.sh, worktree-manager.py, agent-preflight.sh.
# Red-green: legacy mode plus pristine pre-change scripts (materialized via
# `git show HEAD:path`, never `git stash`) FAIL the isolation assertions
# (shared-file cross-talk, hooks in the common dir, hooksPath unset), while
# the default run PASSES.
#
# Fully hermetic: a throwaway bare "origin" plus a clone under a tmpdir. No
# cargo, no network, no interaction with the real repo's hooks or worktree
# state (except the tracked-never case, which reads this repo's ignore rules).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SCRIPTS_DIR="${HOOKSPATH_FIXTURE_SCRIPTS_DIR:-${REPO_ROOT}/scripts}"
WRAPPER="${SCRIPTS_DIR}/worktree-add.sh"
MANAGER_SRC="${SCRIPTS_DIR}/worktree-manager.py"
PREFLIGHT="${SCRIPTS_DIR}/agent-preflight.sh"
INSTALLER_MODE="${HOOKSPATH_FIXTURE_INSTALLER_MODE:-hookspath}"

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

if [[ "$INSTALLER_MODE" != "hookspath" && "$INSTALLER_MODE" != "legacy" ]]; then
  echo "ERROR: HOOKSPATH_FIXTURE_INSTALLER_MODE must be hookspath or legacy, got '$INSTALLER_MODE'"
  exit 2
fi
for f in "$WRAPPER" "$MANAGER_SRC" "$PREFLIGHT"; do
  if [[ ! -f "$f" ]]; then echo "ERROR: script under test not found: $f"; exit 1; fi
done
if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: python3 not found on PATH"
  exit 1
fi

hooks_dir_of() {
  git -C "$1" rev-parse --path-format=absolute --git-path hooks
}

common_hooks_clean() {
  [[ -z "$(find "$1" -type f ! -name '*.sample' 2>/dev/null)" ]]
}

# Fixture-owned stub installer, committed into the fixture repo at both
# revisions. Emulates the real installer's write contract per INSTALLER_MODE.
write_fixture_installer() {
  mkdir -p "$(dirname "$1")"
  if [[ "$INSTALLER_MODE" == "hookspath" ]]; then
    cat > "$1" <<'FIXTURE'
#!/usr/bin/env bash
set -u
# Fan out like the real installer: the running tree's authority bytes go to
# EVERY tree's own dir first, then the shared config flips (#17426 review:
# siblings must never be left unguarded by another tree's provision).
top="$(git rev-parse --show-toplevel)"
while IFS= read -r wt; do
  [[ -n "$wt" && -d "$wt" ]] || continue
  # The tree's own dir spelled directly (.githooks): --git-path would still
  # resolve to the common dir before the flip, exactly like the real
  # installer joins INSTALLER_HOOKS_PATH onto each enumerated root.
  dest="$wt/.githooks/pre-push"
  mkdir -p "$(dirname "$dest")"
  cp "$top/hooks/pre-push" "$dest"
  printf '\n' >> "$dest"
  chmod +x "$dest"
  commit_dest="$wt/.githooks/pre-commit"
  printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$commit_dest"
  chmod +x "$commit_dest"
done < <(git worktree list --porcelain | sed -n 's/^worktree //p')
git config --local core.hooksPath .githooks
FIXTURE
  else
    cat > "$1" <<'FIXTURE'
#!/usr/bin/env bash
set -u
dest="$(git rev-parse --git-common-dir)/hooks/pre-push"
mkdir -p "$(dirname "$dest")"
cp hooks/pre-push "$dest"
printf '\n' >> "$dest"
chmod +x "$dest"
commit_dest="$(git rev-parse --git-common-dir)/hooks/pre-commit"
printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$commit_dest"
chmod +x "$commit_dest"
FIXTURE
  fi
  chmod +x "$1"
}

# Fixture hook authority. Both revisions carry the #4220-style self-heal
# (re-copy the checked-in authority over the running hook); only the marker
# differs, so a cross-tree rewrite is byte-visible.
write_authority() {
  cat > "$1" <<FIXTURE
#!/usr/bin/env bash
SELF_TOP="\$(git rev-parse --show-toplevel)"
cp "\$SELF_TOP/hooks/pre-push" "\$0" 2>/dev/null || true
echo fixture-hook-$2
FIXTURE
}

echo "=== core.hooksPath isolation fixture (#17414 rule C, installer=$INSTALLER_MODE) ==="
echo ""

# -- Case 1: tracked-never ------------------------------------------------
# Reads THIS repo (the change under test), not the scripts override: the
# .githooks dir must be ignored and must never be committed.
if git -C "$REPO_ROOT" check-ignore -q .githooks/pre-push; then
  pass "tracked-never: .githooks is ignored"
else
  fail "tracked-never: .githooks/pre-push is NOT ignored (missing .gitignore entry)"
fi
if [[ -z "$(git -C "$REPO_ROOT" ls-files .githooks)" ]]; then
  pass "tracked-never: no .githooks path is tracked"
else
  fail "tracked-never: tracked .githooks paths exist: $(git -C "$REPO_ROOT" ls-files .githooks)"
fi

TMPDIR_BASE="$(mktemp -d)"

# -- Fixture setup: two revisions ------------------------------------------
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
)
write_authority "${AGENT_ONE}/hooks/pre-push" "v1"
write_fixture_installer "${AGENT_ONE}/scripts/install-githooks.sh"
(
  cd "$AGENT_ONE"
  git add hooks/pre-push scripts/install-githooks.sh
  git commit -q -m "rev1: hook authority v1 + installer"
)
REV1="$(git -C "$AGENT_ONE" rev-parse HEAD)"
write_authority "${AGENT_ONE}/hooks/pre-push" "v2"
(
  cd "$AGENT_ONE"
  git add hooks/pre-push
  git commit -q -m "rev2: hook authority v2"
  git push -q origin main
)
REV2="$(git -C "$AGENT_ONE" rev-parse HEAD)"
COMMON_HOOKS="$(git -C "$AGENT_ONE" rev-parse --path-format=absolute --git-common-dir)/hooks"
rm -f "$COMMON_HOOKS/pre-push" "$COMMON_HOOKS/pre-commit"
cp "$MANAGER_SRC" "${AGENT_ONE}/scripts/worktree-manager.py"

run_manager() {
  local state_file="$1" managed_root="$2" subcommand="$3"
  shift 3
  (
    cd "$AGENT_ONE"
    python3 scripts/worktree-manager.py "$subcommand" \
      --state-file "$state_file" --managed-root "$managed_root" "$@"
  )
}

# -- Case 2: provision A (wrapper) lands A's own hooks ---------------------
WT_A="${TMPDIR_BASE}/wt-a"
CASE2_EXIT=0
CASE2_OUT="$(cd "$AGENT_ONE" && bash "$WRAPPER" -b "feature/wt-a" "$WT_A" "$REV1" 2>&1)" || CASE2_EXIT=$?
HOOKS_A="$(hooks_dir_of "$WT_A" 2>/dev/null || true)"
INSTALLED_A="$HOOKS_A/pre-push"

if [[ "$CASE2_EXIT" -ne 0 ]]; then
  fail "provision A: exited $CASE2_EXIT: $CASE2_OUT"
elif [[ "$INSTALLER_MODE" == "hookspath" && "$HOOKS_A" != */wt-a/.githooks ]]; then
  fail "provision A: hooks dir is $HOOKS_A, expected the tree's own .githooks"
elif ! grep -q "fixture-hook-v1" "$INSTALLED_A" 2>/dev/null; then
  fail "provision A: installed pre-push does not carry the v1 authority"
else
  pass "provision A: worktree at rev1 gets installed v1 hooks"
fi
cp "$INSTALLED_A" "${TMPDIR_BASE}/a-pre-push.snapshot" 2>/dev/null || true

# -- Case 3: provision B (manager) keeps A guarded ---------------------------
STATE_B="${TMPDIR_BASE}/state-b.json"
MANAGED_B="${TMPDIR_BASE}/managed-b"
CASE3_EXIT=0
CASE3_OUT="$(run_manager "$STATE_B" "$MANAGED_B" allocate --slot slot-b --branch feature/slot-b --ref "$REV2" 2>&1)" || CASE3_EXIT=$?
WT_B="${MANAGED_B}/slot-b"
HOOKS_B="$(hooks_dir_of "$WT_B" 2>/dev/null || true)"
INSTALLED_B="$HOOKS_B/pre-push"
HOOKS_A_AFTER="$(hooks_dir_of "$WT_A" 2>/dev/null || true)"

if [[ "$CASE3_EXIT" -ne 0 ]]; then
  fail "provision B: exited $CASE3_EXIT: $CASE3_OUT"
elif [[ "$INSTALLER_MODE" == "hookspath" && "$HOOKS_B" != */slot-b/.githooks ]]; then
  fail "provision B: hooks dir is $HOOKS_B, expected the slot's own .githooks"
elif ! grep -q "fixture-hook-v2" "$INSTALLED_B" 2>/dev/null; then
  fail "provision B: installed pre-push does not carry the v2 authority"
elif [[ "$INSTALLER_MODE" == "hookspath" && "$HOOKS_A_AFTER" != */wt-a/.githooks ]]; then
  fail "provision B: A's hooks dir is $HOOKS_A_AFTER, expected A's own .githooks"
elif [[ "$INSTALLER_MODE" == "hookspath" ]] && ! grep -q "fixture-hook-v2" "$INSTALLED_A" 2>/dev/null; then
  fail "provision B: A's installed hook is missing the fan-out v2 bytes (sibling left unguarded)"
elif [[ "$INSTALLER_MODE" == "hookspath" && ! -x "$INSTALLED_A" ]]; then
  fail "provision B: A's installed hook is not executable after the fan-out"
elif [[ "$INSTALLER_MODE" == "legacy" ]] && ! cmp -s "${TMPDIR_BASE}/a-pre-push.snapshot" "$INSTALLED_A"; then
  fail "provision B: provisioning B altered A's installed bytes"
else
  pass "provision B: slot at rev2 gets v2 hooks, A stays guarded via fan-out"
fi
cp "$INSTALLED_A" "${TMPDIR_BASE}/a-pre-push.snapshot" 2>/dev/null || true

# -- Case 4: push-sim from B heals only B's own file -------------------------
PUSH_OUT=""
if [[ -d "$WT_B" && -f "$INSTALLED_B" ]]; then
  PUSH_OUT="$(cd "$WT_B" && bash "$INSTALLED_B" </dev/null 2>&1 || true)"
fi
if [[ "$PUSH_OUT" != *"fixture-hook-v"* ]]; then
  fail "push-sim from B: installed hook did not execute (output: $PUSH_OUT)"
elif ! cmp -s "${TMPDIR_BASE}/a-pre-push.snapshot" "$INSTALLED_A"; then
  fail "push-sim from B: executing B's hook altered A's installed bytes"
else
  pass "push-sim from B: B's hook self-heals its own file, A's bytes unchanged"
fi

# -- Case 5: shared common dir uninvolved ------------------------------------
if common_hooks_clean "$COMMON_HOOKS"; then
  pass "common dir uninvolved: no real hooks in the shared common hooks dir"
else
  fail "common dir uninvolved: real hooks present in $COMMON_HOOKS"
fi

# -- Case 6: hooksPath is the installer-managed relative value ---------------
HOOKSPATH_VALUE="$(git -C "$WT_A" config --get core.hooksPath 2>/dev/null || true)"
if [[ "$HOOKSPATH_VALUE" == ".githooks" ]]; then
  pass "hooksPath stays worktree-relative and installer-managed (.githooks)"
else
  fail "hooksPath stays worktree-relative and installer-managed (got '$HOOKSPATH_VALUE')"
fi

# -- Case 7: preflight passes in B, fails closed on skew in A ---------------
if [[ "$INSTALLER_MODE" == "hookspath" ]]; then
  CODE_A=0
  (cd "$WT_A" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" >/dev/null 2>&1) || CODE_A=$?
  CODE_B=0
  (cd "$WT_B" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" >/dev/null 2>&1) || CODE_B=$?
  # B is at rev2 with v2 bytes: green. A is at rev1 with fan-out v2 bytes:
  # per-tree staleness must fail closed (exit 7), never silently pass.
  if [[ "$CODE_B" -eq 0 && "$CODE_A" -eq 7 ]]; then
    pass "preflight passes in B and fails closed (exit 7) on skew in A"
  else
    fail "preflight passes in B and fails closed on skew in A (got exit $CODE_A in A, $CODE_B in B)"
  fi
else
  pass "preflight passes in B and fails closed on skew in A (skipped in legacy mode)"
fi

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi
exit 0
