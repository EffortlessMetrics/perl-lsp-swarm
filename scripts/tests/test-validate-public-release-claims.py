#!/usr/bin/env python3
"""Focused tests for the candidate-bound public claims validator."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import unittest
import tempfile
import json
from pathlib import Path


ROOT = Path(__file__).parents[2]
SPEC = importlib.util.spec_from_file_location("validate_public_release_claims", ROOT / "scripts/validate_public_release_claims.py")
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def catalog() -> dict:
    return {
        "schema_version": "public_release_claims.v1",
        "release": "0.18.0",
        "track": "public-beta",
        "subject_sha": "0" * 40,
        "topology_digest": "sha256:" + "1" * 64,
        "claims": [
            {
                "id": "install.windows.powershell",
                "surfaces": ["README.md", "install.ps1"],
                "audience": "user",
                "text_or_command": ".\\install.ps1 -Version 0.18.0",
                "authority": "installed_transition",
                "evidence_refs": ["#5903", "receipt:install-transition"],
                "status": "bounded",
                "public_context": "swarm",
                "limitation": "Publication-repository promotion is not yet proven.",
            }
        ],
    }


class PublicReleaseClaimsTests(unittest.TestCase):
    def test_valid_candidate_bound_claim_passes(self) -> None:
        MODULE.validate_claims(catalog())

    def test_catalog_binds_to_exact_topology_bytes(self) -> None:
        value = catalog()
        topology = {
            "schema": 1,
            "release": "0.18.0",
            "track": "public-beta",
            "frozen_product_sha": "0" * 40,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "release-topology.json"
            raw = json.dumps(topology, separators=(",", ":")).encode()
            path.write_bytes(raw)
            value["topology_digest"] = "sha256:" + hashlib.sha256(raw).hexdigest()
            MODULE.validate_topology_binding(value, path)

            path.write_bytes(raw + b"\n")
            with self.assertRaisesRegex(ValueError, "does not match topology bytes"):
                MODULE.validate_topology_binding(value, path)

    def assert_invalid(self, mutation, message: str) -> None:
        value = copy.deepcopy(catalog())
        mutation(value)
        with self.assertRaisesRegex(ValueError, message):
            MODULE.validate_claims(value)

    def test_limitation_field_is_required_even_for_proven_claims(self) -> None:
        def remove_limitation(value: dict) -> None:
            value["claims"][0].pop("limitation")

        self.assert_invalid(remove_limitation, "limitation is required")

    def test_bounded_claim_requires_limitation(self) -> None:
        self.assert_invalid(lambda value: value["claims"][0].update({"limitation": None}), "requires a limitation")

    def test_claims_must_be_sorted_and_unique(self) -> None:
        value = catalog()
        duplicate = copy.deepcopy(value["claims"][0])
        duplicate["id"] = "install.windows.archive"
        value["claims"].extend([duplicate, copy.deepcopy(value["claims"][0])])
        with self.assertRaisesRegex(ValueError, "unique"):
            MODULE.validate_claims(value)

        value["claims"] = [copy.deepcopy(value["claims"][0]), duplicate]
        with self.assertRaisesRegex(ValueError, "sorted by id"):
            MODULE.validate_claims(value)

    def test_proven_claim_cannot_hide_a_blank_limitation(self) -> None:
        self.assert_invalid(lambda value: value["claims"][0].update({"status": "proven", "limitation": "   "}), "limitation")

    def test_not_proven_claim_is_still_explicitly_bounded(self) -> None:
        self.assert_invalid(lambda value: value["claims"][0].update({"status": "not_proven", "limitation": None}), "requires a limitation")

    def test_install_claim_cannot_use_an_unrelated_authority(self) -> None:
        self.assert_invalid(
            lambda value: value["claims"][0].update({"authority": "api_audit"}),
            "authority does not own claim id",
        )

    def test_unmapped_claim_namespace_fails_closed(self) -> None:
        self.assert_invalid(
            lambda value: value["claims"][0].update({"id": "maturity.public_beta"}),
            "unmapped claim namespace",
        )


if __name__ == "__main__":
    unittest.main()
