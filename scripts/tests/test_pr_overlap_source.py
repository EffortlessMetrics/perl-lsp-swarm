#!/usr/bin/env python3
"""
Focused tests for scripts/pr_overlap.py — typed changed-file sources (#15346).

Claim under proof: a failed changed-file source must be distinct from a
genuinely empty changed-file set. Concretely:

1. A real empty changed-file set (gh exit 0, no output) is ``known_empty``
   and may still produce ``isolated``.
2. Any source failure (non-zero exit, missing gh, rate limit, auth failure,
   malformed payload) is ``unavailable`` and can never produce ``isolated``
   or any other definitive class — every affected pair is ``not_proven``
   with a structured refusal (no prose parsing downstream).
3. One unavailable PR poisons only its own pairs; proven overlap among
   complete PRs survives.
4. Human text and ``--json`` expose the same pair disposition.
5. ``main`` exits distinctly (3) when a cluster source was unavailable.

Run with: python3 scripts/tests/test_pr_overlap_source.py
Returns exit code 0 on all-pass, 1 on any failure.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

_SCRIPTS_DIR = os.path.join(os.path.dirname(__file__), "..")
sys.path.insert(0, _SCRIPTS_DIR)

import importlib.util

_spec = importlib.util.spec_from_file_location(
    "pr_overlap",
    os.path.join(_SCRIPTS_DIR, "pr_overlap.py"),
)
assert _spec is not None and _spec.loader is not None
pr_overlap = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pr_overlap)  # type: ignore[union-attr]

# ---------------------------------------------------------------------------
# gh subprocess stubs
# ---------------------------------------------------------------------------


def _completed(stdout: str, returncode: int = 0, stderr: str = ""):
    proc = subprocess.CompletedProcess(
        args=["gh"], returncode=returncode, stdout=stdout, stderr=stderr
    )
    return proc


def _run_patch(stdout: str = "", returncode: int = 0, stderr: str = ""):
    """Patch pr_overlap.subprocess.run to return a fixed CompletedProcess."""
    return mock.patch.object(
        pr_overlap.subprocess, "run", return_value=_completed(stdout, returncode, stderr)
    )


def _fail_patch(exc: Exception):
    """Patch pr_overlap.subprocess.run to raise the given exception."""
    return mock.patch.object(pr_overlap.subprocess, "run", side_effect=exc)


class TestDiscoverPrFiles(unittest.TestCase):
    """Typed discovery: known / known_empty / unavailable, never [] on failure."""

    def test_nonempty_success_is_known_with_count_and_digest(self) -> None:
        with _run_patch(stdout="a.rs\nb/tests/t.rs\n"):
            state = pr_overlap._discover_pr_files(101)
        self.assertEqual(state["status"], "known")
        self.assertEqual(state["files"], ["a.rs", "b/tests/t.rs"])
        self.assertEqual(state["count"], 2)
        self.assertEqual(len(state["digest"]), 64)

    def test_real_empty_set_is_known_empty_not_unavailable(self) -> None:
        with _run_patch(stdout=""):
            state = pr_overlap._discover_pr_files(102)
        self.assertEqual(state["status"], "known_empty")
        self.assertEqual(state["files"], [])
        self.assertEqual(state["count"], 0)

    def test_nonzero_exit_is_unavailable_with_code_and_detail(self) -> None:
        with _run_patch(returncode=1, stderr="gh: boom"):
            state = pr_overlap._discover_pr_files(103)
        self.assertEqual(state["status"], "unavailable")
        self.assertEqual(state["code"], "gh-pr-view-exit-1")
        self.assertEqual(state["detail"], "gh: boom")
        self.assertIn("gh", state["command"])

    def test_rate_limit_is_explicit_source_state(self) -> None:
        with _run_patch(returncode=1, stderr="API rate limit exceeded for install"):
            state = pr_overlap._discover_pr_files(104)
        self.assertEqual(state["status"], "unavailable")
        self.assertEqual(state["code"], "gh-rate-limit")

    def test_auth_failure_is_explicit_source_state(self) -> None:
        with _run_patch(returncode=1, stderr="HTTP 401: Bad credentials"):
            state = pr_overlap._discover_pr_files(105)
        self.assertEqual(state["status"], "unavailable")
        self.assertEqual(state["code"], "gh-auth")

    def test_missing_gh_binary_is_unspawnable(self) -> None:
        with _fail_patch(FileNotFoundError("gh not found")):
            state = pr_overlap._discover_pr_files(106)
        self.assertEqual(state["status"], "unavailable")
        self.assertEqual(state["code"], "gh-unspawnable")

    def test_malformed_payload_is_unavailable_not_empty(self) -> None:
        # A malformed/non-array payload makes `gh --jq` exit non-zero; that
        # must surface as unavailable, never as a known-empty file set.
        with _run_patch(returncode=1, stderr="jq: error: Cannot index array with \"path\""):
            state = pr_overlap._discover_pr_files(107)
        self.assertEqual(state["status"], "unavailable")
        self.assertNotIn("files", state)

    def test_digest_is_order_canonical(self) -> None:
        with _run_patch(stdout="b.rs\na.rs\n"):
            one = pr_overlap._discover_pr_files(108)
        with _run_patch(stdout="a.rs\nb.rs\n"):
            two = pr_overlap._discover_pr_files(108)
        self.assertEqual(one["digest"], two["digest"])


class TestSourceGateInPairs(unittest.TestCase):
    """Only known/known_empty sources may reach a definitive class."""

    def _pr(self, pr_id: str, source: dict, files: list[str]) -> dict:
        return pr_overlap._normalise_pr(
            {"id": pr_id, "files": files, "source": source}
        )

    def test_unavailable_pair_is_not_proven_never_isolated(self) -> None:
        broken = self._pr(
            "A",
            {"status": "unavailable", "code": "gh-rate-limit", "detail": "x", "command": ["gh"]},
            [],
        )
        good = self._pr("B", {"status": "known"}, ["src/other.rs"])
        result = pr_overlap.classify_pair(broken, good)
        self.assertEqual(result["class"], "not_proven")
        self.assertNotEqual(result["class"], "isolated")
        self.assertIsNone(result["jaccard_files"])
        self.assertEqual(
            result["refusal"]["reason"], "pr_changed_file_source_unavailable"
        )
        self.assertEqual(result["refusal"]["sources"]["a"]["code"], "gh-rate-limit")

    def test_two_known_empty_prs_may_still_be_isolated(self) -> None:
        empty_a = self._pr("A", {"status": "known_empty", "files": []}, [])
        empty_b = self._pr("B", {"status": "known_empty", "files": []}, [])
        result = pr_overlap.classify_pair(empty_a, empty_b)
        self.assertEqual(result["class"], "isolated")

    def test_cluster_failure_maps_to_not_proven_end_to_end(self) -> None:
        # The historical defect: _fetch returned [] and the pair read as
        # isolated. Simulate the same failure through the cluster builder.
        with mock.patch.object(
            pr_overlap,
            "_discover_pr_files",
            return_value={
                "status": "unavailable",
                "code": "gh-pr-view-exit-1",
                "detail": "gh: boom",
                "command": ["gh"],
            },
        ):
            prs = pr_overlap.build_prs_from_cluster(["1", "2"])
        results = pr_overlap.generate_report(prs)
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["class"], "not_proven")
        self.assertNotEqual(results[0]["class"], "isolated")

    def test_input_mode_defaults_to_known_source(self) -> None:
        normalised = pr_overlap._normalise_pr({"id": "A", "files": []})
        self.assertEqual(normalised["source"]["status"], "known")


class TestUnavailablePoisoning(unittest.TestCase):
    """One unavailable PR must not erase proven facts among complete PRs."""

    def test_partial_poison_keeps_complete_pair_definitive(self) -> None:
        broken = {
            "id": "A",
            "files": [],
            "source": {
                "status": "unavailable",
                "code": "gh-auth",
                "detail": "bad credentials",
                "command": ["gh"],
            },
        }
        b = {"id": "B", "files": ["src/x.rs", "src/y.rs"]}
        c = {"id": "C", "files": ["src/x.rs", "src/y.rs", "tests/t.rs"]}
        results = pr_overlap.generate_report([broken, b, c])
        by_pair = {(r["id_a"], r["id_b"]): r for r in results}
        self.assertEqual(by_pair[("A", "B")]["class"], "not_proven")
        self.assertEqual(by_pair[("A", "C")]["class"], "not_proven")
        self.assertEqual(by_pair[("B", "C")]["class"], "sequence-both")

    def test_human_and_json_agree_on_disposition(self) -> None:
        broken = {
            "id": "A",
            "files": [],
            "source": {"status": "unavailable", "code": "gh-rate-limit"},
        }
        b = {"id": "B", "files": ["src/x.rs"]}
        results = pr_overlap.generate_report([broken, b])
        line = pr_overlap.format_result(results[0])
        self.assertIn("not_proven", line)
        self.assertIn("gh-rate-limit", line)
        self.assertNotIn("files=", line.split("—")[0])

    def test_refusal_is_structured_not_prose(self) -> None:
        broken = {
            "id": "A",
            "files": [],
            "source": {"status": "unavailable", "code": "gh-pr-not-found"},
        }
        b = {"id": "B", "files": ["src/x.rs"]}
        results = pr_overlap.generate_report([broken, b])
        refusal = results[0]["refusal"]
        self.assertEqual(refusal["reason"], "pr_changed_file_source_unavailable")
        self.assertEqual(refusal["sources"]["a"]["code"], "gh-pr-not-found")


class TestMainExitCodes(unittest.TestCase):
    """The command exits distinctly when a complete result was requested."""

    def _run_main(self, argv: list[str]) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = pr_overlap.main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_cluster_source_failure_exits_3_with_not_proven_report(self) -> None:
        unavailable = {
            "status": "unavailable",
            "code": "gh-pr-view-exit-1",
            "detail": "gh: boom",
            "command": ["gh"],
        }
        with mock.patch.object(
            pr_overlap, "_discover_pr_files", return_value=unavailable
        ):
            code, out, err = self._run_main(
                ["--cluster", "1", "2", "--json"]
            )
        self.assertEqual(code, pr_overlap.EXIT_SOURCE_UNAVAILABLE)
        self.assertEqual(code, 3)
        self.assertIn("not_proven", out)
        posture = json.loads(err)
        self.assertEqual(posture["posture"], "NOT_PROVEN")
        self.assertEqual(
            posture["reason"], "pr_changed_file_source_unavailable"
        )
        self.assertEqual(posture["unavailable_prs"], ["1", "2"])

    def test_cluster_success_exits_0(self) -> None:
        known_a = {"status": "known", "files": ["a.rs"], "count": 1, "digest": "x"}
        known_b = {"status": "known", "files": ["b.rs"], "count": 1, "digest": "y"}
        with mock.patch.object(
            pr_overlap, "_discover_pr_files", side_effect=[known_a, known_b]
        ):
            code, out, _err = self._run_main(["--cluster", "1", "2", "--json"])
        self.assertEqual(code, pr_overlap.EXIT_OK)
        results = json.loads(out)
        self.assertEqual(results[0]["class"], "isolated")

    def test_input_mode_is_unaffected_by_source_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "in.json")
            with open(path, "w", encoding="utf-8") as fh:
                json.dump(
                    {"prs": [{"id": 1, "files": ["a.rs"]}, {"id": 2, "files": ["b.rs"]}]},
                    fh,
                )
            code, out, _err = self._run_main([path, "--json"])
        self.assertEqual(code, pr_overlap.EXIT_OK)
        self.assertEqual(json.loads(out)[0]["class"], "isolated")


if __name__ == "__main__":
    unittest.main(verbosity=2)
