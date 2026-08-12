#!/usr/bin/env python3
"""Focused tests for scripts/release_build_identity.py."""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("release_build_identity.py")
SPEC = importlib.util.spec_from_file_location(
    "release_build_identity", MODULE_PATH
)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = subject
SPEC.loader.exec_module(subject)


def valid_mapping() -> dict[str, str]:
    return {
        "schema_version": subject.INPUT_SCHEMA,
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "release_version": "0.18.0",
        "source_revision": "a" * 40,
        "source_tree_digest": "b" * 64,
        "target": "x86_64-unknown-linux-gnu",
        "profile": "release",
        "candidate_identity": "v0.18.0-rc1",
        "artifact_role": "archive",
        "product_identity_contract_digest": "c" * 64,
        "release_topology_digest": "d" * 64,
        "toolchain_digest": "e" * 64,
    }


def valid_packet(
    executable: str, package: str, role: str
) -> dict[str, object]:
    identity = valid_mapping()
    return {
        "schema_version": subject.PACKET_SCHEMA,
        "product": {
            "name": "perl-lsp",
            "public_repository": "EffortlessMetrics/perl-lsp",
            "development_repository": "EffortlessMetrics/perl-lsp-swarm",
        },
        "binary": {
            "executable": executable,
            "cargo_package": package,
            "role": role,
            "version": identity["release_version"],
        },
        "build": {
            "source_revision": identity["source_revision"],
            "source_tree_digest": identity["source_tree_digest"],
            "target": identity["target"],
            "profile": "release",
            "identity_state": "exact",
        },
        "artifact": {
            "role": "archive",
            "candidate_identity": identity["candidate_identity"],
        },
        "compatibility": {
            "expected_product_identity_version": 1,
            "dap_posture": "preview",
        },
    }


