"""Exercise setup planning and the real Just recipes without compiling Rust.

Git, Bash, Python and Just are required instruments; missing tools are failures,
not skipped proof. Cargo delegates are explicit recording doubles.
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


def isolated_environment():
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("CARGO_", "GIT_", "PYTHON", "RUSTUP_", "SCCACHE_"))
           and key not in ("DEVPLANE", "TMPDIR", "TEMP", "TMP", "XDG_CACHE_HOME")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    return env


def snapshot(root):
    return {str(path.relative_to(root)): (
        "symlink:" + os.readlink(path) if path.is_symlink() else
        "directory" if path.is_dir() else hashlib.sha256(path.read_bytes()).hexdigest()
    ) for path in root.rglob("*")}


class DevplanePlanTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="devplane-plan-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "checkout with spaces $cash"
        scripts = self.repo / "scripts"
        scripts.mkdir(parents=True)
        for name in ("devplane-init", "cargo_admitted.py"):
            shutil.copy2(ROOT / "scripts" / name, scripts / name)
        self.env = isolated_environment()
        self.env.update(HOME=str(self.root / "home"), USERPROFILE=str(self.root / "home"))
        self.git("init", "-q", "-b", "main")
        self.git("add", "scripts")
        self.git("-c", "user.name=Build fixture", "-c", "user.email=fixture@example.invalid",
                 "-c", "core.hooksPath=" + os.devnull, "commit", "-qm", "fixture")
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        self.tool_log = self.root / "unexpected-tool"
        for name in ("cargo", "rustc", "rustup", "sccache"):
            tool = fake_bin / name
            tool.write_text('#!/bin/sh\nprintf called > "$UNEXPECTED_TOOL"\nexit 91\n')
            tool.chmod(0o755)
        self.env.update(PATH=str(fake_bin) + os.pathsep + self.env["PATH"],
                        UNEXPECTED_TOOL=str(self.tool_log))

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, env=self.env,
                              capture_output=True, text=True, check=True).stdout.strip()

    def invoke(self, cwd=None, extra_env=None):
        cwd = cwd or self.repo
        env = self.env | (extra_env or {})
        before = snapshot(self.root)
        result = subprocess.run(["bash", str(self.repo / "scripts/devplane-init")],
                                cwd=cwd, env=env, capture_output=True, text=True, timeout=15)
        after = snapshot(self.root)
        changed = {path for path in before.keys() | after.keys()
                   if before.get(path) != after.get(path)}
        self.assertEqual(changed, set(), "planning changed filesystem entries: " + repr(sorted(changed)))
        self.assertFalse(self.tool_log.exists(), "planning invoked a build/cache tool")
        return result

    def report(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        rows = [line.removeprefix("devplane plan: ") for line in result.stdout.splitlines()
                if line.startswith("devplane plan: ")]
        self.assertEqual(len(rows), 1, result.stdout)
        report = json.loads(rows[0])
        self.assertEqual(report["status"], "planned")
        for key in ("allocated", "capacity_assessed", "lease_acquired"):
            self.assertIs(report[key], False, key)
        self.assertIn("not allocated", result.stdout)
        self.assertIn("capacity not assessed", result.stdout)
        self.assertNotIn("cargo-safe", result.stdout)
        self.assertNotIn("--workspace", result.stdout)
        self.assertNotIn("sccache", result.stdout)
        return report

    def owner_plan(self, cwd, extra_env=None):
        source = (
            "import json, os, sys; sys.path.insert(0, sys.argv[1]); "
            "from cargo_admitted import resource_plan; "
            "worktree, slot, paths = resource_plan(os.environ); "
            "print(json.dumps({'worktree': str(worktree), 'slot': str(slot), "
            "'resources': {key: str(path) for key, path in paths.items()}}))"
        )
        result = subprocess.run([sys.executable, "-B", "-c", source, str(self.repo / "scripts")],
                                cwd=cwd, env=self.env | (extra_env or {}), capture_output=True,
                                text=True, check=True, timeout=15)
        return json.loads(result.stdout)

    def assert_owner_paths(self, report, cwd, extra_env=None):
        expected = self.owner_plan(cwd, extra_env)
        self.assertEqual({key: report[key] for key in expected}, expected)

    def test_default_plan_is_read_only_and_matches_admission_paths(self):
        result = self.invoke()
        report = self.report(result)
        self.assert_owner_paths(report, self.repo)
        self.assertEqual(report["resources"]["cargo_home"], str(self.root / "home/.cargo"))
        self.assertIn("scripts/cargo-admitted --preflight check -p PACKAGE --profile agent --locked", result.stdout)
        self.assertIn("just agent-check PACKAGE", result.stdout)

    def test_explicit_storage_and_temp_match_owner_without_capacity_claim(self):
        override = {"DEVPLANE": str(self.root / "selected storage"),
                    "CARGO_HOME": str(self.root / "shared cargo"),
                    "TMPDIR": str(self.root / "selected temp"),
                    "MIN_FREE_GB": "not-a-budget", "CARGO_BUILD_JOBS": "not-selected"}
        report = self.report(self.invoke(extra_env=override))
        self.assert_owner_paths(report, self.repo, override)
        self.assertEqual(report["resources"]["cargo_home"], override["CARGO_HOME"])
        self.assertEqual(report["resources"]["temp"], override["TMPDIR"])

    def test_git_trace_environment_cannot_write_diagnostic_files(self):
        override = {name: str(self.root / (name.lower() + ".log")) for name in (
            "GIT_TRACE", "GIT_TRACE_SETUP", "GIT_TRACE_PERFORMANCE",
            "GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF")}
        self.report(self.invoke(extra_env=override))

    def test_git_trace_configuration_cannot_write_diagnostic_files(self):
        config = self.root / "trace.gitconfig"
        for target in ("normalTarget", "eventTarget", "perfTarget"):
            self.git("config", "--file", str(config), "trace2." + target,
                     str(self.root / (target + ".log")))
        self.report(self.invoke(extra_env={"GIT_CONFIG_GLOBAL": str(config)}))

    def test_nested_directory_and_linked_worktree_keep_canonical_partition(self):
        nested = self.repo / "nested/deeper"
        nested.mkdir(parents=True)
        sibling = self.root / "linked worktree"
        self.git("worktree", "add", "-q", "-b", "sibling", str(sibling))
        main = self.report(self.invoke())
        nested_report = self.report(self.invoke(cwd=nested))
        sibling_report = self.report(self.invoke(cwd=sibling))
        self.assertEqual(main, nested_report)
        self.assertEqual(main["slot"], sibling_report["slot"])
        for key in ("target", "build"):
            self.assertNotEqual(main["resources"][key], sibling_report["resources"][key])
        self.assertEqual(main["resources"]["cargo_home"], sibling_report["resources"]["cargo_home"])
        self.assert_owner_paths(sibling_report, sibling)

    def test_git_and_storage_overrides_refuse_without_allocating(self):
        for override in ({"GIT_DIR": ""}, {"GIT_WORK_TREE": str(self.repo)},
                         {"GIT_COMMON_DIR": str(self.repo / ".git")},
                         {"CARGO_TARGET_DIR": str(self.root / "old target")},
                         {"CARGO_BUILD_BUILD_DIR": str(self.root / "old build")},
                         {"DEVPLANE": "relative-storage"}):
            with self.subTest(override=override):
                result = self.invoke(extra_env=override)
                self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
                self.assertIn("DENY", result.stderr)
                self.assertNotIn("devplane plan: ", result.stdout)

    def test_nonrepository_invocation_refuses_without_allocating(self):
        result = self.invoke(cwd=self.root)
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        self.assertIn("DENY", result.stderr)
        self.assertNotIn("devplane plan: ", result.stdout)

    def test_linked_storage_refuses_and_preserves_link_and_destination(self):
        destination = self.root / "existing storage"
        destination.mkdir()
        (destination / "retained-evidence").write_text("keep\n")
        link = self.root / "linked storage"
        try:
            link.symlink_to(destination, target_is_directory=True)
        except OSError as error:
            if os.name == "nt" and getattr(error, "winerror", None) == 1314:
                self.skipTest("native Windows symlink privilege unavailable")
            raise
        result = self.invoke(extra_env={"DEVPLANE": str(link)})
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        self.assertIn("linked storage path is unsupported", result.stderr)
        self.assertNotIn("devplane plan: ", result.stdout)

    def test_foreign_windows_storage_refuses_without_allocating(self):
        # Keep the plan's base inside the fixture even for a broken implementation;
        # the foreign path is only an inherited override, never a cleanup root.
        result = self.invoke(extra_env={"CARGO_TARGET_DIR": "/mnt/c/foreign-build-target"})
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        expected = "absolute native Windows path" if os.name == "nt" else "foreign Windows storage"
        self.assertIn(expected, result.stderr)
        self.assertNotIn("devplane plan: ", result.stdout)


class AgentRecipeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if shutil.which("just") is None:
            raise AssertionError("Just is required for executable agent-recipe proof; install it on PATH")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="admitted-recipes-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout with spaces"
        scripts = self.root / "scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(ROOT / "justfile", self.root / "justfile")
        self.arguments = self.root / "arguments"
        self.calls = self.root / "calls"
        self.legacy = self.root / "unexpected-legacy"
        delegate = scripts / "cargo-admitted"
        delegate.write_text('#!/bin/sh\nprintf "called\\0" >> "$CALLS"\n'
                            'printf "%s\\0" "$@" > "$ARGUMENT_LOG"\nexit "$EXECUTOR_STATUS"\n')
        delegate.chmod(0o755)
        for path in (scripts / "cargo-safe", scripts / "cargo"):
            path.write_text('#!/bin/sh\nprintf called > "$LEGACY_LOG"\nexit 91\n')
            path.chmod(0o755)
        self.env = isolated_environment()
        self.env.update(ARGUMENT_LOG=str(self.arguments), CALLS=str(self.calls),
                        LEGACY_LOG=str(self.legacy), EXECUTOR_STATUS="0",
                        PATH=str(scripts) + os.pathsep + self.env["PATH"])

    def invoke(self, recipe, *args, status=0):
        for path in (self.arguments, self.calls, self.legacy):
            path.unlink(missing_ok=True)
        result = subprocess.run(["just", "--justfile", str(self.root / "justfile"), recipe, *args],
                                cwd=self.root, env=self.env | {"EXECUTOR_STATUS": str(status)},
                                capture_output=True, text=True, timeout=15)
        self.assertFalse(self.legacy.exists(), "recipe fell back to legacy/raw Cargo")
        return result

    def assert_delegated(self, expected):
        self.assertTrue(self.arguments.is_file(), "admitted delegate was not invoked")
        self.assertEqual([os.fsdecode(arg) for arg in self.arguments.read_bytes().split(b"\0")[:-1]], expected)
        self.assertEqual(self.calls.read_bytes(), b"called\0", "admission must be invoked exactly once")

    def test_package_is_required_before_any_execution(self):
        for recipe in ("agent-check", "agent-test"):
            with self.subTest(recipe=recipe):
                result = self.invoke(recipe)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("argument", result.stderr)
                self.assertFalse(self.arguments.exists())
                self.assertFalse(self.calls.exists())

    def test_check_default_is_one_package_through_admission(self):
        result = self.invoke("agent-check", "perl-token")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_delegated(["check", "-p", "perl-token", "--profile", "agent", "--locked"])

    def test_test_requires_a_target_selector_before_any_execution(self):
        # The shortcut requires the selector first; runtime filters alone do not narrow compilation.
        for args in ((), ("",), ("wanted",), ("wanted", "--lib"),
                     ("--", "--lib"), ("--", "--test", "selected"),
                     ("--offline", "--lib"), ("--features", "--lib"),
                     ("--target", "host-triple"), ("--unknown",),
                     ("--lib=extra",), ("--doc=extra",), ("--tests=extra",)):
            with self.subTest(args=args):
                result = self.invoke("agent-test", "perl-token", *args)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertFalse(self.arguments.exists(), "invalid scope reached admission")
                self.assertFalse(self.calls.exists())

    def test_named_selectors_require_an_unambiguous_nonempty_value(self):
        for selector in ("--bin", "--test", "--example", "--bench"):
            for args in ((selector,), (selector, ""), (selector, "--"),
                         (selector, "--lib"), (selector, "-name"), (selector + "=",)):
                with self.subTest(args=args):
                    result = self.invoke("agent-test", "perl-token", *args)
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertFalse(self.arguments.exists(), "missing target value reached admission")
                    self.assertFalse(self.calls.exists())

    def test_explicit_target_forms_and_literal_values_reach_admission(self):
        forms = [(flag,) for flag in ("--lib", "--doc", "--bins", "--tests",
                                     "--examples", "--benches", "--all-targets")]
        target = "target $(touch target-injected) `touch target-too`; *\nquote'\"$cash"
        for selector in ("--bin", "--test", "--example", "--bench"):
            forms.extend(((selector, target), (selector + "=" + target,),
                          (selector + "=-literal-name",)))
        extra = ["--features", "feature with spaces", "--", "--lib", "", "case*"]
        for selection in forms:
            with self.subTest(selection=selection):
                args = [*selection, *extra]
                result = self.invoke("agent-test", "perl-token", *args)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assert_delegated(["test", "-p", "perl-token", "--profile", "agent", "--locked", *args])
        for name in ("target-injected", "target-too"):
            self.assertFalse((self.root / name).exists())

    def test_arguments_and_program_separator_are_literal(self):
        args = ["--test", "specific-target", "--features", "feature with spaces", "--",
                "name $(touch injected) `touch injected-too`;", "", "*", "line\nbreak", "quote'\"$cash"]
        package = "package $(touch package-injected)"
        for operation in ("check", "test"):
            with self.subTest(operation=operation):
                result = self.invoke("agent-" + operation, package, *args)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assert_delegated([operation, "-p", package, "--profile", "agent", "--locked", *args])
        for name in ("injected", "injected-too", "package-injected"):
            self.assertFalse((self.root / name).exists())

    def test_explicit_scope_and_admission_refusal_are_preserved(self):
        args = ["--all-targets", "--offline"]
        for operation in ("check", "test"):
            with self.subTest(operation=operation):
                result = self.invoke("agent-" + operation, "perl-token", *args, status=75)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("75", result.stderr)
                self.assert_delegated([operation, "-p", "perl-token", "--profile", "agent", "--locked", *args])

    def test_vscode_process_task_supplies_a_declared_package_input(self):
        config = json.loads((ROOT / ".vscode/tasks.json").read_text(encoding="utf-8"))
        task = next(task for task in config["tasks"] if task["label"] == "perl-lsp: agent-check")
        self.assertEqual(task["type"], "process")
        self.assertEqual(task["command"], "just")
        self.assertGreaterEqual(len(task["args"]), 2, "VS Code task must supply the required package")
        token = task["args"][1]
        self.assertRegex(token, r"^\$\{input:[A-Za-z0-9_.-]+\}$")
        inputs = [entry for entry in config["inputs"] if entry["id"] == token[8:-1]]
        self.assertEqual(len(inputs), 1, "package input must resolve uniquely")
        self.assertEqual(inputs[0]["type"], "promptString")
        self.assertTrue(inputs[0]["default"].strip())
        result = self.invoke(task["args"][0], "perl-token", *task["args"][2:])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_delegated(["check", "-p", "perl-token", "--profile", "agent", "--locked"])


class FrontdoorWiringTests(unittest.TestCase):
    def test_existing_ci_job_has_just_and_reaches_changed_entrypoints(self):
        workflow = (ROOT / ".github/workflows/ci-gate-self-tests.yml").read_text(encoding="utf-8")
        filters = workflow.split("jobs:", 1)[0]
        for path in ("scripts/devplane-init", "scripts/tests/test_cargo_admitted_frontdoor.py", ".vscode/tasks.json"):
            self.assertTrue("- '" + path + "'" in filters, "missing workflow path: " + path)
        job = workflow.split("  cargo-admitted-storage-self-test:", 1)[1].split("\n  cargo-toolchain-guard-self-test:", 1)[0]
        self.assertIn("tool: just@", job)
        self.assertIn("fallback: none", job)
        self.assertLess(job.index("tool: just@"), job.index("python3 -m unittest discover"))
        self.assertIn("-p 'test_cargo_admitted*.py'", job)


if __name__ == "__main__":
    unittest.main()
