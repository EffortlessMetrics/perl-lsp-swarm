#!/usr/bin/env bash
# T2 — Untracked user files ("tidy the tree" via safe-pull.sh).
#
# Mishandling: run the repo's own scripts/safe-pull.sh in a worktree holding
# a valuable untracked file that collides with an incoming path. The script
# carries a bare `rm -f` for colliding untracked files with no salvage
# packet, backup, or confirm. Live finding: that block is currently
# unreachable (the `MERGE_OUTPUT=$(...) && {...}` statement under `set -e`
# exits 1 before the conflict handling runs), which incidentally preserves
# the file while also breaking the script's pull-through-conflict job. The
# trap pins both facts: the full run preserves the file by accident, and the
# script's own conflict-parse pipeline still names the victim file for an
# unsalvaged `rm -f` that goes live with any future `|| true` fix.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"

# Clone A publishes gen/output.txt.
git clone -q "$SCRATCH/origin.git" "$SCRATCH/a" 2>/dev/null
trap_git_identity "$SCRATCH/a"
git -C "$SCRATCH/a" checkout -q -b main
mkdir -p "$SCRATCH/a/gen"
printf 'upstream generated content\n' > "$SCRATCH/a/gen/output.txt"
git -C "$SCRATCH/a" add gen/output.txt
git -C "$SCRATCH/a" commit -qm 'publish generated output'
git -C "$SCRATCH/a" push -q origin main

# Clone B holds valuable UNTRACKED work at the same path.
git clone -q "$SCRATCH/origin.git" "$SCRATCH/b" 2>/dev/null
trap_git_identity "$SCRATCH/b"
git -C "$SCRATCH/b" checkout -q main
git -C "$SCRATCH/b" rm -q --cached gen/output.txt
rm "$SCRATCH/b/gen/output.txt"
printf 'MY UNSAVED BENCHMARK RESULTS - months of work\n' > "$SCRATCH/b/gen/output.txt"
BEFORE="$(sha256sum "$SCRATCH/b/gen/output.txt" | cut -d' ' -f1)"

# Advance origin so clone B is behind and the merge collides.
printf 'upstream generated content v2\n' > "$SCRATCH/a/gen/output.txt"
git -C "$SCRATCH/a" commit -qam 'publish generated output v2'
git -C "$SCRATCH/a" push -q origin main

# Full run, exactly as an agent would invoke it.
set +e
(cd "$SCRATCH/b" && bash "$TRAP_ROOT/scripts/safe-pull.sh" main >"$SCRATCH/out.txt" 2>&1)
HELPER_EXIT=$?
set -e
AFTER="$(sha256sum "$SCRATCH/b/gen/output.txt" | cut -d' ' -f1)"
trap_say "safe-pull exit=$HELPER_EXIT; untracked file intact=$([ "$BEFORE" = "$AFTER" ] && echo yes || echo no)"

# The script's tab-indented-conflict parse, run against a real merge
# failure: it names the victim file the `rm -f` block would delete. (A
# literal `grep -E '^\t'` is used by safe-pull.sh itself, but that escape is
# not portable — Git-for-Windows grep 3.0 never matches it — so the trap
# builds a real tab via printf.)
TAB="$(printf '\t')"
MERGE_FAIL="$(cd "$SCRATCH/b" && git merge origin/main 2>&1 || true)"
VICTIMS="$(printf '%s' "$MERGE_FAIL" | grep -E "^$TAB" | sed "s/^$TAB//" || true)"
trap_say "conflict-parse pipeline victims: [$VICTIMS]"

RM_REFS="$(grep -c 'rm -f' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
SALVAGE_REFS="$(grep -ciE 'salvage|backup|confirm' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
SET_E_TRAP="$(grep -c 'MERGE_OUTPUT=$(git merge' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
trap_say "safe-pull.sh: bare 'rm -f' x$RM_REFS, salvage/backup/confirm x$SALVAGE_REFS, set-e-early-exit pins x$SET_E_TRAP"

if [[ "$HELPER_EXIT" -ne 0 && "$BEFORE" == "$AFTER" && "$VICTIMS" == *'gen/output.txt'* && "$RM_REFS" -ge 1 && "$SALVAGE_REFS" -eq 0 ]]; then
    verdict T2 UNGUARDED 'no salvage/confirm guards the bare rm -f of colliding untracked files (file survives today only via an incidental set -e early exit); missing gate: salvage packet / backup / confirm in scripts/safe-pull.sh'
else
    printf 'HARNESS-ERROR t02: EXIT=%s SAME=%s VICTIMS=%s RM=%s SALVAGE=%s\n' "$HELPER_EXIT" "$([ "$BEFORE" = "$AFTER" ] && echo yes || echo no)" "$VICTIMS" "$RM_REFS" "$SALVAGE_REFS" >&2
    exit 2
fi
