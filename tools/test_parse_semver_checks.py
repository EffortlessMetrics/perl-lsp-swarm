#!/usr/bin/env python3
"""Focused tests for tools/parse_semver_checks.py.

Falsifiers for the v0.18 waived-break ledger driver: unrecognized tool output
must fail closed, filtered runs must never publish the canonical denominator,
crates absent from the baseline must record a terminal not_applicable, and the
publication exit code must fail on any unresolved instrument error.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).with_name("parse_semver_checks.py")
SPEC = importlib.util.spec_from_file_location("parse_semver_checks", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load {SCRIPT_PATH}")
psc = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = psc
SPEC.loader.exec_module(psc)

HEAD_SHA = "a" * 40
BASELINE_COMMIT = "b" * 40
HEAD_TREE = "c" * 40
TOOL_VERSION = "cargo-semver-checks 0.47.0"

SAMPLE_OUTPUT = """\
--- failure enum_no_repr_variant_added: enum variant added without #[non_exhaustive] ---

Description:
    some description

Failed in:
    `Foo::Bar` in crates/perl-ast/src/lib.rs:42
    `Baz::Qux` in C:\\build\\perl-lsp\\crates\\perl-ast\\src\\lib.rs:97

Checked [https://docs.rs/crate/x/1.0.0] 245 checks: 240 pass, 3 fail, 2 warn, 0 skip
"""


def make_result(name: str, *, status_inputs: str = "pass", exit_code: int = 0,
                fail_count: int = 0, parse_error: str = "",
                not_applicable: bool = False) -> psc.CrateResult:
    return psc.CrateResult(
        name=name,
        exit_code=exit_code,
        pass_count=1,
        fail_count=fail_count,
        parse_error=parse_error,
        not_applicable=not_applicable,
        disposition_basis="baseline_absent" if not_applicable else "",
    )


class LocationRegexTests(unittest.TestCase):
    def test_windows_absolute_path_parses(self):
        line = "    `Foo::Bar` in C:\\build\\perl-lsp\\crates\\perl-ast\\src\\lib.rs:97"
        m = psc.LOCATION_RE.match(line)
        self.assertIsNotNone(m, "Windows drive-letter colon must not break path parsing")
        self.assertEqual(m.group("path"), "C:\\build\\perl-lsp\\crates\\perl-ast\\src\\lib.rs")
        self.assertEqual(m.group("line"), "97")

    def test_posix_relative_path_parses(self):
        m = psc.LOCATION_RE.match("    `Foo::Bar` in crates/perl-ast/src/lib.rs:42")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("path"), "crates/perl-ast/src/lib.rs")
        self.assertEqual(m.group("line"), "42")
        self.assertEqual(m.group("loc"), "`Foo::Bar`")


class ParseHumanOutputTests(unittest.TestCase):
    def test_sample_output_parses_counts_and_locations(self):
        p, f, w, s, failures = psc.parse_human_output(SAMPLE_OUTPUT)
        self.assertEqual((p, f, w, s), (240, 3, 2, 0))
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0].rule, "enum_no_repr_variant_added")
        self.assertEqual(len(failures[0].locations), 2)

    def test_no_summary_raises(self):
        with self.assertRaises(psc.SemverParseError):
            psc.parse_human_output("Compiling finished. All good, no findings.\n")

    def test_duplicate_summary_raises(self):
        with self.assertRaises(psc.SemverParseError):
            psc.parse_human_output(SAMPLE_OUTPUT + SAMPLE_OUTPUT)

    def test_minimal_clean_summary_parses_optional_groups(self):
        text = "Checked [https://docs.rs/crate/x/1.0.0] 12 checks: 12 pass, 0 skip\n"
        p, f, w, s, failures = psc.parse_human_output(text)
        self.assertEqual((p, f, w, s), (12, 0, 0, 0))
        self.assertEqual(failures, [])


class ResultFromRawTests(unittest.TestCase):
    def test_zero_exit_with_unrecognized_output_is_error_not_pass(self):
        result = psc.result_from_raw("perl-fake", 0, "warning: the output format drifted\n")
        self.assertEqual(result.status, "error")
        self.assertIn("no 'Checked", result.parse_error)
        self.assertEqual(result.major_break_count(), 0)

    def test_well_formed_failure_output_is_fail(self):
        result = psc.result_from_raw("perl-fake", 1, SAMPLE_OUTPUT)
        self.assertEqual(result.status, "fail")
        self.assertEqual(result.major_break_count(), 3)
        self.assertTrue(result.raw_report_sha256)


class RatchetListTests(unittest.TestCase):
    def test_parses_names_skipping_comments_and_blanks(self):
        text = "# header comment\n\nperl-lexer\n  perl-symbol  \n# tail\n"
        self.assertEqual(psc.parse_ratchet_list(text), ["perl-lexer", "perl-symbol"])

    def test_empty_list_fails(self):
        with self.assertRaises(SystemExit):
            psc.parse_ratchet_list("# only comments\n")


class FilteredRunGuardTests(unittest.TestCase):
    def test_filtered_run_refuses_canonical_out_dir(self):
        with self.assertRaises(SystemExit):
            psc.check_filtered_out_dir(True, psc.DEFAULT_OUT.resolve())

    def test_filtered_run_allows_draft_out_dir(self):
        draft = psc.REPO / "target" / "semver-ledger-draft"
        psc.check_filtered_out_dir(True, draft)  # must not raise

    def test_full_run_may_target_canonical_out_dir(self):
        psc.check_filtered_out_dir(False, psc.DEFAULT_OUT.resolve())  # must not raise


class BaselineAbsenceTests(unittest.TestCase):
    def test_manifest_absent_at_baseline(self):
        def fake_runner(cmd, **_kwargs):
            class P:
                returncode = 1
            return P()

        self.assertFalse(psc.manifest_exists_at("crates/perl-x/Cargo.toml",
                                                BASELINE_COMMIT, runner=fake_runner))

    def test_manifest_present_at_baseline(self):
        def fake_runner(cmd, **_kwargs):
            class P:
                returncode = 0
            return P()

        self.assertTrue(psc.manifest_exists_at("crates/perl-ast/Cargo.toml",
                                               BASELINE_COMMIT, runner=fake_runner))

    def test_not_applicable_result_shape(self):
        result = psc.crate_not_in_baseline("perl-source-identity",
                                           "crates/perl-source-identity/Cargo.toml",
                                           BASELINE_COMMIT)
        self.assertEqual(result.status, "not_applicable")
        self.assertEqual(result.disposition_basis, "baseline_absent")
        self.assertIn("does not exist at baseline commit", result.disposition_evidence)


class RenderJsonTests(unittest.TestCase):
    def test_provenance_and_posix_ratchet_list(self):
        results = [
            make_result("perl-pass"),
            make_result("perl-break", fail_count=2),
            make_result("perl-source-identity", not_applicable=True),
            make_result("perl-errored", parse_error="no summary", exit_code=1),
        ]
        out = psc.render_json(results, HEAD_SHA, BASELINE_COMMIT, TOOL_VERSION, HEAD_TREE)
        self.assertEqual(out["schema"], 2)
        self.assertEqual(out["ratchet_list"],
                         ".ci/public-api-baselines/ratchet-crates.txt")
        self.assertEqual(out["baseline_commit"], BASELINE_COMMIT)
        self.assertEqual(out["head_tree"], HEAD_TREE)
        self.assertEqual(out["tool_version"], TOOL_VERSION)
        self.assertEqual(out["totals"]["crates_with_breaks"], 1)
        self.assertEqual(out["totals"]["crates_not_applicable"], 1)
        self.assertEqual(out["totals"]["crates_with_tool_errors"], 1)
        self.assertEqual(out["totals"]["major_level_breaks_absorbed"], 2)
        by_name = {c["name"]: c for c in out["crates"]}
        self.assertEqual(by_name["perl-source-identity"]["status"], "not_applicable")
        self.assertEqual(by_name["perl-source-identity"]["disposition"]["basis"],
                         "baseline_absent")
        self.assertNotIn("disposition", by_name["perl-break"])
        self.assertIsInstance(by_name["perl-break"]["raw_report_sha256"], str)


class PublicationExitCodeTests(unittest.TestCase):
    def test_any_error_fails_even_with_waived_breaks(self):
        results = [
            make_result("perl-break", fail_count=11),
            make_result("perl-lsp-rs-core", parse_error="no summary", exit_code=101),
        ]
        self.assertEqual(psc.publication_exit_code(results), 1)

    def test_all_terminal_with_breaks_is_zero(self):
        results = [
            make_result("perl-break", fail_count=3),
            make_result("perl-pass"),
            make_result("perl-new", not_applicable=True),
        ]
        self.assertEqual(psc.publication_exit_code(results), 0)

    def test_all_terminal_without_breaks_is_one(self):
        results = [make_result("perl-pass"), make_result("perl-new", not_applicable=True)]
        self.assertEqual(psc.publication_exit_code(results), 1)


if __name__ == "__main__":
    unittest.main()
