#!/usr/bin/env bash
# Measure runtimes of the local CI lanes (Issue #211).
#
# Goal: baseline timings before changing workflows, so we can prove cost savings.
#
# Output:
#   artifacts/ci-time.json  (machine-readable)
#   artifacts/ci-time.md    (human-readable)
#
# Schema contract (#15381): this script is the manual `bash` fallback for the
# canonical `cargo xtask ci-measure` producer. Both producers MUST emit the
# same `schema_version` so a consumer can identify the file shape, while
# `producer` identifies which tool actually wrote the file — this script MUST
# NOT claim the canonical producer's identity. `SCHEMA_VERSION` below is kept
# byte-identical with `xtask/src/tasks/ci_measure.rs`'s `SCHEMA_VERSION`; the
# inline test in that file ratchets it and asserts the producers differ.
SCHEMA_VERSION="ci-time.v1"
PRODUCER="measure-ci-time-sh"

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARTIFACTS="$ROOT/artifacts"
mkdir -p "$ARTIFACTS"

python_bin="python3"
if ! command -v python3 >/dev/null 2>&1; then
  python_bin="python"
fi

now_iso() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

time_cmd() {
  local name="$1"
  shift
  echo "==> $name" >&2
  "$python_bin" - "$name" "$@" "$SCHEMA_VERSION" "$PRODUCER" <<'PY'
import json
import os
import subprocess
import sys
import time

name = sys.argv[1]
cmd = sys.argv[2:-2]
schema_version = sys.argv[-2]
producer = sys.argv[-1]

start = time.perf_counter()
proc = subprocess.run(
    cmd,
    cwd=os.environ.get("ROOT"),
    text=True,
    stdout=sys.stderr,
    stderr=sys.stderr,
)
end = time.perf_counter()

ndjson_line = json.dumps({
  "schema_version": schema_version,
  "producer": producer,
  "name": name,
  "seconds": round(end - start, 3),
  "returncode": proc.returncode,
}) + "\n"
sys.stdout.write(ndjson_line)
sys.exit(proc.returncode)
PY
}

export ROOT

tmp_json="$ARTIFACTS/ci-time.ndjson"
rm -f "$tmp_json"

# Lane set: mirrors what actually matters for merges.
# Keep this small and stable; add more lanes only when we decide to pay for them.
time_cmd "ci-format"             just ci-format             | tee -a "$tmp_json"
time_cmd "ci-docs-check"         just ci-docs-check         | tee -a "$tmp_json"
time_cmd "ci-clippy-lib"         just ci-clippy-lib         | tee -a "$tmp_json"
time_cmd "clippy-prod-no-unwrap" just clippy-prod-no-unwrap | tee -a "$tmp_json"
time_cmd "ci-test-lib"           just ci-test-lib           | tee -a "$tmp_json"
time_cmd "ci-lsp-def"            just ci-lsp-def            | tee -a "$tmp_json"
time_cmd "status-check"          just status-check          | tee -a "$tmp_json"

# Build consolidated JSON
"$python_bin" - "$tmp_json" "$ARTIFACTS/ci-time.json" "$(now_iso)" "$SCHEMA_VERSION" "$PRODUCER" <<'PY'
import json
import sys
from pathlib import Path

ndjson = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
out_path = Path(sys.argv[2])
generated_at = sys.argv[3]
schema_version = sys.argv[4]
producer = sys.argv[5]

rows = [json.loads(line) for line in ndjson if line.strip()]
total = round(sum(r["seconds"] for r in rows), 3)

payload = {
  "schema_version": schema_version,
  "producer": producer,
  "generated_at": generated_at,
  "lanes": rows,
  "total_seconds": total,
}
out_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

# Build Markdown table
"$python_bin" - "$ARTIFACTS/ci-time.json" "$ARTIFACTS/ci-time.md" <<'PY'
import json
import sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
out = Path(sys.argv[2])

lines = []
lines.append("# CI Timing Baseline")
lines.append("")
lines.append(f"- Generated at: `{data['generated_at']}`")
lines.append(f"- Total: `{data['total_seconds']}s`")
lines.append(f"- Schema: `{data.get('schema_version', 'unknown')}`")
lines.append(f"- Producer: `{data.get('producer', 'unknown')}`")
lines.append("")
lines.append("| Lane | Seconds | RC |")
lines.append("|------|---------|----|")
for r in data["lanes"]:
  lines.append(f"| `{r['name']}` | {r['seconds']} | {r['returncode']} |")
out.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

echo ""
echo "Wrote:"
echo "  - $ARTIFACTS/ci-time.json"
echo "  - $ARTIFACTS/ci-time.md"
