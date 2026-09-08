#!/usr/bin/env python3
"""Falsifiers for scripts/ci/dap_editor_transport_inventory.py."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_editor_transport_inventory.py"
SPEC = importlib.util.spec_from_file_location("dap_editor_transport_inventory", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
SCHEMA = MODULE.sys.modules["dap_editor_transport_schema"]


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
        "first_mile_surfaces": [
            "crates/perl-dap/README.md",
            "docs/tutorials/DAP_USER_GUIDE.md",
        ],
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
                "paths": ["crates/perl-dap/tests/dap_coverage_audit_tests.rs"],
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
                "id": "helix",
                "evidence_stage": "planned",
                "support_status": "planned",
                "launch_mechanism": "declared perl-dap --stdio",
                "transport": "stdio",
                "editor_socket_required": False,
                "evidence_paths": ["docs/EDITORS/HELIX_SETUP.md"],
                "required_markers": ["perl-dap --stdio"],
                "forbidden_markers": ["--socket"],
                "support_owner": "#7742",
                "migration": "none; already stdio",
                "claim_boundary": "declared stdio; no actual Helix DAP receipt",
                "blocks_retirement": False,
            },
            {
                "id": "sublime",
                "evidence_stage": "package",
                "support_status": "preview",
                "launch_mechanism": "StdioTransport",
                "transport": "stdio",
                "editor_socket_required": False,
                "evidence_paths": ["clients/sublime/LSP-perllsp/dap_support.py"],
                "required_markers": ['"--stdio"'],
                "forbidden_markers": ["--socket"],
                "support_owner": "#7711",
                "migration": "none; already stdio",
                "claim_boundary": "package stdio launch",
                "blocks_retirement": False,
            },
            {
                "id": "unsupported-generic-tcp",
                "evidence_stage": "none",
                "support_status": "unsupported",
                "launch_mechanism": "invented",
                "transport": "editor_tcp",
                "editor_socket_required": True,
                "evidence_paths": [],
                "required_markers": [],
                "forbidden_markers": [],
                "support_owner": "#10564",
                "migration": "n/a",
                "claim_boundary": "theoretical TCP client cannot block retirement",
                "blocks_retirement": False,
                "dap_claimed": False,
            },
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
                "migration": "none; already stdio",
                "claim_boundary": "shipped executable descriptor is stdio",
                "blocks_retirement": False,
            },
        ],
    }
    inventory["digest"] = SCHEMA.inventory_digest(inventory)
    return inventory


def write_valid_tree(root: Path, inventory: dict | None = None) -> dict:
    payload = inventory if inventory is not None else valid_inventory()
    write(
        root / "crates/perl-dap/src/main.rs",
        "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
        "fn run_external_peer_bridge_stdio() {}\n"
        "fn run_external_peer_listen() {}\n"
        "fn main() { let _ = native_editor_socket_retired(); }\n",
    )
    write(
        root / "crates/perl-dap/src/debug_adapter/transport.rs",
        "fn run_with_io() {}\n",
    )
    write(
        root / "crates/perl-dap/src/server/lifecycle.rs",
        "impl DapServer { pub fn run(&mut self) {}\n}\n",
    )
    write(
        root / "crates/perl-dap/src/backend/peer_launch.rs",
        "fn listen() { TcpListener::bind(resolved.as_slice()).ok(); }\n",
    )
    write(
        root / "crates/perl-dap/src/tcp_attach/mod.rs",
        "//! VS Code <-> Native DAP Adapter <-> TCP Socket <-> Perl::LanguageServer DAP\n"
        "pub use session::TcpAttachSession;\n",
    )
    write(
        root / "crates/perl-lsp-rs-core/src/runtime/launcher/mod.rs",
        "pub struct TransportArgs { pub socket: bool, pub port: Option<u16> }\n",
    )
    write(root / "crates/perl-dap/README.md", "# perl-dap\n\nperl-dap --stdio\n")
    write(root / "docs/tutorials/DAP_USER_GUIDE.md", "# Guide\n\nperl-dap --stdio\n")
    write(
        root / "vscode-extension/src/debugAdapter.ts",
        "return new vscode.DebugAdapterExecutable(dapPath, args);\n",
    )
    write(
        root / "clients/sublime/LSP-perllsp/dap_support.py",
        "return [str(path.resolve()), \"--stdio\"]\n",
    )
    write(root / "docs/EDITORS/HELIX_SETUP.md", "Helix uses perl-dap --stdio\n")
    write(
        root / "crates/perl-dap/tests/dap_coverage_audit_tests.rs",
        "let _ = server.run_socket(port);\n",
    )
    write(
        root / ".ci/dap/editor-transport-inventory.v1.json",
        json.dumps(payload, indent=2) + "\n",
    )
    return payload


def run_check(root: Path) -> list[str]:
    inventory = json.loads((root / ".ci/dap/editor-transport-inventory.v1.json").read_text())
    return MODULE.check_inventory(root, inventory)


class EditorTransportInventoryTests(unittest.TestCase):
    def test_valid_tree_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            self.assertEqual(run_check(root), [])

    def test_vscode_executable_descriptor_resolves_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "vscode-extension/src/debugAdapter.ts",
                "return new vscode.DebugAdapterServer(13603);\n",
            )
            errors = run_check(root)
            self.assertTrue(any("vscode" in item and "DebugAdapterExecutable" in item for item in errors))
            self.assertTrue(any("DebugAdapterServer" in item for item in errors))

    def test_sublime_and_helix_declared_routes_resolve_stdio(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(root / "clients/sublime/LSP-perllsp/dap_support.py", "return [str(path)]\n")
            write(root / "docs/EDITORS/HELIX_SETUP.md", "Helix uses TCP\n")
            errors = run_check(root)
            self.assertTrue(any("sublime" in item and "--stdio" in item for item in errors))
            self.assertTrue(any("helix" in item and "perl-dap --stdio" in item for item in errors))

    def test_native_socket_surface_is_classified_retire_not_supported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["transports"]:
                if row["id"] == "native-editor-tcp":
                    row["disposition"] = "retain"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("native-editor-tcp" in item and "retire" in item for item in errors))

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["cli_flags"]:
                if row["flag"] == "--socket":
                    row["disposition"] = "retain"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("--socket" in item and "retire" in item for item in errors))

    def test_external_peer_editor_wrapper_retire_keeps_debugger_peer_tcp(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["transports"]:
                if row["id"] == "external-peer-editor-tcp":
                    row["disposition"] = "retain"
                if row["id"] == "debugger-peer-tcp":
                    row["disposition"] = "retire"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("external-peer-editor-tcp" in item and "retire" in item for item in errors))
            self.assertTrue(any("debugger-peer-tcp" in item and "retain" in item for item in errors))

    def test_generic_dap_attach_is_not_editor_tcp(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["transports"]:
                if row["id"] == "dap-attach":
                    row["role"] = "editor"
                    row["kind"] = "dap_attach"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("dap-attach" in item and "editor transport" in item for item in errors))

    def test_test_only_loopback_cannot_satisfy_product_client_row(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["clients"]:
                if row["id"] == "vscode":
                    row["evidence_paths"] = ["crates/perl-dap/tests/dap_coverage_audit_tests.rs"]
                    row["required_markers"] = ["run_socket"]
                    row["forbidden_markers"] = []
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("test-only evidence" in item for item in errors))

    def test_product_client_markers_cannot_be_satisfied_by_a_listed_test_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["clients"]:
                if row["id"] == "vscode":
                    row["evidence_paths"] = [
                        "vscode-extension/src/debugAdapter.ts",
                        "crates/perl-dap/tests/dap_coverage_audit_tests.rs",
                    ]
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            write(
                root / "vscode-extension/src/debugAdapter.ts",
                "return new vscode.DebugAdapterServer(13603);\n",
            )
            write(
                root / "crates/perl-dap/tests/dap_coverage_audit_tests.rs",
                "return new vscode.DebugAdapterExecutable(dapPath, args);\n",
            )
            errors = run_check(root)
            self.assertTrue(any("vscode" in item and "DebugAdapterExecutable" in item for item in errors))
            self.assertTrue(any("DebugAdapterServer" in item for item in errors))

    def test_stale_docs_saying_socket_is_a_run_mode_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "docs/tutorials/DAP_USER_GUIDE.md",
                "## Attach Over TCP\n\nUse socket mode:\n\nperl-dap --socket --port 13603\n",
            )
            errors = run_check(root)
            self.assertTrue(any("stale editor-socket product run mode" in item for item in errors))
            self.assertTrue(any("perl-dap --socket" in item for item in errors))

    def test_invented_unsupported_client_cannot_block_retirement(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["clients"]:
                if row["id"] == "unsupported-generic-tcp":
                    row["blocks_retirement"] = True
                    row["dap_claimed"] = False
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("unsupported-generic-tcp" in item and "must not block" in item for item in errors))

    def test_supported_client_requiring_tcp_stops_the_train(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["clients"]:
                if row["id"] == "vscode":
                    row["editor_socket_required"] = True
                    row["blocks_retirement"] = True
                    row["transport"] = "editor_tcp"
                    row["forbidden_markers"] = []
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any(item.startswith("STOP_TRAIN:") and "vscode" in item for item in errors))
            self.assertTrue(any("ruling_status=accepted despite" in item for item in errors))

    def test_digest_is_order_independent_but_file_order_is_required(self) -> None:
        inventory = valid_inventory()
        shuffled = json.loads(json.dumps(inventory))
        shuffled["transports"] = list(reversed(shuffled["transports"]))
        self.assertEqual(SCHEMA.inventory_digest(inventory), SCHEMA.inventory_digest(shuffled))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root, shuffled)
            errors = run_check(root)
            self.assertTrue(any("transports must be sorted by id" in item for item in errors))

    def test_unlabeled_dap_to_dap_proxy_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/mystery_proxy.rs",
                "//! VS Code <-> Native DAP Adapter <-> TCP Socket <-> Perl::LanguageServer DAP\n",
            )
            errors = run_check(root)
            self.assertTrue(any("mystery_proxy.rs" in item and "not inventoried" in item for item in errors))

    def test_unclaimed_production_bind_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/mystery.rs",
                "fn rogue() { TcpListener::bind((\"127.0.0.1\", 0)).ok(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(any("mystery.rs" in item and "no bind_site owner" in item for item in errors))

    def test_second_production_bind_in_an_owned_file_requires_its_own_bind_site(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/backend/peer_launch.rs",
                "fn listen() { TcpListener::bind(resolved.as_slice()).ok(); }\n"
                "fn bind_second_listener() { TcpListener::bind((\"127.0.0.1\", 2)).ok(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any(
                    "peer_launch.rs" in item and "bind_site" in item and "count" in item
                    for item in errors
                )
            )

    def test_cfg_test_binds_are_not_production_owners(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/backend/external_peer.rs",
                "fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn bind() { TcpListener::bind((\"127.0.0.1\", 0)).ok(); }\n}\n",
            )
            self.assertEqual(run_check(root), [])

    def test_cfg_test_use_does_not_hide_production_bind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/backend/peer_launch.rs",
                "#[cfg(test)]\nuse perl_tdd_support::must;\nfn listen() { TcpListener::bind(resolved.as_slice()).ok(); }\n",
            )
            self.assertEqual(run_check(root), [])

    def test_returned_native_editor_listener_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            inventory["bind_sites"].append(
                {
                    "id": "native-editor-socket",
                    "path": "crates/perl-dap/src/debug_adapter/transport.rs",
                    "role": "editor",
                    "transport_id": "native-editor-tcp",
                    "disposition": "retire",
                }
            )
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            write(
                root / "crates/perl-dap/src/debug_adapter/transport.rs",
                "fn bind_socket_listener(port: u16) { TcpListener::bind((\"127.0.0.1\", port)).ok(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any("native editor TCP bind site" in item and "returned" in item for item in errors)
            )
            self.assertTrue(
                any("TcpListener::bind" in item and "transport.rs" in item for item in errors)
            )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/server/lifecycle.rs",
                "impl DapServer { pub fn run_socket(&mut self, port: u16) {}\n}\n",
            )
            errors = run_check(root)
            self.assertTrue(any("run_socket" in item and "lifecycle.rs" in item for item in errors))

    def test_native_admission_calling_bind_editor_listener_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn bind_editor_listener() { std::net::TcpListener::bind((\"127.0.0.1\", 1)).ok(); }\n"
                "fn run_external_peer_bridge() { bind_editor_listener(); }\n"
                "fn run_external_peer_listen() { bind_editor_listener(); }\n"
                "fn main() {\n"
                "    if resolve_socket_port(&args.transport).is_some() {\n"
                "        let _ = bind_editor_listener();\n"
                "    }\n"
                "    let _ = native_editor_socket_retired();\n"
                "}\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any("fn main regained a native editor bind_editor_listener" in item for item in errors),
                errors,
            )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn bind_editor_listener() { std::net::TcpListener::bind((\"127.0.0.1\", 1)).ok(); }\n"
                "fn run_external_peer_bridge() { bind_editor_listener(); }\n"
                "fn run_external_peer_listen() { bind_editor_listener(); }\n"
                "fn run_native_socket() { bind_editor_listener(); }\n"
                "fn main() { let _ = native_editor_socket_retired(); let _ = run_native_socket(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any("fn run_native_socket calls bind_editor_listener" in item for item in errors),
                errors,
            )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn bind_editor_listener() { std::net::TcpListener::bind((\"127.0.0.1\", 1)).ok(); }\n"
                "fn run_external_peer_bridge() { bind_editor_listener(); }\n"
                "fn run_external_peer_listen() { bind_editor_listener(); }\n"
                "fn main() { let _ = native_editor_socket_retired(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any("bind_editor_listener returned after #10566" in item for item in errors),
                errors,
            )
            self.assertTrue(
                any(
                    "fn run_external_peer_bridge calls bind_editor_listener after #10566" in item
                    for item in errors
                ),
                errors,
            )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_valid_tree(root)
            write(
                root / "crates/perl-dap/src/main.rs",
                "transport: perl_lsp_rs_core::runtime::launcher::TransportArgs\n"
                "fn bind_editor_listener() { std::net::TcpListener::bind((\"127.0.0.1\", 1)).ok(); }\n"
                "fn run_external_peer_bridge() { bind_editor_listener(); }\n"
                "fn run_external_peer_listen() { bind_editor_listener(); }\n"
                "fn main() { server.run(); }\n",
            )
            errors = run_check(root)
            self.assertTrue(
                any("native_editor_socket_retired" in item and "fn main" in item for item in errors),
                errors,
            )

    def test_debugger_peer_listener_mislabeled_as_editor_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = valid_inventory()
            for row in inventory["bind_sites"]:
                if row["id"] == "debugger-peer-listen":
                    row["role"] = "editor"
                    row["transport_id"] = "debugger-peer-tcp"
                    row["disposition"] = "retire"
            inventory["digest"] = SCHEMA.inventory_digest(inventory)
            write_valid_tree(root, inventory)
            errors = run_check(root)
            self.assertTrue(any("mislabels a debugger-peer listener" in item for item in errors))

    def test_live_tree_matches_frozen_inventory(self) -> None:
        repo = Path(__file__).resolve().parents[2]
        manifest = repo / ".ci/dap/editor-transport-inventory.v1.json"
        inventory = json.loads(manifest.read_text(encoding="utf-8"))
        errors = MODULE.check_inventory(repo, inventory)
        self.assertEqual(errors, [], errors)


if __name__ == "__main__":
    unittest.main()
