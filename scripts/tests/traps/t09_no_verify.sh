#!/usr/bin/env bash
# T9 — `--no-verify` bypass ("let CI sort it out").
#
# Mishandling: commit and push with --no-verify in a worktree whose hooks
# were never installed (the shipped default). Guard under test: mandatory
# hook provisioning plus a gate that cannot be bypassed this way.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git clone -q "$SCRATCH/origin.git" "$SCRATCH/wt" 2>/dev/null
trap_git_identity "$SCRATCH/wt"
git -C "$SCRATCH/wt" checkout -q -b trap/t9-bypass

ACTIVE_HOOKS="$(find "$SCRATCH/wt/.git/hooks" -type f ! -name '*.sample' | wc -l)"
trap_say "active (non-sample) hooks in fresh fixture worktree: $ACTIVE_HOOKS"

printf 'change\n' > "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" add file.txt
git -C "$SCRATCH/wt" commit -q --no-verify -m 'bypass commit gate'
git -C "$SCRATCH/wt" push -q --no-verify origin trap/t9-bypass
trap_say "commit --no-verify and push --no-verify both succeeded with no hooks installed"

# Static pins: the repo documents --no-verify as a skip hatch and ships no
# auto-install (hooks/ carries only pre-push; install is a manual xtask call).
BYPASS_DOCS="$(grep -c -- '--no-verify' "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" || true)"
SHIPPED_HOOKS="$(ls "$TRAP_ROOT/hooks" | tr '\n' ' ')"
INSTALL_ADVISORY="$(grep -c 'install-githooks' "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" || true)"
trap_say "--no-verify skip-hatch mentions in git_hooks.rs=$BYPASS_DOCS; shipped hooks=[$SHIPPED_HOOKS]; install-githooks refs=$INSTALL_ADVISORY"

if [[ "$ACTIVE_HOOKS" -eq 0 && "$BYPASS_DOCS" -ge 1 ]]; then
    verdict T9 UNGUARDED 'no-verify commit+push ran with no hooks and no fallback; missing gate: mandatory hook provisioning at worktree setup + a non-bypassable enforcement path'
else
    printf 'HARNESS-ERROR t09: ACTIVE=%s BYPASS=%s\n' "$ACTIVE_HOOKS" "$BYPASS_DOCS" >&2
    exit 2
fi
