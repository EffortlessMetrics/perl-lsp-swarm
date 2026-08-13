#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
packet="$repo_root/.ci/fixtures/zed-perl-upstream/zed-core"

bash -n "$repo_root/scripts/apply-zed-core-perl-defaults.sh"

python3 - "$packet" <<'PY'
from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

packet = Path(sys.argv[1])
with (packet / "manifest.toml").open("rb") as handle:
    manifest = tomllib.load(handle)
with (packet / "compatibility-matrix.v1.json").open(encoding="utf-8") as handle:
    matrix = json.load(handle)
patch = (packet / "perl-defaults.patch").read_text(encoding="utf-8")

expected = ["perlnavigator-server", "!perl-lsp", "!perllsp", "..."]
if manifest.get("ordering") != expected:
    raise SystemExit("error: manifest does not preserve the reviewed Perl ordering")
if manifest.get("base_commit") != "7733b9922665f103abda7c6a3fde6b9dfdc8eba9":
    raise SystemExit("error: Zed base commit drifted")
if manifest.get("target_blob") != "a03ad8874243f167e86deba8f975268eb384d20f":
    raise SystemExit("error: Zed default settings blob drifted")

needle = '"language_servers": ["perlnavigator-server", "!perl-lsp", "!perllsp", "..."]'
if patch.count(needle) != 1:
    raise SystemExit("error: patch must contain the exact server order once")
if '"perl-lsp"' not in patch or '"perllsp"' not in patch:
    raise SystemExit("error: independent alternative IDs are missing")
if '"!perlnavigator-server"' in patch:
    raise SystemExit("error: candidate must not disable the current default provider")

rows = matrix.get("rows", [])
if len(rows) != 4 or any(row.get("observed") != "not_proven" for row in rows):
    raise SystemExit("error: compatibility matrix must retain four unproven cells")
if matrix.get("submission_order", {}).get("status") != "unresolved_pending_actual_host":
    raise SystemExit("error: submission order must remain unresolved before host proof")

print("Zed core defaults packet checks passed.")
PY
