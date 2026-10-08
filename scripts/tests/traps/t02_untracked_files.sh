#!/usr/bin/env bash
# T2 -- Untracked user files ("tidy the tree" via safe-pull.sh).
#
# Mishandling: run the repo's own scripts/safe-pull.sh in a worktree holding
# a valuable untracked file that collides with an incoming path.
#
# Two tree shapes are tested, because the guard for this vector is in flight
# (issue #17403, repair PR #17407):
#   - pre-guard: the script carries a bare `rm -f` for colliding untracked
#     files with no salvage packet, backup, or confirm. Live finding: the
#     victim-parse pipeline (`grep -E '^\t'`) matches nothing on GNU grep
#     3.x (measured on WSL grep 3.11; the escape is not interpreted in
#     ERE), so under `set -euo pipefail` the empty parse kills the script
#     with a silent exit 1 before `rm -f` runs -- which incidentally
#     preserves the file while also breaking pull-through-conflict. The
#     trap pins both facts: the full run preserves the file by accident,
#     and the corrected (real-tab) parse still names the victim file for
#     an unsalvaged `rm -f` that goes live with any future parse fix that
#     forgets salvage.
#   - post-guard: the script salvages colliding untracked files into a
#     packet under .git/safe-pull-salvage/, retries the merge, and exits 0.
#     The trap verifies the original bytes landed in the packet and the
#     worktree received the upstream content.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"

# Clone A publishes a base commit; clone B starts from it with a clean index.
git clone -q "$SCRATCH/origin.git" "$SCRATCH/a" 2>/dev/null
trap_git_identity "$SCRATCH/a"
git -C "$SCRATCH/a" checkout -q -b main
printf 'base\n' > "$SCRATCH/a/README.md"
git -C "$SCRATCH/a" add README.md
git -C "$SCRATCH/a" commit -qm 'base commit'
git -C "$SCRATCH/a" push -q origin main
git clone -q "$SCRATCH/origin.git" "$SCRATCH/b" 2>/dev/null
trap_git_identity "$SCRATCH/b"
git -C "$SCRATCH/b" checkout -q main

# Clone B holds valuable UNTRACKED work at a path the remote is about to
# add. The index stays clean: no staged dirt, so the collision is purely
# untracked-vs-incoming and the post-guard script can complete the pull.
mkdir -p "$SCRATCH/b/gen"
printf 'MY UNSAVED BENCHMARK RESULTS - months of work\n' > "$SCRATCH/b/gen/output.txt"
BEFORE="$(sha256sum "$SCRATCH/b/gen/output.txt" | cut -d' ' -f1)"

# Advance origin so clone B is behind and the merge collides.
mkdir -p "$SCRATCH/a/gen"
printf 'upstream generated content\n' > "$SCRATCH/a/gen/output.txt"
git -C "$SCRATCH/a" add gen/output.txt
git -C "$SCRATCH/a" commit -qm 'publish generated output'
git -C "$SCRATCH/a" push -q origin main
REMOTE_HASH="$(git -C "$SCRATCH/a" show HEAD:gen/output.txt | sha256sum | cut -d' ' -f1)"

# Full run, exactly as an agent would invoke it.
set +e
(cd "$SCRATCH/b" && bash "$TRAP_ROOT/scripts/safe-pull.sh" main >"$SCRATCH/out.txt" 2>&1)
HELPER_EXIT=$?
set -e
AFTER=""
if [[ -f "$SCRATCH/b/gen/output.txt" ]]; then
    AFTER="$(sha256sum "$SCRATCH/b/gen/output.txt" | cut -d' ' -f1)"
fi
trap_say "safe-pull exit=$HELPER_EXIT; untracked file present=$([ -n "$AFTER" ] && echo yes || echo no); intact=$([ "$BEFORE" = "$AFTER" ] && echo yes || echo no)"

# The conflict-parse pipeline, run against a real merge failure: with a
# portable real-tab match it names the victim file the `rm -f` block would
# delete. safe-pull.sh itself uses a literal `grep -E '^\t'` (pinned below
# as PROD_PARSE): GNU grep does not interpret that escape in ERE (verified:
# no match on WSL grep 3.11 and Git-for-Windows grep 3.0), so the trap
# builds a real tab via printf to show the victim the production pipeline
# is TRYING to select. Combined with `set -euo pipefail` (STRICT_PIN), the
# unmatchable parse is also the incidental exit-1 that preserves the file.
TAB="$(printf '\t')"
MERGE_FAIL="$(cd "$SCRATCH/b" && git merge origin/main 2>&1 || true)"
VICTIMS="$(printf '%s' "$MERGE_FAIL" | grep -E "^$TAB" | sed "s/^$TAB//" || true)"
trap_say "conflict-parse pipeline victims: [$VICTIMS]"

