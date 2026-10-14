#!/usr/bin/env bash
# Complete repository proof for a governed EM CI Rust caller. This entry point
# does not select runners, install tools, or replace the required RIPR workflow.
set -Eeuo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ $(pwd -P) == "$(realpath -- "${GITHUB_WORKSPACE:?}")" ]]
# Match the canonical RustSmall owner's checkout hygiene without changing global
# Git configuration. Hook-provided overrides must not redirect the proof's Git
# reads to a different worktree, index or object database.
git_local_env=$(git rev-parse --local-env-vars)
readarray -t git_local_env_names <<< "$git_local_env"
unset "${git_local_env_names[@]}"
[[ $(git rev-parse HEAD) == "${GITHUB_SHA:?}" ]] || {
  echo '::error::RIPR checkout differs from the evaluated event SHA' >&2
  exit 1
}
[[ ${GITHUB_RUN_ID:?} =~ ^[1-9][0-9]*$ ]]
[[ ${GITHUB_RUN_ATTEMPT:?} =~ ^[1-9][0-9]*$ ]]
[[ -d ${RUNNER_TEMP:?} ]]

# Resolve immutable event revisions, retaining the separate canonical PR head.
# Labels are data passed as one argument, never executable shell text.
metadata=$(python3 - <<'PY'
import json, os, re, subprocess
with open(os.environ['GITHUB_EVENT_PATH'], encoding='utf-8') as stream:
    event = json.load(stream)
kind = os.environ['GITHUB_EVENT_NAME']
head = ''
labels = ''
if kind == 'pull_request':
    pr = event['pull_request']
    if any(pr[side]['repo']['full_name'] != os.environ['GITHUB_REPOSITORY']
           for side in ('base', 'head')):
        raise SystemExit('RIPR proof requires an authorized same-repository PR')
    base, head = pr['base']['sha'], pr['head']['sha']
    labels = ','.join(label['name'] for label in pr.get('labels', []))
elif kind == 'merge_group':
    base = event['merge_group']['base_sha']
    if event['merge_group']['head_sha'] != os.environ['GITHUB_SHA']:
        raise SystemExit('RIPR merge-group head differs from the evaluated checkout')
elif kind in {'push', 'workflow_dispatch', 'schedule'}:
    repo = event.get('repository')
    if not isinstance(repo, dict):
        repo = {}
    ref = repo.get('default_branch')
    if not isinstance(ref, str) or subprocess.run(
        ['git', 'check-ref-format', '--branch', ref],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    ).returncode != 0:
        raise SystemExit('invalid repository default branch')
    remote = 'refs/remotes/origin/' + ref
    if subprocess.run(
        ['git', 'show-ref', '--verify', '--quiet', remote],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    ).returncode != 0:
        raise SystemExit(
            'repository default branch ref is not reachable: origin/' + ref
        )
    try:
        base = subprocess.check_output(
            ['git', 'rev-parse', '--verify', remote + '^{commit}'], text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except subprocess.CalledProcessError:
        raise SystemExit(
            'repository default branch ref is not reachable: origin/' + ref
        )
else:
    raise SystemExit('unsupported RIPR proof event')
for revision in (base, head):
    if revision and not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise SystemExit('invalid immutable RIPR revision')
    if revision:
        subprocess.run(['git', 'cat-file', '-e', revision + '^{commit}'], check=True)
        if kind in {'pull_request', 'merge_group'}:
            subprocess.run(['git', 'merge-base', '--is-ancestor', revision, 'HEAD'], check=True)
if any(character in labels for character in '\x00\r\n'):
    raise SystemExit('invalid label data')
print(base)
print(head)
print(labels)
PY
)
mapfile -t subject <<< "$metadata"
base=${subject[0]}
pr_head=${subject[1]:-}
labels=${subject[2]:-}

# A non-PR run compares the evaluated checkout against the default-branch
# ref through a three-dot base...HEAD scope, so the scope is empty whenever
# the checkout is at or behind the ref: the steady-state tip run (equal) and
# the main-moved race (checkout is a strict ancestor of the ref) alike. An
# empty scope is expected, not a failure — but it must be declared, never
# silent. The repo-wide proof below still applies.
if [[ $GITHUB_EVENT_NAME != pull_request && $GITHUB_EVENT_NAME != merge_group ]]; then
  if [[ $base == "$GITHUB_SHA" ]]; then
    echo '::notice::RIPR base equals the evaluated checkout; diff-scoped proof covers an empty range' >&2
  elif git merge-base --is-ancestor "$GITHUB_SHA" "$base"; then
    echo '::notice::RIPR base is ahead of the evaluated checkout; diff-scoped proof covers an empty range' >&2
  fi
fi

# The activated image owns RIPR. Qualification must exercise its real 0.10.1
# identity and the existing receipt validators; never install a different tool.
version=$(ripr --version)
[[ $version == 'ripr 0.10.1' ]] || {
  echo "::error::image-owned RIPR identity is '$version', expected ripr 0.10.1" >&2
  exit 1
}
export RIPR_VERSION=0.10.1 RIPR_MAX_DIFF_INDEX_FILES=2560
export CARGO_INCREMENTAL=0

for component in .ci .ci/artifacts .ci/artifacts/ripr; do
  [[ ! -L $component && ( ! -e $component || -d $component ) ]] || {
    echo '::error::non-directory or symlink in RIPR artifact root' >&2; exit 1;
  }
done
artifact_dir=$(pwd -P)/.ci/artifacts/ripr
if [[ -e $artifact_dir ]]; then
  existing=$(find "$artifact_dir" -mindepth 1 -print -quit) || exit 1
  [[ -z $existing ]] || {
    echo '::error::RIPR artifact root is not empty for this invocation' >&2
    exit 1
  }
fi
mkdir -p -- "$artifact_dir"
export RIPR_FRESHNESS_HANDOFF
RIPR_FRESHNESS_HANDOFF=$(mktemp -d "$RUNNER_TEMP/ripr-freshness.XXXXXX")
export RIPR_FRESHNESS_TOKEN="$GITHUB_RUN_ID/$GITHUB_RUN_ATTEMPT/$$"
failed=0

fresh() {
  [[ -f $RIPR_FRESHNESS_HANDOFF/clear-succeeded ]] &&
    [[ $(cat "$RIPR_FRESHNESS_HANDOFF/clear-succeeded") == "$RIPR_FRESHNESS_TOKEN" ]]
}

# Guard canonical source ancestors before invalidation as well as export.
# Existing paths are never replaced; each copy checks its destination parents.
handoff_receipts() {
  python3 - "$artifact_dir" "$1" <<'PY'
import os, shutil, stat, sys
from pathlib import Path
root = Path.cwd()
destination = Path(sys.argv[1])
mode = sys.argv[2]
sources = (
    'target/ripr/pr', 'target/ripr/review', 'target/xtask/impacted-evidence',
    'target/receipts/quality/ripr-plus.json',
    'target/receipts/quality/ripr-badge-producer.json',
    'target/receipts/quality/quality-gate-ripr.json',
    'target/receipts/quality/quality-gate-ripr.md',
)

def directories(path, create=False):
    current = root
    for component in path.relative_to(root).parts:
        current /= component
        try:
            info = current.lstat()
        except FileNotFoundError:
            if create:
                current.mkdir()
            continue
        if not stat.S_ISDIR(info.st_mode):
            raise SystemExit('non-directory or linked RIPR ancestor: ' + str(current))

def fresh_target(path):
    directories(path.parent, create=True)
    if path.exists() or path.is_symlink():
        raise SystemExit('pre-existing RIPR export destination: ' + str(path))

# The repository-relative source boundary must hold before any producer can
# invalidate or write through these paths. Repeat it after producer execution.
for relative in sources:
    source = root / relative
    directories(source.parent)
    try:
        info = source.lstat()
    except FileNotFoundError:
        continue
    directory_source = relative in sources[:3]
    expected = stat.S_ISDIR(info.st_mode) if directory_source else stat.S_ISREG(info.st_mode)
    if not expected or (not directory_source and info.st_nlink != 1):
        raise SystemExit('non-regular or linked RIPR source: ' + relative)
directories(destination)
for name in ('ripr-tool-identity.txt', 'ripr-cgroup-memory.txt'):
    path = destination / name
    if path.exists() or path.is_symlink():
        raise SystemExit('pre-existing RIPR diagnostic destination: ' + name)
if mode == 'guard':
    raise SystemExit(0)
assert mode == 'copy'

def copy_file(source):
    directories(source.parent)
    info = source.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise SystemExit('non-regular or linked RIPR receipt: ' + str(source))
    target = destination / source.relative_to(root)
    fresh_target(target)
    shutil.copyfile(source, target, follow_symlinks=False)

def traversal_error(error):
    raise error

for relative in sources:
    source = root / relative
    if not source.exists():
        continue
    if not source.is_dir():
        copy_file(source)
        continue
    for walked, children, files in os.walk(source, followlinks=False, onerror=traversal_error):
        walked = Path(walked)
        directories(walked)
        children[:] = [name for name in children if not name.startswith('.')]
        for name in children:
            directories(walked / name)
        for name in files:
            if not name.startswith('.'):
                copy_file(walked / name)
PY
}

finish() {
  status=$?
  trap - EXIT
  set +e
  # A diagnostic failure may turn success red, but must retain an existing
  # signal or proof failure instead of replacing its exit status.
  fail_finish() { if [[ $status -eq 0 ]]; then status=1; fi; }
  # Current partial diagnostics are useful on failure; older files are never
  # exported if this invocation's canonical invalidation did not complete.
  if fresh; then
    # Match the existing upload's hidden-file exclusion. The separate
    # target/ripr/stdout-staging tree is not in the canonical source list.
    # Copy regular files without following links into private/stale paths.
    handoff_receipts copy || fail_finish
    if handoff_receipts guard; then
      {
        printf 'version=%s\n' "$version"
        sha256sum -- "$(command -v ripr)"
      } > "$artifact_dir/ripr-tool-identity.txt" || fail_finish
      for metric in memory.peak memory.max memory.events; do
        if [[ -r /sys/fs/cgroup/$metric ]]; then
          printf '%s\n' "$metric"
          cat -- "/sys/fs/cgroup/$metric"
      else
        printf '%s=NOT_PROVEN\n' "$metric"
      fi
    done > "$artifact_dir/ripr-cgroup-memory.txt" || fail_finish
    else
      fail_finish
    fi
  else
    echo '::error::no current freshness handoff; canonical RIPR files were not exported' >&2
    fail_finish
  fi
  exit "$status"
}
trap finish EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

run() {
  printf 'RIPR stage %s: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
  if "$@"; then :; else failed=1; fi
}

handoff_receipts guard
ripr doctor
run cargo xtask ripr-pr --base "$base" --head HEAD --pr-head "$pr_head"
run cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
mkdir -p target/receipts/quality
printf '{"schema_version":1,"kind":"ripr_badge_producer","head":"%s","root":".","source_format":"ripr-plus repo-badge-json","ripr_version":"%s"}\n' \
  "$GITHUB_SHA" "$RIPR_VERSION" > target/receipts/quality/ripr-badge-producer.json
run cargo xtask ripr-review-comments --base "$base" --head HEAD --pr-head "$pr_head" --timeout-seconds 3600
run cargo xtask impacted-evidence "--labels-csv=$labels"
run cargo xtask ripr-pr-summary
run cargo xtask ripr-annotations
cargo xtask ripr-suppression-audit || true

fresh || { echo '::error::canonical invalidation did not complete' >&2; exit 1; }
run cargo xtask ripr-pr --base "$base" --head HEAD --pr-head "$pr_head" --check
run cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check
run cargo xtask ripr-review-comments --base "$base" --head HEAD --pr-head "$pr_head" --check
run cargo xtask impacted-evidence "--labels-csv=$labels" --check
run cargo xtask ripr-pr-summary --check
run cargo xtask ripr-annotations --check
gate=(cargo xtask quality-gate --mode enforce-new-ripr
  --ripr-receipt target/receipts/quality/ripr-plus.json
  --ripr-pr-receipt target/ripr/pr/repo-exposure.json
  --review-receipt target/ripr/review/comments.json
  --ripr-base "$base" --ripr-head HEAD
  --receipt target/receipts/quality/quality-gate-ripr.json
  --summary target/receipts/quality/quality-gate-ripr.md)
run "${gate[@]}"
run "${gate[@]}" --check

# The raw payload is part of the proof package, including multi-GiB diagnostics.
# Platform publication validates fixed aggregate limits without dropping it.
for required in target/ripr/pr/raw-check.json target/ripr/pr/repo-exposure.json \
  target/ripr/review/comments.json target/receipts/quality/ripr-plus.json \
  target/receipts/quality/quality-gate-ripr.json target/receipts/quality/quality-gate-ripr.md \
  target/xtask/impacted-evidence/latest.json target/xtask/impacted-evidence/latest.md; do
  [[ -s $required ]] || { echo "::error::missing RIPR proof file: $required" >&2; failed=1; }
done
exit "$failed"
