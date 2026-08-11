#!/usr/bin/env python3
"""Falsifiers for scripts/ci/dap_protocol_authority.py."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_protocol_authority.py"
SPEC = importlib.util.spec_from_file_location("dap_protocol_authority", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

COMMIT = "a" * 40
DOC = f"""# DAP Protocol Authority and Compatibility

Owner: #6737

The base protocol uses Content-Length framed JSON and is not JSON-RPC.
This document distinguishes standard DAP from every project extension.
The inlineValues request is a project extension.
Pinned commit: {COMMIT}
Pinned blob: {{blob}}
"""


def fake_schema() -> dict:
    return {
        "$schema": "http://json-schema.org/draft-04/schema#",
        "title": "Debug Adapter Protocol",
        "description": "test authority",
        "type": "object",
        "definitions": {
            "ProtocolMessage": {"required": ["seq", "type"]},
            "Request": {
                "allOf": [
                    {"$ref": "#/definitions/ProtocolMessage"},
                    {"required": ["type", "command"]},
                ]
            },
            "Response": {
                "allOf": [
                    {"$ref": "#/definitions/ProtocolMessage"},
                    {"required": ["type", "request_seq", "success", "command"]},
                ]
            },
            "Event": {
                "allOf": [
                    {"$ref": "#/definitions/ProtocolMessage"},
                    {"required": ["type", "event"]},
                ]
            },
            "ContinuedEvent": {
                "description": "a debug adapter is not expected to send this event after a request"
            },
        },
    }


def schema_bytes(schema: dict | None = None) -> bytes:
    return (json.dumps(schema or fake_schema(), sort_keys=True) + "\n").encode("utf-8")


def manifest_for(data: bytes, *, include_sha256: bool = True) -> dict:
    return {
        "schema_version": MODULE.MANIFEST_SCHEMA,
        "upstream": {
            "repository": "microsoft/debug-adapter-protocol",
            "commit": COMMIT,
            "path": "debugAdapterProtocol.json",
            "git_blob_sha1": MODULE.git_blob_sha1(data),
            "sha256": hashlib.sha256(data).hexdigest() if include_sha256 else None,
            "raw_url": (
                "https://raw.githubusercontent.com/microsoft/debug-adapter-protocol/"
                f"{COMMIT}/debugAdapterProtocol.json"
            ),
        },
        "base_protocol": {
            "name": "Debug Adapter Protocol",
            "transport": "Content-Length framed JSON",
            "json_rpc": False,
            "required_definitions": list(MODULE.REQUIRED_DEFINITIONS),
        },
        "project_extensions": [
            {
                "wire_name": "inlineValues",
                "kind": "request",
                "classification": "extension",
                "version": "test.v1",
                "owner": "#2374",
            }
        ],
        "project_configuration": [
            {
                "surface": "launch/attach arguments",
                "classification": "adapter-configuration",
                "owner": "#4754",
            }
        ],
    }


class DapProtocolAuthorityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.data = schema_bytes()
        self.manifest = manifest_for(self.data)
        self._write_docs(self.manifest["upstream"]["git_blob_sha1"])

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_docs(self, blob: str, *, divergent: bool = False) -> None:
        source = DOC.format(blob=blob)
        paths = [self.root / path for path in MODULE.DOC_PATHS]
        for path in paths:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source, encoding="utf-8")
        if divergent:
            paths[1].write_text(source + "\ndiverged\n", encoding="utf-8")

    def assertAuthorityError(self, callback) -> None:  # noqa: N802 - unittest helper
        with self.assertRaises(MODULE.AuthorityError):
            callback()

    def test_happy_path_validates_manifest_schema_and_docs(self) -> None:
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        MODULE.validate_docs(self.root, validated)
        self.assertEqual(observed["git_blob_sha1"], self.manifest["upstream"]["git_blob_sha1"])
        self.assertEqual(observed["sha256"], self.manifest["upstream"]["sha256"])

    def test_observe_allows_missing_sha256_but_check_does_not(self) -> None:
        manifest = manifest_for(self.data, include_sha256=False)
        validated = MODULE.validate_manifest(manifest, require_sha256=False)
        MODULE.validate_schema_bytes(self.data, validated, require_sha256=False)
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_wrong_upstream_repository_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["upstream"]["repository"] = "attacker/debug-adapter-protocol"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_mutable_or_mismatched_raw_url_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["upstream"]["raw_url"] = (
            "https://raw.githubusercontent.com/microsoft/debug-adapter-protocol/"
            "refs/heads/main/debugAdapterProtocol.json"
        )
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_raw_url_query_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["upstream"]["raw_url"] += "?cache=1"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_wrong_git_blob_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["upstream"]["git_blob_sha1"] = "0" * 40
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        )

    def test_wrong_sha256_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["upstream"]["sha256"] = "0" * 64
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        )

    def test_missing_base_definition_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"].pop("Response")
        data = schema_bytes(schema)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_required_envelope_field_removal_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"]["Response"]["allOf"][1]["required"].remove("request_seq")
        data = schema_bytes(schema)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_continued_event_guidance_removal_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"]["ContinuedEvent"]["description"] = "execution continued"
        data = schema_bytes(schema)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_invalid_json_fails_after_content_identity_matches(self) -> None:
        data = b"{"
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_oversized_schema_fails(self) -> None:
        data = b"x" * (MODULE.MAX_SCHEMA_BYTES + 1)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_inline_values_misclassified_as_standard_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_extensions"][0]["classification"] = "standard"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_duplicate_extension_identity_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_extensions"].append(copy.deepcopy(manifest["project_extensions"][0]))
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_json_rpc_claim_in_docs_fails(self) -> None:
        self._write_docs(self.manifest["upstream"]["git_blob_sha1"])
        path = self.root / MODULE.DOC_PATHS[0]
        path.write_text(path.read_text(encoding="utf-8") + "\nJSON-RPC 2.0\n", encoding="utf-8")
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))

    def test_canonical_and_book_docs_must_match(self) -> None:
        self._write_docs(self.manifest["upstream"]["git_blob_sha1"], divergent=True)
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))


if __name__ == "__main__":
    unittest.main()
