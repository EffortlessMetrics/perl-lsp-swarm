from collections import Counter
import unittest

import xtask_bin_selection as audit


class SelectionTest(unittest.TestCase):
    def test_producer_expansion(self):
        commands = audit.routes((audit.HERE / "required-merge-surface.sh").read_text())
        self.assertIn(
            ["--locked", "--", "--nocapture", "changelog", "changie"], commands
        )
        (exact,) = (args for args in commands if "--exact" in args)
        self.assertEqual(4, len([arg for arg in exact if "::" in arg]))

    def test_expanding_filter_moves_only_runnable_existing_names(self):
        universe = {"owner::one", "owner::two", "owner::ignored", "other::three"}
        narrow = ["--locked", "--", "--exact", "owner::one"]
        broad = ["--locked", "owner", "--"]
        answers = {
            tuple(narrow): {"owner::one"},
            tuple([*narrow, "owner::two"]): {"owner::one", "owner::two"},
            tuple(broad): universe - {"other::three"},
            ("--locked", "--"): universe,
            ("--locked", "--", "--ignored"): {"owner::ignored"},
        }
        known = {"owner::two", "owner::ignored", "removed"}

        def evaluate(commands):
            return audit.report(
                commands, known, lambda args: Counter(answers[tuple(args)])
            )

        before = evaluate([narrow])
        self.assertEqual(before["expansion_control_added"], "owner::two")
        after = evaluate([narrow, broad])
        self.assertEqual(
            before["historical_uncovered"], ["owner::ignored", "owner::two"]
        )
        self.assertEqual(after["historical_selected"], ["owner::two"])
        self.assertEqual(after["historical_uncovered"], ["owner::ignored"])
        self.assertEqual(after["historical_absent"], ["removed"])
        self.assertEqual(after["selected"], ["owner::one", "owner::two"])

    def test_unknown_option_refused(self):
        with self.assertRaises(ValueError):
            audit.routes(
                "cargo test -p xtask --bin xtask --locked -- --include-ignored"
            )

    def test_broader_target_selection_is_refused(self):
        for selector in ("--bins", "--tests", '--bin "$target"', "", "--workspace"):
            with self.subTest(selector=selector), self.assertRaises(ValueError):
                audit.routes(
                    "cargo test -p xtask --bin xtask --locked -- filter\n"
                    f"cargo test -p xtask {selector} --locked -- filter"
                )

    def test_historical_population_integrity(self):
        source = (audit.HERE / "xtask-bin-historical-failures.txt").read_text()
        self.assertEqual(len(audit.historical_paths(source)), 14)
        for changed in ("", source.split("\n", 1)[1], source + source.splitlines()[0]):
            with self.assertRaises(ValueError):
                audit.historical_paths(changed)

    def test_duplicate_libtest_rows_preserve_runnable_instance(self):
        name = "tasks::module_train_live::tests::a_pre_change_snapshot_is_rejected_as_older_schema_not_as_tampering"
        inventory = audit.parse_listing(
            f"{name}: test\n{name}: test\n\n2 tests, 0 benchmarks\n"
        )
        ignored = audit.parse_listing(f"{name}: test\n\n1 test, 0 benchmarks\n")
        route = ["--locked", "--", "--exact", name]

        def listing(args):
            return ignored if "--ignored" in args else inventory

        data = audit.report([route], {name}, listing)
        self.assertEqual(data["inventory_slots"], 2)
        self.assertEqual(data["inventory_count"], 1)
        self.assertEqual(data["routes"][0]["runnable_slots"], 1)
        self.assertEqual(data["routes"][0]["ignored_slots"], 1)
        self.assertEqual(data["historical_selected"], [name])
        with self.assertRaises(ValueError):
            audit.parse_listing(f"{name}: test\n\n2 tests, 0 benchmarks\n")
