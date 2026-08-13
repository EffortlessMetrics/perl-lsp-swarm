#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
packet="$repo_root/.ci/fixtures/zed-perl-upstream/submission"

python3 - "$packet" <<'PY'
from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

packet = Path(sys.argv[1])
with (packet / "manifest.toml").open("rb") as handle:
    manifest = tomllib.load(handle)
with (packet / "changed-files.v1.json").open(encoding="utf-8") as handle:
    changed = json.load(handle)
body = (packet / "pr-body.md").read_text(encoding="utf-8")

if manifest.get("schema_version") != "zed-perllsp-upstream-submission.v1":
    raise SystemExit("error: unexpected submission manifest schema")
if manifest.get("status") == "ready" or manifest.get("ready") is True:
    required = [
        ("candidate", "proposed_version"),
        ("candidate", "candidate_commit"),
        ("candidate", "patch_sha256"),
        ("candidate", "changed_files_sha256"),
        ("contracts", "settings_contract_sha256"),
        ("contracts", "managed_assets_contract_sha256"),
        ("contracts", "defaults_packet_sha256"),
        ("contracts", "actual_host_receipt_sha256"),
        ("evidence", "actual_zed_version"),
        ("evidence", "actual_zed_platform"),
        ("evidence", "actual_extension_wasm_sha256"),
        ("evidence", "actual_perllsp_sha256"),
    ]
    missing = [f"{section}.{key}" for section, key in required if not manifest.get(section, {}).get(key)]
    if missing:
        raise SystemExit(f"error: ready packet lacks {missing}")
    for key in [
        "actual_host_result",
        "managed_download_result",
        "settings_round_trip_result",
        "default_compatibility_result",
    ]:
        if manifest["evidence"].get(key) != "pass":
            raise SystemExit(f"error: ready packet has non-pass evidence.{key}")
    if manifest["submission"].get("submission_order") == "unresolved_pending_actual_host":
        raise SystemExit("error: ready packet has unresolved submission order")
    if "[BLOCKED:" in body:
        raise SystemExit("error: ready packet retains blocked PR-body markers")
else:
    if manifest.get("status") != "blocked_pending_fan_in" or manifest.get("ready") is not False:
        raise SystemExit("error: non-ready packet must be explicitly blocked")
    if not manifest.get("blockers") or "[BLOCKED:" not in body:
        raise SystemExit("error: blocked packet must expose blockers in manifest and PR body")

paths = [entry.get("path") for entry in changed.get("files", [])]
expected = {
    "README.md",
    "extension.toml",
    "src/perl.rs",
    "languages/perl/config.toml",
    "languages/perl/semantic_token_rules.json",
}
if set(paths) != expected or not all(entry.get("required") is True for entry in changed["files"]):
    raise SystemExit("error: changed-file map does not match the reviewed candidate")

print("Zed upstream submission packet checks passed.")
PY
