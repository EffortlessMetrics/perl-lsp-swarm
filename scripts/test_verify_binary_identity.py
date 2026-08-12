#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
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

    def test_malformed_packet_is_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            executable.write_text("#!/bin/sh\nprintf 'not-json'\n", encoding="utf-8")
            executable.chmod(0o755)
            with self.assertRaises(VerificationError):
                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 2.0)


if __name__ == "__main__":
    unittest.main()