class ReleaseBuildIdentityTests(unittest.TestCase):
    def test_closed_input_rejects_unknown_fields(self) -> None:
        value = valid_mapping()
        value["forged"] = "accepted"
        with self.assertRaisesRegex(subject.BuildIdentityError, "unknown"):
            subject.ReleaseBuildIdentity.from_mapping(value)

    def test_release_identity_rejects_workspace_or_moving_inputs(self) -> None:
        for field, value in [
            ("source_revision", "main"),
            ("source_tree_digest", "f" * 63),
            ("target", "/tmp/custom-target.json"),
            ("profile", "debug"),
            ("candidate_identity", "../../private"),
            ("artifact_role", "unknown"),
        ]:
            with self.subTest(field=field):
                candidate = valid_mapping()
                candidate[field] = value
                with self.assertRaises(subject.BuildIdentityError):
                    subject.ReleaseBuildIdentity.from_mapping(candidate)

    def test_topology_must_bind_exact_source_target_and_members(self) -> None:
        identity = valid_mapping()
        topology = {
            "schema": 1,
            "release": identity["release_version"],
            "frozen_product_sha": identity["source_revision"],
            "prepared_swarm_sha": None,
            "binary_targets": [
                {
                    "target": identity["target"],
                    "required_members": [
                        "perllsp",
                        "perl-dap",
                        "README.md",
                    ],
                }
            ],
        }
        subject.validate_topology(
            topology,
            release_version=identity["release_version"],
            source_revision=identity["source_revision"],
            target=identity["target"],
        )
        topology["binary_targets"][0]["required_members"].remove(
            "perl-dap"
        )
        with self.assertRaisesRegex(subject.BuildIdentityError, "perl-dap"):
            subject.validate_topology(
                topology,
                release_version=identity["release_version"],
                source_revision=identity["source_revision"],
                target=identity["target"],
            )

    def test_packet_validation_is_load_bearing_for_source_tree(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        packet = valid_packet("perllsp", "perllsp", "server")
        subject.validate_packet(
            packet,
            identity=identity,
            executable="perllsp",
            package="perllsp",
            role="server",
        )
        packet["build"]["source_tree_digest"] = "f" * 64
        with self.assertRaisesRegex(
            subject.BuildIdentityError, "build identity mismatch"
        ):
            subject.validate_packet(
                packet,
                identity=identity,
                executable="perllsp",
                package="perllsp",
                role="server",
            )

    def test_packet_cannot_self_attest_final_executable_digest(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        packet = valid_packet("perl-dap", "perl-dap", "dap")
        packet["artifact"]["digest"] = "f" * 64
        with self.assertRaisesRegex(subject.BuildIdentityError, "self-attest"):
            subject.validate_packet(
                packet,
                identity=identity,
                executable="perl-dap",
                package="perl-dap",
                role="dap",
            )

    def test_prepare_binds_authorities_and_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            product = root / "policy" / "product-identity.toml"
            product.parent.mkdir()
            product.write_text(
                """schema_version = 1
[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"
""",
                encoding="utf-8",
            )
            topology = root / "target" / "release-topology.json"
            topology.parent.mkdir()
            topology.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "release": "0.18.0",
                        "frozen_product_sha": "a" * 40,
                        "prepared_swarm_sha": None,
                        "binary_targets": [
                            {
                                "target": "x86_64-unknown-linux-gnu",
                                "required_members": [
                                    "perllsp",
                                    "perl-dap",
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            output = root / "target" / "identity.json"
            args = argparse.Namespace(
                workspace_root=root,
                repository="EffortlessMetrics/perl-lsp-swarm",
                release_version="0.18.0",
                source_revision="a" * 40,
                target="x86_64-unknown-linux-gnu",
                candidate_identity="v0.18.0-rc1",
                artifact_role="archive",
                release_topology=topology,
                product_identity=product,
                runner="cargo",
                output=output,
                github_env=None,
            )
            with (
                mock.patch.object(subject, "tracked_tree_is_clean"),
                mock.patch.object(
                    subject, "current_revision", return_value="a" * 40
                ),
                mock.patch.object(
                    subject,
                    "canonical_tree_digest",
                    return_value="b" * 64,
                ),
                mock.patch.object(
                    subject, "toolchain_digest", return_value="c" * 64
                ),
            ):
                first = subject.prepare_identity(args)
                first_bytes = output.read_bytes()
                second = subject.prepare_identity(args)
                second_bytes = output.read_bytes()
            self.assertEqual(first, second)
            self.assertEqual(first_bytes, second_bytes)
            self.assertEqual(json.loads(first_bytes), first.as_dict())

    def test_build_commands_are_structured_and_share_one_environment(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        server = subject.build_command(
            "cargo", identity, "perllsp", "perllsp"
        )
        dap = subject.build_command(
            "cargo", identity, "perl-dap", "perl-dap"
        )
        self.assertEqual(server[:2], ["cargo", "build"])
        self.assertEqual(dap[:2], ["cargo", "build"])
        self.assertIn(identity.target, server)
        self.assertIn("--locked", server)
        env = subject.build_environment(identity)
        self.assertEqual(
            env["PERL_LSP_BUILD_REVISION"], identity.source_revision
        )
        self.assertEqual(
            env["PERL_LSP_SOURCE_TREE_DIGEST"],
            identity.source_tree_digest,
        )
        self.assertEqual(
            env["PERL_LSP_CANDIDATE_ID"], identity.candidate_identity
        )
        self.assertEqual(env["PERL_LSP_ARTIFACT_ROLE"], "archive")

    def test_cross_identity_command_executes_the_declared_target(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        command = subject.identity_command(
            "cross",
            identity,
            "perllsp",
            "perllsp",
            Path("/ignored"),
        )
        self.assertEqual(command[0:2], ["cross", "run"])
        self.assertIn(identity.target, command)
        self.assertEqual(command[-2:], ["--", "--identity-json"])

    def test_github_env_projection_is_closed_and_deterministic(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "github.env"
            subject.append_github_env(path, identity)
            first = path.read_text(encoding="utf-8")
            subject.append_github_env(path, identity)
            second = path.read_text(encoding="utf-8")
        expected_lines = [
            f"PERL_LSP_BUILD_REVISION={identity.source_revision}",
            f"PERL_LSP_SOURCE_TREE_DIGEST={identity.source_tree_digest}",
            f"PERL_LSP_TARGET_TRIPLE={identity.target}",
            "PERL_LSP_BUILD_PROFILE=release",
            f"PERL_LSP_CANDIDATE_ID={identity.candidate_identity}",
            "PERL_LSP_ARTIFACT_ROLE=archive",
        ]
        self.assertEqual(first.splitlines(), expected_lines)
        self.assertEqual(second.splitlines(), expected_lines + expected_lines)

    def test_verify_receipt_names_external_build_execution(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            input_path = root / "identity.json"
            receipt_path = root / "receipt.json"
            input_path.write_bytes(
                subject.canonical_json_bytes(identity.as_dict())
            )
            args = argparse.Namespace(
                workspace_root=root,
                input=input_path,
                runner="cargo",
                receipt=receipt_path,
            )
            observed = [
                {"role": "server", "executable": "perllsp"},
                {"role": "dap", "executable": "perl-dap"},
            ]
            with (
                mock.patch.object(subject, "verify_checkout"),
                mock.patch.object(
                    subject,
                    "toolchain_digest",
                    return_value=identity.toolchain_digest,
                ),
                mock.patch.object(
                    subject, "execute_identity", side_effect=observed
                ),
            ):
                receipt = subject.verify_binaries(
                    args, build_execution="external_release_workflow"
                )
        self.assertEqual(
            receipt["build_execution"], "external_release_workflow"
        )
        self.assertEqual(len(receipt["build_commands"]), 2)
        self.assertEqual(receipt["binaries"], observed)

    def test_atomic_write_creates_missing_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "nested" / "receipt.json"
            subject.write_atomic(path, b'{"ok":true}\n')
            self.assertEqual(path.read_bytes(), b'{"ok":true}\n')


if __name__ == "__main__":
    unittest.main()
