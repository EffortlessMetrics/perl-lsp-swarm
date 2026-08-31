#!/usr/bin/env python3
"""Tests for rolling_installed_observation.py."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest

MODULE_PATH = pathlib.Path(__file__).with_name("rolling_installed_observation.py")
SPEC = importlib.util.spec_from_file_location("rolling_installed_observation", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

SHA = "a" * 40
OTHER_SHA = "b" * 40
VSIX_SHA = "c" * 64


def write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def smoke_receipt(
    server: pathlib.Path,
    *,
    repository_sha: str = SHA,
    server_hash: str | None = None,
    platform: str = "linux",
    vscode_version: str = "1.125.0",
) -> dict[str, object]:
    return {
        "schema_version": "vscode_current_source_smoke.v1",
        "receipt_kind": "vscode_current_source_smoke",
        "repository_sha": repository_sha,
        "platform": platform,
        "architecture": "x64",
        "vscode_version": vscode_version,
        "server": {
            "source_sha": repository_sha,
            "path": str(server),
            "sha256": server_hash or MODULE.sha256(server),
        },
        "vsix": {"path": "deleted-after-proof.vsix", "sha256": VSIX_SHA},
        "stages": {
            "package_creation": {"status": "pass"},
            "package_inventory": {"status": "pass", "behavior_safe": True},
            "behavioral_smoke": {"status": "pass"},
            "activation_failure_journey": {"status": "pass"},
            "crash_recovery_journey": {"status": "pass"},
        },
        "instrument_failure": None,
        "cleanup_failure": None,
        "overall": "pass",
    }


class ObservationTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.server = self.root / "perllsp"
        self.dap = self.root / "perl-dap"
        self.server.write_bytes(b"server-bytes")
        self.dap.write_bytes(b"dap-bytes")
        self.archive = self.root / "product-unit.zip"
        self.archive.write_bytes(b"archive-bytes")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def build_row(
        self,
        *,
        receipt: dict[str, object] | None,
        row_id: str = "linux-minimum",
        platform: str = "linux",
        vscode_version: str = "1.125.0",
    ) -> dict[str, object]:
        receipts = self.root / f"receipts-{row_id}"
        if receipt is not None:
            write_json(receipts / "current-source-orchestration.json", receipt)
        output = self.root / f"{row_id}.json"
        result = MODULE.main(
            [
                "row",
                "--source-sha",
                SHA,
                "--source-version",
                "0.17.0",
                "--row-id",
                row_id,
                "--platform",
                platform,
                "--architecture",
                "x64",
                "--host-role",
                "minimum_supported" if row_id == "linux-minimum" else "current_stable",
                "--vscode-version",
                vscode_version,
                "--server",
                str(self.server),
                "--dap",
                str(self.dap),
                "--archive",
                str(self.archive),
                "--receipts-root",
                str(receipts),
                "--smoke-outcome",
                "success" if receipt is not None else "failure",
                "--output",
                str(output),
            ]
        )
        self.assertEqual(result, 0)
        return json.loads(output.read_text(encoding="utf-8"))

    def test_exact_smoke_promotes_only_directly_observed_cells(self) -> None:
        row = self.build_row(receipt=smoke_receipt(self.server))
        self.assertEqual(row["cells"]["artifact_identity"], "pass")
        self.assertEqual(row["cells"]["packaged_provider_edit_journey"], "pass")
        self.assertEqual(row["cells"]["process_cleanup"], "pass")
        self.assertEqual(row["cells"]["host_version_exactness"], "pass")
        self.assertEqual(row["cells"]["native_critic_installed"], "not_proven")
        self.assertEqual(row["cells"]["full_document_utf16_installed"], "not_proven")
        self.assertEqual(row["cells"]["dap_preview_installed"], "not_proven")
        self.assertEqual(row["status"], "not_proven")
        self.assertEqual(row["zero_budget_counts"]["wrong_binary_or_artifact"], 0)

    def test_stable_selector_does_not_claim_concrete_host_version(self) -> None:
        row = self.build_row(
            receipt=smoke_receipt(self.server, vscode_version="stable"),
            row_id="linux-current",
            vscode_version="stable",
        )
        self.assertEqual(row["subject"]["vscode_selector"], "stable")
        self.assertIsNone(row["subject"]["vscode_concrete_version"])
        self.assertEqual(row["cells"]["host_version_exactness"], "not_proven")
        self.assertEqual(row["status"], "not_proven")

    def test_wrong_server_hash_is_instrument_defect_not_pass(self) -> None:
        row = self.build_row(
            receipt=smoke_receipt(self.server, server_hash="d" * 64)
        )
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertEqual(row["zero_budget_counts"]["wrong_binary_or_artifact"], 1)
        self.assertTrue(any("server hash" in finding for finding in row["findings"]))

    def test_missing_receipt_is_explicit_instrument_failure(self) -> None:
        row = self.build_row(receipt=None)
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertEqual(
            row["cells"]["packaged_provider_edit_journey"], "instrument_defect"
        )
        self.assertEqual(row["status"], "not_proven")

    def test_cross_sha_receipt_cannot_satisfy_row(self) -> None:
        row = self.build_row(
            receipt=smoke_receipt(self.server, repository_sha=OTHER_SHA)
        )
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertTrue(any("no exact" in finding for finding in row["findings"]))

    def test_package_output_is_deterministic(self) -> None:
        first = self.root / "first.zip"
        second = self.root / "second.zip"
        for output in (first, second):
            result = MODULE.main(
                [
                    "package",
                    "--source-sha",
                    SHA,
                    "--source-version",
                    "0.17.0",
                    "--platform",
                    "linux",
                    "--architecture",
                    "x64",
                    "--server",
                    str(self.server),
                    "--dap",
                    str(self.dap),
                    "--output",
                    str(output),
                ]
            )
            self.assertEqual(result, 0)
        self.assertEqual(MODULE.sha256(first), MODULE.sha256(second))


class FanInTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.topology = self.root / "release-topology.json"
        self.topology.write_text("{}\n", encoding="utf-8")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def row(
        self, row_id: str, *, source_sha: str = SHA, verdict: str = "pass"
    ) -> None:
        value = {
            "schema_version": MODULE.ROW_SCHEMA,
            "row_id": row_id,
            "subject": {"repository_sha": source_sha},
            "artifacts": {"perllsp": {"sha256": "1" * 64}},
            "mechanism_receipt": {"sha256": "2" * 64},
            "cells": {"observed": verdict},
            "zero_budget_counts": {
                key: 0 for key in MODULE.ZERO_BUDGET_KEYS
            },
            "status": (
                "blocked"
                if verdict == "product_defect"
                else "not_proven"
                if verdict != "pass"
                else "pass"
            ),
        }
        write_json(self.root / "rows" / f"{row_id}.json", value)

    def fan_in(self, require_ready: bool = False) -> tuple[int, dict[str, object]]:
        output = self.root / "packet.json"
        argv = [
            "fan-in",
            "--source-sha",
            SHA,
            "--source-version",
            "0.17.0",
            "--rows-root",
            str(self.root / "rows"),
            "--topology",
            str(self.topology),
            "--output",
            str(output),
        ]
        if require_ready:
            argv.append("--require-ready")
        result = MODULE.main(argv)
        return result, json.loads(output.read_text(encoding="utf-8"))

    def test_missing_platform_row_remains_not_proven(self) -> None:
        self.row("linux-minimum")
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertEqual(
            packet["missing_rows"], ["linux-current", "windows-current"]
        )
        self.assertFalse(packet["freezes_product"])
        self.assertFalse(packet["closes_6056"])

    def test_complete_primary_rows_cannot_hide_unproven_retained_targets(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertEqual(packet["platforms"]["linux"], "pass")
        self.assertEqual(packet["platforms"]["windows"], "pass")
        self.assertEqual(packet["platforms"]["other_retained_targets"], "not_proven")
        self.assertIn(
            "topology:other_retained_targets", packet["not_proven_cells"]
        )

    def test_cross_sha_row_is_rejected_and_cannot_be_ready(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current", source_sha=OTHER_SHA)
        self.row("windows-current")
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertIn("linux-current", packet["missing_rows"])
        self.assertTrue(
            any(
                "another source SHA" in item
                for item in packet["instrument_defects"]
            )
        )


if __name__ == "__main__":
    unittest.main()
