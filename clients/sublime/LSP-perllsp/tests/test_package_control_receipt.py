from __future__ import annotations

import copy
import hashlib
import importlib.util
import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path


PACKAGE_ROOT = Path(__file__).resolve().parents[1]
CLIENT_ROOT = PACKAGE_ROOT.parent
VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_package_control_receipt.py"
SCHEMA_PATH = CLIENT_ROOT / "sublime-package-control-receipt.v1.schema.json"
SUBJECT_PATH = CLIENT_ROOT / "package-control-subject.v1.json"
HOST_VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_host_receipt.py"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load {name}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_validator():
    return load_module(VALIDATOR_PATH, "sublime_package_control_receipt_validator")


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def fill_digest(nibble: str) -> str:
    return nibble * 64


def listed_subject() -> dict:
    validator = load_validator()
    subject = json.loads(SUBJECT_PATH.read_text(encoding="utf-8"))
    subject["status"] = "listed"
    subject["channel"]["observable"] = True
    subject["channel"]["listing_url"] = "https://packagecontrol.io/packages/LSP-perllsp"
    subject["channel"]["channel_commit"] = "c" * 40
    subject["package"]["version"] = "0.1.0"
    subject["package"]["tag"] = "v0.1.0"
    subject["package"]["tree_sha256"] = fill_digest("a")
    subject["package"]["installed_digest"] = fill_digest("b")
    subject["perllsp_asset"]["release"] = "v0.17.0"
    subject["perllsp_asset"]["target"] = "x86_64-unknown-linux-gnu"
    subject["perllsp_asset"]["asset_name"] = "perllsp"
    subject["perllsp_asset"]["asset_url"] = "https://example.test/perllsp"
    subject["perllsp_asset"]["asset_sha256"] = fill_digest("d")
    subject["compatibility"]["result"] = "compatible"
    subject["clean_profile"] = {
        field: True for field in validator.CLEAN_PROFILE_FIELDS
    }
    subject["clean_profile"]["identity"] = "clean-profile-test"
    return subject


def passing_receipt(subject: dict, subject_digest: str) -> dict:
    validator = load_validator()
    pending_pass = {"result": "pass", "evidence": "observed"}
    receipt = validator.not_run_template()
    receipt["result"] = "pass"
    receipt["recorded_at"] = "2026-09-29T00:00:00+00:00"
    receipt["listing"]["observable"] = True
    receipt["listing"]["status"] = "listed"
    receipt["install_route"] = validator.PASS_INSTALL_ROUTE
    receipt["host"] = {
        "name": "Sublime Text",
        "version": "4200",
        "platform": "linux",
        "arch": "x64",
    }
    receipt["lsp_package"] = copy.deepcopy(subject["lsp_package"])
    receipt["helper_package"].update(
        {
            "source": validator.PASS_HELPER_SOURCE,
            "version": subject["package"]["version"],
            "tag": subject["package"]["tag"],
            "tree_sha256": subject["package"]["tree_sha256"],
            "installed_digest": subject["package"]["installed_digest"],
        }
    )
    receipt["binary"] = {
        "path": "/tmp/perllsp",
        "sha256": fill_digest("e"),
        "command": ["/tmp/perllsp", "--stdio"],
        "release": subject["perllsp_asset"]["release"],
        "archive_sha256": subject["perllsp_asset"]["asset_sha256"],
    }
    receipt["resolution_route"] = "managed_download"
    receipt["compatibility"] = copy.deepcopy(subject["compatibility"])
    receipt["fixtures"] = {
        "pl": "app.pl",
        "pm": "customlib/Greeting.pm",
        "t": "t/greeting.t",
        "digest": fill_digest("f"),
    }
    receipt["clean_profile"] = {field: True for field in validator.CLEAN_PROFILE_FIELDS}
    receipt["activation"] = {name: dict(pending_pass) for name in validator.ACTIVATION_CELLS}
    receipt["journey"] = {
        name: dict(pending_pass)
        for name in (*validator.REQUIRED_JOURNEY_FOR_PASS, *validator.OPTIONAL_JOURNEY)
    }
    receipt["journey"]["command_7704"] = {"result": "not_proven", "evidence": None}
    receipt["promotion"]["public_artifact_installed"] = "eligible"
    receipt["public_subject"]["sha256"] = subject_digest
    return receipt


