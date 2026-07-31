#!/usr/bin/env bash
set -euo pipefail

repo=$(mktemp -d)
outside=$(mktemp -d)
cleanup() {
  rm -rf "$repo" "$outside"
}
trap cleanup EXIT

git -C "$repo" init -q
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name "Test User"
printf 'test fixture\n' > "$repo/README.md"
git -C "$repo" add README.md
git -C "$repo" commit -qm "initial"
git -C "$repo" remote add origin .
git -C "$repo" update-ref refs/remotes/origin/main HEAD
manager="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/worktree-manager.py"

# Case 1: allocate, list, and release a clean canonical worktree.
allocation=$(
  python3 "$manager" --repo-root "$repo" allocate \
    --slot 1 \
    --kind pr \
    --id 4318 \
    --slug lifecycle
)
python3 - "$allocation" <<'PY'
import json
import sys

value = json.loads(sys.argv[1])
assert value["slot"] == 1
assert value["kind"] == "pr"
assert value["id"] == "4318"
assert value["branch"] == "agent/pr-4318-lifecycle"
assert value["path"] == ".agent-worktrees/0001-pr-4318-lifecycle"
assert value["owner"] is None
PY

listed=$(python3 "$manager" --repo-root "$repo" list)
python3 - "$listed" <<'PY'
import json
import sys

value = json.loads(sys.argv[1])
assert len(value) == 1
assert value[0]["slot"] == 1
PY

python3 "$manager" --repo-root "$repo" release --slot 1 >/dev/null
test ! -e "$repo/.agent-worktrees/0001-pr-4318-lifecycle"

# Case 2: dirty worktrees require an explicit force, and force removes them.
python3 "$manager" --repo-root "$repo" allocate \
  --slot 2 \
  --kind issue \
  --id 4327 \
  --slug dirty >/dev/null
printf 'dirty\n' > "$repo/.agent-worktrees/0002-issue-4327-dirty/untracked.txt"
if python3 "$manager" --repo-root "$repo" release --slot 2 \
  >"$repo/release.stdout" 2>"$repo/release.stderr"; then
  echo "dirty managed worktree was released without --force" >&2
  exit 1
fi
grep -q "dirty" "$repo/release.stderr"
python3 "$manager" --repo-root "$repo" release --slot 2 --force >/dev/null
test ! -e "$repo/.agent-worktrees/0002-issue-4327-dirty"

# Case 3: stale state does not let cleanup escape the repository-owned root.
mkdir -p "$outside/keep"
printf 'sentinel\n' > "$outside/keep/sentinel.txt"
mkdir -p "$repo/.agent-worktrees/.worktree-manager"
cat > "$repo/.agent-worktrees/.worktree-manager/state.json" <<JSON
{
  "version": 1,
  "repo_root": "$repo",
  "allocations": {
    "3": {
      "slot": 3,
      "kind": "task",
      "id": "stale",
      "slug": "escape",
      "branch": "agent/task-stale-escape",
      "path": "../../$(basename "$outside")/keep",
      "base_ref": "origin/main"
    }
  }
}
JSON
listed=$(python3 "$manager" --repo-root "$repo" list)
test "$listed" = "[]"
test -f "$outside/keep/sentinel.txt"

# Case 4: concurrent allocations serialize state updates instead of losing one.
python3 "$manager" --repo-root "$repo" allocate \
  --slot 4 --kind task --id 4 --slug parallel-a \
  >"$repo/parallel-a.json" &
pid_a=$!
python3 "$manager" --repo-root "$repo" allocate \
  --slot 5 --kind task --id 5 --slug parallel-b \
  >"$repo/parallel-b.json" &
pid_b=$!
wait "$pid_a"
wait "$pid_b"
listed=$(python3 "$manager" --repo-root "$repo" list)
python3 - "$listed" <<'PY'
import json
import sys

value = json.loads(sys.argv[1])
assert [entry["slot"] for entry in value] == [4, 5]
PY
python3 "$manager" --repo-root "$repo" release --slot 4 >/dev/null
python3 "$manager" --repo-root "$repo" release --slot 5 >/dev/null

# Case 5: only canonical kinds and safe lowercase slugs are accepted.
if python3 "$manager" --repo-root "$repo" allocate \
  --slot 6 --kind review --id 6 --slug valid \
  >"$repo/invalid-kind.stdout" 2>"$repo/invalid-kind.stderr"; then
  echo "unsupported allocation kind was accepted" >&2
  exit 1
fi
grep -q "kind must be one of" "$repo/invalid-kind.stderr"
if python3 "$manager" --repo-root "$repo" allocate \
  --slot 6 --kind task --id 6 --slug Bad_Slug \
  >"$repo/invalid-slug.stdout" 2>"$repo/invalid-slug.stderr"; then
  echo "unsafe allocation slug was accepted" >&2
  exit 1
fi
grep -q "slug must contain only" "$repo/invalid-slug.stderr"

# Case 6: cross-repository state is refused without touching either repository.
other_repo=$(mktemp -d)
git -C "$other_repo" init -q
git -C "$other_repo" config user.email test@example.com
git -C "$other_repo" config user.name "Test User"
printf 'other fixture\n' > "$other_repo/README.md"
git -C "$other_repo" add README.md
git -C "$other_repo" commit -qm "initial"
git -C "$other_repo" remote add origin .
mkdir -p "$other_repo/.agent-worktrees/.worktree-manager"
cp "$repo/.agent-worktrees/.worktree-manager/state.json" \
  "$other_repo/.agent-worktrees/.worktree-manager/state.json"
if python3 "$manager" --repo-root "$other_repo" list \
  >"$other_repo/cross-repo.stdout" 2>"$other_repo/cross-repo.stderr"; then
  echo "cross-repository state was accepted" >&2
  rm -rf "$other_repo"
  exit 1
fi
grep -q "does not match repository root" "$other_repo/cross-repo.stderr"
rm -rf "$other_repo"

# Case 7: owner metadata round-trips and release requires the recorded owner.
owned=$(
  python3 "$manager" --repo-root "$repo" allocate \
    --slot 7 --kind task --id 7 --slug owned --owner lane-a
)
python3 - "$owned" <<'PY'
import json
import sys

value = json.loads(sys.argv[1])
assert value["owner"] == "lane-a"
PY
if python3 "$manager" --repo-root "$repo" release --slot 7 \
  >"$repo/missing-owner.stdout" 2>"$repo/missing-owner.stderr"; then
  echo "owner-protected worktree was released without --owner" >&2
  exit 1
fi
grep -q "owned by" "$repo/missing-owner.stderr"
if python3 "$manager" --repo-root "$repo" release --slot 7 --owner lane-b \
  >"$repo/wrong-owner.stdout" 2>"$repo/wrong-owner.stderr"; then
  echo "owner-protected worktree was released by another owner" >&2
  exit 1
fi
grep -q "owned by" "$repo/wrong-owner.stderr"
python3 "$manager" --repo-root "$repo" release --slot 7 --owner lane-a >/dev/null
