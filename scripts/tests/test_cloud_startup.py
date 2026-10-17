"""Execute the repository cloud startup with disposable tools, no installs/network."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(os.environ.get("SETUP_UNDER_TEST", ROOT / "scripts/cloud-startup"))


class SetupTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.temp.cleanup)
        root = Path(cls.temp.name)
        repo, shared, fake = root / "repo", root / "shared", root / "bin"
        for path in (
            repo / "vscode-extension", repo / "scripts", shared / "cargo/bin",
            shared / "node-v26.6.0-linux-x64/bin", fake,
        ):
            path.mkdir(parents=True)
        (repo / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.96.0"\n', encoding="utf-8"
        )
        log = root / "calls.jsonl"
        driver = '''#!/usr/bin/python3
import json
import os
import pathlib
import sys

name = pathlib.Path(sys.argv[0]).name
with open(os.environ["SETUP_LOG"], "a", encoding="utf-8") as stream:
    stream.write(json.dumps({
        "name": name, "args": sys.argv[1:],
        "toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
        "floor": os.environ.get("MIN_FREE_GB"),
        "jobs": os.environ.get("CARGO_BUILD_JOBS"),
        "auto_install": os.environ.get("RUSTUP_AUTO_INSTALL"),
        "path": os.environ.get("PATH"),
    }) + "\\n")
if name == "npm" and sys.argv[1:] == ["run", "doctor"] and os.environ.get("SETUP_FAIL_DOCTOR"):
    sys.exit(23)
if name == "just" and sys.argv[1:] == ["--version"]:
    print("just 1.58.0")
if name == "node":
    print("v26.6.0")
if name == "npm" and sys.argv[1:] == ["--version"]:
    print("11.18.0")
if name == "curl":
    dest = pathlib.Path(sys.argv[sys.argv.index("-o") + 1])
    dest.write_text("0" * 64 + "  node-v26.6.0-linux-x64.tar.xz\\n", encoding="utf-8")
'''
        for name in ("rustup", "rustc", "cargo", "just", "node", "npm", "curl", "tar", "sha256sum"):
            executable = fake / name
            executable.write_text(driver, encoding="utf-8")
            executable.chmod(0o755)
        for name in ("rustup", "rustc", "cargo", "just", "node", "npm"):
            (shared / "cargo/bin" / name).symlink_to(fake / name)
        (shared / "cargo/bin/python3").symlink_to(sys.executable)
        (repo / "scripts/check_release_tag_provenance.py").write_text(
            'import os, json, sys\n'
            'with open(os.environ["SETUP_LOG"], "a", encoding="utf-8") as stream:\n'
            '    stream.write(json.dumps({"name": "history", "args": sys.argv[1:]}) + "\\n")\n',
            encoding="utf-8",
        )
        source = SCRIPT.read_text(encoding="utf-8")
        # The override also supports the historical host script as a RED control.
        if os.environ.get("SETUP_UNDER_TEST"):
            source = source.replace("/workspace/perl-lsp-swarm", str(repo))
            source = source.replace("/workspace/shared", str(shared))
        candidate = repo / "scripts/cloud-startup"
        candidate.write_text(source, encoding="utf-8")
        env = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("CARGO_", "RUSTUP_"))
            and key not in ("MIN_FREE_GB", "MAX_USED_PCT")
        }
        env.update(
            PATH=str(fake) + ":" + env["PATH"], SETUP_LOG=str(log),
            PERL_LSP_CLOUD_SHARED=str(shared),
        )
        cls.repo, cls.candidate, cls.env, cls.log = repo, candidate, env, log
        cls.result = subprocess.run(
            ["/bin/bash", str(candidate)], cwd=repo, env=env,
            text=True, encoding="utf-8", capture_output=True, timeout=15,
        )
        cls.calls = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def run_with_pin(self, contents, extra_env=None):
        pin = self.repo / "rust-toolchain.toml"
        previous = pin.read_bytes()
        log = self.log.with_name("alternate.jsonl")
        log.unlink(missing_ok=True)
        try:
            if contents is None:
                pin.unlink()
            else:
                pin.write_text(contents, encoding="utf-8")
            result = subprocess.run(
                ["/bin/bash", str(self.candidate)], cwd=self.repo,
                env=dict(self.env, SETUP_LOG=str(log), **(extra_env or {})),
                text=True, encoding="utf-8", capture_output=True, timeout=15,
            )
            calls = [
                json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()
            ] if log.exists() else []
            return result, calls
        finally:
            pin.write_bytes(previous)

    def test_second_pin_discriminates_hardcoded_toolchain(self):
        result, calls = self.run_with_pin(
            '# pin caf\u00e9\n[toolchain]\nchannel = "1.97.0"\n',
            {"PYTHONUTF8": "0", "PYTHONCOERCECLOCALE": "0", "LC_ALL": "C", "LANG": "C"},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(calls)
        self.assertTrue(all(c["toolchain"] == "1.97.0" for c in calls if "toolchain" in c))

    def test_invalid_or_missing_pin_stops_before_provisioning(self):
        for contents in (None, "[toolchain]\nchannel = [\n"):
            with self.subTest(contents=contents):
                result, calls = self.run_with_pin(contents)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [], "tool provisioning continued after pin resolution failed")

    def test_setup_completes(self):
        self.assertEqual(self.result.returncode, 0, self.result.stderr)
        self.assertEqual([(c["name"], c["args"]) for c in self.calls], [
            ("rustc", ["--version"]), ("cargo", ["--version"]), ("just", ["--version"]),
            ("history", ["--prepare-git", "--repo-root", "."]),
            ("just", ["devplane-init"]), ("just", ["agent-preflight"]),
            ("npm", ["run", "doctor"]), ("npm", ["ci"]),
        ])
        self.assertTrue(all(c["auto_install"] == "0" for c in self.calls if "auto_install" in c))
        with self.subTest(doctor_failure=True):
            result, calls = self.run_with_pin(
                '[toolchain]\nchannel = "1.96.0"\n', {"SETUP_FAIL_DOCTOR": "1"}
            )
            self.assertEqual(result.returncode, 23)
            self.assertEqual(calls[-1]["name"], "npm")
            self.assertEqual(calls[-1]["args"], ["run", "doctor"])
            self.assertFalse(any(c["name"] == "npm" and c["args"] == ["ci"] for c in calls))
        with self.subTest(empty_path=True):
            # Poison an external command needed by the old bootstrap before PATH setup.
            marker = self.repo / "cwd-command-ran"
            poison = self.repo / "dirname"
            poison.write_text(
                '#!/bin/sh\n: > cwd-command-ran\nexec /usr/bin/dirname "$@"\n',
                encoding="utf-8",
            )
            poison.chmod(0o755)
            result, calls = self.run_with_pin(
                '[toolchain]\nchannel = "1.96.0"\n', {"PATH": ""}
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(marker.exists(), "startup searched the caller cwd before configuring PATH")
            self.assertTrue(calls)
            self.assertTrue(all("" not in c["path"].split(":") for c in calls if "path" in c))

    def test_refreshed_repository_pin_is_the_only_toolchain_selected(self):
        self.assertTrue(self.calls)
        self.assertTrue(all(c["toolchain"] == "1.96.0" for c in self.calls if "toolchain" in c))
        self.assertFalse(
            any(c["name"] == "rustup" for c in self.calls),
            "startup must not provision or change the global toolchain",
        )

    def test_setup_preserves_default_capacity_and_job_policy(self):
        self.assertTrue(all(
            c["floor"] is None and c["jobs"] is None for c in self.calls if "floor" in c
        ))

    def test_history_preparation_precedes_existing_setup_preflight(self):
        self.assertTrue(any(c["name"] == "history" for c in self.calls))
        history = next(i for i, c in enumerate(self.calls) if c["name"] == "history")
        init = next(i for i, c in enumerate(self.calls) if c["name"] == "just" and c["args"] == ["devplane-init"])
        preflight = next(i for i, c in enumerate(self.calls) if c["name"] == "just" and c["args"] == ["agent-preflight"])
        self.assertEqual(self.calls[history]["args"], ["--prepare-git", "--repo-root", "."])
        self.assertLess(history, init)
        self.assertLess(init, preflight)


if __name__ == "__main__":
    unittest.main(verbosity=2)
