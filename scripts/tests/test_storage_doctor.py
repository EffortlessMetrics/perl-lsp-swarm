"""Offline retained-output controls; no Cargo or large disk allocation."""

import os
import json
import sys
from contextlib import contextmanager
from unittest import mock
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


BASH = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else shutil.which("bash")
SCRIPT = Path(__file__).resolve().parents[2] / "scripts/storage-doctor"


def posix(path):
    value = Path(path).resolve().as_posix()
    return f"/{value[0].lower()}{value[2:]}" if os.name == "nt" else value


@unittest.skipUnless(shutil.which("bash") and shutil.which("git"), "requires Bash and Git")
class StorageDoctorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="storage-doctor-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "checkout with spaces"
        self.repo.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(PATH=str(self.bin) + os.pathsep + self.env["PATH"],
                        DEVPLANE=str(self.root / "devplane"),
                        TMPDIR=str(self.root), KB="8", DU_STATUS="0")
        subprocess.run(["git", "init", "-q", str(self.repo)], env=self.env, check=True)
        self.stub("sccache", "exit 0")
        self.stub("du", 'if [ "$1" = "-sk" ]; then\n'
                  '  printf "%s\\t%s\\n" "$KB" "${@: -1}"\n'
                  '  exit "$DU_STATUS"\nfi\nexit 0')

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/usr/bin/env bash\n" + body + "\n", encoding="utf-8", newline="\n")
        path.chmod(0o755)

    def target(self, parent="package with spaces"):
        path = self.repo / parent / "target"
        path.mkdir(parents=True)
        return path

    def run_doctor(self):
        result = subprocess.run([BASH, "--noprofile", "--norc", "-c", 'export PATH="$1:$PATH"; shift; exec bash "$@"',
                                 "storage-control", posix(self.bin), posix(SCRIPT)], cwd=self.repo, env=self.env,
                                text=True, capture_output=True)
        self.assertFalse(list(self.root.glob("storage-doctor-targets.*")), "temporary list leaked")
        return result

    def test_empty_inventory_is_known_empty(self):
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("== repo-local target dirs ==\nnone\n", result.stdout)

    def test_reports_complete_whitespace_path(self):
        path = self.target("package with spaces" if os.name == "nt" else "package\twith spaces\nand newline")
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("0.0G\t" + path.as_posix() + "\n", result.stdout)

    def test_threshold_and_one_kib_over_preserve_refusal(self):
        path = self.target()
        for kb, status in ((1024 * 1024, 0), (1024 * 1024 + 1, 1), (2**64, 1)):
            with self.subTest(kb=kb):
                self.env["KB"] = str(kb)
                result = self.run_doctor()
                self.assertEqual(result.returncode, status, result.stderr)
                if status:
                    self.assertIn("exceeds 1G: " + path.as_posix(), result.stderr)

    def test_failed_or_partial_measurement_is_not_empty_or_success(self):
        path = self.target()
        for kb in ("", "8"):
            with self.subTest(kb=kb):
                self.env.update(KB=kb, DU_STATUS="1")
                result = self.run_doctor()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("could not measure", result.stderr)
                self.assertIn(path.as_posix(), result.stderr)
                self.assertNotIn("\nnone\n", result.stdout)

    def test_successful_du_without_a_byte_count_is_unknown(self):
        self.target()
        for kb in ("", "unknown", "-1"):
            with self.subTest(kb=kb):
                self.env["KB"] = kb
                result = self.run_doctor()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("invalid target size", result.stderr)

    def test_failed_enumeration_is_not_empty_or_success(self):
        self.stub("find", "exit 1")
        result = self.run_doctor()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not enumerate", result.stderr)
        self.assertNotIn("\nnone\n", result.stdout)

    def test_partial_enumeration_cannot_establish_complete_usage(self):
        path = self.target()
        self.env["PARTIAL_TARGET"] = path.as_posix()
        self.stub("find", 'printf "%s\\0" "$PARTIAL_TARGET"; exit 1')
        result = self.run_doctor()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not enumerate", result.stderr)

    def test_measurement_failure_recovers_on_next_observation(self):
        self.target()
        self.env["DU_STATUS"] = "1"
        self.assertNotEqual(self.run_doctor().returncode, 0)
        self.env["DU_STATUS"] = "0"
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_later_target_is_measured_and_can_refuse(self):
        first = self.target("first small package")
        later = self.target("later package")
        self.env.update(FIRST_TARGET=first.as_posix(), LATER_TARGET=later.as_posix())
        # Enumeration order is explicit; filesystem traversal order is not an oracle.
        self.stub("find", 'printf "%s\\0%s\\0" "$FIRST_TARGET" "$LATER_TARGET"')
        self.stub("du", 'if [ "$1" = "-sk" ]; then\n'
                  '  if [ "${@: -1}" = "$LATER_TARGET" ]; then\n'
                  '    printf "%s\\t%s\\n" "$LATER_KB" "$LATER_TARGET"\n'
                  '    exit "$LATER_STATUS"\n  fi\n'
                  '  printf "8\\t%s\\n" "${@: -1}"\nfi\nexit 0')
        for kb, du_status, expected in (("8", "0", 0), ("1048577", "0", 1), ("8", "1", 1)):
            with self.subTest(kb=kb, du_status=du_status):
                self.env.update(LATER_KB=kb, LATER_STATUS=du_status)
                result = self.run_doctor()
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertIn(first.as_posix(), result.stdout)
                if expected:
                    self.assertIn(later.as_posix(), result.stderr)
                else:
                    self.assertIn(later.as_posix(), result.stdout)

    def test_real_small_output_is_measured_without_mutation(self):
        (self.bin / "du").unlink()
        path = self.target()
        payload = path / "retained"
        payload.write_bytes(b"retained output\n" * 512)
        before = payload.stat()
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(path.as_posix(), result.stdout)
        after = payload.stat()
        self.assertEqual((after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns),
                         (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns))

    def test_devplane_without_descriptor_is_unknown_not_guessed(self):
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("NOT_PROVEN: no admitted resource descriptor", result.stdout)
        self.assertNotIn("devplane not initialized", result.stdout)

    def test_clipped_enumeration_is_not_complete_or_empty(self):
        self.stub("find", "printf '%65536s' x")
        result = self.run_doctor()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("enumeration clipped", result.stderr)
        self.assertNotIn("\nnone\n", result.stdout)



class DescriptorInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="descriptor-storage-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "checkout"
        self.repo.mkdir()
        self.saved_cwd = Path.cwd()
        self.addCleanup(os.chdir, self.saved_cwd)
        os.chdir(self.repo)
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.environment = mock.patch.dict(os.environ, env, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        subprocess.run(["git", "init", "-q", str(self.repo)], env=env, check=True)
        sys.path.insert(0, str(SCRIPT.parent))
        self.addCleanup(sys.path.remove, str(SCRIPT.parent))
        import storage_observation
        self.observer = storage_observation
        from cargo_admitted import resource_plan, budget_scope
        self.env = dict(env, DEVPLANE=str(self.root / "non-basename-base"),
                        CARGO_HOME=str(self.root / "home"), TMPDIR=str(self.root / "tmp"))
        worktree, slot, self.paths = resource_plan(self.env)
        for path in self.paths.values():
            path.mkdir(parents=True)
        self.data = {"worktree": str(worktree), "scope": budget_scope(["check", "-p", "xtask"],
                     self.env, worktree, self.paths, 2), "resources": {key: str(path) for key, path in self.paths.items()},
                     "lease": str(slot / "cargo-active"), "pid": 123, "admission": {"private": "SENSITIVE"}}
        self.file = self.root / "descriptor.json"

    def observe(self, raw=None):
        self.file.write_text(json.dumps(self.data) if raw is None else raw, encoding="utf-8")
        return self.observer.observe(self.file)

    def test_actual_descriptor_plan_accepts_separate_profile_and_triple_roots(self):
        for name, relative, value in (("target", "agent/.fingerprint/xtask-a/lib.json", 2**64-3),
                ("build", "aarch64-unknown-linux-gnu/custom/.fingerprint/serde-b/lib.json", 2**64-2)):
            path = self.paths[name] / relative
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps({"path": value, "rustflags": ["SENSITIVE"]}))
        result = self.observe()
        self.assertTrue(result["descriptor_valid"], result)
        self.assertTrue(result["inventory_complete"])
        self.assertEqual({entry["resource"] for entry in result["fingerprints"]}, {"target", "build"})
        self.assertEqual({entry["hashes"]["path"] for entry in result["fingerprints"]}, {str(2**64-3), str(2**64-2)})
        self.assertNotIn("SENSITIVE", json.dumps(result))
        self.assertNotIn(str(self.root), json.dumps(result))
        self.assertEqual(result["observation_kind"], "historical_only")
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertFalse(result["admitted_before_sample"])
        for key in ("peak_bytes", "build_delta_bytes", "open_unlinked_bytes", "wsl_guest_free_bytes",
                    "vhdx_allocated_bytes", "host_backing_free_bytes"):
            self.assertIsNone(result[key])

    def test_clippy_descriptor_scope_accepts_bounded_clippy_setup(self):
        import copy

        def subject(name):
            return {"path": f"/toolchains/{name}", "file_identity": [1, 2, 3, 4, 5],
                    "sha256": "a" * 64}

        clippy = copy.deepcopy(self.data)
        clippy["scope"]["operation"] = "clippy"
        clippy["scope"]["clippy_setup"] = {
            "toolchain": {"pin": "1.95.0", "host": "x86_64-unknown-linux-gnu",
                          "root": "/toolchains/1.95.0", "subjects": {"cargo": subject("cargo")},
                          "pin_subject": subject("rust-toolchain.toml")},
            "configuration": [subject("clippy.toml")],
            "renderer": subject("cargo_admitted.py"),
        }
        result = self.observe(json.dumps(clippy))
        self.assertTrue(result["descriptor_valid"], result)

        for mutate in (
                lambda s: s.update(operation="check"),
                lambda s: s["clippy_setup"].update(extra={}),
                lambda s: s["clippy_setup"]["toolchain"].update(pin="latest"),
                lambda s: s["clippy_setup"]["renderer"].update(sha256="not-hex"),
                lambda s: s["clippy_setup"].pop("configuration"),
        ):
            broken = copy.deepcopy(clippy)
            mutate(broken["scope"])
            result = self.observe(json.dumps(broken))
            self.assertFalse(result["descriptor_valid"], result)
            self.assertIn("descriptor_scope_mismatch", result["reasons"])

    def test_hardlinks_are_deduplicated_across_combined_roots(self):
        payload = self.paths["build"] / "payload"
        payload.write_bytes(b"x" * 8192)
        os.link(payload, self.paths["target"] / "same-payload")
        result = self.observe()
        self.assertTrue(result["inventory_complete"], result)
        self.assertEqual(result["combined"]["apparent_bytes"], 8192)
        self.assertEqual(sum(row["apparent_bytes"] for row in result["resources"].values()), 16384)
        if os.name == "nt":
            self.assertIsNone(result["combined"]["allocated_bytes"])
        else:
            self.assertIsNotNone(result["combined"]["allocated_bytes"])

    def test_changing_hardlink_alias_keeps_partial_bytes_but_invalidates_totals(self):
        root = self.paths["build"]
        changed_alias = root / "a"
        first_alias = root / "b"
        changed_alias.write_bytes(b"x")
        os.link(changed_alias, first_alias)
        real_scandir, real_lstat = os.scandir, Path.lstat
        changed = False

        @contextmanager
        def ordered_scandir(path):
            with real_scandir(path) as children:
                # DFS pops b before a, independently of filesystem enumeration.
                yield iter(sorted(children, key=lambda child: child.name))

        def lstat(path):
            nonlocal changed
            if path == changed_alias and not changed:
                changed_alias.write_bytes(b"xx")
                changed = True
            return real_lstat(path)

        with mock.patch.object(self.observer.os, "scandir", ordered_scandir), mock.patch.object(Path, "lstat", lstat):
            result = self.observer.inventory(self.paths)
        self.assertTrue(changed)
        self.assertEqual(first_alias.stat().st_size, 2)
        self.assertFalse(result["inventory_complete"])
        self.assertFalse(result["fingerprints_complete"])
        build = result["resources"]["build"]
        self.assertIn("entry_changed", build["reasons"])
        self.assertEqual(build["observed_apparent_bytes"], 1)
        self.assertEqual(result["combined"]["observed_apparent_bytes"], 1)
        for row in (build, result["combined"]):
            self.assertIsNone(row["apparent_bytes"])
            self.assertIsNone(row["allocated_bytes"])

    def test_empty_is_known_only_when_all_named_roots_are_present(self):
        result = self.observe()
        self.assertTrue(result["inventory_complete"])
        self.assertEqual(result["combined"]["apparent_bytes"], 0)
        self.paths["build"].rmdir()
        result = self.observe()
        self.assertFalse(result["inventory_complete"])
        self.assertIsNone(result["combined"]["apparent_bytes"])
        self.assertIn("missing_or_changed_entry", result["resources"]["build"]["reasons"])

    def test_malformed_duplicate_nonfinite_and_oversized_inputs_are_unknown(self):
        for raw in ("{}", "[]", '{"scope":{},"scope":{}}', '{"scope":NaN}', "x" * 65537):
            with self.subTest(raw=raw[:40]):
                result = self.observe(raw)
                self.assertFalse(result["descriptor_valid"])
                self.assertFalse(result["inventory_complete"])

    def test_scope_host_worktree_resources_and_scalar_types_are_checked(self):
        original = json.loads(json.dumps(self.data))
        changes = [("hostname", "wrong-host"), ("platform", "wrong-platform"), ("jobs", True),
                   ("cargo_argv_sha256", "not-a-hash"), ("worktree", str(self.root))]
        for key, value in changes:
            with self.subTest(key=key):
                self.data = json.loads(json.dumps(original))
                self.data["scope"][key] = value
                self.assertFalse(self.observe()["descriptor_valid"])
        self.data = original
        self.data["resources"]["build"] = str(self.root)
        self.assertFalse(self.observe()["descriptor_valid"])

    def test_consistent_sibling_resources_and_dot_paths_are_rejected(self):
        original = json.loads(json.dumps(self.data))
        wrong = str(self.paths["target"].parent.parent / ("f" * 64) / "target")
        self.data["resources"]["target"] = wrong
        self.data["scope"]["resources"]["target"] = wrong
        self.assertFalse(self.observe()["descriptor_valid"])
        self.data = original
        wrong = str(self.paths["temp"] / "missing") + "/../"
        self.data["resources"]["temp"] = wrong
        self.data["scope"]["resources"]["temp"] = wrong
        self.assertFalse(self.observe()["descriptor_valid"])

    def test_nested_working_directory_and_changed_shell_hints(self):
        nested = self.repo / "nested"
        nested.mkdir()
        os.chdir(nested)
        self.data["scope"]["working_directory"] = str(nested)
        with mock.patch.dict(os.environ, {"DEVPLANE": str(self.root / "wrong"),
                "CARGO_TARGET_DIR": str(self.root / "wrong"), "CARGO_HOME": str(self.root / "wrong")}):
            self.assertTrue(self.observe()["descriptor_valid"])
        self.data["scope"]["working_directory"] = str(self.repo)
        self.assertFalse(self.observe()["descriptor_valid"])

    def test_relative_descriptor_input_and_prefix_are_supported(self):
        self.file = self.repo / "record.json"
        self.observe()
        result = self.observer.observe("record.json")
        self.assertTrue(result["descriptor_valid"], result)
        self.file.write_text("cargo-admitted resources: " + json.dumps(self.data))
        self.assertTrue(self.observer.observe("record.json")["descriptor_valid"])

    def test_descriptor_cli_does_not_bypass_existing_local_target_guard(self):
        self.observe()
        target = self.repo / "target"
        target.mkdir()
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        du = fake_bin / "du"
        du.write_text('#!/usr/bin/env bash\nprintf "1048577\\t%s\\n" "${@: -1}"\n', newline="\n")
        du.chmod(0o755)
        result = subprocess.run([BASH, "--noprofile", "--norc", "-c",
            'export PATH="$1:$PATH"; shift; exec bash "$@"', "descriptor-guard",
            posix(fake_bin), posix(SCRIPT), "--descriptor", posix(self.file), "--json"],
            cwd=self.repo, text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertTrue(json.loads(result.stdout)["descriptor_valid"], result.stdout)
        self.assertIn("repo-local guard", result.stderr)

    def test_failed_repository_identity_is_sanitized_unknown(self):
        self.observe()
        result = subprocess.run([sys.executable, "-B", str(SCRIPT.parent / "storage_observation.py"),
            "--descriptor", str(self.file)], cwd=self.root, text=True, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stderr, "")
        self.assertFalse(json.loads(result.stdout)["descriptor_valid"])
        with mock.patch.object(self.observer, "resource_plan", side_effect=subprocess.TimeoutExpired("git", 2)):
            self.assertFalse(self.observe()["descriptor_valid"])

    def test_root_replaced_during_enumeration_is_not_complete(self):
        root = self.paths["build"]
        original = root.with_name("retained-original-build")
        foreign = self.root / "foreign"
        foreign.mkdir()
        (foreign / "must-not-read.json").write_text("SENSITIVE")
        real_scandir = os.scandir
        swapped = False
        def scandir(path):
            nonlocal swapped
            if Path(path) == root and not swapped:
                root.rename(original)
                try:
                    root.symlink_to(foreign, target_is_directory=True)
                except OSError:
                    original.rename(root)
                    self.skipTest("symlink creation not permitted on this host")
                swapped = True
            return real_scandir(path)
        with mock.patch.object(self.observer.os, "scandir", side_effect=scandir):
            result = self.observer.inventory(self.paths)
        self.assertTrue(swapped)
        self.assertFalse(result["inventory_complete"])
        self.assertIsNone(result["combined"]["apparent_bytes"])
        self.assertNotIn("SENSITIVE", json.dumps(result))

    def test_linked_entry_is_rejected_before_scandir_or_fingerprint_read(self):
        root = self.paths["build"]
        unit = root / "triple/custom/.fingerprint/unit"
        unit.mkdir(parents=True)
        payload = unit / "lib.json"
        payload.write_text('{"path":5}')
        real_lstat = Path.lstat
        real_scandir = os.scandir
        real_read = self.observer.read_file
        for linked_path in (root / "triple", root / "triple/custom", unit, payload):
            with self.subTest(location=linked_path.relative_to(root)):
                visited, read = [], []
                def lstat(path):
                    original = real_lstat(path)
                    if path == linked_path:
                        return mock.Mock(st_mode=original.st_mode,
                            st_file_attributes=getattr(__import__("stat"), "FILE_ATTRIBUTE_REPARSE_POINT", 1024))
                    return original
                def scandir(path):
                    visited.append(Path(path))
                    return real_scandir(path)
                def read_file(path, *args, **kwargs):
                    read.append(Path(path))
                    return real_read(path, *args, **kwargs)
                with mock.patch.object(Path, "lstat", lstat), mock.patch.object(self.observer.os, "scandir", side_effect=scandir), mock.patch.object(self.observer, "read_file", side_effect=read_file):
                    result = self.observer.inventory(self.paths)
                self.assertFalse(result["inventory_complete"])
                self.assertNotIn(linked_path, visited)
                self.assertNotIn(payload, read)
                self.assertFalse(result["fingerprints_complete"])

    def test_linked_descriptor_and_resource_ancestors_are_rejected(self):
        self.observe()
        alias = self.root / "hardlink-descriptor"
        os.link(self.file, alias)
        self.assertFalse(self.observer.observe(alias)["descriptor_valid"])
        alias.unlink()
        link = self.root / "linked"
        try:
            link.symlink_to(self.paths["temp"], target_is_directory=True)
        except OSError:
            self.skipTest("symlink creation not permitted on this host")
        self.data["resources"]["temp"] = str(link / "child")
        self.data["scope"]["resources"]["temp"] = str(link / "child")
        self.assertFalse(self.observe()["descriptor_valid"])

    def test_entry_depth_time_and_fingerprint_bounds_preserve_partial_unknown(self):
        (self.paths["target"] / "retained").write_bytes(b"abc")
        for limits in ({"max_entries": 2}, {"max_depth": 0}, {"max_seconds": 0}):
            with self.subTest(limits=limits):
                result = self.observer.inventory(self.paths, **limits)
                self.assertFalse(result["inventory_complete"])
                self.assertIsNone(result["combined"]["apparent_bytes"])
        path = self.paths["build"] / "agent/.fingerprint/fixture/lib.json"
        path.parent.mkdir(parents=True)
        path.write_text('{"path":5}')
        result = self.observer.inventory(self.paths, max_fingerprint_bytes=2)
        self.assertTrue(result["inventory_complete"])
        self.assertFalse(result["fingerprints_complete"])
        self.assertGreater(result["combined"]["observed_apparent_bytes"], 3)

    def test_failed_fingerprint_reads_share_one_content_budget(self):
        accepted = self.paths["target"] / "agent/.fingerprint/good/lib.json"
        accepted.parent.mkdir(parents=True)
        accepted.write_bytes(b"{}")
        growing = [self.paths["build"] / f"triple/custom/.fingerprint/unit-{index}/lib.json"
                   for index in range(4)]
        for path in growing:
            path.parent.mkdir(parents=True)
        real_open, real_fdopen = os.open, os.fdopen
        for allowance, grown_size, read_error in ((64, 66, False), (8, 3, False), (8, 3, True)):
            with self.subTest(allowance=allowance, grown_size=grown_size, read_error=read_error):
                for path in growing:
                    path.write_bytes(b"{}")
                opened_paths, read_sizes = {}, []

                def open_file(path, *args, **kwargs):
                    descriptor = real_open(path, *args, **kwargs)
                    opened_paths[descriptor] = Path(path)
                    return descriptor

                class GrowingReader:
                    def __init__(self, stream, path):
                        self.stream, self.path = stream, path

                    def fileno(self):
                        return self.stream.fileno()

                    def read(self, size):
                        # Actual growth after the truthful opened-file stat,
                        # immediately before the real bounded content read.
                        if self.path in growing:
                            self.path.write_bytes(b"{}" + b" " * (grown_size - 2))
                        raw = self.stream.read(size)
                        read_sizes.append(len(raw))
                        if read_error and self.path in growing:
                            raise OSError("SENSITIVE partial read failure")
                        return raw

                @contextmanager
                def fdopen(descriptor, *args, **kwargs):
                    with real_fdopen(descriptor, *args, **kwargs) as stream:
                        yield GrowingReader(stream, opened_paths[descriptor])

                with mock.patch.object(self.observer.os, "open", open_file), mock.patch.object(self.observer.os, "fdopen", fdopen):
                    result = self.observer.inventory(self.paths, max_fingerprint_bytes=allowance)
                self.assertGreater(sum(read_sizes), len(b"{}"))
                self.assertLessEqual(sum(read_sizes), allowance + 1)
                self.assertLess(len(read_sizes), len(growing) + 1)
                self.assertEqual(result["status"], "NOT_PROVEN")
                self.assertFalse(result["fingerprints_complete"])
                self.assertFalse(result["inventory_complete"])
                self.assertIn("entry_changed", result["resources"]["build"]["reasons"])
                self.assertIsNone(result["resources"]["build"]["apparent_bytes"])
                self.assertIsNone(result["combined"]["apparent_bytes"])
                self.assertEqual(len(result["fingerprints"]), 1)
                self.assertEqual(result["fingerprints"][0]["resource"], "target")
                self.assertIn("fingerprint_unavailable_or_clipped", result["fingerprint_reasons"])
                self.assertGreater(result["combined"]["observed_apparent_bytes"], 0)
                self.assertNotIn("SENSITIVE", json.dumps(result))

    def test_short_fingerprint_read_cannot_claim_complete_content(self):
        payload = self.paths["build"] / "agent/.fingerprint/unit/lib.json"
        payload.parent.mkdir(parents=True)
        payload.write_bytes(b"{}    ")
        real_fdopen = os.fdopen

        class ShortReader:
            def __init__(self, stream):
                self.stream = stream

            def fileno(self):
                return self.stream.fileno()

            def read(self, size):
                return self.stream.read(min(size, 2))

        @contextmanager
        def fdopen(*args, **kwargs):
            with real_fdopen(*args, **kwargs) as stream:
                yield ShortReader(stream)

        with mock.patch.object(self.observer.os, "fdopen", fdopen):
            result = self.observer.inventory(self.paths)
        self.assertFalse(result["fingerprints_complete"])
        self.assertEqual(result["fingerprints"], [])
        self.assertTrue(result["inventory_complete"])
        self.assertEqual(result["combined"]["apparent_bytes"], 6)

    def test_unreadable_disappearing_and_zero_identity_never_report_empty_success(self):
        real_scandir = os.scandir
        def denied(path):
            if Path(path) == self.paths["target"]:
                raise PermissionError("SENSITIVE")
            return real_scandir(path)
        with mock.patch.object(self.observer.os, "scandir", side_effect=denied):
            result = self.observer.inventory(self.paths)
        self.assertFalse(result["inventory_complete"])
        self.assertNotIn("SENSITIVE", json.dumps(result))


if __name__ == "__main__":
    unittest.main()
