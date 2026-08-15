"""Content-bound DAP authority receipt construction."""

from __future__ import annotations

import hashlib
import json
import time
from typing import Any, Mapping

from dap_authority_common import RECEIPT_SCHEMA, manifest_rows, object_value


def _canonical_json_bytes(value: Mapping[str, Any]) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def _normalized_rows(manifest: Mapping[str, Any], key: str) -> list[Mapping[str, Any]]:
    rows = [dict(sorted(row.items())) for row in manifest_rows(manifest, key)]
    return sorted(rows, key=lambda row: json.dumps(row, sort_keys=True))


def build_receipt(
    manifest: Mapping[str, Any],
    observed: Mapping[str, Any],
    production: Mapping[str, Any],
) -> Mapping[str, Any]:
    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    return {
        "schema_version": RECEIPT_SCHEMA,
        "created_unix_seconds": int(time.time()),
        "authority": {
            "manifest_sha256": hashlib.sha256(_canonical_json_bytes(manifest)).hexdigest(),
            "project_extensions": _normalized_rows(manifest, "project_extensions"),
            "project_configuration": _normalized_rows(manifest, "project_configuration"),
        },
        "upstream": {
            "repository": upstream.get("repository"),
            "commit": upstream.get("commit"),
            "path": upstream.get("path"),
            "raw_url": upstream.get("raw_url"),
        },
        "observed": dict(observed),
        "production": dict(production),
        "classification": {
            "base_protocol": "Debug Adapter Protocol",
            "transport": "Content-Length framed JSON",
            "json_rpc": False,
        },
    }
