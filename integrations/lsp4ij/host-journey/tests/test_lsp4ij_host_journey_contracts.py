"""Discriminating tests for the LSP4IJ host-journey contracts.

These suites pin three claims:

1. a well-formed declared-host launch spec and a well-formed session receipt
   are accepted (calibration objects below are constructed purely to exercise
   the validators offline; they are never evidence and never leave test scope);
2. malformed and hostile receipts/specs are rejected — most importantly any
   receipt stamped with a synthetic capture origin, which is forbidden for
   production closure;
3. the checked-in JSON schemas and the hand validators do not drift apart.

Run with::

    python -m unittest discover -s integrations/lsp4ij/host-journey/tests -p "test_*.py"
"""
from __future__ import annotations

import copy
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HOST_JOURNEY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HOST_JOURNEY_DIR))

import validate_lsp4ij_host_receipt as receipt_validator  # noqa: E402
import validate_lsp4ij_launch_spec as spec_validator  # noqa: E402

VALID_BUILD_NUMBER = "IC-241.18034.62"
VENDORED_LSP4IJ_COMMIT = "1f62a3f8d8718db00b3db9189772f3a9172e4fb3"
PLACEHOLDER_SOURCE_SHA = "0" * 40


def _sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


BINARY_SHA = _sha("perllsp-binary")


def make_valid_launch_spec() -> dict:
    return {
        "schema_version": 1,
        "stage": "declared_host",
        "source_sha": PLACEHOLDER_SOURCE_SHA,
        "declared_ide": {
            "product": "IntelliJ IDEA Community Edition",
            "build_number": VALID_BUILD_NUMBER,
            "platform": "windows",
            "arch": "x64",
            "distribution_root": "${PERLLSP_INTELLIJ_CE_HOME}",
        },
        "lsp4ij_plugin": {
            "id": "com.redhat.devtools.lsp4ij",
            "version": "0.20.1",
            "upstream_repository": "redhat-developer/lsp4ij",
            "pinned_commit": VENDORED_LSP4IJ_COMMIT,
            "source": "pinned_release_archive",
        },
        "server_binary": {
            "path": "${CARGO_TARGET_DIR}/release/perllsp.exe",
            "sha256": BINARY_SHA,
            "command": ["${CARGO_TARGET_DIR}/release/perllsp.exe", "--stdio"],
        },
        "sandbox": {
            "config_root": "${PERLLSP_LSP4IJ_SANDBOX}/config",
            "system_root": "${PERLLSP_LSP4IJ_SANDBOX}/system",
            "plugins_root": "${PERLLSP_LSP4IJ_SANDBOX}/plugins",
            "log_root": "${PERLLSP_LSP4IJ_SANDBOX}/log",
        },
        "fixture_project": {
            "root": "integrations/lsp4ij/host-journey/host-fixture",
        },
    }


