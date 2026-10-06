#!/usr/bin/env bash
# T8 — Push straight to main ("skip the PR ceremony").
#
# Mishandling: do agent work directly on main. Guard under test:
# scripts/agent-preflight.sh check 1, which must exit 1 on a protected
# branch. Companion pin: hooks/pre-push itself has no ref-name gate, so the
# preflight refusal is the only in-repo executable signal.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q -b main "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"
printf 'base\n' > "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" add file.txt
git -C "$SCRATCH/wt" commit -qm base

set +e
(cd "$SCRATCH/wt" && env -u CARGO_TARGET_DIR bash "$TRAP_ROOT/scripts/agent-preflight.sh" >"$SCRATCH/preflight.txt" 2>&1)
CODE=$?
set -e
trap_say "agent-preflight.sh exit code on main: $CODE"
BRANCH_PIN="$(grep -c "On protected branch 'main'" "$SCRATCH/preflight.txt" || true)"
trap_say "protected-branch refusal pins=$BRANCH_PIN"

# hooks/pre-push: no non-comment line names a protected ref as a gate.
HOOK_REF_GATE="$(grep -vE '^\s*#' "$TRAP_ROOT/hooks/pre-push" | grep -cE "refs/heads/(main|master)|protected branch" || true)"
trap_say "hooks/pre-push non-comment protected-ref gate lines: $HOOK_REF_GATE"

if [[ "$CODE" -eq 1 && "$BRANCH_PIN" -ge 1 && "$HOOK_REF_GATE" -eq 0 ]]; then
    verdict T8 PASS 'preflight check 1 exits 1 on main (caveat: voluntary invocation; hooks/pre-push has no ref-name gate of its own)'
else
    printf 'HARNESS-ERROR t08: CODE=%s PIN=%s HOOK=%s\n' "$CODE" "$BRANCH_PIN" "$HOOK_REF_GATE" >&2
    exit 2
fi
