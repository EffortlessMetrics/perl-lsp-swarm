#!/usr/bin/env python3
"""Tests for rolling_installed_observation.py."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
import zipfile

MODULE_PATH = pathlib.Path(__file__).with_name("rolling_installed_observation.py")
SPEC = importlib.util.spec_from_file_location("rolling_installed_observation", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

SHA = "a" * 40
OTHER_SHA = "b" * 40
VSIX_SHA = "c" * 64
VERSION = "0.17.0"


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
    stages: dict[str, object] | None = None,
    instrument_failure: str | None = None,
) -> dict[str, object]:
    default_stages: dict[str, object] = {
        "package_creation": {"status": "pass"},
        "package_inventory": {"status": "pass", "behavior_safe": True},
        "behavioral_smoke": {"status": "pass"},
        "activation_failure_journey": {"status": "pass"},
        "crash_recovery_journey": {"status": "pass"},
    }
    if stages:
        default_stages.update(stages)
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
        "stages": default_stages,
        "instrument_failure": instrument_failure,
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
        self.package("linux")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def package(self, platform: str = "linux") -> None:
        result = self.run_row(
            [
                "package",
                "--source-sha",
                SHA,
                "--source-version",
                VERSION,
                "--platform",
                platform,
                "--architecture",
                "x64",
                "--server",
                str(self.server),
                "--dap",
                str(self.dap),
                "--output",
                str(self.archive),
            ]
        )
        self.assertEqual(result, 0)

    def run_row(self, argv: list[str]) -> int:
        return MODULE.main(argv)

    def build_row(
        self,
        *,
        receipt: dict[str, object] | None,
        row_id: str = "linux-minimum",
        platform: str = "linux",
        vscode_version: str = "1.125.0",
        smoke_outcome: str | None = None,
    ) -> dict[str, object]:
        receipts = self.root / f"receipts-{row_id}"
        if receipt is not None:
            write_json(receipts / "current-source-orchestration.json", receipt)
        output = self.root / f"{row_id}.json"
        result = self.run_row(
            [
                "row",
                "--source-sha",
                SHA,
                "--source-version",
                VERSION,
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
                smoke_outcome or ("success" if receipt is not None else "failure"),
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

    def test_failed_stage_is_product_defect_and_blocks_row(self) -> None:
        receipt = smoke_receipt(
            self.server,
            stages={
                "behavioral_smoke": {
                    "status": "failed",
                    "reason": "published_extension_smoke_failed",
                }
            },
        )
        receipt["overall"] = "failed"
        row = self.build_row(receipt=receipt, smoke_outcome="failure")
        self.assertEqual(
            row["cells"]["packaged_provider_edit_journey"], "product_defect"
        )
        self.assertEqual(row["status"], "blocked")
        self.assertEqual(row["zero_budget_counts"]["silent_product_failure"], 1)

    def test_host_resolution_failure_is_not_a_product_defect(self) -> None:
        for reason, expected_cell in (
            ("vscode_host_resolution_network", "instrument_defect"),
            ("vscode_host_resolution_cache", "instrument_defect"),
            ("vscode_host_resolution_runner", "instrument_defect"),
            ("vscode_host_resolution_unavailable", "not_proven"),
        ):
            with self.subTest(reason=reason):
                status = "not_proven" if reason.endswith("unavailable") else "failed"
                row = self.build_row(
                    receipt=smoke_receipt(
                        self.server,
                        stages={
                            "behavioral_smoke": {"status": status, "reason": reason}
                        },
                    ),
                )
                self.assertEqual(
                    row["cells"]["packaged_provider_edit_journey"], expected_cell
                )
                self.assertNotEqual(row["status"], "blocked")

    def test_instrument_failure_field_is_recorded(self) -> None:
        row = self.build_row(
            receipt=smoke_receipt(
                self.server, instrument_failure="xvfb failed to start"
            )
        )
        self.assertTrue(
            any("instrument" in finding for finding in row["findings"])
        )

    def windows_receipt(self, behavioral: dict[str, object]) -> dict[str, object]:
        receipt = smoke_receipt(
            self.server,
            platform="win32",
            vscode_version="stable",
            stages={"behavioral_smoke": behavioral},
        )
        receipt["overall"] = (
            "pass" if behavioral.get("status") == "pass" else "failed"
        )
        return receipt

    def test_windows_behavioral_guard_is_unsupported_not_product_defect(self) -> None:
        self.package("windows")
        row = self.build_row(
            receipt=self.windows_receipt(
                {"status": "failed", "reason": "published_extension_smoke_failed"}
            ),
            row_id="windows-current",
            platform="windows",
            vscode_version="stable",
            smoke_outcome="failure",
        )
        # The candidate-bound journey cannot execute on Windows by product
        # policy; its failure is the guard boundary, never a product defect.
        self.assertEqual(
            row["cells"]["packaged_provider_edit_journey"],
            "unsupported_or_withdrawn",
        )
        self.assertNotEqual(row["status"], "blocked")
        self.assertTrue(
            any("policy-restricted" in finding for finding in row["findings"])
        )

    def test_windows_behavioral_pass_contradicts_policy(self) -> None:
        self.package("windows")
        row = self.build_row(
            receipt=self.windows_receipt({"status": "pass"}),
            row_id="windows-current",
            platform="windows",
            vscode_version="stable",
        )
        # A candidate-bound behavioral pass on Windows means the product
        # policy moved; the row must be reclassified, not trusted.
        self.assertEqual(
            row["cells"]["packaged_provider_edit_journey"], "instrument_defect"
        )
        self.assertTrue(
            any("policy drifted" in finding for finding in row["findings"])
        )

    def test_arbitrary_archive_bytes_cannot_pass(self) -> None:
        self.archive.write_bytes(b"arbitrary-non-zip-bytes")
        row = self.build_row(receipt=smoke_receipt(self.server))
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertTrue(
            any("not a valid zip" in finding for finding in row["findings"])
        )

    def test_archive_with_wrong_member_set_fails(self) -> None:
        with zipfile.ZipFile(self.archive, "w") as unit:
            unit.writestr("perllsp", b"server-bytes")
            unit.writestr("artifact-manifest.json", b"{}")
        row = self.build_row(receipt=smoke_receipt(self.server))
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertTrue(
            any("members" in finding for finding in row["findings"])
        )

    def test_archive_with_substituted_server_bytes_fails(self) -> None:
        result = self.run_row(
            [
                "package",
                "--source-sha",
                SHA,
                "--source-version",
                VERSION,
                "--platform",
                "linux",
                "--architecture",
                "x64",
                "--server",
                str(self.dap),
                "--dap",
                str(self.dap),
                "--output",
                str(self.archive),
            ]
        )
        self.assertEqual(result, 0)
        # The archive was packaged from the DAP bytes under the server name, so
        # its perllsp member cannot be the built release server.
        with zipfile.ZipFile(self.archive) as unit:
            names = sorted(unit.namelist())
        self.assertIn("perl-dap", names)
        row = self.build_row(receipt=smoke_receipt(self.server))
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")

    def test_manifest_from_another_source_sha_fails(self) -> None:
        result = self.run_row(
            [
                "package",
                "--source-sha",
                OTHER_SHA,
                "--source-version",
                VERSION,
                "--platform",
                "linux",
                "--architecture",
                "x64",
                "--server",
                str(self.server),
                "--dap",
                str(self.dap),
                "--output",
                str(self.archive),
            ]
        )
        self.assertEqual(result, 0)
        row = self.build_row(receipt=smoke_receipt(self.server))
        self.assertEqual(row["cells"]["artifact_identity"], "instrument_defect")
        self.assertTrue(
            any("source_sha" in finding for finding in row["findings"])
        )

    def test_row_axis_drift_fails_closed(self) -> None:
        result = self.run_row(
            [
                "row",
                "--source-sha",
                SHA,
                "--source-version",
                VERSION,
                "--row-id",
                "windows-current",
                "--platform",
                "linux",
                "--architecture",
                "x64",
                "--host-role",
                "current_stable",
                "--vscode-version",
                "stable",
                "--server",
                str(self.server),
                "--dap",
                str(self.dap),
                "--archive",
                str(self.archive),
                "--receipts-root",
                str(self.root / "receipts-none"),
                "--smoke-outcome",
                "failure",
                "--output",
                str(self.root / "drifted.json"),
            ]
        )
        self.assertEqual(result, 2)
        self.assertFalse((self.root / "drifted.json").exists())

    def test_host_role_drift_fails_closed(self) -> None:
        result = self.run_row(
            [
                "row",
                "--source-sha",
                SHA,
                "--source-version",
                VERSION,
                "--row-id",
                "linux-minimum",
                "--platform",
                "linux",
                "--architecture",
                "x64",
                "--host-role",
                "current_stable",
                "--vscode-version",
                "1.125.0",
                "--server",
                str(self.server),
                "--dap",
                str(self.dap),
                "--archive",
                str(self.archive),
                "--receipts-root",
                str(self.root / "receipts-none"),
                "--smoke-outcome",
                "failure",
                "--output",
                str(self.root / "wrong-role.json"),
            ]
        )
        self.assertEqual(result, 2)

    def test_package_output_is_deterministic(self) -> None:
        first = self.root / "first.zip"
        second = self.root / "second.zip"
        for output in (first, second):
            result = self.run_row(
                [
                    "package",
                    "--source-sha",
                    SHA,
                    "--source-version",
                    VERSION,
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
        write_json(self.topology, {"release": VERSION, "frozen_product_sha": SHA})

    def tearDown(self) -> None:
        self.temp.cleanup()

    def row(
        self,
        row_id: str,
        *,
        source_sha: str = SHA,
        verdict: str = "pass",
        cells: dict[str, str] | None = None,
        status: str | None = None,
        subject_override: dict[str, object] | None = None,
        nested_layout: bool = False,
        findings: list[str] | None = None,
    ) -> None:
        spec = MODULE.ROW_SPECS.get(
            row_id,
            {
                "platform": "linux",
                "architecture": "x64",
                "host_role": "current_stable",
                "host_selector": "stable",
            },
        )
        selector = "1.125.0" if spec["host_selector"] == "concrete" else "stable"
        subject: dict[str, object] = {
            "kind": "exact_current_main",
            "repository_sha": source_sha,
            "source_version": VERSION,
            "platform": spec["platform"],
            "architecture": spec["architecture"],
            "host_role": spec["host_role"],
            "vscode_selector": selector,
            "vscode_concrete_version": None if selector == "stable" else selector,
        }
        if subject_override:
            subject.update(subject_override)
        cell_map = (
            dict(cells)
            if cells is not None
            else {name: verdict for name in MODULE.REQUIRED_CELLS}
        )
        if cell_map:
            try:
                derived = MODULE.summarize_row(cell_map)
            except MODULE.ObservationError:
                derived = "pass"
        else:
            derived = "pass"
        value = {
            "schema_version": MODULE.ROW_SCHEMA,
            "row_id": row_id,
            "subject": subject,
            "artifacts": {
                "perllsp": {"name": "perllsp", "sha256": "1" * 64},
                "perl_dap": {"name": "perl-dap", "sha256": "2" * 64},
                "product_unit_archive": {
                    "name": "perl-lsp-product-unit.zip",
                    "sha256": "3" * 64,
                },
                "vsix": {"sha256": VSIX_SHA, "retained": False},
            },
            "mechanism_receipt": {
                "kind": "vscode_current_source_smoke.v1",
                "sha256": "4" * 64,
                "logical_name": "current-source-orchestration.json",
                "overall": "pass",
            },
            "cells": cell_map,
            "findings": findings or [],
            "zero_budget_counts": {
                key: (
                    1
                    if key == "silent_product_failure" and verdict == "product_defect"
                    else 0
                )
                for key in MODULE.ZERO_BUDGET_KEYS
            },
            "status": status if status is not None else derived,
        }
        if nested_layout:
            destination = (
                self.root
                / "rows"
                / f"rolling-installed-row-{row_id}"
                / "rolling-installed-row.json"
            )
        else:
            destination = self.root / "rows" / f"{row_id}.json"
        write_json(destination, value)

    def fan_in(self, require_ready: bool = False) -> tuple[int, dict[str, object]]:
        output = self.root / "packet.json"
        argv = [
            "fan-in",
            "--source-sha",
            SHA,
            "--source-version",
            VERSION,
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

    def test_fan_in_schema_is_not_the_canonical_packet(self) -> None:
        self.row("linux-minimum")
        _, packet = self.fan_in()
        self.assertEqual(
            packet["schema_version"], "rolling_installed_public_beta_fan_in.v1"
        )
        self.assertEqual(
            packet["canonical_packet_schema"], "pre_freeze_public_beta_acceptance.v1"
        )
        self.assertNotEqual(
            packet["schema_version"], packet["canonical_packet_schema"]
        )
        self.assertEqual(packet["target_release"], VERSION)
        self.assertEqual(packet["source_version"], VERSION)

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

    def test_three_row_artifact_directories_survive_handoff(self) -> None:
        # The fan-in job downloads one artifact directory per row. This layout
        # must yield three distinct rows; flattening them into one directory
        # (merge-multiple) would leave exactly one rolling-installed-row.json.
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id, nested_layout=True)
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertEqual(packet["missing_rows"], [])
        self.assertEqual(packet["platforms"]["linux"], "pass")
        self.assertEqual(packet["platforms"]["windows"], "pass")

    def test_product_defect_row_blocks_and_is_counted(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current", verdict="product_defect")
        self.row("windows-current")
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertEqual(packet["freeze_recommendation"], "blocked")
        self.assertEqual(packet["platforms"]["linux"], "blocked")
        self.assertTrue(packet["product_blockers"])
        self.assertTrue(
            all(
                blocker.startswith("linux-current:")
                for blocker in packet["product_blockers"]
            )
        )
        self.assertEqual(
            packet["zero_budget_counts"]["silent_product_failure"], 1
        )
        self.assertEqual(
            packet["journey_cells"]["linux-current"]["artifact_identity"],
            "product_defect",
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

    def test_opposite_platform_relabelling_fails(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        self.row("windows-current", subject_override={"platform": "linux"})
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertIn("windows-current", packet["missing_rows"])
        self.assertTrue(
            any("platform" in item for item in packet["instrument_defects"])
        )

    def test_duplicate_row_id_is_rejected(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        self.row("linux-minimum", nested_layout=True)  # duplicate id, new dir
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertTrue(
            any(
                "duplicate row_id" in item
                for item in packet["instrument_defects"]
            )
        )
        self.assertEqual(packet["freeze_recommendation"], "not_proven")

    def test_unexpected_row_id_is_rejected(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        self.row("macos-current")
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertTrue(
            any(
                "unexpected row" in item
                for item in packet["instrument_defects"]
            )
        )
        self.assertEqual(packet["freeze_recommendation"], "not_proven")

    def test_empty_cells_cannot_claim_pass(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        self.row("linux-minimum", cells={}, status="pass", nested_layout=True)
        # Replace the flat row with the empty-cell impostor.
        (self.root / "rows" / "linux-minimum.json").unlink()
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertIn("linux-minimum", packet["missing_rows"])
        self.assertNotEqual(packet["platforms"]["linux"], "pass")
        self.assertTrue(
            any(
                "cell denominator" in item
                for item in packet["instrument_defects"]
            )
        )

    def test_declared_status_cannot_override_derived_status(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        self.row("windows-current", verdict="instrument_defect", status="pass")
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertTrue(
            any(
                "declares status" in item
                for item in packet["instrument_defects"]
            )
        )
        # The derived (not declared) status drives the packet.
        self.assertEqual(
            packet["vs_code_hosts"]["current_stable_windows"]["status"],
            "not_proven",
        )
        self.assertEqual(packet["platforms"]["windows"], "not_proven")

    def test_unknown_cell_verdict_is_rejected(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        cells = {name: "pass" for name in MODULE.REQUIRED_CELLS}
        cells["artifact_identity"] = "green"
        self.row("windows-current", cells=cells, status="pass")
        _, packet = self.fan_in()
        self.assertIn("windows-current", packet["missing_rows"])
        self.assertTrue(
            any(
                "invalid verdict" in item
                for item in packet["instrument_defects"]
            )
        )

    def test_topology_from_another_release_fails_closed(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        write_json(
            self.topology, {"release": "9.9.9", "frozen_product_sha": SHA}
        )
        _, packet = self.fan_in()
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertIsNone(packet["release_topology_digest"])
        self.assertTrue(
            any(
                "release" in item and "9.9.9" in item
                for item in packet["instrument_defects"]
            )
        )

    def test_topology_from_another_source_sha_fails_closed(self) -> None:
        for row_id in MODULE.REQUIRED_ROWS:
            self.row(row_id)
        write_json(
            self.topology, {"release": VERSION, "frozen_product_sha": OTHER_SHA}
        )
        _, packet = self.fan_in()
        self.assertEqual(packet["freeze_recommendation"], "not_proven")
        self.assertIsNone(packet["release_topology_digest"])

    def test_row_findings_survive_fan_in(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        self.row(
            "windows-current",
            verdict="unsupported_or_withdrawn",
            findings=[
                "current-source smoke instrument reported a failure; affected "
                "cells are instrument evidence, not product evidence"
            ],
        )
        _, packet = self.fan_in()
        self.assertTrue(
            any(
                "instrument reported a failure" in item
                for item in packet["row_findings"]["windows-current"]
            )
        )

    def test_artifact_identity_pass_requires_exact_hashes(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        self.row("windows-current")
        # Null out the server identity on a row that still claims pass.
        row_path = self.root / "rows" / "windows-current.json"
        value = json.loads(row_path.read_text(encoding="utf-8"))
        value["artifacts"]["perllsp"]["sha256"] = None
        write_json(row_path, value)
        result, packet = self.fan_in(require_ready=True)
        self.assertEqual(result, 1)
        self.assertIn("windows-current", packet["missing_rows"])
        self.assertEqual(packet["platforms"]["windows"], "not_proven")
        self.assertTrue(
            any(
                "null" in item and "identities" in item
                for item in packet["instrument_defects"]
            )
        )

    def test_boolean_or_negative_zero_budget_counts_are_malformed(self) -> None:
        for bad_value in (True, -1):
            with self.subTest(bad_value=bad_value):
                rows = self.root / "rows"
                if rows.exists():
                    for stale in rows.rglob("*.json"):
                        stale.unlink()
                self.row("linux-minimum")
                self.row("linux-current")
                self.row("windows-current")
                row_path = rows / "windows-current.json"
                value = json.loads(row_path.read_text(encoding="utf-8"))
                value["zero_budget_counts"]["false_exact"] = bad_value
                write_json(row_path, value)
                result, packet = self.fan_in()
                self.assertEqual(result, 0)
                self.assertIn("windows-current", packet["missing_rows"])
                self.assertTrue(
                    any(
                        "zero-budget count false_exact is malformed" in item
                        for item in packet["instrument_defects"]
                    )
                )

    def test_invalid_utf8_row_is_malformed_not_fatal(self) -> None:
        self.row("linux-minimum")
        self.row("linux-current")
        corrupt = self.root / "rows" / "windows-current.json"
        corrupt.write_bytes(b'\xff\xfe{"schema_version":')
        result, packet = self.fan_in()
        self.assertEqual(result, 0)
        self.assertIn("windows-current", packet["missing_rows"])
        self.assertTrue(
            any(
                "cannot read JSON" in item
                for item in packet["instrument_defects"]
            )
        )


if __name__ == "__main__":
    unittest.main()
