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

    def test_formatter_package_test_graphs_exclude_core_and_ai_transport(self):
        for package in ("perl-tidy", "perl-tidy-perltidy", "perl-lsp-perltidy"):
            resolved = graph(package, edges="normal,build,dev")
            self.assertIn(package, resolved)
            for name in ("perl-lsp-rs-core", "ureq", "rustls", "ring"):
                with self.subTest(package=package, dependency=name):
                    self.assertNotIn(name, resolved)

    def test_native_graph_has_no_process_or_application_dependency(self):
        resolved = graph("perl-tidy", "--no-default-features")
        self.assertIn("perl-parser-core", resolved)
        self.assertIn("perl-source-identity", resolved)
        for name in ("perl-subprocess-runtime", "perl-lsp-perltidy", "perl-tidy-perltidy",
                     "perl-lsp-rs-core", "perl-lsp-rs", "perl-workspace", "lsp-types"):
            with self.subTest(dependency=name):
                self.assertNotIn(name, resolved)

    def test_external_adapter_has_no_native_or_parser_dependency(self):
        resolved = graph("perl-tidy-perltidy")
        self.assertIn("perl-subprocess-runtime", resolved)
        for name in ("perl-tidy", "perl-lsp-perltidy", "perl-parser-core", "perl-lsp-rs-core"):
            with self.subTest(dependency=name):
                self.assertNotIn(name, resolved)

    def test_first_party_application_graph_retires_the_compatibility_package(self):
        for package in ("perl-lsp-rs-core", "perl-lsp-rs", "xtask"):
            resolved = graph(package)
            with self.subTest(package=package):
                self.assertIn("perl-tidy", resolved)
                self.assertIn("perl-tidy-perltidy", resolved)
                self.assertNotIn("perl-lsp-perltidy", resolved)

    def test_first_party_sources_retire_compatibility_imports(self):
        for source_root in ("crates/perl-lsp-rs-core/src", "crates/perl-lsp-rs/src", "xtask/src"):
            for source in (ROOT / source_root).rglob("*.rs"):
                with self.subTest(source=source.relative_to(ROOT)):
                    self.assertNotIn("perl_lsp_perltidy", source.read_text())

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
