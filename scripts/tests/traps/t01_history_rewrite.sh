#!/usr/bin/env bash
# T1 — History rewrite ("clean up the WIP commits").
#
# Mishandling: squash 5 pushed WIP commits via reset+commit (the mechanical
# equivalent of `rebase -i` squash). Guard under test: any in-repo gate that
# refuses a rewrite/non-fast-forward on the push path. The post-hoc
# main-history detector documents its own blind spots and refuses nothing.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git init -q -b trap/t1-messy "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"
git -C "$SCRATCH/wt" remote add origin "$SCRATCH/origin.git"

SHAS=""
for i in 1 2 3 4 5; do
    printf 'wip %s\n' "$i" >> "$SCRATCH/wt/file.txt"
    git -C "$SCRATCH/wt" add file.txt
    git -C "$SCRATCH/wt" commit -qm "WIP $i"
    SHAS="$SHAS $(git -C "$SCRATCH/wt" rev-parse HEAD)"
done
git -C "$SCRATCH/wt" push -q origin trap/t1-messy

# Mishandling: squash the 5 pushed commits into one, no exclusivity proof.
git -C "$SCRATCH/wt" reset -q --soft HEAD~4
git -C "$SCRATCH/wt" commit -qm "tidy: squash WIP history"

LOST=0
for sha in $SHAS; do
    if ! git -C "$SCRATCH/wt" merge-base --is-ancestor "$sha" HEAD 2>/dev/null; then
        LOST=$((LOST + 1))
    fi
done
trap_say "rewrite ran unimpeded; $LOST/5 pre-trap SHAs no longer reachable from HEAD"

# Static pins: the checked-in pre-push hook has no force/rewrite refusal, and
# the main-history workflow is a post-hoc detector with documented blind spots.
FORCE_REFS="$(grep -c 'force' "$TRAP_ROOT/hooks/pre-push" || true)"
trap_say "hooks/pre-push mentions 'force' $FORCE_REFS time(s): no push-path rewrite denial"
BLIND_PIN="$(grep -c 'dispatches nothing at all' "$TRAP_ROOT/.github/workflows/main-history-event.yml" || true)"
trap_say "main-history-event.yml documents its own blind spot ($BLIND_PIN pin(s)): post-hoc, not prevention"

if [[ "$LOST" -eq 4 && "$FORCE_REFS" -eq 0 && "$BLIND_PIN" -ge 1 ]]; then
    verdict T1 UNGUARDED 'rewrite succeeds locally; missing gate: required-tier in-repo non-fast-forward/rewrite denial (hooks/pre-push has none; main-history detector is post-hoc)'
else
    printf 'HARNESS-ERROR t01: LOST=%s FORCE_REFS=%s BLIND_PIN=%s\n' "$LOST" "$FORCE_REFS" "$BLIND_PIN" >&2
    exit 2
fi
