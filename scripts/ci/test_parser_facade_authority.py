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

    DEPENDENCY_TABLES = {
        "normal": "dependencies",
        "dev": "dev-dependencies",
        "build": "build-dependencies",
    }

    def dependency_section(self, context: str) -> str:
        if context.startswith("target:"):
            table = self.DEPENDENCY_TABLES[context.removeprefix("target:")]
            return f"target.'cfg(unix)'.{table}"
        return self.DEPENDENCY_TABLES[context]

    def write_feature_gates(self, names: list[str], test_names: list[str] | None = None) -> None:
        """Reproduce the cfg predicates that make a feature a real source boundary.

        Production gates live under `src/`; test-profile gates live under `tests/`, so a
        feature that only gates test source cannot be presented as a production boundary.
        """
        def body(values: list[str]) -> str:
            return "".join(
                f'#[cfg(feature = "{name}")]\nfn gate_{index}() {{}}\n'
                for index, name in enumerate(values)
            )

        self.write("crates/perl-parser/src/feature_gates.rs", body(names))
        self.write(
            "crates/perl-parser/tests/feature_profiles.rs",
            body(test_names if test_names is not None else self.test_gated_features),
        )

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
        optional_dependency = next(
            row["name"] for row in self.ledger["dependencies"] if row["optional"]
        )
        features = []
        self.gated_features: list[str] = []
        self.test_gated_features: list[str] = []
        for row in self.ledger["features"]:
            isolation = row["isolation"]
            if isolation == "feature_aggregate":
                value = list(self.ledger["default_features"])
            elif isolation in ("dependencies_and_source", "dependencies_only"):
                value = [optional_dependency]
            else:
                value = []
            if isolation in ("dependencies_and_source", "source_only"):
                self.gated_features.append(row["name"])
            elif isolation == "test_source_only":
                self.test_gated_features.append(row["name"])
            features.append(f'{json.dumps(row["name"])} = {json.dumps(value)}')
        sections: dict[str, list[str]] = {}
        for row in self.ledger["dependencies"]:
            value = '{ version = "1", optional = true }' if row["optional"] else '"1"'
            for context in row["contexts"]:
                sections.setdefault(
                    self.dependency_section(context), []
                ).append(f'{row["name"]} = {value}')
        targets = []
        for row in self.ledger["targets"]:
            targets += [f'[[{row["kind"]}]]', f'name = {json.dumps(row["name"])}']
            if row["required_features"]:
                targets.append("required-features = " + json.dumps(row["required_features"]))
        manifest = ['[package]\nname="perl-parser"\nversion="0.1.0"\n']
        for header, lines in sections.items():
            manifest.append(f"[{header}]\n" + "\n".join(lines) + "\n")
        manifest.append("[features]\n" + "\n".join(features) + "\n")
        manifest.append("\n".join(targets) + "\n")
        self.write("crates/perl-parser/Cargo.toml", "\n".join(manifest))
        self.write_feature_gates(self.gated_features)
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

    def test_inline_public_module_fails(self) -> None:
        path = self.root / "crates/perl-parser/src/lib.rs"
        path.write_text(path.read_text() + "pub mod surprise { pub fn hidden() {} }\n")
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_reexport_replacement_fails(self) -> None:
        path = self.root / "crates/perl-parser/src/lib.rs"
        path.write_text(path.read_text().replace("pub use core::Parser;", "pub use core::ParserConfig;"))
        with self.assertRaisesRegex(ValueError, "public re-exports differs"):
            check(self.root, self.ledger_path)

    def test_reexport_alias_fails(self) -> None:
        path = self.root / "crates/perl-parser/src/lib.rs"
        path.write_text(path.read_text().replace(
            "pub use core::Parser;", "pub use core::Parser as parser_alias;"
        ))
        with self.assertRaisesRegex(ValueError, "public re-exports differs"):
            check(self.root, self.ledger_path)

    def test_aliased_consumer_dependency_is_normalized(self) -> None:
        path = self.root / self.ledger["consumer_groups"][0]["members"][0]
        path.write_text(path.read_text().replace(
            '[dependencies]\nperl-parser="1"',
            '[dependencies]\nparser_alias = { package = "perl-parser", version = "1" }',
        ))
        check(self.root, self.ledger_path)

    def test_dev_only_consumer_cannot_claim_production_usage(self) -> None:
        path = self.root / self.ledger["consumer_groups"][0]["members"][0]
        path.write_text(path.read_text().replace(
            '[dependencies]\nperl-parser="1"',
            '[dev-dependencies]\nperl-parser="1"',
        ))
        with self.assertRaisesRegex(ValueError, "claims production usage but reaches the facade as dev_only"):
            check(self.root, self.ledger_path)

    def test_partially_dev_consumer_group_must_declare_mixed_usage(self) -> None:
        group = next(g for g in self.ledger["consumer_groups"] if len(g["members"]) > 1)
        path = self.root / group["members"][0]
        path.write_text(path.read_text().replace(
            '[dependencies]\nperl-parser="1"',
            '[dev-dependencies]\nperl-parser="1"',
        ))
        with self.assertRaisesRegex(ValueError, "reaches the facade as mixed"):
            check(self.root, self.ledger_path)

    def test_ruling_cannot_repoint_governed_source(self) -> None:
        self.ledger["sources"]["manifest"] = "fixtures/alternate/Cargo.toml"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "governed source paths differ"):
            check(self.root, self.ledger_path)

    def test_digest_covers_full_ledger(self) -> None:
        _, baseline = check(self.root, self.ledger_path)
        self.ledger["ruling"]["next_implementation"] = "#9999"
        self.write_ledger(self.ledger)
        _, changed = check(self.root, self.ledger_path)
        self.assertNotEqual(baseline["authority_digest"], changed["authority_digest"])

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

    @property
    def manifest_path(self) -> Path:
        return self.root / "crates/perl-parser/Cargo.toml"

    def add_dependency(self, section: str, line: str) -> None:
        """Insert a dependency into an existing table, or create the table once."""
        text = self.manifest_path.read_text()
        header = f"[{section}]"
        if header in text:
            text = text.replace(header, f"{header}\n{line}", 1)
        else:
            text = f"{text}\n{header}\n{line}\n"
        self.manifest_path.write_text(text)

    def move_dependency(self, name: str, source: str, destination: str) -> None:
        text = self.manifest_path.read_text()
        for candidate in (f'{name} = "1"', f'{name} = {{ version = "1", optional = true }}'):
            if candidate in text:
                text = text.replace(f"{candidate}\n", "", 1)
                self.manifest_path.write_text(text)
                self.add_dependency(destination, candidate)
                return
        raise AssertionError(f"fixture does not declare {name} in {source}")

    def dependency_row(self, name: str) -> dict:
        return next(row for row in self.ledger["dependencies"] if row["name"] == name)

    def feature_row(self, name: str) -> dict:
        return next(row for row in self.ledger["features"] if row["name"] == name)

    # --- dependency universe -------------------------------------------------

    def test_new_normal_dependency_without_a_row_fails(self) -> None:
        self.add_dependency("dependencies", 'surprise = "1"')
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_new_dev_dependency_without_a_row_fails(self) -> None:
        self.add_dependency("dev-dependencies", 'surprise = "1"')
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_new_build_dependency_without_a_row_fails(self) -> None:
        self.add_dependency("build-dependencies", 'surprise = "1"')
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_new_target_dependency_without_a_row_fails(self) -> None:
        self.add_dependency("target.'cfg(windows)'.dependencies", 'surprise = "1"')
        with self.assertRaisesRegex(ValueError, "unclassified=surprise"):
            check(self.root, self.ledger_path)

    def test_dependency_context_drift_fails(self) -> None:
        """A production dependency silently demoted to dev must not stay production."""
        row = next(
            row for row in self.ledger["dependencies"]
            if row["contexts"] == ["normal"] and not row["optional"]
        )
        self.move_dependency(row["name"], "dependencies", "dev-dependencies")
        with self.assertRaisesRegex(ValueError, f'dependency {row["name"]} claims contexts'):
            check(self.root, self.ledger_path)

    def test_target_dependency_context_is_distinguished_from_plain_normal(self) -> None:
        """A target-qualified edge is not the same denominator row as an unconditional one."""
        row = next(
            row for row in self.ledger["dependencies"]
            if row["contexts"] == ["normal"] and not row["optional"]
        )
        self.move_dependency(row["name"], "dependencies", "target.'cfg(unix)'.dependencies")
        with self.assertRaisesRegex(ValueError, "claims contexts \\['normal'\\]"):
            check(self.root, self.ledger_path)

    def test_test_dev_only_cannot_cover_a_production_dependency(self) -> None:
        row = self.dependency_row("perl-lexer")
        row["classification"] = "test_dev_only"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "reachable from a production dependency context"):
            check(self.root, self.ledger_path)

    # --- feature isolation truth --------------------------------------------

    def test_taxonomy_feature_cannot_claim_source_isolation(self) -> None:
        row = next(r for r in self.ledger["features"] if r["isolation"] == "taxonomy_only")
        row["isolation"] = "source_only"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "but selects taxonomy_only"):
            check(self.root, self.ledger_path)

    def test_removing_a_feature_cfg_gate_invalidates_source_isolation(self) -> None:
        gated = [name for name in self.gated_features if self.feature_row(name)["isolation"] == "source_only"]
        self.write_feature_gates([name for name in self.gated_features if name != gated[0]])
        with self.assertRaisesRegex(ValueError, f"feature {gated[0]} claims isolation source_only"):
            check(self.root, self.ledger_path)

    def test_test_only_gate_cannot_claim_production_source_isolation(self) -> None:
        """A feature gated only from tests is a test profile, not a source boundary."""
        row = next(r for r in self.ledger["features"] if r["isolation"] == "test_source_only")
        row["isolation"] = "source_only"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(
            ValueError, f'feature {row["name"]} claims isolation source_only but selects test_source_only'
        ):
            check(self.root, self.ledger_path)

    def test_moving_a_gate_from_src_to_tests_downgrades_isolation(self) -> None:
        gated = next(
            r["name"] for r in self.ledger["features"] if r["isolation"] == "source_only"
        )
        self.write_feature_gates(
            [name for name in self.gated_features if name != gated],
            self.test_gated_features + [gated],
        )
        with self.assertRaisesRegex(ValueError, "but selects test_source_only"):
            check(self.root, self.ledger_path)

    def test_feature_isolation_value_must_be_supported(self) -> None:
        self.feature_row("cli")["isolation"] = "architectural_boundary"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "isolation is unsupported"):
            check(self.root, self.ledger_path)

    def test_missing_feature_isolation_fails(self) -> None:
        del self.feature_row("cli")["isolation"]
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "isolation must be a non-empty string"):
            check(self.root, self.ledger_path)

    # --- pending evidence ----------------------------------------------------

    def test_review_row_without_pending_evidence_fails(self) -> None:
        row = next(r for r in self.ledger["features"] if r["disposition"] == "review")
        del row["pending"]
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "pending must be an object for a review disposition"):
            check(self.root, self.ledger_path)

    def test_pending_requires_every_named_field(self) -> None:
        row = next(r for r in self.ledger["features"] if r["disposition"] == "review")
        del row["pending"]["resolves_when"]
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "pending.resolves_when must be a non-empty string"):
            check(self.root, self.ledger_path)

    def test_pending_owner_must_be_an_issue_reference(self) -> None:
        row = next(r for r in self.ledger["features"] if r["disposition"] == "review")
        row["pending"]["owner"] = "the parser team"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "pending.owner must be a GitHub issue reference"):
            check(self.root, self.ledger_path)

    def test_pending_predecessor_must_be_an_issue_reference_or_none(self) -> None:
        row = next(r for r in self.ledger["features"] if r["disposition"] == "review")
        row["pending"]["predecessor"] = "later"
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "pending.predecessor must be a GitHub issue reference"):
            check(self.root, self.ledger_path)

    def test_settled_row_cannot_carry_pending_evidence(self) -> None:
        row = next(r for r in self.ledger["features"] if r["disposition"] != "review")
        row["pending"] = {
            "owner": "#7063", "predecessor": "none",
            "reason": "unsettled", "resolves_when": "later",
        }
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "pending is only valid for a review disposition"):
            check(self.root, self.ledger_path)

    def test_incremental_generation_cannot_be_production(self) -> None:
        self.ledger["incremental_public_modules"][0]["production_eligible"] = True
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "historical incremental modules"):
            check(self.root, self.ledger_path)

    def test_unledgered_incremental_export_fails(self) -> None:
        # The source keeps every export written by setUp; only the ledger loses a
        # row. This is the drift shape that reddened main in #14212: an export
        # reaches the facade with no classification behind it.
        self.ledger["incremental_public_exports"] = [
            row
            for row in self.ledger["incremental_public_exports"]
            if row["name"] != "geometry_attachment::SourceGeometryAttachment"
        ]
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(
            ValueError, "unclassified=geometry_attachment::SourceGeometryAttachment"
        ):
            check(self.root, self.ledger_path)

    def test_incremental_export_cannot_be_promoted_to_production(self) -> None:
        # The canonical production set is a reviewed list, so a staged export
        # cannot become production-eligible by editing its own row alone.
        for row in self.ledger["incremental_public_exports"]:
            if row["name"] == "geometry_attachment::SourceGeometryAttachment":
                row["production_eligible"] = True
        self.write_ledger(self.ledger)
        with self.assertRaisesRegex(ValueError, "canonical incremental export marker"):
            check(self.root, self.ledger_path)


if __name__ == "__main__":
    unittest.main()
