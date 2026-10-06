#!/usr/bin/env bash
# scripts/worktree-add.sh — provisioned worktree creation (issue #17406).
#
# Wraps `git worktree add` so a fresh worktree gets current git hooks by
# default, then verifies currency. The provisioning step fails LOUDLY
# (non-zero exit with the installer as the printed fix) — never silently.
#
# Honesty note: local hooks cannot be strictly enforced. Raw
# `git worktree add`, fresh clones, and `git push --no-verify` all bypass
# this wrapper. The bar is provisioned-by-default with drift detection
# (agent-preflight.sh check 7 fails on missing/stale hooks), not unbreakable.
#
# Usage (from the coordination checkout — see docs/reference/WORKTREE_PROTOCOL.md):
#   bash scripts/worktree-add.sh -b fix/<issue>-<slug> .worktrees/<short-slot> origin/main
#   bash scripts/worktree-add.sh .worktrees/<short-slot> <existing-branch>
# All arguments are forwarded unchanged to `git worktree add`.
#
# Provisioning policy:
#   - no hooks/pre-push authority in the new worktree → skip (nothing to
#     provision; e.g. a revision that predates the hook);
#   - installed pre-push already current (bytes + executable bit) → skip the
#     installer (no cargo build; linked worktrees share the common hooks dir,
#     so this is the common case);
#   - otherwise run the installer, then re-verify; any failure exits non-zero.
#
# A provisioning failure does NOT roll back the created worktree (it may hold
# the only checkout of its branch). Rerun the installer from the new worktree
# and retry.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER_HINT="bash scripts/install-githooks.sh"

if [[ $# -eq 0 ]]; then
  echo "ERROR: usage: bash scripts/worktree-add.sh <git-worktree-add args...>" >&2
  exit 2
fi

BEFORE_LIST="$(git worktree list --porcelain 2>/dev/null | grep '^worktree ' || true)"

if ! git worktree add "$@"; then
  echo "ERROR: git worktree add failed (see above); no worktree created, hooks not provisioned" >&2
  exit 1
fi

AFTER_LIST="$(git worktree list --porcelain 2>/dev/null | grep '^worktree ' || true)"
NEW_WT=""
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  if ! grep -qxF "$line" <<< "$BEFORE_LIST"; then
    NEW_WT="${line#worktree }"
    break
  fi
done <<< "$AFTER_LIST"

if [[ -z "$NEW_WT" ]]; then
  echo "ERROR: worktree created but the new path could not be determined; hooks NOT provisioned" >&2
  echo "    Fix: cd into the new worktree and run: $INSTALLER_HINT" >&2
  exit 1
fi

AUTHORITY="$NEW_WT/hooks/pre-push"
if [[ ! -f "$AUTHORITY" ]]; then
  echo "worktree-add: no hooks/pre-push authority in $NEW_WT — skipping hook provisioning"
  exit 0
fi

INSTALLED="$(git -C "$NEW_WT" rev-parse --git-common-dir 2>/dev/null)/hooks/pre-push"

# True when the installed hook matches the checked-in authority. The
# command-substitution comparison strips trailing newlines, mirroring
# check_githooks' normalization (the installer appends one "\n" to the
# generated script, so a byte-exact diff would always report drift).
hooks_current() {
  [[ -f "$INSTALLED" ]] || return 1
  [[ "$(tr -d '\r' < "$INSTALLED")" == "$(tr -d '\r' < "$AUTHORITY")" ]] || return 1
  case "$(uname -s 2>/dev/null || echo unknown)" in
    MINGW* | MSYS* | CYGWIN*)
      return 0
      ;; # Windows has no exec-bit semantics (mirrors check_githooks' cfg gate)
    *)
      [[ -x "$INSTALLED" ]] || return 1
      ;;
  esac
  return 0
}

if hooks_current; then
  echo "worktree-add: git hooks already current — installer skipped (no build)"
  exit 0
fi

# Prefer the new tree's own installer so installer and authority stay at the
# same revision (a worktree cut from an old base carries its own hook
# generation). Fall back to this wrapper's installer when the tree predates
# the installer script; the verification below then fails loudly on skew
# instead of silently leaving zero hooks.
INSTALLER="$NEW_WT/scripts/install-githooks.sh"
if [[ ! -f "$INSTALLER" ]]; then
  INSTALLER="$SCRIPT_DIR/install-githooks.sh"
fi

echo "worktree-add: provisioning git hooks for $NEW_WT ..."
if ! (cd "$NEW_WT" && bash "$INSTALLER"); then
  echo "ERROR: hook provisioning FAILED for worktree $NEW_WT (installer exit non-zero)" >&2
  echo "    The worktree exists but its hooks are missing or stale." >&2
  echo "    Fix: cd $NEW_WT && $INSTALLER_HINT" >&2
  exit 1
fi

if ! hooks_current; then
  echo "ERROR: hook provisioning FAILED verification for worktree $NEW_WT" >&2
  echo "    The installer ran but $INSTALLED still does not match hooks/pre-push." >&2
  echo "    (One cause is revision skew: a worktree cut from an old base whose hook" >&2
  echo "    authority predates the installer. Check out a current base or provision manually.)" >&2
  echo "    Fix: cd $NEW_WT && $INSTALLER_HINT" >&2
  exit 1
fi

echo "worktree-add: git hooks provisioned and verified for $NEW_WT"
