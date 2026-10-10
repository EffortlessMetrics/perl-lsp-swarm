"""Execute the maintained selection CLI over fixed Git/Cargo subjects (#17485).

Pass --runner with the immutable executable produced by an admitted build.
No product source is compiled by these selection calls.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

RUNNER = None


class SelectionCli(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="selection fixture ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("GIT_", "GITHUB_"))}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "-q")
        self.git("config", "user.name", "Selection fixture")
        self.git("config", "user.email", "selection@example.invalid")
        self.git("remote", "add", "origin", "https://github.com/owner/repo.git")
        self.write("Cargo.toml", '[workspace]\nmembers=["crates/alpha", "crates/beta", "crates/perl-uri"]\nresolver="2"\n')
        for name in ("alpha", "beta", "perl-uri"):
            dependency = '\n[dependencies]\nalpha={path="../alpha"}\n' if name == "beta" else ""
            self.write(f"crates/{name}/Cargo.toml", f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n{dependency}')
            self.write(f"crates/{name}/src/lib.rs", 'compile_error!("selection must never compile product");\n')
        self.write("docs/start.md", "base\n")
        self.base = self.commit()

    def write(self, path, data):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(data)

    def git(self, *args):
        result = subprocess.run(["git", *args], cwd=self.root, env=self.env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def cli(self, *args, status=0):
        result = subprocess.run([RUNNER, *args, "--root", str(self.root)],
                                cwd=self.root, env=self.env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, status, result.stdout + result.stderr)
        return result

    def capture(self):
        receipt = self.root / "subject.json"
        self.cli("ci-subject", "--event-name", "explicit", "--repository", "owner/repo",
                 "--base-sha", self.base, "--head-sha", self.git("rev-parse", "HEAD"),
                 "--receipt", str(receipt))
        return receipt

    def scope(self, receipt):
        result = self.cli("ci-scope", "--subject", str(receipt), "--format", "json")
        self.assertTrue(result.stdout.endswith("\n"))
        return json.loads(result.stdout)

    def test_docs_nonempty_subject_preserves_receipt_identity(self):
        self.write("docs/start.md", "changed\n")
        head = self.commit()
        receipt = self.capture()
        identity = json.loads(receipt.read_text())
        self.assertEqual(identity["producer"], "cargo-xtask-ci-subject")
        self.assertEqual(identity["schema_version"], "ci-subject.v1")
        self.assertEqual(identity["status"], "RESOLVED")
        scope = self.scope(receipt)
        self.assertEqual(scope["head_sha"], head)
        self.assertEqual(scope["schema_version"], 2)
        self.assertEqual(scope["changed_files"], ["docs/start.md"])
        self.assertEqual(scope["diff_class"], "prose_only")
        self.assertFalse(scope["platform_overrides"]["windows_runner"])
        self.assertFalse(scope["direct_crates"])

    def test_code_reverse_dependency_and_windows_negative(self):
        self.write("crates/alpha/src/lib.rs", 'compile_error!("changed alpha");\n')
        self.commit()
        scope = self.scope(self.capture())
        self.assertEqual([r["name"] for r in scope["direct_crates"]], ["alpha"])
        self.assertIn("beta", [r["name"] for r in scope["reverse_dep_closure"]])
        self.assertFalse(scope["platform_overrides"]["windows_runner"])
        self.assertTrue(scope["selected_lanes"])

    def test_windows_sensitive_crate_selects_platform(self):
        self.write("crates/perl-uri/src/lib.rs", 'compile_error!("changed URI");\n')
        self.commit()
        scope = self.scope(self.capture())
        self.assertTrue(scope["platform_overrides"]["windows_runner"])
        self.assertIn("perl-uri", scope["platform_overrides"]["windows_test_crates"])

    def test_scope_refuses_stale_checkout_without_rewriting_receipt(self):
        self.write("docs/start.md", "changed\n")
        self.commit()
        receipt = self.capture()
        before = receipt.read_bytes()
        self.git("checkout", "-q", self.base)
        self.cli("ci-scope", "--subject", str(receipt), status=1)
        self.assertEqual(before, receipt.read_bytes())

    def test_scope_refuses_tampered_digest(self):
        self.write("docs/start.md", "changed\n")
        self.commit()
        receipt = self.capture()
        value = json.loads(receipt.read_text())
        value["changed_input_digest"] = "0" * 64
        receipt.write_text(json.dumps(value))
        before = receipt.read_bytes()
        self.cli("ci-scope", "--subject", str(receipt), status=1)
        self.assertEqual(before, receipt.read_bytes())

    def test_invalid_explicit_base_emits_failure_receipt(self):
        receipt = self.root / "failed.json"
        self.cli("ci-subject", "--event-name", "explicit", "--repository", "owner/repo",
                 "--base-sha", "bad-base", "--head-sha", self.base,
                 "--receipt", str(receipt), status=1)
        result = json.loads(receipt.read_text())
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertEqual(result["error_code"], "MALFORMED_SHA")

    @unittest.skipUnless(Path("/dev/full").exists(), "native POSIX write-failure control")
    def test_stdout_failure_preserves_legacy_exit(self):
        self.write("docs/start.md", "changed\n")
        self.commit()
        with open("/dev/full", "wb") as sink:
            result = subprocess.run([RUNNER, "ci-scope", "--base", self.base,
                                     "--root", str(self.root), "--format", "json"],
                                    cwd=self.root, env=self.env, stdout=sink,
                                    stderr=subprocess.PIPE, timeout=30)
        self.assertEqual(result.returncode, 101, result.stderr)

    def test_range_missing_base_refuses_and_text_fallback_remains(self):
        self.write("docs/start.md", "changed\n")
        self.commit()
        result = self.cli("ci-scope", "--base", "missing-base", status=1)
        self.assertEqual(result.stdout, "")
        result = self.cli("ci-scope", "--base", self.base, "--format", "legacy-text")
        self.assertIn("=== CI Scope Classifier (schema v2) ===", result.stdout)
        self.assertIn("Diff class: prose_only", result.stdout)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--runner", required=True)
    options, remaining = parser.parse_known_args()
    RUNNER = str(Path(options.runner).resolve())
    unittest.main(argv=[__file__, *remaining])
