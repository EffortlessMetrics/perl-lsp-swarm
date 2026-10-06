#!/usr/bin/env bash
# Safe git pull that handles untracked file conflicts and stale branch tracking.
#
# Problem: `git pull` fails when the remote adds files that already exist locally
# as untracked files (common with generated files, worktree leftovers, etc.).
# Additionally, `@{u}` (upstream tracking ref) can be stale or missing, causing
# scripts that rely on it to fail silently.
#
# Colliding untracked files are moved to a timestamped salvage packet under the
# git dir (never deleted) before the merge is retried, so the pull is recoverable.
#
# Usage:
#   scripts/safe-pull.sh              # pull from origin/main
#   scripts/safe-pull.sh my-branch    # pull from origin/my-branch
set -euo pipefail

BRANCH="${1:-main}"
REMOTE="origin"

# Ensure we are inside a git repository
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "ERROR: Not inside a git repository."
  exit 1
fi

echo "==> Fetching ${REMOTE}..."
if ! git fetch "${REMOTE}" "${BRANCH}"; then
  echo "ERROR: Failed to fetch ${REMOTE}/${BRANCH} (network issue or remote not configured)."
  exit 1
fi

# Use explicit remote ref instead of @{u} to avoid stale tracking issues
LOCAL_HEAD=$(git rev-parse HEAD)
REMOTE_HEAD=$(git rev-parse "${REMOTE}/${BRANCH}")

if [ "${LOCAL_HEAD}" = "${REMOTE_HEAD}" ]; then
  echo "Already up to date."
  exit 0
fi

# Show what would change
BEHIND=$(git rev-list HEAD.."${REMOTE}/${BRANCH}" --count)
echo "==> ${BEHIND} commit(s) behind ${REMOTE}/${BRANCH}"

# Try a merge, capturing both stdout and stderr in a single attempt.
# IMPORTANT: We do NOT attempt the merge twice. The first failed merge can leave
# git in a merge-in-progress state (for content conflicts), so re-running merge
# would get "You have not concluded your current merge" instead of the real error.
MERGE_OUTPUT=$(git merge "${REMOTE}/${BRANCH}" 2>&1) && {
  echo "Pull succeeded."
  exit 0
}

# Merge failed — check what kind of failure
if echo "${MERGE_OUTPUT}" | grep -q "would be overwritten by merge"; then
  # Untracked file conflicts: git refused to start the merge, so no cleanup needed.
  # Extract conflicting file paths from the error message.
  # Git formats these as tab-indented file paths between the error header and footer.
  # NOTE: `\t` is not a portable regex tab escape — GNU grep ERE does not
  # interpret it (verified on grep 3.0 and 3.11), so match a literal tab.
  # The `|| true` keeps `set -e`/`pipefail` from exiting inside this
  # assignment when nothing matches, so the graceful fallback below stays
  # reachable instead of dying silently with no message.
  TAB="$(printf '\t')"
  CONFLICTING_FILES=$(echo "${MERGE_OUTPUT}" \
    | grep -E "^${TAB}" \
    | sed "s/^${TAB}//" || true)

  if [ -z "${CONFLICTING_FILES}" ]; then
    echo "ERROR: Detected untracked file conflict but could not parse file list."
    echo "Raw output:"
    echo "${MERGE_OUTPUT}"
    exit 1
  fi

  # Salvage boundary (issue #17403): colliding paths are UNTRACKED, so git has
  # no record of their content. Move each one into a timestamped salvage packet
  # (same packet convention as clean-worktrees.sh recovery packets) instead of
  # deleting it, so the pull stays recoverable. The packet lives under the git
  # dir so the merge retry below can never collide with it.
  GIT_DIR="$(git rev-parse --absolute-git-dir 2>/dev/null || true)"
  if [ -z "${GIT_DIR}" ]; then
    echo "ERROR: Could not resolve the git dir; refusing to touch conflicting files."
    exit 1
  fi
  # mktemp -d (not timestamp-pid mkdir) so two salvages can never share a
  # packet: a reused packet dir would let the second mv silently overwrite the
  # first salvage's same-named file.
  SALVAGE_PARENT="${GIT_DIR}/safe-pull-salvage"
  if ! mkdir -p "${SALVAGE_PARENT}"; then
    echo "ERROR: Could not create salvage parent ${SALVAGE_PARENT}; refusing to touch conflicting files."
    exit 1
  fi
  SALVAGE_DIR="$(mktemp -d "${SALVAGE_PARENT}/$(date -u +%Y-%m-%dT%H-%M-%SZ)-XXXXXX" 2>/dev/null || true)"
  if [ -z "${SALVAGE_DIR}" ] || [ ! -d "${SALVAGE_DIR}" ]; then
    echo "ERROR: Could not create salvage directory under ${SALVAGE_PARENT}; refusing to touch conflicting files."
    exit 1
  fi
  echo "==> Salvaging conflicting untracked files:"
  echo "    salvage -> ${SALVAGE_DIR}"
  while IFS= read -r f; do
    if [ -n "${f}" ]; then
      if [ ! -e "${f}" ] && [ ! -L "${f}" ]; then
        echo "  skip ${f} (no longer present)"
        continue
      fi
      if ! mkdir -p "${SALVAGE_DIR}/$(dirname "${f}")"; then
        echo "ERROR: Could not create salvage path for ${f}; aborting before the retry."
        exit 1
      fi
      echo "  salvage ${f}"
      if ! mv "${f}" "${SALVAGE_DIR}/${f}"; then
        echo "ERROR: Could not salvage ${f} to ${SALVAGE_DIR}/${f}; aborting before the retry."
        exit 1
      fi
    fi
  done <<< "${CONFLICTING_FILES}"

  # Retry the merge after salvaging conflicts
  echo "==> Retrying merge..."
  git merge "${REMOTE}/${BRANCH}"
  echo "Pull succeeded after salvaging untracked conflicts to ${SALVAGE_DIR}."
  exit 0
fi

# Content conflict or other merge failure — abort the in-progress merge if any
if git rev-parse --verify MERGE_HEAD >/dev/null 2>&1; then
  git merge --abort 2>/dev/null || true
fi

# Surface the error
echo "ERROR: Merge failed for an unexpected reason:"
echo "${MERGE_OUTPUT}"
exit 1
