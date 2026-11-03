#!/usr/bin/env python3
"""Whole-tree regression for issue #18049.

PR #16633 replaced the reviewed ledger stanza for `shogo82148/actions-setup-perl`
instead of adding one, which orphaned `.github/workflows/vscode-installed-first-hour-windows.yml`
even though that file was outside the bump diff. Diff-scoped bump proof cannot
observe that failure mode; this test scans every live workflow `uses:` site.
"""

from __future__ import annotations

import re
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / ".ci/policies/action-pin-provenance.toml"
WORKFLOWS_DIR = ROOT / ".github/workflows"
ACTION = "shogo82148/actions-setup-perl"
RETIRED_SHA = "53e33bb27be492a926eee378e8a5f7ff6618b061"
EXPECTED_SITE_COUNT = 5
WINDOWS_FIRST_HOUR = ".github/workflows/vscode-installed-first-hour-windows.yml"
MATRIX_WORKFLOW = ".github/workflows/perl-version-matrix.yml"

USES_RE = re.compile(rf"uses: {re.escape(ACTION)}@([0-9a-f]{{40}})")


def reviewed_shas() -> set[str]:
    pins = tomllib.loads(LEDGER.read_text(encoding="utf-8"))["pin"]
    return {row["sha"] for row in pins if row.get("action") == ACTION}


def workflow_uses() -> list[tuple[str, int, str]]:
    found: list[tuple[str, int, str]] = []
    for path in sorted(WORKFLOWS_DIR.glob("*.yml")):
        rel = f".github/workflows/{path.name}"
        for line_number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            match = USES_RE.search(line)
            if match:
                found.append((rel, line_number, match.group(1)))
    return found


class ActionPinSetupPerlSitesTests(unittest.TestCase):
    def test_all_live_sites_share_one_reviewed_coordinate(self) -> None:
        uses = workflow_uses()
        self.assertEqual(len(uses), EXPECTED_SITE_COUNT, uses)
        shas = {sha for _, _, sha in uses}
        self.assertEqual(len(shas), 1, shas)
        sha = shas.pop()
        self.assertIn(sha, reviewed_shas())

    def test_windows_first_hour_site_matches_matrix_coordinate(self) -> None:
        by_file = {rel: (line, sha) for rel, line, sha in workflow_uses()}
        self.assertIn(WINDOWS_FIRST_HOUR, by_file)
        self.assertIn(MATRIX_WORKFLOW, by_file)
        self.assertEqual(by_file[WINDOWS_FIRST_HOUR][1], by_file[MATRIX_WORKFLOW][1])

    def test_retired_setup_perl_sha_has_no_workflow_references(self) -> None:
        for path in WORKFLOWS_DIR.glob("*.yml"):
            self.assertNotIn(RETIRED_SHA, path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
