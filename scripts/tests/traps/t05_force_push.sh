#!/usr/bin/env bash
# T5 — Force-push ("just get it up there").
#
# Mishandling: push --force over a teammate commit after a non-fast-forward
# rejection. Guard under test: any in-repo push-path refusal of force pushes.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git clone -q "$SCRATCH/origin.git" "$SCRATCH/local" 2>/dev/null
trap_git_identity "$SCRATCH/local"
git -C "$SCRATCH/local" checkout -q -b trap/t5-push
printf 'base\n' > "$SCRATCH/local/file.txt"
git -C "$SCRATCH/local" add file.txt
git -C "$SCRATCH/local" commit -qm base
git -C "$SCRATCH/local" push -q origin trap/t5-push

# Simulated teammate advances the remote.
git clone -q "$SCRATCH/origin.git" "$SCRATCH/mate" 2>/dev/null
trap_git_identity "$SCRATCH/mate"
git -C "$SCRATCH/mate" checkout -q trap/t5-push
printf 'teammate work\n' >> "$SCRATCH/mate/file.txt"
git -C "$SCRATCH/mate" commit -qam 'teammate commit T'
git -C "$SCRATCH/mate" push -q origin trap/t5-push
T="$(git -C "$SCRATCH/mate" rev-parse HEAD)"

# Local diverges; plain push is rejected; --force is used.
printf 'my work\n' >> "$SCRATCH/local/file.txt"
git -C "$SCRATCH/local" commit -qam 'my commit'
if git -C "$SCRATCH/local" push origin trap/t5-push >"$SCRATCH/push.txt" 2>&1; then
    printf 'HARNESS-ERROR t05: plain push unexpectedly succeeded\n' >&2
    exit 2
fi
trap_say "plain push rejected (non-fast-forward), as designed"
git -C "$SCRATCH/local" push -q --force origin trap/t5-push
trap_say "push --force accepted by fixture origin"

if git --git-dir="$SCRATCH/origin.git" merge-base --is-ancestor "$T" refs/heads/trap/t5-push 2>/dev/null; then
    printf 'HARNESS-ERROR t05: teammate commit unexpectedly still reachable\n' >&2
    exit 2
fi
trap_say "teammate commit T no longer reachable from origin/trap/t5-push: work destroyed"

FORCE_REFS="$(grep -c 'force' "$TRAP_ROOT/hooks/pre-push" || true)"
trap_say "hooks/pre-push mentions 'force' $FORCE_REFS time(s): no push-path force denial"

if [[ "$FORCE_REFS" -eq 0 ]]; then
    verdict T5 UNGUARDED 'force-push destroyed a teammate commit with no local refusal; missing gate: in-repo push-path refusal of force/non-fast-forward pushes (hooks/pre-push has no ref/force check)'
else
    printf 'HARNESS-ERROR t05: FORCE_REFS=%s\n' "$FORCE_REFS" >&2
    exit 2
fi
