#!/usr/bin/env bash
# scripts/target-gc.sh — safe manual garbage collection of stale repo-local
# cargo target/ directories (#12791).
#
# DRY-RUN BY DEFAULT: nothing is deleted unless --apply is passed.
#
# Safety contract:
#   * candidates are only `target/` directories at the repo root and directly
#     under .worktrees/*/ — never the cargo registry, never lockfiles, never
#     anything else;
#   * a target/ is stale only when NOTHING inside it has been modified within
#     the threshold (default 30 days); a single fresh file keeps the tree;
#   * refuses to run while the devplane build flock is held (a lane may be
#     mid-build against a candidate);
#   * deletion authority for executor-managed reclamation stays with #11671 —
#     this is a manual maintenance tool and replaces nothing there.
#
# Usage:
#   scripts/target-gc.sh [--days N]            # dry-run report
#   scripts/target-gc.sh [--days N] --apply    # delete the stale candidates
#   scripts/target-gc.sh --self-test           # discrimination test (creates
#                                              # and removes its own fixtures)

set -euo pipefail

days=30
apply=0
self_test=0
plumbing=""
for arg in "$@"; do
  case "$arg" in
    --days=*) days="${arg#--days=}" ;;
    --days) echo "error: use --days=N" >&2; exit 64 ;;
    --apply) apply=1 ;;
    --self-test) self_test=1 ;;
    --self-test-dry-run) plumbing="dry" ;;
    --self-test-apply) plumbing="apply" ;;
    -h|--help)
      sed -n '2,22p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *) echo "error: unknown argument: $arg" >&2; exit 64 ;;
  esac
done
case "$days" in
  ''|*[!0-9]*) echo "error: --days must be a non-negative integer, got: $days" >&2; exit 64 ;;
esac

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
repo_name="$(basename "$repo_root")"
devplane="${DEVPLANE:-${XDG_CACHE_HOME:-$HOME/.cache}/devplane/$repo_name}"
build_lock="$devplane/locks/cargo-build.lock"

refuse_if_build_lock_held() {
  # flock(1) is Linux-only; where it is absent there is no flock contract to
  # violate (cargo-safe degrades the same way), so the check is skipped.
  command -v flock >/dev/null 2>&1 || return 0
  mkdir -p "$(dirname "$build_lock")"
  exec {build_lock_fd}>"$build_lock"
  if ! flock -n "$build_lock_fd"; then
    echo "REFUSING: devplane build flock is held ($build_lock) — a build may be in progress; rerun when it finishes." >&2
    exit 75
  fi
}

# is_stale_target DIR — succeeds when no entry inside DIR is newer than $days.
is_stale_target() {
  local dir="$1"
  local fresh_entries
  # -print -quit stops at the first fresh entry, so fresh trees cost ~nothing;
  # only genuinely stale trees pay the full walk (and they are the point).
  if ! fresh_entries=$(find "$dir" -newermt "$days days ago" -print -quit 2>/dev/null); then
    return 2
  fi
  if [ -n "$fresh_entries" ]; then
    return 1
  fi
  return 0
}

collect_candidates() {
  local root="$1"
  [ -d "$root/target" ] && printf '%s\n' "$root/target"
  local wt
  for wt in "$root"/.worktrees/*/target; do
    [ -d "$wt" ] && printf '%s\n' "$wt"
  done
}

run_gc() {
  local root="$1"
  refuse_if_build_lock_held

  local stale=()
  local candidate
  while IFS= read -r candidate; do
    if is_stale_target "$candidate"; then
      stale+=("$candidate")
    else
      local scan_rc=$?
      if [ "$scan_rc" -eq 2 ]; then
        echo "REFUSING: freshness scan failed for candidate: $candidate" >&2
        return 75
      fi
    fi
  done < <(collect_candidates "$root")

  if [ "${#stale[@]}" -eq 0 ]; then
    echo "target-gc: no stale target/ directories (threshold: ${days}d, root: $root)"
    return 0
  fi

  local total_kb=0
  local candidate size_kb
  for candidate in "${stale[@]}"; do
    size_kb=$(du -sk "$candidate" 2>/dev/null | cut -f1)
    total_kb=$(( total_kb + ${size_kb:-0} ))
    printf 'stale: %s (%s MB, nothing newer than %sd)\n' \
      "$candidate" "$(( ${size_kb:-0} / 1024 ))" "$days"
  done

  if [ "$apply" -ne 1 ]; then
    echo "target-gc: dry-run — $(( total_kb / 1024 )) MB would be reclaimed; re-run with --apply to delete."
    return 0
  fi

  for candidate in "${stale[@]}"; do
    # Defense in depth: only ever delete paths that end in /target below the
    # repo root, never the registry, never lockfiles.
    case "$candidate" in
      "$root"/target|"$root"/.worktrees/*/target) ;;
      *) echo "REFUSING: candidate outside the allowed shape: $candidate" >&2; exit 65 ;;
    esac
    # Revalidate immediately before removal: freshness evidence must be
    # current at the deletion point, not only at classification time.
    if ! is_stale_target "$candidate"; then
      echo "skipped (no longer provably stale at deletion point): $candidate"
      continue
    fi
    rm -rf -- "$candidate"
    echo "deleted: $candidate"
  done
  echo "target-gc: reclaimed $(( total_kb / 1024 )) MB across ${#stale[@]} stale target/ directorie(s)."
}

