#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("generate_release_topology.py")
SPEC = importlib.util.spec_from_file_location("release_topology", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseTopologyTests(unittest.TestCase):
    def test_target_derivation_preserves_runner_and_archive_identity(self):
        workflow = """
        matrix:
          include:
            - target: x86_64-unknown-linux-gnu
              os: ubuntu-22.04
            - target: aarch64-pc-windows-msvc
              os: windows-11-arm
        steps:
        """
        targets = MODULE.derive_targets(workflow, "0.18.0")
        self.assertEqual([entry["target"] for entry in targets], [
            "x86_64-unknown-linux-gnu", "aarch64-pc-windows-msvc"
        ])
        self.assertEqual(targets[0]["archive_name"], "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz")
        self.assertEqual(targets[1]["required_members"][:2], ["perllsp.exe", "perl-dap.exe"])

    def test_duplicate_target_is_rejected(self):
        workflow = """
        matrix:
          include:
            - target: x86_64-unknown-linux-gnu
              os: ubuntu-22.04
            - target: x86_64-unknown-linux-gnu
              os: ubuntu-22.04
        steps:
        """
        with self.assertRaises(MODULE.TopologyError):
            MODULE.derive_targets(workflow, "0.18.0")

    def test_publish_cycle_is_rejected(self):
        metadata = {
            "metadata": {"publish": {"allow": ["a", "b"]}},
            "workspace_members": ["a-id", "b-id"],
            "packages": [
                {"id": "a-id", "name": "a", "version": "0.18.0", "manifest_path": "/a/Cargo.toml", "publish": None, "dependencies": [{"name": "b", "source": None}]},
                {"id": "b-id", "name": "b", "version": "0.18.0", "manifest_path": "/b/Cargo.toml", "publish": None, "dependencies": [{"name": "a", "source": None}]},
            ],
        }
        with self.assertRaises(MODULE.TopologyError):
            MODULE.derive_crates(metadata)


if __name__ == "__main__":
    unittest.main()
