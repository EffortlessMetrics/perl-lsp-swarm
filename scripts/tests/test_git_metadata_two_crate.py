#!/usr/bin/env python3
"""Independent, opt-in two-crate Cargo regression for LSP Git metadata watches.

GIT_METADATA_REAL_BUILD=1 GIT_METADATA_PROOF_ROOT=/absolute/new/path \
  python3 scripts/tests/test_git_metadata_two_crate.py -v

Copies the production build.rs byte-for-byte into a dependency-free library;
a second crate consumes its compile-time identity. No repository Rust build.
All fixture sources, private outputs, admission receipts and logs are retained.
The caller must admit the sequential workload and select an installed toolchain.
GIT_METADATA_BUILD_SCRIPT can select a historical script for a red control.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest

REPO = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.environ.get("GIT_METADATA_REAL_BUILD") == "1", "opt-in real Cargo fixture")
class GitMetadataTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.proof = Path(os.environ["GIT_METADATA_PROOF_ROOT"]).resolve()
        cls.proof.mkdir(parents=True, exist_ok=False)
        cls.script = Path(os.environ.get("GIT_METADATA_BUILD_SCRIPT", REPO / "crates/perl-lsp-rs/build.rs"))
        cls.env = dict(os.environ, DEVPLANE=str(cls.proof / "resources"))
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "MIN_FREE_GB", "MAX_USED_PCT",
                    "CARGO_STORAGE_POLICY", "CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE"):
            cls.env.pop(key, None)
        cls.rows = []
        cls.wrapper = [sys.executable, str(REPO / "scripts/cargo_admitted.py")]

    def command(self, root, *args):
        run = subprocess.run(args, cwd=root, env=self.env, capture_output=True, text=True,
                             encoding="utf-8", errors="backslashreplace", timeout=60)
        self.assertEqual(run.returncode, 0, (args, run.stdout, run.stderr))
        return run.stdout.strip()

    def git(self, root, *args):
        return self.command(root, "git", *args)

    def fixture(self, name):
        root = self.proof / name
        for crate in ("identity", "consumer"):
            (root / "crates" / crate / "src").mkdir(parents=True)
        (root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/identity", "crates/consumer"]\nresolver="2"\n')
        (root / "crates/identity/Cargo.toml").write_text('[package]\nname="metadata-identity"\nversion="0.1.0"\nedition="2021"\n')
        (root / "crates/identity/build.rs").write_bytes(self.script.read_bytes())
        (root / "crates/identity/src/lib.rs").write_text(
            'pub const ID: &str = concat!(env!("BUILD_REVISION_KIND"), " ", env!("BUILD_REVISION"));\n')
        (root / "crates/consumer/Cargo.toml").write_text(
            '[package]\nname="metadata-consumer"\nversion="0.1.0"\nedition="2021"\n'
            '[dependencies]\nmetadata-identity={path="../identity"}\n')
        (root / "crates/consumer/src/main.rs").write_text(
            'fn main() { println!("REVISION={}", metadata_identity::ID); }\n')
        self.git(root, "init", "-q", "-b", "fixture")
        self.commit(root)
        return root

    def commit(self, root):
        self.git(root, "add", "Cargo.toml", "crates")
        self.git(root, "-c", "user.name=Metadata Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-qm", "fixture", "--allow-empty")

    def revision(self, root):
        return "commit " + self.git(root, "rev-parse", "--short", "HEAD")

    def build(self, root, label, fresh, expected):
        args = ["run", "-p", "metadata-consumer", "--offline", "--message-format=json", "-vv"]
        pre = subprocess.run([*self.wrapper, "--preflight", *args], cwd=root, env=self.env,
                             capture_output=True, text=True, timeout=60)
        (root / f"{label}.preflight.log").write_text(pre.stdout + pre.stderr)
        reports = [json.loads(line.split(": ", 1)[1]) for line in pre.stderr.splitlines()
                   if line.startswith("cargo-admitted preflight:")]
        self.assertEqual(len(reports), 1, pre.stderr)
        if pre.returncode:
            self.assertEqual(pre.returncode, 75, pre.stderr)
            self.assertIn("insufficient disk headroom", reports[0].get("reason", ""), pre.stderr)
        budget = root / "fixture-budget.json"
        budget.write_text(json.dumps(dict(schema_version=1, scope=reports[0]["scope"],
            reserve_bytes=8 * 1024**3, expected_growth_bytes=1024**3,
            basis="Operator fixture sizing: two dependency-free tiny crates, one production build script, installed toolchain, offline. Sequential owner; 1 GiB additional peak allowance, 8 GiB reserve. Not a repository workload envelope.")))
        run = subprocess.run([*self.wrapper, "--budget-file", str(budget), *args], cwd=root,
                             env=self.env, capture_output=True, text=True,
                             encoding="utf-8", errors="backslashreplace", timeout=60)
        (root / f"{label}.log").write_text(run.stdout + run.stderr)
        self.assertEqual(run.returncode, 0, run.stderr[-6000:])
        artifacts = {}
        for line in run.stdout.splitlines():
            if line.startswith('{'):
                event = json.loads(line)
                if event.get("reason") == "compiler-artifact" and event["target"]["kind"] != ["custom-build"]:
                    artifacts[event["target"]["name"]] = event["fresh"]
        observed = [line for line in run.stdout.splitlines() if line.startswith("REVISION=")]
        row = dict(fixture=root.name, label=label, artifacts=artifacts, output=observed,
                   dirty=[line for line in run.stderr.splitlines() if "Dirty metadata-identity" in line])
        self.rows.append(row)
        (self.proof / "results.json").write_text(json.dumps(dict(
            script_sha256=hashlib.sha256(self.script.read_bytes()).hexdigest(),
            cargo=self.command(root, "cargo", "--version"),
            rustc=self.command(root, "rustc", "--version"), rows=self.rows), indent=2) + "\n")
        self.assertEqual(observed, ["REVISION=" + expected], row)
        expected_freshness = fresh if isinstance(fresh, dict) else {
            "metadata_identity": fresh, "metadata-consumer": fresh}
        self.assertEqual(artifacts, expected_freshness, row)

    def test_absent_packed_refs_commit_and_consumer_edit(self):
        root = self.fixture("absent-packed")
        self.assertFalse((root / ".git/packed-refs").exists())
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "warm", True, self.revision(root))
        self.commit(root)
        self.build(root, "commit", False, self.revision(root))
        self.build(root, "commit-repeat", True, self.revision(root))
        main = root / "crates/consumer/src/main.rs"
        main.write_text(main.read_text() + "// consumer-only source edit\n")
        self.build(root, "consumer-edit", {"metadata_identity": True, "metadata-consumer": False},
                   self.revision(root))
        self.build(root, "consumer-edit-repeat", True, self.revision(root))

    def test_linked_commit_tags_packing_and_detached_head(self):
        original = self.fixture("primary")
        root = self.proof / "linked"
        self.git(original, "worktree", "add", "-q", "-b", "linked-fixture", str(root))
        self.assertTrue((root / ".git").is_file())
        self.assertFalse((root / ".git/HEAD").exists())
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "warm", True, self.revision(root))
        self.commit(root)
        tip = self.git(root, "rev-parse", "HEAD")
        self.build(root, "linked-commit", False, self.revision(root))
        self.build(root, "commit-repeat", True, self.revision(root))
        # A primary-worktree commit must not replace this worktree's identity.
        self.commit(original)
        self.build(root, "primary-commit", True, self.revision(root))
        self.git(original, "tag", "shared-tag", tip)
        self.build(root, "common-tag", False, "tag shared-tag")
        self.build(root, "tag-repeat", True, "tag shared-tag")
        self.git(original, "pack-refs", "--all", "--prune")
        self.build(root, "packed", False, "tag shared-tag")
        self.build(root, "packed-repeat", True, "tag shared-tag")
        self.git(original, "tag", "-d", "shared-tag")
        self.build(root, "packed-tag-delete", False, self.revision(root))
        self.build(root, "delete-repeat", True, self.revision(root))
        self.commit(root)
        self.build(root, "packed-to-loose-commit", False, self.revision(root))
        self.build(root, "packed-to-loose-repeat", True, self.revision(root))
        self.git(root, "checkout", "-q", "--detach", tip)
        self.build(root, "detach", False, self.revision(root))
        self.build(root, "detach-repeat", True, self.revision(root))
        self.commit(root)
        self.build(root, "detached-commit", False, self.revision(root))
        self.build(root, "detached-repeat", True, self.revision(root))

    def test_absent_discovery_retains_unknown_and_recovers(self):
        root = self.fixture("no-discovery")
        # Admission sees the repository, while Git from the nested build script
        # cannot discover it. This tests fallback, not a literal source export.
        self.env = dict(self.env, GIT_CEILING_DIRECTORIES=str(root))
        self.build(root, "unknown-cold", False, "unknown unknown")
        self.build(root, "unknown-repeat", False, "unknown unknown")
        self.env.pop("GIT_CEILING_DIRECTORIES")
        self.build(root, "recovered", False, self.revision(root))
        self.build(root, "recovered-repeat", True, self.revision(root))


if __name__ == "__main__":
    unittest.main()
