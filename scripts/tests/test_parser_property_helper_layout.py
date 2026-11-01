"""Guard property-suite selection with Cargo metadata; compile no Rust.

Run: python3 scripts/tests/test_parser_property_helper_layout.py -v
"""

import json
import os
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
TESTS = ROOT / "crates/perl-parser/tests"
CONSUMERS = {
    "prop_deletion",
    "prop_qw",
    "prop_quote_like",
    "prop_whitespace",
    "prop_whitespace_idempotence",
}


class PropertyHelperLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            [
                "cargo",
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--locked",
                "--offline",
            ],
            cwd=ROOT,
            env=dict(os.environ, RUSTUP_AUTO_INSTALL="0"),
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
        parser = next(
            package
            for package in json.loads(result.stdout)["packages"]
            if package["name"] == "perl-parser"
        )
        cls.targets = {target["name"]: target for target in parser["targets"]}

    def test_helper_is_not_a_standalone_test_target(self):
        self.assertTrue(
            "prop_test_utils" not in self.targets,
            "shared helper is registered as a standalone target",
        )
        self.assertFalse((TESTS / "prop_test_utils.rs").exists())
        self.assertTrue((TESTS / "support/prop_test_utils.rs").is_file())
        self.assertFalse(
            any(
                Path(target["src_path"]) == TESTS / "support/prop_test_utils.rs"
                for target in self.targets.values()
            ),
            "shared helper must not be explicitly registered either",
        )

    def test_each_property_suite_remains_a_test_target(self):
        for name in sorted(CONSUMERS):
            with self.subTest(suite=name):
                self.assertEqual(self.targets[name]["kind"], ["test"])
                self.assertTrue(self.targets[name]["test"])

    def test_each_property_suite_includes_the_shared_helper(self):
        for name in sorted(CONSUMERS):
            with self.subTest(suite=name):
                source = (TESTS / f"{name}.rs").read_text(encoding="utf-8")
                self.assertRegex(
                    source,
                    r'(?m)^include!\("support/prop_test_utils\.rs"\);$',
                    f"{name} must include the shared support helper",
                )
                self.assertFalse(
                    'include!("prop_test_utils.rs");' in source,
                    f"{name} still includes the autodiscovered location",
                )


if __name__ == "__main__":
    unittest.main()
