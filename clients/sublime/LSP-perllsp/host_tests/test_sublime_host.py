from __future__ import annotations

import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Generator

import sublime
from LSP.plugin.core.protocol import Request
from LSP.plugin.core.registry import windows
from LSP.plugin.core.types import ClientStates
from LSP.plugin.core.url import filename_to_uri
from unittesting import DeferrableTestCase

TIMEOUT_MS = 120_000
VALID_APP = """use strict;
use warnings;
use Greeting;

my $message = Greeting::greet("Sublime");
print $message;
"""


class AwaitResult:
    def __init__(self) -> None:
        self.done = False
        self.value: Any = None
        self.error: Any = None

    def succeed(self, value: Any) -> None:
        self.value = value
        self.done = True

    def fail(self, error: Any) -> None:
        self.error = error
        self.done = True


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _position(view: sublime.View, needle: str, within: int = 0) -> dict[str, int]:
    region = view.find(needle, 0, sublime.LITERAL)
    if region.a < 0:
        raise AssertionError(f"needle not found: {needle}")
    row, column = view.rowcol(region.a + within)
    return {"line": row, "character": column}


def _request(session: Any, method: str, params: dict[str, Any]) -> AwaitResult:
    pending = AwaitResult()
    session.send_request(Request(method, params), pending.succeed, pending.fail)
    return pending


