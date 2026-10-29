"""Resolve the real xtask/product Cargo graphs without compiling the task runner."""

import os
import pathlib
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


def graph(package, *options, edges="normal,build"):
    env = dict(os.environ, RUSTUP_AUTO_INSTALL="0")
    command = [
        "cargo", "tree", "--manifest-path", str(ROOT / "Cargo.toml"),
        "-p", package, "--locked", "--offline", "--edges", edges,
        "--prefix", "none", "--format", "{p} [{f}]", *options,
    ]
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, check=True)
    packages = {}
    for line in result.stdout.splitlines():
        name = line.split()[0]
        features = set(line.split("[", 1)[1].split("]", 1)[0].split(",")) - {""}
        packages.setdefault(name, set()).update(features)
    return packages


class AiDependencyBoundary(unittest.TestCase):
    def assert_ai_enabled(self, resolved):
        self.assertTrue({"default", "ai-provider"} <= resolved["perl-lsp-rs-core"])
        for name in ("ureq", "rustls", "ring"):
            self.assertIn(name, resolved)

    def test_ordinary_and_parser_tooling_do_not_compile_ai(self):
        for features in (None, "legacy", "parser-tasks"):
            with self.subTest(features=features):
                options = [] if features is None else ["--features", features]
                resolved = graph("xtask", *options)
                self.assertIn("perl-lsp-rs-core", resolved)
                self.assertNotIn("ai-provider", resolved["perl-lsp-rs-core"])
                for name in ("ureq", "rustls", "ring"):
                    self.assertTrue(name not in resolved, f"tooling unexpectedly selects {name}")
                if features == "parser-tasks":
                    self.assertIn("tree-sitter-perl-c", resolved)

    def test_every_other_consumer_retains_core_defaults(self):
        for package, edges in (
            ("perl-dap", "normal,build"),
            ("perl-lsp-rs", "normal,build"),
            ("perl-parser", "normal,build,dev"),
        ):
            with self.subTest(package=package):
                self.assert_ai_enabled(graph(package, edges=edges))

    def test_perltidy_test_graph_excludes_core_and_ai_transport(self):
        resolved = graph("perl-lsp-perltidy", edges="normal,build,dev")
        self.assertIn("perl-lsp-perltidy", resolved)
        for name in ("perl-lsp-rs-core", "ureq", "rustls", "ring"):
            with self.subTest(package="perl-lsp-perltidy", dependency=name):
                self.assertNotIn(name, resolved)

    def test_core_and_server_defaults_remain_ai_capable(self):
        self.assert_ai_enabled(graph("perl-lsp-rs-core"))
        self.assert_ai_enabled(graph("perl-lsp-rs", "--no-default-features"))

    def test_tooling_can_explicitly_activate_the_existing_ai_feature(self):
        resolved = graph("xtask", "--features", "perl-lsp-rs-core/ai-provider")
        self.assertIn("ai-provider", resolved["perl-lsp-rs-core"])
        for name in ("ureq", "rustls", "ring"):
            self.assertIn(name, resolved)


if __name__ == "__main__":
    unittest.main()
