#!/usr/bin/env bash
set -euo pipefail

# Compile the current worktree's shared selection owner exactly once. Only the
# executable emitted by this successful admitted build may be used by the lane.
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$repo_root"
admission_args=()
if [[ "${1:-}" == --budget-file && $# == 2 && -n "$2" ]]; then
  admission_args=(--budget-file "$2")
elif [[ $# != 0 ]]; then
  echo 'bootstrap-selection: expected no arguments or --budget-file PATH' >&2
  exit 2
fi
artifact_log=$(mktemp)
trap 'rm -f -- "$artifact_log"' EXIT
"$repo_root/scripts/cargo-admitted" "${admission_args[@]}" build --locked \
  -p perl-ci-hygiene --bin perl-ci-hygiene --message-format=json > "$artifact_log"
python3 - "$repo_root" "$artifact_log" <<'PY'
import json
import os
from pathlib import Path
import sys

root, log = map(Path, sys.argv[1:])
source = (root / "crates/perl-ci-hygiene/src/main.rs").resolve()
executables = []
finished = False
for line in log.read_text().splitlines():
    row = json.loads(line)
    if row.get("reason") == "build-finished":
        finished = row.get("success") is True
    if row.get("reason") == "compiler-artifact":
        target = row.get("target", {})
        if target.get("name") == "perl-ci-hygiene" and target.get("kind") == ["bin"]:
            if Path(target["src_path"]).resolve() != source:
                raise SystemExit("bootstrap-selection: wrong executable source")
            executables.append(row.get("executable"))
if not finished or len(executables) != 1 or not executables[0]:
    raise SystemExit("bootstrap-selection: missing unique successful executable artifact")
executable = Path(executables[0])
if not executable.is_absolute() or not executable.is_file() or not os.access(executable, os.X_OK):
    raise SystemExit("bootstrap-selection: emitted executable is unavailable")
print(executable)
PY
