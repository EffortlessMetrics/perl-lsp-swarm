#!/usr/bin/env python3
"""Discriminating oracle and workflow controls for the definition protocol probe."""

import importlib.util
from pathlib import Path
import time
import unittest
from unittest.mock import patch

PROBE_PATH = Path(__file__).with_name("qualified_definition_protocol_probe.py")
SPEC = importlib.util.spec_from_file_location("definition_protocol_probe", PROBE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {PROBE_PATH}")
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)
ROOT = PROBE_PATH.resolve().parents[2]
CALLER_PATH = Path.cwd().resolve() / "probe-fixture" / "app" / "main.pl"
TARGET_PATH = Path.cwd().resolve() / "probe-fixture" / "lib" / "Scale24" / "Mod00.pm"


def location(path: Path, line: int):
    return [{"uri": path.as_uri(), "range": {"start": {"line": line, "character": 0},
                                           "end": {"line": line, "character": 36}}}]


def successful_report():
    return {"observations": {"qualified_before_target_open": [],
                             "bare_local_control": location(CALLER_PATH, 5),
                             "qualified_after_target_open": location(TARGET_PATH, 3),
                             "qualified_inside_package_block": [],
                             "qualified_alias_after_edit": location(CALLER_PATH, 1)},
            "position_encoding": "utf-16", "exit": 0, "cleanup": "protocol_exit_reaped",
            "binary_sha256_before": "a" * 64, "binary_sha256_after": "a" * 64}