self_test() {
  local tmp rc
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN

  # Fixture: one fresh worktree target, one stale worktree target, plus
  # registry-looking and lockfile-looking decoys that must never be touched.
  mkdir -p "$tmp/.worktrees/fresh/target/sub" "$tmp/.worktrees/stale/target/sub"
  mkdir -p "$tmp/.worktrees/stale/registry-cache"
  echo fresh > "$tmp/.worktrees/fresh/target/sub/new.o"
  echo stale > "$tmp/.worktrees/stale/target/sub/old.o"
  echo decoy > "$tmp/.worktrees/stale/registry-cache/keep.me"
  echo lock > "$tmp/.worktrees/stale/target/Cargo.lock"
  touch -d "60 days ago" \
    "$tmp/.worktrees/stale/target" \
    "$tmp/.worktrees/stale/target/sub" \
    "$tmp/.worktrees/stale/target/sub/old.o" \
    "$tmp/.worktrees/stale/target/Cargo.lock" \
    "$tmp/.worktrees/stale/registry-cache" \
    "$tmp/.worktrees/stale/registry-cache/keep.me"

  # Discrimination 1: dry-run selects exactly the stale tree.
  local report
  report=$(TARGET_GC_SELFTEST_DRY_RUN="$tmp" DEVPLANE="$tmp/devplane" bash "${BASH_SOURCE[0]}" --days=30 --self-test-dry-run)
  if ! grep -q "stale: $tmp/.worktrees/stale/target" <<<"$report"; then
    echo "SELF-TEST FAILED: stale tree not selected:" >&2
    echo "$report" >&2
    exit 1
  fi
  if grep -q "stale: $tmp/.worktrees/fresh/target" <<<"$report"; then
    echo "SELF-TEST FAILED: fresh tree wrongly selected:" >&2
    echo "$report" >&2
    exit 1
  fi
  if grep -q "registry-cache" <<<"$report"; then
    echo "SELF-TEST FAILED: registry-looking path wrongly selected:" >&2
    echo "$report" >&2
    exit 1
  fi

  # Discrimination 2: --apply removes only the stale tree; the fresh tree,
  # its contents, and the decoys survive.
  TARGET_GC_SELFTEST_ROOT="$tmp" DEVPLANE="$tmp/devplane" bash "${BASH_SOURCE[0]}" --days=30 --self-test-apply >/dev/null
  [ -f "$tmp/.worktrees/fresh/target/sub/new.o" ] || { echo "SELF-TEST FAILED: fresh tree content removed" >&2; exit 1; }
  [ -f "$tmp/.worktrees/stale/registry-cache/keep.me" ] || { echo "SELF-TEST FAILED: registry decoy removed" >&2; exit 1; }
  [ ! -e "$tmp/.worktrees/stale/target" ] || { echo "SELF-TEST FAILED: stale tree not deleted" >&2; exit 1; }

  # Discrimination 3: flock held -> refusal (where flock exists).
  if command -v flock >/dev/null 2>&1; then
    mkdir -p "$tmp/devplane/locks"
    rc=0
    flock "$tmp/devplane/locks/cargo-build.lock" -c \
      "TARGET_GC_SELFTEST_ROOT='$tmp' DEVPLANE='$tmp/devplane' bash '${BASH_SOURCE[0]}' --self-test-dry-run >/dev/null 2>&1" || rc=$?
    if [ "$rc" -eq 0 ]; then
      echo "SELF-TEST FAILED: ran while the devplane flock was held" >&2
      exit 1
    fi
  fi

  # Discrimination 4: a failed freshness scan fails CLOSED — the run refuses
  # and nothing is classified stale. Shadow `find` with a stub that always
  # fails; even the 60-day-old tree must not be selected or deleted.
  mkdir -p "$tmp/bin"
  printf '#!/bin/sh\nexit 1\n' > "$tmp/bin/find"
  chmod +x "$tmp/bin/find"
  rc=0
  report=$(PATH="$tmp/bin:$PATH" TARGET_GC_SELFTEST_DRY_RUN="$tmp" DEVPLANE="$tmp/devplane" bash "${BASH_SOURCE[0]}" --days=30 --self-test-dry-run 2>&1) || rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "SELF-TEST FAILED: a failed freshness scan did not refuse the run:" >&2
    echo "$report" >&2
    exit 1
  fi
  if grep -q "^stale: " <<<"$report"; then
    echo "SELF-TEST FAILED: a failed freshness scan classified a tree as stale:" >&2
    echo "$report" >&2
    exit 1
  fi

  echo "target-gc self-test: OK (stale-only selection, apply preserves fresh+decoys, flock refusal, scan failure refuses)"
}

# Self-test plumbing: run the GC against an injected root instead of the repo.
if [ -n "$plumbing" ]; then
  if [ "$plumbing" = "dry" ]; then
    repo_root="${TARGET_GC_SELFTEST_DRY_RUN:-}"
  else
    repo_root="${TARGET_GC_SELFTEST_ROOT:-}"
  fi
  if [ -z "$repo_root" ]; then
    echo "error: $plumbing self-test plumbing requires its injected fixture root" >&2
    exit 64
  fi
  [ "$plumbing" = "apply" ] && apply=1
  run_gc "$repo_root"
  exit 0
fi

if [ "$self_test" -eq 1 ]; then
  self_test
  exit 0
fi

run_gc "$repo_root"