def make_valid_receipt() -> dict:
    """Validator calibration object.

    Constructed offline to prove what the validator admits; it records no
    real host observation and is never published as a receipt artifact.
    Production closure always comes from receipts produced by an actual
    hosted journey run.
    """
    return {
        "schema_version": 1,
        "stage": "exact_source_local",
        "source_sha": PLACEHOLDER_SOURCE_SHA,
        "recorded_at": "2026-08-26T18:00:00+00:00",
        # Bound for real: recomputed from the calibration launch spec using
        # the exact canonicalization the validators enforce.
        "launch_spec_digest": spec_validator.canonical_spec_digest(make_valid_launch_spec()),
        "host": {
            "product": "IntelliJ IDEA Community Edition",
            "build_number": VALID_BUILD_NUMBER,
            "platform": "windows",
            "arch": "x64",
            "os_version": "Windows 11 Pro 26200",
            "jbr_version": "21.0.5b8.1",
        },
        "lsp4ij_plugin": {
            "id": "com.redhat.devtools.lsp4ij",
            "version": "0.20.1",
            "upstream_repository": "redhat-developer/lsp4ij",
            "pinned_commit": VENDORED_LSP4IJ_COMMIT,
        },
        "server_binary": {
            "path": "${CARGO_TARGET_DIR}/release/perllsp.exe",
            "sha256": BINARY_SHA,
            "command": ["${CARGO_TARGET_DIR}/release/perllsp.exe", "--stdio"],
        },
        "session_initialize": {
            "origin": "live_wire_capture",
            "request_sha256": _sha("initialize-request"),
            "response_sha256": _sha("initialize-response"),
            "observed_capabilities": {
                "completion": True,
                "hover": True,
                "diagnostic": True,
                "documentSymbol": True,
            },
        },
        "repro_readiness": {
            "origin": "live_wire_capture",
            "fixture_opened": True,
            "first_diagnostics_settled": True,
            "evidence_sha256": _sha("diagnostics-settle"),
        },
        "provider_taps": [
            {
                "provider": "completion",
                "file_suffix": ".pl",
                "origin": "live_wire_capture",
                "result_sha256": _sha("completion-result"),
                "latency_ms": 42,
            },
            {
                "provider": "hover",
                "file_suffix": ".pm",
                "origin": "live_wire_capture",
                "result_sha256": _sha("hover-result"),
            },
            {
                "provider": "diagnostic",
                "file_suffix": ".pl",
                "origin": "live_wire_capture",
                "result_sha256": _sha("diagnostic-result"),
            },
            {
                "provider": "references",
                "file_suffix": ".t",
                "origin": "live_wire_capture",
                "result_sha256": _sha("references-result"),
            },
        ],
        "process_ledger": {
            "spawned_server_pids": [4711],
            "all_orderly_exited": True,
        },
    }


class ContractCase(unittest.TestCase):
    def assertRejected(self, mutator, builder, validator) -> None:
        payload = builder()
        mutator(payload)
        with self.assertRaises(ValueError):
            validator.validate(copy.deepcopy(payload))


