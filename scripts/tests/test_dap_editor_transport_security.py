#!/usr/bin/env python3
"""Negative controls for the #10567 DAP editor-transport security instrument.

These tests fail when the instrument converts missing observation to zero
listeners, confuses editor and debugger-peer roles, accepts a stale binary,
serializes a peer credential, or records unknown cleanup as pass.
"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_editor_transport_security.py"
SPEC = importlib.util.spec_from_file_location("dap_editor_transport_security", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

INVENTORY_SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_editor_transport_inventory.py"
INV_SPEC = importlib.util.spec_from_file_location("dap_editor_transport_inventory", INVENTORY_SCRIPT)
assert INV_SPEC is not None and INV_SPEC.loader is not None
INVENTORY = importlib.util.module_from_spec(INV_SPEC)
INV_SPEC.loader.exec_module(INVENTORY)
SCHEMA = INVENTORY.sys.modules["dap_editor_transport_schema"]
SCAN = INVENTORY.sys.modules["dap_editor_transport_scan"]


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def valid_inventory() -> dict:
    inventory = {
        "schema_version": SCHEMA.INVENTORY_SCHEMA,
        "conflict_key": SCHEMA.CONFLICT_KEY,
        "adr": "docs/adr/0047-dap-stdio-only-editor-transport.md",
        "ruling_status": "accepted",
        "tcp_required_supported_client": None,
        "invariants": list(SCHEMA.REQUIRED_INVARIANTS),
        "first_mile_surfaces": ["crates/perl-dap/README.md"],
        "transports": [
            {
                "id": "dap-attach",
                "kind": "dap_attach",
                "role": "attach_backend",
                "disposition": "retain",
                "authority": "product",
                "owner_issue": "#10564",
                "paths": ["crates/perl-dap/src/tcp_attach/mod.rs"],
                "claim_boundary": "DAP attach is a protocol request, not editor TCP",
            },
            {
                "id": "dap-to-dap-proxy",
                "kind": "dap_to_dap_proxy",
                "role": "dap_proxy",
                "disposition": "not_product",
                "authority": "historical",
                "owner_issue": "#10564",
                "paths": ["crates/perl-dap/src/tcp_attach/mod.rs"],
                "claim_boundary": "legacy PLS proxy is not an editor transport",
            },
            {
                "id": "debugger-peer-tcp",
                "kind": "debugger_peer_tcp",
                "role": "debugger_peer",
                "disposition": "retain",
                "authority": "product",
                "owner_issue": "#6949",
                "paths": ["crates/perl-dap/src/backend/peer_launch.rs"],
                "claim_boundary": "authenticated debugger-peer TCP stays separate",
            },
            {
                "id": "external-peer-editor-stdio",
                "kind": "external_peer_editor_stdio",
                "role": "editor",
                "disposition": "retain",
                "authority": "product",
                "owner_issue": "#10564",
                "paths": ["crates/perl-dap/src/main.rs"],
                "claim_boundary": "external-peer editor stdio remains product",
            },
            {
                "id": "external-peer-editor-tcp",
                "kind": "external_peer_editor_tcp",
                "role": "editor",
                "disposition": "retire",
                "authority": "product",
                "owner_issue": "#10566",
                "paths": ["crates/perl-dap/src/main.rs"],
                "claim_boundary": "external-peer editor socket wrapper is retire",
            },
            {
                "id": "native-editor-stdio",
                "kind": "native_editor_stdio",
                "role": "editor",
                "disposition": "retain",
                "authority": "product",
                "owner_issue": "#10564",
                "paths": ["crates/perl-dap/src/main.rs"],
                "claim_boundary": "stdio is the product editor transport",
            },
            {
                "id": "native-editor-tcp",
                "kind": "native_editor_tcp",
                "role": "editor",
                "disposition": "retire",
                "authority": "product",
                "owner_issue": "#10565",
                "paths": ["crates/perl-dap/src/debug_adapter/transport.rs"],
                "claim_boundary": "native --socket is retire, not supported",
            },
            {
                "id": "test-only-loopback",
                "kind": "test_only_loopback",
                "role": "test_only",
                "disposition": "not_product",
                "authority": "test",
                "owner_issue": "#10564",
                "paths": ["crates/perl-dap/tests/dap_editor_transport_security.rs"],
                "claim_boundary": "loopback fixtures cannot satisfy product clients",
            },
        ],
        "bind_sites": [
            {
                "id": "debugger-peer-listen",
                "path": "crates/perl-dap/src/backend/peer_launch.rs",
                "role": "debugger_peer",
                "transport_id": "debugger-peer-tcp",
                "disposition": "retain",
            },
        ],
        "cli_flags": [
            {
                "id": "perl-dap-port",
                "flag": "--port",
                "applies_to": "perl-dap",
                "disposition": "retire",
            },
            {
                "id": "perl-dap-socket",
                "flag": "--socket",
                "applies_to": "perl-dap",
                "disposition": "retire",
            },
        ],
        "dap_to_dap_relays": [
            {
                "id": "tcp-attach-pls-proxy",
                "path": "crates/perl-dap/src/tcp_attach/mod.rs",
                "disposition": "not_product",
                "authority": "historical",
            }
        ],
        "clients": [
            {
                "id": "vscode",
                "evidence_stage": "shipped",
                "support_status": "supported",
                "launch_mechanism": "DebugAdapterExecutable",
                "transport": "stdio",
                "editor_socket_required": False,
                "evidence_paths": ["vscode-extension/src/debugAdapter.ts"],
                "required_markers": ["DebugAdapterExecutable"],
                "forbidden_markers": ["DebugAdapterServer", "--socket"],
                "support_owner": "#6694",
                "migration": "none",
                "claim_boundary": "stdio child",
                "blocks_retirement": False,
            }
        ],
    }
    inventory["digest"] = SCHEMA.inventory_digest(inventory)
    return inventory


def write_valid_tree(root: Path, inventory: dict | None = None) -> dict:
    inventory = inventory if inventory is not None else valid_inventory()
    write(
        root / "crates/perl-dap/src/main.rs",
        "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
        "fn main() {\n"
        "    if resolve_socket_port(&args.transport).is_some() {\n"
        "        return Err(native_editor_socket_retired());\n"
        "    }\n"
        "}\n",
    )
    write(root / "crates/perl-dap/src/debug_adapter/transport.rs", "fn run_with_io() {}\n")
    write(root / "crates/perl-dap/src/server/lifecycle.rs", "fn run() {}\n")
    write(
        root / "crates/perl-dap/src/backend/peer_launch.rs",
        "fn listen() { TcpListener::bind(resolved.as_slice()).ok(); }\n",
    )
    write(
        root / "crates/perl-dap/src/tcp_attach/mod.rs",
        "struct TcpAttach;\nVS Code <-> Native DAP Adapter <-> TCP Socket <-> Perl::LanguageServer DAP\n",
    )
    write(
        root / "crates/perl-lsp-rs-core/src/runtime/launcher/mod.rs",
        "pub socket: bool\npub port: Option<u16>\n",
    )
    write(root / "crates/perl-dap/README.md", "perl-dap --stdio\n")
    write(
        root / "vscode-extension/src/debugAdapter.ts",
        "new DebugAdapterExecutable(command, buildDapExecutableArgs())\n",
    )
    write(root / ".ci/dap/editor-transport-inventory.v1.json", json.dumps(inventory, indent=2))
    return inventory


def observed(role: str | None = None, *, instrument: str = "linux_procfs") -> dict:
    inventory = [] if role is None else [{"port": 5000, "role": role}]
    return {"instrument": instrument, "inventory": inventory}


def base_mode(mode: str, **overrides: object) -> dict:
    row = {
        "mode": mode,
        "editor_transport": "stdio",
        "listeners": observed() if mode != "external_peer_listen" else observed("debugger_peer"),
        "historical_port_probes": {"13603": "connection_refused"},
        "old_cli_refusal": {"verdict": "pass", "failed_before_bind": True},
        "dap_discriminator": {"verdict": "pass", "initialize": "stdio"},
        "peer_authentication": {"class": "not_applicable" if mode == "native" else "authenticated"},
        "cross_session_replay": {"verdict": "pass" if mode != "native" else "not_applicable"},
        "stdout_stderr_purity": {"verdict": "pass"},
        "cleanup": {"state": "clean"},
        "verdict": "pass",
    }
    row.update(overrides)
    return row


def valid_runtime_receipt(**overrides: object) -> dict:
    receipt = {
        "schema_version": MODULE.SECURITY_SCHEMA,
        "candidate": {"git_sha": "abc123", "tree": "clean"},
        "binary": {
            "path": "/tmp/perl-dap",
            "observed_path": "/tmp/perl-dap",
            "sha256": "a" * 64,
            "observed_sha256": "a" * 64,
            "source": "cargo_bin_exe",
        },
        "runner": {"os": "linux", "arch": "x86_64"},
        "modes": [
            base_mode("native"),
            base_mode("external_peer_connect"),
            base_mode("external_peer_listen"),
        ],
        "static_recurrence": {"verdict": "pass", "error_count": 0, "errors": []},
        "limitations": ["installed client remains #6694"],
        "verdict": "pass",
    }
    receipt.update(overrides)
    return receipt


class MissingInstrumentIsNotZero(unittest.TestCase):
    def test_missing_socket_instrument_is_instrument_failure(self) -> None:
        self.assertEqual(MODULE.socket_observation_verdict(None), "instrument_failure")
        self.assertEqual(
            MODULE.socket_observation_verdict({"instrument": "missing", "inventory": []}),
            "instrument_failure",
        )
        self.assertEqual(
            MODULE.socket_observation_verdict({"instrument": "error", "inventory": []}),
            "instrument_failure",
        )

    def test_observed_empty_inventory_may_pass(self) -> None:
        self.assertEqual(MODULE.socket_observation_verdict(observed()), "pass")

    def test_linux_procfs_without_inventory_is_instrument_failure(self) -> None:
        self.assertEqual(
            MODULE.socket_observation_verdict({"instrument": "linux_procfs"}),
            "instrument_failure",
        )

    def test_unsupported_platform_is_not_proven_not_zero(self) -> None:
        self.assertEqual(
            MODULE.socket_observation_verdict(
                {"instrument": "unsupported_platform", "inventory": []}
            ),
            "not_proven",
        )

    def test_combine_does_not_promote_missing_to_pass(self) -> None:
        self.assertEqual(MODULE.combine_verdicts([]), "not_proven")
        self.assertEqual(
            MODULE.combine_verdicts(["pass", "instrument_failure"]),
            "instrument_failure",
        )
        self.assertEqual(MODULE.combine_verdicts(["pass", "not_proven"]), "not_proven")
        self.assertEqual(MODULE.combine_verdicts(["failed", "not_proven"]), "failed")


class RoleConfusion(unittest.TestCase):
    def test_reintroduced_native_editor_listener_fails(self) -> None:
        verdict, errors = MODULE.classify_listener_roles(
            "native", observed("editor_dap")
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("editor_dap" in item for item in errors), errors)

    def test_bind_editor_listener_in_peer_mode_fails(self) -> None:
        verdict, errors = MODULE.classify_listener_roles(
            "external_peer_connect", observed("editor_dap")
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("editor_dap" in item for item in errors), errors)

    def test_peer_listen_labeled_as_editor_is_role_confusion(self) -> None:
        verdict, errors = MODULE.classify_listener_roles(
            "external_peer_listen", observed("editor_dap")
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("role confusion" in item for item in errors), errors)

    def test_peer_listen_unclassified_is_not_zero_pass(self) -> None:
        verdict, errors = MODULE.classify_listener_roles(
            "external_peer_listen", observed()
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("positively classify" in item for item in errors), errors)

    def test_peer_listen_two_listeners_is_not_a_single_peer(self) -> None:
        observation = {
            "instrument": "linux_procfs",
            "inventory": [
                {"port": 5000, "role": "debugger_peer"},
                {"port": 5001, "role": "debugger_peer"},
            ],
        }
        verdict, errors = MODULE.classify_listener_roles("external_peer_listen", observation)
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("exactly one debugger_peer" in item for item in errors), errors)
        verdict, errors = MODULE.classify_listener_roles(
            "external_peer_listen", observed("debugger_peer")
        )
        self.assertEqual(verdict, "pass", errors)
        self.assertEqual(errors, [])

    def test_inventory_bind_site_role_confusion_is_rejected(self) -> None:
        inventory = valid_inventory()
        inventory["bind_sites"][0]["role"] = "editor"
        errors = MODULE.scan_bind_site_role_confusion(inventory)
        self.assertTrue(
            any("debugger-peer TCP as an editor listener" in item for item in errors),
            errors,
        )


class StaleBinaryAndCleanup(unittest.TestCase):
    def test_stale_binary_hash_fails(self) -> None:
        verdict, errors = MODULE.identity_verdict(
            "/tmp/perl-dap",
            "/tmp/perl-dap",
            "a" * 64,
            "b" * 64,
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("stale or other perl-dap binary hash" in item for item in errors))

    def test_other_binary_path_fails(self) -> None:
        verdict, errors = MODULE.identity_verdict(
            "/tmp/perl-dap",
            "/usr/bin/perl-dap",
            "a" * 64,
            "a" * 64,
        )
        self.assertEqual(verdict, "failed")
        self.assertTrue(any("stale or other perl-dap binary path" in item for item in errors))

    def test_missing_identity_is_not_proven(self) -> None:
        verdict, errors = MODULE.identity_verdict(None, None, None, None)
        self.assertEqual(verdict, "not_proven")
        self.assertTrue(errors)

    def test_cleanup_unknown_is_not_pass(self) -> None:
        verdict, errors = MODULE.cleanup_verdict("unknown")
        self.assertEqual(verdict, "not_proven")
        self.assertTrue(any("cleanup unknown" in item for item in errors))

    def test_cleanup_leaked_fails(self) -> None:
        verdict, _errors = MODULE.cleanup_verdict("leaked")
        self.assertEqual(verdict, "failed")


class SecretLeakageAndReceiptPassPromotion(unittest.TestCase):
    def test_peer_token_in_receipt_is_rejected(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"][2]["peer_authentication"] = {
            "class": "authenticated",
            "token": "0123456789abcdef0123456789abcdef",
        }
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(any("secret material" in item for item in errors), errors)

    def test_canary_in_receipt_is_rejected(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["limitations"].append("dap-10567-peer-token-canary")
        errors = MODULE.validate_receipt(
            receipt, canaries=["dap-10567-peer-token-canary"]
        )
        self.assertTrue(any("canary" in item for item in errors), errors)

    def test_missing_instrument_row_cannot_declare_pass(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"][0]["listeners"] = {"instrument": "missing", "inventory": []}
        receipt["modes"][0]["verdict"] = "pass"
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(
            any("declared pass" in item and "observation" in item for item in errors),
            errors,
        )

    def test_cleanup_unknown_row_cannot_declare_pass(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"][0]["cleanup"] = {"state": "unknown"}
        receipt["modes"][0]["verdict"] = "pass"
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(any("declared pass" in item for item in errors), errors)

    def test_skipped_mode_cannot_be_overall_pass(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"] = [base_mode("native"), base_mode("external_peer_connect")]
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(any("skipped modes" in item for item in errors), errors)

    def test_valid_receipt_has_no_schema_errors(self) -> None:
        self.assertEqual(MODULE.validate_receipt(valid_runtime_receipt()), [])

    def test_failed_cli_refusal_cannot_declare_pass(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"][0]["old_cli_refusal"] = {
            "verdict": "failed",
            "failed_before_bind": False,
        }
        receipt["modes"][0]["verdict"] = "pass"
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(any("subproof" in item for item in errors), errors)

    def test_failed_dap_discriminator_cannot_declare_pass(self) -> None:
        receipt = valid_runtime_receipt()
        receipt["modes"][1]["dap_discriminator"] = {
            "verdict": "failed",
            "initialize": "tcp",
        }
        receipt["modes"][1]["verdict"] = "pass"
        errors = MODULE.validate_receipt(receipt)
        self.assertTrue(any("subproof" in item for item in errors), errors)

    def test_check_rejects_non_pass_runtime_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            receipt = valid_runtime_receipt()
            receipt["verdict"] = "failed"
            runtime_path = root / "runtime.json"
            runtime_path.write_text(json.dumps(receipt), encoding="utf-8")
            static_path = root / "static.json"
            rc = MODULE.main(
                [
                    "check",
                    "--root",
                    str(root),
                    "--manifest",
                    str(root / ".ci/dap/editor-transport-inventory.v1.json"),
                    "--receipt",
                    str(static_path),
                    "--runtime-receipt",
                    str(runtime_path),
                ]
            )
            self.assertEqual(rc, 1)

    def test_check_accepts_pass_runtime_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            runtime_path = root / "runtime.json"
            runtime_path.write_text(json.dumps(valid_runtime_receipt()), encoding="utf-8")
            static_path = root / "static.json"
            rc = MODULE.main(
                [
                    "check",
                    "--root",
                    str(root),
                    "--manifest",
                    str(root / ".ci/dap/editor-transport-inventory.v1.json"),
                    "--receipt",
                    str(static_path),
                    "--runtime-receipt",
                    str(runtime_path),
                ]
            )
            self.assertEqual(rc, 0)


class SourceReintroductionFalsifiers(unittest.TestCase):
    def test_reintroduced_run_socket_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/debug_adapter/transport.rs",
                "pub fn run_socket(port: u16) { TcpListener::bind((\"127.0.0.1\", port)).ok(); }\n",
            )
            errors = MODULE.static_security_errors(root, valid_inventory())
            self.assertTrue(
                any("run_socket" in item or "TcpListener::bind" in item for item in errors),
                errors,
            )

    def test_bind_editor_listener_in_peer_helpers_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn bind_editor_listener() { std::net::TcpListener::bind((\"127.0.0.1\", 1)).ok(); }\n"
                "fn run_external_peer_bridge() { bind_editor_listener(); }\n"
                "fn main() { let _ = native_editor_socket_retired(); }\n",
            )
            errors = SCAN.scan_retired_native_editor_listener(root, valid_inventory())
            self.assertTrue(
                any("bind_editor_listener" in item for item in errors),
                errors,
            )

    def test_accept_and_ignore_socket_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn main() { start_stdio(); }\n",
            )
            errors = SCAN.scan_retired_native_editor_listener(root, valid_inventory())
            self.assertTrue(
                any("native_editor_socket_retired" in item for item in errors),
                errors,
            )

    def test_socket_relay_claiming_stdio_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            inventory["dap_to_dap_relays"][0]["authority"] = "product"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = MODULE.static_security_errors(root, inventory)
            self.assertTrue(
                any("must not be a product editor transport" in item for item in errors),
                errors,
            )


if __name__ == "__main__":
    unittest.main()
