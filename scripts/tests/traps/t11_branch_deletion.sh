#!/usr/bin/env bash
# T11 — Branch deletion without admission ("it's merged, delete it").
#
# Mishandling: raw `git branch -D` on a branch that still carries unpushed
# commits. Guard under test: scripts/branch-deletion-admission, whose binary
# owns the live graph/tip/identity/worktree-ownership checks and exits
# RETAIN_EXIT_CODE=3 unless deletion is admitted. The cargo-built binary is
# pinned statically (no build in this suite); the raw-delete hole it leaves
# open is demonstrated live so the caveat is evidence, not prose.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git clone -q "$SCRATCH/origin.git" "$SCRATCH/wt" 2>/dev/null
trap_git_identity "$SCRATCH/wt"
git -C "$SCRATCH/wt" checkout -q -b main
printf 'base\n' > "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" add file.txt
git -C "$SCRATCH/wt" commit -qm base
git -C "$SCRATCH/wt" push -q origin main
git -C "$SCRATCH/wt" checkout -q -b trap/t11-stale
printf 'unpushed work\n' >> "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" commit -qam 'unpushed commit U'
UNPUSHED="$(git -C "$SCRATCH/wt" rev-parse HEAD)"
git -C "$SCRATCH/wt" checkout -q main

# Raw muscle-memory deletion: succeeds and destroys the unpushed commit.
git -C "$SCRATCH/wt" branch -q -D trap/t11-stale
if git -C "$SCRATCH/wt" rev-parse --verify -q trap/t11-stale >/dev/null 2>&1; then
    printf 'HARNESS-ERROR t11: raw branch -D unexpectedly refused\n' >&2
    exit 2
fi
trap_say "raw 'git branch -D' deleted the branch; unpushed commit $UNPUSHED orphaned"

# Static pins on the executable guard: the wrapper, the fail-closed retain
# exit code, the worktree-ownership check, the routing test, and the
# cleanup integration that routes local deletes through the admission.
WRAPPER_OK=0
[[ -x "$TRAP_ROOT/scripts/branch-deletion-admission" ]] && WRAPPER_OK=1
RETAIN_PIN="$(grep -c 'RETAIN_EXIT_CODE: i32 = 3' "$TRAP_ROOT/xtask/src/branch_deletion_admission/mod.rs" || true)"
OWNERSHIP_PIN="$(grep -c 'WorktreeOwnership' "$TRAP_ROOT/xtask/src/branch_deletion_admission/mod.rs" || true)"
ROUTING_TEST=0
[[ -f "$TRAP_ROOT/scripts/tests/test-branch-deletion-admission-routing.sh" ]] && ROUTING_TEST=1
CLEANUP_PIN="$(grep -c 'branch_deletion_admitted' "$TRAP_ROOT/scripts/cleanup-completed-worktrees.sh" || true)"
trap_say "wrapper-exec=$WRAPPER_OK retain-code pins=$RETAIN_PIN ownership pins=$OWNERSHIP_PIN routing-test=$ROUTING_TEST cleanup-integration pins=$CLEANUP_PIN"

if [[ "$WRAPPER_OK" -eq 1 && "$RETAIN_PIN" -ge 1 && "$OWNERSHIP_PIN" -ge 1 && "$ROUTING_TEST" -eq 1 && "$CLEANUP_PIN" -ge 1 ]]; then
    verdict T11 PASS 'fail-closed admission (exit 3 unless admitted) exists, is tested, and owns cleanup deletes (caveats: voluntary adoption; raw -D unblocked as shown; server-side auto-delete bypasses it)'
else
    printf 'HARNESS-ERROR t11: WRAP=%s RETAIN=%s OWN=%s ROUTE=%s CLEAN=%s\n' "$WRAPPER_OK" "$RETAIN_PIN" "$OWNERSHIP_PIN" "$ROUTING_TEST" "$CLEANUP_PIN" >&2
    exit 2
fi