class LaunchSpecContractTest(ContractCase):
    def test_calibration_spec_is_accepted(self) -> None:
        spec_validator.validate(make_valid_launch_spec())

    def test_checked_in_example_spec_is_accepted(self) -> None:
        example = json.loads(
            (HOST_JOURNEY_DIR / "declared-host.launch-spec.example.json").read_text(encoding="utf-8")
        )
        spec_validator.validate(example)

    def test_stage_drift_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s.update(stage="exact_source_local"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_release_archive_pin_requires_exact_commit(self) -> None:
        self.assertRejected(
            lambda s: s["lsp4ij_plugin"].pop("pinned_commit"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_below_maintained_lsp4ij_line_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["lsp4ij_plugin"].update(version="0.19.5"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_ambiguous_marketplace_pin_with_wrong_commit_shape(self) -> None:
        self.assertRejected(
            lambda s: s["lsp4ij_plugin"].update(pinned_commit="deadbeef"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_duplicate_sandbox_roots_are_rejected(self) -> None:
        def share_log_dir(spec: dict) -> None:
            spec["sandbox"]["plugins_root"] = spec["sandbox"]["log_root"]

        self.assertRejected(share_log_dir, make_valid_launch_spec, spec_validator)

    def test_missing_sandbox_plane_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["sandbox"].pop("config_root"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_non_stdio_command_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["server_binary"].update(command=["perllsp", "--pipe"]),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_foreign_server_binary_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["server_binary"].update(command=["perl-languageserver", "--stdio"]),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_bad_ide_build_number_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["declared_ide"].update(build_number="241.18034.62"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_omitted_binary_digest_is_rejected(self) -> None:
        self.assertRejected(
            lambda s: s["server_binary"].pop("sha256"),
            make_valid_launch_spec,
            spec_validator,
        )

    def test_declared_path_and_command_target_must_be_the_same_reference(self) -> None:
        def diverge(s: dict) -> None:
            s["server_binary"]["path"] = "${CARGO_TARGET_DIR}/release/perllsp.exe"
            s["server_binary"]["command"] = ["${PERLLSP_OVERRIDE_DIR}/perllsp.exe", "--stdio"]

        self.assertRejected(diverge, make_valid_launch_spec, spec_validator)

    def test_separator_and_case_normalization_still_bind_one_reference(self) -> None:
        spec = make_valid_launch_spec()
        if spec_validator.os.path.sep == "\\":
            spec["server_binary"]["command"][0] = "${cargo_target_dir}\\RELEASE\\perllsp.exe"
        else:
            spec["server_binary"]["command"][0] = "${CARGO_TARGET_DIR}/release/../release/perllsp.exe"
        spec_validator.validate(spec)
        # Same declared reference, different byte-level spelling: this is a
        # different spec file, so a receipt bound to it must carry ITS digest.
        receipt = make_valid_receipt()
        receipt["launch_spec_digest"] = spec_validator.canonical_spec_digest(spec)
        receipt_validator.validate_bound_to_launch_spec(receipt, spec)


class HostReceiptContractTest(ContractCase):
    def test_calibration_receipt_is_accepted(self) -> None:
        receipt_validator.validate(make_valid_receipt())

    def test_every_non_live_origin_is_forbidden(self) -> None:
        """The synthetic-forbidden law: no origin other than live_wire_capture closes production."""
        for section in ("session_initialize", "repro_readiness"):
            self.assertRejected(
                lambda r, section=section: r[section].update(origin="synthetic"),
                make_valid_receipt,
                receipt_validator,
            )
            self.assertRejected(
                lambda r, section=section: r[section].update(origin="replay_from_fixture"),
                make_valid_receipt,
                receipt_validator,
            )
        self.assertRejected(
            lambda r: r["provider_taps"][1].update(origin="synthetic_unit_input"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_initialize_capture_without_request_digest_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["session_initialize"].pop("request_sha256"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_uppercase_digest_is_rejected(self) -> None:
        def upper_request(r: dict) -> None:
            r["session_initialize"]["response_sha256"] = (
                r["session_initialize"]["response_sha256"].upper()
            )

        self.assertRejected(upper_request, make_valid_receipt, receipt_validator)

    def test_truncated_source_sha_is_rejected(self) -> None:
        self.assertRejected(lambda r: r.update(source_sha="abc123"), make_valid_receipt, receipt_validator)

    def test_non_boolean_capability_presence_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["session_initialize"]["observed_capabilities"].update(hover="yes"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_core_capability_observed_absent_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["session_initialize"]["observed_capabilities"].update(hover=False),
            make_valid_receipt,
            receipt_validator,
        )

    def test_taps_missing_core_surface_are_rejected(self) -> None:
        def drop_core(r: dict) -> None:
            r["provider_taps"] = [
                tap for tap in r["provider_taps"] if tap["provider"] in {"formatting", "references"}
            ] or [{"provider": "formatting", "file_suffix": ".pl", "origin": "live_wire_capture",
                   "result_sha256": _sha("fmt")}]

        self.assertRejected(drop_core, make_valid_receipt, receipt_validator)

    def test_taps_not_exercising_the_pl_subject_are_rejected(self) -> None:
        def only_pm(r: dict) -> None:
            for tap in r["provider_taps"]:
                tap["file_suffix"] = ".pm"

        self.assertRejected(only_pm, make_valid_receipt, receipt_validator)

    def test_unknown_provider_name_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["provider_taps"][0].update(provider="inlineCompletionMagic"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_pid_ledger_rejects_zero_negative_and_duplicates(self) -> None:
        self.assertRejected(lambda r: r["process_ledger"].update(spawned_server_pids=[0]), make_valid_receipt,
                            receipt_validator)
        self.assertRejected(lambda r: r["process_ledger"].update(spawned_server_pids=[-7]), make_valid_receipt,
                            receipt_validator)
        self.assertRejected(lambda r: r["process_ledger"].update(spawned_server_pids=[9, 9]), make_valid_receipt,
                            receipt_validator)

    def test_disorderly_shutdown_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["process_ledger"].update(all_orderly_exited=False),
            make_valid_receipt,
            receipt_validator,
        )

    def test_naive_timestamp_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r.update(recorded_at="2026-08-26T18:00:00"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_plugin_identity_drift_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["lsp4ij_plugin"].update(id="org.perl.intellij.plugin"),
            make_valid_receipt,
            receipt_validator,
        )
        self.assertRejected(
            lambda r: r["lsp4ij_plugin"].update(version="0.12.2"),
            make_valid_receipt,
            receipt_validator,
        )

    def test_command_drift_after_the_fact_is_rejected(self) -> None:
        self.assertRejected(
            lambda r: r["server_binary"].update(command=["target/release/perllsp.exe", "--port=7777"]),
            make_valid_receipt,
            receipt_validator,
        )

    def test_undeclared_top_level_key_is_rejected(self) -> None:
        self.assertRejected(lambda r: r.update(notes={"extra": 1}), make_valid_receipt, receipt_validator)


class SchemaValidatorParityTest(unittest.TestCase):
    """The checked-in schemas are the machine-readable face of the contract;
    they must not drift away from what the hand validators enforce."""

    @staticmethod
    def _load(name: str) -> dict:
        return json.loads((HOST_JOURNEY_DIR / name).read_text(encoding="utf-8"))

    def test_top_level_required_keys_match_contract_faces(self) -> None:
        receipt_schema = self._load("lsp4ij-host-receipt.v1.schema.json")
        self.assertEqual(set(receipt_schema["required"]), set(make_valid_receipt()))

        spec_schema = self._load("lsp4ij-launch-spec.v1.schema.json")
        self.assertEqual(set(spec_schema["required"]), set(make_valid_launch_spec()))

    def test_every_origin_property_pins_the_live_capture_enum(self) -> None:
        receipt_schema = self._load("lsp4ij-host-receipt.v1.schema.json")
        origins = []
        stack = list(receipt_schema["properties"].values())
        while stack:
            node = stack.pop()
            if isinstance(node, dict):
                if "origin" in node.get("properties", {}):
                    origins.append(node["properties"]["origin"])
                stack.extend(value for value in node.values() if isinstance(value, (dict, list)))
        items = receipt_schema["properties"]["provider_taps"]["items"]
        origins.append(items["properties"]["origin"])
        self.assertTrue(origins)
        for origin in origins:
            self.assertEqual(origin.get("$ref"), "#/$defs/capture_origin")
        self.assertEqual(receipt_schema["$defs"]["capture_origin"]["enum"], ["live_wire_capture"])

    def test_capability_map_values_are_boolean_typed(self) -> None:
        receipt_schema = self._load("lsp4ij-host-receipt.v1.schema.json")
        capability_map = receipt_schema["properties"]["session_initialize"]["properties"]["observed_capabilities"]
        self.assertEqual(capability_map.get("additionalProperties"), {"type": "boolean"})

    def test_schema_encodes_the_required_true_core_capabilities(self) -> None:
        """Schema-only consumers must reach the same core verdict as the hand validator."""
        receipt_schema = self._load("lsp4ij-host-receipt.v1.schema.json")
        capability_map = receipt_schema["properties"]["session_initialize"]["properties"]["observed_capabilities"]
        self.assertEqual(set(capability_map["required"]), {"completion", "hover", "diagnostic"})
        for name in ("completion", "hover", "diagnostic"):
            self.assertEqual(capability_map["properties"][name], {"const": True})

    def test_launch_spec_schema_requires_the_binary_digest(self) -> None:
        spec_schema = self._load("lsp4ij-launch-spec.v1.schema.json")
        self.assertEqual(
            set(spec_schema["properties"]["server_binary"]["required"]),
            {"path", "sha256", "command"},
        )


class ReceiptLaunchSpecBindingTest(ContractCase):
    """A receipt means nothing unless it is provably the observation of its
    declared precondition; the validator must bind them."""

    def test_calibration_pair_binds_cleanly(self) -> None:
        receipt_validator.validate_bound_to_launch_spec(make_valid_receipt(), make_valid_launch_spec())

    def test_receipt_supplied_with_a_different_spec_is_rejected(self) -> None:
        other = make_valid_launch_spec()
        other["source_sha"] = "1" * 40
        with self.assertRaises(ValueError):
            receipt_validator.validate_bound_to_launch_spec(make_valid_receipt(), other)

    def test_identity_drift_under_a_correctly_recomputed_digest_is_rejected(self) -> None:
        drifted = make_valid_receipt()
        drifted["lsp4ij_plugin"]["version"] = "0.21.0"
        with self.assertRaises(ValueError):
            receipt_validator.validate_bound_to_launch_spec(drifted, make_valid_launch_spec())

    def test_source_sha_drift_between_observation_and_declaration_is_rejected(self) -> None:
        drifted = make_valid_receipt()
        drifted["source_sha"] = "a" * 40
        with self.assertRaises(ValueError):
            receipt_validator.validate_bound_to_launch_spec(drifted, make_valid_launch_spec())

    def test_command_target_drift_from_the_declared_path_is_rejected(self) -> None:
        drifted = make_valid_receipt()
        drifted["server_binary"]["command"][0] = "${CARGO_TARGET_DIR}/debug/perllsp.exe"
        with self.assertRaises(ValueError):
            receipt_validator.validate_bound_to_launch_spec(drifted, make_valid_launch_spec())


class ValidatorCliBehaviorTest(unittest.TestCase):
    def _run(self, script: str, *targets: Path | None) -> subprocess.CompletedProcess[str]:
        command = [sys.executable, str(HOST_JOURNEY_DIR / script)]
        command.extend(str(t) for t in targets if t is not None)
        return subprocess.run(command, capture_output=True, text=True, timeout=60, check=False)

    def test_receipt_cli_requires_the_launch_spec(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "receipt.json"
            receipt.write_text(json.dumps(make_valid_receipt()), encoding="utf-8")
            usage = self._run("validate_lsp4ij_host_receipt.py", receipt)
            self.assertEqual(usage.returncode, 2)

            spec = Path(tmp) / "spec.json"
            spec.write_text(json.dumps(make_valid_launch_spec()), encoding="utf-8")
            good = self._run("validate_lsp4ij_host_receipt.py", receipt, spec)
            self.assertEqual(good.returncode, 0)

    def test_receipt_cli_rejects_a_mismatched_launch_spec(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "receipt.json"
            receipt.write_text(json.dumps(make_valid_receipt()), encoding="utf-8")
            other = make_valid_launch_spec()
            other["declared_ide"]["build_number"] = "IC-251.23774.435"
            wrong = Path(tmp) / "other-spec.json"
            wrong.write_text(json.dumps(other), encoding="utf-8")
            bound = self._run("validate_lsp4ij_host_receipt.py", receipt, wrong)
            self.assertEqual(bound.returncode, 1)
            self.assertIn("launch_spec_digest", bound.stderr)

    def test_invalid_or_missing_files_exit_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            spec = Path(tmp) / "spec.json"
            spec.write_text(json.dumps(make_valid_launch_spec()), encoding="utf-8")

            bad = Path(tmp) / "bad.json"
            bad.write_text(json.dumps({"schema_version": 2}), encoding="utf-8")
            invalid = self._run("validate_lsp4ij_host_receipt.py", bad, spec)
            self.assertEqual(invalid.returncode, 1)
            self.assertIn("bad.json", invalid.stderr)

            missing = Path(tmp) / "absent.json"
            nonexistent = self._run("validate_lsp4ij_host_receipt.py", missing, spec)
            self.assertEqual(nonexistent.returncode, 1)

    def test_launch_spec_cli_end_to_end(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            good = Path(tmp) / "spec.json"
            good.write_text(json.dumps(make_valid_launch_spec()), encoding="utf-8")
            self.assertEqual(self._run("validate_lsp4ij_launch_spec.py", good).returncode, 0)

            bad = Path(tmp) / "spec-bad.json"
            payload = make_valid_launch_spec()
            payload["sandbox"]["log_root"] = payload["sandbox"]["config_root"]
            bad.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(self._run("validate_lsp4ij_launch_spec.py", bad).returncode, 1)


if __name__ == "__main__":
    unittest.main()
