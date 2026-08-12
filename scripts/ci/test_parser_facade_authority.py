#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from parser_facade_authority import check, load_ledger

FIXTURE_LEDGER = Path(__file__).resolve().parents[2] / ".ci/parser-facade"


class ParserFacadeAuthorityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ledger = load_ledger(FIXTURE_LEDGER)
        self.write_fixture()

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def write_ledger(self, ledger: dict[str, object]) -> None:
        mapping = {
            "ruling.json": ["schema_version", "ruling", "sources", "default_features"],
            "features.json": ["schema_version", "features"],
            "dependencies.json": ["schema_version", "dependencies"],
            "public-surface.json": [
                "schema_version", "public_modules", "public_reexport_groups", "targets"
            ],
            "incremental.json": [
                "schema_version", "incremental_public_modules",
                "incremental_public_exports", "incremental_public_functions"
            ],
            "consumers.json": ["schema_version", "consumer_groups"],
        }
        for name, keys in mapping.items():
            self.write(
                f".ci/parser-facade/{name}",
                json.dumps({key: ledger[key] for key in keys}, indent=2) + "\n",
            )

    def write_fixture(self) -> None:
        self.write_ledger(self.ledger)
        features = []
        for row in self.ledger["features"]:
            value = json.dumps(self.ledger["default_features"]) if row["name"] == "default" else "[]"
            features.append(f'{json.dumps(row["name"])} = {value}')
        dependencies = []
        for row in self.ledger["dependencies"]:
            value = '{ version = "1", optional = true }' if row["optional"] else '"1"'
            dependencies.append(f'{row["name"]} = {value}')
        targets = []
        for row in self.ledger["targets"]:
            targets += [f'[[{row["kind"]}]]', f'name = {json.dumps(row["name"])}']
            if row["required_features"]:
                targets.append("required-features = " + json.dumps(row["required_features"]))
        self.write(
            "crates/perl-parser/Cargo.toml",
            '[package]\nname="perl-parser"\nversion="0.1.0"\n\n[dependencies]\n'
            + "\n".join(dependencies)
            + "\n\n[features]\n"
            + "\n".join(features)
            + "\n\n"
            + "\n".join(targets)
            + "\n",
        )
        modules = "\n".join(f'pub mod {row["name"]};' for row in self.ledger["public_modules"])
        exports = "\n".join(
            f"pub use {member};"
            for group in self.ledger["public_reexport_groups"]
            for member in group["members"]
        )
        self.write("crates/perl-parser/src/lib.rs", modules + "\n" + exports + "\n")
        incremental_modules = "\n".join(
            f'pub mod {row["name"]};' for row in self.ledger["incremental_public_modules"]
        )
        incremental_exports = "\n".join(
            f'pub use {row["name"]};' for row in self.ledger["incremental_public_exports"]
        )
        self.write(
            "crates/perl-parser/src/incremental/mod.rs",
            incremental_modules + "\n" + incremental_exports + "\npub fn apply_edits() {}\n",
        )
        self.write(
            "crates/perl-parser-core/Cargo.toml",
            '[package]\nname="perl-parser-core"\nversion="0.1.0"\n\n[dependencies]\nserde="1"\n',
        )
        for consumer in (
            member
            for group in self.ledger["consumer_groups"]
            for member in group["members"]
        ):
            self.write(
                consumer,
                '[package]\nname="consumer"\nversion="0.1.0"\n\n[dependencies]\nperl-parser="1"\n',
            )

    @property
    def ledger_path(self) -> Path:
        return self.root / ".ci/parser-facade"

    def test_complete_fixture_passes(self) -> None:
        _, summary = check(self.root, self.ledger_path)
        self.assertEqual(summary["ruling"], "staged_migration")
        self.assertEqual(summary["public_modules"], len(self.ledger["public_modules"]))

    def test_new_public_module_fails(self) -> None:
        path = self.root / "crates/perl-parser/src/lib.rs"
        path.write_text(path.read_text() + "pub mod surprise;\n")
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_reexport_replacement_fails(self) -> None:
        path = self.root / "crates/perl-parser/src/lib.rs"
        path.write_text(path.read_text().replace("pub use core::Parser;", "pub use core::ParserConfig;"))
        with self.assertRaisesRegex(ValueError, "public re-exports differs"):
            check(self.root, self.ledger_path)

    def test_default_experimental_feature_fails(self) -> None:
        self.ledger["default_features"].append("experimental-features")
        for row in self.ledger["features"]:
            if row["name"] == "experimental-features":
                row["default"] = True
        self.write_ledger(self.ledger)
        cargo = self.root / "crates/perl-parser/Cargo.toml"
        cargo.write_text(
            cargo.read_text().replace(
                '"default" = ["workspace", "lsp-compat", "workspace_refactor"]',
                '"default" = ["workspace", "lsp-compat", "workspace_refactor", "experimental-features"]',
            )
        )
        with self.assertRaisesRegex(ValueError, "experimental feature"):
            check(self.root, self.ledger_path)

    def test_new_consumer_fails(self) -> None:
        self.write(
            "crates/new-consumer/Cargo.toml",
            '[package]\nname="new"\nversion="0.1.0"\n[dependencies]\nperl-parser="1"\n',
        )
        with self.assertRaisesRegex(ValueError, "unclassified=crates/new-consumer/Cargo.toml"):
            check(self.root, self.ledger_path)

    def test_forbidden_parser_core_edge_fails(self) -> None:
        path = self.root / "crates/perl-parser-core/Cargo.toml"
        path.write_text(path.read_text() + 'perl-workspace="1"\n')
        with self.assertRaisesRegex(ValueError, "forbidden product/transport"):
            check(self.root, self.ledger_path)

    def test_incremental_generation_cannot_be_production(self) -> None:
        self.ledger["incremental_public_modules"][0]["production_eligible"] = True
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "historical incremental modules"):
            check(self.root, self.ledger_path)


if __name__ == "__main__":
    unittest.main()
