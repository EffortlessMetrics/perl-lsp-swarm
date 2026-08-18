from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

PACKAGE = Path(__file__).resolve().parents[1]
if str(PACKAGE) not in sys.path:
    sys.path.insert(0, str(PACKAGE))
SPEC = importlib.util.spec_from_file_location("perllsp_integration_status", PACKAGE / "integration_status.py")
assert SPEC and SPEC.loader
integration_status = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(integration_status)


class IntegrationStatusTests(unittest.TestCase):
    def _managed_subject(self, storage: Path) -> tuple[dict, dict, Path, Path]:
        return integration_status._managed_paths(storage, "linux", "x64")

    def _write_verified_cache(self, storage: Path, payload: bytes = b"trusted") -> Path:
        _manifest, asset, install_dir, binary = self._managed_subject(storage)
        install_dir.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(payload)
        binary.chmod(0o755)
        binary.with_name("install.json").write_text(
            json.dumps(
                {
                    "archive_sha256": asset["sha256"],
                    "binary_sha256": hashlib.sha256(payload).hexdigest(),
                }
            ),
            encoding="utf-8",
        )
        return binary

    def test_missing_managed_server_is_action_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            payload = integration_status.collect_status(
                Path(directory),
                "linux",
                "x64",
                which=lambda _name: None,
                debugger_registered=False,
            )
        self.assertEqual(payload["structural_state"], "action_required")
        self.assertEqual(payload["semantic_support"], "not_assessed")
        self.assertEqual(payload["server"]["state"], "missing")
        self.assertIn("managed_server_missing", payload["reason_tokens"])
        self.assertIn("compatibility_not_proven", payload["reason_tokens"])
        self.assertFalse(payload["mutated"])

    def test_verified_cache_is_usable_but_not_semantically_promoted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)
            self._write_verified_cache(storage)
            before = sorted(path.relative_to(storage).as_posix() for path in storage.rglob("*"))
            payload = integration_status.collect_status(
                storage,
                "linux",
                "x64",
                which=lambda _name: None,
                debugger_registered=False,
            )
            after = sorted(path.relative_to(storage).as_posix() for path in storage.rglob("*"))
        self.assertEqual(before, after)
        self.assertEqual(payload["server"]["state"], "verified_cache")
        self.assertEqual(payload["structural_state"], "usable_candidate")
        self.assertEqual(payload["compatibility"]["compatibility"], "not_proven")
        self.assertEqual(payload["semantic_support"], "not_assessed")

    def test_tampered_cache_is_reported_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)
            binary = self._write_verified_cache(storage)
            binary.write_bytes(b"tampered")
            metadata = binary.with_name("install.json").read_bytes()
            payload = integration_status.collect_status(
                storage,
                "linux",
                "x64",
                which=lambda _name: None,
            )
            self.assertEqual(binary.read_bytes(), b"tampered")
            self.assertEqual(binary.with_name("install.json").read_bytes(), metadata)
        self.assertEqual(payload["server"]["state"], "invalid_cache")
        self.assertIn("managed_server_invalid_cache", payload["reason_tokens"])

    def test_external_server_is_user_owned_and_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "perllsp"
            binary.write_bytes(b"external")
            payload = integration_status.collect_status(
                Path(directory) / "storage",
                "linux",
                "x64",
                server_path="perllsp",
                which=lambda name: str(binary) if name == "perllsp" else None,
            )
        self.assertEqual(payload["server"]["state"], "resolved")
        self.assertEqual(payload["server"]["mode"], "external_user_managed")
        self.assertEqual(payload["server"]["support_disposition"], "not_proven")
        self.assertIn("external_server_user_owned", payload["reason_tokens"])

    def test_clear_invalid_cache_refuses_verified_subject(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)
            binary = self._write_verified_cache(storage)
            with self.assertRaisesRegex(
                integration_status.IntegrationStatusError,
                "verified",
            ):
                integration_status.clear_invalid_managed_cache(storage, "linux", "x64")
            self.assertTrue(binary.is_file())

    def test_clear_invalid_cache_removes_only_known_install_dir(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)
            binary = self._write_verified_cache(storage)
            binary.write_bytes(b"tampered")
            sentinel = storage / "sentinel"
            sentinel.write_text("keep", encoding="utf-8")
            receipt = integration_status.clear_invalid_managed_cache(storage, "linux", "x64")
            self.assertEqual(receipt["result"], "removed")
            self.assertFalse(binary.parent.exists())
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

    def test_repair_is_noop_for_verified_cache(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)
            binary = self._write_verified_cache(storage)
            opener = mock.Mock(side_effect=AssertionError("network must not be used"))
            receipt = integration_status.repair_managed_server(
                storage,
                "linux",
                "x64",
                opener=opener,
            )
            self.assertFalse(receipt["mutated"])
            self.assertEqual(Path(receipt["binary_path"]), binary)
            opener.assert_not_called()

    def test_repair_requires_post_install_verified_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            storage = Path(directory)

            def fake_install(path: Path, platform: str, arch: str, opener):
                del opener
                _manifest, asset, install_dir, binary = integration_status._managed_paths(
                    path, platform, arch
                )
                install_dir.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"repaired")
                binary.chmod(0o755)
                binary.with_name("install.json").write_text(
                    json.dumps(
                        {
                            "archive_sha256": asset["sha256"],
                            "binary_sha256": hashlib.sha256(b"repaired").hexdigest(),
                        }
                    ),
                    encoding="utf-8",
                )
                return binary

            with mock.patch.object(integration_status, "install_server", fake_install):
                receipt = integration_status.repair_managed_server(
                    storage,
                    "linux",
                    "x64",
                    opener=lambda *_args, **_kwargs: None,
                )
        self.assertTrue(receipt["mutated"])
        self.assertEqual(receipt["result"], "verified")
        self.assertEqual(receipt["compatibility"], "not_proven")

    def test_status_output_is_bounded_and_marks_structural_scope(self) -> None:
        payload = {
            "structural_state": "usable_candidate",
            "platform": "linux",
            "architecture": "x64",
            "compatibility": {
                "compatibility": "not_proven",
                "currentness": "not_proven",
            },
            "server": {"mode": "managed", "state": "verified_cache"},
            "dap": {"mode": "external_user_managed", "state": "unavailable"},
            "reason_tokens": ["semantic_support_not_assessed"],
        }
        rendered = integration_status.format_status(payload)
        self.assertIn("Semantic support: not assessed", rendered)
        self.assertLessEqual(len(rendered), integration_status.MAX_STATUS_CHARS + 128)


if __name__ == "__main__":
    unittest.main()
