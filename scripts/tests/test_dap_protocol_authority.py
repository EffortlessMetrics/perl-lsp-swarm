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
Pinned commit: {COMMIT}
Pinned blob: {{blob}}

| Wire name | Kind | Classification | Version | Owner |
|---|---|---|---|---|
| `inlineValues` | `request` | `extension` | `test.v1` | `#2374` |

| Surface | Classification | Owner |
|---|---|---|
| `launch/attach arguments` | `adapter-configuration` | `#4754` |

<a id="4-breakpoint-requests"></a>
## Breakpoint requests
"""


def request_definition(command: str) -> dict:
    return {
        "allOf": [
            {"$ref": "#/definitions/Request"},
            {"properties": {"command": {"enum": [command]}}, "required": ["command"]},
        ]
    }


def event_definition(event: str, *, description: str = "event") -> dict:
    return {
        "allOf": [
            {"$ref": "#/definitions/Event"},
            {
                "description": description,
                "properties": {"event": {"enum": [event]}},
                "required": ["event"],
            },
        ]
    }


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
            "InitializeRequest": request_definition("initialize"),
            "InitializedEvent": event_definition("initialized"),
            "ContinuedEvent": event_definition(
                "continued",
                description=(
                    "a debug adapter is not expected to send this event after a request"
                ),
            ),
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
        self._write_production()

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

    def _write_production(
        self,
        *,
        commands: tuple[str, ...] = ("initialize", "inlineValues"),
        events: tuple[str, ...] = ("initialized", "continued"),
        dynamic_event: bool = False,
    ) -> None:
        dispatch = self.root / MODULE.DISPATCH_PATH
        dispatch.parent.mkdir(parents=True, exist_ok=True)
        rendered_commands = "\n".join(f'        "{command}",' for command in commands)
        dispatch.write_text(
            "impl DebugAdapter {\n"
            f"    const SUPPORTED_COMMANDS: [&str; {len(commands)}] = [\n"
            f"{rendered_commands}\n"
            "    ];\n"
            "}\n",
            encoding="utf-8",
        )

        event_source = self.root / MODULE.DEBUG_ADAPTER_ROOT / "events.rs"
        event_source.parent.mkdir(parents=True, exist_ok=True)
        calls = "\n".join(
            f'        self.send_event("{event}", None);' for event in events
        )
        if dynamic_event:
            calls += "\n        self.send_event(event_name, None);"
        event_source.write_text(
            "impl DebugAdapter {\n"
            "    fn emit(&self, event_name: &str) {\n"
            f"{calls}\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )

    def _validate(self, manifest: dict | None = None, data: bytes | None = None):
        selected_manifest = manifest or self.manifest
        selected_data = data or self.data
        validated = MODULE.validate_manifest(selected_manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(
            selected_data, validated, require_sha256=True
        )
        production = MODULE.validate_production_boundary(self.root, validated, observed)
        return validated, observed, production

    def assertAuthorityError(self, callback) -> None:  # noqa: N802 - unittest helper
        with self.assertRaises(MODULE.AuthorityError):
            callback()

    def test_happy_path_validates_authority_docs_and_production_boundary(self) -> None:
        validated, observed, production = self._validate()
        MODULE.validate_docs(self.root, validated)
        receipt = MODULE.build_receipt(validated, observed, production)
        self.assertEqual(observed["git_blob_sha1"], self.manifest["upstream"]["git_blob_sha1"])
        self.assertEqual(observed["sha256"], self.manifest["upstream"]["sha256"])
        self.assertEqual(
            production["project_extensions"],
            [{"kind": "request", "wire_name": "inlineValues"}],
        )
        self.assertEqual(receipt["authority"]["project_extensions"], self.manifest["project_extensions"])
        self.assertEqual(
            receipt["authority"]["project_configuration"],
            self.manifest["project_configuration"],
        )
        self.assertRegex(receipt["authority"]["manifest_sha256"], r"^[0-9a-f]{64}$")

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

    def test_protocol_message_inheritance_removal_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"]["Request"]["allOf"].pop(0)
        data = schema_bytes(schema)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_continued_event_guidance_removal_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"]["ContinuedEvent"]["allOf"][1]["description"] = (
            "execution continued"
        )
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

    def test_unknown_extension_kind_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_extensions"][0]["kind"] = "notification"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_duplicate_extension_identity_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_extensions"].append(copy.deepcopy(manifest["project_extensions"][0]))
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_extension_that_appears_upstream_fails(self) -> None:
        schema = fake_schema()
        schema["definitions"]["InlineValuesRequest"] = request_definition("inlineValues")
        data = schema_bytes(schema)
        manifest = manifest_for(data)
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_schema_bytes(data, validated, require_sha256=True)
        )

    def test_unclassified_production_request_fails(self) -> None:
        self._write_production(commands=("initialize", "inlineValues", "perlProbe"))
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_stale_manifest_extension_fails(self) -> None:
        self._write_production(commands=("initialize",))
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_unclassified_production_event_fails(self) -> None:
        self._write_production(events=("initialized", "continued", "perlTrace"))
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_dynamic_event_name_fails_closed(self) -> None:
        self._write_production(dynamic_event=True)
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_extension_metadata_must_match_docs(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_extensions"][0]["version"] = "test.v2"
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, validated))

    def test_breakpoint_compatibility_anchor_is_required(self) -> None:
        path = self.root / MODULE.DOC_PATHS[0]
        text = path.read_text(encoding="utf-8").replace(
            '<a id="4-breakpoint-requests"></a>\n', ""
        )
        path.write_text(text, encoding="utf-8")
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))

    def test_json_rpc_claim_in_docs_fails(self) -> None:
        path = self.root / MODULE.DOC_PATHS[0]
        path.write_text(
            path.read_text(encoding="utf-8") + "\nJSON-RPC 2.0\n",
            encoding="utf-8",
        )
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))

    def test_canonical_and_book_docs_must_match(self) -> None:
        self._write_docs(self.manifest["upstream"]["git_blob_sha1"], divergent=True)
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))

    def test_receipt_digest_changes_with_manifest_metadata(self) -> None:
        validated, observed, production = self._validate()
        first = MODULE.build_receipt(validated, observed, production)
        changed = copy.deepcopy(validated)
        changed["project_extensions"][0]["owner"] = "#9999"
        second = MODULE.build_receipt(changed, observed, production)
        self.assertNotEqual(
            first["authority"]["manifest_sha256"],
            second["authority"]["manifest_sha256"],
        )
        self.assertEqual(second["authority"]["project_extensions"][0]["owner"], "#9999")


if __name__ == "__main__":
    unittest.main()
