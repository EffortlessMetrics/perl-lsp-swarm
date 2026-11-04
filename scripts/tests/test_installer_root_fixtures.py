"""Opt-in production-module probes; every installer target is a disposable repo.

INSTALLER_ROOT_FIXTURE_BUILD=1 enables the bounded admitted build. These are
acceptance tests: current main is expected to fail the copied-root, sibling and
ambiguous-caller assertions until the installer owner's repair is integrated.
No full xtask executable, CLI --root support, or platform qualification is claimed.
"""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def run(argv, cwd, env, check=True):
    result = subprocess.run([str(a) for a in argv], cwd=cwd, env=env,
                            capture_output=True, text=True, timeout=300)
    if check and result.returncode:
        raise RuntimeError(f"{argv}: exit {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result


def snapshot(paths):
    """Compare raw bytes, including presence; never normalize line endings."""
    result = {}
    for path in paths:
        if path.is_dir():
            result[str(path)] = "directory"
            result.update({str(p): p.read_bytes() for p in path.rglob("*") if p.is_file()})
        else:
            result[str(path)] = path.read_bytes() if path.exists() else None
    return result


@unittest.skipUnless(os.environ.get("INSTALLER_ROOT_FIXTURE_BUILD") == "1",
                     "opt-in admitted Rust fixture build")
class InstallerRootFixtures(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Retain build resources and logs, including any unsettled admission lease.
        # Explicit cleanup can follow independent owner verification.
        cls.sandbox = Path(tempfile.mkdtemp(prefix="installer-root-proof-")).resolve()
        print("retained_fixture=" + str(cls.sandbox), flush=True)
        cls.env = {k: v for k, v in os.environ.items()
                   if not k.startswith("GIT_") and k not in
                   ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER",
                    "RUSTC_WORKSPACE_WRAPPER")}
        cls.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(cls.sandbox / "global"),
                       GIT_TEMPLATE_DIR=str(cls.sandbox / "templates"),
                       FIXTURE_SANDBOX=str(cls.sandbox), RUSTUP_AUTO_INSTALL="0",
                       RUSTUP_TOOLCHAIN="1.95.0", DEVPLANE=str(cls.sandbox / "devplane"))
        (cls.sandbox / "templates").mkdir()
        cls.a, cls.b, cls.sibling = [cls.sandbox / n for n in ("compiled root", "caller", "sibling")]
        for root in (cls.a, cls.b):
            root.mkdir()
            run(["git", "init", "-q", root], cls.sandbox, cls.env)
            run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-q", "--allow-empty", "-m", "fixture"], root, cls.env)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = []\n[workspace.package]\n'
                'repository="https://github.com/EffortlessMetrics/perl-lsp-swarm"\n', encoding="utf-8")
        (cls.b / "xtask").mkdir()
        (cls.b / "xtask/Cargo.toml").write_text('[package]\nname="xtask"\n', encoding="utf-8")
        run(["git", "worktree", "add", "-q", "-b", "fixture-sibling", cls.sibling], cls.a, cls.env)
        cls.foreign = cls.sandbox / "no project"
        cls.foreign.mkdir()
        cls.manifest = cls.a / "xtask"
        src = cls.manifest / "src"
        src.mkdir(parents=True)
        modules = {"utils.rs": "xtask/src/utils.rs",
                   "ci_hygiene.rs": "xtask/src/tasks/ci_hygiene.rs",
                   "git_hooks.rs": "crates/perl-ci-hygiene/src/git_hooks.rs"}
        cls.identities = {}
        for destination, source in modules.items():
            data = (ROOT / source).read_bytes()
            (src / destination).write_bytes(data)
            cls.identities[source] = hashlib.sha256(data).hexdigest()
        # Copy the actual standalone CLI root finder too, without rewriting its body.
        main = (ROOT / "crates/perl-ci-hygiene/src/main.rs").read_text(encoding="utf-8")
        finder = main.split("fn find_repo_root()", 1)[1].split("pub(crate) fn display_path", 1)[0]
        (src / "finder.rs").write_text("fn find_repo_root()" + finder, encoding="utf-8")
        dependency = json.dumps(str(ROOT / "crates/perl-ci-hygiene"))
        (cls.manifest / "Cargo.toml").write_text(
            '[package]\nname="installer-root-probe"\nversion="0.0.0"\nedition="2024"\n'
            'repository="https://github.com/EffortlessMetrics/perl-lsp-swarm"\n'
            '[workspace]\n[dependencies]\ncolor-eyre="=0.6.5"\n'
            'serde={version="1",features=["derive"]}\nserde_json="1"\ntoml="1.1.2"\n'
            f'perl-ci-hygiene={{path={dependency}}}\n', encoding="utf-8")
        (src / "main.rs").write_text(r'''
use color_eyre::eyre::{Result, bail};
use std::{env, path::{Path, PathBuf}};
mod utils;
mod ci_hygiene;
mod git_hooks;
include!("finder.rs");
fn owned(path: &Path) -> Result<()> {
    let sandbox = PathBuf::from(env::var_os("FIXTURE_SANDBOX").ok_or_else(||
        color_eyre::eyre::eyre!("missing fixture sandbox"))?).canonicalize()?;
    if !path.canonicalize()?.starts_with(sandbox) { bail!("non-fixture target refused"); }
    Ok(())
}
fn probe() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    owned(&env::current_dir()?)?;
    owned(&utils::project_root()?)?;
    match args.first().map(String::as_str) {
        Some("forward") => ci_hygiene::run("install-githooks".into(), vec![]),
        Some("explicit-forward") => {
            let root = PathBuf::from(args.get(1).ok_or_else(||
                color_eyre::eyre::eyre!("explicit fixture root required"))?);
            owned(&root)?;
            utils::select_worktree_root(&root)?;
            ci_hygiene::run("install-githooks".into(), vec![])
        }
        Some("install-root") => {
            let root = PathBuf::from(args.get(1).ok_or_else(||
                color_eyre::eyre::eyre!("explicit fixture root required"))?);
            owned(&root)?;
            git_hooks::cmd_install_githooks(&root)?;
            Ok(())
        }
        // Cargo shim and fresh local-binary shim execute the actual installer,
        // replacing only the expensive nested compilation/standalone dispatch.
        Some("run") | Some("install-githooks") => {
            let root = find_repo_root()?;
            owned(&root)?;
            git_hooks::cmd_install_githooks(&root)?;
            Ok(())
        }
        _ => bail!("unsupported fixture invocation"),
    }
}
fn main() {
    if let Err(error) = probe() { eprintln!("{error:#}"); std::process::exit(1); }
}
''', encoding="utf-8")
        # The caller owns sizing for this exact offline hygiene-library build.
        # No default reserve or growth policy is weakened by enabling the test.
        wrapper = ROOT / "scripts/cargo_admitted.py"
        argv = ["build", "--offline"]
        preflight = run([sys.executable, wrapper, "--preflight", *argv], cls.manifest, cls.env, False)
        line = next((x for x in preflight.stderr.splitlines()
                     if x.startswith("cargo-admitted preflight: ")), None)
        if line is None:
            tail = "\n".join(preflight.stderr.splitlines()[-20:])
            raise RuntimeError(
                f"preflight failed (exit {preflight.returncode}) without a report line; stderr tail:\n{tail}")
        plan = json.loads(line.split(": ", 1)[1])
        budget = cls.sandbox / "budget.json"
        budget.write_text(json.dumps({"schema_version": 1, "scope": plan["scope"],
            "reserve_bytes": int(os.environ["INSTALLER_ROOT_FIXTURE_RESERVE_GIB"]) * 1024**3,
            "expected_growth_bytes": int(os.environ["INSTALLER_ROOT_FIXTURE_GROWTH_GIB"]) * 1024**3,
            "basis": os.environ["INSTALLER_ROOT_FIXTURE_BASIS"]}), encoding="utf-8")
        build = run([sys.executable, wrapper, "--budget-file", budget, *argv], cls.manifest, cls.env, False)
        (cls.sandbox / "build.stdout").write_text(build.stdout, encoding="utf-8")
        (cls.sandbox / "build.stderr").write_text(build.stderr, encoding="utf-8")
        if build.returncode:
            raise RuntimeError(f"fixture build exit {build.returncode}; see {cls.sandbox}/build.stderr")
        descriptor_line = next((x for x in build.stderr.splitlines()
                                if x.startswith("cargo-admitted resources: ")), None)
        if descriptor_line is None:
            tail = "\n".join(build.stderr.splitlines()[-20:])
            raise RuntimeError(
                f"build failed (exit {build.returncode}) without a resources line; stderr tail:\n{tail}")
        descriptor = json.loads(descriptor_line.split(": ", 1)[1])
        cls.binary = cls.sandbox / ("probe" + (".exe" if os.name == "nt" else ""))
        # Resolve the private target from the admitted descriptor, never a shared cache.
        target = Path(descriptor["resources"]["target"])
        shutil.copy2(target / "debug" / ("installer-root-probe" + cls.binary.suffix), cls.binary)
        cls.bin_dir = cls.sandbox / "bin"
        cls.bin_dir.mkdir()
        shutil.copy2(cls.binary, cls.bin_dir / ("cargo" + cls.binary.suffix))
        cls.env["PATH"] = str(cls.bin_dir) + os.pathsep + cls.env["PATH"]
        cls.configs = [cls.a / ".git/config", cls.b / ".git/config"]
        cls.gitdirs = [Path(run(["git", "rev-parse", "--absolute-git-dir"], root, cls.env).stdout.strip())
                       for root in (cls.a, cls.b, cls.sibling)]
        cls.configs.extend(directory / "config.worktree" for directory in cls.gitdirs)
        cls.original_configs = snapshot(cls.configs)
        print("module_sha256=" + json.dumps(cls.identities, sort_keys=True), flush=True)
        print("executable_sha256=" + hashlib.sha256(cls.binary.read_bytes()).hexdigest(), flush=True)

    def setUp(self):
        for name, data in self.original_configs.items():
            path = Path(name)
            if data is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(data)
        self.paths = list(self.configs) + [self.sandbox / "global"]
        for directory in self.gitdirs:
            hooks = directory / "hooks"
            hooks.mkdir(exist_ok=True)
            (hooks / "post-commit").write_bytes(b"# unrelated common/private hook\r\n\xff")
            self.paths.append(hooks)
        for root in (self.a, self.b, self.sibling):
            hooks = root / ".githooks"
            hooks.mkdir(exist_ok=True)
            for hook in ("pre-commit", "pre-push", "post-commit"):
                path = hooks / hook
                path.write_bytes(b"#!/bin/sh\r\n# fixture sentinel\x00\xff\r\n")
            self.paths.append(hooks)
        for root in (self.a, self.b):
            local = root / "target/debug" / ("perl-ci-hygiene" + self.binary.suffix)
            if local.exists():
                local.unlink()
        self.before = snapshot(self.paths)

    def invoke(self, *args, cwd=None):
        return run([self.binary, *args], cwd or self.b, self.env, False)

    def assert_unchanged(self, paths):
        expected = {name: data for name, data in self.before.items()
                    if any(Path(name) == p or Path(name).is_relative_to(p) for p in paths)}
        actual = snapshot(paths)
        changed = sorted(name for name in expected.keys() | actual.keys()
                         if expected.get(name) != actual.get(name) or (name in expected) != (name in actual))
        self.assertEqual(changed, [], "hook/config bytes or inventory changed: " + repr(changed))

    def assert_installed(self, root):
        source = (ROOT / "crates/perl-ci-hygiene/src/git_hooks.rs").read_text(encoding="utf-8")
        expected_commit = source.split('const PRE_COMMIT_HOOK: &str = r#"', 1)[1].split('"#;', 1)[0]
        expected_push = source.split("fn pre_push_hook_script()", 1)[1].split('r#"', 1)[1].split('"#', 1)[0]
        for name, expected in (("pre-commit", expected_commit), ("pre-push", expected_push)):
            path = root / ".githooks" / name
            # write_git_hook appends exactly one LF to the embedded authority.
            self.assertEqual(path.read_bytes(), (expected + "\n").encode("utf-8"))
            if os.name != "nt":
                self.assertTrue(path.stat().st_mode & 0o111, name + " must be executable")
        effective = run(["git", "config", "--get", "core.hooksPath"], root, self.env)
        self.assertEqual(effective.stdout.rstrip("\n"), ".githooks")

    def test_copied_forwarder_does_not_write_compiled_checkout(self):
        result = self.invoke("forward")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_unchanged([p for p in self.paths if p.is_relative_to(self.a)
                               or p.is_relative_to(self.sibling)])
        self.assert_installed(self.b)

    def test_explicit_component_root_installs_and_preserves_unrelated_repository(self):
        result = self.invoke("install-root", self.b, cwd=self.a)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_unchanged([p for p in self.paths if not p.is_relative_to(self.b)])
        self.assert_installed(self.b)
        self.assert_unchanged([self.b / ".githooks/post-commit"])

    def test_explicit_runtime_root_aligns_forwarder_and_child(self):
        result = self.invoke("explicit-forward", self.b, cwd=self.a)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_unchanged([p for p in self.paths if not p.is_relative_to(self.b)])
        self.assert_installed(self.b)

    def test_fresh_local_binary_branch_preserves_unrelated_repository(self):
        for root in (self.a, self.b):
            crate = root / "crates/perl-ci-hygiene"
            crate.mkdir(parents=True, exist_ok=True)
            (crate / "Cargo.toml").write_text("# freshness sentinel\n", encoding="utf-8")
            local = root / "target/debug" / ("perl-ci-hygiene" + self.binary.suffix)
            local.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.binary, local)
            os.utime(local, None)
        result = self.invoke("forward")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_unchanged([p for p in self.paths if not p.is_relative_to(self.b)])
        self.assert_installed(self.b)

    def test_explicit_component_root_preserves_sibling_effective_configuration(self):
        before = run(["git", "config", "--get", "core.hooksPath"], self.sibling, self.env, False)
        result = self.invoke("install-root", self.a)
        self.assertEqual(result.returncode, 0, result.stderr)
        after = run(["git", "config", "--get", "core.hooksPath"], self.sibling, self.env, False)
        self.assertEqual((after.returncode, after.stdout), (before.returncode, before.stdout),
                         "shared config changed sibling effective hooksPath")

    def test_explicit_runtime_root_refuses_ambiguous_git_selector(self):
        env = dict(self.env, GIT_DIR=str(self.a / ".git"))
        result = run([self.binary, "explicit-forward", self.b], self.a, env, False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing an ambiguous repository", result.stderr)
        self.assert_unchanged(self.paths)

    def test_explicit_component_root_preserves_sibling_hook_bytes(self):
        result = self.invoke("install-root", self.a)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_unchanged([p for p in self.paths if p.is_relative_to(self.sibling)
                               or p.is_relative_to(self.b)
                               or p.is_relative_to(self.gitdirs[2])])

    def test_ambiguous_foreign_caller_refuses_before_any_write(self):
        result = self.invoke("forward", cwd=self.foreign)
        self.assertNotEqual(result.returncode, 0, "foreign caller silently selected compiled root")
        self.assertTrue(result.stderr.strip(), "refusal must explain the selection failure")
        self.assert_unchanged(self.paths)

    def test_invalid_explicit_component_root_refuses_without_writes(self):
        result = self.invoke("install-root", self.foreign)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("git worktree list failed", result.stderr)
        self.assert_unchanged(self.paths)


if __name__ == "__main__":
    unittest.main(verbosity=2)
