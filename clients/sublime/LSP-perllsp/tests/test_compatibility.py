from __future__ import annotations

import copy
import hashlib
import importlib.util
import unittest
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("perllsp_compatibility", PACKAGE / "compatibility.py")
assert SPEC and SPEC.loader
compatibility = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compatibility)


class CompatibilityAuthorityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = compatibility.load_record()

    def test_checked_record_is_honestly_not_proven(self) -> None:
        validated = compatibility.validate_record(self.record)
        self.assertEqual(validated["compatibility"], "not_proven")
        self.assertEqual(validated["currentness"], "not_proven")
        self.assertIsNone(validated["package"]["version"])
        self.assertIsNone(validated["package"]["tree_sha256"])

    def test_public_ready_rejects_not_proven(self) -> None:
        with self.assertRaisesRegex(compatibility.CompatibilityError, "exact compatible pair"):
            compatibility.assert_managed_install_allowed(
                self.record,
                require_public_ready=True,
            )

    def test_unreviewed_latest_is_forbidden(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["managed_policy"]["allow_unreviewed_latest"] = True
        with self.assertRaisesRegex(compatibility.CompatibilityError, "unreviewed latest"):
            compatibility.validate_record(candidate)

    def test_numeric_version_equality_does_not_create_compatibility(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["package"]["version"] = candidate["server"]["version"]
        candidate["package"]["tree_sha256"] = "a" * 64
        candidate["compatibility"] = "compatible"
        candidate["currentness"] = "current"
        with self.assertRaisesRegex(compatibility.CompatibilityError, "exact actual-host"):
            compatibility.validate_record(candidate)

    def test_incompatible_pair_fails_even_for_development(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["package"]["version"] = "0.1.0"
        candidate["package"]["tree_sha256"] = "a" * 64
        candidate["compatibility"] = "incompatible"
        candidate["currentness"] = "stale_unsupported"
        candidate["evidence"].append(
            {
                "kind": "actual_host_failure",
                "reference": "#fixture",
                "actual_host": True,
                "exact_pair": True,
                "receipt_sha256": "b" * 64,
                "result": "failed",
            }
        )
        with self.assertRaisesRegex(compatibility.CompatibilityError, "explicitly incompatible"):
            compatibility.assert_managed_install_allowed(candidate)

    def test_compatible_requires_an_exact_receipt_digest(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["package"]["version"] = "0.1.0"
        candidate["package"]["tree_sha256"] = "a" * 64
        candidate["compatibility"] = "compatible"
        candidate["currentness"] = "current"
        candidate["evidence"].append(
            {
                "kind": "actual_host",
                "reference": "#fixture",
                "actual_host": True,
                "exact_pair": True,
                "receipt_sha256": None,
            }
        )
        with self.assertRaisesRegex(compatibility.CompatibilityError, "exact actual-host"):
            compatibility.validate_record(candidate)

    def test_server_manifest_digest_mismatch_fails(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["server"]["manifest_sha256"] = "0" * 64
        with self.assertRaisesRegex(compatibility.CompatibilityError, "manifest digest"):
            compatibility.validate_record(candidate)

    def test_exact_compatible_pair_can_pass_public_gate(self) -> None:
        candidate = copy.deepcopy(self.record)
        candidate["package"]["version"] = "0.1.0"
        candidate["package"]["tree_sha256"] = "a" * 64
        candidate["compatibility"] = "compatible"
        candidate["currentness"] = "current"
        candidate["evidence"].append(
            {
                "kind": "actual_host",
                "reference": "#fixture",
                "actual_host": True,
                "exact_pair": True,
                "receipt_sha256": "b" * 64,
                "result": "passed",
            }
        )
        self.assertIs(
            compatibility.assert_managed_install_allowed(
                candidate,
                require_public_ready=True,
            ),
            candidate,
        )

    def test_manifest_hash_is_bound_to_exact_bytes(self) -> None:
        expected = hashlib.sha256(
            (PACKAGE / "server-manifest.json").read_bytes()
        ).hexdigest()
        self.assertEqual(self.record["server"]["manifest_sha256"], expected)


if __name__ == "__main__":
    unittest.main()
