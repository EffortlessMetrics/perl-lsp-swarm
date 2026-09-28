#!/usr/bin/env python3
"""bash_path must normalize Windows-persisted paths without corrupting POSIX ones.

The blanket backslash rewrite the #15435 fix started from also rewrote
POSIX paths whose components legally contain backslash characters; the
drive-prefix discrimination must keep those intact while still repairing
Windows-persisted strings that reach a POSIX harness (#15435). The cases
below use ``PurePosixPath`` so the POSIX semantics are pinned on every
host, including Windows CI.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from bash_binary import bash_path

BACKSLASH = chr(92)


class BashPathTests(unittest.TestCase):
    def test_windows_persisted_path_is_rewritten_on_any_host(self) -> None:
        # A Windows-persisted string seen through POSIX path semantics.
        self.assertEqual(
            bash_path(
                PurePosixPath("F:" + BACKSLASH + "code" + BACKSLASH + "x.sh")
            ),
            "F:/code/x.sh",
        )

    def test_posix_component_with_literal_backslash_is_untouched(self) -> None:
        # Legal POSIX component names may contain backslashes; rewriting
        # them corrupts the path (the #15435 review's exit-127 probe).
        self.assertEqual(
            bash_path(
                PurePosixPath("parent" + BACKSLASH + "segment/x.sh")
            ),
            "parent" + BACKSLASH + "segment/x.sh",
        )

    def test_plain_posix_path_is_untouched(self) -> None:
        self.assertEqual(
            bash_path(PurePosixPath("scripts/ci/run.sh")),
            "scripts/ci/run.sh",
        )

    def test_colon_without_separator_is_not_a_windows_drive(self) -> None:
        self.assertEqual(
            bash_path(PurePosixPath("C:no-drive/foo.sh")),
            "C:no-drive/foo.sh",
        )

    def test_native_windows_path_reaches_bash_with_forward_separators(
        self,
    ) -> None:
        self.assertEqual(
            bash_path(Path("F:" + BACKSLASH + "code" + BACKSLASH + "x.sh")),
            "F:/code/x.sh",
        )


if __name__ == "__main__":
    unittest.main()
