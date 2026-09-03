#!/usr/bin/env python3
"""Focused tests for the Cargo feature role registry validator (#8409).

Every rule the validator enforces has a negative control here: a mutation
that a correct validator must reject.  Without them a green run would only
prove the validator is silent, not that it discriminates.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("validate_cargo_feature_roles.py")
SPEC = importlib.util.spec_from_file_location("cargo_feature_roles", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
# Registered before exec so the module's dataclasses resolve their own module.
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)

ROOT = Path(__file__).resolve().parents[2]


def facts(crate: str, name: str, **overrides: object) -> object:
    defaults = dict(
        crate=crate,
        name=name,
        manifest=f"crates/{crate}/Cargo.toml",
        kind="explicit",
        edges=(),
        cfg_uses=1,
        inbound_refs=(),
        required_by_targets=(),
        in_default_closure=False,
    )
    defaults.update(overrides)
    return validator.FeatureFacts(**defaults)


def registry(rows: list[dict[str, object]], **overrides: object) -> object:
    defaults = dict(
        schema_version=validator.SCHEMA_VERSION,
        policy=validator.POLICY_NAME,
        roles=validator.ROLES,
        consumer_signals=validator.CONSUMER_SIGNALS,
        authority={
            "build_combinations": "#3790",
            "product_maturity": "#6731",
        },
        rows=rows,
    )
    defaults.update(overrides)
    return validator.Registry(**defaults)


def row(**overrides: object) -> dict[str, object]:
    base: dict[str, object] = {
        "crate": "demo",
        "name": "alpha",
        "role": "build_composition",
        "owner": "#8409",
        "consumers": ["cfg_gated"],
    }
    base.update(overrides)
    return base


_WORKSPACE: dict[tuple[str, str], object] | None = None


def workspace() -> dict[tuple[str, str], object]:
    """Discover the live workspace once; the full scan is the slow part."""
    global _WORKSPACE
    if _WORKSPACE is None:
        _WORKSPACE = validator.discover(ROOT)
    return _WORKSPACE


class CurrentRepositoryTests(unittest.TestCase):
    def test_current_registry_matches_current_source(self) -> None:
        self.assertEqual(
            validator.validate(validator.load_registry(ROOT), workspace()), []
        )

    def test_discovery_finds_every_declared_workspace_feature(self) -> None:
        discovered = workspace()
        # A registry covering zero features would make every other rule vacuous.
        self.assertGreater(len(discovered), 50)
        self.assertIn(("perl-lsp-rs", "workspace"), discovered)
        self.assertIn(("xtask", "parser-tasks"), discovered)

    def test_required_features_consumer_is_recorded_for_a_real_target(self) -> None:
        # crates/perl-parser declares [[bin]] perl-parse with
        # required-features = ["cli"]; without reading that table the feature
        # looks unconsumed.
        cli = workspace()[("perl-parser", "cli")]
        self.assertIn("required_features", cli.observed_signals())
        self.assertIn("bin:perl-parse", cli.required_by_targets)

    def test_discovery_is_deterministic_across_runs(self) -> None:
        self.assertEqual(workspace(), validator.discover(ROOT))


class CfgDetectionTests(unittest.TestCase):
    def test_counts_plain_cfg_and_cfg_macro(self) -> None:
        source = '#[cfg(feature = "alpha")]\nfn a() {}\nconst B: bool = cfg!(feature = "alpha");\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"alpha": 2})

    def test_counts_cfg_attr_and_nested_predicates(self) -> None:
        source = (
            '#[cfg_attr(not(feature = "slow"), ignore)]\n'
            '#[cfg(all(not(windows), feature = "deep"))]\n'
        )
        self.assertEqual(
            validator.count_cfg_uses_in_source(source), {"slow": 1, "deep": 1}
        )

    def test_ignores_feature_strings_outside_cfg_forms(self) -> None:
        source = 'let doc = "feature = \\"alpha\\"";\n// feature = "beta"\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {})

    def test_ignores_cfg_in_line_and_block_comments(self) -> None:
        source = (
            '// #[cfg(feature = "ghost")]\n'
            '/* #[cfg(feature = "phantom")] */\n'
            '/** doc: #[cfg(feature = "doc_ghost")] */\n'
            '#[cfg(feature = "real")]\nfn a() {}\n'
        )
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_ignores_cfg_inside_string_and_raw_string_literals(self) -> None:
        source = (
            'let fixture = "#[cfg(feature = \\"quoted\\")]";\n'
            'let raw = r#"#[cfg(feature = "rawghost")]"#;\n'
            '#[cfg(feature = "real")]\nfn a() {}\n'
        )
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_nested_block_comment_does_not_swallow_following_code(self) -> None:
        source = '/* outer /* inner */ */\n#[cfg(feature = "real")]\nfn a() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_paren_in_char_literal_does_not_break_span_matching(self) -> None:
        source = "let close = ')';\n#[cfg(all(unix, feature = \"real\"))]\nfn a() {}\n"
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_lifetime_is_not_treated_as_a_char_literal(self) -> None:
        source = "struct S<'a>(&'a str);\n#[cfg(feature = \"real\")]\nfn a() {}\n"
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_multiline_cfg_attribute_is_counted(self) -> None:
        source = '#[cfg(all(\n    not(windows),\n    feature = "real"\n))]\nfn a() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"real": 1})

    def test_quoted_paren_inside_the_cfg_does_not_close_the_span(self) -> None:
        # A ')' inside a string argument is text, not structure. Closing the
        # span there silently drops a genuinely gated feature.
        source = '#[cfg_attr(doc = "close ) here", feature = "alpha")]\nfn a() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"alpha": 1})

    def test_quoted_paren_in_nested_predicate_does_not_close_the_span(self) -> None:
        source = '#[cfg(all(feature = "beta", doc = "a ) b"))]\nfn b() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"beta": 1})

    def test_quoted_open_paren_does_not_unbalance_the_span(self) -> None:
        source = '#[cfg_attr(doc = "open ( here", feature = "gamma")]\nfn c() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"gamma": 1})

    def test_sequential_cfg_forms_are_both_counted(self) -> None:
        source = '#[cfg(feature = "a")]\nfn a() {}\n#[cfg(feature = "b")]\nfn b() {}\n'
        self.assertEqual(validator.count_cfg_uses_in_source(source), {"a": 1, "b": 1})


class ObservedDispositionTests(unittest.TestCase):
    def test_signals_follow_evidence(self) -> None:
        self.assertEqual(facts("d", "a").observed_signals(), ("cfg_gated",))
        self.assertEqual(
            facts("d", "a", cfg_uses=0, edges=("dep:x",)).observed_signals(),
            ("composition",),
        )
        self.assertEqual(
            facts("d", "a", cfg_uses=0, inbound_refs=("d/b",)).observed_signals(),
            ("propagated",),
        )
        self.assertEqual(
            facts("d", "a", cfg_uses=0, required_by_targets=("bin:x",)).observed_signals(),
            ("required_features",),
        )
        self.assertEqual(facts("d", "a", cfg_uses=0).observed_signals(), ())

    def test_concurrent_signals_are_all_reported_not_collapsed(self) -> None:
        # A single-valued disposition would report only `cfg_gated` here and
        # silently lose the other three, so losing one could not be detected.
        both = facts(
            "d",
            "a",
            cfg_uses=3,
            edges=("dep:x",),
            inbound_refs=("d/b",),
            required_by_targets=("bin:x",),
        )
        self.assertEqual(
            both.observed_signals(),
            ("cfg_gated", "composition", "propagated", "required_features"),
        )

    def test_losing_one_of_several_signals_is_drift(self) -> None:
        discovered = {
            ("demo", "alpha"): facts("demo", "alpha", cfg_uses=1, inbound_refs=())
        }
        errors = validator.validate(
            registry([row(consumers=["cfg_gated", "propagated"])]), discovered
        )
        self.assertTrue(
            any("declares consumers=['cfg_gated', 'propagated']" in e for e in errors),
            errors,
        )


class NegativeControlTests(unittest.TestCase):
    """Each test mutates one thing that a correct validator must reject."""

    def assert_rejects(self, errors: list[str], needle: str) -> None:
        self.assertTrue(
            any(needle in error for error in errors),
            f"expected an error containing {needle!r}, got {errors}",
        )

    def test_same_signal_set_in_another_order_is_an_ordering_error_not_drift(
        self,
    ) -> None:
        discovered = {
            ("demo", "alpha"): facts("demo", "alpha", cfg_uses=1, inbound_refs=("d/b",))
        }
        errors = validator.validate(
            registry([row(consumers=["propagated", "cfg_gated"])]), discovered
        )
        self.assertEqual(len(errors), 1, errors)
        self.assert_rejects(errors, "must be sorted and deduplicated")
        # Equivalent evidence must never be reported as a change in evidence.
        self.assertNotIn("discovery observed", errors[0])

    def test_unreadable_source_tree_is_an_instrument_failure_not_absence(self) -> None:
        # Silently returning zero cfg uses would look exactly like a feature
        # losing its last gate, turning a broken instrument into a finding.
        with tempfile.TemporaryDirectory() as temp:
            missing = Path(temp) / "gone"
            with self.assertRaises(validator.ValidationError):
                validator.crate_sources(missing)

    def test_unregistered_feature_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(registry([]), discovered)
        self.assert_rejects(errors, "unregistered Cargo feature")

    def test_stale_row_fails(self) -> None:
        errors = validator.validate(registry([row()]), {})
        self.assert_rejects(errors, "stale row")

    def test_duplicate_row_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(registry([row(), row()]), discovered)
        self.assert_rejects(errors, "duplicate row")

    def test_unknown_role_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(registry([row(role="invented")]), discovered)
        self.assert_rejects(errors, "unknown role")

    def test_missing_owner_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(registry([row(owner="a person")]), discovered)
        self.assert_rejects(errors, "owner must be an issue reference")

    def test_unknown_key_fails_so_maturity_cannot_leak_in(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(
            registry([row(maturity="proven", advertised=True)]), discovered
        )
        self.assert_rejects(errors, "unknown key(s)")

    def test_consumer_disposition_drift_fails(self) -> None:
        # The row claims the feature is cfg-gated; the source no longer gates it.
        discovered = {("demo", "alpha"): facts("demo", "alpha", cfg_uses=0)}
        errors = validator.validate(registry([row()]), discovered)
        self.assert_rejects(errors, "declares consumers=['cfg_gated']")

    def test_unconsumed_feature_without_migration_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha", cfg_uses=0)}
        errors = validator.validate(
            registry([row(consumers=[])]), discovered
        )
        self.assert_rejects(errors, "requires an explicit 'migration' disposition")

    def test_unconsumed_feature_with_migration_passes(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha", cfg_uses=0)}
        errors = validator.validate(
            registry([row(consumers=[], migration="retire under #8411")]),
            discovered,
        )
        self.assertEqual(errors, [])

    def test_empty_default_needs_no_migration(self) -> None:
        discovered = {("demo", "default"): facts("demo", "default", cfg_uses=0)}
        errors = validator.validate(
            registry([row(name="default", consumers=[])]), discovered
        )
        self.assertEqual(errors, [])

    def test_migration_required_role_without_migration_fails(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(registry([row(role="legacy_alias")]), discovered)
        self.assert_rejects(errors, "requires an explicit 'migration' disposition")

    def test_blank_migration_is_not_a_disposition(self) -> None:
        discovered = {("demo", "alpha"): facts("demo", "alpha")}
        errors = validator.validate(
            registry([row(role="legacy_alias", migration="   ")]), discovered
        )
        self.assert_rejects(errors, "requires an explicit 'migration' disposition")

    def test_test_only_feature_enabled_by_default_fails(self) -> None:
        discovered = {
            ("demo", "helper"): facts("demo", "helper", in_default_closure=True)
        }
        errors = validator.validate(
            registry(
                [
                    row(
                        name="helper",
                        role="test_only",
                        migration="retire under #8411",
                    )
                ]
            ),
            discovered,
        )
        self.assert_rejects(errors, "reachable from the crate's own default feature")

    def test_experimental_feature_enabled_by_default_fails(self) -> None:
        discovered = {
            ("demo", "alpha"): facts("demo", "alpha", in_default_closure=True)
        }
        errors = validator.validate(
            registry([row(role="experimental_opt_in")]), discovered
        )
        self.assert_rejects(errors, "reachable from the crate's own default feature")

    def test_test_implying_name_classified_as_build_fails(self) -> None:
        discovered = {("demo", "test-helpers"): facts("demo", "test-helpers")}
        errors = validator.validate(registry([row(name="test-helpers")]), discovered)
        self.assert_rejects(errors, "the name asserts test status")

    def test_experimental_implying_name_classified_as_product_fails(self) -> None:
        discovered = {
            ("demo", "experimental-x"): facts("demo", "experimental-x")
        }
        errors = validator.validate(
            registry([row(name="experimental-x", role="product_profile")]), discovered
        )
        self.assert_rejects(errors, "the name asserts experimental status")

    def test_roadmap_implying_name_without_exception_fails(self) -> None:
        for name in ("dap-phase3", "classifier-v2", "workspace_refactor"):
            with self.subTest(name=name):
                discovered = {("demo", name): facts("demo", name)}
                errors = validator.validate(registry([row(name=name)]), discovered)
                self.assert_rejects(errors, "asserts roadmap position")

    def test_recorded_name_exception_accepts_the_name(self) -> None:
        discovered = {("demo", "dap-phase3"): facts("demo", "dap-phase3")}
        errors = validator.validate(
            registry(
                [
                    row(
                        name="dap-phase3",
                        name_exception={"roadmap": "renamed under #8411"},
                    )
                ]
            ),
            discovered,
        )
        self.assertEqual(errors, [])

    def test_roadmap_exception_does_not_excuse_a_test_implying_name(self) -> None:
        # The whole point of keying the exception: an exception granted for a
        # roadmap name must not silence the test-status rule for the same row.
        discovered = {
            ("demo", "phase2-test-helpers"): facts("demo", "phase2-test-helpers")
        }
        errors = validator.validate(
            registry(
                [
                    row(
                        name="phase2-test-helpers",
                        role="build_composition",
                        name_exception={"roadmap": "renamed under #8411"},
                    )
                ]
            ),
            discovered,
        )
        self.assertTrue(
            any("asserts test status" in error for error in errors),
            f"expected the test-status rule to still fire, got {errors}",
        )

    def test_bare_string_name_exception_is_rejected(self) -> None:
        # The old blanket form must not keep working, or the rescope is cosmetic.
        discovered = {("demo", "dap-phase3"): facts("demo", "dap-phase3")}
        errors = validator.validate(
            registry([row(name="dap-phase3", name_exception="renamed under #8411")]),
            discovered,
        )
        self.assertTrue(
            any("must be a table keyed by the rule" in error for error in errors),
            f"expected a bare-string rejection, got {errors}",
        )

    def test_unknown_name_exception_rule_is_rejected(self) -> None:
        discovered = {("demo", "dap-phase3"): facts("demo", "dap-phase3")}
        errors = validator.validate(
            registry(
                [
                    row(
                        name="dap-phase3",
                        name_exception={"roadmap": "ok", "whatever": "no"},
                    )
                ]
            ),
            discovered,
        )
        self.assertTrue(
            any("unknown 'name_exception' rule" in error for error in errors),
            f"expected an unknown-rule rejection, got {errors}",
        )

    def test_unsorted_rows_fail(self) -> None:
        discovered = {
            ("demo", "alpha"): facts("demo", "alpha"),
            ("demo", "beta"): facts("demo", "beta"),
        }
        errors = validator.validate(
            registry([row(name="beta"), row(name="alpha")]), discovered
        )
        self.assert_rejects(errors, "must be sorted by (crate, name)")

    def test_widened_role_vocabulary_fails(self) -> None:
        errors = validator.validate(
            registry([], roles=validator.ROLES + ("invented_role",)), {}
        )
        self.assert_rejects(errors, "must match the #8409 vocabulary exactly")

    def test_missing_separate_authority_fails(self) -> None:
        errors = validator.validate(registry([], authority={}), {})
        self.assert_rejects(errors, "build_combinations")
        self.assert_rejects(errors, "product_maturity")

    def test_wrong_schema_version_fails(self) -> None:
        errors = validator.validate(registry([], schema_version=99), {})
        self.assert_rejects(errors, "schema_version must be")

    def test_feature_cycle_fails(self) -> None:
        discovered = {
            ("demo", "alpha"): facts("demo", "alpha", edges=("beta",)),
            ("demo", "beta"): facts("demo", "beta", edges=("alpha",)),
        }
        errors = validator.validate(
            registry([row(name="alpha"), row(name="beta")]), discovered
        )
        self.assert_rejects(errors, "feature cycle")


class DiscoveryUnitTests(unittest.TestCase):
    def write_workspace(self, root: Path, manifests: dict[str, str]) -> None:
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/*"]\n', encoding="utf-8"
        )
        for name, body in manifests.items():
            crate = root / "crates" / name
            (crate / "src").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(body, encoding="utf-8")

    def test_optional_dependency_creates_an_implicit_feature(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[dependencies]\n"
                        'serde = { version = "1", optional = true }\n'
                    )
                },
            )
            discovered = validator.discover(root)
            self.assertIn(("demo", "serde"), discovered)
            self.assertEqual(
                discovered[("demo", "serde")].kind, "implicit_optional_dep"
            )

    def test_dep_reference_suppresses_the_implicit_feature(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[features]\n"
                        'json = ["dep:serde"]\n'
                        "[dependencies]\n"
                        'serde = { version = "1", optional = true }\n'
                    )
                },
            )
            discovered = validator.discover(root)
            self.assertIn(("demo", "json"), discovered)
            self.assertNotIn(("demo", "serde"), discovered)

    def test_optional_build_dependency_creates_an_implicit_feature(self) -> None:
        # Verified against `cargo metadata --no-deps`, which reports `cc` as a
        # feature of a crate whose only mention of it is an optional
        # [build-dependencies] entry.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[build-dependencies]\n"
                        'cc = { version = "1", optional = true }\n'
                    )
                },
            )
            self.assertIn(("demo", "cc"), validator.discover(root))

    def test_optional_target_dependency_creates_an_implicit_feature(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[target.'cfg(unix)'.dependencies]\n"
                        'libc = { version = "0.2", optional = true }\n'
                        "[target.'cfg(windows)'.build-dependencies]\n"
                        'winres = { version = "0.1", optional = true }\n'
                    )
                },
            )
            discovered = validator.discover(root)
            self.assertIn(("demo", "libc"), discovered)
            self.assertIn(("demo", "winres"), discovered)

    def test_optional_dev_dependency_is_not_a_feature(self) -> None:
        # Cargo refuses an optional dev-dependency outright, so inventing a
        # feature for one would fail on a manifest that cannot exist.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[dev-dependencies]\n"
                        'tempfile = { version = "3", optional = true }\n'
                    )
                },
            )
            self.assertNotIn(("demo", "tempfile"), validator.discover(root))

    def test_overlapping_dependency_name_is_not_suppressed_by_substring(self) -> None:
        # `dep:serde_derive` must not suppress the implicit `serde` feature;
        # `cargo metadata` reports `serde` here.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[features]\n"
                        'uses-long = ["dep:serde_derive"]\n'
                        "[dependencies]\n"
                        'serde = { version = "1", optional = true }\n'
                        'serde_derive = { version = "1", optional = true }\n'
                    )
                },
            )
            discovered = validator.discover(root)
            self.assertIn(("demo", "serde"), discovered)
            self.assertNotIn(("demo", "serde_derive"), discovered)

    def test_literal_workspace_member_path_resolves(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "src").mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["."]\n\n[package]\nname = "demo"\n'
                "[features]\nalpha = []\n",
                encoding="utf-8",
            )
            self.assertIn(("demo", "alpha"), validator.discover(root))

    def test_required_features_add_and_remove_changes_the_observed_row(self) -> None:
        manifest = (
            '[package]\nname = "demo"\n'
            "[features]\n"
            "cli = []\n"
        )
        with_target = manifest + '\n[[bin]]\nname = "demo-cli"\nrequired-features = ["cli"]\n'
        for body, expected in ((manifest, ()), (with_target, ("required_features",))):
            with self.subTest(has_target=bool(expected)):
                with tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    self.write_workspace(root, {"demo": body})
                    facts = validator.discover(root)[("demo", "cli")]
                    self.assertEqual(facts.observed_signals(), expected)

    def test_required_features_are_read_from_every_target_table(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[features]\n"
                        "a = []\nb = []\nc = []\nd = []\n"
                        '\n[[test]]\nname = "t"\nrequired-features = ["a"]\n'
                        '\n[[example]]\nname = "e"\nrequired-features = ["b"]\n'
                        '\n[[bench]]\nname = "n"\nrequired-features = ["c"]\n'
                        '\n[lib]\nrequired-features = ["d"]\n'
                    )
                },
            )
            discovered = validator.discover(root)
            for name, label in (("a", "test:t"), ("b", "example:e"), ("c", "bench:n")):
                self.assertIn(label, discovered[("demo", name)].required_by_targets)
            self.assertIn("lib", discovered[("demo", "d")].required_by_targets)

    def test_build_output_under_the_crate_is_not_scanned(self) -> None:
        # A warm `target/` holds generated .rs files; counting their cfg forms
        # would make the observed evidence depend on whether the tree was built.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nghost = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text("pub fn a() {}\n", encoding="utf-8")
            for buried in ("target/debug/build/generated.rs", ".git/hooks/sample.rs"):
                path = crate / buried
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('#[cfg(feature = "ghost")]\nfn g() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "ghost")]
            self.assertEqual(facts.cfg_uses, 0)
            self.assertEqual(facts.observed_signals(), ())

    def test_test_fixtures_are_data_not_compiled_consumers(self) -> None:
        # Cargo compiles src/**, build.rs, and the top-level files of tests/,
        # benches/ and examples/ — never tests/fixtures/**. A .rs file there is
        # read as text by a test. crates/perl-lexer/tests/fixtures/ really does
        # hold `#[cfg(feature = "simd")]` selectors that exist to be scanned,
        # and crediting them marked a documented no-op feature as consumed.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nsimd = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text("pub fn a() {}\n", encoding="utf-8")
            for buried in (
                "tests/fixtures/selector.rs",
                "tests/fixtures/nested/inner.rs",
                "testdata/sample.rs",
                "snapshots/snap.rs",
            ):
                path = crate / buried
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('#[cfg(feature = "simd")]\nfn s() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "simd")]
            self.assertEqual(facts.cfg_uses, 0)
            self.assertEqual(facts.observed_signals(), ())

    def test_inherited_workspace_path_dependency_is_a_member(self) -> None:
        # `dep = { workspace = true }` inherits the root declaration. This
        # workspace states 41 of its 41 in-tree paths that way, so resolving
        # only the literal form would miss how paths are actually declared.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "listed": (
                        '[package]\nname = "listed"\n'
                        '[features]\nalpha = []\n'
                        '[dependencies]\nhidden = { workspace = true }\n'
                    ),
                    "hidden": '[package]\nname = "hidden"\n[features]\nbeta = []\n',
                },
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/listed"]\n'
                '[workspace.dependencies]\nhidden = { path = "crates/hidden" }\n',
                encoding="utf-8",
            )
            discovered = validator.discover(root)
            self.assertIn(("listed", "alpha"), discovered)
            self.assertIn(("hidden", "beta"), discovered)

    def test_root_only_workspace_without_members_is_classified(self) -> None:
        # A root manifest with both [package] and [workspace] and no members
        # is a valid one-crate workspace; rejecting it would fail instead of
        # classifying its features.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "src").mkdir(parents=True)
            (root / "src" / "lib.rs").write_text("", encoding="utf-8")
            (root / "Cargo.toml").write_text(
                '[package]\nname = "solo"\n[workspace]\n[features]\nalpha = []\n',
                encoding="utf-8",
            )
            self.assertIn(("solo", "alpha"), validator.discover(root))

    def test_malformed_members_value_is_an_instrument_failure(self) -> None:
        # Coercing a non-list `members` to "no members" would silently scan only
        # the root package and let every intended member escape classification.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "src").mkdir(parents=True)
            (root / "src" / "lib.rs").write_text("", encoding="utf-8")
            (root / "Cargo.toml").write_text(
                '[package]\nname = "solo"\n[workspace]\nmembers = "crates/listed"\n',
                encoding="utf-8",
            )
            with self.assertRaises(validator.ValidationError):
                validator.member_dirs(root)

    def test_virtual_workspace_without_members_still_fails(self) -> None:
        # The existing error must survive: a virtual manifest naming no members
        # governs nothing, and silently passing would be a denominator hole.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            with self.assertRaises(validator.ValidationError):
                validator.member_dirs(root)

    def test_unlisted_in_tree_path_dependency_is_a_member(self) -> None:
        # Cargo enrols a path dependency living inside the workspace directory
        # even when `members` never names it. If discovery missed it, its
        # features would escape classification entirely — a denominator hole.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "listed": (
                        '[package]\nname = "listed"\n'
                        '[features]\nalpha = []\n'
                        '[dependencies]\nhidden = { path = "../hidden" }\n'
                    ),
                    "hidden": '[package]\nname = "hidden"\n[features]\nbeta = []\n',
                },
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/listed"]\n', encoding="utf-8"
            )
            discovered = validator.discover(root)
            self.assertIn(("listed", "alpha"), discovered)
            self.assertIn(("hidden", "beta"), discovered)

    def test_path_dependency_outside_the_workspace_is_not_a_member(self) -> None:
        # The mirror risk: enrolling an out-of-tree path dependency would demand
        # registry rows for a crate Cargo does not govern, blocking policy CI.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "ws"
            root.mkdir()
            outside = Path(temp) / "outside"
            (outside / "src").mkdir(parents=True)
            (outside / "Cargo.toml").write_text(
                '[package]\nname = "outside"\n[features]\ngamma = []\n', encoding="utf-8"
            )
            (outside / "src" / "lib.rs").write_text("", encoding="utf-8")
            self.write_workspace(
                root,
                {
                    "listed": (
                        '[package]\nname = "listed"\n'
                        '[features]\nalpha = []\n'
                        '[dependencies]\noutside = { path = "../../../outside" }\n'
                    )
                },
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/listed"]\n', encoding="utf-8"
            )
            discovered = validator.discover(root)
            self.assertIn(("listed", "alpha"), discovered)
            self.assertNotIn(("outside", "gamma"), discovered)

    def test_excluded_in_tree_path_dependency_is_not_enrolled(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "listed": (
                        '[package]\nname = "listed"\n'
                        '[features]\nalpha = []\n'
                        '[dependencies]\nskipped = { path = "../skipped" }\n'
                    ),
                    "skipped": '[package]\nname = "skipped"\n[features]\ndelta = []\n',
                },
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/listed"]\n'
                'exclude = ["crates/skipped"]\n',
                encoding="utf-8",
            )
            discovered = validator.discover(root)
            self.assertIn(("listed", "alpha"), discovered)
            self.assertNotIn(("skipped", "delta"), discovered)

    def test_workspace_exclude_removes_a_glob_matched_member(self) -> None:
        # Cargo's [workspace].exclude removes a directory from glob expansion.
        # Without this the excluded crate's features would be demanded in the
        # registry, which would block CI on a manifest that is not a member.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "kept": '[package]\nname = "kept"\n[features]\nalpha = []\n',
                    "dropped": '[package]\nname = "dropped"\n[features]\nbeta = []\n',
                },
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/*"]\nexclude = ["crates/dropped"]\n',
                encoding="utf-8",
            )
            discovered = validator.discover(root)
            self.assertIn(("kept", "alpha"), discovered)
            self.assertNotIn(("dropped", "beta"), discovered)

    def test_workspace_exclude_does_not_drop_an_explicit_member(self) -> None:
        # An explicitly listed member is a member even if exclude names it;
        # dropping it would silently stop governing a real crate.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"kept": '[package]\nname = "kept"\n[features]\nalpha = []\n'},
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/kept"]\nexclude = ["crates/kept"]\n',
                encoding="utf-8",
            )
            self.assertIn(("kept", "alpha"), validator.discover(root))

    def test_include_from_src_into_an_excluded_dir_is_a_compiled_consumer(self) -> None:
        # The exclusion is about what the compiler sees, not about the
        # directory's name. crates/perl-lexer/src/lexer/helpers/cursor.rs really
        # does `include!("../../../tests/fixtures/...inc")` into production src/,
        # so a gate reached that way is compiled in and must be counted.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text(
                'include!("../tests/fixtures/gate.inc");\n', encoding="utf-8"
            )
            buried = crate / "tests" / "fixtures" / "gate.inc"
            buried.parent.mkdir(parents=True, exist_ok=True)
            buried.write_text('#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 1)
            self.assertEqual(facts.observed_signals(), ("cfg_gated",))

    def test_path_attribute_into_an_excluded_dir_is_a_compiled_consumer(self) -> None:
        # included_targets() follows PATH_ATTR_TARGET_RE as well as include!,
        # but only the include! half was pinned. #[path] is the form rustfmt
        # produces for a module living outside its conventional location.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text(
                '#[path = "../tests/fixtures/gate.rs"]\nmod gate;\n', encoding="utf-8"
            )
            buried = crate / "tests" / "fixtures" / "gate.rs"
            buried.parent.mkdir(parents=True, exist_ok=True)
            buried.write_text('#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 1)
            self.assertEqual(facts.observed_signals(), ("cfg_gated",))

    def test_data_named_dir_under_src_is_compiled_source(self) -> None:
        # Cargo compiles the whole of src/** through the module graph, so a
        # directory there is compiled no matter what it is named. This
        # workspace really has src/**/snapshots/ directories.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text("mod snapshots;\n", encoding="utf-8")
            buried = crate / "src" / "snapshots" / "mod.rs"
            buried.parent.mkdir(parents=True, exist_ok=True)
            buried.write_text('#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 1)

    def test_excluded_dir_not_reached_by_include_stays_data(self) -> None:
        # The discriminating half: reaching one file in an excluded directory
        # must not drag in its neighbours. This is the perl-lexer/simd salvage
        # finding — those selectors are read as text, never include!d.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        "[package]\nname = \"demo\"\n"
                        "[features]\nalpha = []\nsimd = []\n"
                    )
                },
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text(
                'include!("../tests/fixtures/gate.inc");\n', encoding="utf-8"
            )
            fixtures = crate / "tests" / "fixtures"
            fixtures.mkdir(parents=True, exist_ok=True)
            (fixtures / "gate.inc").write_text(
                '#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8"
            )
            (fixtures / "selector.rs").write_text(
                '#[cfg(feature = "simd")]\nfn s() {}\n', encoding="utf-8"
            )
            discovered = validator.discover(root)
            self.assertEqual(discovered[("demo", "alpha")].cfg_uses, 1)
            self.assertEqual(discovered[("demo", "simd")].cfg_uses, 0)
            self.assertEqual(discovered[("demo", "simd")].observed_signals(), ())

    def test_include_inside_a_string_literal_is_not_followed(self) -> None:
        # An include! that is quoted text is inert, for the same reason a quoted
        # cfg predicate is not a consumer.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text(
                'const S: &str = "include!(\\"../tests/fixtures/gate.inc\\")";\n',
                encoding="utf-8",
            )
            buried = crate / "tests" / "fixtures" / "gate.inc"
            buried.parent.mkdir(parents=True, exist_ok=True)
            buried.write_text('#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8")
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 0)

    def test_include_of_generated_build_output_is_not_followed(self) -> None:
        # include!(concat!(env!("OUT_DIR"), ...)) names build output, which is
        # why target/ is skipped at all. It has no literal path to resolve.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "src" / "lib.rs").write_text(
                'include!(concat!(env!("OUT_DIR"), "/generated.rs"));\n',
                encoding="utf-8",
            )
            generated = crate / "target" / "generated.rs"
            generated.parent.mkdir(parents=True, exist_ok=True)
            generated.write_text(
                '#[cfg(feature = "alpha")]\nfn g() {}\n', encoding="utf-8"
            )
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 0)

    def test_include_cycle_terminates(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nalpha = []\n'},
            )
            src = root / "crates" / "demo" / "src"
            (src / "lib.rs").write_text('include!("a.rs");\n', encoding="utf-8")
            (src / "a.rs").write_text(
                'include!("lib.rs");\n#[cfg(feature = "alpha")]\nfn g() {}\n',
                encoding="utf-8",
            )
            facts = validator.discover(root)[("demo", "alpha")]
            self.assertEqual(facts.cfg_uses, 1)

    def test_real_test_target_at_the_top_level_still_counts(self) -> None:
        # The exclusion must not swallow `tests/*.rs`, which Cargo does compile.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {"demo": '[package]\nname = "demo"\n[features]\nreal = []\n'},
            )
            crate = root / "crates" / "demo"
            (crate / "tests").mkdir(parents=True, exist_ok=True)
            (crate / "tests" / "it.rs").write_text(
                '#[cfg(feature = "real")]\nfn t() {}\n', encoding="utf-8"
            )
            self.assertEqual(validator.discover(root)[("demo", "real")].cfg_uses, 1)

    def test_default_closure_is_transitive_within_the_crate(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[features]\n"
                        'default = ["outer"]\n'
                        'outer = ["inner"]\n'
                        "inner = []\n"
                    )
                },
            )
            discovered = validator.discover(root)
            self.assertTrue(discovered[("demo", "inner")].in_default_closure)

    def test_cross_crate_default_edges_stay_out_of_the_closure(self) -> None:
        # Cross-crate propagation is #3790's subject, not this registry's.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_workspace(
                root,
                {
                    "demo": (
                        '[package]\nname = "demo"\n'
                        "[features]\n"
                        'default = ["other/thing"]\n'
                    ),
                    "other": ('[package]\nname = "other"\n[features]\nthing = []\n'),
                },
            )
            discovered = validator.discover(root)
            self.assertFalse(discovered[("other", "thing")].in_default_closure)
            self.assertEqual(
                discovered[("other", "thing")].inbound_refs, ("demo/default",)
            )


if __name__ == "__main__":
    unittest.main()
