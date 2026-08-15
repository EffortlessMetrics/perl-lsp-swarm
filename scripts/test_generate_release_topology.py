#!/usr/bin/env python3
import importlib.util
import json
from contextlib import contextmanager
from copy import deepcopy
from tempfile import TemporaryDirectory
import unittest
from pathlib import Path
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("generate_release_topology.py")
SPEC = importlib.util.spec_from_file_location("release_topology", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseTopologyTests(unittest.TestCase):
    @contextmanager
    def valid_manifest_fixture(self):
        release = "0.18.0"
        frozen_sha = "a" * 40
        workflow = """
        matrix:
          include:
            - target: x86_64-unknown-linux-gnu
              os: ubuntu-22.04
        steps:
        """
        downstream = {"targets": [{"triple": "x86_64-unknown-linux-gnu"}]}
        downloader = """
        return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
        archPrefix = arch === 'arm64' ? 'aarch64' : 'x86_64';
        return `${archPrefix}-unknown-linux-${libc}`;
        value === 'gnu';
        value === 'musl';
        return 'x86_64-pc-windows-msvc';
        """
        metadata = {
            "metadata": {"publish": {"allow": ["fixture-crate"]}},
            "workspace_members": ["fixture-id"],
            "packages": [
                {
                    "id": "fixture-id",
                    "name": "fixture-crate",
                    "version": release,
                    "manifest_path": "PLACEHOLDER",
                    "publish": None,
                    "dependencies": [],
                }
            ],
        }

        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "fixture/Cargo.toml").parent.mkdir(parents=True)
            (root / "fixture/Cargo.toml").write_text(
                "[package]\nname = 'fixture-crate'\n", encoding="utf-8"
            )
            metadata["packages"][0]["manifest_path"] = str(root / "fixture/Cargo.toml")
            files = {
                "Cargo.toml": "[workspace.package]\nversion = '0.18.0'\n",
                "Cargo.lock": "# fixture lock\n",
                ".github/workflows/release.yml": workflow,
                "vscode-extension/package.json": json.dumps(
                    {"name": "perl-lsp-rs", "version": release}
                ),
                "docs/reference/downstream-dap-integrations.json": json.dumps(
                    downstream
                ),
                "vscode-extension/src/downloader.ts": downloader,
                "scripts/inject-sha-assets.sh": "#!/bin/sh\n",
            }
            for relative, contents in files.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents, encoding="utf-8")

            targets = MODULE.derive_targets(workflow, release)
            crates = MODULE.derive_crates(metadata)
            for entry in crates:
                entry["package_path"] = (
                    Path(entry["package_path"])
                    .resolve()
                    .relative_to(root.resolve())
                    .as_posix()
                )
            manifest = {
                "schema": MODULE.SCHEMA,
                "release": release,
                "frozen_product_sha": frozen_sha,
                "prepared_swarm_sha": None,
                "workspace_version": release,
                "published_crates": crates,
                "crate_count": len(crates),
                "binary_targets": targets,
                "archive_count": len(targets),
                "vsix": {
                    "version": release,
                    "asset_name": "perl-lsp-rs-0.18.0.vsix",
                    "package_path": "vscode-extension",
                    "managed_targets": ["x86_64-unknown-linux-gnu"],
                    "bundled_targets": [],
                },
                "primary_channels": MODULE.PRIMARY_CHANNELS,
                "sources": {},
            }
            for relative in MODULE.SOURCE_PATHS:
                path = root / relative
                manifest["sources"][relative] = {
                    "path": relative,
                    "sha256": MODULE.sha256(path),
                }

            with patch.object(
                MODULE, "cargo_metadata", return_value=metadata
            ), patch.object(MODULE, "git_head", return_value=frozen_sha):
                yield root, manifest, frozen_sha

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
        self.assertEqual(
            [entry["target"] for entry in targets],
            ["x86_64-unknown-linux-gnu", "aarch64-pc-windows-msvc"],
        )
        self.assertEqual(
            targets[0]["archive_name"], "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz"
        )
        self.assertEqual(
            targets[1]["required_members"][:2], ["perllsp.exe", "perl-dap.exe"]
        )

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
                {
                    "id": "a-id",
                    "name": "a",
                    "version": "0.18.0",
                    "manifest_path": "/a/Cargo.toml",
                    "publish": None,
                    "dependencies": [{"name": "b", "source": None}],
                },
                {
                    "id": "b-id",
                    "name": "b",
                    "version": "0.18.0",
                    "manifest_path": "/b/Cargo.toml",
                    "publish": None,
                    "dependencies": [{"name": "a", "source": None}],
                },
            ],
        }
        with self.assertRaises(MODULE.TopologyError):
            MODULE.derive_crates(metadata)

    def test_downloader_target_derivation_requires_native_windows_arm64(self):
        source = """
        return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
        return `${archPrefix}-unknown-linux-${libc}`;
        return 'x86_64-pc-windows-msvc';
        """
        workflow_targets = {
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        }
        self.assertNotIn(
            "aarch64-pc-windows-msvc",
            MODULE.derive_downloader_targets(source, workflow_targets),
        )

    def test_downloader_target_derivation_accepts_all_reachable_targets(self):
        source = """
        return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
        archPrefix = arch === 'arm64' ? 'aarch64' : 'x86_64';
        return `${archPrefix}-unknown-linux-${libc}`;
        value === 'gnu';
        value === 'musl';
        return 'x86_64-pc-windows-msvc';
        return 'aarch64-pc-windows-msvc';
        """
        workflow_targets = {
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        }
        self.assertEqual(
            MODULE.derive_downloader_targets(source, workflow_targets),
            workflow_targets,
        )

    def test_manifest_mutations_fail_closed(self):
        with self.valid_manifest_fixture() as (root, manifest, frozen_sha):
            MODULE.validate_manifest(manifest, root, frozen_sha)
            mutations = {}

            missing_crate = deepcopy(manifest)
            missing_crate["published_crates"] = []
            missing_crate["crate_count"] = 0
            mutations["missing crate"] = missing_crate

            extra_crate = deepcopy(manifest)
            extra_crate["published_crates"].append({"name": "unexpected"})
            extra_crate["crate_count"] = len(extra_crate["published_crates"])
            mutations["extra crate"] = extra_crate

            missing_target = deepcopy(manifest)
            missing_target["binary_targets"] = []
            missing_target["archive_count"] = 0
            mutations["missing target"] = missing_target

            extra_target = deepcopy(manifest)
            extra_target["binary_targets"].append(
                {
                    **extra_target["binary_targets"][0],
                    "target": "aarch64-unknown-linux-gnu",
                    "archive_name": "perllsp-0.18.0-aarch64-unknown-linux-gnu.tar.gz",
                }
            )
            extra_target["archive_count"] = len(extra_target["binary_targets"])
            mutations["extra target"] = extra_target

            archive_member = deepcopy(manifest)
            archive_member["binary_targets"][0]["required_members"][0] = "tampered"
            mutations["archive member"] = archive_member

            managed_target = deepcopy(manifest)
            managed_target["vsix"]["managed_targets"].append(
                "aarch64-unknown-linux-gnu"
            )
            mutations["managed target mapping"] = managed_target

            vsix_version = deepcopy(manifest)
            vsix_version["vsix"]["version"] = "0.18.1"
            mutations["VSIX version"] = vsix_version

            source_hash = deepcopy(manifest)
            source_hash["sources"]["Cargo.toml"]["sha256"] = "0" * 64
            mutations["source hash"] = source_hash

            missing_source = deepcopy(manifest)
            del missing_source["sources"]["Cargo.toml"]
            mutations["missing source"] = missing_source

            for name, candidate in mutations.items():
                with self.subTest(name=name), self.assertRaises(MODULE.TopologyError):
                    MODULE.validate_manifest(candidate, root, frozen_sha)


if __name__ == "__main__":
    unittest.main()
