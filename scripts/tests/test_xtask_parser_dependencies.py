"""Qualify xtask's resolved parser-tool dependency boundary without compiling it.

Run with the installed repository toolchain and populated Cargo cache:
python3 scripts/tests/test_xtask_parser_dependencies.py

CI invokes this proof directly in ci-gate-self-tests.yml's parser-policy-self-test.
Its graph-only setup warms the parser-tasks superset; each assertion then resolves
its own exact feature selection offline. No Rust compilation is needed. The
parser_dependency_graph integration wrapper remains part of full xtask testing.
"""

import os
import pathlib
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


def dependencies(*features):
    command = [
        "cargo", "tree", "--manifest-path", str(ROOT / "Cargo.toml"),
        "-p", "xtask", "--locked", "--offline", "--edges", "normal,build",
        "--prefix", "none", "--format", "{p}",
    ]
    if features:
        command.extend(["--features", ",".join(features)])
    env = dict(os.environ, RUSTUP_AUTO_INSTALL="0")
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, check=True)
    return {line.split()[0] for line in result.stdout.splitlines() if line.strip()}


class ParserDependencyBoundary(unittest.TestCase):
    def test_ordinary_commands_do_not_compile_bindings_toolchain(self):
        graph = dependencies()
        self.assertIn("xtask", graph)
        self.assertIn("perl-lsp-rs-core", graph)
        for package in ("bindgen", "clang-sys", "tree-sitter-perl-c"):
            self.assertTrue(package not in graph, f"ordinary graph unexpectedly selects {package}")

    def test_legacy_parser_does_not_require_bindings_toolchain(self):
        graph = dependencies("legacy")
        self.assertIn("perl-parser-pest", graph)
        for package in ("bindgen", "clang-sys", "tree-sitter-perl-c"):
            self.assertTrue(package not in graph, f"legacy graph unexpectedly selects {package}")

    def test_parser_tasks_retain_bindings_toolchain(self):
        graph = dependencies("parser-tasks")
        for package in ("bindgen", "clang-sys", "tree-sitter-perl-c", "perl-parser-pest"):
            self.assertIn(package, graph)


if __name__ == "__main__":
    unittest.main()
