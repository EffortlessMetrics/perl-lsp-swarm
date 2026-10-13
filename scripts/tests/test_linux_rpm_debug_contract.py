#!/usr/bin/env python3
"""Exercise the rendered RPM prep guard with real stripped and debug ELF inputs."""
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class RpmDebugContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        for tool in ("cc", "readelf", "strip", "objcopy"):
            self.assertIsNotNone(shutil.which(tool), f"required fixture tool: {tool}")
        source = self.work / "main.c"
        source.write_text("int main(void) { return 0; }\n")
        self.debug = self.work / "debug"
        subprocess.run(["cc", "-g", str(source), "-o", str(self.debug)], check=True)
        self.stripped = self.work / "stripped"
        shutil.copy2(self.debug, self.stripped)
        subprocess.run(["strip", "--strip-debug", str(self.stripped)], check=True)
        output = self.work / "rendered"
        subprocess.run([
            "python3", str(ROOT / "scripts/render-linux-packages.py"),
            "--version", "0.17.0", "--download-base", "https://example.invalid/release",
            "--download-sha256", "0" * 64, "--arch", "x86_64",
            "--output-dir", str(output),
        ], check=True, capture_output=True)
        self.spec = (output / "dnf/perl-lsp.spec").read_text()
        prep = self.spec.split("%prep\n", 1)[1].split("%build\n", 1)[0]
        # Native autosetup is covered by the package build; here supply its unpacked cwd.
        self.guard = "\n".join(line for line in prep.splitlines() if not line.startswith("%autosetup"))
        self.payload = self.work / "payload"
        self.payload.mkdir()
        for name in ("perllsp", "perl-dap"):
            shutil.copy2(self.stripped, self.payload / name)

    def run_guard(self):
        return subprocess.run(["bash", "-eu", "-c", self.guard], cwd=self.payload,
                              text=True, capture_output=True)

    def test_stripped_pair_passes_without_changing_input(self):
        before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.payload.iterdir()}
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(before, {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.payload.iterdir()})

    def test_debug_information_is_refused_for_each_binary_before_mutation(self):
        for name in ("perllsp", "perl-dap"):
            with self.subTest(binary=name):
                shutil.copy2(self.debug, self.payload / name)
                before = (self.payload / name).read_bytes()
                result = self.run_guard()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("debug information", result.stderr)
                self.assertEqual(before, (self.payload / name).read_bytes())
                shutil.copy2(self.stripped, self.payload / name)

    def test_additional_debug_section_markers_are_refused(self):
        for section in (".zdebug_info", ".gnu_debugdata", ".gnu_debuglink", ".gnu_debugaltlink"):
            with self.subTest(section=section):
                destination = self.payload / "perl-dap"
                shutil.copy2(self.stripped, destination)
                subprocess.run(["objcopy", "--add-section", f"{section}={self.debug}", str(destination)], check=True)
                before = destination.read_bytes()
                result = self.run_guard()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("debug information", result.stderr)
                self.assertEqual(before, destination.read_bytes())

    def test_real_compressed_dwarf_is_refused_before_mutation(self):
        destination = self.payload / "perl-dap"
        shutil.copy2(self.debug, destination)
        subprocess.run(["objcopy", "--compress-debug-sections=zlib-gnu", str(destination)], check=True)
        sections = subprocess.check_output(["readelf", "-SW", str(destination)], text=True)
        self.assertIn(".zdebug_info", sections, "fixture must contain compressed DWARF")
        before = destination.read_bytes()
        result = self.run_guard()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("debug information", result.stderr)
        self.assertEqual(before, destination.read_bytes())

    def test_invalid_and_missing_elf_are_refused(self):
        for content in (b"", b"not an ELF executable\n", None):
            with self.subTest(content=content):
                destination = self.payload / "perl-dap"
                if content is None:
                    destination.unlink()
                else:
                    destination.write_bytes(content)
                result = self.run_guard()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("readable ELF", result.stderr)

    def test_non_program_elf_is_refused(self):
        for arguments in (("-c",), ("-shared", "-fPIC")):
            with self.subTest(arguments=arguments):
                destination = self.payload / "perl-dap"
                subprocess.run(["cc", *arguments, str(self.work / "main.c"), "-o", str(destination)], check=True)
                subprocess.run(["strip", "--strip-debug", str(destination)], check=True)
                result = self.run_guard()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("readable ELF", result.stderr)

    def test_native_rpm_debug_macro_disposition_and_deleted_control(self):
        self.assertIsNotNone(shutil.which("rpmspec"), "required native RPM parser")

        def packages(spec):
            probe = self.work / "macro-probe.spec"
            # RPM 4.20 creates this spec part at install time. Explicitly invoke
            # the configured native macro as an oracle; this is not a build proof.
            probe.write_text(spec + "\n%debug_package\n")
            result = subprocess.run(["rpmspec", "-q", "--rpms", "--qf", "%{NAME}\\n", str(probe)],
                                    check=True, text=True, capture_output=True)
            return result.stdout.splitlines()

        self.assertEqual(packages(self.spec), ["perl-lsp"])
        deleted = re.sub(r"^%global debug_package .*\n", "", self.spec, flags=re.MULTILINE)
        self.assertNotEqual(deleted, self.spec, "macro deletion control must change the spec")
        self.assertIn("perl-lsp-debuginfo", packages(deleted))


if __name__ == "__main__":
    unittest.main()
