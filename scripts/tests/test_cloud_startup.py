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
        repo = root / "repo"
        shared = root / "shared"
        fake = root / "bin"
        active_node = root / "active-node/bin"
        stale_node = shared / "node-v26.6.0-linux-x64/bin"
        for path in (
            repo / "vscode-extension",
            repo / "scripts",
            shared / "cargo/bin",
            fake,
            active_node,
            stale_node,
        ):
            path.mkdir(parents=True)
        (repo / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.96.0"\n', encoding="utf-8"
        )
        log = root / "calls.jsonl"
        driver = root / "tool-driver"
        driver.write_text(
            '''#!/usr/bin/python3
import json
import os
import pathlib
import sys

name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ["SETUP_LOG"], "a", encoding="utf-8") as stream:
    stream.write(json.dumps({
        "name": name,
        "args": args,
        "executable": sys.argv[0],
        "toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
        "floor": os.environ.get("MIN_FREE_GB"),
        "jobs": os.environ.get("CARGO_BUILD_JOBS"),
        "auto_install": os.environ.get("RUSTUP_AUTO_INSTALL"),
        "path": os.environ.get("PATH"),
    }) + "\\n")
if os.environ.get("SETUP_STABLE_ONLY") and name in {"rustup", "rustc", "cargo"}:
    if os.environ.get("RUSTUP_TOOLCHAIN") != "stable":
        sys.exit(86)
if os.environ.get("SETUP_PIN_MISSING") and name in {"rustup", "rustc", "cargo"}:
    if not os.environ.get("RUSTUP_TOOLCHAIN"):
        sys.exit(87)
if name == "rustup" and args == ["show", "active-toolchain"]:
    selected = os.environ.get("RUSTUP_TOOLCHAIN") or "1.96.0"
    print(f"{selected}-x86_64-unknown-linux-gnu (selected)")
if name == "rustc":
    version = "1.99.1" if os.environ.get("RUSTUP_TOOLCHAIN") == "stable" else "1.96.0"
    print(f"rustc {version}")
if name == "cargo":
    version = "1.99.1" if os.environ.get("RUSTUP_TOOLCHAIN") == "stable" else "1.96.0"
    print(f"cargo {version}")
if name == "just" and args == ["--version"]:
    print("just 1.58.0")
if name == "node":
    print(os.environ.get("SETUP_NODE_VERSION", "v26.13.0"))
if name == "npm" and args == ["--version"]:
    print(os.environ.get("SETUP_NPM_VERSION", "11.18.0"))
if name == "npm" and args == ["run", "doctor"]:
    node = os.environ.get("SETUP_NODE_VERSION", "v26.13.0")
    npm = os.environ.get("SETUP_NPM_VERSION", "11.18.0")
    if not node.startswith("v26.") or npm != "11.18.0" or os.environ.get("SETUP_FAIL_DOCTOR"):
        sys.exit(23)
''',
            encoding="utf-8",
        )
        driver.chmod(0o755)

        for name in ("rustup", "rustc", "cargo", "just"):
            (shared / "cargo/bin" / name).symlink_to(driver)
        (shared / "cargo/bin/python3").symlink_to(sys.executable)
        for name in ("node", "npm"):
            (active_node / name).symlink_to(driver)

        stale_marker = root / "stale-node-selected"
        stale_driver = '''#!/bin/sh
: > "$SETUP_STALE_NODE_MARKER"
exit 88
'''
        for name in ("node", "npm"):
            executable = stale_node / name
            executable.write_text(stale_driver, encoding="utf-8")
            executable.chmod(0o755)

        (repo / "scripts/check_release_tag_provenance.py").write_text(
            'import os, json, sys\n'
            'with open(os.environ["SETUP_LOG"], "a", encoding="utf-8") as stream:\n'
            '    stream.write(json.dumps({"name": "history", "args": sys.argv[1:], '
            '"toolchain": os.environ.get("RUSTUP_TOOLCHAIN")}) + "\\n")\n',
            encoding="utf-8",
        )
        source = SCRIPT.read_text(encoding="utf-8")
        if os.environ.get("SETUP_UNDER_TEST"):
            source = source.replace("/workspace/perl-lsp-swarm", str(repo))
            source = source.replace("/workspace/shared", str(shared))
        candidate = repo / "scripts/cloud-startup"
        candidate.write_text(source, encoding="utf-8")
        candidate.chmod(0o755)

        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("CARGO_", "RUSTUP_"))
            and key not in ("MIN_FREE_GB", "MAX_USED_PCT")
        }
        env.update(
            PATH=f"{active_node}:{fake}",
            SETUP_LOG=str(log),
            SETUP_STALE_NODE_MARKER=str(stale_marker),
            PERL_LSP_CLOUD_SHARED=str(shared),
        )
        cls.repo = repo
        cls.candidate = candidate
        cls.env = env
        cls.log = log
        cls.active_node = active_node
        cls.stale_marker = stale_marker
        cls.result = subprocess.run(
            ["/bin/bash", str(candidate)],
            cwd=repo,
            env=env,
            text=True,
            encoding="utf-8",
            capture_output=True,
            timeout=15,
        )
        cls.calls = [
            json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()
        ]

    def run_with_pin(self, contents, extra_env=None):
        pin = self.repo / "rust-toolchain.toml"
        previous = pin.read_bytes()
        log = self.log.with_name("alternate.jsonl")
        log.unlink(missing_ok=True)
        self.stale_marker.unlink(missing_ok=True)
        try:
            if contents is None:
                pin.unlink()
            else:
                pin.write_text(contents, encoding="utf-8")
            result = subprocess.run(
                ["/bin/bash", str(self.candidate)],
                cwd=self.repo,
                env=dict(self.env, SETUP_LOG=str(log), **(extra_env or {})),
                text=True,
                encoding="utf-8",
                capture_output=True,
                timeout=15,
            )
            calls = [
                json.loads(line)
                for line in log.read_text(encoding="utf-8").splitlines()
            ] if log.exists() else []
            return result, calls
        finally:
            pin.write_bytes(previous)

    def test_setup_completes_with_repository_default_unset_in_environment(self):
        self.assertEqual(self.result.returncode, 0, self.result.stderr)
        self.assertEqual(
            [(c["name"], c["args"]) for c in self.calls],
            [
                ("rustup", ["show", "active-toolchain"]),
                ("rustc", ["--version"]),
                ("cargo", ["--version"]),
                ("node", ["--version"]),
                ("npm", ["--version"]),
                ("just", ["--version"]),
                ("history", ["--prepare-git", "--repo-root", "."]),
                ("just", ["devplane-init"]),
                ("just", ["agent-preflight"]),
                ("npm", ["run", "doctor"]),
                ("npm", ["ci"]),
            ],
        )
        self.assertTrue(
            all(c.get("toolchain") is None for c in self.calls),
            "startup manufactured RUSTUP_TOOLCHAIN instead of using repository resolution",
        )
        self.assertIn(
            "rust-toolchain source=rustup-resolution selector=<unset> repository-default=1.96.0",
            self.result.stdout,
        )
        self.assertIn(
            "1.96.0-x86_64-unknown-linux-gnu (selected)", self.result.stdout
        )
        self.assertTrue(
            all(c.get("auto_install") == "0" for c in self.calls if "auto_install" in c)
        )

    def test_explicit_stable_survives_every_child_without_installing_the_pin(self):
        result, calls = self.run_with_pin(
            '[toolchain]\nchannel = "1.96.0"\n',
            {"RUSTUP_TOOLCHAIN": "stable", "SETUP_STABLE_ONLY": "1"},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(calls)
        self.assertTrue(all(c.get("toolchain") == "stable" for c in calls))
        self.assertIn(
            "rust-toolchain source=environment selector=stable repository-default=1.96.0",
            result.stdout,
        )
        self.assertIn("rustc 1.99.1", result.stdout)
        rustup_args = [c["args"] for c in calls if c["name"] == "rustup"]
        self.assertEqual(rustup_args, [["show", "active-toolchain"]])

    def test_second_pin_is_reported_without_overriding_explicit_selection(self):
        result, calls = self.run_with_pin(
            '# pin caf\u00e9\n[toolchain]\nchannel = "1.97.0"\n',
            {
                "RUSTUP_TOOLCHAIN": "stable",
                "SETUP_STABLE_ONLY": "1",
                "PYTHONUTF8": "0",
                "PYTHONCOERCECLOCALE": "0",
                "LC_ALL": "C",
                "LANG": "C",
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(calls)
        self.assertTrue(all(c.get("toolchain") == "stable" for c in calls))
        self.assertIn("repository-default=1.97.0", result.stdout)

    def test_invalid_or_missing_pin_stops_before_tool_or_setup_calls(self):
        for contents in (None, "[toolchain]\nchannel = [\n"):
            with self.subTest(contents=contents):
                result, calls = self.run_with_pin(
                    contents, {"RUSTUP_TOOLCHAIN": "stable", "SETUP_STABLE_ONLY": "1"}
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])

    def test_missing_repository_selected_toolchain_fails_before_setup(self):
        result, calls = self.run_with_pin(
            '[toolchain]\nchannel = "1.96.0"\n', {"SETUP_PIN_MISSING": "1"}
        )
        self.assertEqual(result.returncode, 87)
        self.assertEqual(
            [(c["name"], c["args"]) for c in calls],
            [("rustup", ["show", "active-toolchain"])],
        )
        self.assertEqual(calls[0]["auto_install"], "0")
        self.assertFalse(any(c["name"] == "history" for c in calls))
        self.assertFalse(any(c["name"] == "npm" for c in calls))

    def test_repeated_startup_does_not_mutate_repository_or_toolchain_state(self):
        def snapshot():
            return {
                str(path.relative_to(self.repo)): path.read_bytes()
                for path in self.repo.rglob("*")
                if path.is_file()
            }

        before = snapshot()
        first, first_calls = self.run_with_pin(
            '[toolchain]\nchannel = "1.96.0"\n',
            {"RUSTUP_TOOLCHAIN": "stable", "SETUP_STABLE_ONLY": "1"},
        )
        middle = snapshot()
        second, second_calls = self.run_with_pin(
            '[toolchain]\nchannel = "1.96.0"\n',
            {"RUSTUP_TOOLCHAIN": "stable", "SETUP_STABLE_ONLY": "1"},
        )
        after = snapshot()
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(before, middle)
        self.assertEqual(middle, after)
        for calls in (first_calls, second_calls):
            rustup_args = [c["args"] for c in calls if c["name"] == "rustup"]
            self.assertEqual(rustup_args, [["show", "active-toolchain"]])

    def test_active_node_wins_over_stale_shared_patch_directory(self):
        self.assertEqual(self.result.returncode, 0, self.result.stderr)
        self.assertFalse(self.stale_marker.exists())
        node = next(c for c in self.calls if c["name"] == "node")
        npm = next(c for c in self.calls if c["name"] == "npm")
        self.assertTrue(node["executable"].startswith(str(self.active_node)))
        self.assertTrue(npm["executable"].startswith(str(self.active_node)))
        self.assertFalse(
            any(
                "node-v26.6.0-linux-x64/bin" in element
                for element in node["path"].split(":")
            )
        )

    def test_wrong_node_or_npm_stops_before_npm_ci(self):
        cases = (
            {"SETUP_NODE_VERSION": "v25.9.0"},
            {"SETUP_NPM_VERSION": "11.17.0"},
            {"SETUP_FAIL_DOCTOR": "1"},
        )
        for extra in cases:
            with self.subTest(extra=extra):
                result, calls = self.run_with_pin(
                    '[toolchain]\nchannel = "1.96.0"\n', extra
                )
                self.assertEqual(result.returncode, 23)
                self.assertEqual(calls[-1]["name"], "npm")
                self.assertEqual(calls[-1]["args"], ["run", "doctor"])
                self.assertFalse(
                    any(c["name"] == "npm" and c["args"] == ["ci"] for c in calls)
                )

    def test_missing_node_fails_actionably_without_searching_checkout(self):
        node = self.active_node / "node"
        saved = node.readlink()
        poison = self.repo / "node"
        marker = self.repo / "cwd-node-ran"
        poison.write_text('#!/bin/sh\n: > cwd-node-ran\nexit 0\n', encoding="utf-8")
        poison.chmod(0o755)
        node.unlink()
        try:
            result, calls = self.run_with_pin('[toolchain]\nchannel = "1.96.0"\n')
        finally:
            node.symlink_to(saved)
            poison.unlink()
        self.assertEqual(result.returncode, 127)
        self.assertIn("required command not found on PATH: node", result.stderr)
        self.assertFalse(marker.exists())
        self.assertFalse(any(c["name"] == "history" for c in calls))

    def test_setup_preserves_capacity_jobs_and_operation_order(self):
        self.assertTrue(
            all(
                c["floor"] is None and c["jobs"] is None
                for c in self.calls
                if "floor" in c
            )
        )
        history = next(i for i, c in enumerate(self.calls) if c["name"] == "history")
        init = next(
            i
            for i, c in enumerate(self.calls)
            if c["name"] == "just" and c["args"] == ["devplane-init"]
        )
        preflight = next(
            i
            for i, c in enumerate(self.calls)
            if c["name"] == "just" and c["args"] == ["agent-preflight"]
        )
        doctor = next(
            i
            for i, c in enumerate(self.calls)
            if c["name"] == "npm" and c["args"] == ["run", "doctor"]
        )
        install = next(
            i
            for i, c in enumerate(self.calls)
            if c["name"] == "npm" and c["args"] == ["ci"]
        )
        self.assertLess(history, init)
        self.assertLess(init, preflight)
        self.assertLess(preflight, doctor)
        self.assertLess(doctor, install)


if __name__ == "__main__":
    unittest.main(verbosity=2)