class OracleTests(unittest.TestCase):
    def outcomes(self, report):
        return PROBE.assertions(report, CALLER_PATH, TARGET_PATH)

    def test_correct_refusal_local_call_and_recovery_pass(self):
        self.assertTrue(all(self.outcomes(successful_report()).values()))

    def test_historical_wrong_caller_is_red_while_controls_stay_green(self):
        report = successful_report()
        report["observations"]["qualified_before_target_open"] = location(CALLER_PATH, 5)
        outcomes = self.outcomes(report)
        self.assertFalse(outcomes.pop("missing_index_never_returns_wrong_package"))
        self.assertTrue(all(outcomes.values()))

    def test_decoy_and_wrong_target_line_are_rejected(self):
        report = successful_report()
        for bad in (location(CALLER_PATH.with_name("Decoy.pm"), 1), location(TARGET_PATH, 2)):
            report["observations"]["qualified_before_target_open"] = bad
            self.assertFalse(self.outcomes(report)["missing_index_never_returns_wrong_package"])

    def test_always_empty_implementation_fails_both_positive_controls(self):
        report = successful_report()
        report["observations"] = {key: [] for key in report["observations"]}
        outcomes = self.outcomes(report)
        self.assertTrue(outcomes["missing_index_never_returns_wrong_package"])
        self.assertFalse(outcomes["bare_local_declaration_retained"])
        self.assertFalse(outcomes["target_didOpen_recovers_exact_declaration"])
        self.assertFalse(outcomes["exact_qualified_alias_retained_after_edit"])

    def test_enclosing_package_and_unobserved_block_call_are_rejected(self):
        report = successful_report()
        report["observations"]["qualified_inside_package_block"] = location(CALLER_PATH, 0)
        self.assertFalse(self.outcomes(report)["unresolved_call_never_returns_enclosing_package"])
        del report["observations"]["qualified_inside_package_block"]
        self.assertFalse(self.outcomes(report)["unresolved_call_never_returns_enclosing_package"])

    def test_unobserved_request_is_not_a_trustworthy_empty_answer(self):
        report = successful_report()
        del report["observations"]["qualified_before_target_open"]
        self.assertFalse(self.outcomes(report)["missing_index_never_returns_wrong_package"])

    def test_forced_kill_or_mutated_binary_cannot_pass(self):
        report = successful_report()
        report["cleanup"] = "owned_process_killed_reaped"
        self.assertFalse(self.outcomes(report)["server_clean_exit"])
        report = successful_report()
        report["binary_sha256_after"] = "b" * 64
        self.assertFalse(self.outcomes(report)["binary_unchanged"])
        del report["binary_sha256_before"]
        del report["binary_sha256_after"]
        self.assertFalse(self.outcomes(report)["binary_unchanged"])

    def test_uri_encoding_and_exact_single_target(self):
        answer = location(TARGET_PATH, 3)
        # Encode only the filesystem drive separator, never the file scheme.
        answer[0]["uri"] = location(TARGET_PATH, 3)[0]["uri"].replace("file:///C:", "file:///C%3A")
        self.assertTrue(PROBE.at_declaration(answer, TARGET_PATH, 3))
        self.assertFalse(PROBE.at_declaration(answer * 2, TARGET_PATH, 3))

    def test_stock_environment_removes_test_and_logging_overrides(self):
        with patch.dict(PROBE.os.environ, {"PERL_LSP_E2E": "1", "perl_lsp_log": "trace",
                                          "LSP_TEST_FALLBACKS": "1", "LC_ALL": "C.UTF-8",
                                          "PATH": "kept"}, clear=True):
            self.assertEqual(PROBE.stock_environment(), {"PATH": "kept"})

    def test_rpc_error_or_missing_result_is_instrument_failure(self):
        for response in ({"id": 1, "error": {"code": -32603}}, {"id": 1}):
            session = PROBE.Session.__new__(PROBE.Session)
            session.serial = 0
            session.deadline = time.perf_counter() + 30
            session.report = {"requests": []}
            session.send = lambda message: None
            session.receive = lambda deadline: response
            with self.assertRaises(ValueError):
                session.request("textDocument/definition", {})

    def test_only_document_supersession_retries_with_one_operation_deadline(self):
        session = PROBE.Session.__new__(PROBE.Session)
        session.deadline = time.perf_counter() + 30
        attempts = []

        def request(method, params, operation_deadline):
            attempts.append(operation_deadline)
            if len(attempts) == 1:
                raise PROBE.SupersededRequest("document moved")
            return []

        session.request = request
        self.assertEqual(session.request_after_edit("textDocument/definition", {}), [])
        self.assertEqual(attempts[0], attempts[1])
        self.assertLessEqual(attempts[0], time.perf_counter() + PROBE.REQUEST_SECONDS)
        session.request = lambda *args, **kwargs: (_ for _ in ()).throw(ValueError("server failure"))
        with self.assertRaises(ValueError):
            session.request_after_edit("textDocument/definition", {})

    def test_probe_positions_point_at_the_callable_in_utf16(self):
        lines = PROBE.CALLER.splitlines()
        self.assertEqual(lines[6][24], "m")
        self.assertEqual(lines[7][8], "m")
        self.assertTrue(lines[5].startswith("sub compute_0"))
        self.assertTrue(PROBE.TARGET_TEXT.splitlines()[3].startswith("sub compute_0"))
        self.assertEqual(PROBE.PACKAGE_BLOCK[PROBE.PACKAGE_BLOCK.index("compute_0") + 2], "m")
        self.assertEqual(PROBE.QUALIFIED_ALIAS.splitlines()[2][15], "m")


class WorkflowTests(unittest.TestCase):
    def test_existing_windows_job_uses_own_built_binary_and_retains_receipt(self):
        text = (ROOT / ".github/workflows/vscode-installed-first-hour-windows.yml").read_text(encoding="utf-8")
        self.assertIn("      - 'scripts/ci/qualified_definition_protocol_probe.py'", text)
        self.assertIn("      - 'scripts/ci/test_qualified_definition_protocol_probe.py'", text)
        build = text.index("      - name: Build current release server and DAP")
        probe = text.index("      - name: Qualified definition protocol regression")
        installed = text.index("      - name: Run installed first-hour byte leaf")
        self.assertLess(build, probe)
        self.assertLess(probe, installed)
        step = text[probe:installed]
        self.assertIn("working-directory: .", step)
        self.assertIn("${{ runner.temp }}/perl-lsp-first-hour-target/release/perllsp.exe", step)
        self.assertIn("${{ github.event.pull_request.head.sha }}", step)
        self.assertIn("--binary", step)
        self.assertIn("--source-sha", step)
        self.assertIn("target/receipts/vscode-smoke/installed-first-hour-windows/qualified-definition-protocol", step)
        self.assertIn("if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }", step)
        self.assertIn("permissions:\n  contents: read", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn("      - name: Upload installed first-hour receipts\n        if: always()", text)
        self.assertNotIn("GITHUB_TOKEN", step)


if __name__ == "__main__":
    unittest.main()
