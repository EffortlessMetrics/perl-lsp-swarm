#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
packet="$repo_root/.ci/fixtures/zed-perl-upstream/submission"

python3 - "$packet" <<'PY'
from __future__ import annotations

import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path

packet = Path(sys.argv[1])
with (packet / "manifest.toml").open("rb") as handle:
    manifest = tomllib.load(handle)
changed_path = packet / "changed-files.v1.json"
with changed_path.open(encoding="utf-8") as handle:
    changed = json.load(handle)
body = (packet / "pr-body.md").read_text(encoding="utf-8")

GIT_OID = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
STATUS = manifest.get("status")
READY_FLAG = manifest.get("ready")

if (STATUS == "ready") != (READY_FLAG is True):
    raise SystemExit(
        "error: readiness fields disagree "
        f"(status={STATUS!r}, ready={READY_FLAG!r}); "
        "require status='ready' with ready=true, or blocked with ready=false"
    )

if STATUS == "ready" and READY_FLAG is True:
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

    candidate = manifest["candidate"]
    if not GIT_OID.fullmatch(str(candidate["candidate_commit"])):
        raise SystemExit("error: ready packet candidate.candidate_commit must be a 40-char lowercase git oid")
    for key in ("patch_sha256", "changed_files_sha256"):
        if not SHA256.fullmatch(str(candidate[key])):
            raise SystemExit(f"error: ready packet candidate.{key} must be a 64-char lowercase sha256")
    for section, key in (
        ("contracts", "settings_contract_sha256"),
        ("contracts", "managed_assets_contract_sha256"),
        ("contracts", "defaults_packet_sha256"),
        ("contracts", "actual_host_receipt_sha256"),
        ("evidence", "actual_extension_wasm_sha256"),
        ("evidence", "actual_perllsp_sha256"),
    ):
        value = manifest[section][key]
        if not SHA256.fullmatch(str(value)):
            raise SystemExit(f"error: ready packet {section}.{key} must be a 64-char lowercase sha256")

    recomputed = hashlib.sha256(changed_path.read_bytes()).hexdigest()
    if candidate["changed_files_sha256"] != recomputed:
        raise SystemExit(
            "error: ready packet candidate.changed_files_sha256 does not match "
            f"recomputed digest {recomputed}"
        )
    if changed.get("status") != "frozen_for_submission":
        raise SystemExit(
            "error: ready packet requires changed-files map status frozen_for_submission "
            f"(got {changed.get('status')!r})"
        )

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
elif STATUS == "blocked_pending_fan_in" and READY_FLAG is False:
    if not manifest.get("blockers") or "[BLOCKED:" not in body:
        raise SystemExit("error: blocked packet must expose blockers in manifest and PR body")
else:
    raise SystemExit(
        "error: packet must be explicitly blocked_pending_fan_in with ready=false "
        "or ready with ready=true"
    )

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
