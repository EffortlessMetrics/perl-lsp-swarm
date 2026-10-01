#!/usr/bin/env bash
# Complete repository proof for a governed EM CI Rust caller. This entry point
# does not select runners, install tools, or replace the required RIPR workflow.
set -Eeuo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ $(pwd -P) == "$(realpath -- "${GITHUB_WORKSPACE:?}")" ]]
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
    if pr['head']['repo']['full_name'] != os.environ['GITHUB_REPOSITORY']:
        raise SystemExit('RIPR proof requires an authorized same-repository PR')
    base, head = pr['base']['sha'], pr['head']['sha']
    labels = ','.join(label['name'] for label in pr.get('labels', []))
elif kind == 'merge_group':
    base = event['merge_group']['base_sha']
elif kind in {'push', 'workflow_dispatch', 'schedule'}:
    ref = event['repository']['default_branch']
    if not re.fullmatch(r'[A-Za-z0-9._/-]+', ref) or '..' in ref:
        raise SystemExit('invalid repository default branch')
    base = subprocess.check_output(
        ['git', 'rev-parse', '--verify', 'origin/' + ref + '^{commit}'], text=True
    ).strip()
else:
    raise SystemExit('unsupported RIPR proof event')
for revision in (base, head):
    if revision and not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise SystemExit('invalid immutable RIPR revision')
    if revision:
        subprocess.run(['git', 'cat-file', '-e', revision + '^{commit}'], check=True)
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
  [[ ! -L $component ]] || { echo '::error::symlink in RIPR artifact root' >&2; exit 1; }
done
artifact_dir=$GITHUB_WORKSPACE/.ci/artifacts/ripr
if [[ -e $artifact_dir && -n $(find "$artifact_dir" -mindepth 1 -print -quit) ]]; then
  echo '::error::RIPR artifact root is not empty for this invocation' >&2
  exit 1
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
    python3 - "$artifact_dir" <<'PY' || fail_finish
import os, shutil, stat, sys
from pathlib import Path
destination = Path(sys.argv[1])
sources = (
    'target/ripr/pr', 'target/ripr/review', 'target/xtask/impacted-evidence',
    'target/receipts/quality/ripr-plus.json',
    'target/receipts/quality/ripr-badge-producer.json',
    'target/receipts/quality/quality-gate-ripr.json',
    'target/receipts/quality/quality-gate-ripr.md',
)
def copy_file(source):
    info = source.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise SystemExit('non-regular or linked RIPR receipt: ' + str(source))
    target = destination / source
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target, follow_symlinks=False)
for relative in sources:
    source = Path(relative)
    if source.is_symlink():
        raise SystemExit('linked RIPR source: ' + relative)
    if not source.exists():
        continue
    if not source.is_dir():
        copy_file(source)
        continue
    for root, directories, files in os.walk(source, followlinks=False):
        directories[:] = [name for name in directories if not name.startswith('.')]
        for name in directories:
            if (Path(root) / name).is_symlink():
                raise SystemExit('linked RIPR directory: ' + str(Path(root) / name))
        for name in files:
            if not name.startswith('.'):
                copy_file(Path(root) / name)
PY
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
