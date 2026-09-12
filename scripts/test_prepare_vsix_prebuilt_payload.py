"""Integration checks for the release-evidence to VSIX-input adapter."""

from __future__ import annotations

import hashlib
import json
import subprocess
import io
import tarfile
import tempfile
import unittest
import importlib.util
import sys
from pathlib import Path


SOURCE = "a" * 40
VERSION = "0.17.0"
TARGET = "x86_64-unknown-linux-gnu"


def digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


class PrebuiltPayloadAdapterTests(unittest.TestCase):
    def fixture(self, root: Path) -> dict[str, Path | str]:
        package_name = f"perllsp-{VERSION}-{TARGET}"
        package = root / "dist" / package_name
        package.mkdir(parents=True)
        archive = root / "dist" / f"{package_name}.tar.gz"
        payloads = {"perllsp": b"server", "perl-dap": b"dap"}
        for name, value in payloads.items():
            (package / name).write_bytes(value)
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(package, arcname=package_name)
        topology = {"schema": 1, "release": VERSION, "frozen_product_sha": SOURCE, "prepared_swarm_sha": None, "binary_targets": [{"target": TARGET, "os": "linux", "architecture": "x86_64", "libc": "gnu", "archive_name": archive.name, "required_members": ["perllsp", "perl-dap"]}]}
        topology_path = root / "topology.json"
        topology_path.write_text(json.dumps(topology), encoding="utf-8")
        topology_sha = digest(topology_path.read_bytes())
        identity = {"schema_version": "perl_lsp.release_build_identity.v1", "repository": "EffortlessMetrics/perl-lsp-swarm", "source_revision": SOURCE, "source_tree_digest": "b" * 64, "release_version": VERSION, "target": TARGET, "profile": "release", "candidate_identity": "candidate-1", "artifact_role": "archive", "product_identity_contract_digest": "c" * 64, "release_topology_digest": topology_sha, "toolchain_digest": "d" * 64}
        receipt_path = root / "receipt.json"
        binaries = []
        for name, role in (("perllsp", "server"), ("perl-dap", "dap")):
            packet = {"schema_version": "perl_lsp.binary_identity.v1", "product": {"name": "perl-lsp", "public_repository": "EffortlessMetrics/perl-lsp", "development_repository": "EffortlessMetrics/perl-lsp-swarm"}, "binary": {"executable": name, "cargo_package": name, "role": role, "version": VERSION}, "build": {"source_revision": SOURCE, "source_tree_digest": "b" * 64, "target": TARGET, "profile": "release", "identity_state": "exact"}, "artifact": {"role": "archive", "digest": None, "candidate_identity": "candidate-1"}, "compatibility": {"expected_product_identity_version": 1, "dap_posture": "preview"}, "limitations": []}
            binaries.append({"role": role, "executable": name, "path_role": f"target/{TARGET}/release/{name}", "file_sha256": digest(b"build-" + name.encode()), "packet_sha256": digest(json.dumps(packet, sort_keys=True, separators=(",", ":")).encode() + b"\n"), "packet": packet})
        receipt = {"schema_version": "perl_lsp.release_build_identity_receipt.v1", "status": "pass", "input_sha256": digest(json.dumps(identity, sort_keys=True, separators=(",", ":")).encode() + b"\n"), "input": identity, "runner": "cargo", "build_execution": "external_release_workflow", "build_commands": [["cargo", "build", "--locked", "--release", "--target", TARGET, "-p", "perllsp", "--bin", "perllsp"], ["cargo", "build", "--locked", "--release", "--target", TARGET, "-p", "perl-dap", "--bin", "perl-dap"]], "binaries": binaries, "claim_boundary": "test"}
        receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
        evidence = {"schema_version": "perl_lsp.release_package_evidence.v1", "status": "pass", "source_revision": SOURCE, "release_version": VERSION, "target": TARGET, "archive": {"name": archive.name, "sha256": digest(archive.read_bytes())}, "binaries": [{"executable": name, "member_path": f"{package_name}/{name}", "pre_strip_sha256": row["file_sha256"], "post_strip_sha256": digest(payloads[name])} for name, row in zip(payloads, binaries, strict=True)]}
        evidence_path = root / "evidence.json"
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        projection_path = root / "projection.json"
        projection_path.write_text(json.dumps({"releaseTopologySha256": topology_sha, "targets": [{"target": TARGET, "os": "linux", "architecture": "x86_64", "libc": "gnu", "archiveName": archive.name, "requiredMembers": ["perllsp", "perl-dap"]}], "includeUniversalManaged": False}), encoding="utf-8")
        return {"receipt": receipt_path, "evidence": evidence_path, "archive": archive, "topology": topology_path, "projection": projection_path, "output": root / "output", "dist": root / "dist"}

    def command(self, paths: dict[str, Path | str], **overrides: str) -> list[str]:
        values = {
            "source_sha": SOURCE,
            "target": TARGET,
            "candidate_id": "candidate-1",
            "release_version": VERSION,
            "inventory_sha256": "c" * 64,
            "extension_id": "EffortlessMetrics.perl-lsp-rs",
        }
        values.update(overrides)
        return ["python", "scripts/prepare_vsix_prebuilt_payload.py", "--receipt", str(paths["receipt"]), "--package-evidence", str(paths["evidence"]), "--archive", str(paths["archive"]), "--topology", str(paths["topology"]), "--projection", str(paths["projection"]), "--output", str(paths["output"]), "--source-sha", values["source_sha"], "--target", values["target"], "--candidate-id", values["candidate_id"], "--release-version", values["release_version"], "--inventory-sha256", values["inventory_sha256"], "--extension-id", values["extension_id"]]

    def namespace(self, paths: dict[str, Path | str]) -> object:
        spec = importlib.util.spec_from_file_location("prepare_payload", Path(__file__).with_name("prepare_vsix_prebuilt_payload.py"))
        if spec is None or spec.loader is None:
            raise RuntimeError("unable to load adapter")
        module = importlib.util.module_from_spec(spec)
        scripts_dir = str(Path(__file__).parent)
        sys.path.insert(0, scripts_dir)
        try:
            spec.loader.exec_module(module)
        finally:
            sys.path.remove(scripts_dir)
        return module, type("Args", (), {"receipt": paths["receipt"], "package_evidence": paths["evidence"], "archive": paths["archive"], "topology": paths["topology"], "projection": paths["projection"], "output": paths["output"], "source_sha": SOURCE, "target": TARGET, "candidate_id": "candidate-1", "release_version": VERSION, "inventory_sha256": "c" * 64, "extension_id": "EffortlessMetrics.perl-lsp-rs"})()

    def test_real_evidence_pipeline_emits_payload_and_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            command = self.command(paths)
            result = subprocess.run(command, cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = json.loads((paths["output"] / "vsix-candidate-payload.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["package"]["vscodeTargetId"], "linux-x64")
            self.assertEqual((paths["output"] / "bin" / "linux-x64" / "perllsp").read_bytes(), b"server")
            consumer = subprocess.run(["node", "-e", "const fs=require('fs'); const {preparePrebuiltPayload}=require('./vscode-extension/scripts/package-vsix.js'); const root=process.argv[1]; const env={PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST:process.argv[2],PERL_LSP_VSIX_PROJECTION_INPUT:process.argv[3],PERL_LSP_PREBUILT_SERVER_PATH:process.argv[4],PERL_LSP_PREBUILT_DAP_PATH:process.argv[5],PERL_LSP_CURRENT_SOURCE_SHA:'a'.repeat(40),PERL_LSP_RUST_TARGET:'x86_64-unknown-linux-gnu',PERL_LSP_VSCODE_TARGET:'linux-x64'}; const staged=preparePrebuiltPayload(fs,env,root); if(staged.manifest.package.vscodeTargetId!=='linux-x64'||fs.readFileSync(root+'/bin/linux-x64/perllsp','utf8')!=='server'||fs.readFileSync(root+'/bin/linux-x64/perl-dap','utf8')!=='dap') process.exit(2); staged.cleanup();", str(root / "consumer"), str(paths["output"] / "vsix-candidate-payload.json"), str(paths["projection"]), str(paths["output"] / "bin" / "linux-x64" / "perllsp"), str(paths["output"] / "bin" / "linux-x64" / "perl-dap")], cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertEqual(consumer.returncode, 0, consumer.stderr)

    def test_wrong_candidate_fails_before_creating_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            command = self.command(paths, candidate_id="wrong")
            result = subprocess.run(command, cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

    def test_malformed_build_identity_is_rejected_before_materialization(self) -> None:
        mutations = (
            ("schema_version", "unknown.identity.v9"),
            ("repository", "attacker/example"),
            ("profile", "debug"),
            ("artifact_role", "binary"),
            ("product_identity_contract_digest", "not-a-sha256"),
        )
        for field, value in mutations:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths = self.fixture(root)
                receipt = json.loads(Path(paths["receipt"]).read_text(encoding="utf-8"))
                receipt["input"][field] = value
                receipt["input_sha256"] = digest(
                    (json.dumps(receipt["input"], sort_keys=True, separators=(",", ":")) + "\n").encode()
                )
                Path(paths["receipt"]).write_text(json.dumps(receipt), encoding="utf-8")
                result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

    def test_expected_input_and_archive_failures_leave_no_output(self) -> None:
        for field, value in (("target", "x86_64-unknown-linux-musl"), ("inventory_sha256", "invalid")):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths = self.fixture(root)
                result = subprocess.run(self.command(paths, **{field: value}), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            evidence = json.loads(Path(paths["evidence"]).read_text(encoding="utf-8"))
            evidence["archive"]["sha256"] = "e" * 64
            Path(paths["evidence"]).write_text(json.dumps(evidence), encoding="utf-8")
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("archive digest", result.stderr)
            self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            projection = json.loads(Path(paths["projection"]).read_text(encoding="utf-8"))
            for field, value in (("archiveName", "other.tar.gz"), ("os", "windows"), ("architecture", "aarch64"), ("libc", "musl"), ("requiredMembers", ["perllsp"])):
                with self.subTest(field=field):
                    altered = json.loads(json.dumps(projection))
                    altered["targets"][0][field] = value
                    Path(paths["projection"]).write_text(json.dumps(altered), encoding="utf-8")
                    result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("differs from validated release topology", result.stderr)
                    self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            paths["output"].mkdir()
            (paths["output"] / "vsix-candidate-payload.json").write_text("existing", encoding="utf-8")
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to overwrite", result.stderr)

            evidence = json.loads(Path(paths["evidence"]).read_text(encoding="utf-8"))
            evidence["archive"]["sha256"] = digest(Path(paths["archive"]).read_bytes())
            evidence["binaries"][0]["post_strip_sha256"] = "f" * 64
            Path(paths["evidence"]).write_text(json.dumps(evidence), encoding="utf-8")
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("member digest", result.stderr)

    def test_missing_dap_and_unsafe_member_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            evidence = json.loads(Path(paths["evidence"]).read_text(encoding="utf-8"))
            evidence["binaries"] = evidence["binaries"][:1]
            Path(paths["evidence"]).write_text(json.dumps(evidence), encoding="utf-8")
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("exactly two", result.stderr)
            self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            package_name = f"perllsp-{VERSION}-{TARGET}"
            with tarfile.open(paths["archive"], "w:gz") as bundle:
                link = tarfile.TarInfo(f"{package_name}/perllsp")
                link.type = tarfile.SYMTYPE
                link.linkname = "outside"
                bundle.addfile(link)
                data = b"dap"
                member = tarfile.TarInfo(f"{package_name}/perl-dap")
                member.size = len(data)
                bundle.addfile(member, io.BytesIO(data))
            evidence = json.loads(Path(paths["evidence"]).read_text(encoding="utf-8"))
            evidence["archive"]["sha256"] = digest(Path(paths["archive"]).read_bytes())
            Path(paths["evidence"]).write_text(json.dumps(evidence), encoding="utf-8")
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("regular", result.stderr)
            self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())

    def test_symlinked_output_parent_is_rejected_without_external_write(self) -> None:
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            paths = self.fixture(root)
            (paths["output"] / "bin").mkdir(parents=True)
            try:
                (paths["output"] / "bin" / "linux-x64").symlink_to(Path(outside), target_is_directory=True)
            except OSError as error:
                if getattr(error, "winerror", None) == 1314:
                    self.skipTest(f"symlink privilege unavailable: {error}")
                raise
            result = subprocess.run(self.command(paths), cwd=Path(__file__).parents[1], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("output parent", result.stderr)
            self.assertEqual(list(Path(outside).iterdir()), [])

    def test_staging_setup_and_partial_copy_failures_clean_owned_temp(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            module, args = self.namespace(paths)
            original_mkdtemp = module.tempfile.mkdtemp
            def broken_stage(**kwargs: object) -> str:
                stage = Path(original_mkdtemp(**kwargs))
                (stage / "bin").write_text("collision", encoding="utf-8")
                return str(stage)
            module.tempfile.mkdtemp = broken_stage
            try:
                with self.assertRaises(OSError):
                    module.build(args)
            finally:
                module.tempfile.mkdtemp = original_mkdtemp
            self.assertEqual(list(root.glob(".prebuilt-payload-*")), [])

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            module, args = self.namespace(paths)
            original_copy = module.copy_selected_member
            def broken_copy(archive: Path, name: str, output: Path) -> str:
                output.write_bytes(b"partial")
                raise OSError("injected copy failure")
            module.copy_selected_member = broken_copy
            try:
                with self.assertRaisesRegex(OSError, "injected copy failure"):
                    module.build(args)
            finally:
                module.copy_selected_member = original_copy
            self.assertEqual(list(root.glob(".prebuilt-payload-*")), [])

    def test_competing_publication_after_precheck_is_rejected_without_overwrite(self) -> None:
        for collided_name in ("perllsp", "vsix-candidate-payload.json"):
            with self.subTest(collided_name=collided_name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                paths = self.fixture(root)
                module, args = self.namespace(paths)
                original_mkdir = Path.mkdir
                final_dir = paths["output"] / "bin" / "linux-x64"
                injected = False

                def competing_mkdir(
                    self: Path,
                    mode: int = 0o777,
                    parents: bool = False,
                    exist_ok: bool = False,
                ) -> None:
                    nonlocal injected
                    original_mkdir(self, mode=mode, parents=parents, exist_ok=exist_ok)
                    if self == final_dir and not injected:
                        destination = (
                            final_dir / collided_name
                            if collided_name == "perllsp"
                            else paths["output"] / collided_name
                        )
                        destination.write_bytes(b"COMPETITOR-SENTINEL")
                        injected = True

                Path.mkdir = competing_mkdir
                try:
                    with self.assertRaises(FileExistsError):
                        module.build(args)
                finally:
                    Path.mkdir = original_mkdir

                self.assertTrue(injected)
                destination = (
                    final_dir / collided_name
                    if collided_name == "perllsp"
                    else paths["output"] / collided_name
                )
                self.assertEqual(destination.read_bytes(), b"COMPETITOR-SENTINEL")
                if collided_name == "perllsp":
                    self.assertEqual(
                        [path for path in paths["output"].rglob("*") if path.is_file()],
                        [destination],
                    )
                else:
                    self.assertEqual(
                        (final_dir / "perllsp").read_bytes(), b"server"
                    )
                    self.assertEqual(
                        (final_dir / "perl-dap").read_bytes(), b"dap"
                    )
                self.assertEqual(list(root.glob(".prebuilt-payload-*")), [])

    def test_rollback_preserves_replaced_publication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = self.fixture(root)
            module, args = self.namespace(paths)
            original_link = module.os.link
            calls = 0
            destination = paths["output"] / "bin" / "linux-x64" / "perllsp"
            injected = False

            def replacing_link(source: Path, target: Path) -> None:
                nonlocal calls, injected
                calls += 1
                if calls == 1:
                    original_link(source, target)
                    target.unlink()
                    target.write_bytes(b"COMPETITOR-SENTINEL")
                    injected = True
                    return
                raise OSError("injected second publication failure")

            module.os.link = replacing_link
            try:
                with self.assertRaisesRegex(OSError, "injected second publication failure"):
                    module.build(args)
            finally:
                module.os.link = original_link

            self.assertTrue(injected)
            self.assertEqual(destination.read_bytes(), b"COMPETITOR-SENTINEL")
            self.assertFalse((paths["output"] / "vsix-candidate-payload.json").exists())
            self.assertEqual(list(root.glob(".prebuilt-payload-*")), [])

if __name__ == "__main__":
    unittest.main()
