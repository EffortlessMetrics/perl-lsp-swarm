#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import importlib.util
import os
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("publication_runtime_identity.py")
SPEC = importlib.util.spec_from_file_location("publication_runtime_identity", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
runtime_identity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runtime_identity
SPEC.loader.exec_module(runtime_identity)

RuntimeIdentityError = runtime_identity.RuntimeIdentityError
compose = runtime_identity.compose

PUBLIC_SHA = "a" * 40
SERVER_DIGEST = "b" * 64
DAP_DIGEST = "c" * 64


def packet(
    role: str,
    *,
    source: str = PUBLIC_SHA,
    target: str = "x86_64-unknown-linux-gnu",
    candidate: str | None = "rc1",
) -> dict[str, object]:
    executable = "perllsp" if role == "server" else "perl-dap"
    artifact: dict[str, object] = {"role": "archive"}
    if candidate is not None:
        artifact["candidate_identity"] = candidate
    return {
        "schema_version": "perl_lsp.binary_identity.v1",
        "product": {"name": "perl-lsp"},
        "binary": {
            "executable": executable,
            "cargo_package": executable,
            "role": role,
            "version": "0.18.0",
        },
        "build": {
            "source_revision": source,
            "target": target,
            "identity_state": "exact",
        },
        "artifact": artifact,
    }


def subject(role: str, *, digest: str | None = None, **packet_args: object) -> dict[str, object]:
    return {
        "filename": "perllsp" if role == "server" else "perl-dap",
        "path_role": "staged_archive",
        "executable_sha256": digest or (SERVER_DIGEST if role == "server" else DAP_DIGEST),
        "packet": packet(role, **packet_args),
    }


def observation() -> dict[str, object]:
    invariant_ids = [
        "targets_requested_are_built",
        "archive_members_match_consumers",
        "server_dap_pairing",
        "extension_claims_match_vsix",
        "public_install_docs_are_executable",
        "support_posture_matches_claims",
        "artifact_traceable_to_public_sha",
        "product_path_coverage_complete",
        "release_repo_unique_dispositions_complete",
    ]
    return {
        "schema_version": 1,
        "swarm": {
            "repository": "EffortlessMetrics/perl-lsp-swarm",
            "sha": "d" * 40,
            "tree_digest": "e" * 64,
            "version": "0.18.0",
        },
        "public": {
            "repository": "EffortlessMetrics/perl-lsp",
            "sha": PUBLIC_SHA,
            "tree_digest": "f" * 64,
            "version": "0.18.0",
        },
        "manifest": None,
        "differences": [],
        "invariants": [
            {"id": identity, "status": "not_proven", "owner": "fixture", "evidence": ["fixture"]}
            for identity in invariant_ids
        ],
    }


def bundle() -> dict[str, object]:
    return {
        "schema_version": "perl_lsp.publication_runtime_identity.v1",
        "expected": {
            "tree_sha": PUBLIC_SHA,
            "version": "0.18.0",
            "target": "x86_64-unknown-linux-gnu",
            "candidate_identity": "rc1",
            "server_sha256": SERVER_DIGEST,
            "dap_sha256": DAP_DIGEST,
        },
        "server": subject("server"),
        "dap": subject("dap"),
        "extension": {
            "id": "EffortlessMetrics.perl-lsp-rs",
            "version": "0.18.0",
            "candidate_identity": "rc1",
            "package_sha256": "1" * 64,
        },
        "topology": {
            "digest": "2" * 64,
            "selected_target": "x86_64-unknown-linux-gnu",
        },
    }


class PublicationRuntimeIdentityTests(unittest.TestCase):
    def test_exact_bundle_promotes_runtime_invariants_to_pass(self) -> None:
        result = compose(observation(), bundle())
        self.assertEqual(result["differences"], [])
        statuses = {item["id"]: item["status"] for item in result["invariants"]}
        self.assertEqual(statuses["server_dap_pairing"], "pass")
        self.assertEqual(statuses["extension_claims_match_vsix"], "pass")
        self.assertEqual(statuses["artifact_traceable_to_public_sha"], "pass")

    def test_wrong_server_bytes_with_same_packet_are_product_drift(self) -> None:
        value = bundle()
        value["server"] = subject("server", digest="9" * 64)
        result = compose(observation(), value)
        rows = {item["path"]: item for item in result["differences"]}
        self.assertEqual(rows["runtime/server_identity"]["classification"], "product_drift")
        self.assertIn("executable_digest_mismatch", rows["runtime/server_identity"]["evidence"])
        statuses = {item["id"]: item["status"] for item in result["invariants"]}
        self.assertEqual(statuses["artifact_traceable_to_public_sha"], "fail")

    def test_missing_expected_dap_digest_is_not_proven(self) -> None:
        value = bundle()
        value["expected"]["dap_sha256"] = None
        result = compose(observation(), value)
        rows = {item["path"]: item for item in result["differences"]}
        self.assertEqual(
            rows["runtime/dap_identity_evidence"]["classification"],
            "unknown_or_not_proven",
        )
        self.assertIn(
            "expected_artifact_digest_not_proven",
            rows["runtime/dap_identity_evidence"]["evidence"],
        )

    def test_same_version_different_source_is_product_drift(self) -> None:
        value = bundle()
        value["server"] = subject("server", source="9" * 40)
        result = compose(observation(), value)
        rows = {item["path"]: item for item in result["differences"]}
        self.assertEqual(rows["runtime/server_identity"]["classification"], "product_drift")
        self.assertIn("source_revision_mismatch", rows["runtime/server_identity"]["evidence"])

    def test_mixed_server_dap_candidate_is_product_drift(self) -> None:
        value = bundle()
        value["dap"] = subject("dap", candidate="rc2")
        result = compose(observation(), value)
        rows = {item["path"]: item for item in result["differences"]}
        self.assertEqual(rows["runtime/server_dap_pair"]["classification"], "product_drift")
        self.assertIn("server_dap_candidate_mismatch", rows["runtime/server_dap_pair"]["evidence"])

    def test_missing_candidate_and_extension_digest_are_not_proven(self) -> None:
        value = bundle()
        value["server"] = subject("server", candidate=None)
        value["extension"]["package_sha256"] = None
        result = compose(observation(), value)
        classifications = {
            item["path"]: item["classification"] for item in result["differences"]
        }
        self.assertEqual(
            classifications["runtime/server_identity_evidence"], "unknown_or_not_proven"
        )
        self.assertEqual(
            classifications["runtime/extension_identity_evidence"], "unknown_or_not_proven"
        )

    def test_bundle_for_another_public_sha_is_rejected(self) -> None:
        value = bundle()
        value["expected"]["tree_sha"] = "8" * 40
        with self.assertRaises(RuntimeIdentityError):
            compose(observation(), value)

    def test_duplicate_runtime_invariant_is_rejected(self) -> None:
        value = observation()
        value["invariants"].append(
            {"id": "server_dap_pairing", "status": "pass", "owner": "other", "evidence": ["x"]}
        )
        with self.assertRaises(RuntimeIdentityError):
            compose(value, bundle())


class RuntimeIdentityVersionBindingTests(unittest.TestCase):
    def test_bundle_for_another_published_version_is_rejected(self) -> None:
        value = observation()
        value["public"]["version"] = "9.9.9"
        with self.assertRaises(RuntimeIdentityError):
            compose(value, bundle())


def _write_executable(path: Path, body: str) -> Path:
    path.write_text("#!/usr/bin/env python3\n" + textwrap.dedent(body), encoding="utf-8")
    path.chmod(0o755)
    return path


class RuntimeIdentityQueryTests(unittest.TestCase):
    """Cover the one process-executing boundary, `_query_packet`."""

    def test_relative_executable_is_hashed_and_executed_as_one_file(self) -> None:
        emit = 'import json, sys\nsys.stdout.write(json.dumps({{"marker": "{0}"}}))\n'
        with tempfile.TemporaryDirectory() as root:
            staged = Path(root) / "staged"
            decoy = Path(root) / "decoy"
            staged.mkdir()
            decoy.mkdir()
            staged_binary = _write_executable(staged / "perllsp", emit.format("staged"))
            _write_executable(decoy / "perllsp", emit.format("decoy"))
            staged_digest = hashlib.sha256(staged_binary.read_bytes()).hexdigest()

            previous_path = os.environ.get("PATH", "")
            previous_cwd = Path.cwd()
            os.environ["PATH"] = f"{decoy}{os.pathsep}{previous_path}"
            os.chdir(staged)
            try:
                digest, packet_value = runtime_identity._query_packet(Path("./perllsp"), 30.0)
            finally:
                os.chdir(previous_cwd)
                os.environ["PATH"] = previous_path

        # A bare relative path stringifies to `perllsp`; without absolute resolution
        # subprocess finds the decoy on PATH while the digest is read from the cwd.
        self.assertEqual(
            packet_value["marker"], "staged", "executed a different file than it hashed"
        )
        self.assertEqual(digest, staged_digest, "digest does not belong to the executed file")

    def test_unbounded_writer_is_rejected_without_buffering_it_all(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            target = _write_executable(
                Path(root) / "perllsp",
                """
                import sys
                while True:
                    sys.stdout.write("x" * 65536)
                    sys.stdout.flush()
                """,
            )
            with self.assertRaises(RuntimeIdentityError) as caught:
                runtime_identity._query_packet(target, 30.0)
        self.assertIn("too large", str(caught.exception))

    def test_drain_stops_reading_shortly_after_the_limit(self) -> None:
        """The bound must hold while output is produced, not after the child exits.

        A post-hoc size check cannot bound memory against a writer that never exits,
        so assert the reader itself stops near the limit rather than at EOF.
        """
        limit = 4096
        with tempfile.TemporaryDirectory() as root:
            target = _write_executable(
                Path(root) / "noisy",
                """
                import sys
                while True:
                    sys.stdout.write("x" * 65536)
                    sys.stdout.flush()
                """,
            )
            with subprocess.Popen(
                [str(target)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            ) as process:
                try:
                    stdout, _, overflow = runtime_identity._drain_bounded(
                        process, limit, 30.0, time.monotonic() + 30.0
                    )
                finally:
                    process.kill()
        self.assertTrue(overflow, "a never-ending writer must trip the bound")
        self.assertLess(
            len(stdout), limit + 1024 * 1024, "reader buffered far past the limit"
        )


if __name__ == "__main__":
    unittest.main()
