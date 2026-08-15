"""Offline negative and binding tests for the zed_assets receipt package."""

from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from typing import Any, Callable
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from zed_assets import cli, common, framing, producer, validation  # noqa: E402
from zed_assets.archive import extract_expected  # noqa: E402
from zed_assets.common import ReceiptError, sha256_bytes  # noqa: E402
from zed_assets.contract import validate_contract  # noqa: E402
from zed_assets.process import run_stdio_smoke  # noqa: E402

REPO = SCRIPTS.parent
CONTRACT_PATH = REPO / ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json"
TEMPLATE_PATH = REPO / ".ci/fixtures/zed-perl-upstream/receipts/managed-asset-template.json"
SCHEMA_PATH = REPO / ".ci/schemas/zed-managed-asset-receipt.v1.schema.json"


def synthetic_contract(asset_size: int, asset_digest: str) -> dict:
    """A minimal valid contract: one managed zip target plus the required
    explicitly-unsupported Windows ARM64 row. The managed row uses an os no
    verifier matches, so execution tests stay platform independent."""
    return {
        "schema_version": "zed_perllsp_managed_downloads.v1",
        "source": {
            "repository": "EffortlessMetrics/perl-lsp",
            "release_id": 1,
            "tag": "v9.9.9",
            "version": "9.9.9",
            "prerelease": False,
        },
        "identity": {
            "server_id": "perllsp",
            "executable": "perllsp",
            "arguments": ["--stdio"],
        },
        "targets": [
            {
                "os": "plan9",
                "architecture": "x86_64",
                "target": "x86_64-unknown-linux-musl",
                "disposition": "managed",
                "archive_type": "zip",
                "asset_name": "perllsp-9.9.9-x86_64-unknown-linux-musl.zip",
                "asset_id": 11,
                "asset_size": asset_size,
                "asset_digest": asset_digest,
                "archive_member": "perllsp.exe",
                "installed_path": "perllsp-9.9.9-x86_64-unknown-linux-musl/perllsp.exe",
                "make_executable": False,
                "host_execution": "not_proven",
            },
            {
                "os": "windows",
                "architecture": "aarch64",
                "target": "aarch64-pc-windows-msvc",
                "disposition": "unsupported",
                "reason": "no native Windows ARM64 asset",
            },
        ],
        "claim_boundary": {
            "public_asset_metadata": "captured",
            "archive_extraction": "not_proven",
            "perllsp_version_execution": "not_proven",
            "stdio_initialize_shutdown": "not_proven",
            "actual_zed_host": "not_proven",
        },
    }


def fake_release(contract: dict) -> dict:
    return {
        "id": contract["source"]["release_id"],
        "tag_name": contract["source"]["tag"],
        "draft": False,
        "prerelease": False,
        "assets": [
            {
                "id": row["asset_id"],
                "name": row["asset_name"],
                "size": row["asset_size"],
                "digest": None,
                "url": "https://example.invalid/asset",
                "browser_download_url": "https://example.invalid/asset",
            }
            for row in contract["targets"]
            if row["disposition"] == "managed"
        ],
    }


def passing_receipt(contract: dict) -> dict:
    digest = "sha256:" + "1" * 64
    receipt: dict[str, Any] = {
        "schema_version": "zed_managed_asset_receipt.v1",
        "result": "pass",
        "observed_at": "2026-08-14T00:00:00Z",
        "contract": {
            "relative_path": "contract.json",
            "sha256": None,
            "schema_version": "zed_perllsp_managed_downloads.v1",
        },
        "release": {
            "repository": "EffortlessMetrics/perl-lsp",
            "id": 1,
            "tag": "v9.9.9",
            "version": "9.9.9",
            "prerelease": False,
            "draft": False,
            "published_at": "2026-01-01T00:00:00Z",
        },
        "verifier": {"os": "plan9", "version": "1", "architecture": "x86_64", "python": "3.11"},
        "targets": [],
        "limitations": [],
        "claim_boundary": {
            "asset_bytes": "proven",
            "archive_layout": "proven",
            "host_process": "not_executed_on_this_verifier",
            "actual_zed": "not_proven",
            "public_registry": "not_proven",
        },
    }
    for row in contract["targets"]:
        target_row: dict[str, Any] = {
            "target": row["target"],
            "os": row["os"],
            "architecture": row["architecture"],
            "disposition": row["disposition"],
            "result": row["disposition"],
            "asset": None,
            "archive": None,
            "binary": None,
            "stdio_smoke": None,
            "errors": [],
        }
        if row["disposition"] == "managed":
            target_row["result"] = "managed_extracted_not_executed"
            target_row["asset"] = {
                "id": row["asset_id"],
                "name": row["asset_name"],
                "url": "https://example.invalid/asset",
                "size": row["asset_size"],
                "sha256": digest,
                "archive_type": row["archive_type"],
            }
            target_row["archive"] = {
                "members_sha256": digest,
                "required_member": row["archive_member"],
                "installed_name": "perllsp.exe",
                "safe": True,
            }
            target_row["binary"] = {"name": "perllsp.exe", "sha256": digest, "executable": False}
        receipt["targets"].append(target_row)
    return receipt


