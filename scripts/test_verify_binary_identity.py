#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("verify_binary_identity.py")
SPEC = importlib.util.spec_from_file_location("verify_binary_identity", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
verify_binary_identity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_binary_identity
SPEC.loader.exec_module(verify_binary_identity)

ExpectedBinary = verify_binary_identity.ExpectedBinary
ObservedBinary = verify_binary_identity.ObservedBinary
VerificationError = verify_binary_identity.VerificationError
observe = verify_binary_identity.observe
verify = verify_binary_identity.verify
main = verify_binary_identity.main
SCHEMA_VERSION = verify_binary_identity.SCHEMA_VERSION


def packet(
    *,
    executable: str = "perllsp",
    package: str = "perllsp",
    role: str = "server",
    version: str = "0.18.0",
    target: str | None = "x86_64-unknown-linux-gnu",
    source: str | None = "abc123",
    candidate: str | None = "rc1",
    digest: str | None = None,
) -> dict[str, object]:
    build: dict[str, object] = {"identity_state": "exact"}
    if target is not None:
        build["target"] = target
    if source is not None:
        build["source_revision"] = source
    artifact: dict[str, object] = {"role": "archive"}
    if candidate is not None:
        artifact["candidate_identity"] = candidate
    if digest is not None:
        artifact["digest"] = digest
    return {
        "schema_version": "perl_lsp.binary_identity.v1",
        "product": {
            "name": "perl-lsp",
            "public_repository": "EffortlessMetrics/perl-lsp",
            "development_repository": "EffortlessMetrics/perl-lsp-swarm",
        },
        "binary": {
            "executable": executable,
            "cargo_package": package,
            "role": role,
            "version": version,
        },
        "build": build,
        "artifact": artifact,
        "compatibility": {
            "expected_product_identity_version": 1,
            "dap_posture": "preview",
        },
    }


def observed(path: str, payload: dict[str, object], role: str) -> ObservedBinary:
    expected = (
        ExpectedBinary(Path(path), "perllsp", "perllsp", "server")
        if role == "server"
        else ExpectedBinary(Path(path), "perl-dap", "perl-dap", "dap")
    )
    return ObservedBinary(expected=expected, sha256="deadbeef", packet=payload)


class VerifyBinaryIdentityTests(unittest.TestCase):
    def test_exact_server_and_dap_pair_verifies(self) -> None:
        server = observed("perllsp", packet(), "server")
        dap = observed(
            "perl-dap",
            packet(executable="perl-dap", package="perl-dap", role="dap"),
            "dap",
        )
        receipt = verify(
            server,
            dap,
            expected_version="0.18.0",
            expected_target="x86_64-unknown-linux-gnu",
            expected_candidate="rc1",
            require_dap=True,
        )
        self.assertEqual(receipt["verdict"], "verified")
        self.assertEqual(receipt["reasons"], [])

    def test_same_version_different_source_is_rejected(self) -> None:
        server = observed("perllsp", packet(), "server")
        dap = observed(
            "perl-dap",
            packet(executable="perl-dap", package="perl-dap", role="dap", source="different"),
            "dap",
        )
        receipt = verify(
            server,
            dap,
            expected_version="0.18.0",
            expected_target="x86_64-unknown-linux-gnu",
            expected_candidate="rc1",
            require_dap=True,
        )
        self.assertEqual(receipt["verdict"], "mismatch")
        self.assertIn("server_dap_source_mismatch", receipt["reasons"])

    def test_wrong_role_and_package_are_rejected(self) -> None:
        server = observed(
            "perllsp",
            packet(executable="perl-dap", package="perl-dap", role="dap"),
            "server",
        )
        receipt = verify(
            server,
            None,
            expected_version="0.18.0",
            expected_target="x86_64-unknown-linux-gnu",
            expected_candidate="rc1",
            require_dap=False,
        )
        self.assertEqual(receipt["verdict"], "mismatch")
        self.assertIn("executable_mismatch", receipt["reasons"])
        self.assertIn("cargo_package_mismatch", receipt["reasons"])
        self.assertIn("role_mismatch", receipt["reasons"])

    def test_self_reported_artifact_digest_is_rejected(self) -> None:
        server = observed("perllsp", packet(digest="sha256:claimed"), "server")
        receipt = verify(
            server,
            None,
            expected_version="0.18.0",
            expected_target="x86_64-unknown-linux-gnu",
            expected_candidate="rc1",
            require_dap=False,
        )
        self.assertIn("self_reported_artifact_digest_forbidden", receipt["reasons"])

    def test_missing_required_dap_is_rejected(self) -> None:
        server = observed("perllsp", packet(), "server")
        receipt = verify(
            server,
            None,
            expected_version="0.18.0",
            expected_target="x86_64-unknown-linux-gnu",
            expected_candidate="rc1",
            require_dap=True,
        )
        self.assertIn("dap_required_but_missing", receipt["reasons"])

    def test_observation_hashes_exact_bytes_and_runs_identity_command(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            payload = json.dumps(packet())
            executable.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                f"payload = {payload!r}\n"
                "if sys.argv[1:] != ['--identity-json']:\n"
                "    raise SystemExit(9)\n"
                "print(payload)\n",
                encoding="utf-8",
            )
            executable.chmod(0o755)
            result = observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 2.0)
            self.assertEqual(result.packet["binary"]["role"], "server")
            self.assertEqual(len(result.sha256), 64)

    def test_digest_mismatch_against_trusted_topology_row(self) -> None:
        under_test = ObservedBinary(
            expected=ExpectedBinary(
                Path("staged-perllsp"), "perllsp", "perllsp", "server", expected_sha256="a" * 64
            ),
            sha256="b" * 64,
            packet=packet(),
        )
        receipt = verify(
            under_test,
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        self.assertIn("server_sha256_mismatch", receipt["reasons"])
        self.assertEqual(receipt["verdict"], "mismatch")

    def test_matching_trusted_digest_is_required_for_verified(self) -> None:
        digest = "c" * 64
        under_test = ObservedBinary(
            expected=ExpectedBinary(
                Path("staged-perllsp"), "perllsp", "perllsp", "server", expected_sha256=digest
            ),
            sha256=digest,
            packet=packet(),
        )
        receipt = verify(
            under_test,
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        self.assertEqual(receipt["verdict"], "verified")

    def test_receipt_carries_only_the_closed_packet_projection(self) -> None:
        raw = packet()
        raw["private_path"] = "/home/user/secret"
        raw["binary"]["oversized"] = "x" * 2048
        under_test = ObservedBinary(
            expected=ExpectedBinary(Path("staged-perllsp"), "perllsp", "perllsp", "server"),
            sha256="d" * 64,
            packet=raw,
        )
        receipt = verify(
            under_test,
            None,
            expected_version=raw["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        projected = receipt["binaries"][0]["packet_projection"]
        self.assertNotIn("packet", receipt["binaries"][0])
        self.assertNotIn("private_path", json.dumps(receipt))
        self.assertNotIn("oversized", json.dumps(receipt))
        self.assertEqual(projected["role"], "server")
        self.assertEqual(projected["version"], raw["binary"]["version"])

    @unittest.skipUnless(os.name == "posix", "shebang executables")
    def test_oversized_output_is_killed_while_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            lines = [
                "#!/usr/bin/env python3",
                "import sys",
                "limit = 128 * 1024",
                "print('x' * (limit + 4096), flush=True)",
                "while True:",
                "    pass",
            ]
            executable.write_text("\n".join(lines) + "\n", encoding="utf-8")
            executable.chmod(0o755)
            with self.assertRaises(VerificationError) as caught:
                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 3.0)
            self.assertIn("exceeded its bounded pipe", str(caught.exception))

    def _observed(self, raw=None, digest="e" * 64):
        return ObservedBinary(
            expected=ExpectedBinary(Path("staged-perllsp"), "perllsp", "perllsp", "server"),
            sha256=digest,
            packet=raw if raw is not None else packet(),
        )

    def test_wrong_reported_source_revision_is_a_mismatch(self) -> None:
        receipt = verify(
            self._observed(),
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=packet()["build"]["target"],
            expected_candidate=None,
            expected_source="deadbeef",
            require_dap=False,
        )
        self.assertIn("server_source_mismatch_or_not_proven", receipt["reasons"])
        self.assertEqual(receipt["verdict"], "mismatch")

    def test_matching_reported_source_revision_verifies(self) -> None:
        receipt = verify(
            self._observed(),
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=packet()["build"]["target"],
            expected_candidate=None,
            expected_source=packet()["build"]["source_revision"],
            require_dap=False,
        )
        self.assertEqual(receipt["verdict"], "verified")

    def test_partial_build_identity_state_cannot_verify(self) -> None:
        raw = packet()
        raw["build"]["identity_state"] = "partial"
        receipt = verify(
            self._observed(raw),
            None,
            expected_version=raw["binary"]["version"],
            expected_target=raw["build"]["target"],
            expected_candidate=None,
            require_dap=False,
        )
        self.assertIn("build_identity_state_not_exact", receipt["reasons"])
        self.assertEqual(receipt["verdict"], "mismatch")

    def test_expected_target_is_required_by_the_cli(self) -> None:
        with self.assertRaises(SystemExit):
            main([
                "--server", "perllsp",
                "--expected-version", "0.18.0",
            ])

    def test_receipts_validate_against_the_published_schema(self) -> None:
        jsonschema = importlib.import_module("jsonschema")
        schema_path = Path(__file__).resolve().parent.parent / "schemas" / "install_identity_verification.v1.schema.json"
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        verified = verify(
            self._observed(),
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=packet()["build"]["target"],
            expected_candidate=None,
            require_dap=False,
        )
        jsonschema.validate(instance=verified, schema=schema)
        mismatched = verify(
            self._observed(raw={"unexpected": True} if False else packet()),
            None,
            expected_version="0.0.0-not-the-version",
            expected_target=packet()["build"]["target"],
            expected_candidate=None,
            require_dap=False,
        )
        jsonschema.validate(instance=mismatched, schema=schema)
        not_proven = {
            "schema_version": SCHEMA_VERSION,
            "verdict": "not_proven",
            "reasons": ["staged executable is missing: perllsp"],
        }
        jsonschema.validate(instance=not_proven, schema=schema)
        with self.assertRaises(jsonschema.ValidationError):
            jsonschema.validate(instance={"schema_version": SCHEMA_VERSION, "verdict": "verified", "reasons": []}, schema=schema)
        with self.assertRaises(jsonschema.ValidationError):
            jsonschema.validate(instance={"schema_version": SCHEMA_VERSION, "verdict": "mismatch", "reasons": []}, schema=schema)

    @unittest.skipUnless(os.name == "posix", "shebang executables")
    def test_self_mutating_staged_binary_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            payload = json.dumps(packet())
            lines = [
                "#!/usr/bin/env python3",
                "import os, sys",
                "with open(os.path.abspath(sys.argv[0]), 'a', encoding='utf-8') as handle:",
                "    handle.write('# mutation' + 'x' * 64)",
                f"payload = {payload!r}",
                "print(payload)",
            ]
            executable.write_text('\n'.join(lines) + '\n', encoding="utf-8")
            executable.chmod(0o755)
            with self.assertRaises(VerificationError) as caught:
                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 5.0)
            self.assertIn("changed during observation", str(caught.exception))

    def test_malformed_packet_is_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            executable.write_text("#!/bin/sh\nprintf 'not-json'\n", encoding="utf-8")
            executable.chmod(0o755)
            with self.assertRaises(VerificationError):
                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 2.0)


if __name__ == "__main__":
    unittest.main()
