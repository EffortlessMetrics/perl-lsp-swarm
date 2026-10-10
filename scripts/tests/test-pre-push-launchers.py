#!/usr/bin/env python3
"""Exercise both hook authorities with real Git and child-local tool stubs."""
import os
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


class LauncherTests(unittest.TestCase):
    def test_manager_refuses_missing_bootstrap_support(self):
        spec = importlib.util.spec_from_file_location("hook_manager", ROOT / "scripts/worktree-manager.py")
        manager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(manager)
        with tempfile.TemporaryDirectory() as tmp:
            slot = Path(tmp)
            (slot / "hooks").mkdir()
            (slot / "hooks/pre-push").write_text("authority\n")
            for installer, bash in ((False, "/bin/bash"), (True, None)):
                with self.subTest(installer=installer, bash=bash):
                    if installer:
                        (slot / "scripts").mkdir()
                        (slot / "scripts/install-githooks.sh").write_text("installer\n")
                    with patch.object(manager, "_installed_hook_paths", return_value=(slot / "push", slot / "commit")), \
                         patch.object(manager, "_hook_bytes_current", return_value=False), \
                         patch.object(manager, "_usable_bash", return_value=bash), \
                         patch.object(manager, "run") as run:
                        with self.assertRaisesRegex(RuntimeError, "revision-owned installer or Bash unavailable"):
                            manager.provision_worktree_hooks(slot)
                        run.assert_not_called()

    def test_bootstrap_dependency_boundary(self):
        # Conservative lockfile closure includes dev/optional/platform edges:
        # an exclusion here is useful without claiming resolved compiler units.
        packages = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
        by_name = {}
        for package in packages:
            by_name.setdefault(package["name"], []).append(package)
        manifest = tomllib.loads((ROOT / "crates/perl-ci-hygiene/Cargo.toml").read_text())
        pending = ["perl-ci-hygiene", *manifest["dependencies"]]
        seen = set()
        while pending:
            name = pending.pop()
            if name in seen:
                continue
            seen.add(name)
            self.assertIn(name, by_name, f"dependency {name} absent from locked graph")
            for package in by_name[name]:
                pending.extend(dep.split()[0] for dep in package.get("dependencies", []))
        repository_packages = {
            tomllib.loads(path.read_text())["package"]["name"]
            for path in (ROOT / "crates").glob("*/Cargo.toml")
        }
        repository_packages.add("xtask")
        self.assertEqual(seen & repository_packages, {"perl-ci-hygiene", "perl-test-must"})

    def test_launcher_matrix(self):
        source = (ROOT / "crates/perl-ci-hygiene/src/git_hooks.rs").read_text()
        generated = source.split('pub(crate) fn pre_push_hook_script()', 1)[1]
        generated = generated.split('r#"', 1)[1].split('"#', 1)[0]
        hooks = {"generated": generated, "maintained": (ROOT / "hooks/pre-push").read_text()}
        self.assertEqual(hooks["generated"], hooks["maintained"], "hook authorities must agree byte-for-byte")
        for authority, hook in hooks.items():
            for case in ("missing", "nix", "nix-fails", "just", "just-fails",
                         "nix-no-flake", "docs", "delete", "protected-delete",
                         "unknown", "single"):
                with self.subTest(authority=authority, case=case), tempfile.TemporaryDirectory() as tmp:
                    self.run_case(Path(tmp), hook, case)

    def run_case(self, root, hook, case):
        repo = root / "repo with spaces"
        repo.mkdir()
        git = shutil.which("git")
        bash = shutil.which("bash")
        def run_git(*args):
            return subprocess.check_output([git, "-C", str(repo), *args], text=True).strip()
        run_git("init", "-q")
        run_git("config", "user.name", "Hook fixture")
        run_git("config", "user.email", "hook@example.invalid")
        (repo / "base.md").write_text("base\n")
        run_git("add", ".")
        run_git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "base")
        base = run_git("rev-parse", "HEAD")
        path = "docs/a.md" if case == "docs" else "crates/probe/src/lib.rs" if case == "single" else "crates/one/src/lib.rs" if case == "missing" else "src/a.rs"
        changed = repo / path
        changed.parent.mkdir(parents=True, exist_ok=True)
        changed.write_text("changed\n")
        if case == "missing":
            second = repo / "crates/two/src/lib.rs"
            second.parent.mkdir(parents=True)
            second.write_text("changed\n")
        run_git("add", ".")
        run_git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "changed")
        head = run_git("rev-parse", "HEAD")
        hook_path = repo / "pre-push"
        hook_path.write_text(hook)
        tools = root / "tools"
        tools.mkdir()
        # Do not inherit ambient nix/just/cargo, even if the host has them.
        for name in ("git", "bash", "awk", "grep", "head", "sed", "tr", "mktemp", "tee", "rm"):
            os.symlink(shutil.which(name), tools / name)
        log = root / "calls"
        def stub(name, body):
            script = tools / name
            script.write_text(f'#!{bash}\n' + body)
            script.chmod(0o755)
        if case in ("nix", "nix-fails", "nix-no-flake"):
            stub("nix", 'echo "nix $*" >> "$CALLS"\nexit ' + ("37" if case == "nix-fails" else "0") + "\n")
            if case != "nix-no-flake":
                (repo / "flake.nix").write_text("{}\n")
        if case in ("nix", "nix-fails", "just", "just-fails", "nix-no-flake"):
            stub("just", 'echo "just $*" >> "$CALLS"\nexit ' + ("38" if case == "just-fails" else "0") + "\n")
        if case == "single":
            stub("cargo", 'echo "cargo $*" >> "$CALLS"\nif [ "$1" = xtask ]; then echo actual-package; fi\n')
        zero = "0" * 40
        if case in ("delete", "protected-delete"):
            head = zero
        if case == "unknown":
            base = zero
            stub("cargo", "exit 39\n")
        ref = "refs/heads/main" if case == "protected-delete" else "refs/heads/topic"
        env = dict(os.environ, PATH=str(tools), CALLS=str(log))
        for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "PERL_LSP_ALLOW_HISTORY_REWRITE"):
            env.pop(name, None)
        result = subprocess.run([bash, str(hook_path)], cwd=repo, env=env,
                                input=f"refs/heads/topic {head} {ref} {base}\n",
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        expected = {"missing": 1, "nix-fails": 37, "just-fails": 38,
                    "protected-delete": 1, "unknown": 1}.get(case, 0)
        self.assertEqual(result.returncode, expected, result.stdout)
        calls = log.read_text() if log.exists() else ""
        if case == "missing":
            self.assertIn("NOT PROVEN", result.stdout)
            self.assertNotIn("gate passed", result.stdout)
        elif case in ("nix", "nix-fails"):
            self.assertEqual(calls, "nix develop -c just pr-fast\n")
        elif case in ("just", "just-fails", "nix-no-flake"):
            self.assertEqual(calls, "just pr-fast\n")
        elif case == "single":
            self.assertIn("cargo test -p actual-package --locked", calls)
            self.assertIn("Single-crate gate passed", result.stdout)
        else:
            self.assertEqual(calls, "")


if __name__ == "__main__":
    unittest.main()
