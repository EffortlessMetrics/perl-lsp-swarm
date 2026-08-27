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

# Every local branch delete must be guarded by the shared admission.
#
# The previous form of this check was `git(-C [^ ]+)? branch -[dD]`, which has
# no space between `git` and `-C` and therefore could never match the real call
# `git -C "$REPO_ROOT" branch -D`. It passed while an unguarded delete sat in
# cleanup-completed-worktrees.sh. The regex below is verified against a literal
# sample first, so a future edit cannot silently make it vacuous again.
BRANCH_DELETE_RE='git( +-C +[^ ]+)? +branch +-[dD]'
printf 'git_out git -C "$REPO_ROOT" branch -D "$branch" || true\n' \
    | grep -qE "$BRANCH_DELETE_RE" \
    || fail 'the branch-delete detector does not match a real call; the check would be vacuous'
printf 'git branch -D foo\n' | grep -qE "$BRANCH_DELETE_RE" \
    || fail 'the branch-delete detector misses the bare form'
printf 'git branch --list\n' | grep -qE "$BRANCH_DELETE_RE" \
    && fail 'the branch-delete detector matches a non-delete command'
pass 'the branch-delete detector matches real calls and only real calls'

# Scope the check to the enclosing function, not the file. A file-level grep
# passes as soon as the admission is mentioned anywhere, so deleting the guard
# CALL while leaving its helper defined would slip through — the same vacuity
# the detector above exists to prevent.
for file in scripts/cleanup-completed-worktrees.sh scripts/swarm-clean; do
    unguarded="$(awk -v re="$BRANCH_DELETE_RE" '
        /^[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(\)[[:space:]]*\{/ {
            fn = $0; sub(/[[:space:]]*\(\).*/, "", fn)
            infn = 1; deletes = 0; guarded = 0; next
        }
        infn && /^\}/ {
            if (deletes && !guarded) print fn
            infn = 0; next
        }
        infn {
            if ($0 ~ re) deletes = 1
            if ($0 ~ /branch_deletion_admitted/) guarded = 1
        }
    ' "$ROOT/$file")"
    if [[ -n "$unguarded" ]]; then
        fail "$file: these functions delete a local branch without the shared admission: $unguarded"
    fi
    pass "$file guards every local branch delete with the shared admission"
done

REPO="$TMP/repo"
git init -q --bare "$REPO.git"
git init -q -b main "$REPO"
git -C "$REPO" config user.email test@example.invalid
git -C "$REPO" config user.name test
# The helper binds its PR lookup to origin and refuses when it cannot derive
# one, so every fixture needs a real origin.
git -C "$REPO" remote add origin "$REPO.git"
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

# An ADMITTED branch whose local tip never reached the remote must still be
# retained: the admission covers the REMOTE branch, and unpushed local commits
# are unsalvaged work no admission authorized. This is the case a `-d`/`-D`
# ladder cannot catch — squash-merge leaves the branch unmerged by
# reachability, so `-d` always refuses and the fallback to `-D` deletes anyway.
#
# Both repos below have a real origin so the check discriminates on tip
# divergence, not on an unreadable remote.
setup_admitted_repo() {
    local repo="$1" wt="$2" branch="$3" extra_local_commit="$4"
    git init -q --bare "$repo.git"
    git init -q -b main "$repo"
    git -C "$repo" config user.email test@example.invalid
    git -C "$repo" config user.name test
    git -C "$repo" remote add origin "$repo.git"
    printf 'init\n' > "$repo/file"
    git -C "$repo" add file
    git -C "$repo" commit -qm init
    git -C "$repo" push -q origin main
    git -C "$repo" worktree add -q -b "$branch" "$wt"
    printf 'feature\n' > "$wt/file"
    git -C "$wt" add file
    git -C "$wt" commit -qm feature
    # Push BEFORE any extra commit, so origin holds the earlier tip.
    git -C "$wt" push -q origin "$branch"
    if [[ "$extra_local_commit" == "yes" ]]; then
        printf 'unpushed\n' >> "$wt/file"
        git -C "$wt" add file
        git -C "$wt" commit -qm 'local commit that never reached origin'
    fi
    # Merge LAST so the branch is reachable from main either way and the
    # worktree still classifies clean-finished; only the origin tip differs.
    git -C "$repo" merge --no-ff -qm "merge-$branch" "$branch"
}

cat > "$TMP/bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == "--version" ]]; then
  printf 'cargo 1.95.0 (0000000000 2026-01-01)\n'
  exit 0
fi
printf '%s\n' "$*" > "${ADMISSION_LOG}"
exit 0
STUB
chmod +x "$TMP/bin/cargo"

# Diverged local tip -> retained.
setup_admitted_repo "$TMP/diverged" "$TMP/wt-diverged" feature/unpushed yes
PATH="$TMP/bin:$PATH" ADMISSION_LOG="$TMP/admission-diverged.log" \
REPO_ROOT="$TMP/diverged" bash "$ROOT/scripts/swarm-clean" --apply >/dev/null 2>&1 || true
grep -qF 'plan --pr 123 --remote origin' "$TMP/admission-diverged.log" \
    || fail 'the diverged scenario never reached the shared admission'
git -C "$TMP/diverged" show-ref --verify --quiet refs/heads/feature/unpushed \
    || fail 'an admitted branch whose local tip never reached origin was deleted'
pass 'an admitted branch with an unpushed local tip is retained'

# Positive control: local tip == origin tip -> deleted. Without this the test
# above would pass even if the helper retained unconditionally.
setup_admitted_repo "$TMP/insync" "$TMP/wt-insync" feature/insync no
PATH="$TMP/bin:$PATH" ADMISSION_LOG="$TMP/admission-insync.log" \
REPO_ROOT="$TMP/insync" bash "$ROOT/scripts/swarm-clean" --apply >/dev/null 2>&1 || true
if git -C "$TMP/insync" show-ref --verify --quiet refs/heads/feature/insync; then
    fail 'positive control: an admitted branch already at the origin tip was not deleted'
fi
pass 'an admitted branch already at the origin tip is deleted'

printf 'All branch-deletion admission routing checks passed.\n'
