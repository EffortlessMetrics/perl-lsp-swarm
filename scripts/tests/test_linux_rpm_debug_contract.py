#!/usr/bin/env python3
"""Exercise the rendered RPM input guard with real ELF and native build routes."""
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class RpmDebugContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        for tool in ("cc", "readelf", "strip", "objcopy", "objdump", "rpmbuild"):
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
            sys.executable, str(ROOT / "scripts/render-linux-packages.py"),
            "--version", "0.17.0", "--download-base", "https://example.invalid/release",
            "--download-sha256", "0" * 64, "--arch", "x86_64",
            "--output-dir", str(output),
        ], check=True, capture_output=True)
        self.spec = (output / "dnf/perl-lsp.spec").read_text()
        build = re.search(r"(?ms)^%build[ \t]*\n(.*?)^%install[ \t]*$", self.spec)
        self.assertIsNotNone(build, "spec must have a %build section before %install")
        self.guard = build.group(1)
        self.payload = self.work / "payload"
        self.payload.mkdir()
        for name in ("perllsp", "perl-dap"):
            shutil.copy2(self.stripped, self.payload / name)

    def run_guard(self):
        return subprocess.run(["bash", "-eu", "-c", self.guard], cwd=self.payload,
                              text=True, capture_output=True)

    def run_native_build(self, route, debug_binary=None):
        case = self.work / f"native-{route}-{debug_binary or 'stripped'}"
        top = case / "top"
        source_dir = self.spec.split("%autosetup -n ", 1)[1].splitlines()[0]
        asset = Path(re.search(r"(?m)^Source0:\s+(\S+)", self.spec).group(1)).name
        payload = case / "payload"
        payload.mkdir(parents=True)
        for name in ("perllsp", "perl-dap"):
            shutil.copy2(self.debug if name == debug_binary else self.stripped, payload / name)
        (payload / "LICENSE-MIT").write_text("MIT test fixture\n")
        (payload / "LICENSE-APACHE").write_text("Apache test fixture\n")
        (payload / "README.md").write_text("RPM test fixture\n")

        for directory in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"):
            (top / directory).mkdir(parents=True, exist_ok=True)
        spec = re.sub(r"(?m)^\* Thu Jan 01 1970 ", "* Wed Jan 01 2025 ", self.spec)
        spec_path = top / "SPECS/perl-lsp.spec"
        spec_path.write_text(spec)
        archive_path = top / "SOURCES" / asset
        if route == "normal":
            with tarfile.open(archive_path, "w:gz") as archive:
                for path in payload.iterdir():
                    archive.add(path, arcname=f"{source_dir}/{path.name}", recursive=False)
            cwd = case
            source_hash = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        else:
            cwd = payload
            source_hash = {
                path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                for path in payload.iterdir()
            }

        # rpm is apt-installed in this Linux CI job, so RPM's database does
        # not know about the separately provisioned fixture tools.
        command = ["rpmbuild", "-bi", "--nodeps"]
        if route == "build-in-place":
            command.append("--build-in-place")
        command.extend(["--define", f"_topdir {top}", str(spec_path)])
        result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
        installed = {}
        for name in ("perllsp", "perl-dap"):
            paths = list(top.glob(f"BUILDROOT/*/usr/bin/{name}"))
            paths.extend(top.glob(f"BUILD/*/BUILDROOT/usr/bin/{name}"))
            installed[name] = paths
        if route == "normal":
            unchanged = hashlib.sha256(archive_path.read_bytes()).hexdigest() == source_hash
        else:
            unchanged = source_hash == {
                path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                for path in payload.iterdir()
            }
        return result, installed, unchanged

    def test_stripped_pair_passes_without_changing_input(self):
        sections = subprocess.check_output(["readelf", "-SW", str(self.stripped)], text=True)
        self.assertIn(".symtab", sections, "no-DWARF static symbols are valid inputs")
        self.assertIn(".dynsym", sections, "runtime dynamic symbols are valid inputs")
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
        for section in (".zdebug_info", ".gnu_debugdata", ".gnu_debuglink", ".gnu_debugaltlink", ".gdb_index"):
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

    def test_real_stabs_debug_is_refused_before_mutation(self):
        source = self.work / "stabs.c"
        source.write_text(r'''asm(".stabs \"fixture.c\",100,0,0,0\n"
    ".stabs \"main:F(0,1)\",36,0,0,main\n");
int main(void) { return 0; }
''')
        destination = self.payload / "perl-dap"
        subprocess.run(["cc", str(source), "-o", str(destination)], check=True)
        records = subprocess.check_output(["objdump", "--stabs", str(destination)], text=True)
        self.assertIn("fixture.c", records, "fixture must contain source records")
        self.assertIn("main:F(0,1)", records, "fixture must contain function records")
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

    def test_native_build_in_place_and_normal_routes_guard_before_install(self):
        for route in ("build-in-place", "normal"):
            with self.subTest(route=route, case="stripped"):
                result, installed, unchanged = self.run_native_build(route)
                output = result.stdout + result.stderr
                self.assertEqual(result.returncode, 0, output)
                self.assertIn("Executing(%build)", output)
                self.assertIn("Executing(%install)", output)
                if route == "normal":
                    self.assertIn("Executing(%prep)", output)
                for name, paths in installed.items():
                    self.assertEqual(len(paths), 1, f"expected one installed {name}: {output}")
                self.assertTrue(unchanged, "RPM must not mutate the release input")
                for name, paths in installed.items():
                    with self.subTest(route=route, case="stripped", binary=name):
                        self.assertNotIn(".debug_info", subprocess.check_output(
                            ["readelf", "-SW", str(paths[0])], text=True
                        ))

            for name in ("perllsp", "perl-dap"):
                with self.subTest(route=route, case="debug", binary=name):
                    result, installed, unchanged = self.run_native_build(route, name)
                    output = result.stdout + result.stderr
                    self.assertNotEqual(result.returncode, 0, output)
                    if route == "build-in-place":
                        self.assertIn("Executing(%build)", output)
                    self.assertIn("debug information", output)
                    self.assertNotIn("Executing(%install)", output)
                    self.assertEqual(installed, {"perllsp": [], "perl-dap": []}, output)
                    self.assertTrue(unchanged, "refusal must preserve the source input")


if __name__ == "__main__":
    unittest.main()
