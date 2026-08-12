#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
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
DIGEST = "b" * 64


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


def subject(role: str, **packet_args: object) -> dict[str, object]:
    return {
        "filename": "perllsp" if role == "server" else "perl-dap",
        "path_role": "staged_archive",
        "executable_sha256": DIGEST,
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
            "sha": "c" * 40,
            "tree_digest": "d" * 64,
            "version": "0.18.0",
        },
        "public": {
            "repository": "EffortlessMetrics/perl-lsp",
            "sha": PUBLIC_SHA,
            "tree_digest": "e" * 64,
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
        },
        "server": subject("server"),
        "dap": subject("dap"),
        "extension": {
            "id": "EffortlessMetrics.perl-lsp-rs",
            "version": "0.18.0",
            "candidate_identity": "rc1",
            "package_sha256": "f" * 64,
        },
        "topology": {
            "digest": "1" * 64,
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


if __name__ == "__main__":
    unittest.main()