class LoadJsonTests(unittest.TestCase):
    def test_malformed_json_raises_receipt_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text("{not json", encoding="utf-8")
            with self.assertRaises(ReceiptError) as ctx:
                common.load_json(path)
            self.assertIn("not valid JSON", str(ctx.exception))

    def test_missing_path_raises_receipt_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "absent.json"
            with self.assertRaises(ReceiptError) as ctx:
                common.load_json(path)
            self.assertIn("cannot read", str(ctx.exception))

    def test_non_object_json_raises_receipt_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "array.json"
            path.write_text("[1, 2]", encoding="utf-8")
            with self.assertRaises(ReceiptError):
                common.load_json(path)


class CliInputErrorTests(unittest.TestCase):
    def _run_cli(self, argv: list[str]) -> tuple[int, str]:
        stderr = io.StringIO()
        with patch.object(sys, "argv", ["zed_public_asset_receipts.py", *argv]):
            with contextlib.redirect_stderr(stderr):
                code = cli.main()
        return code, stderr.getvalue()

    def test_malformed_contract_json_is_a_clean_cli_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text("{not json", encoding="utf-8")
            code, stderr = self._run_cli(["validate-contract", "--contract", str(path)])
            self.assertEqual(code, 1)
            self.assertTrue(stderr.startswith("error:"), stderr)
            self.assertNotIn("Traceback", stderr)

    def test_missing_receipt_path_is_a_clean_cli_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "absent.json"
            code, stderr = self._run_cli(
                ["validate-receipt", "--receipt", str(path), "--contract", str(CONTRACT_PATH)]
            )
            self.assertEqual(code, 1)
            self.assertTrue(stderr.startswith("error:"), stderr)
            self.assertNotIn("Traceback", stderr)

    def test_validate_receipt_requires_contract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "receipt.json"
            receipt.write_text("{}", encoding="utf-8")
            stderr = io.StringIO()
            with self.assertRaises(SystemExit) as ctx:
                with contextlib.redirect_stderr(stderr):
                    cli.build_parser().parse_args(["validate-receipt", "--receipt", str(receipt)])
            self.assertEqual(ctx.exception.code, 2)


class ValidationBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))
        self.template = json.loads(TEMPLATE_PATH.read_text(encoding="utf-8"))

    def test_not_run_template_validates_against_checked_contract(self) -> None:
        validation.validate_receipt(self.template, CONTRACT_PATH, self.contract)

    def test_wrong_contract_digest_is_rejected(self) -> None:
        receipt = json.loads(TEMPLATE_PATH.read_text(encoding="utf-8"))
        receipt["contract"]["sha256"] = "sha256:" + "0" * 64
        with self.assertRaises(ReceiptError) as ctx:
            validation.validate_receipt(receipt, CONTRACT_PATH, self.contract)
        self.assertIn("does not match the checked contract", str(ctx.exception))

    def _bound_receipt(self) -> tuple[Path, dict, dict]:
        """Write a synthetic contract to disk and return it with a receipt
        whose contract digest matches that exact file."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        tmp_path = Path(temporary.name)
        contract_file = tmp_path / "contract.json"
        contract = synthetic_contract(4, sha256_bytes(b"abcd"))
        contract_file.write_text(json.dumps(contract), encoding="utf-8")
        receipt = passing_receipt(contract)
        receipt["contract"]["sha256"] = common.sha256_file(contract_file)
        return contract_file, contract, receipt

    def test_passing_receipt_bound_to_checked_contract(self) -> None:
        contract_file, contract, receipt = self._bound_receipt()
        validation.validate_receipt(receipt, contract_file, contract)

    def test_empty_evidence_passing_receipt_is_rejected(self) -> None:
        contract_file, contract, receipt = self._bound_receipt()
        for row in receipt["targets"]:
            if row["disposition"] == "managed":
                row["disposition"] = "unsupported"
                row["result"] = "unsupported"
                row["asset"] = None
                row["archive"] = None
                row["binary"] = None
        with self.assertRaises(ReceiptError) as ctx:
            validation.validate_receipt(receipt, contract_file, contract)
        self.assertIn("no managed target evidence", str(ctx.exception))

    def test_passing_receipt_with_empty_targets_is_rejected(self) -> None:
        contract_file, contract, receipt = self._bound_receipt()
        receipt["targets"] = []
        with self.assertRaises(ReceiptError) as ctx:
            validation.validate_receipt(receipt, contract_file, contract)
        self.assertIn("must contain target rows", str(ctx.exception))

    def test_target_set_mismatch_is_rejected(self) -> None:
        contract_file, contract, receipt = self._bound_receipt()
        receipt["targets"] = [row for row in receipt["targets"] if row["disposition"] == "managed"]
        with self.assertRaises(ReceiptError) as ctx:
            validation.validate_receipt(receipt, contract_file, contract)
        self.assertIn("do not match the checked contract target set", str(ctx.exception))

    def test_digest_drift_on_bound_receipt_is_rejected(self) -> None:
        contract_file, contract, receipt = self._bound_receipt()
        # Rewrite the contract file so its recomputed digest no longer matches
        # the digest the receipt recorded.
        contract_file.write_text(json.dumps(contract, indent=2), encoding="utf-8")
        with self.assertRaises(ReceiptError) as ctx:
            validation.validate_receipt(receipt, contract_file, contract)
        self.assertIn("does not match the checked contract", str(ctx.exception))


class VersionBindingTests(unittest.TestCase):
    def _check_version_output(self, output: str, expected_version: str) -> None:
        completed = subprocess.CompletedProcess([], 0, stdout=output, stderr="")
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(subprocess, "run", return_value=completed):
                with self.assertRaises(ReceiptError) as ctx:
                    run_stdio_smoke(Path("perllsp"), Path(tmp), expected_version)
        return ctx.exception

    def test_stale_binary_version_is_rejected(self) -> None:
        error = self._check_version_output("perllsp 0.16.0\n", "0.17.0")
        self.assertIn("does not report the expected", str(error))
        self.assertIn("0.17.0", str(error))
        self.assertIn("0.16.0", str(error))

    def test_non_perllsp_output_is_rejected(self) -> None:
        error = self._check_version_output("some-other-tool 0.17.0\n", "0.17.0")
        self.assertIn("does not identify perllsp", str(error))


class ArchiveTests(unittest.TestCase):
    def test_malformed_zip_becomes_receipt_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "asset.zip"
            path.write_bytes(b"this is not a zip archive")
            with self.assertRaises(ReceiptError) as ctx:
                extract_expected(path, "zip", "perllsp.exe", Path(tmp) / "out", False)
            self.assertIn("malformed zip archive", str(ctx.exception))

    def test_malformed_tar_becomes_receipt_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "asset.tar.gz"
            path.write_bytes(b"this is not a gzip stream")
            with self.assertRaises(ReceiptError) as ctx:
                extract_expected(path, "tar.gz", "perllsp-9.9.9-x/perllsp", Path(tmp) / "out", False)
            self.assertIn("malformed tar.gz archive", str(ctx.exception))

    def test_noncanonical_zip_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "asset.zip"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("./perllsp.exe", b"MZ fake binary")
            with self.assertRaises(ReceiptError) as ctx:
                extract_expected(path, "zip", "perllsp.exe", Path(tmp) / "out", False)
            self.assertIn("noncanonical archive name", str(ctx.exception))

    def test_noncanonical_tar_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "asset.tar.gz"
            with tarfile.open(path, "w:gz") as archive:
                data = b"#!/bin/sh\n"
                info = tarfile.TarInfo("./perllsp-9.9.9-x/perllsp")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
            with self.assertRaises(ReceiptError) as ctx:
                extract_expected(path, "tar.gz", "perllsp-9.9.9-x/perllsp", Path(tmp) / "out", False)
            self.assertIn("noncanonical archive name", str(ctx.exception))

    def test_canonical_zip_member_extracts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "asset.zip"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("perllsp.exe", b"MZ fake binary")
            output, members_digest = extract_expected(
                path, "zip", "perllsp.exe", Path(tmp) / "out", False
            )
            self.assertEqual(output.read_bytes(), b"MZ fake binary")
            self.assertTrue(members_digest.startswith("sha256:"))


class ProducerFailReceiptTests(unittest.TestCase):
    def _run_producer(
        self, contract: dict, payload: bytes, post_run: Callable[[dict, Path], None] | None = None
    ) -> int:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            contract_file = tmp_path / "contract.json"
            contract_file.write_text(json.dumps(contract), encoding="utf-8")
            output = tmp_path / "receipt.json"

            def fake_download(url: str, destination: Path, token: str | None) -> None:
                destination.write_bytes(payload)

            with (
                patch.object(producer, "fetch_json", return_value=fake_release(contract)),
                patch.object(producer, "download_asset", side_effect=fake_download),
                patch.object(producer, "run_stdio_smoke") as smoke,
            ):
                exit_code = producer.execute(contract_file, contract, output, tmp_path / "work", None)
            self.assertFalse(smoke.called, "the non-matching host row must never execute")
            receipt = json.loads(output.read_text(encoding="utf-8"))
            if post_run is not None:
                post_run(receipt, contract_file)
        return exit_code

    def test_malformed_archive_becomes_per_target_fail_receipt(self) -> None:
        payload = b"this is not a zip archive"
        contract = synthetic_contract(len(payload), sha256_bytes(payload))

        def check(receipt: dict, contract_file: Path) -> None:
            self.assertEqual(receipt["result"], "fail")
            managed_rows = [row for row in receipt["targets"] if row["disposition"] == "managed"]
            self.assertEqual(len(managed_rows), 1)
            self.assertEqual(managed_rows[0]["result"], "fail")
            self.assertTrue(
                any("malformed zip archive" in error for error in managed_rows[0]["errors"]),
                managed_rows[0]["errors"],
            )

        exit_code = self._run_producer(contract, payload, check)
        self.assertEqual(exit_code, 1)

    def test_valid_archive_produces_passing_receipt(self) -> None:
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("perllsp.exe", b"MZ fake binary")
        payload = buffer.getvalue()
        contract = synthetic_contract(len(payload), sha256_bytes(payload))

        def check(receipt: dict, contract_file: Path) -> None:
            self.assertEqual(receipt["result"], "pass")
            self.assertEqual(receipt["claim_boundary"]["asset_bytes"], "proven")
            # The producer's own output must validate against the exact contract file.
            validation.validate_receipt(receipt, contract_file, contract)

        exit_code = self._run_producer(contract, payload, check)
        self.assertEqual(exit_code, 0)

    def test_download_digest_drift_becomes_fail_receipt(self) -> None:
        payload = b"tampered bytes"
        contract = synthetic_contract(len(payload), "sha256:" + "7" * 64)

        def check(receipt: dict, contract_file: Path) -> None:
            self.assertEqual(receipt["result"], "fail")
            managed_rows = [row for row in receipt["targets"] if row["disposition"] == "managed"]
            self.assertEqual(managed_rows[0]["result"], "fail")
            self.assertTrue(
                any("digest mismatch" in error for error in managed_rows[0]["errors"]),
                managed_rows[0]["errors"],
            )

        exit_code = self._run_producer(contract, payload, check)
        self.assertEqual(exit_code, 1)


class ContractPathConstraintTests(unittest.TestCase):
    def _assert_rejected(self, mutate: Callable[[dict], None]) -> None:
        contract = synthetic_contract(4, sha256_bytes(b"abcd"))
        mutate(contract)
        with self.assertRaises(ReceiptError) as ctx:
            validate_contract(contract)
        self.assertIn("single relative path component", str(ctx.exception))

    def test_traversal_target_rejected(self) -> None:
        self._assert_rejected(lambda contract: contract["targets"][0].update(target="../outside"))

    def test_absolute_target_rejected(self) -> None:
        self._assert_rejected(lambda contract: contract["targets"][0].update(target="/tmp/outside"))

    def test_multi_component_target_rejected(self) -> None:
        self._assert_rejected(lambda contract: contract["targets"][0].update(target="a/b"))

    def test_backslash_target_rejected(self) -> None:
        self._assert_rejected(
            lambda contract: contract["targets"][0].update(target="a" + chr(92) + "b")
        )

    def test_dot_and_dotdot_targets_rejected(self) -> None:
        for value in (".", ".."):
            self._assert_rejected(
                lambda contract, value=value: contract["targets"][0].update(target=value)
            )

    def test_separator_asset_name_rejected(self) -> None:
        def mutate(contract: dict) -> None:
            contract["targets"][0]["asset_name"] = "perllsp-9.9.9-../../evil.zip"

        self._assert_rejected(mutate)

    def test_separator_version_rejected(self) -> None:
        self._assert_rejected(
            lambda contract: contract["source"].update(version="9.9.9/../../x")
        )


class FramingTests(unittest.TestCase):
    def test_round_trip(self) -> None:
        message = {"jsonrpc": "2.0", "id": 1, "result": {"ok": True}}
        frames = framing.parse_lsp_frames(framing.lsp_frame(message))
        self.assertEqual(frames, [message])

    def test_non_ascii_header_rejected(self) -> None:
        with self.assertRaises(ReceiptError) as ctx:
            framing.parse_lsp_frames(b"Content-Length\xff: 2\r\n\r\n{}")
        self.assertIn("not strict ASCII", str(ctx.exception))

    def test_non_integer_content_length_rejected(self) -> None:
        with self.assertRaises(ReceiptError) as ctx:
            framing.parse_lsp_frames(b"Content-Length: abc\r\n\r\n{}")
        self.assertIn("Content-Length is not an integer", str(ctx.exception))

    def test_malformed_json_body_rejected(self) -> None:
        with self.assertRaises(ReceiptError) as ctx:
            framing.parse_lsp_frames(b"Content-Length: 2\r\n\r\n{!")
        self.assertIn("not valid JSON", str(ctx.exception))


class SchemaBindingTests(unittest.TestCase):
    def test_schema_binds_null_contract_digest_to_not_run(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        rules = schema.get("allOf")
        self.assertTrue(rules, "receipt schema must bind contract.sha256 nullability to result")
        combined = json.dumps(rules)
        self.assertIn("not_run", combined)
        self.assertIn("sha256", combined)
        self.assertIn("pattern", combined)


class WorkDirLifecycleTests(unittest.TestCase):
    def _execute(self, argv: list[str]) -> tuple[int, Path | None]:
        captured: dict[str, Path] = {}

        def fake_execute(contract_path, contract, output, work_dir, token):
            captured["work_dir"] = work_dir
            return 0

        with (
            patch.object(sys, "argv", ["zed_public_asset_receipts.py", *argv]),
            patch.object(cli, "execute", side_effect=fake_execute),
        ):
            code = cli.main()
        return code, captured.get("work_dir")

    def test_implicit_work_dir_is_removed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            contract_file = tmp_path / "contract.json"
            contract_file.write_text("{}", encoding="utf-8")
            code, work_dir = self._execute(
                ["execute", "--contract", str(contract_file), "--output", str(tmp_path / "r.json")]
            )
            self.assertEqual(code, 0)
            self.assertIsNotNone(work_dir)
            self.assertFalse(Path(work_dir).exists())

    def test_explicit_work_dir_is_retained(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            contract_file = tmp_path / "contract.json"
            contract_file.write_text("{}", encoding="utf-8")
            work = tmp_path / "kept"
            code, captured = self._execute(
                [
                    "execute",
                    "--contract",
                    str(contract_file),
                    "--output",
                    str(tmp_path / "r.json"),
                    "--work-dir",
                    str(work),
                ]
            )
            self.assertEqual(code, 0)
            self.assertEqual(captured, work)


if __name__ == "__main__":
    unittest.main()
