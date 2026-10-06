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
#     installer (no cargo build; reachable in the mixed-mode window where
#     core.hooksPath is not yet set and the shared common-dir hooks are
#     current — otherwise each tree owns its hooks and fresh trees always
#     provision, see issue #17414 rule C);
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

# Resolve the destination path from the forwarded `git worktree add` args.
# A before/after snapshot delta of the shared worktree list would race
# concurrent adds (another caller's path could be selected); the args are
# per-call. Prints the path, or nothing when none is found.
resolve_wt_path() {
  local prev="" arg
  local seen_dashdash=false
  for arg in "$@"; do
    if $seen_dashdash; then printf '%s\n' "$arg"; return 0; fi
    if [[ "$prev" == "skip" ]]; then prev=""; continue; fi
    case "$arg" in
      --) seen_dashdash=true ;;
      -b|-B|--reason) prev="skip" ;; # value-taking options consume the next arg
      --*=*) ;;                       # --opt=value never consumes the next arg
      --*) ;;                         # boolean long flags (--force, --no-checkout, ...)
      -b?*|-B?*) ;;                   # attached value (-bfoo): self-contained
      -?) ;;                          # single boolean short flag (-f, -d, -q)
      -??*)
        # Combined short cluster (-qf, -fb): boolean unless it ends in the
        # value-taking b/B, which consumes the next arg.
        case "$arg" in *b|*B) prev="skip";; esac
        ;;
      *) printf '%s\n' "$arg"; return 0 ;;
    esac
  done
  return 1
}

if ! git worktree add "$@"; then
  echo "ERROR: git worktree add failed (see above); no worktree created, hooks not provisioned" >&2
  exit 1
fi

NEW_WT="$(resolve_wt_path "$@" || true)"
if [[ -z "$NEW_WT" ]]; then
  echo "ERROR: worktree created but the new path could not be determined from the arguments; hooks NOT provisioned" >&2
  echo "    Fix: cd into the new worktree and run: $INSTALLER_HINT" >&2
  exit 1
fi

AUTHORITY="$NEW_WT/hooks/pre-push"
if [[ ! -f "$AUTHORITY" ]]; then
  echo "worktree-add: no hooks/pre-push authority in $NEW_WT — skipping hook provisioning"
  exit 0
fi

# --git-path honors the installer's repo-local core.hooksPath, so this follows
# the new tree's own hooks dir (#17414 rule C). --path-format=absolute is
# load-bearing: with a relative hooksPath the bare output is relative to the
# -C dir while this script runs elsewhere.
#
# Resolved twice: the installer may set core.hooksPath as it runs, which moves
# the answer from the common dir to the new tree's own dir mid-provision.
resolve_installed() {
  INSTALLED="$(git -C "$NEW_WT" rev-parse --path-format=absolute --git-path hooks 2>/dev/null)/pre-push"
  INSTALLED_COMMIT="$(git -C "$NEW_WT" rev-parse --path-format=absolute --git-path hooks 2>/dev/null)/pre-commit"
}
resolve_installed

# True when the installed hooks are good. pre-push is compared against the
# checked-in authority (command-substitution comparison strips trailing
# newlines, mirroring check_githooks' normalization: the installer appends
# one "\n" to the generated script, so a byte-exact diff would always report
# drift). pre-commit has no checked-in authority (its bytes are generated
# inside the installer), so it gets a presence + executable check instead:
# the installer always writes both hooks in one invocation, so a current
# pre-push implies a current pre-commit unless the latter was deleted or
# de-executed outright.
hooks_current() {
  [[ -f "$INSTALLED" ]] || return 1
  [[ "$(tr -d '\r' < "$INSTALLED")" == "$(tr -d '\r' < "$AUTHORITY")" ]] || return 1
  [[ -f "$INSTALLED_COMMIT" ]] || return 1
  case "$(uname -s 2>/dev/null || echo unknown)" in
    MINGW* | MSYS* | CYGWIN*)
      return 0
      ;; # Windows has no exec-bit semantics (mirrors check_githooks' cfg gate)
    *)
      [[ -x "$INSTALLED" && -x "$INSTALLED_COMMIT" ]] || return 1
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

resolve_installed
if ! hooks_current; then
  echo "ERROR: hook provisioning FAILED verification for worktree $NEW_WT" >&2
  echo "    The installer ran but $INSTALLED still does not match hooks/pre-push," >&2
  echo "    or $INSTALLED_COMMIT is missing/not executable." >&2
  echo "    (One cause is revision skew: a worktree cut from an old base whose hook" >&2
  echo "    authority predates the installer. Check out a current base or provision manually.)" >&2
  echo "    Fix: cd $NEW_WT && $INSTALLER_HINT" >&2
  exit 1
fi

echo "worktree-add: git hooks provisioned and verified for $NEW_WT"
