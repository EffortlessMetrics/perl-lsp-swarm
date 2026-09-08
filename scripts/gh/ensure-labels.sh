#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CI_CONFIG_PATH="${CI_CONFIG_PATH:-${REPO_ROOT}/.github/ci-config.yml}"

public_api_metadata() {
  python3 - "${CI_CONFIG_PATH}" <<'PY'
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
lines = text.splitlines()
anchors = [
    index
    for index, line in enumerate(lines)
    if re.fullmatch(r"  ci:public-api:\s*", line)
]
if len(anchors) != 1:
    raise SystemExit("ci:public-api metadata is missing from .github/ci-config.yml")

metadata = {}
for line in lines[anchors[0] + 1 :]:
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue

    indent = len(line) - len(line.lstrip(" "))
    if indent <= 2:
        break
    if indent != 4:
        raise SystemExit("ci:public-api metadata contains an invalid indentation")

    field = re.fullmatch(r" {4}([A-Za-z0-9_-]+):\s*(.*)", line)
    if field is None:
        raise SystemExit("ci:public-api metadata contains a malformed field")
    key, raw_value = field.groups()
    if key not in {"color", "description"}:
        raise SystemExit(f"ci:public-api metadata contains unknown field: {key}")
    if key in metadata:
        raise SystemExit(f"ci:public-api metadata contains duplicate field: {key}")

    value = re.fullmatch(r"'([^']*)'", raw_value)
    if value is None:
        raise SystemExit(f"ci:public-api {key} must be a single-quoted scalar")
    metadata[key] = value.group(1)

if set(metadata) != {"color", "description"}:
    raise SystemExit("ci:public-api metadata must define exactly color and description")

color = metadata["color"]
description = metadata["description"]
if re.fullmatch(r"[0-9a-fA-F]{6}", color) is None:
    raise SystemExit("ci:public-api color must be exactly six hexadecimal digits")
if not description.strip() or any(ord(char) < 32 or char == "\t" for char in description):
    raise SystemExit("ci:public-api description must be non-empty and free of control characters")
if len(description) > 100:
    raise SystemExit("ci:public-api description must be at most 100 characters")

print(f"{color}\t{description}")
PY
}

IFS=$'\t' read -r PUBLIC_API_COLOR PUBLIC_API_DESCRIPTION < <(public_api_metadata)
if [[ -z "${PUBLIC_API_COLOR}" || -z "${PUBLIC_API_DESCRIPTION}" ]]; then
  echo "ci:public-api metadata is empty" >&2
  exit 1
fi

# Ensure gh auth works
gh auth status >/dev/null

# Get existing labels as a searchable string
existing="$(gh label list --limit 1000 --json name --jq '.[].name' | tr '\n' '|')"
existing="|${existing}"  # Prefix with | for boundary matching

ensure() {
  local name="$1"
  local color="$2"
  local desc="$3"

  if [[ "$existing" == *"|$name|"* ]]; then
    echo "✓ label exists: $name"
  else
    echo "→ creating label: $name"
    gh label create "$name" --color "$color" --description "$desc"
  fi
}

ensure_reconciled() {
  local name="$1"
  local color="$2"
  local desc="$3"

  if [[ "$existing" == *"|$name|"* ]]; then
    echo "↻ reconciling label: $name"
    gh label edit "$name" --color "$color" --description "$desc"
  else
    echo "→ creating label: $name"
    gh label create "$name" --color "$color" --description "$desc"
  fi
}

echo "=== Type Labels ==="
ensure "type:bug"            "d73a4a" "Something is incorrect or broken"
ensure "type:enhancement"    "a2eeef" "New capability or improvement"
ensure "type:chore"          "cfd3d7" "Maintenance and cleanup"
ensure "type:infrastructure" "0052cc" "CI/build/release/ops work"
ensure "type:docs"           "0075ca" "Documentation changes"

echo ""
echo "=== Priority Labels ==="
# Note: You already have priority:critical, priority:high, etc.
# Adding P0-P3 as aliases for faster typing
ensure "P0-critical" "b60205" "Blocker / must fix immediately"
ensure "P1-high"     "d93f0b" "High impact, fix this sprint"
ensure "P2-medium"   "fbca04" "Normal priority"
ensure "P3-low"      "0e8a16" "Nice to have / backlog"

echo ""
echo "=== Status Labels ==="
# You already have: blocked, in-progress
ensure "status:blocked"      "5319e7" "Blocked by external dependency"
ensure "status:ready"        "0e8a16" "Ready to start"
ensure "status:in-progress"  "1d76db" "Actively being worked"
ensure "status:needs-triage" "ededed" "Needs review / categorization"

echo ""
echo "=== Area Labels ==="
# You already have unprefixed: parser, lsp, tests, infrastructure
# Adding prefixed versions for consistency
ensure "area:ci"      "0052cc" "CI and automation"
ensure "area:parser"  "f9d0c4" "Perl parser"
ensure "area:lsp"     "c5def5" "Language Server Protocol"
ensure "area:dap"     "bfdadc" "Debug Adapter Protocol"
ensure "area:tests"   "e4e669" "Testing infrastructure"
ensure "area:docs"    "0075ca" "Documentation"
ensure "area:lexer"   "d4edda" "Lexer and tokenization"
ensure "area:semantic" "c2e0c6" "Semantic analysis"

echo ""
echo "=== Lane Trigger Labels ==="
ensure_reconciled "ci:public-api" "${PUBLIC_API_COLOR}" "${PUBLIC_API_DESCRIPTION}"

echo ""
echo "=== Done ==="
echo "Label taxonomy is ready. Your automated workflows (gate:*, review:*, fix:*) remain intact."
