#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
packet="$repo_root/.ci/fixtures/zed-perl-upstream/registry"

python3 - "$packet" <<'PY'
from __future__ import annotations

import sys
import tomllib
from pathlib import Path

packet = Path(sys.argv[1])
with (packet / "manifest.toml").open("rb") as handle:
    manifest = tomllib.load(handle)
body = (packet / "pr-body.md").read_text(encoding="utf-8")

if manifest.get("schema_version") != "zed-perl-registry-update.v1":
    raise SystemExit("error: unexpected registry packet schema")

extension = manifest.get("extension")
if not isinstance(extension, dict):
    raise SystemExit("error: registry packet missing extension table")
if extension.get("id") != "perl" or extension.get("submodule_path") != "extensions/perl":
    raise SystemExit("error: registry packet must update the existing Perl extension")
if extension.get("submodule_remote") != "https://github.com/tree-sitter-perl/zed-perl.git":
    raise SystemExit("error: registry packet must retain the HTTPS upstream remote")

submission = manifest.get("submission")
if not isinstance(submission, dict):
    raise SystemExit("error: registry packet missing submission table")
if submission.get("expected_changed_paths") != ["extensions/perl", "extensions.toml"]:
    raise SystemExit("error: registry packet has an unexpected changed-path set")

validation = manifest.get("validation")
if not isinstance(validation, dict):
    raise SystemExit("error: registry packet missing validation table")
zed_defaults = manifest.get("zed_defaults")
if not isinstance(zed_defaults, dict):
    raise SystemExit("error: registry packet missing zed_defaults table")

if manifest.get("status") == "ready" or manifest.get("ready") is True:
    required = ["new_version", "new_commit", "upstream_branch_containing_commit"]
    missing = [key for key in required if not extension.get(key)]
    if missing:
        raise SystemExit(f"error: ready registry packet lacks {missing}")
    if extension.get("new_version") == extension.get("current_version"):
        raise SystemExit("error: ready packet does not advance the extension version")
    if extension.get("new_commit") == extension.get("current_commit"):
        raise SystemExit("error: ready packet does not advance the submodule")
    if not validation.get("submodule_commit_branch_reachable"):
        raise SystemExit("error: ready packet targets an unproven commit")
    if not validation.get("manifest_version_matches"):
        raise SystemExit("error: ready packet has no manifest/version equality proof")
    for key in ["pnpm_sort_extensions", "registry_package_check", "registry_danger_check"]:
        if validation.get(key) != "pass":
            raise SystemExit(f"error: ready packet has non-pass validation.{key}")
    if not validation.get("diff_sha256"):
        raise SystemExit("error: ready packet lacks a final diff digest")
    if zed_defaults.get("state") == "unresolved_pending_actual_host":
        raise SystemExit("error: ready packet has unresolved Zed-default ordering")
    if "[BLOCKED:" in body:
        raise SystemExit("error: ready packet retains blocked PR-body markers")
else:
    if manifest.get("status") != "blocked_pending_upstream_merge" or manifest.get("ready") is not False:
        raise SystemExit("error: non-ready registry packet must be explicitly blocked")
    blockers = manifest.get("blockers") or submission.get("blockers")
    if not blockers or "[BLOCKED:" not in body:
        raise SystemExit("error: blocked packet does not expose its blockers")
    if extension.get("new_commit") or extension.get("new_version"):
        raise SystemExit("error: blocked packet must not invent a merged upstream identity")

print("Zed registry update packet checks passed.")
PY
