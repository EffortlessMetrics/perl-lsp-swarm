from __future__ import annotations

import ast
import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_PATH = PACKAGE_ROOT / "plugin.py"


def plugin_tree() -> ast.Module:
    return ast.parse(PLUGIN_PATH.read_text(encoding="utf-8"))


def command_class(tree: ast.Module) -> ast.ClassDef:
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == "PerllspExecuteCommand":
            return node
    raise AssertionError("PerllspExecuteCommand class not found")


def method(cls: ast.ClassDef, name: str) -> ast.FunctionDef:
    for node in cls.body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return node
    raise AssertionError(f"method {name} not found on PerllspExecuteCommand")


def calls(node: ast.AST) -> list[ast.Call]:
    return [child for child in ast.walk(node) if isinstance(child, ast.Call)]


def call_name(call: ast.Call) -> str:
    """A readable name for simple call targets (self.x, mod.x, bare x)."""
    target = call.func
    parts: list[str] = []
    while isinstance(target, ast.Attribute):
        parts.append(target.attr)
        target = target.value
    if isinstance(target, ast.Name):
        parts.append(target.id)
    return ".".join(reversed(parts))


class PerllspDispatchContractTests(unittest.TestCase):
    """AST contract for the palette-dispatch seam (#9610).

    The macOS journey is the executable witness for the session seam
    (it failed deterministically before the repair and passes after),
    but the local suite cannot import the plugin — it needs the Sublime
    host. These checks bind to the parsed control flow rather than
    source strings, so a regression cannot pass by renaming: session
    gating returning to `is_enabled`, the window-manager fallback
    disappearing, the dispatch leaving its try, or the rejection
    handler being dropped all fail here.
    """

    def test_is_enabled_has_no_session_dependence(self) -> None:
        enabled = method(command_class(plugin_tree()), "is_enabled")
        # Any session-derived expression (name or attribute rooted in
        # `session`, incl. session_by_name / _resolve_session calls)
        # would reintroduce the suppressed-dispatch seam.
        for node in ast.walk(enabled):
            if isinstance(node, ast.Name) and node.id == "session":
                self.fail("is_enabled references a session value")
        for call in calls(enabled):
            name = call_name(call)
            self.assertNotIn("session_by_name", name)
            self.assertNotIn("_resolve_session", name)
            self.assertNotIn("_prepare", name)

    def test_resolve_session_falls_back_to_the_window_manager(self) -> None:
        resolve = method(command_class(plugin_tree()), "_resolve_session")
        names = [call_name(call) for call in calls(resolve)]
        self.assertTrue(
            any("session_by_name" in name for name in names),
            f"no session_by_name lookup in {names}",
        )
        self.assertIn("windows.lookup", names)
        self.assertIn("get_session", names)

    def test_execute_command_runs_inside_a_try_with_a_handler(self) -> None:
        run = method(command_class(plugin_tree()), "run")
        dispatches = [
            call for call in calls(run) if call_name(call) == "session.execute_command"
        ]
        self.assertEqual(1, len(dispatches), "run must dispatch exactly once")
        dispatch = dispatches[0]

        def is_inside_try(node: ast.AST) -> bool:
            return any(
                isinstance(parent, ast.Try) and dispatch in ast.walk(parent)
                for parent in ast.walk(run)
                if isinstance(parent, ast.Try)
            )

        self.assertTrue(is_inside_try(dispatch), "dispatch must run inside a try")
        tries = [parent for parent in ast.walk(run) if isinstance(parent, ast.Try)]
        wrapping = next(t for t in tries if dispatch in ast.walk(t))
        self.assertTrue(wrapping.handlers, "the wrapping try must catch failures")
        handler_names = {call_name(call) for call in calls(wrapping)}
        self.assertTrue(
            any("_diag" in name for name in handler_names),
            "a synchronous failure must be recorded with dispatched=False",
        )

    def test_the_promise_gets_a_rejection_handler(self) -> None:
        run = method(command_class(plugin_tree()), "run")
        defensive_catch = [
            call
            for call in calls(run)
            if call_name(call) == "getattr"
            and len(call.args) >= 2
            and isinstance(call.args[1], ast.Constant)
            and call.args[1].value == "catch"
        ]
        self.assertEqual(
            1, len(defensive_catch), "the promise must attach .catch defensively via getattr"
        )


if __name__ == "__main__":
    unittest.main()
