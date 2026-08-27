#!/usr/bin/env bash
# Focused routing proof for the local cleanup helpers.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; exit 1; }

for file in scripts/swarm-clean scripts/cleanup-completed-worktrees.sh scripts/agent-cleanup.ps1; do
    grep -qF 'branch-deletion-admission' "$ROOT/$file" \
        || fail "$file names the shared branch-deletion admission"
    pass "$file names the shared branch-deletion admission"
done

# The admission is granted for a PR's head branch, so agent-cleanup.ps1 must
# refuse an explicitly supplied -Branch that is not that head; otherwise an
# authorization for branch A is applied to branch B. Static assertion only:
# no pwsh is available in this harness, so this pins that the refusal exists
# and is wired to headRefName, not that it executes correctly.
if ! grep -qF 'is not PR #$PrNumber' "$ROOT/scripts/agent-cleanup.ps1" ||
   ! grep -qF 'headRefName' "$ROOT/scripts/agent-cleanup.ps1"; then
    fail 'agent-cleanup.ps1 binds an explicit -Branch to the merged PR head'
fi
pass 'agent-cleanup.ps1 binds an explicit -Branch to the merged PR head'

if grep -nE 'git(-C [^ ]+)? branch -[dD]' "$ROOT/scripts/cleanup-completed-worktrees.sh" >/dev/null; then
    fail 'cleanup-completed-worktrees has an unguarded local branch delete'
fi
pass 'cleanup-completed-worktrees has no local branch delete path'

REPO="$TMP/repo"
git init -q -b main "$REPO"
git -C "$REPO" config user.email test@example.invalid
git -C "$REPO" config user.name test
printf 'init\n' > "$REPO/file"
git -C "$REPO" add file
git -C "$REPO" commit -qm init
git -C "$REPO" worktree add -q -b feature/retained "$TMP/worktree"
printf 'feature\n' > "$TMP/worktree/file"
git -C "$TMP/worktree" add file
git -C "$TMP/worktree" commit -qm feature
git -C "$REPO" merge --no-ff -qm merge-feature feature/retained

mkdir -p "$TMP/bin"
cat > "$TMP/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == *"--state merged"* ]]; then
  printf '123\n'
fi
STUB
chmod +x "$TMP/bin/gh"

cat > "$TMP/bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
# The shared wrapper runs the #12593 toolchain guard before any build work,
# and the guard probes `cargo --version`. Answer that probe like a modern
# rustup cargo so the guard passes and the ROUTED invocation is what lands in
# the log; otherwise the probe clobbers it and the guard refuses at exit 78.
if [[ "$*" == "--version" ]]; then
  printf 'cargo 1.95.0 (0000000000 2026-01-01)\n'
  exit 0
fi
printf '%s\n' "$*" > "${ADMISSION_LOG}"
exit 3
STUB
chmod +x "$TMP/bin/cargo"

PATH="$TMP/bin:$PATH" \
ADMISSION_LOG="$TMP/admission.log" \
REPO_ROOT="$REPO" \
bash "$ROOT/scripts/swarm-clean" --apply >/dev/null

[[ ! -d "$TMP/worktree" ]] || fail 'cleaned worktree was not removed'
git -C "$REPO" show-ref --verify --quiet refs/heads/feature/retained \
    || fail 'retaining admission did not preserve the local branch'
grep -qF 'run --quiet --locked -p xtask --bin branch-deletion-admission -- plan --pr 123 --remote origin' "$TMP/admission.log" \
    || fail 'local branch deletion did not use the shared admission'
pass 'retaining admission preserves the local branch after worktree cleanup'

printf 'All branch-deletion admission routing checks passed.\n'