RM_REFS="$(grep -c 'rm -f' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
SALVAGE_REFS="$(grep -ciE 'salvage|backup|confirm' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
STRICT_PIN="$(grep -c 'set -euo pipefail' "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
PROD_PARSE="$(grep -cF "grep -E '^\t'" "$TRAP_ROOT/scripts/safe-pull.sh" || true)"
trap_say "safe-pull.sh: bare 'rm -f' x$RM_REFS, salvage/backup/confirm x$SALVAGE_REFS, strict-mode pins x$STRICT_PIN, production-parse pins x$PROD_PARSE"

# Post-guard shape: exit 0, no rm, salvage packet holds the original bytes,
# worktree holds the upstream bytes.
PACKET_COUNT=0
PACKET_HASH=""
if [[ -d "$SCRATCH/b/.git/safe-pull-salvage" ]]; then
    PACKET_COUNT="$(find "$SCRATCH/b/.git/safe-pull-salvage" -name output.txt | wc -l)"
    if [[ "$PACKET_COUNT" -eq 1 ]]; then
        PACKET_HASH="$(sha256sum "$(find "$SCRATCH/b/.git/safe-pull-salvage" -name output.txt)" | cut -d' ' -f1)"
    fi
fi
trap_say "salvage packets holding output.txt: $PACKET_COUNT"

# The guard's promise is byte survival, not pull success: the original
# bytes must remain in the worktree or in exactly one packet. A
# delete-then-retry script re-creates the path via the merge, so "file
# exists" alone proves nothing -- only the byte comparison counts.
WORKTREE_INTACT=0
[[ -n "$AFTER" && "$AFTER" == "$BEFORE" ]] && WORKTREE_INTACT=1
PACKET_INTACT=0
[[ -n "$PACKET_HASH" && "$PACKET_HASH" == "$BEFORE" ]] && PACKET_INTACT=1
trap_say "original bytes intact in worktree=$WORKTREE_INTACT packet=$PACKET_INTACT"

if [[ "$SALVAGE_REFS" -ge 1 && "$WORKTREE_INTACT" -eq 0 && "$PACKET_INTACT" -eq 0 ]]; then
    verdict T2 FAIL 'salvage guard is present but the original bytes are lost from both worktree and packet (guard claimed but bypassed)'
elif [[ "$WORKTREE_INTACT" -eq 0 && "$PACKET_INTACT" -eq 0 ]]; then
    verdict T2 UNGUARDED 'the bare rm -f EXECUTED: colliding untracked file destroyed with no salvage; missing gate: salvage packet / backup / confirm in scripts/safe-pull.sh'
elif [[ "$HELPER_EXIT" -eq 0 && "$RM_REFS" -eq 0 && "$SALVAGE_REFS" -ge 1 && "$AFTER" == "$REMOTE_HASH" && "$PACKET_COUNT" -eq 1 && "$PACKET_HASH" == "$BEFORE" ]]; then
    verdict T2 PASS 'collision salvaged to a packet with original bytes intact, pull completed (guard: safe-pull salvage; caveat: salvage must be discovered and restored by hand)'
elif [[ "$HELPER_EXIT" -ne 0 && "$BEFORE" == "$AFTER" && "$VICTIMS" == *'gen/output.txt'* && "$RM_REFS" -ge 1 && "$SALVAGE_REFS" -eq 0 && "$PROD_PARSE" -ge 1 && "$STRICT_PIN" -ge 1 ]]; then
    verdict T2 UNGUARDED 'no salvage/confirm guards the bare rm -f of colliding untracked files (file survives today only because the unmatchable victim-parse dies silently under set -e/pipefail); missing gate: salvage packet / backup / confirm in scripts/safe-pull.sh'
else
    printf 'HARNESS-ERROR t02: EXIT=%s SAME=%s VICTIMS=%s RM=%s SALVAGE=%s PROD=%s PACKET=%s\n' "$HELPER_EXIT" "$([ "$BEFORE" = "$AFTER" ] && echo yes || echo no)" "$VICTIMS" "$RM_REFS" "$SALVAGE_REFS" "$PROD_PARSE" "$PACKET_COUNT" >&2
    exit 2
fi
