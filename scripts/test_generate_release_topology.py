#!/usr/bin/env python3
import importlib.util
import io
import json
import subprocess
import sys
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from copy import deepcopy
from tempfile import TemporaryDirectory
import unittest
from pathlib import Path
from shutil import copytree
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
            root = Path(temporary) / "prepared"
            root.mkdir()
            (root / "fixture/Cargo.toml").parent.mkdir(parents=True)
            (root / "fixture/Cargo.toml").write_text(
                "[package]\nname = 'fixture-crate'\nversion = '0.18.0'\n\n"
                "[dependencies]\nfixture-crate = { path = '.', version = '0.18.0' }\n\n"
                "[dev-dependencies]\nfixture-crate = { path = '.', version = '0.18.0' }\n",
                encoding="utf-8",
            )
            metadata["packages"][0]["manifest_path"] = str(root / "fixture/Cargo.toml")
            files = {
                "Cargo.toml": "[workspace.package]\nversion = '0.18.0'\n\n"
                "[workspace.dependencies]\n"
                "fixture-crate = { path = 'fixture', version = '0.18.0' }\n",
                "Cargo.lock": "[[package]]\nname = 'fixture-crate'\n"
                "version = '0.18.0'\n\n"
                "[[package]]\nname = 'fixture-crate'\n"
                "version = '99.0.0'\nsource = 'registry+https://example.invalid'\n",
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
                "track": "public-beta",
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
                "secondary_channels": {"docker": "required", "homebrew": "deferred"},
                "sources": {},
            }
            for relative in MODULE.source_paths(crates):
                path = root / relative
                manifest["sources"][relative] = {
                    "path": relative,
                    "sha256": MODULE.sha256(path),
                }

            with patch.object(
                MODULE, "cargo_metadata", return_value=metadata
            ), patch.object(MODULE, "git_head", return_value=frozen_sha), patch.object(
                MODULE, "ensure_committed_topology_inputs"
            ):
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

    def test_schema_accepts_stable_and_rejects_exact_prerelease(self):
        with self.valid_manifest_fixture() as (_, manifest, _):
            MODULE.schema_validate(manifest)
            prerelease = deepcopy(manifest)
            prerelease["release"] = "0.18.0-rc.1"
            with self.assertRaises(MODULE.TopologyError):
                MODULE.schema_validate(prerelease)

    def test_schema_rejects_required_field_omissions_and_unknown_fields(self):
        with self.valid_manifest_fixture() as (root, manifest, frozen_sha):
            missing_track = deepcopy(manifest)
            del missing_track["track"]
            with self.assertRaises(MODULE.TopologyError):
                MODULE.validate_manifest(missing_track, root, frozen_sha)

            missing_channels = deepcopy(manifest)
            del missing_channels["secondary_channels"]
            with self.assertRaises(MODULE.TopologyError):
                MODULE.validate_manifest(missing_channels, root, frozen_sha)

            unknown = deepcopy(manifest)
            unknown["unexpected"] = True
            with self.assertRaises(MODULE.TopologyError):
                MODULE.validate_manifest(unknown, root, frozen_sha)

            malformed_frozen = deepcopy(manifest)
            del malformed_frozen["track"]
            with self.assertRaisesRegex(
                MODULE.TopologyError, "manifest schema validation failed"
            ):
                MODULE.validate_manifest(
                    manifest,
                    root,
                    frozen_sha,
                    "b" * 40,
                    malformed_frozen,
                    "0" * 64,
                    root / "frozen.json",
                    root,
                )

    def test_prepared_projection_accepts_version_identity_only(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            prepared_sha = "b" * 40
            prepared_release = "0.18.1"
            prepared_metadata = deepcopy(
                {
                    "metadata": {"publish": {"allow": ["fixture-crate"]}},
                    "workspace_members": ["fixture-id"],
                    "packages": [
                        {
                            "id": "fixture-id",
                            "name": "fixture-crate",
                            "version": prepared_release,
                            "manifest_path": str(root / "fixture/Cargo.toml"),
                            "publish": None,
                            "dependencies": [],
                        }
                    ],
                }
            )
            (root / "Cargo.toml").write_text(
                f"[workspace.package]\nversion = '{prepared_release}'\n\n"
                "[workspace.dependencies]\n"
                f"fixture-crate = {{ path = 'fixture', version = '{prepared_release}' }}\n",
                encoding="utf-8",
            )
            (root / "Cargo.lock").write_text(
                "[[package]]\nname = 'fixture-crate'\n"
                f"version = '{prepared_release}'\n\n"
                "[[package]]\nname = 'fixture-crate'\n"
                "version = '99.0.0'\nsource = 'registry+https://example.invalid'\n",
                encoding="utf-8",
            )
            (root / "fixture/Cargo.toml").write_text(
                "[package]\nname = 'fixture-crate'\n"
                f"version = '{prepared_release}'\n\n"
                "[dependencies]\n"
                f"fixture-crate = {{ path = '.', version = '{prepared_release}' }}\n\n"
                "[dev-dependencies]\n"
                f"fixture-crate = {{ path = '.', version = '{prepared_release}' }}\n",
                encoding="utf-8",
            )
            (root / "vscode-extension/package.json").write_text(
                json.dumps({"name": "perl-lsp-rs", "version": prepared_release}),
                encoding="utf-8",
            )
            def fake_head(path):
                return frozen_sha if Path(path).resolve() == frozen_root.resolve() else prepared_sha

            with patch.object(
                MODULE, "cargo_metadata", return_value=prepared_metadata
            ), patch.object(MODULE, "git_head", side_effect=fake_head):
                prepared = MODULE.build_manifest(
                    root,
                    prepared_release,
                    frozen_sha,
                    prepared_sha,
                    frozen,
                    frozen_digest,
                    frozen_topology_path,
                    frozen_root,
                )
                MODULE.validate_manifest(
                    prepared,
                    root,
                    frozen_sha,
                    prepared_sha,
                    frozen,
                    frozen_digest,
                    frozen_topology_path,
                    frozen_root,
                )
            self.assertEqual(prepared["release"], prepared_release)
            self.assertEqual(prepared["prepared_swarm_sha"], prepared_sha)
            self.assertEqual(
                prepared["vsix"]["asset_name"], "perl-lsp-rs-0.18.1.vsix"
            )

    def test_prepared_projection_rejects_identity_and_cross_sha_controls(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            prepared = deepcopy(frozen)
            prepared["prepared_swarm_sha"] = "b" * 40
            prepared["published_crates"][0]["name"] = "other-crate"
            with patch.object(MODULE, "git_head", return_value=frozen_sha):
                with self.assertRaises(MODULE.TopologyError):
                    MODULE.validate_prepared_projection(
                        frozen,
                        prepared,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                        root,
                    )

            with self.assertRaises(MODULE.TopologyError):
                MODULE.build_manifest(
                    Path("."), "0.18.1", frozen_sha, frozen_sha
                )

    def test_prepared_projection_rejects_subject_drift_table(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            mutations = {
                "publish order": lambda value: value["published_crates"][0].update(
                    publish_order=2
                ),
                "internal dependencies": lambda value: value["published_crates"][0].update(
                    internal_dependencies=["other-crate"]
                ),
                "target runner": lambda value: value["binary_targets"][0].update(
                    runner="windows-2022"
                ),
                "archive members": lambda value: value["binary_targets"][0].update(
                    required_members=["tampered"]
                ),
                "managed install": lambda value: value["vsix"].update(
                    managed_targets=["other-target"]
                ),
                "secondary channel": lambda value: value["secondary_channels"].update(
                    docker="deferred"
                ),
            }
            for name, mutate in mutations.items():
                prepared = deepcopy(frozen)
                prepared["prepared_swarm_sha"] = "b" * 40
                mutate(prepared)
                with self.subTest(name=name), patch.object(
                    MODULE, "git_head", return_value=frozen_sha
                ), self.assertRaisesRegex(
                    MODULE.TopologyError, "immutable frozen product subject"
                ):
                    MODULE.validate_prepared_projection(
                        frozen,
                        prepared,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                        root,
                    )

    def test_prepared_projection_rejects_unrelated_version_file_edits(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            (root / "Cargo.toml").write_text(
                "[workspace.package]\nversion = '0.18.0'\n\n[workspace.dependencies]\n"
                "fixture-crate = { path = 'fixture', version = '0.18.0' }\n"
                "unrelated = '9.9.9'\n",
                encoding="utf-8",
            )
            prepared = deepcopy(frozen)
            prepared["prepared_swarm_sha"] = "b" * 40
            prepared["sources"]["Cargo.toml"]["sha256"] = MODULE.sha256(
                root / "Cargo.toml"
            )
            with patch.object(MODULE, "git_head", return_value=frozen_sha):
                with self.assertRaisesRegex(MODULE.TopologyError, "outside version metadata"):
                    MODULE.validate_prepared_projection(
                        frozen,
                        prepared,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                        root,
                    )

    def test_prepared_projection_rejects_same_name_registry_lock_change(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            (root / "Cargo.lock").write_text(
                "[[package]]\nname = 'fixture-crate'\nversion = '0.18.0'\n\n"
                "[[package]]\nname = 'fixture-crate'\nversion = '100.0.0'\n"
                "source = 'registry+https://example.invalid'\n",
                encoding="utf-8",
            )
            prepared = deepcopy(frozen)
            prepared["prepared_swarm_sha"] = "b" * 40
            prepared["sources"]["Cargo.lock"]["sha256"] = MODULE.sha256(
                root / "Cargo.lock"
            )
            with patch.object(MODULE, "git_head", return_value=frozen_sha):
                with self.assertRaisesRegex(MODULE.TopologyError, "outside version metadata"):
                    MODULE.validate_prepared_projection(
                        frozen,
                        prepared,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                        root,
                    )

    def test_prepared_projection_rejects_package_feature_metadata_change(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            package_manifest = root / "fixture/Cargo.toml"
            package_manifest.write_text(
                package_manifest.read_text(encoding="utf-8")
                + "\n[features]\ndefault = []\n",
                encoding="utf-8",
            )
            prepared = deepcopy(frozen)
            prepared["prepared_swarm_sha"] = "b" * 40
            relative = "fixture/Cargo.toml"
            prepared["sources"][relative]["sha256"] = MODULE.sha256(package_manifest)
            with patch.object(MODULE, "git_head", return_value=frozen_sha):
                with self.assertRaisesRegex(
                    MODULE.TopologyError, "outside version metadata"
                ):
                    MODULE.validate_prepared_projection(
                        frozen,
                        prepared,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                        root,
                    )

    def test_frozen_authority_digest_mismatch_is_rejected(self):
        with TemporaryDirectory() as temporary:
            path = Path(temporary) / "frozen.json"
            path.write_text("{}\n", encoding="utf-8")
            with self.assertRaises(MODULE.TopologyError):
                MODULE.load_frozen_authority(path, "0" * 64)

    def test_prepared_checkout_sha_must_match_declared_prepared_identity(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            prepared = deepcopy(frozen)
            prepared["prepared_swarm_sha"] = "b" * 40
            with patch.object(MODULE, "git_head", return_value="c" * 40):
                with self.assertRaisesRegex(
                    MODULE.TopologyError, "exact checkout being validated"
                ):
                    MODULE.validate_manifest(
                        prepared,
                        root,
                        frozen_sha,
                        "b" * 40,
                        frozen,
                        frozen_digest,
                        frozen_topology_path,
                        frozen_root,
                    )

    def test_prepared_output_is_deterministic_for_identical_inputs(self):
        with self.valid_manifest_fixture() as (root, frozen, frozen_sha):
            frozen_root = root.parent / f"{root.name}-frozen"
            copytree(root, frozen_root)
            frozen_topology_path = root.parent / f"{root.name}-frozen-topology.json"
            frozen_topology_path.write_text(
                json.dumps(frozen, indent=2) + "\n", encoding="utf-8"
            )
            frozen_digest = MODULE.sha256(frozen_topology_path)
            prepared_sha = "b" * 40
            def fake_head(path):
                return frozen_sha if Path(path).resolve() == frozen_root.resolve() else prepared_sha

            with patch.object(MODULE, "cargo_metadata") as metadata, patch.object(
                MODULE, "git_head", side_effect=fake_head
            ):
                metadata.return_value = {
                    "metadata": {"publish": {"allow": ["fixture-crate"]}},
                    "workspace_members": ["fixture-id"],
                    "packages": [
                        {
                            "id": "fixture-id",
                            "name": "fixture-crate",
                            "version": "0.18.0",
                            "manifest_path": str(root / "fixture/Cargo.toml"),
                            "publish": None,
                            "dependencies": [],
                        }
                    ],
                }
                first = MODULE.build_manifest(
                    root,
                    "0.18.0",
                    frozen_sha,
                    prepared_sha,
                    frozen,
                    frozen_digest,
                    frozen_topology_path,
                    frozen_root,
                )
                second = MODULE.build_manifest(
                    root,
                    "0.18.0",
                    frozen_sha,
                    prepared_sha,
                    frozen,
                    frozen_digest,
                    frozen_topology_path,
                    frozen_root,
                )
            self.assertEqual(first, second)

    def test_cli_real_frozen_and_prepared_checkouts_are_deterministic(self):
        with TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            frozen_root = workspace / "frozen"
            files = {
                "Cargo.toml": "[workspace.package]\nversion = '0.18.0'\n\n"
                "[workspace.dependencies]\n"
                "fixture-crate = { path = 'fixture', version = '0.18.0' }\n",
                "Cargo.lock": "[[package]]\nname = 'fixture-crate'\n"
                "version = '0.18.0'\n\n[[package]]\n"
                "name = 'fixture-crate'\nversion = '99.0.0'\n"
                "source = 'registry+https://example.invalid'\n",
                "fixture/Cargo.toml": "[package]\nname = 'fixture-crate'\n"
                "version = '0.18.0'\n",
                ".github/workflows/release.yml": "matrix:\n  include:\n"
                "    - target: x86_64-unknown-linux-gnu\n"
                "      os: ubuntu-22.04\nsteps:\n",
                "vscode-extension/package.json": json.dumps(
                    {"name": "perl-lsp-rs", "version": "0.18.0"}
                ),
                "docs/reference/downstream-dap-integrations.json": json.dumps(
                    {"targets": [{"triple": "x86_64-unknown-linux-gnu"}]}
                ),
                "vscode-extension/src/downloader.ts": "return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';\n"
                "archPrefix = arch === 'arm64' ? 'aarch64' : 'x86_64';\n"
                "return `${archPrefix}-unknown-linux-${libc}`;\nvalue === 'gnu';\nvalue === 'musl';\n"
                "return 'x86_64-pc-windows-msvc';\nreturn 'aarch64-pc-windows-msvc';\n",
                "scripts/inject-sha-assets.sh": "#!/bin/sh\n",
            }
            for relative, contents in files.items():
                path = frozen_root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents, encoding="utf-8")

            def git(*args, cwd):
                return subprocess.run(
                    ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
                ).stdout.strip()

            git("init", "-q", cwd=frozen_root)
            git("config", "user.email", "test@example.invalid", cwd=frozen_root)
            git("config", "user.name", "Topology Test", cwd=frozen_root)
            git("add", ".", cwd=frozen_root)
            git("commit", "-qm", "frozen", cwd=frozen_root)
            frozen_sha = git("rev-parse", "HEAD", cwd=frozen_root)
            prepared_root = workspace / "prepared"
            git("clone", "-q", str(frozen_root), str(prepared_root), cwd=workspace)
            git("config", "user.email", "test@example.invalid", cwd=prepared_root)
            git("config", "user.name", "Topology Test", cwd=prepared_root)
            for relative in (
                "Cargo.toml",
                "fixture/Cargo.toml",
                "vscode-extension/package.json",
            ):
                path = prepared_root / relative
                path.write_text(
                    path.read_text(encoding="utf-8").replace("0.18.0", "0.18.1"),
                    encoding="utf-8",
                )
            lock = prepared_root / "Cargo.lock"
            lock.write_text(
                lock.read_text(encoding="utf-8").replace(
                    "version = '0.18.0'", "version = '0.18.1'", 1
                ),
                encoding="utf-8",
            )
            git("add", ".", cwd=prepared_root)
            git("commit", "-qm", "prepared", cwd=prepared_root)
            prepared_sha = git("rev-parse", "HEAD", cwd=prepared_root)

            def controlled_metadata(root):
                root = Path(root).resolve()
                version = "0.18.1" if root == prepared_root.resolve() else "0.18.0"
                return {
                    "metadata": {"publish": {"allow": ["fixture-crate"]}},
                    "workspace_members": ["fixture-id"],
                    "packages": [
                        {
                            "id": "fixture-id",
                            "name": "fixture-crate",
                            "version": version,
                            "manifest_path": str(root / "fixture/Cargo.toml"),
                            "publish": None,
                            "dependencies": [],
                        }
                    ],
                }

            def run_cli(arguments):
                stdout = io.StringIO()
                stderr = io.StringIO()
                with patch.object(
                    MODULE, "cargo_metadata", side_effect=controlled_metadata
                ), patch.object(
                    sys, "argv", [sys.executable, *arguments]
                ), redirect_stdout(stdout), redirect_stderr(stderr):
                    try:
                        returncode = MODULE.main()
                    except SystemExit as error:
                        returncode = int(error.code)
                return returncode, stdout.getvalue(), stderr.getvalue()

            frozen_output = frozen_root / "release_topology.frozen.v1.json"
            frozen_run = run_cli(
                [
                    "--root",
                    str(frozen_root),
                    "--release",
                    "0.18.0",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--output",
                    str(frozen_output),
                ]
            )
            self.assertEqual(frozen_run[0], 0, frozen_run[2])
            self.assertIn("release-topology: PASS", frozen_run[1])
            prepared_authority = prepared_root / "release_topology.frozen.v1.json"
            prepared_authority.write_bytes(frozen_output.read_bytes())
            digest = MODULE.sha256(frozen_output)
            outputs = []
            for index in (1, 2):
                output = prepared_root / f"release_topology.{index}.json"
                run = run_cli(
                    [
                        "--root",
                        str(prepared_root),
                        "--release",
                        "0.18.1",
                        "--frozen-product-sha",
                        frozen_sha,
                        "--prepared-swarm-sha",
                        prepared_sha,
                        "--frozen-root",
                        str(frozen_root),
                        "--frozen-topology",
                        str(prepared_authority),
                        "--frozen-topology-sha256",
                        digest,
                        "--output",
                        str(output),
                    ]
                )
                self.assertEqual(run[0], 0, run[2])
                self.assertIn("release-topology: PASS", run[1])
                outputs.append(output.read_bytes())
            self.assertEqual(outputs[0], outputs[1])

            dirty_input = prepared_root / "scripts/inject-sha-assets.sh"
            clean_input = dirty_input.read_text(encoding="utf-8")
            dirty_input.write_text(clean_input + "# dirty topology input\n", encoding="utf-8")
            dirty_run = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    prepared_sha,
                    "--frozen-root",
                    str(frozen_root),
                    "--frozen-topology",
                    str(prepared_authority),
                    "--frozen-topology-sha256",
                    digest,
                    "--output",
                    str(prepared_root / "dirty.json"),
                ]
            )
            self.assertNotEqual(dirty_run[0], 0)
            self.assertIn("topology input has unstaged changes", dirty_run[2])
            dirty_input.write_text(clean_input, encoding="utf-8")

            wrong_root = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    prepared_sha,
                    "--frozen-root",
                    str(prepared_root),
                    "--frozen-topology",
                    str(prepared_authority),
                    "--frozen-topology-sha256",
                    digest,
                    "--output",
                    str(prepared_root / "release_topology.1.json"),
                ]
            )
            self.assertNotEqual(wrong_root[0], 0)
            self.assertIn("supplied frozen checkout", wrong_root[2])

            wrong_prepared = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    "c" * 40,
                    "--frozen-root",
                    str(frozen_root),
                    "--frozen-topology",
                    str(prepared_authority),
                    "--frozen-topology-sha256",
                    digest,
                    "--output",
                    str(prepared_root / "release_topology.1.json"),
                ]
            )
            self.assertNotEqual(wrong_prepared[0], 0)
            self.assertIn("differs from the reviewed prepared SHA", wrong_prepared[2])

            frozen_bytes = prepared_authority.read_bytes()
            prepared_authority.write_bytes(outputs[0])
            replaced_authority = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    prepared_sha,
                    "--frozen-root",
                    str(frozen_root),
                    "--frozen-topology",
                    str(prepared_authority),
                    "--frozen-topology-sha256",
                    MODULE.sha256(prepared_authority),
                    "--output",
                    str(prepared_root / "release_topology.1.json"),
                ]
            )
            prepared_authority.write_bytes(frozen_bytes)
            self.assertNotEqual(replaced_authority[0], 0)
            self.assertIn("must not already bind prepared_swarm_sha", replaced_authority[2])

            invalid = json.loads(outputs[0])
            del invalid["track"]
            invalid_path = prepared_root / "invalid.json"
            invalid_path.write_text(json.dumps(invalid), encoding="utf-8")
            check = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "9.9.9",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--output",
                    str(invalid_path),
                ]
            )
            self.assertNotEqual(check[0], 0)
            self.assertIn("schema validation failed", check[2])
            self.assertNotIn("manifest release differs", check[2])

            malformed_frozen = deepcopy(json.loads(frozen_bytes))
            del malformed_frozen["track"]
            malformed_frozen_path = prepared_root / "malformed-frozen.json"
            malformed_frozen_path.write_text(
                json.dumps(malformed_frozen), encoding="utf-8"
            )
            malformed_frozen_run = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "9.9.9",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    prepared_sha,
                    "--frozen-root",
                    str(frozen_root),
                    "--frozen-topology",
                    str(malformed_frozen_path),
                    "--frozen-topology-sha256",
                    MODULE.sha256(malformed_frozen_path),
                    "--output",
                    str(prepared_root / "release_topology.1.json"),
                ]
            )
            self.assertNotEqual(malformed_frozen_run[0], 0)
            self.assertIn("schema validation failed", malformed_frozen_run[2])
            self.assertNotIn("manifest release differs", malformed_frozen_run[2])

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

            expected_errors = {
                "missing crate": "published_crates",
                "extra crate": "published_crates",
                "missing target": "binary_targets",
                "extra target": "binary_targets",
                "archive member": "binary_targets",
                "managed target mapping": "VSIX managed targets",
                "VSIX version": "VSIX version",
                "source hash": "source hash is stale",
                "missing source": "source hash set",
            }
            for name, candidate in mutations.items():
                with self.subTest(name=name), self.assertRaisesRegex(
                    MODULE.TopologyError, expected_errors[name]
                ):
                    MODULE.validate_manifest(candidate, root, frozen_sha)


if __name__ == "__main__":
    unittest.main()
