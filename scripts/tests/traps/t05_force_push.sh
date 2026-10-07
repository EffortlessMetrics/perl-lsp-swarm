#!/usr/bin/env bash
# T5 — Force-push ("just get it up there").
#
# Mishandling: push --force over a teammate commit after a non-fast-forward
# rejection. Guard under test: hooks/pre-push ref-update refusal (#17427),
# which must fail closed locally with a recovery order while the teammate
# commit stays reachable. Control: the same --force with the documented
# admit-list hatch naming a throwaway ref must proceed, proving the refusal
# came from the hook and the hatch works end to end.
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

# Local diverges; plain push is rejected; --force is the mishandling.
printf 'my work\n' >> "$SCRATCH/local/file.txt"
git -C "$SCRATCH/local" commit -qam 'my commit'
if git -C "$SCRATCH/local" push origin trap/t5-push >"$SCRATCH/push.txt" 2>&1; then
    printf 'HARNESS-ERROR t05: plain push unexpectedly succeeded\n' >&2
    exit 2
fi
trap_say "plain push rejected (non-fast-forward), as designed"

# Throwaway ref for the admit-list control, set up while hookless. Both
# sides diverge doc-only so the admitted push takes the hook's pure-git doc
# fast path (no cargo/just) and the control measures the hatch, not a gate.
BASE="$(git -C "$SCRATCH/local" rev-parse HEAD~1)"
git -C "$SCRATCH/local" checkout -q -b trap/t5-hatch "$BASE"
git -C "$SCRATCH/local" push -q origin trap/t5-hatch
git -C "$SCRATCH/mate" fetch -q origin
git -C "$SCRATCH/mate" checkout -q trap/t5-hatch
printf 'mate notes\n' > "$SCRATCH/mate/mate.md"
git -C "$SCRATCH/mate" add mate.md
git -C "$SCRATCH/mate" commit -qm 'mate hatch note'
git -C "$SCRATCH/mate" push -q origin trap/t5-hatch
git -C "$SCRATCH/local" checkout -q trap/t5-hatch
printf 'local notes\n' > "$SCRATCH/local/local.md"
git -C "$SCRATCH/local" add local.md
git -C "$SCRATCH/local" commit -qm 'local hatch note'
HATCH_LOCAL="$(git -C "$SCRATCH/local" rev-parse HEAD)"

# Static pin: the checked-in hook carries the ref-update refusal.
GUARD_PIN="$(grep -c 'merge-base --is-ancestor' "$TRAP_ROOT/hooks/pre-push" || true)"
trap_say "hooks/pre-push ref-update guard pins=$GUARD_PIN"

# Live proof: install the checked-in hook and attempt the force-push.
cp "$TRAP_ROOT/hooks/pre-push" "$SCRATCH/local/.git/hooks/pre-push"
chmod +x "$SCRATCH/local/.git/hooks/pre-push"
set +e
git -C "$SCRATCH/local" push --force origin trap/t5-push >"$SCRATCH/force-push.txt" 2>&1
PUSH_CODE=$?
set -e
REFUSAL_PIN="$(grep -c 'Refusing non-fast-forward' "$SCRATCH/force-push.txt" || true)"
ORDER_PIN="$(grep -c 'PERL_LSP_ALLOW_HISTORY_REWRITE' "$SCRATCH/force-push.txt" || true)"
trap_say "force-push exit=$PUSH_CODE refusal pins=$REFUSAL_PIN recovery-order pins=$ORDER_PIN"

T_INTACT=0
if git --git-dir="$SCRATCH/origin.git" merge-base --is-ancestor "$T" refs/heads/trap/t5-push 2>/dev/null; then
    T_INTACT=1
fi
trap_say "teammate commit T reachable from origin/trap/t5-push: $T_INTACT"

# Control: the documented hatch admits the throwaway ref through the same hook.
# Fetch first so the remote objects exist locally and the hook's doc-diff can
# resolve; the push is still non-fast-forward, so only the hatch admits it.
git -C "$SCRATCH/local" fetch -q origin
set +e
PERL_LSP_ALLOW_HISTORY_REWRITE="refs/heads/trap/t5-hatch" \
    git -C "$SCRATCH/local" push --force origin trap/t5-hatch >"$SCRATCH/hatch-push.txt" 2>&1
HATCH_CODE=$?
set -e
HATCH_REMOTE="$(git --git-dir="$SCRATCH/origin.git" rev-parse refs/heads/trap/t5-hatch)"
trap_say "admitted force-push exit=$HATCH_CODE remote tip $HATCH_REMOTE local tip $HATCH_LOCAL"

if [[ "$GUARD_PIN" -ge 1 && "$PUSH_CODE" -ne 0 && "$REFUSAL_PIN" -ge 1 && "$ORDER_PIN" -ge 1 \
    && "$T_INTACT" -eq 1 && "$HATCH_CODE" -eq 0 && "$HATCH_REMOTE" == "$HATCH_LOCAL" ]]; then
    verdict T5 PASS 'force-push over teammate commit refused locally with recovery order; teammate commit intact; admit-list hatch verified on a throwaway ref (guard: hooks/pre-push ref-update refusal #17427)'
else
    printf 'HARNESS-ERROR t05: GUARD=%s PUSH=%s REF=%s ORD=%s T=%s HATCH=%s HREMOTE=%s\n' \
        "$GUARD_PIN" "$PUSH_CODE" "$REFUSAL_PIN" "$ORDER_PIN" "$T_INTACT" "$HATCH_CODE" "$HATCH_REMOTE" >&2
    exit 2
fi
