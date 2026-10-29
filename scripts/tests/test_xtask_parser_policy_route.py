"""Guard the direct graph-only parser-policy route and its failure semantics."""

import importlib.util
import pathlib
import re
import subprocess
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "parser_dependency_policy", ROOT / "scripts/tests/test_xtask_parser_dependencies.py"
)
POLICY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POLICY)


class ParserPolicyRoute(unittest.TestCase):
    def test_feature_queries_only_inspect_the_selected_graph(self):
        for features in ((), ("legacy",), ("parser-tasks",)):
            with self.subTest(features=features), mock.patch.object(
                POLICY.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "xtask\n", "")
            ) as run:
                self.assertEqual(POLICY.dependencies(*features), {"xtask"})
                command = run.call_args.args[0]
                self.assertEqual(command[:2], ["cargo", "tree"])
                for flag in ("--locked", "--offline"):
                    self.assertIn(flag, command)
                self.assertEqual(command[command.index("-p") + 1], "xtask")
                self.assertEqual(command[command.index("--edges") + 1], "normal,build")
                self.assertNotIn("--all-features", command)
                if features:
                    self.assertEqual(command[command.index("--features") + 1], features[0])
                else:
                    self.assertNotIn("--features", command)
                self.assertTrue(run.call_args.kwargs["check"])

    def test_graph_command_failure_cannot_become_empty_success(self):
        failure = subprocess.CalledProcessError(101, ["cargo", "tree"])
        with mock.patch.object(POLICY.subprocess, "run", side_effect=failure):
            with self.assertRaises(subprocess.CalledProcessError):
                POLICY.dependencies()

    def test_policy_keeps_both_forbidden_and_required_dependency_controls(self):
        case = POLICY.ParserDependencyBoundary()
        with mock.patch.object(POLICY, "dependencies", return_value={"xtask", "perl-lsp-rs-core", "bindgen"}):
            with self.assertRaises(AssertionError):
                case.test_ordinary_commands_do_not_compile_bindings_toolchain()
        with mock.patch.object(POLICY, "dependencies", return_value={"xtask", "perl-parser-pest"}):
            with self.assertRaises(AssertionError):
                case.test_parser_tasks_retain_bindings_toolchain()
        self.assertEqual(unittest.defaultTestLoader.loadTestsFromTestCase(POLICY.ParserDependencyBoundary).countTestCases(), 3)

    def test_ci_runs_the_proof_directly_and_keeps_full_wrapper(self):
        workflow = (ROOT / ".github/workflows/ci-gate-self-tests.yml").read_text()
        match = re.search(r"^  parser-policy-self-test:\n(.*?)(?=^  [a-z][a-z0-9-]*:|\Z)", workflow, re.M | re.S)
        self.assertIsNotNone(match, "direct policy CI job missing")
        job = match.group(1)
        for script in ("test_xtask_parser_policy_route.py", "test_xtask_parser_dependencies.py"):
            self.assertIn("run: python3 scripts/tests/" + script, job)
            self.assertIn("      - 'scripts/tests/" + script + "'", workflow)
        self.assertIn("run: cargo tree -p xtask --features parser-tasks --locked --edges normal,build", job)
        self.assertNotRegex(job, r"cargo\s+(?:build|test|run|xtask|check|clippy)\b")
        self.assertNotIn("continue-on-error", job)
        for path in ("xtask/Cargo.toml", "crates/**/Cargo.toml", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"):
            self.assertIn("      - '" + path + "'", workflow)
        wrapper = (ROOT / "xtask/tests/parser_dependency_graph.rs").read_text()
        self.assertIn("test_xtask_parser_dependencies.py", wrapper)
        self.assertIn("output.status.success()", wrapper)


if __name__ == "__main__":
    unittest.main()
