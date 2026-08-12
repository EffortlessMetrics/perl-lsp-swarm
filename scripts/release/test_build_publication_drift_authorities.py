#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("build_publication_drift_authorities.py")
SPEC = importlib.util.spec_from_file_location("build_publication_drift_authorities", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
authorities = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = authorities
SPEC.loader.exec_module(authorities)

AuthorityError = authorities.AuthorityError
build = authorities.build


def observation() -> dict[str, object]:
    return {
        "swarm": {
            "repository": "EffortlessMetrics/perl-lsp-swarm",
            "sha": "a" * 40,
            "tree_digest": "b" * 64,
            "version": "0.18.0",
        },
        "public": {
            "repository": "EffortlessMetrics/perl-lsp",
            "sha": "c" * 40,
            "tree_digest": "d" * 64,
            "version": "0.18.0",
        },
        "manifest": {"path": "policy/publication.json", "sha256": "e" * 64},
    }


def control() -> dict[str, str]:
    return {
        "schema_version": "perl_lsp.publication_drift_control.v1",
        "control_sha": "f" * 40,
        "control_tree_digest": "1" * 64,
        "workflow_sha256": "2" * 64,
    }


class PublicationDriftAuthoritiesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.topology = self.root / "topology.json"
        self.claims = self.root / "claims.json"
        self.api = self.root / "api.json"
        self.runtime = self.root / "runtime.json"
        for path, content in (
            (self.topology, "topology"),
            (self.claims, "claims"),
            (self.api, "api"),
            (self.runtime, "runtime"),
        ):
            path.write_text(content, encoding="utf-8")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_packet_hashes_exact_authority_bytes_and_subjects(self) -> None:
        value = build(
            observation(),
            control(),
            self.root,
            self.topology,
            self.claims,
            self.api,
            self.runtime,
        )
        self.assertEqual(value["control"], control())
        self.assertEqual(value["subjects"]["public"]["sha"], "c" * 40)
        for field in ("topology", "public_claims", "api_audit", "runtime_bundle"):
            self.assertRegex(value[field]["sha256"], r"^[0-9a-f]{64}$")
            self.assertFalse(Path(value[field]["path"]).is_absolute())

    def test_runtime_bundle_may_be_absent_for_preparation(self) -> None:
        value = build(
            observation(),
            control(),
            self.root,
            self.topology,
            self.claims,
            self.api,
            None,
        )
        self.assertIsNone(value["runtime_bundle"])

    def test_authority_path_escape_is_rejected(self) -> None:
        escaped = self.root.parent / "escaped.json"
        escaped.write_text("outside", encoding="utf-8")
        with self.assertRaisesRegex(AuthorityError, "escapes"):
            build(
                observation(),
                control(),
                self.root,
                escaped,
                self.claims,
                self.api,
                None,
            )

    def test_control_or_subject_identity_is_load_bearing(self) -> None:
        broken_control = control()
        broken_control["control_sha"] = "invalid"
        with self.assertRaisesRegex(AuthorityError, "control identity"):
            build(
                observation(),
                broken_control,
                self.root,
                self.topology,
                self.claims,
                self.api,
                None,
            )


if __name__ == "__main__":
    unittest.main()
