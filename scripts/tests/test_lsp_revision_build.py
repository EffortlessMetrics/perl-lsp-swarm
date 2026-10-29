#!/usr/bin/env python3
"""Opt-in real Cargo proof for the production LSP revision build script.

LSP_REVISION_REAL_BUILD_TEST=1 LSP_REVISION_PROOF_ROOT=/absolute/new/path \
  python3 -m unittest discover -s scripts/tests -p test_lsp_revision_build.py -v

Uses the installed toolchain and cargo-admitted, preserves all evidence/cache state.
The caller owns these sequential fixtures and their later artifact consumers.
A dependency-free script/binary gets a conservative 1 GiB additional-growth
budget with 8 GiB reserve; this is not a whole-LSP or cloud-host recommendation.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

REPO = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.environ.get("LSP_REVISION_REAL_BUILD_TEST") == "1", "opt-in real Cargo proof")
class RevisionBuildTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.proof = Path(os.environ["LSP_REVISION_PROOF_ROOT"])
        cls.proof.mkdir(parents=True, exist_ok=False)
        cls.script = Path(os.environ.get("LSP_REVISION_BUILD_SCRIPT", REPO / "crates/perl-lsp-rs/build.rs"))
        cls.env = dict(os.environ, DEVPLANE=str(cls.proof / "devplane"))
        # Select fixture-private outputs via the unchanged admitted interface.
        # Capacity comes from this exact-command operator fixture budget.
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "MIN_FREE_GB", "MAX_USED_PCT",
                    "CARGO_STORAGE_POLICY", "CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE"):
            cls.env.pop(key, None)
        cls.rows = []

    def command(self, cwd, args):
        return subprocess.run(args, cwd=cwd, env=self.env, text=True,
                              capture_output=True, encoding="utf-8", errors="backslashreplace", check=True).stdout.strip()

    def git(self, root, *args):
        return self.command(root, ["git", *args])

    def fixture(self, name, ref_format=None):
        root = self.proof / name
        crate = root / "crates/probe"
        (crate / "src").mkdir(parents=True)
        (root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/probe"]\nresolver="2"\n')
        (crate / "Cargo.toml").write_text('[package]\nname="revision-probe"\nversion="0.1.0"\nedition="2021"\n')
        (crate / "build.rs").write_bytes(self.script.read_bytes())
        (crate / "src/main.rs").write_text('fn main() { println!("{} {}", env!("BUILD_REVISION_KIND"), env!("BUILD_REVISION")); }\n')
        flags = ["--ref-format=" + ref_format] if ref_format else []
        self.git(root, "init", "-q", "-b", "probe", *flags)
        self.commit(root)
        return root

    def commit(self, root):
        self.git(root, "add", "Cargo.toml", "crates")
        self.git(root, "-c", "user.name=Revision Probe", "-c", "user.email=probe@example.invalid",
                 "commit", "-qm", "probe update", "--allow-empty")

    def build(self, root, label, fresh, expected):
        args = ["run", "-p", "revision-probe", "--offline", "-vv"]
        # Native Windows uses the existing Python entrypoint; the extensionless
        # Bash entrypoint is not a native Windows executable.
        wrapper = ([sys.executable, str(REPO / "scripts/cargo_admitted.py")]
                   if os.name == "nt" else [str(REPO / "scripts/cargo-admitted")])
        pre = subprocess.run([*wrapper, "--preflight", *args], cwd=root, env=self.env,
                             text=True, capture_output=True, encoding="utf-8", errors="backslashreplace")
        line = next((line for line in pre.stderr.splitlines()
                     if line.startswith("cargo-admitted preflight:")), None)
        self.assertIsNotNone(line, pre.stderr[-6000:])
        report = json.loads(line.split(": ", 1)[1])
        # The default 40 GiB floor can refuse this constrained fixture host.
        # Only its capacity refusal supplies scope for an explicit budget;
        # guard/lease/argument failures must retain their original diagnosis.
        if pre.returncode:
            self.assertEqual(pre.returncode, 75, pre.stderr[-6000:])
            self.assertIn("insufficient disk headroom", report.get("reason", ""), pre.stderr[-6000:])
        self.assertIsInstance(report.get("scope"), dict, pre.stderr[-6000:])
        budget = root / "budget.json"
        budget.write_text(json.dumps(dict(schema_version=1, scope=report["scope"],
            reserve_bytes=8 * 1024**3, expected_growth_bytes=1024**3,
            basis="Operator sizing: dependency-free revision build script and tiny binary; installed toolchain, offline, no download. Conservative 1 GiB additional growth and 8 GiB reserve. Actual bytes captured each run.")))
        start = time.monotonic()
        run = subprocess.run([*wrapper, "--budget-file", str(budget), *args], cwd=root,
                             env=self.env, text=True, capture_output=True, encoding="utf-8", errors="backslashreplace")
        elapsed = time.monotonic() - start
        log = run.stdout + run.stderr
        (root / f"{label}.log").write_text(log)
        self.assertEqual(run.returncode, 0, log[-6000:])
        descriptor = json.loads(next(line.split(": ", 1)[1] for line in run.stderr.splitlines()
                                    if line.startswith("cargo-admitted resources:")))
        paths = descriptor["resources"]
        sizes = {}
        allocated = {}
        for role in ("target", "build"):
            path = Path(paths[role])
            files = [p for p in path.rglob("*") if p.is_file()]
            sizes[role] = sum(p.stat().st_size for p in files)
            if os.name == "posix":
                allocated[role] = sum(p.stat().st_blocks * 512 for p in files)
        compiled = "Compiling revision-probe" in run.stderr
        # Cargo's real decision and the consumed compile-time label are separate oracles.
        output = "\n".join(line for line in run.stdout.splitlines()
                           if not line.startswith("[revision-probe 0.1.0] "))
        row = dict(fixture=root.name, label=label, elapsed_seconds=elapsed,
                   compiled=compiled, output=output, logical_bytes=sizes, allocated_bytes=allocated,
                   script_ran="cargo:rustc-env=BUILD_REVISION=" in run.stdout,
                   watchers=[line.split("cargo:rerun-if-changed=", 1)[1]
                             for line in run.stdout.splitlines() if "cargo:rerun-if-changed=" in line],
                   dirty=[line.strip() for line in run.stderr.splitlines() if "Dirty revision-probe" in line])
        self.rows.append(row)
        (self.proof / "measurements.json").write_text(json.dumps(dict(
            build_script_sha256=hashlib.sha256(self.script.read_bytes()).hexdigest(),
            cargo=self.command(root, ["cargo", "--version"]),
            rustc=self.command(root, ["rustc", "--version"]), rows=self.rows), indent=2)+"\n")
        self.assertEqual(compiled, not fresh, row)
        self.assertEqual(output, expected, row)
        if fresh:
            self.assertIn("Fresh revision-probe", run.stderr)
        return row

    def revision(self, root):
        return "commit " + self.git(root, "rev-parse", "--short", "HEAD")

    def test_clone_cold_warm_edit_and_revision_controls(self):
        root = self.fixture("clone")
        expected = self.revision(root)
        self.assertFalse((root / ".git/packed-refs").exists())
        self.build(root, "cold", False, expected)
        self.build(root, "warm", True, expected)
        main = root / "crates/probe/src/main.rs"
        main.write_text(main.read_text().replace('println!("{} {}"', 'println!("edited {} {}"'))
        edit = self.build(root, "small-edit", False, "edited " + expected)
        self.assertFalse(edit["script_ran"], edit)
        self.build(root, "edit-repeat", True, "edited " + expected)
        self.commit(root)
        self.build(root, "commit", False, "edited " + self.revision(root))
        self.build(root, "commit-repeat", True, "edited " + self.revision(root))
        self.git(root, "tag", "probe-tag")
        self.build(root, "tag-create", False, "edited tag probe-tag")
        self.build(root, "tag-repeat", True, "edited tag probe-tag")
        self.git(root, "pack-refs", "--all", "--prune")
        self.build(root, "pack-refs", False, "edited tag probe-tag")
        self.build(root, "pack-repeat", True, "edited tag probe-tag")
        self.git(root, "tag", "-d", "probe-tag")
        self.build(root, "tag-delete", False, "edited " + self.revision(root))
        self.build(root, "delete-repeat", True, "edited " + self.revision(root))
        self.commit(root)
        self.build(root, "packed-branch-commit", False, "edited " + self.revision(root))
        self.build(root, "packed-branch-repeat", True, "edited " + self.revision(root))
        self.git(root, "tag", "loose-tag")
        self.build(root, "loose-tag-create", False, "edited tag loose-tag")
        self.git(root, "tag", "-d", "loose-tag")
        self.build(root, "loose-tag-delete", False, "edited " + self.revision(root))
        self.build(root, "loose-delete-repeat", True, "edited " + self.revision(root))
        # Once tags/heads directories exist, unrelated ref churn must be fresh.
        self.git(root, "update-ref", "refs/remotes/probe/other", "HEAD")
        self.build(root, "remote-ref", True, "edited " + self.revision(root))
        self.git(root, "checkout", "-q", "--detach", "HEAD")
        self.build(root, "detach", False, "edited " + self.revision(root))
        self.build(root, "detach-repeat", True, "edited " + self.revision(root))
        self.git(root, "config", "core.abbrev", "17")
        self.build(root, "local-config", False, "edited " + self.revision(root))
        self.build(root, "config-repeat", True, "edited " + self.revision(root))

    def test_linked_worktree(self):
        original = self.fixture("original")
        root = self.proof / "linked"
        self.git(original, "worktree", "add", "-q", "-b", "linked-probe", str(root))
        cold = self.build(root, "cold", False, self.revision(root))
        self.assertIn(root / ".git", [Path(path) for path in cold["watchers"]])
        self.build(root, "warm", True, self.revision(root))
        self.commit(root)
        self.build(root, "commit", False, self.revision(root))
        self.build(root, "commit-repeat", True, self.revision(root))
        self.git(original, "config", "extensions.worktreeConfig", "true")
        self.build(root, "worktree-config-enabled", False, self.revision(root))
        self.git(root, "config", "--worktree", "core.abbrev", "13")
        self.build(root, "worktree-config-created", False, self.revision(root))
        self.build(root, "worktree-config-repeat", True, self.revision(root))
        self.git(original, "tag", "linked-tag", self.git(root, "rev-parse", "HEAD"))
        self.build(root, "common-tag", False, "tag linked-tag")
        self.build(root, "tag-repeat", True, "tag linked-tag")

    def test_linked_packed_refs_and_detached_head(self):
        original = self.fixture("linked-packed-original")
        previous = self.git(original, "rev-parse", "HEAD")
        # Empty commits keep the source identical: only revision inputs change.
        self.commit(original)
        root = self.proof / "linked-packed"
        self.git(original, "worktree", "add", "-q", "-b", "packed-probe", str(root))
        self.git(original, "pack-refs", "--all", "--prune")
        common = Path(self.git(root, "rev-parse", "--path-format=absolute", "--git-common-dir"))
        private = Path(self.git(root, "rev-parse", "--absolute-git-dir"))
        self.assertTrue((common / "packed-refs").is_file())
        self.assertFalse((common / "refs/heads/packed-probe").exists())
        self.assertFalse((private / "packed-refs").exists())
        self.assertFalse((private / "config.worktree").exists())
        cold = self.build(root, "packed-cold", False, self.revision(root))
        watched = [Path(path) for path in cold["watchers"]]
        self.build(root, "packed-warm", True, self.revision(root))
        # HEAD belongs to the linked worktree; common HEAD is unchanged.
        original_head = self.git(original, "rev-parse", "HEAD")
        self.git(root, "checkout", "-q", "--detach", previous)
        self.assertNotEqual(self.git(root, "rev-parse", "HEAD"), original_head)
        self.build(root, "detached-switch", False, self.revision(root))
        self.build(root, "detached-repeat", True, self.revision(root))
        self.git(original, "tag", "detached-packed-tag", previous)
        self.build(root, "common-tag-created", False, "tag detached-packed-tag")
        self.build(root, "common-tag-repeat", True, "tag detached-packed-tag")
        self.git(original, "pack-refs", "--all", "--prune")
        self.assertFalse((common / "refs/tags/detached-packed-tag").exists())
        self.build(root, "common-tag-packed", False, "tag detached-packed-tag")
        self.build(root, "common-packed-repeat", True, "tag detached-packed-tag")
        # Git's transient loose-ref lock also touches the empty tags directory.
        # Preserve its timestamp so only packed-refs can expose this deletion.
        tags = common / "refs/tags"
        tags_stat = tags.stat()
        self.git(original, "tag", "-d", "detached-packed-tag")
        os.utime(tags, ns=(tags_stat.st_atime_ns, tags_stat.st_mtime_ns))
        self.build(root, "common-packed-tag-deleted", False, self.revision(root))
        self.build(root, "common-delete-repeat", True, self.revision(root))
        self.assertIn(private / "HEAD", watched)
        self.assertIn(common / "packed-refs", watched)
        for path in watched:
            resolved = path if path.is_absolute() else root / "crates/probe" / path
            self.assertTrue(resolved.exists(), str(resolved))

    def test_export_without_git(self):
        root = self.fixture("export")
        # Admission still sees its fixture Git root; build-script discovery
        # stops below it, exercising the same no-Git fallback as an export.
        self.env = dict(self.env, GIT_CEILING_DIRECTORIES=str(root))
        self.build(root, "cold", False, "unknown unknown")
        self.build(root, "warm", False, "unknown unknown")
        self.env.pop("GIT_CEILING_DIRECTORIES")
        self.build(root, "discovery-restored", False, self.revision(root))
        self.build(root, "discovery-repeat", True, self.revision(root))
        self.env["GIT_CEILING_DIRECTORIES"] = str(root)
        self.build(root, "discovery-removed", False, "unknown unknown")
        self.build(root, "removed-repeat", False, "unknown unknown")

    @unittest.skipUnless(os.name == "posix", "POSIX byte-path control")
    def test_non_utf8_common_directory_refreshes_conservatively(self):
        original = self.fixture(os.fsdecode(b"original-\xff"))
        root = self.proof / "byte-path-linked"
        self.git(original, "worktree", "add", "-q", "-b", "byte-path-probe", str(root))
        self.build(root, "cold", False, self.revision(root))
        # Unrepresentable metadata intentionally trades reuse for freshness.
        self.build(root, "repeat", False, self.revision(root))
        self.commit(root)
        self.build(root, "commit", False, self.revision(root))
        self.git(original, "tag", "byte-path-tag", self.git(root, "rev-parse", "HEAD"))
        self.build(root, "tag", False, "tag byte-path-tag")

    def test_reftable_refreshes_conservatively(self):
        root = self.fixture("reftable", ref_format="reftable")
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "repeat", False, self.revision(root))
        self.commit(root)
        self.build(root, "commit", False, self.revision(root))
        self.git(root, "tag", "reftable-tag")
        self.build(root, "tag", False, "tag reftable-tag")

    @unittest.skipUnless(os.name == "posix", "POSIX byte-ref control")
    def test_non_utf8_branch_refreshes_conservatively(self):
        root = self.fixture("byte-branch")
        self.git(root, "checkout", "-q", "-b", os.fsdecode(b"branch-\xff"))
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "repeat", False, self.revision(root))
        self.commit(root)
        self.build(root, "commit", False, self.revision(root))

    def test_git_path_environment_refreshes(self):
        root = self.fixture("git-path-env")
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "warm", True, self.revision(root))
        self.env = dict(self.env, PATH=str(root) + os.pathsep + self.env["PATH"])
        self.build(root, "path-changed", False, self.revision(root))
        self.build(root, "path-repeat", True, self.revision(root))

    def test_symbolic_ref_chain_retarget_refreshes(self):
        root = self.fixture("symbolic-chain")
        self.git(root, "branch", "old-target", "HEAD")
        self.commit(root)
        self.git(root, "symbolic-ref", "refs/heads/alias-two", "refs/heads/old-target")
        self.git(root, "symbolic-ref", "refs/heads/alias-one", "refs/heads/alias-two")
        self.git(root, "symbolic-ref", "HEAD", "refs/heads/alias-one")
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "warm", True, self.revision(root))
        self.git(root, "symbolic-ref", "refs/heads/alias-two", "refs/heads/probe")
        self.build(root, "retarget", False, self.revision(root))
        self.build(root, "retarget-repeat", True, self.revision(root))

    def test_repository_creation_after_export_refreshes(self):
        root = self.fixture("discovery-created")
        self.env = dict(self.env, GIT_CEILING_DIRECTORIES=str(root))
        self.build(root, "cold", False, "unknown unknown")
        crate = root / "crates/probe"
        self.git(crate, "init", "-q", "-b", "created-probe")
        self.git(crate, "add", "Cargo.toml", "build.rs", "src")
        self.git(crate, "-c", "user.name=Revision Probe", "-c", "user.email=probe@example.invalid",
                 "commit", "-qm", "discovery created")
        self.build(root, "git-created", False, self.revision(crate))
        self.build(root, "created-repeat", True, self.revision(crate))

    @unittest.skipUnless(os.name == "posix", "POSIX symlink refs")
    def test_symlink_ref_retarget_refreshes(self):
        root = self.fixture("symlink-chain")
        self.git(root, "branch", "old-target", "HEAD")
        self.commit(root)
        self.git(root, "config", "core.preferSymlinkRefs", "true")
        self.git(root, "symbolic-ref", "HEAD", "refs/heads/probe")
        self.assertTrue((root / ".git/HEAD").is_symlink())
        self.build(root, "cold", False, self.revision(root))
        self.build(root, "warm", False, self.revision(root))
        self.git(root, "symbolic-ref", "HEAD", "refs/heads/old-target")
        self.build(root, "retarget", False, self.revision(root))
        self.build(root, "retarget-repeat", False, self.revision(root))


if __name__ == "__main__":
    unittest.main()