class PackageControlReceiptTests(unittest.TestCase):
    def test_committed_subject_stays_blocked_and_unpromoted(self) -> None:
        validator = load_validator()
        raw = SUBJECT_PATH.read_bytes()
        subject = json.loads(raw)
        validator.validate_subject(subject)
        self.assertEqual(subject["status"], "blocked_pending_listing")
        self.assertFalse(subject["channel"]["observable"])
        self.assertEqual(subject["promotion"]["public_artifact_installed"], "not_proven")
        self.assertEqual(subject["promotion"]["package_control_leading_setup"], "not_proven")
        self.assertIsNone(subject["package"]["version"])
        self.assertEqual(len(sha256_bytes(raw)), 64)

    def test_schema_and_not_run_template_share_the_public_stage(self) -> None:
        validator = load_validator()
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        template = validator.not_run_template()
        self.assertEqual(schema["properties"]["stage"]["const"], "package_control_public")
        self.assertEqual(
            schema["properties"]["public_subject"]["properties"]["relative_path"]["const"],
            validator.SUBJECT_RELATIVE_PATH,
        )
        self.assertEqual(
            set(schema["properties"]["journey"]["required"]),
            set(validator.REQUIRED_JOURNEY_FOR_PASS) | set(validator.OPTIONAL_JOURNEY),
        )
        validator.validate_shape(template)
        self.assertEqual(template["result"], "not_run")
        self.assertEqual(template["listing"]["status"], "blocked_pending_listing")

    def test_host_validator_still_rejects_public_stage_overclaim(self) -> None:
        host = load_module(HOST_VALIDATOR_PATH, "sublime_host_receipt_validator")
        payload = {
            "schema_version": 1,
            "stage": "package_control_public",
            "source_sha": "a" * 40,
            "recorded_at": "2026-09-29T00:00:00+00:00",
            "host": {
                "name": "Sublime Text",
                "version": "4200",
                "platform": "linux",
                "arch": "x64",
            },
            "lsp_package": {
                "repository": "sublimelsp/LSP",
                "ref": "cc9f5201d9f053d9ab67aa0ea575b494fd133803",
            },
            "helper_package": {
                "name": "LSP-perllsp",
                "source": "clients/sublime/LSP-perllsp",
            },
            "binary": {
                "path": "/tmp/perllsp",
                "sha256": fill_digest("b"),
                "command": ["/tmp/perllsp", "--stdio"],
            },
            "fixtures": {"pl": "app.pl", "pm": "Greeting.pm", "t": "greeting.t"},
            "assertions": {name: True for name in host.REQUIRED_ASSERTIONS},
        }
        with self.assertRaisesRegex(ValueError, "exact_source_local"):
            host.validate(payload)

    def test_shape_rejects_exact_source_stage_relabel(self) -> None:
        validator = load_validator()
        payload = validator.not_run_template()
        payload["stage"] = "exact_source_local"
        with self.assertRaisesRegex(ValueError, "package_control_public"):
            validator.validate_shape(payload)

    def test_pass_requires_listed_subject_bytes(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        receipt = passing_receipt(subject, sha256_bytes(subject_bytes))
        validator.validate_pass(receipt, subject_bytes)

        with self.assertRaisesRegex(ValueError, "bound public subject"):
            validator.validate_pass(receipt, None)

        blocked = SUBJECT_PATH.read_bytes()
        with self.assertRaisesRegex(ValueError, "public subject is not listed"):
            validator.validate_pass(receipt, blocked)

        receipt["public_subject"]["sha256"] = fill_digest("0")
        with self.assertRaisesRegex(ValueError, "does not match the bound subject bytes"):
            validator.validate_pass(receipt, subject_bytes)

    def test_local_and_custom_sources_cannot_pass(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        digest = sha256_bytes(subject_bytes)
        for route in (
            "exact_source_local",
            "extra_packages",
            "local_checkout",
            "custom_channel",
            "manual_copy",
        ):
            receipt = passing_receipt(subject, digest)
            receipt["install_route"] = route
            with self.assertRaisesRegex(
                ValueError,
                "install route must be package_control_default_channel",
            ):
                validator.validate_pass(receipt, subject_bytes)
        for source in validator.FORBIDDEN_PASS_SOURCES:
            receipt = passing_receipt(subject, digest)
            receipt["helper_package"]["source"] = source
            with self.assertRaisesRegex(
                ValueError,
                "exact-source helper source cannot satisfy a Package Control pass",
            ):
                validator.validate_pass(receipt, subject_bytes)

    def test_listing_alone_is_not_a_public_install_pass(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        receipt = passing_receipt(subject, sha256_bytes(subject_bytes))
        receipt["journey"]["default_channel_install"] = {"result": "not_proven", "evidence": None}
        with self.assertRaisesRegex(ValueError, "listing is not a behavior proof"):
            validator.validate_pass(receipt, subject_bytes)

    def test_activation_families_cannot_inherit(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        receipt = passing_receipt(subject, sha256_bytes(subject_bytes))
        receipt["activation"]["pm"] = dict(receipt["activation"]["pl"])
        receipt["activation"]["t"] = {"result": "not_proven", "evidence": None}
        with self.assertRaisesRegex(ValueError, "cannot inherit"):
            validator.validate_pass(receipt, subject_bytes)

    def test_dirty_profile_and_workspace_authority_fail_closed(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        digest = sha256_bytes(subject_bytes)

        cached = passing_receipt(subject, digest)
        cached["clean_profile"]["prior_cache_absent"] = False
        with self.assertRaisesRegex(ValueError, "clean-profile preconditions are not proven"):
            validator.validate_pass(cached, subject_bytes)

        competing = passing_receipt(subject, digest)
        competing["clean_profile"]["competing_perl_provider_absent"] = False
        with self.assertRaisesRegex(ValueError, "clean-profile preconditions are not proven"):
            validator.validate_pass(competing, subject_bytes)

        workspace = passing_receipt(subject, digest)
        workspace["clean_profile"]["workspace_does_not_replace_server_authority"] = False
        with self.assertRaisesRegex(ValueError, "cannot replace package-owned server authority"):
            validator.validate_pass(workspace, subject_bytes)

    def test_managed_binary_and_compatibility_are_exact(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        digest = sha256_bytes(subject_bytes)

        override = passing_receipt(subject, digest)
        override["resolution_route"] = "binary_override"
        with self.assertRaisesRegex(ValueError, "managed_download"):
            validator.validate_pass(override, subject_bytes)

        guessed = passing_receipt(subject, digest)
        guessed["compatibility"]["result"] = "not_proven"
        with self.assertRaisesRegex(ValueError, "compatibility result must be compatible"):
            validator.validate_pass(guessed, subject_bytes)

        subject_incompatible = listed_subject()
        subject_incompatible["compatibility"]["result"] = "incompatible"
        incompatible_bytes = json.dumps(subject_incompatible, sort_keys=True).encode("utf-8")
        claimed = passing_receipt(subject_incompatible, sha256_bytes(incompatible_bytes))
        claimed["compatibility"]["result"] = "compatible"
        with self.assertRaisesRegex(ValueError, "bound public subject is not compatible"):
            validator.validate_pass(claimed, incompatible_bytes)

        mismatch = passing_receipt(subject, digest)
        mismatch["helper_package"]["tag"] = "v9.9.9"
        with self.assertRaisesRegex(ValueError, "identities disagree"):
            validator.validate_pass(mismatch, subject_bytes)

        archive = passing_receipt(subject, digest)
        archive["binary"]["archive_sha256"] = fill_digest("0")
        with self.assertRaisesRegex(ValueError, "identities disagree"):
            validator.validate_pass(archive, subject_bytes)

        windows = passing_receipt(subject, digest)
        windows["host"]["platform"] = "windows"
        windows["host"]["arch"] = "x64"
        with self.assertRaisesRegex(ValueError, "asset target does not match the recorded host"):
            validator.validate_pass(windows, subject_bytes)

    def test_missing_subject_file_fails_closed_from_cli(self) -> None:
        validator = load_validator()
        with tempfile.TemporaryDirectory() as directory:
            receipt_path = Path(directory) / "receipt.json"
            receipt_path.write_text(json.dumps(validator.not_run_template()), encoding="utf-8")
            missing = Path(directory) / "missing-subject.json"
            with redirect_stderr(io.StringIO()):
                self.assertEqual(validator.main(["validate", str(receipt_path), str(missing)]), 1)

    def test_stale_diagnostics_unapplied_edit_and_orphan_fail_closed(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        digest = sha256_bytes(subject_bytes)

        stale = passing_receipt(subject, digest)
        stale["journey"]["pull_diagnostics_after_edit"] = {"result": "fail", "evidence": "stale"}
        with self.assertRaisesRegex(ValueError, "post-edit diagnostics are not proven current"):
            validator.validate_pass(stale, subject_bytes)

        unapplied = passing_receipt(subject, digest)
        unapplied["journey"]["workspace_edit_applied"] = {
            "result": "fail",
            "evidence": "returned-only",
        }
        with self.assertRaisesRegex(ValueError, "returned but not applied"):
            validator.validate_pass(unapplied, subject_bytes)

        orphan = passing_receipt(subject, digest)
        orphan["journey"]["clean_shutdown"] = {"result": "fail", "evidence": "orphan"}
        with self.assertRaisesRegex(ValueError, "orphaned server process"):
            validator.validate_pass(orphan, subject_bytes)

    def test_blocked_subject_cannot_invent_promotion(self) -> None:
        validator = load_validator()
        subject = json.loads(SUBJECT_PATH.read_text(encoding="utf-8"))
        subject["promotion"]["public_artifact_installed"] = "eligible"
        with self.assertRaisesRegex(ValueError, "promotion cannot be claimed on a blocked subject"):
            validator.validate_subject(subject)

        listed = listed_subject()
        listed["promotion"]["package_control_leading_setup"] = "eligible"
        with self.assertRaisesRegex(ValueError, "leading Package Control setup"):
            validator.validate_subject(listed)

    def test_not_run_receipt_cannot_mark_install_eligible(self) -> None:
        validator = load_validator()
        payload = validator.not_run_template()
        payload["promotion"]["public_artifact_installed"] = "eligible"
        with self.assertRaisesRegex(ValueError, "cannot be eligible without a pass"):
            validator.validate_shape(payload)

    def test_pass_may_mark_install_eligible_but_not_leading_setup(self) -> None:
        validator = load_validator()
        subject = listed_subject()
        subject_bytes = json.dumps(subject, sort_keys=True).encode("utf-8")
        receipt = passing_receipt(subject, sha256_bytes(subject_bytes))
        validator.validate_pass(receipt, subject_bytes)
        receipt["promotion"]["package_control_leading_setup"] = "eligible"
        with self.assertRaisesRegex(ValueError, "leading Package Control setup"):
            validator.validate_pass(receipt, subject_bytes)


if __name__ == "__main__":
    unittest.main()
