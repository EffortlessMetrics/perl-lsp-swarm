#!/usr/bin/env bash
# T7 — `git stash` in the shared worktree.
#
# Mishandling: stash WIP to "quickly switch branches". Guard under test:
# scripts/agent-preflight.sh check 6, which must exit 6 on any stash entry.
# The fixture owns its .git directory, so its stash list is fixture-local and
# the real shared refs/stash is never touched.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q -b main "$SCRATCH/main"
trap_git_identity "$SCRATCH/main"
printf 'base\n' > "$SCRATCH/main/file.txt"
git -C "$SCRATCH/main" add file.txt
git -C "$SCRATCH/main" commit -qm base
# A linked worktree satisfies preflight checks 2 (isolation) and 4 (cwd).
git -C "$SCRATCH/main" worktree add -q "$SCRATCH/wt" -b trap/t7-stash
trap_git_identity "$SCRATCH/wt"
printf 'wip work in progress\n' >> "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" stash push -q -m 'trap t7 wip'
trap_say "fixture stash list: $(git -C "$SCRATCH/wt" stash list | wc -l) entr(y/ies)"

set +e
(cd "$SCRATCH/wt" && env -u CARGO_TARGET_DIR bash "$TRAP_ROOT/scripts/agent-preflight.sh" >"$SCRATCH/preflight.txt" 2>&1)
CODE=$?
set -e
trap_say "agent-preflight.sh exit code: $CODE"
SHARED_PIN="$(grep -c 'SHARED across all worktrees' "$SCRATCH/preflight.txt" || true)"
DANGEROUS_ADVICE="$(grep -cE 'stash (pop|drop|clear)' "$SCRATCH/preflight.txt" || true)"
trap_say "shared-stash warning pins=$SHARED_PIN; dangerous pop/drop/clear advice=$DANGEROUS_ADVICE"

if [[ "$CODE" -eq 6 && "$SHARED_PIN" -ge 1 && "$DANGEROUS_ADVICE" -eq 0 ]]; then
    verdict T7 PASS 'preflight check 6 exits 6 on stash entries with safe guidance (caveat: voluntary invocation; nothing forces preflight to run)'
else
    printf 'HARNESS-ERROR t07: CODE=%s SHARED=%s ADVICE=%s\n' "$CODE" "$SHARED_PIN" "$DANGEROUS_ADVICE" >&2
    exit 2
fi
