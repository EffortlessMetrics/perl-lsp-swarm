#!/usr/bin/env bash
# T1 — History rewrite ("clean up the WIP commits").
#
# Mishandling: squash 5 pushed WIP commits via reset+commit (the mechanical
# equivalent of `rebase -i` squash), then force-push the rewrite. Guard under
# test: hooks/pre-push ref-update refusal (#17427), which must fail closed on
# the non-fast-forward push with a recovery order while published history
# stays intact. A fast-forward doc push through the same hook must still
# succeed. The post-hoc main-history detector documents its own blind spots
# and refuses nothing; it is evidence here, not the guard.
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
PUBLISHED="$(git -C "$SCRATCH/wt" rev-parse HEAD)"
trap_say "published 5 WIP commits; remote tip $PUBLISHED"

# Mishandling: squash the 5 pushed commits into one, no exclusivity proof.
git -C "$SCRATCH/wt" reset -q --soft HEAD~4
git -C "$SCRATCH/wt" commit -qm "tidy: squash WIP history"

LOST=0
for sha in $SHAS; do
    if ! git -C "$SCRATCH/wt" merge-base --is-ancestor "$sha" HEAD 2>/dev/null; then
        LOST=$((LOST + 1))
    fi
done
trap_say "rewrite ran unimpeded locally; $LOST/5 pre-trap SHAs no longer reachable from HEAD"

# Static pins: the checked-in pre-push hook carries the ref-update refusal
# and its documented admit-list hatch; the main-history workflow is still a
# post-hoc detector with documented blind spots.
GUARD_PIN="$(grep -c 'merge-base --is-ancestor' "$TRAP_ROOT/hooks/pre-push" || true)"
HATCH_PIN="$(grep -c 'PERL_LSP_ALLOW_HISTORY_REWRITE' "$TRAP_ROOT/hooks/pre-push" || true)"
BLIND_PIN="$(grep -c 'dispatches nothing at all' "$TRAP_ROOT/.github/workflows/main-history-event.yml" || true)"
trap_say "hooks/pre-push ref-update guard pins=$GUARD_PIN hatch pins=$HATCH_PIN; main-history blind-spot pins=$BLIND_PIN"

# Live proof: install the checked-in hook and force-push the rewrite.
cp "$TRAP_ROOT/hooks/pre-push" "$SCRATCH/wt/.git/hooks/pre-push"
chmod +x "$SCRATCH/wt/.git/hooks/pre-push"
set +e
git -C "$SCRATCH/wt" push --force origin trap/t1-messy >"$SCRATCH/force-push.txt" 2>&1
PUSH_CODE=$?
set -e
REFUSAL_PIN="$(grep -c 'Refusing non-fast-forward' "$SCRATCH/force-push.txt" || true)"
ORDER_PIN="$(grep -c 'Recover with: git fetch' "$SCRATCH/force-push.txt" || true)"
trap_say "force-push exit=$PUSH_CODE refusal pins=$REFUSAL_PIN recovery-order pins=$ORDER_PIN"

REMOTE_TIP="$(git --git-dir="$SCRATCH/origin.git" rev-parse refs/heads/trap/t1-messy)"
INTACT=0
for sha in $SHAS; do
    if git --git-dir="$SCRATCH/origin.git" merge-base --is-ancestor "$sha" refs/heads/trap/t1-messy 2>/dev/null; then
        INTACT=$((INTACT + 1))
    fi
done
trap_say "remote tip $REMOTE_TIP (published $PUBLISHED); $INTACT/5 published SHAs still reachable from origin"

# The same hook must still admit a fast-forward push: rewind to the published
# tip, add a doc-only commit (pure-git doc fast path, no cargo/just), push.
git -C "$SCRATCH/wt" reset -q --hard "$PUBLISHED"
printf 'notes\n' > "$SCRATCH/wt/notes.md"
git -C "$SCRATCH/wt" add notes.md
git -C "$SCRATCH/wt" commit -qm 'docs: trap notes'
set +e
git -C "$SCRATCH/wt" push origin trap/t1-messy >"$SCRATCH/ff-push.txt" 2>&1
FF_CODE=$?
set -e
FF_TIP="$(git --git-dir="$SCRATCH/origin.git" rev-parse refs/heads/trap/t1-messy)"
LOCAL_TIP="$(git -C "$SCRATCH/wt" rev-parse HEAD)"
trap_say "fast-forward doc push exit=$FF_CODE remote tip $FF_TIP local tip $LOCAL_TIP"

if [[ "$LOST" -eq 4 && "$GUARD_PIN" -ge 1 && "$HATCH_PIN" -ge 1 && "$BLIND_PIN" -ge 1 \
    && "$PUSH_CODE" -ne 0 && "$REFUSAL_PIN" -ge 1 && "$ORDER_PIN" -ge 1 \
    && "$REMOTE_TIP" == "$PUBLISHED" && "$INTACT" -eq 5 \
    && "$FF_CODE" -eq 0 && "$FF_TIP" == "$LOCAL_TIP" ]]; then
    verdict T1 PASS 'force-push of rewritten history refused locally with recovery order; published remote tip intact; fast-forward doc push still succeeds (guard: hooks/pre-push ref-update refusal #17427)'
else
    printf 'HARNESS-ERROR t01: LOST=%s GUARD=%s HATCH=%s BLIND=%s PUSH=%s REF=%s ORD=%s REMOTE=%s INTACT=%s FF=%s FFTIP=%s\n' \
        "$LOST" "$GUARD_PIN" "$HATCH_PIN" "$BLIND_PIN" "$PUSH_CODE" "$REFUSAL_PIN" "$ORDER_PIN" "$REMOTE_TIP" "$INTACT" "$FF_CODE" "$FF_TIP" >&2
    exit 2
fi
