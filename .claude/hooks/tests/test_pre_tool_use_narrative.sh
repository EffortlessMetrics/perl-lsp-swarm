#!/usr/bin/env bash
# Test the narrative-stripping preprocessor in .claude/hooks/pre-tool-use.sh
# (#4006). Trigger phrases inside narrative -- a commit message, a heredoc
# body, a quoted argument, a comment -- must no longer false-block. An
# UNQUOTED real invocation of a guarded operation must still block.
#
# Both directions are exercised by actually piping the JSON tool_input
# through the hook and asserting the exit code (never by reading the regex).

set -eu

HOOK="$(git rev-parse --show-toplevel)/.claude/hooks/pre-tool-use.sh"
FAIL=0

# Build the tool-input JSON with jq so quotes/newlines in the command are
# encoded safely regardless of shell quoting.
run_case() {
  local label="$1" cmd="$2" expected_exit="$3" readonly_flag="${4:-0}" cwd="${5:-}"
  local actual dir
  dir="${cwd:-$PWD}"
  actual=$(
    cd "$dir" && \
    CLAUDE_AGENT_READONLY="$readonly_flag" \
      jq -nc --arg c "$cmd" '{tool_input:{command:$c}}' | \
      CLAUDE_AGENT_READONLY="$readonly_flag" bash "$HOOK" >/dev/null 2>&1
    echo $?
  )
  if [ "$actual" = "$expected_exit" ]; then
    echo "PASS  $label (exit $actual)"
  else
    echo "FAIL  $label (expected $expected_exit, got $actual) CMD=$cmd"
    FAIL=1
  fi
}

MAIN="$(git rev-parse --show-toplevel)"

# A temporary linked worktree, used both for the "worktree add still blocked"
# case and to exercise the CWD-inside-worktree branch of the hook.
WORKTREE_TMP="$(mktemp -d)/wt-narrative-$$"
git worktree add --detach "$WORKTREE_TMP" HEAD >/dev/null 2>&1
trap 'git worktree remove --force "$WORKTREE_TMP" >/dev/null 2>&1 || true' EXIT

echo "############################################"
echo "# SHOULD-NOW-ALLOW (narrative, exit 0)"
echo "############################################"

run_case "allow: commit msg mentions 'git worktree add'" \
  'git commit -m "removed the redundant git worktree add line"' 0 0 "$WORKTREE_TMP"

run_case "allow: commit msg mentions 'git push --force'" \
  'git commit -m "fixed: git push --force is now blocked"' 0

run_case "allow: commit msg mentions 'git reset --hard'" \
  'git commit -m "docs: explain why git reset --hard is dangerous"' 0

run_case "allow: heredoc body documents rm -rf /etc" \
  "$(printf 'gh issue create --title x --body-file - <<EOF\nDo not run rm -rf /etc, it wipes the system.\nEOF')" 0

run_case "allow: heredoc body documents git reset --hard" \
  "$(printf 'gh pr create --title x --body-file - <<EOF\nThis PR forbids git reset --hard in CI.\nEOF')" 0

run_case "allow: heredoc body documents git push --force" \
  "$(printf 'gh issue comment 1 --body-file - <<EOF\nNever run git push --force on main.\nEOF')" 0

run_case "allow: heredoc body documents git worktree add" \
  "$(printf 'cat <<EOF > NOTES.md\nUse git worktree add to create a new worktree.\nEOF')" 0

run_case "allow: quoted description arg with git push --force" \
  'gh pr comment 42 --body "please do not git push --force here"' 0

run_case "allow: quoted description arg with rm -rf /etc" \
  'gh issue create --title "safety" --body "block rm -rf /etc"' 0

run_case "allow: comment narrative mentions rm -rf /etc" \
  'echo hi  # note: rm -rf /etc would be catastrophic' 0

run_case "allow: comment narrative mentions git stash" \
  'echo hi  # do not git stash here' 0

run_case "allow: bare -- terminator narrative in commit msg" \
  'git commit -m "use -- to terminate options, e.g. git checkout -- file"' 0

run_case "allow: gh issue create body-file heredoc, multi-hazard" \
  "$(printf 'gh issue create --title x --body-file - <<EOF\nObserved: rm -rf /etc, git reset --hard, git push --force, git stash all blocked by the hook.\nEOF')" 0

echo
echo "############################################"
echo "# SHOULD-STILL-BLOCK (real unquoted hazard, exit 2)"
echo "############################################"

run_case "block: real rm -rf /etc" 'rm -rf /etc' 2
run_case "block: real rm -rf /" 'rm -rf /' 2
run_case "block: real git reset --hard" 'git reset --hard' 2
run_case "block: real git push --force" 'git push --force' 2
run_case "block: real git push -f" 'git push -f origin main' 2
run_case "block: real refspec force-push" 'git push origin +main' 2
run_case "block: real git stash" 'git stash' 2
run_case "block: real cargo publish" 'cargo publish' 2
run_case "block: real git clean -fd" 'git clean -fd' 2
run_case "block: real git checkout ." 'git checkout .' 2

run_case "block: real git worktree add inside linked worktree" \
  'git worktree add ../x main' 2 0 "$WORKTREE_TMP"

echo
echo "############################################"
echo "# SHOULD-STILL-BLOCK: narrative wrapper does NOT defeat a trailing real hazard"
echo "############################################"

run_case "block: heredoc narrative followed by real rm -rf /etc" \
  "$(printf 'cat <<EOF\ndocs about safety\nEOF\nrm -rf /etc')" 2

run_case "block: commit msg narrative followed by real git push --force" \
  "$(printf 'git commit -m \"about worktree add\" && git push --force')" 2

echo
echo "############################################"
echo "# SHOULD-STILL-BLOCK: M4b read-only-agent guard (real mutation, narrative-wrapped)"
echo "############################################"

run_case "ro: real git commit still blocked" 'git commit -m wip' 2 1
run_case "ro: real git worktree add still blocked" 'git worktree add ../x main' 2 1
run_case "ro: real gh pr merge still blocked" 'gh pr merge 42' 2 1
run_case "ro: real file redirect still blocked" 'echo hi > out.txt' 2 1

run_case "ro-allow: git log --grep quoting the literal phrase 'git commit'" \
  'git log --grep "git commit"' 0 1

if [ "$FAIL" -eq 0 ]; then
  echo
  echo "All narrative-stripping test cases passed."
  exit 0
else
  echo
  echo "Some narrative-stripping test cases failed."
  exit 1
fi
