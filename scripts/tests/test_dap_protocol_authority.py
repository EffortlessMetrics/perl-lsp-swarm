#!/usr/bin/env python3
"""Falsifiers for scripts/ci/dap_protocol_authority.py."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import re
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

### Versioned custom families

A custom family is registered for explicit negotiation and is not standard DAP.

| Family | Version | Classification | Owner |
|---|---|---|---|
| `perl-lsp/loadedModuleReload` | `1` | `custom_dap_extension` | `#10138` |

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


def family_for() -> dict:
    return {
        "family": "perl-lsp/loadedModuleReload",
        "version": 1,
        "classification": "custom_dap_extension",
        "request_name": "perl-lsp/loadedModuleReload",
        "event_names": [],
        "capability_advertisement": "unadvertised-until-r04",
        "dispatched": False,
        "backed": False,
        "owner": "#10138",
        "contract": "#10097",
        "negotiation": {
            "mode": "explicit-client-declaration",
            "selection": "highest-mutual-version",
            "unknown_version_policy": "reject-closed",
            "unknown_variant_policy": "reject-closed",
            "unknown_field_policy": "reject-closed",
            "session_binding": "epoch",
            "restart_effect": "prior-family-and-operation-identities-invalid",
        },
        "identity_policy": {
            "subject_shape": "adapter-issued-opaque-tokens-only",
            "raw_client_input": "refused",
            "correlation": "operation-id-on-every-request-response-pair",
            "terminal_vocabulary": "frozen-#10097-outcome-codes",
            "possibly_applied_boundary": "runtime_mutation_begins",
        },
        "bounds": {
            "max_request_bytes": 8192,
            "max_identity_chars": 256,
            "max_digest_chars": 128,
            "max_reasons": 16,
            "max_reason_chars": 96,
            "max_detail_chars": 256,
            "max_retained_operations": 64,
        },
        "redaction": "codes-and-opaque-identities-only",
        "cancellation": "honored-only-before-runtime_mutation_begins",
        "standard_dap_exclusion": True,
        "schema": "schemas/loaded_module_reload_family.v1.schema.json",
        "typescript_projection": "vscode-extension/src/loadedModuleReloadFamily.generated.ts",
        "rust_contract": "crates/perl-dap/src/reload_family.rs",
        "vectors": ".spec/10138-loaded-module-reload-family/fixtures",
        "generator_check": "cargo test -p perl-dap reload_family --locked",
    }


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
        "project_families": [family_for()],
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

    @staticmethod
    def _render_row(command: str, *, standard: tuple[str, ...]) -> str:
        # The class must agree with the pinned schema; the handler is a
        # valid Rust identifier derived from the wire name.
        handler = re.sub(r"[^a-z0-9]+", "_", command.lower()).strip("_")
        kind = "standard" if command in standard else "extension"
        return f'    {kind} "{command}" => handle_{handler}(arguments),'

    # The macro definition the gate structurally validates. Fixtures carry it
    # verbatim so a test exercises the same exclusivity proof production does.
    MACRO_DEFINITION = """macro_rules! dap_request_table {
    ( $( $class:ident $command:literal => $handler:ident $arity:tt ),* $(,)? ) => {
        impl DebugAdapter {
            pub(super) fn dispatch_request(
                &mut self,
                request_seq: i64,
                command: &str,
                arguments: Option<Value>,
            ) -> DapMessage {
                let seq = self.next_seq();

                match command {
                    $(
                        $command => dap_dispatch_call!(
                            self, $handler, seq, request_seq, arguments, $arity
                        ),
                    )*
                    _ => DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: command.to_string(),
                        body: None,
                        message: Some(Self::unknown_command_message(command)),
                    },
                }
            }
        }
    };
}
"""

    @classmethod
    def _render_table(
        cls,
        commands: tuple[str, ...],
        *,
        standard: tuple[str, ...] = ("initialize",),
        extra_body: str = "",
        macro_definition: str | None = None,
    ) -> str:
        rows = "\n".join(cls._render_row(command, standard=standard) for command in commands)
        definition = cls.MACRO_DEFINITION if macro_definition is None else macro_definition
        return f"{definition}\ndap_request_table! {{\n{rows}\n{extra_body}}}\n"

    def _write_production(
        self,
        *,
        commands: tuple[str, ...] = ("initialize", "inlineValues"),
        events: tuple[str, ...] = ("initialized", "continued"),
        dynamic_event: bool = False,
        dispatch_source: str | None = None,
    ) -> None:
        dispatch = self.root / MODULE.DISPATCH_PATH
        dispatch.parent.mkdir(parents=True, exist_ok=True)
        dispatch.write_text(
            dispatch_source
            if dispatch_source is not None
            else self._render_table(commands),
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

    def test_family_section_is_required(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        del manifest["project_families"]
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_unnamespaced_family_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["family"] = "loadedModuleReload"
        manifest["project_families"][0]["request_name"] = "loadedModuleReload"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_standard_request_name_fails(self) -> None:
        # A bare standard DAP request spelling can never be a custom family.
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["family"] = "perl-lsp/restart"
        manifest["project_families"][0]["request_name"] = "restart"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_bad_classification_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["classification"] = "extension"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_version_zero_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["version"] = 0
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_standard_capability_advertisement_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["capability_advertisement"] = "supportsLoadedModuleReload"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_unknown_negotiation_policy_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["negotiation"]["unknown_variant_policy"] = "ignore"
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_unnamespaced_event_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["event_names"] = ["stopped"]
        self.assertAuthorityError(lambda: MODULE.validate_manifest(manifest, require_sha256=True))

    def test_family_dispatch_mismatch_fails(self) -> None:
        # Adding a production route for a family recorded as `dispatched:
        # false` must fail closed.
        #
        # This asserts only that the combination is rejected. It does not
        # reach `validate_production_boundary`'s family-dispatch branch: the
        # undeclared route trips the extension-inventory check first. The
        # ordering is not incidental — `validate_manifest` refuses a family
        # whose `request_name` also appears in `project_extensions`, so a
        # dispatched-but-undeclared family request cannot be spelled in a way
        # that reaches the later branch. `test_family_declared_dispatched_
        # without_a_route_fails` covers that branch from the reachable side.
        self._write_production(commands=("initialize", "inlineValues", "perl-lsp/loadedModuleReload"))
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_family_declared_dispatched_without_a_route_fails(self) -> None:
        # The reachable half of the family-dispatch invariant, and the one
        # that actually exercises the branch: the record claims a runtime
        # route the executable table does not provide.
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["dispatched"] = True
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        with self.assertRaises(MODULE.AuthorityError) as raised:
            MODULE.validate_production_boundary(self.root, validated, observed)
        self.assertIn("dispatch mismatch", str(raised.exception))

    def test_family_emitted_event_fails(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["event_names"] = ["perl-lsp/loadedModuleReloadOutcome"]
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        self._write_production(
            events=("initialized", "continued", "perl-lsp/loadedModuleReloadOutcome")
        )
        self.assertAuthorityError(
            lambda: MODULE.validate_production_boundary(self.root, validated, observed)
        )

    def test_family_metadata_must_match_docs(self) -> None:
        # The docs must carry the manifest's exact family row: an owner
        # change in the manifest without the documentation row is drift.
        manifest = copy.deepcopy(self.manifest)
        manifest["project_families"][0]["owner"] = "#9999"
        validated = MODULE.validate_manifest(manifest, require_sha256=True)
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, validated))

    def test_family_row_missing_from_docs_fails(self) -> None:
        for relative in MODULE.DOC_PATHS:
            path = self.root / relative
            path.write_text(
                path.read_text(encoding="utf-8").replace(
                    "| `perl-lsp/loadedModuleReload` | `1` | `custom_dap_extension` | `#10138` |",
                    "",
                    1,
                ),
                encoding="utf-8",
            )
        self.assertAuthorityError(lambda: MODULE.validate_docs(self.root, self.manifest))

    def test_receipt_records_the_family_boundary(self) -> None:
        validated, observed, production = self._validate()
        receipt = MODULE.build_receipt(validated, observed, production)
        self.assertEqual(
            receipt["authority"]["project_families"][0]["family"],
            "perl-lsp/loadedModuleReload",
        )
        self.assertEqual(
            production["project_families"],
            [
                {
                    "family": "perl-lsp/loadedModuleReload",
                    "request_name": "perl-lsp/loadedModuleReload",
                    "event_names": [],
                    "dispatched": False,
                }
            ],
        )

    # --- #9527: the executable request table is the only request authority ---

    def _production_rows(self):
        validated = MODULE.validate_manifest(self.manifest, require_sha256=True)
        observed = MODULE.validate_schema_bytes(self.data, validated, require_sha256=True)
        return MODULE.validate_production_boundary(self.root, validated, observed)

    def test_request_rows_are_derived_from_the_executable_table(self) -> None:
        production = self._production_rows()
        self.assertEqual(
            production["request_rows"],
            [
                {
                    "row_id": "dap.request.initialize",
                    "command": "initialize",
                    "class": "standard",
                    "handler": "handle_initialize",
                },
                {
                    "row_id": "dap.request.inlineValues",
                    "command": "inlineValues",
                    "class": "extension",
                    "handler": "handle_inlinevalues",
                },
            ],
        )

    def test_row_order_does_not_change_row_identity(self) -> None:
        before = self._production_rows()["request_rows"]
        self._write_production(commands=("inlineValues", "initialize"))
        self.assertEqual(before, self._production_rows()["request_rows"])

    def test_out_of_table_routes_are_rejected_in_every_rust_arm_shape(self) -> None:
        # The realistic split-brain: a request routed outside the table is
        # executable but invisible to the inventory. Keying the check on
        # `=> self.handle_` would recognise only the first shape below, so
        # every ordinary way to spell a match arm is covered.
        arms = {
            "plain": '"sneak" => self.handle_sneak(seq, request_seq, arguments),',
            "braced": '"sneak" => { self.handle_sneak(seq, request_seq, arguments) }',
            "guarded": '"sneak" if ready => self.handle_sneak(seq, request_seq, arguments),',
            "helper_fn": '"sneak" => route_elsewhere(seq, request_seq),',
            "assoc_fn": '"sneak" => Self::handle_sneak(seq, request_seq),',
            "namespaced_name": '"perl-lsp/sneak" => self.handle_sneak(seq, request_seq, arguments),',
            "underscored_name": '"sneak_route" => self.handle_sneak(seq, request_seq, arguments),',
            "dotted_name": '"sneak.route" => self.handle_sneak(seq, request_seq, arguments),',
        }
        for label, arm in arms.items():
            with self.subTest(arm=label):
                source = self._render_table(("initialize", "inlineValues"))
                source += (
                    "impl DebugAdapter {\n"
                    "    fn dispatch_extra(&mut self, command: &str) -> DapMessage {\n"
                    "        match command {\n"
                    f"            {arm}\n"
                    "            _ => unknown(),\n"
                    "        }\n"
                    "    }\n"
                    "}\n"
                )
                self._write_production(dispatch_source=source)
                self.assertAuthorityError(self._production_rows)

    def test_routes_cannot_escape_the_generated_dispatch_body(self) -> None:
        # The exclusivity claim is structural, not shape-matching: these are
        # routes with no arm shape in common, including a pre-match special
        # case that contains no `=>` at all.
        escapes = {
            "pre_match_special_case": (
                "                let seq = self.next_seq();",
                "                let seq = self.next_seq();\n"
                '                if command == "vendor/reload" {\n'
                "                    return self.handle_vendor(seq, request_seq);\n"
                "                }",
            ),
            "braced_arm": (
                "                    _ => DapMessage::Response {",
                '                    "vendor/reload" => { self.handle_vendor(seq, request_seq) }\n'
                "                    _ => DapMessage::Response {",
            ),
            "guarded_arm": (
                "                    _ => DapMessage::Response {",
                '                    "vendor/reload" if ready => self.handle_vendor(seq),\n'
                "                    _ => DapMessage::Response {",
            ),
            "early_return_helper": (
                "                let seq = self.next_seq();",
                "                let seq = self.next_seq();\n"
                "                if let Some(r) = self.vendor_route(command) { return r; }",
            ),
        }
        for label, (anchor, replacement) in escapes.items():
            with self.subTest(escape=label):
                self.assertIn(anchor, self.MACRO_DEFINITION)
                definition = self.MACRO_DEFINITION.replace(anchor, replacement, 1)
                self._write_production(
                    dispatch_source=self._render_table(
                        ("initialize", "inlineValues"), macro_definition=definition
                    )
                )
                self.assertAuthorityError(self._production_rows)

    def test_command_cannot_be_delegated_out_of_the_match(self) -> None:
        # Pinning the body's shape is not sufficient on its own: these keep
        # the arm count, the single match, every keyword rule and the
        # no-string-literal rule intact, while handing the command to code
        # the table does not generate.
        fallback = (
            "                    _ => DapMessage::Response {\n"
            "                        seq,\n"
            "                        request_seq,\n"
            "                        success: false,\n"
            "                        command: command.to_string(),\n"
            "                        body: None,\n"
            "                        message: Some(Self::unknown_command_message(command)),\n"
            "                    },"
        )
        delegations = {
            "fallback_delegates_to_helper": (
                fallback,
                "                    _ => self.route_unknown(seq, request_seq, command),",
            ),
            "fallback_delegates_to_assoc_fn": (
                fallback,
                "                    _ => Self::vendor(command),",
            ),
            "scrutinee_is_normalized": (
                "match command {",
                "match self.normalize(command) {",
            ),
            # Contains only allow-listed sub-expressions, yet still hands
            # command-derived data to a helper — which is why the fallback is
            # pinned by position and exact text rather than by fragments.
            "fallback_wraps_permitted_expression": (
                fallback,
                "                    _ => self.route_unknown("
                "Self::unknown_command_message(command)),",
            ),
            "fallback_fields_reordered": (
                fallback,
                "                    _ => DapMessage::Response { request_seq, seq, "
                "success: false, command: command.to_string(), body: None, "
                "message: Some(Self::unknown_command_message(command)), },",
            ),
            # Handing the command to a helper *before* the match clears the
            # scrutinee and fallback pins and uses no forbidden keyword; only
            # the residual command-escape rule catches it.
            "command_handed_out_before_the_match": (
                "                let seq = self.next_seq();",
                "                let seq = self.next_seq();\n"
                "                let _ = self.audit(command);",
            ),
        }
        for label, (anchor, replacement) in delegations.items():
            with self.subTest(delegation=label):
                self.assertIn(anchor, self.MACRO_DEFINITION)
                definition = self.MACRO_DEFINITION.replace(anchor, replacement, 1)
                self._write_production(
                    dispatch_source=self._render_table(
                        ("initialize", "inlineValues"), macro_definition=definition
                    )
                )
                self.assertAuthorityError(self._production_rows)

    def test_a_second_dispatch_definition_fails_closed(self) -> None:
        source = self._render_table(("initialize", "inlineValues"))
        source += (
            "impl DebugAdapter {\n"
            "    fn dispatch_request(&mut self, command: &str) -> DapMessage {\n"
            "        self.vendor(command)\n"
            "    }\n"
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_dispatch_defined_outside_the_macro_fails_closed(self) -> None:
        # Hand-writing the body and leaving the table as decoration must not
        # pass, even though the table itself still parses.
        source = self._render_table(("initialize", "inlineValues"), macro_definition="")
        source += (
            "impl DebugAdapter {\n"
            "    fn dispatch_request(&mut self, command: &str) -> DapMessage {\n"
            "        self.vendor(command)\n"
            "    }\n"
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_a_macro_that_does_not_define_dispatch_fails_closed(self) -> None:
        # The sharpest version of the decoy: a real macro definition and a
        # real table are present, but the routing body is hand-written
        # outside them, so the table describes rather than generates.
        hollow_macro = (
            "macro_rules! dap_request_table {\n"
            "    ( $( $class:ident $command:literal => $handler:ident $arity:tt ),* $(,)? ) => {\n"
            "        pub(crate) const SUPPORTED_COMMANDS: [&str; 2] = [$($command),*];\n"
            "    };\n"
            "}\n"
        )
        source = self._render_table(
            ("initialize", "inlineValues"), macro_definition=hollow_macro
        )
        source += (
            "impl DebugAdapter {\n"
            "    fn dispatch_request(&mut self, command: &str) -> DapMessage {\n"
            "        self.vendor(command)\n"
            "    }\n"
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_commented_out_table_cannot_supply_requests(self) -> None:
        commented = "\n".join(
            f"// {line}" for line in self._render_table(("initialize",)).splitlines()
        )
        self._write_production(dispatch_source=commented + "\n")
        self.assertAuthorityError(self._production_rows)

    def test_block_commented_row_does_not_enter_the_inventory(self) -> None:
        source = self._render_table(
            ("initialize", "inlineValues"),
            extra_body='    /* standard "ghost" => handle_ghost(arguments), */\n',
        )
        self._write_production(dispatch_source=source)
        self.assertNotIn(
            "ghost", [row["command"] for row in self._production_rows()["request_rows"]]
        )

    def test_string_literal_decoy_does_not_enter_the_inventory(self) -> None:
        source = self._render_table(("initialize", "inlineValues"))
        source += 'const DECOY: &str = "standard \\"ghost\\" => handle_ghost(arguments),";\n'
        self._write_production(dispatch_source=source)
        self.assertNotIn(
            "ghost", [row["command"] for row in self._production_rows()["request_rows"]]
        )

    def test_raw_string_decoy_is_neither_a_row_nor_a_stray_route(self) -> None:
        # A raw string has no escapes, so a naive scanner reads the quotes
        # inside it as code and either invents a row or reports a false
        # stray route.
        source = self._render_table(("initialize", "inlineValues"))
        source += 'const DECOY: &str = r#"  "ghost" => self.handle_ghost(x),  "#;\n'
        self._write_production(dispatch_source=source)
        self.assertNotIn(
            "ghost", [row["command"] for row in self._production_rows()["request_rows"]]
        )

    def test_raw_string_containing_comment_marker_is_not_code(self) -> None:
        source = self._render_table(("initialize", "inlineValues"))
        source += 'const DECOY: &str = r"// not a comment";\n'
        self._write_production(dispatch_source=source)
        self.assertEqual(
            ["initialize", "inlineValues"],
            [row["command"] for row in self._production_rows()["request_rows"]],
        )

    def test_brace_inside_a_string_cannot_unbalance_the_table(self) -> None:
        source = self._render_table(("initialize", "inlineValues"))
        source += 'const DECOY: &str = "}";\n'
        self._write_production(dispatch_source=source)
        self.assertEqual(
            ["initialize", "inlineValues"],
            [row["command"] for row in self._production_rows()["request_rows"]],
        )

    def test_two_request_tables_fail_closed(self) -> None:
        source = self._render_table(("initialize", "inlineValues"))
        source += self._render_table(("initialize",))
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_missing_request_table_fails_closed(self) -> None:
        self._write_production(dispatch_source="impl DebugAdapter {}\n")
        self.assertAuthorityError(self._production_rows)

    def test_duplicate_wire_name_fails_closed(self) -> None:
        # Distinct handlers, so only the wire-name uniqueness rule can
        # reject this pair.
        source = (
            self.MACRO_DEFINITION
            + "dap_request_table! {\n"
            '    standard "initialize" => handle_initialize(arguments),\n'
            '    standard "initialize" => handle_initialize_again(arguments),\n'
            '    extension "inlineValues" => handle_inline_values(arguments),\n'
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_two_rows_sharing_one_handler_fail_closed(self) -> None:
        source = (
            self.MACRO_DEFINITION
            + "dap_request_table! {\n"
            '    standard "initialize" => handle_shared(arguments),\n'
            '    extension "inlineValues" => handle_shared(arguments),\n'
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_unparsed_trailing_table_content_fails_closed(self) -> None:
        source = self._render_table(
            ("initialize", "inlineValues"),
            extra_body="    standard ghost => handle_ghost(arguments),\n",
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_unparsed_content_between_rows_fails_closed(self) -> None:
        # Residue in the middle of the table must fail closed too; a
        # trailing-only check would let this through.
        source = (
            self.MACRO_DEFINITION
            + "dap_request_table! {\n"
            '    standard "initialize" => handle_initialize(arguments),\n'
            "    if cfg!(feature = \"x\") { route_somewhere_else(); }\n"
            '    extension "inlineValues" => handle_inline_values(arguments),\n'
            "}\n"
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_misclassified_standard_row_fails_closed(self) -> None:
        # `inlineValues` is not in the pinned upstream schema, so a row
        # cannot claim it is standard DAP.
        source = self._render_table(
            ("initialize", "inlineValues"), standard=("initialize", "inlineValues")
        )
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_standard_request_hidden_as_extension_fails_closed(self) -> None:
        source = self._render_table(("initialize", "inlineValues"), standard=())
        self._write_production(dispatch_source=source)
        self.assertAuthorityError(self._production_rows)

    def test_removing_a_route_removes_it_from_the_inventory(self) -> None:
        # The inventory follows executable routing: a withdrawn route cannot
        # linger as a stale production request.
        self._write_production(commands=("initialize", "inlineValues"))
        self.assertIn("inlineValues", self._production_rows()["commands"])
        self._write_production(commands=("initialize",))
        self.assertAuthorityError(self._production_rows)


if __name__ == "__main__":
    unittest.main()
