#!/usr/bin/env python3
import importlib.util
import hashlib
import io
import json
import os
import re
import shlex
import subprocess
import sys
import tomllib
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from copy import deepcopy
from tempfile import TemporaryDirectory
import unittest
from pathlib import Path
from shutil import copytree, which
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("generate_release_topology.py")
sys.path.insert(0, str(MODULE_PATH.parent))
SPEC = importlib.util.spec_from_file_location("release_topology", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseTopologyTests(unittest.TestCase):
    @contextmanager
    def valid_manifest_fixture(self, schema_version=1):
        release = "0.18.0"
        frozen_sha = "a" * 40
        workflow = """
        matrix:
          include:
            - target: x86_64-unknown-linux-gnu
              os: ubuntu-22.04
        steps:
        """
        if schema_version == 2:
            actual = (MODULE_PATH.parents[1] / ".github/workflows/release.yml").read_text(
                encoding="utf-8"
            )
            candidate_start = actual.index("  candidate:\n")
            candidate_end = actual.index("\n  publisher-eligibility:", candidate_start)
            workflow = "jobs:\n  build:\n" + workflow.rstrip() + "\n" + actual[candidate_start:candidate_end]
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
            schema_relative = MODULE.schema_relative_path(schema_version)
            schema_path = root / schema_relative
            schema_path.parent.mkdir(parents=True, exist_ok=True)
            schema_path.write_text(
                (MODULE_PATH.parents[1] / schema_relative).read_text(encoding="utf-8"), encoding="utf-8"
            )

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
                "schema": schema_version,
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
            if schema_version == 2:
                manifest["checksum_assets"] = [{
                    "asset_name": "SHA256SUMS", "algorithm": "sha256",
                    "channel": "github_release",
                    "archive_targets": ["x86_64-unknown-linux-gnu"],
                }]
            for relative in MODULE.source_paths(crates, schema_version=schema_version):
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

    def test_v2_schema_accepts_public_checksum_inventory_without_upgrading_v1(self):
        with self.valid_manifest_fixture() as (_, manifest, _):
            MODULE.schema_validate(manifest)
            manifest["schema"] = 2
            manifest["checksum_assets"] = [{
                "asset_name": "SHA256SUMS",
                "algorithm": "sha256",
                "channel": "github_release",
                "archive_targets": ["x86_64-unknown-linux-gnu"],
            }]
            MODULE.schema_validate(manifest)
            manifest["schema"] = 1
            with self.assertRaises(MODULE.TopologyError):
                MODULE.schema_validate(manifest)

    def test_raw_schema_admission_at_both_manifest_file_boundaries(self):
        cases = [
            ('"schema":1.0', True), ('"schema":2.00', True),
            ('"schema":2e0', True), ('"schema":20e-1', True),
            ('"schema":0.2e1', True), ('"schema":200.0e-2', True),
            ('"schema":2.00000000000000000000', True),
            ('"schema":2e9999999999999999999999999', False),
            ('"schema":1.0000000000000001', False),
            ('"schema":0.99999999999999999', False),
            ('"schema":2.0000000000000001', False),
            ('"schema":1.99999999999999999', False),
            ('"schema":true', False), ('"schema":"2"', False),
            ('"schema":3', False), ('"schema":1,"schema":2', False),
            ('"schema":1,"sche\\u006da":2', False),
            ('"decoy":{"schema":3},"sche\\u006da":2e0', True),
            ('"schema":2,"unrelated":1e9999999999999999999999999', True),
        ]
        with TemporaryDirectory() as directory:
            path = Path(directory) / "topology.json"
            for fields, accepted in cases:
                raw = ("{" + fields + "}").encode()
                path.write_bytes(raw)
                digest = hashlib.sha256(raw).hexdigest()
                for load in (lambda: MODULE.load_manifest(path), lambda: MODULE.load_frozen_authority(path, digest)[0]):
                    with self.subTest(fields=fields, load=load):
                        if accepted:
                            self.assertEqual(load(), json.loads(raw))
                            self.assertEqual(path.read_bytes(), raw)
                        else:
                            with self.assertRaisesRegex(MODULE.TopologyError, "schema"):
                                load()

    def test_manifest_reader_preserves_utf8_only_input(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "topology.json"
            path.write_bytes('{"schema":2}'.encode("utf-16"))
            with self.assertRaises(MODULE.TopologyError):
                MODULE.load_manifest(path)

    def test_schema_version_rejects_nonintegral_or_unsupported_identities(self):
        with self.valid_manifest_fixture() as (_, manifest, _):
            manifest["schema"] = 1.0
            MODULE.schema_validate(manifest)
            for version in (True, False, "1", "2", 1.5, 2.5, 3, None):
                with self.subTest(version=repr(version)):
                    manifest["schema"] = version
                    with self.assertRaises(MODULE.TopologyError):
                        MODULE.schema_validate(manifest)

    def test_v2_generation_and_validation_bind_exact_checksum_membership(self):
        with self.valid_manifest_fixture(schema_version=2) as (root, manifest, frozen_sha):
            generated = MODULE.build_manifest(root, "0.18.0", frozen_sha, schema_version=2)
            self.assertEqual(generated, manifest)
            MODULE.validate_manifest(generated, root, frozen_sha)
            mutations = {
                "missing assets": lambda value: value.pop("checksum_assets"),
                "extra asset": lambda value: value["checksum_assets"].append(deepcopy(value["checksum_assets"][0])),
                "archive-local name": lambda value: value["checksum_assets"][0].update(asset_name="SHA256SUMS.txt"),
                "wrong hash": lambda value: value["checksum_assets"][0].update(algorithm="sha512"),
                "wrong channel": lambda value: value["checksum_assets"][0].update(channel="docker"),
                "omitted target": lambda value: value["checksum_assets"][0].update(archive_targets=[]),
                "duplicate target": lambda value: value["checksum_assets"][0]["archive_targets"].append("x86_64-unknown-linux-gnu"),
                "foreign target": lambda value: value["checksum_assets"][0]["archive_targets"].append("aarch64-unknown-linux-gnu"),
                "non-archive subject": lambda value: value["checksum_assets"][0]["archive_targets"].append("sbom-spdx.json"),
                "stale workflow": lambda value: value["sources"][".github/workflows/release.yml"].update(sha256="0" * 64),
                "stale schema": lambda value: value["sources"]["schemas/release_topology.v2.schema.json"].update(sha256="0" * 64),
            }
            for name, mutate in mutations.items():
                with self.subTest(name=name):
                    candidate = deepcopy(manifest)
                    mutate(candidate)
                    with self.assertRaises(MODULE.TopologyError):
                        MODULE.validate_manifest(candidate, root, frozen_sha)
            workflow_path = root / ".github/workflows/release.yml"
            workflow = workflow_path.read_text(encoding="utf-8")
            for candidate in (
                workflow.replace(" -o -name '*.zip'", ""),
                workflow.replace("xargs -0 sha256sum", "xargs -0 sha512sum"),
                workflow.replace("cd candidate/dist", "cd artifacts"),
            ):
                workflow_path.write_text(candidate, encoding="utf-8")
                current = deepcopy(manifest)
                current["sources"][".github/workflows/release.yml"]["sha256"] = MODULE.sha256(workflow_path)
                with self.assertRaisesRegex(MODULE.TopologyError, "checksum producer"):
                    MODULE.validate_manifest(current, root, frozen_sha)
                with self.assertRaisesRegex(MODULE.TopologyError, "checksum producer"):
                    MODULE.build_manifest(root, "0.18.0", frozen_sha, schema_version=2)
            workflow_path.write_text(workflow, encoding="utf-8")

    def test_v2_producer_shape_rejects_changed_commands_and_removed_producer(self):
        workflow = (MODULE_PATH.parents[1] / ".github/workflows/release.yml").read_text(encoding="utf-8")
        MODULE.consolidated_checksum_producer(workflow)
        mutations = {
            "removed producer": workflow.replace("- name: Generate consolidated SHA256SUMS", "- name: Removed checksum producer"),
            "duplicate producer": workflow + workflow,
            "omitted zip": workflow.replace(" -o -name '*.zip'", ""),
            "hash algorithm": workflow.replace("xargs -0 sha256sum", "xargs -0 sha512sum"),
            "output name": workflow.replace("> SHA256SUMS", "> SHA256SUMS.txt"),
            "output directory": workflow.replace("cd candidate/dist", "cd artifacts"),
            "removed duplicate guard": workflow.replace("            ARCHIVE_NAMES[$name]=1\n", ""),
            "extra command": workflow.replace("          cat SHA256SUMS", "          cat SHA256SUMS\n          rm SHA256SUMS"),
        }
        for name, candidate in mutations.items():
            with self.subTest(name=name):
                self.assertNotEqual(candidate, workflow)
                with self.assertRaises(MODULE.TopologyError):
                    MODULE.consolidated_checksum_producer(candidate)

    def test_v2_producer_rejects_wrong_execution_context_with_current_source_hash(self):
        with self.valid_manifest_fixture(schema_version=2) as (fixture_root, _, _):
            workflow = (fixture_root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        MODULE.consolidated_checksum_producer(workflow)
        start = workflow.index("      - name: Generate consolidated SHA256SUMS")
        end = workflow.index("      - name:", start + 1)
        producer = workflow[start:end]
        download_start = workflow.index("      - name: Download release archive artifacts")
        download_end = workflow.index("      - name:", download_start + 1)
        download = workflow[download_start:download_end]
        mutations = {
            "disabled decoy": workflow[:start] + workflow[end:] + "\n  checksum-decoy:\n    if: false\n    steps:\n" + producer,
            "candidate disabled": workflow.replace("  candidate:\n", "  candidate:\n    if: false\n"),
            "candidate container": workflow.replace("  candidate:\n", "  candidate:\n    container: alpine\n"),
            "candidate runner": workflow.replace("  candidate:\n    name: Build terminal release candidate\n    needs: [build, release-metadata]\n    runs-on: ubuntu-24.04", "  candidate:\n    name: Build terminal release candidate\n    needs: [build, release-metadata]\n    runs-on: windows-latest"),
            "candidate dependencies": workflow.replace("needs: [build, release-metadata]", "needs: [release-metadata]"),
            "missing download": workflow.replace(download, ""),
            "late download": workflow.replace(download, "").replace(producer, producer + download),
            "foreign archive pattern": workflow.replace("pattern: perllsp-*", "pattern: other-*"),
            "foreign archive directory": workflow.replace("path: artifacts", "path: other-artifacts"),
            "foreign download run": workflow.replace(download, download.replace("        with:", "        run: echo foreign\n        with:")),
            "workflow run defaults": "defaults:\n  run:\n    working-directory: foreign\n" + workflow,
            "candidate run defaults": workflow.replace("  candidate:\n", "  candidate:\n    defaults:\n      run:\n        shell: pwsh\n"),
            "global path": "env:\n  PATH: /unsupported-tools\n" + workflow,
            "global bash env": "env:\n  BASH_ENV: foreign.sh\n" + workflow,
            "duplicate global env": "env:\n  CARGO_TERM_COLOR: always\n  RUST_BACKTRACE: 1\nenv:\n  PATH: /unsupported-tools\n" + workflow,
            "inline global env": "env: {PATH: /unsupported-tools}\n" + workflow,
            "commented jobs": workflow.replace("jobs:\n", "jobs: # unsupported layout\n"),
            "inline jobs": workflow.replace("jobs:\n", "jobs: {}\n"),
            "archive deletion in listing": workflow.replace("run: ls -R artifacts", "run: rm -rf artifacts"),
            "extra intervening step": workflow.replace(producer, "      - name: Alter archive inputs\n        run: rm -rf artifacts\n\n" + producer),
            "missing listing": workflow.replace("      - name: Display structure of downloaded files\n        run: ls -R artifacts\n", ""),
            "changed checkout": workflow.replace("persist-credentials: false", "persist-credentials: true"),
            "changed toolchain": workflow.replace("toolchain: stable", "toolchain: nightly"),
            "changed identity download": workflow.replace("pattern: release-build-identity-*", "pattern: other-identity-*"),
            "reordered prelude": workflow.replace(download, "").replace("      - name: Checkout", download + "      - name: Checkout", 1),
        }
        with self.valid_manifest_fixture(schema_version=2) as (root, manifest, frozen_sha):
            workflow_path = root / ".github/workflows/release.yml"
            for name, candidate in mutations.items():
                with self.subTest(name=name):
                    self.assertNotEqual(candidate, workflow)
                    with self.assertRaises(MODULE.TopologyError):
                        MODULE.consolidated_checksum_producer(candidate)
                    workflow_path.write_text(candidate, encoding="utf-8")
                    current = deepcopy(manifest)
                    current["sources"][".github/workflows/release.yml"]["sha256"] = MODULE.sha256(workflow_path)
                    with self.assertRaisesRegex(MODULE.TopologyError, "checksum producer"):
                        MODULE.validate_manifest(current, root, frozen_sha)
                    with self.assertRaisesRegex(MODULE.TopologyError, "checksum producer"):
                        MODULE.build_manifest(root, "0.18.0", frozen_sha, schema_version=2)

    def test_actual_checksum_producer_executes_both_archive_formats_and_negative_controls(self):
        workflow = (MODULE_PATH.parents[1] / ".github/workflows/release.yml").read_text(encoding="utf-8")
        body = MODULE.consolidated_checksum_producer(workflow)
        targets = MODULE.derive_targets(workflow, "0.18.0")
        self.assertTrue(any(target["archive_name"].endswith(".zip") for target in targets))
        self.assertTrue(any(target["archive_name"].endswith(".tar.gz") for target in targets))
        bash = which("bash")
        self.assertIsNotNone(bash, "Linux Bash (or WSL Bash) is required for the producer oracle")
        platform = subprocess.run([bash, "--noprofile", "--norc", "-c", "uname -s"],
                                  capture_output=True, text=True, timeout=30)
        self.assertEqual(platform.returncode, 0, platform.stderr)
        self.assertEqual(platform.stdout.strip(), "Linux", "The production checksum oracle requires Linux; Git Bash uses a different sha256sum default")

        def execute(script, duplicate=False):
            with TemporaryDirectory() as temporary:
                root = Path(temporary)
                expected = {}
                for index, target in enumerate(targets):
                    name = target["archive_name"]
                    archive = root / "artifacts" / str(index) / name
                    archive.parent.mkdir(parents=True)
                    payload = f"archive fixture for {target['target']}\n".encode()
                    archive.write_bytes(payload)
                    expected[name] = hashlib.sha256(payload).hexdigest()
                for name in ("extension.vsix", "sbom-spdx.json", "SHA256SUMS.txt"):
                    (root / "artifacts" / name).write_text("not an archive", encoding="utf-8")
                if duplicate:
                    extra = root / "artifacts" / "duplicate"
                    extra.mkdir()
                    (extra / targets[0]["archive_name"]).write_bytes(b"duplicate")
                directory = shlex.quote(str(root))
                enter_fixture = (f'cd -- "$(wslpath -u {directory})"' if os.name == "nt"
                                 else f"cd -- {directory}")
                # Keep the WSL launcher's cwd outside the disposable fixture;
                # its host-side directory handle may outlive the Linux shell.
                run = subprocess.run([bash, "--noprofile", "--norc", "-s"], cwd=MODULE_PATH.parent,
                                     input=(enter_fixture + "\n" + script).encode("utf-8"),
                                     capture_output=True, timeout=30)
                run.stdout = run.stdout.decode("utf-8")
                run.stderr = run.stderr.decode("utf-8")
                sums = root / "candidate/dist/SHA256SUMS"
                expected_text = "".join(f"{expected[name]}  {name}\n" for name in sorted(expected))
                observed = sums.read_text(encoding="utf-8") if sums.is_file() else None
                return run, observed, expected_text

        run, observed, expected = execute(body)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(observed, expected)
        for mutation in (
            body.replace(" -o -name '*.zip'", ""),
            body.replace("xargs -0 sha256sum", "xargs -0 sha512sum"),
            body.replace("SHA256SUMS", "SHA256SUMS.txt"),
        ):
            run, observed, expected = execute(mutation)
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertNotEqual(observed, expected)
        run, _, _ = execute(body, duplicate=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("Duplicate archive filename", run.stdout)

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

    def test_git_input_check_preserves_fatal_repository_error(self):
        with TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(
                MODULE.TopologyError, "cannot inspect tracked topology inputs:.*not a git repository"
            ):
                MODULE.ensure_committed_topology_inputs(
                    Path(temporary), ["Cargo.toml"]
                )

    def test_malformed_workspace_dependencies_are_rejected_as_typed_errors(self):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text(
                "[workspace]\ndependencies = 'not-a-table'\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(
                MODULE.TopologyError, "workspace.dependencies must be a table"
            ):
                MODULE.normalized_source_value(
                    root, "Cargo.toml", "0.18.0", set(), set(), set()
                )

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

            with patch.object(MODULE, "git_head", return_value="b" * 40):
                with self.assertRaisesRegex(
                    MODULE.TopologyError, "exact checkout being validated"
                ):
                    MODULE.validate_manifest(manifest, root)

            same_frozen_prepared = deepcopy(manifest)
            same_frozen_prepared["prepared_swarm_sha"] = frozen_sha
            with patch.object(MODULE, "git_head", return_value=frozen_sha):
                with self.assertRaisesRegex(
                    MODULE.TopologyError, "must differ from frozen_product_sha"
                ):
                    MODULE.validate_manifest(same_frozen_prepared, root)

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

    def test_real_repository_workspace_inherited_lock_versions_are_allowed(self):
        repository_root = MODULE_PATH.parents[1]
        workspace = tomllib.loads(
            (repository_root / "Cargo.toml").read_text(encoding="utf-8")
        )["workspace"]
        member_paths = []
        for member in workspace["members"]:
            candidate = repository_root / member
            matches = (
                sorted(candidate.parent.glob(candidate.name))
                if "*" in member
                else [candidate]
            )
            member_paths.extend(
                (path / "Cargo.toml").relative_to(repository_root).as_posix()
                for path in matches
            )
        inherited = MODULE.workspace_inherited_package_names(
            repository_root, member_paths
        )
        published = set(workspace["metadata"]["publish"]["allow"])
        permitted = published | set(inherited)
        private_inherited = set(inherited) - published
        explicit_private = set()
        for relative in member_paths:
            package = tomllib.loads(
                (repository_root / relative).read_text(encoding="utf-8")
            )["package"]
            if (
                package["name"] not in published
                and package.get("version") != {"workspace": True}
            ):
                explicit_private.add(package["name"])
        self.assertTrue(private_inherited)
        self.assertIn("xtask", private_inherited)
        self.assertTrue(explicit_private)
        self.assertTrue(private_inherited.isdisjoint(explicit_private))

        frozen_release = workspace["package"]["version"]
        version_match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", frozen_release)
        self.assertIsNotNone(version_match)
        major, minor, patch = version_match.groups()
        prepared_release = f"{major}.{minor}.{int(patch) + 1}"

        frozen_lock = repository_root / "Cargo.lock"
        raw = frozen_lock.read_text(encoding="utf-8")
        blocks = re.split(r"(?=^\[\[package\]\])", raw, flags=re.MULTILINE)
        changed = 0
        prepared_blocks = []
        for block in blocks:
            parsed = tomllib.loads(block) if "[[package]]" in block else {}
            package = parsed.get("package", [{}])[0]
            if (
                package.get("name") in permitted
                and package.get("source") is None
                and package.get("version") == frozen_release
            ):
                block = block.replace(
                    f'version = "{frozen_release}"',
                    f'version = "{prepared_release}"',
                    1,
                )
                changed += 1
            prepared_blocks.append(block)
        self.assertEqual(changed, len(permitted))
        with TemporaryDirectory() as temporary:
            prepared_root = Path(temporary)
            (prepared_root / "Cargo.lock").write_text(
                "".join(prepared_blocks), encoding="utf-8"
            )
            self.assertEqual(
                MODULE.normalized_source_value(
                    repository_root,
                    "Cargo.lock",
                    frozen_release,
                    published,
                    set(),
                    set(inherited),
                ),
                MODULE.normalized_source_value(
                    prepared_root,
                    "Cargo.lock",
                    prepared_release,
                    published,
                    set(),
                    set(inherited),
                ),
            )

    def test_lock_normalization_rejects_registry_and_private_explicit_deltas(self):
        def write_lock(root, version, source=None, name="private-inherited"):
            source_line = f"source = '{source}'\n" if source else ""
            (root / "Cargo.lock").write_text(
                f"[[package]]\nname = '{name}'\nversion = '{version}'\n{source_line}",
                encoding="utf-8",
            )

        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            prepared = root / "prepared"
            prepared.mkdir()
            write_lock(root, "0.17.0", "registry+https://example.invalid")
            write_lock(
                prepared, "0.18.0", "registry+https://example.invalid"
            )
            self.assertNotEqual(
                MODULE.normalized_source_value(
                    root,
                    "Cargo.lock",
                    "0.17.0",
                    set(),
                    set(),
                    {"private-inherited"},
                ),
                MODULE.normalized_source_value(
                    prepared,
                    "Cargo.lock",
                    "0.18.0",
                    set(),
                    set(),
                    {"private-inherited"},
                ),
            )

            write_lock(root, "0.17.0", name="private-explicit")
            write_lock(prepared, "0.18.0", name="private-explicit")
            self.assertNotEqual(
                MODULE.normalized_source_value(
                    root,
                    "Cargo.lock",
                    "0.17.0",
                    set(),
                    set(),
                    {"private-inherited"},
                ),
                MODULE.normalized_source_value(
                    prepared,
                    "Cargo.lock",
                    "0.18.0",
                    set(),
                    set(),
                    {"private-inherited"},
                ),
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
        self.cli_real_frozen_and_prepared_checkouts_are_deterministic(schema_version=1)

    def test_cli_v2_real_frozen_and_prepared_checkouts_are_deterministic(self):
        self.cli_real_frozen_and_prepared_checkouts_are_deterministic(schema_version=2)

    def cli_real_frozen_and_prepared_checkouts_are_deterministic(self, schema_version):
        schema_relative = MODULE.schema_relative_path(schema_version)
        with TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            frozen_root = workspace / "frozen"
            files = {
                "Cargo.toml": "[workspace.package]\nversion = '0.18.0'\n\n"
                "[workspace.dependencies]\n"
                "fixture-crate = { path = 'fixture', version = '0.18.0' }\n"
                "private-inherited = { path = 'private-inherited', version = '0.18.0' }\n"
                "private-explicit = { path = 'private-explicit', version = '0.18.0' }\n",
                "Cargo.lock": "[[package]]\nname = 'fixture-crate'\n"
                "version = '0.18.0'\n\n[[package]]\n"
                "name = 'private-inherited'\nversion = '0.18.0'\n\n"
                "[[package]]\nname = 'private-explicit'\nversion = '0.18.0'\n\n"
                "[[package]]\nname = 'fixture-crate'\nversion = '99.0.0'\n"
                "source = 'registry+https://example.invalid'\n",
                "private-inherited/Cargo.toml": "[package]\n"
                "name = 'private-inherited'\nversion.workspace = true\n\n"
                "[dependencies]\nfixture-crate = { path = '../fixture', version = '0.18.0' }\n",
                "private-explicit/Cargo.toml": "[package]\n"
                "name = 'private-explicit'\nversion = '0.18.0'\n\n"
                "[dependencies]\nfixture-crate = { path = '../fixture', version = '0.18.0' }\n",
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
            actual_workflow = (MODULE_PATH.parents[1] / ".github/workflows/release.yml").read_text(encoding="utf-8")
            candidate_start = actual_workflow.index("  candidate:\n")
            candidate_end = actual_workflow.index("\n  publisher-eligibility:", candidate_start)
            files[".github/workflows/release.yml"] = (
                "jobs:\n  build:\n"
                + "".join("    " + line + "\n" for line in files[".github/workflows/release.yml"].splitlines())
                + actual_workflow[candidate_start:candidate_end]
            )
            for relative, contents in files.items():
                path = frozen_root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents, encoding="utf-8")
            for version in (1, 2):
                relative = MODULE.schema_relative_path(version)
                schema_path = frozen_root / relative
                schema_path.parent.mkdir(parents=True, exist_ok=True)
                schema_path.write_text(
                    (MODULE_PATH.parents[1] / relative).read_text(encoding="utf-8"), encoding="utf-8"
                )

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
            root_manifest = prepared_root / "Cargo.toml"
            root_text = root_manifest.read_text(encoding="utf-8")
            root_text = root_text.replace(
                "[workspace.package]\nversion = '0.18.0'",
                "[workspace.package]\nversion = '0.18.1'",
            )
            root_text = root_text.replace(
                "fixture-crate = { path = 'fixture', version = '0.18.0' }",
                "fixture-crate = { path = 'fixture', version = '0.18.1' }",
            )
            root_text = root_text.replace(
                "private-inherited = { path = 'private-inherited', version = '0.18.0' }",
                "private-inherited = { path = 'private-inherited', version = '0.18.1' }",
            )
            root_manifest.write_text(root_text, encoding="utf-8")
            for relative in (
                "fixture/Cargo.toml",
                "private-inherited/Cargo.toml",
                "vscode-extension/package.json",
            ):
                path = prepared_root / relative
                path.write_text(
                    path.read_text(encoding="utf-8").replace("0.18.0", "0.18.1"),
                    encoding="utf-8",
                )
            private_explicit = prepared_root / "private-explicit/Cargo.toml"
            private_explicit.write_text(
                private_explicit.read_text(encoding="utf-8").replace(
                    "fixture-crate = { path = '../fixture', version = '0.18.0' }",
                    "fixture-crate = { path = '../fixture', version = '0.18.1' }",
                ),
                encoding="utf-8",
            )
            lock = prepared_root / "Cargo.lock"
            lock.write_text(
                lock.read_text(encoding="utf-8")
                .replace(
                    "name = 'fixture-crate'\nversion = '0.18.0'",
                    "name = 'fixture-crate'\nversion = '0.18.1'",
                )
                .replace(
                    "name = 'private-inherited'\nversion = '0.18.0'",
                    "name = 'private-inherited'\nversion = '0.18.1'",
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
                    "workspace_members": [
                        "fixture-id",
                        "private-inherited-id",
                        "private-explicit-id",
                    ],
                    "packages": [
                        {
                            "id": "fixture-id",
                            "name": "fixture-crate",
                            "version": version,
                            "manifest_path": str(root / "fixture/Cargo.toml"),
                            "publish": None,
                            "dependencies": [],
                        },
                        {
                            "id": "private-inherited-id",
                            "name": "private-inherited",
                            "version": version,
                            "manifest_path": str(root / "private-inherited/Cargo.toml"),
                            "publish": False,
                            "dependencies": [],
                        },
                        {
                            "id": "private-explicit-id",
                            "name": "private-explicit",
                            "version": "0.18.0",
                            "manifest_path": str(root / "private-explicit/Cargo.toml"),
                            "publish": False,
                            "dependencies": [],
                        },
                    ],
                }

            def run_cli(arguments):
                stdout = io.StringIO()
                stderr = io.StringIO()
                with patch.object(
                    MODULE, "cargo_metadata", side_effect=controlled_metadata
                ), patch.object(
                    sys, "argv", [sys.executable, *([] if schema_version == 1 else ["--schema-version", "2"]), *arguments]
                ), redirect_stdout(stdout), redirect_stderr(stderr):
                    try:
                        returncode = MODULE.main()
                    except SystemExit as error:
                        returncode = int(error.code)
                return returncode, stdout.getvalue(), stderr.getvalue()

            frozen_output = frozen_root / f"release_topology.frozen.v{schema_version}.json"
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
            explicit_output = frozen_root / "explicit-selection.json"
            explicit = run_cli([
                "--root", str(frozen_root), "--release", "0.18.0",
                "--frozen-product-sha", frozen_sha, "--schema-version", str(schema_version),
                "--output", str(explicit_output),
            ])
            self.assertEqual(explicit[0], 0, explicit[2])
            self.assertEqual(explicit_output.read_bytes(), frozen_output.read_bytes())
            for bad_version in ("0", "3", "true", "1.5"):
                unsupported = run_cli([
                    "--root", str(frozen_root), "--release", "0.18.0",
                    "--frozen-product-sha", frozen_sha, "--schema-version", bad_version,
                    "--output", str(explicit_output),
                ])
                self.assertNotEqual(unsupported[0], 0)
            if schema_version == 2:
                frozen_value = json.loads(frozen_output.read_bytes())
                for corrupt in ("missing", "archive-local", "foreign-target"):
                    changed = deepcopy(frozen_value)
                    if corrupt == "missing":
                        del changed["checksum_assets"]
                    elif corrupt == "archive-local":
                        changed["checksum_assets"][0]["asset_name"] = "SHA256SUMS.txt"
                    else:
                        changed["checksum_assets"][0]["archive_targets"].append("foreign-target")
                    invalid_output = frozen_root / "invalid-checksums.json"
                    invalid_output.write_text(json.dumps(changed), encoding="utf-8")
                    invalid_check = run_cli([
                        "--root", str(frozen_root), "--release", "0.18.0",
                        "--frozen-product-sha", frozen_sha, "--check",
                        "--output", str(invalid_output),
                    ])
                    self.assertNotEqual(invalid_check[0], 0)
                    self.assertIn("checksum_assets", invalid_check[2])
            prepared_authority = prepared_root / f"release_topology.frozen.v{schema_version}.json"
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
            prepared_value = json.loads(outputs[0])
            self.assertEqual(prepared_value["schema"], schema_version)
            self.assertEqual("checksum_assets" in prepared_value, schema_version == 2)
            if schema_version == 2:
                self.assertEqual(prepared_value["checksum_assets"], json.loads(frozen_output.read_bytes())["checksum_assets"])

            # The check command never guesses a newer contract or upgrades old
            # authority; callers must select the version they intend to consume.
            mismatch = run_cli([
                "--root", str(frozen_root), "--release", "0.18.0",
                "--frozen-product-sha", frozen_sha, "--check",
                "--schema-version", str(3 - schema_version),
                "--output", str(frozen_output),
            ])
            self.assertNotEqual(mismatch[0], 0)
            self.assertIn("manifest schema differs from --schema-version", mismatch[2])
            crossed = run_cli([
                "--root", str(prepared_root), "--release", "0.18.1",
                "--frozen-product-sha", frozen_sha, "--prepared-swarm-sha", prepared_sha,
                "--frozen-root", str(frozen_root), "--frozen-topology", str(prepared_authority),
                "--frozen-topology-sha256", digest, "--schema-version", str(3 - schema_version),
                "--output", str(prepared_root / "crossed.json"),
            ])
            self.assertNotEqual(crossed[0], 0)
            self.assertIn("frozen/prepared topology schema versions must match", crossed[2])

            legacy_manifest = json.loads(outputs[0])
            legacy_manifest["frozen_product_sha"] = prepared_sha
            legacy_manifest["prepared_swarm_sha"] = "c" * 40
            legacy_path = prepared_root / "legacy.json"
            legacy_path.write_text(json.dumps(legacy_manifest), encoding="utf-8")
            legacy_prepared = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    prepared_sha,
                    "--output",
                    str(legacy_path),
                ]
            )
            self.assertNotEqual(legacy_prepared[0], 0)
            self.assertIn("requires an explicit prepared validation authority", legacy_prepared[2])

            stray_authority = run_cli(
                [
                    "--root",
                    str(frozen_root),
                    "--check",
                    "--release",
                    "0.18.0",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--frozen-root",
                    str(frozen_root),
                    "--output",
                    str(frozen_output),
                ]
            )
            self.assertNotEqual(stray_authority[0], 0)
            self.assertIn("auxiliary arguments require --frozen-topology", stray_authority[2])

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

            prepared_schema = prepared_root / schema_relative
            clean_schema = prepared_schema.read_text(encoding="utf-8")
            prepared_schema.write_text(clean_schema + "\n", encoding="utf-8")
            dirty_schema_run = run_cli(
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
                    digest,
                    "--output",
                    str(prepared_root / "release_topology.1.json"),
                ]
            )
            self.assertNotEqual(dirty_schema_run[0], 0)
            self.assertIn("schema source hash is stale", dirty_schema_run[2])
            prepared_schema.write_text(clean_schema + "\n", encoding="utf-8")
            git("add", schema_relative, cwd=prepared_root)
            git("commit", "-qm", "invalid schema change", cwd=prepared_root)
            changed_schema_sha = git("rev-parse", "HEAD", cwd=prepared_root)
            changed_schema_manifest = json.loads(outputs[0])
            changed_schema_manifest["prepared_swarm_sha"] = changed_schema_sha
            changed_schema_manifest["sources"][schema_relative]["sha256"] = (
                MODULE.sha256(prepared_schema)
            )
            changed_schema_output = prepared_root / "changed-schema.json"
            changed_schema_output.write_text(
                json.dumps(changed_schema_manifest), encoding="utf-8"
            )
            changed_schema_run = run_cli(
                [
                    "--root",
                    str(prepared_root),
                    "--check",
                    "--release",
                    "0.18.1",
                    "--frozen-product-sha",
                    frozen_sha,
                    "--prepared-swarm-sha",
                    changed_schema_sha,
                    "--frozen-root",
                    str(frozen_root),
                    "--frozen-topology",
                    str(prepared_authority),
                    "--frozen-topology-sha256",
                    digest,
                    "--output",
                    str(changed_schema_output),
                ]
            )
            self.assertNotEqual(changed_schema_run[0], 0)
            self.assertIn("outside version metadata", changed_schema_run[2])
            prepared_schema.write_text(clean_schema, encoding="utf-8")
            git("add", schema_relative, cwd=prepared_root)
            git("commit", "-qm", "restore schema", cwd=prepared_root)

            private_explicit.write_text(
                private_explicit.read_text(encoding="utf-8").replace(
                    "name = 'private-explicit'\nversion = '0.18.0'",
                    "name = 'private-explicit'\nversion = '0.18.1'",
                ),
                encoding="utf-8",
            )
            git("add", "private-explicit/Cargo.toml", cwd=prepared_root)
            git("commit", "-qm", "invalid private explicit bump", cwd=prepared_root)
            invalid_prepared = json.loads(outputs[0])
            invalid_prepared["prepared_swarm_sha"] = git(
                "rev-parse", "HEAD", cwd=prepared_root
            )
            invalid_prepared["sources"]["private-explicit/Cargo.toml"]["sha256"] = (
                MODULE.sha256(private_explicit)
            )
            with self.assertRaisesRegex(
                MODULE.TopologyError, "outside version metadata"
            ):
                MODULE.validate_prepared_projection(
                    json.loads(frozen_output.read_bytes()),
                    invalid_prepared,
                    digest,
                    prepared_authority,
                    frozen_root,
                    prepared_root,
                )

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

    def test_downloader_target_derivation_accepts_windows_target_constants(self):
        source = """
        const WINDOWS_X64_TARGET = 'x86_64-pc-windows-msvc';
        const WINDOWS_ARM64_TARGET = 'aarch64-pc-windows-msvc';
        if (arch === 'arm64') return WINDOWS_ARM64_TARGET;
        return WINDOWS_X64_TARGET;
        """
        workflow_targets = {
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        }
        self.assertEqual(
            MODULE.derive_downloader_targets(source, workflow_targets),
            workflow_targets,
        )

    def test_downloader_target_derivation_rejects_unused_or_wrong_constants(self):
        unused = "const WINDOWS_X64_TARGET = 'x86_64-pc-windows-msvc';"
        wrong = """
        const WINDOWS_X64_TARGET = 'other-target';
        return WINDOWS_X64_TARGET;
        """
        workflow_targets = {"x86_64-pc-windows-msvc"}
        self.assertEqual(MODULE.derive_downloader_targets(unused, workflow_targets), set())
        self.assertEqual(MODULE.derive_downloader_targets(wrong, workflow_targets), set())

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
