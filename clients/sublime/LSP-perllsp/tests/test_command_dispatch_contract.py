from __future__ import annotations

import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_PATH = PACKAGE_ROOT / "plugin.py"


def plugin_source() -> str:
    return PLUGIN_PATH.read_text(encoding="utf-8")


def method_body(source: str, name: str) -> str:
    """The source between one `def name(` and the next method at its indent.

    Structural rather than an AST walk so the contract runner stays
    stdlib-only; the plugin's methods are flat enough that the next
    same-indent `def` reliably closes the body.
    """
    start = source.index(f"    def {name}(")
    body_indent = 4
    lines = source[start:].splitlines(keepends=True)
    collected: list[str] = []
    for index, line in enumerate(lines[1:], start=1):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip())
        if (
            stripped.startswith("def ")
            and indent == body_indent
            and index > 1
            or stripped.startswith("class ")
        ):
            break
        collected.append(line)
    return "".join(collected)


class PerllspDispatchContractTests(unittest.TestCase):
    """Structural contract for the palette-dispatch seam (#9610).

    The macOS journey is the executable witness for the session seam
    (it failed deterministically before the repair and passes after),
    but the local suite cannot import the plugin — it needs the Sublime
    host. These source-contract checks pin the repaired shape so the
    regression is caught without a host: session gating returning to
    `is_enabled`, the window-manager fallback disappearing, or the
    dispatch/rejection diagnostics being dropped all fail here.
    """

    def test_is_enabled_does_not_gate_on_session_presence(self) -> None:
        body = method_body(plugin_source(), "is_enabled")
        self.assertNotIn("session_by_name", body)
        self.assertNotIn("_prepare", body)
        self.assertIn("COMMAND_SPECS", body)

    def test_run_resolves_the_session_through_the_window_manager(self) -> None:
        source = plugin_source()
        self.assertIn("windows.lookup(window).get_session", source)
        resolve = method_body(source, "_resolve_session")
        self.assertIn("session_by_name", resolve)
        self.assertIn("windows.lookup(window)", resolve)

    def test_dispatch_wraps_execute_command_and_reports_synchronous_failure(self) -> None:
        run_body = method_body(plugin_source(), "run")
        self.assertIn("session.execute_command(params, progress=True, view=self.view)", run_body)
        try_start = run_body.index("try:")
        call_site = run_body.index("session.execute_command")
        self.assertLess(try_start, call_site, "execute_command must run inside the try")
        self.assertIn("dispatched=False", run_body)

    def test_promise_rejections_are_recorded(self) -> None:
        source = plugin_source()
        self.assertIn("kind=promise_rejected", source)
        self.assertIn('getattr(promise, "catch", None)', source)


if __name__ == "__main__":
    unittest.main()
