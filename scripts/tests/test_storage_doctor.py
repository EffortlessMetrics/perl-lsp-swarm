"""Offline retained-output controls; no Cargo or large disk allocation."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts/storage-doctor"


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
        path.write_text("#!/usr/bin/env bash\n" + body + "\n")
        path.chmod(0o755)

    def target(self, parent="package with spaces"):
        path = self.repo / parent / "target"
        path.mkdir(parents=True)
        return path

    def run_doctor(self):
        result = subprocess.run(["bash", str(SCRIPT)], cwd=self.repo, env=self.env,
                                text=True, capture_output=True)
        self.assertFalse(list(self.root.glob("storage-doctor-targets.*")), "temporary list leaked")
        return result

    def test_empty_inventory_is_known_empty(self):
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("== repo-local target dirs ==\nnone\n", result.stdout)

    def test_reports_complete_whitespace_path(self):
        path = self.target("package\twith spaces\nand newline")
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("0.0G\t" + str(path) + "\n", result.stdout)

    def test_threshold_and_one_kib_over_preserve_refusal(self):
        path = self.target()
        for kb, status in ((1024 * 1024, 0), (1024 * 1024 + 1, 1), (2**64, 1)):
            with self.subTest(kb=kb):
                self.env["KB"] = str(kb)
                result = self.run_doctor()
                self.assertEqual(result.returncode, status, result.stderr)
                if status:
                    self.assertIn("exceeds 1G: " + str(path), result.stderr)

    def test_failed_or_partial_measurement_is_not_empty_or_success(self):
        path = self.target()
        for kb in ("", "8"):
            with self.subTest(kb=kb):
                self.env.update(KB=kb, DU_STATUS="1")
                result = self.run_doctor()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("could not measure", result.stderr)
                self.assertIn(str(path), result.stderr)
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
        self.env["PARTIAL_TARGET"] = str(path)
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
        self.env.update(FIRST_TARGET=str(first), LATER_TARGET=str(later))
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
                self.assertIn(str(first), result.stdout)
                if expected:
                    self.assertIn(str(later), result.stderr)
                else:
                    self.assertIn(str(later), result.stdout)

    def test_real_small_output_is_measured_without_mutation(self):
        (self.bin / "du").unlink()
        path = self.target()
        payload = path / "retained"
        payload.write_bytes(b"retained output\n" * 512)
        before = payload.stat()
        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(path), result.stdout)
        self.assertEqual(payload.stat(), before)

    @unittest.skipUnless(os.name == "posix" and shutil.which("du"), "requires POSIX du accounting")
    def test_devplane_hardlinks_are_labeled_as_order_dependent_batch_attribution(self):
        (self.bin / "du").unlink()
        devplane = Path(self.env["DEVPLANE"])
        first = devplane / "a-build"
        later = devplane / "z-target"
        first.mkdir(parents=True)
        later.mkdir()
        payload = first / "shared-output"
        payload.write_bytes(b"x" * (128 * 1024))
        os.link(payload, later / payload.name)
        self.env["LC_ALL"] = "C"

        def du(*paths, human=False):
            return subprocess.check_output(
                ["du", "-sh" if human else "-sk", *map(str, paths)],
                env=self.env, text=True)

        batch = du(first, later)
        reverse = du(later, first)
        independent = [int(du(path).split("\t", 1)[0]) for path in (first, later)]
        def sizes(text):
            return [int(row.split("\t", 1)[0]) for row in text.splitlines()]
        # Real nonempty inode sharing distinguishes unique batch attribution
        # from summing both standalone footprints without relying on timings.
        self.assertGreater(payload.stat().st_blocks, 0)
        self.assertLess(sum(sizes(batch)), sum(independent))
        self.assertGreater(sizes(batch)[0], sizes(batch)[1])
        self.assertGreater(sizes(reverse)[0], sizes(reverse)[1])

        result = self.run_doctor()
        self.assertEqual(result.returncode, 0, result.stderr)
        section = result.stdout.split("== devplane ==\n", 1)[1].split("\n== cargo env ==", 1)[0]
        note = "Batch du counts shared hardlinks once; per-path sizes depend on traversal order."
        self.assertEqual(section, note + "\n" + du(first, later, human=True))


if __name__ == "__main__":
    unittest.main()
