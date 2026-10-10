"""Execute the maintained selection CLI over fixed Git/Cargo subjects (#17485).

Pass --runner with the immutable executable produced by an admitted build.
No product source is compiled by these selection calls.
For isolated qualification, --compare-runner accepts a separately admitted real
xtask binary and checks identical subjects, stdout, receipts, errors and statuses.
CI uses the lightweight runner alone; this script never builds either caller.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

RUNNER = None
COMPARE_RUNNER = None
CALLER_ROOT = None


def error_payload(text):
    """Compare error contents across the two existing top-level renderers.

    xtask's Result main renders eyre Debug (Error/numbered chain/location),
    while hygiene renders Display alternate. Keep every message and warning;
    discard only that established presentation and source-location footer.
    """
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    # Accept only the observed terminal footer from the shared selection owner.
    # An extra message after this block must remain visible and fail comparison.
    text = re.sub(
        r"\n\nLocation:\n   crates/perl-ci-hygiene/src/"
        r"(?:ci_subject|ci_scope|change_set)\.rs:\d+(?::\d+)?\n\n"
        r"Backtrace omitted\. Run with RUST_BACKTRACE=1 environment variable to display it\.\n"
        r"Run with RUST_BACKTRACE=full to include source snippets\.\n?\Z", "", text)
    text = re.sub(r"(?m)^Error:\s*", "", text)
    text = re.sub(r"(?m)^\s*\d+: ", "", text)
    return text.strip()


class SelectionCli(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="selection fixture ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("GIT_", "GITHUB_"))}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                        RUST_BACKTRACE="0", RUST_LIB_BACKTRACE="0")
        self.git("init", "-q")
        self.git("config", "user.name", "Selection fixture")
        self.git("config", "user.email", "selection@example.invalid")
        self.git("remote", "add", "origin", "https://github.com/owner/repo.git")
        self.write("Cargo.toml", '[workspace]\nmembers=["crates/alpha", "crates/beta", "crates/perl-uri", "crates/perl-ci-hygiene"]\nresolver="2"\n')
        for name in ("alpha", "beta", "perl-uri", "perl-ci-hygiene"):
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
        if COMPARE_RUNNER:
            receipt = self.root / args[args.index("--receipt") + 1] if "--receipt" in args else None
            before = receipt.read_bytes() if receipt and receipt.exists() else None
            if receipt and receipt.exists():
                receipt.unlink()  # legacy must produce its own receipt, not reuse ours.
            legacy = subprocess.run([COMPARE_RUNNER, *args, "--root", str(self.root)],
                                    cwd=self.root, env=self.env,
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(legacy.returncode, result.returncode, legacy.stderr)
            self.assertEqual(legacy.stdout, result.stdout)
            self.assertEqual(error_payload(legacy.stderr), error_payload(result.stderr))
            if receipt:
                self.assertEqual(receipt.read_bytes() if receipt.exists() else None, before)
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

    def test_empty_range_keeps_an_empty_scope(self):
        scope = self.scope(self.capture())
        self.assertEqual(scope["changed_files"], [])
        self.assertFalse(scope["direct_crates"])

    def test_auto_base_keeps_the_same_nonempty_range(self):
        self.write("docs/start.md", "changed\n")
        self.commit()
        scope = json.loads(self.cli("ci-scope", "--format", "json").stdout)
        self.assertEqual(scope["changed_files"], ["docs/start.md"])
        self.assertEqual(self.git("rev-parse", scope["base"]), self.base)

    def test_callers_default_root_matches_at_repository_root(self):
        if not COMPARE_RUNNER or not CALLER_ROOT:
            self.skipTest("requires real pair and their source checkout")
        results = [subprocess.run([runner, "ci-scope", "--base", "HEAD", "--format", "json"],
                                  cwd=CALLER_ROOT, env=self.env, capture_output=True,
                                  text=True, timeout=30)
                   for runner in (RUNNER, COMPARE_RUNNER)]
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(results[0].stdout, results[1].stdout)
        self.assertEqual(results[0].stderr, results[1].stderr)
        self.assertEqual(json.loads(results[0].stdout)["changed_files"], [])

    def test_push_subject_flags_and_environment_match(self):
        self.write("docs/start.md", "changed\n")
        head = self.commit()
        event = self.root / "push.json"
        event.write_text(json.dumps({"before": self.base, "after": head,
                                    "repository": {"full_name": "owner/repo"}}))
        receipt = self.root / "subject.json"
        flags = self.cli("ci-subject", "--event-name", "push", "--event-path", str(event),
                         "--repository", "owner/repo", "--github-sha", head,
                         "--receipt", str(receipt))
        explicit = receipt.read_bytes()
        self.env.update(GITHUB_EVENT_NAME="push", GITHUB_EVENT_PATH=str(event),
                        GITHUB_REPOSITORY="owner/repo", GITHUB_SHA=head)
        inherited = self.cli("ci-subject", "--receipt", str(receipt))
        self.assertEqual(flags.stdout, inherited.stdout)
        self.assertEqual(receipt.read_bytes(), explicit)

    def test_push_subject_refuses_contradictory_github_sha(self):
        self.write("docs/start.md", "changed\n")
        head = self.commit()
        event = self.root / "push.json"
        event.write_text(json.dumps({"before": self.base, "after": head,
                                    "repository": {"full_name": "owner/repo"}}))
        receipt = self.root / "failed.json"
        self.cli("ci-subject", "--event-name", "push", "--event-path", str(event),
                 "--repository", "owner/repo", "--github-sha", self.base,
                 "--receipt", str(receipt), status=1)
        self.assertEqual(json.loads(receipt.read_text())["status"], "NOT_PROVEN")

    def test_windows_sensitive_crate_selects_platform(self):
        self.write("crates/perl-uri/src/lib.rs", 'compile_error!("changed URI");\n')
        self.commit()
        scope = self.scope(self.capture())
        self.assertTrue(scope["platform_overrides"]["windows_runner"])
        self.assertIn("perl-uri", scope["platform_overrides"]["windows_test_crates"])

    def test_relocated_classifier_retains_parser_ratchet_selection(self):
        self.write("crates/perl-ci-hygiene/src/ci_scope.rs", "// classifier change\n")
        self.commit()
        scope = self.scope(self.capture())
        self.assertTrue(scope["lanes"]["parser_ratchet"]["selected"])
        self.assertIn("changed_path:crates/perl-ci-hygiene/src/ci_scope.rs",
                      scope["lanes"]["parser_ratchet"]["reasons"])

    def test_unrelated_hygiene_sibling_keeps_parser_ratchet_unselected(self):
        self.write("crates/perl-ci-hygiene/src/version_sync.rs", "// sibling change\n")
        self.commit()
        scope = self.scope(self.capture())
        self.assertFalse(scope["lanes"]["parser_ratchet"]["selected"])

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
        for runner in filter(None, (RUNNER, COMPARE_RUNNER)):
            with open("/dev/full", "wb") as sink:
                result = subprocess.run([runner, "ci-scope", "--base", self.base,
                                         "--root", str(self.root), "--format", "json"],
                                        cwd=self.root, env=self.env, stdout=sink,
                                        stderr=subprocess.PIPE, timeout=30)
            self.assertEqual(result.returncode, 101, result.stderr)
            self.assertIn(b"failed printing to stdout", result.stderr)

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
    parser.add_argument("--compare-runner", help="Optional immutable real xtask executable; never built by this test")
    parser.add_argument("--caller-root", help="Optional source checkout to exercise both existing default-root entry points")
    options, remaining = parser.parse_known_args()
    RUNNER = str(Path(options.runner).resolve())
    COMPARE_RUNNER = str(Path(options.compare_runner).resolve()) if options.compare_runner else None
    CALLER_ROOT = str(Path(options.caller_root).resolve()) if options.caller_root else None
    unittest.main(argv=[__file__, *remaining])