class SublimePerllspHostJourney(DeferrableTestCase):
    @classmethod
    def setUpClass(cls) -> Generator[Any, None, None]:
        super().setUpClass()
        cls.root = Path(os.environ["PERLLSP_SUBLIME_FIXTURE"]).resolve()
        cls.binary = Path(os.environ["PERLLSP_TEST_BINARY"]).resolve()
        cls.receipt_path = Path(os.environ["PERLLSP_SUBLIME_RECEIPT"]).resolve()
        if not cls.binary.is_file():
            raise AssertionError(f"current-source perllsp binary missing: {cls.binary}")

        settings = sublime.load_settings("LSP-perllsp.sublime-settings")
        settings.set("server_path", str(cls.binary))
        settings.set("env", {"PERLLSP_SUBLIME_HOST_TEST": "1"})
        lsp_settings = sublime.load_settings("LSP.sublime-settings")
        lsp_settings.set("semantic_highlighting", True)

        cls.window = sublime.active_window()
        cls.window.set_project_data({"folders": [{"path": str(cls.root)}]})
        cls.wm = windows.lookup(cls.window)
        cls.paths = {
            "pl": cls.root / "app.pl",
            "pm": cls.root / "customlib" / "Greeting.pm",
            "t": cls.root / "t" / "greeting.t",
        }
        cls.views = {
            family: cls.window.open_file(str(path))
            for family, path in cls.paths.items()
        }
        for view in cls.views.values():
            yield {"condition": lambda view=view: not view.is_loading(), "timeout": TIMEOUT_MS}
            yield {
                "condition": lambda view=view: view.match_selector(0, "source.perl"),
                "timeout": TIMEOUT_MS,
            }

        cls.view = cls.views["pl"]
        yield {
            "condition": lambda: cls.wm.get_session("LSP-perllsp", str(cls.paths["pl"])) is not None,
            "timeout": TIMEOUT_MS,
        }
        cls.session = cls.wm.get_session("LSP-perllsp", str(cls.paths["pl"]))
        if cls.session is None:
            raise AssertionError("LSP-perllsp did not attach to app.pl")
        yield {
            "condition": lambda: cls.session.state == ClientStates.READY,
            "timeout": TIMEOUT_MS,
        }
        cls.uri = filename_to_uri(str(cls.paths["pl"]))
        yield {
            "condition": lambda: cls.session.get_session_buffer_for_uri_async(cls.uri) is not None,
            "timeout": TIMEOUT_MS,
        }
        cls.buffer = cls.session.get_session_buffer_for_uri_async(cls.uri)
        if cls.buffer is None:
            raise AssertionError("LSP session buffer missing for app.pl")
        yield {"condition": lambda: cls.buffer.opened, "timeout": TIMEOUT_MS}

    def test_real_host_interoperability_journey(self) -> Generator[Any, None, None]:
        observed: dict[str, bool] = {}

        for family, path in self.paths.items():
            session = self.wm.get_session("LSP-perllsp", str(path))
            self.assertIsNotNone(session, f"LSP-perllsp did not attach to .{family}")
        observed["file_family_activation"] = True

        configured_command = list(self.session.config.command)
        self.assertEqual(configured_command[-1], "--stdio")
        self.assertIn(configured_command[0], ("${server_path}", str(self.binary)))
        self.assertEqual(
            Path(sublime.load_settings("LSP-perllsp.sublime-settings").get("server_path")).resolve(),
            self.binary,
        )
        command = [str(self.binary), "--stdio"]
        observed["exact_binary_launch"] = True

        yield {
            "condition": lambda: len(self.buffer.diagnostics) > 0,
            "timeout": TIMEOUT_MS,
        }
        observed["pull_diagnostics_open"] = True
        self.view.run_command("select_all")
        self.view.run_command("insert", {"characters": VALID_APP})
        yield {
            "condition": lambda: self.buffer.last_synced_version == self.view.change_count(),
            "timeout": TIMEOUT_MS,
        }
        yield {
            "condition": lambda: len(self.buffer.diagnostics) == 0,
            "timeout": TIMEOUT_MS,
        }
        observed["pull_diagnostics_after_edit"] = True

        self.view.run_command("select_all")
        self.view.run_command(
            "insert",
            {"characters": VALID_APP.replace("print $message;", "print $mes;")},
        )
        yield {
            "condition": lambda: self.buffer.last_synced_version == self.view.change_count(),
            "timeout": TIMEOUT_MS,
        }
        completion = _request(
            self.session,
            "textDocument/completion",
            {
                "textDocument": {"uri": self.uri},
                "position": _position(self.view, "$mes", len("$mes")),
            },
        )
        yield {"condition": lambda: completion.done, "timeout": TIMEOUT_MS}
        self.assertIsNone(completion.error)
        items = completion.value.get("items", []) if isinstance(completion.value, dict) else completion.value
        self.assertIsInstance(items, list)
        self.assertTrue(
            any("message" in str(item.get("label", "")) for item in items if isinstance(item, dict)),
            f"completion did not contain message: {items!r}",
        )
        observed["completion"] = True

        self.view.run_command("select_all")
        self.view.run_command("insert", {"characters": VALID_APP})
        yield {
            "condition": lambda: self.buffer.last_synced_version == self.view.change_count(),
            "timeout": TIMEOUT_MS,
        }

        symbol_position = _position(self.view, "greet", 2)
        definition = _request(
            self.session,
            "textDocument/definition",
            {"textDocument": {"uri": self.uri}, "position": symbol_position},
        )
        hover = _request(
            self.session,
            "textDocument/hover",
            {
                "textDocument": {"uri": self.uri},
                "position": _position(self.view, "$message", 2),
            },
        )
        yield {"condition": lambda: definition.done and hover.done, "timeout": TIMEOUT_MS}
        self.assertIsNone(definition.error)
        self.assertIsNone(hover.error)
        definition_payload = definition.value
        locations = definition_payload if isinstance(definition_payload, list) else [definition_payload]
        self.assertTrue(
            any(
                isinstance(location, dict)
                and "Greeting.pm" in str(location.get("uri") or location.get("targetUri"))
                for location in locations
            ),
            f"definition did not resolve to customlib/Greeting.pm: {definition_payload!r}",
        )
        self.assertIsNotNone(hover.value)
        observed["definition"] = True
        observed["hover"] = True
        observed["project_configuration"] = True

        rename = _request(
            self.session,
            "textDocument/rename",
            {
                "textDocument": {"uri": self.uri},
                "position": _position(self.view, "$message", 2),
                "newName": "$output",
            },
        )
        yield {"condition": lambda: rename.done, "timeout": TIMEOUT_MS}
        self.assertIsNone(rename.error)
        self.assertIsInstance(rename.value, dict)
        apply_promise = self.session.apply_workspace_edit_async(rename.value, is_refactoring=True)
        applied = AwaitResult()
        apply_promise.then(applied.succeed)
        yield {"condition": lambda: applied.done, "timeout": TIMEOUT_MS}
        yield {
            "condition": lambda: "$output" in self.view.substr(sublime.Region(0, self.view.size())),
            "timeout": TIMEOUT_MS,
        }
        observed["workspace_edit_applied"] = True

        sublime.set_timeout_async(lambda: self.buffer.do_semantic_tokens_async(self.view, False))
        yield {
            "condition": lambda: bool(self.buffer.semantic_tokens.data),
            "timeout": TIMEOUT_MS,
        }
        # Token data arriving is not yet the scope being applied: the client
        # decodes and adds the semantic regions on the UI thread afterwards,
        # so the custom-scope assertion waits for the regions like every
        # other observation in this journey.
        try:
            yield {
                "condition": lambda: bool(self.view.find_by_selector("variable.other.scalar.perl")),
                "timeout": TIMEOUT_MS,
            }
        except Exception:
            # Diagnostic evidence for the scope-mapping chain: what the client
            # decoded and which scopes the view actually carries.
            try:
                import json as _json

                tokens = []
                for token in self.buffer.semantic_tokens:
                    tokens.append(
                        {
                            "type": token.type,
                            "modifiers": token.modifiers,
                            "range": "{a}:{b}".format(a=token.range.begin.pt, b=token.range.end.pt),
                        }
                    )
                print(
                    "scope-diagnostic tokens=" + _json.dumps(tokens[:24]),
                    "scope-diagnostic view-sample=" + _json.dumps(
                        self.view.substr(sublime.Region(0, min(80, self.view.size())))
                    ),
                    "scope-diagnostic regions=" + _json.dumps(
                        [k for k in self.view.regions() if "lsp" in k.lower() or "semantic" in k.lower()]
                    ),
                )
            except Exception as error:  # noqa: BLE001 - diagnostics must never mask
                print("scope-diagnostic failed: " + repr(error))
            raise
        self.assertTrue(
            self.view.find_by_selector("variable.other.scalar.perl"),
            "custom scalar-variable semantic scope was not applied",
        )
        observed["semantic_tokens"] = True
        observed["custom_semantic_mapping"] = True

        receipt = {
            "schema_version": 1,
            "stage": "exact_source_local",
            "source_sha": os.environ.get("PERLLSP_SOURCE_SHA", ""),
            "recorded_at": datetime.now(timezone.utc).isoformat(),
            "host": {
                "name": "Sublime Text",
                "version": sublime.version(),
                "platform": sublime.platform(),
                "arch": sublime.arch(),
            },
            "lsp_package": {
                "repository": "sublimelsp/LSP",
                "ref": os.environ.get("PERLLSP_LSP_REF", ""),
            },
            "helper_package": {
                "name": "LSP-perllsp",
                "source": "clients/sublime/LSP-perllsp",
            },
            "binary": {
                "path": str(self.binary),
                "sha256": _sha256(self.binary),
                "command": command,
            },
            "fixtures": {key: str(path.relative_to(self.root)) for key, path in self.paths.items()},
            "assertions": observed,
        }
        self.receipt_path.parent.mkdir(parents=True, exist_ok=True)
        self.receipt_path.write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    @classmethod
    def tearDownClass(cls) -> Generator[Any, None, None]:
        if getattr(cls, "session", None) is not None:
            sublime.set_timeout_async(cls.session.end_async)
            yield {
                "condition": lambda: cls.session.state == ClientStates.STOPPING,
                "timeout": TIMEOUT_MS,
            }
        for view in getattr(cls, "views", {}).values():
            if view.is_valid():
                view.set_scratch(True)
                view.close()
        settings = sublime.load_settings("LSP-perllsp.sublime-settings")
        settings.erase("server_path")
        settings.erase("env")
        sublime.load_settings("LSP.sublime-settings").erase("semantic_highlighting")
        super().tearDownClass()
