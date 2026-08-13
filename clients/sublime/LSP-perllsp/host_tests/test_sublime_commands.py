from __future__ import annotations

import json
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Generator

import sublime
from LSP.plugin.core.registry import windows
from LSP.plugin.core.types import ClientStates
from unittesting import DeferrableTestCase

from ..command_surface import COMMAND_SPECS, DESTRUCTIVE_COMMAND_IDS

TIMEOUT_MS = 120_000


class SublimePerllspCommandJourney(DeferrableTestCase):
    @classmethod
    def setUpClass(cls) -> Generator[Any, None, None]:
        super().setUpClass()
        cls.root = Path(os.environ["PERLLSP_SUBLIME_FIXTURE"]).resolve()
        cls.binary = Path(os.environ["PERLLSP_TEST_BINARY"]).resolve()
        cls.receipt_path = Path(os.environ["PERLLSP_SUBLIME_COMMAND_RECEIPT"]).resolve()

        settings = sublime.load_settings("LSP-perllsp.sublime-settings")
        settings.set("server_path", str(cls.binary))
        settings.set("env", {"PERLLSP_SUBLIME_COMMAND_HOST_TEST": "1"})

        cls.window = sublime.active_window()
        cls.window.set_project_data({"folders": [{"path": str(cls.root)}]})
        cls.path = cls.root / "app.pl"
        cls.view = cls.window.open_file(str(cls.path))
        yield {"condition": lambda: not cls.view.is_loading(), "timeout": TIMEOUT_MS}
        yield {
            "condition": lambda: cls.view.match_selector(0, "source.perl"),
            "timeout": TIMEOUT_MS,
        }

        cls.wm = windows.lookup(cls.window)
        yield {
            "condition": lambda: cls.wm.get_session("LSP-perllsp", str(cls.path)) is not None,
            "timeout": TIMEOUT_MS,
        }
        cls.session = cls.wm.get_session("LSP-perllsp", str(cls.path))
        if cls.session is None:
            raise AssertionError("LSP-perllsp did not attach to the command fixture")
        yield {
            "condition": lambda: cls.session.state == ClientStates.READY,
            "timeout": TIMEOUT_MS,
        }

    def test_workspace_trust_report_through_palette_adapter(self) -> Generator[Any, None, None]:
        spec = COMMAND_SPECS["workspace_trust_report"]
        advertised = self.session.get_capability("executeCommandProvider.commands", [])
        self.assertIn(spec.command_id, advertised)
        self.assertTrue(
            {item.command_id for item in COMMAND_SPECS.values()}.isdisjoint(DESTRUCTIVE_COMMAND_IDS)
        )

        self.window.destroy_output_panel("perllsp")
        self.view.run_command("perllsp_execute", {"action": spec.action})

        def panel_text() -> str:
            panel = self.window.find_output_panel("perllsp")
            if panel is None:
                return ""
            return panel.substr(sublime.Region(0, panel.size()))

        yield {
            "condition": lambda: spec.caption in panel_text(),
            "timeout": TIMEOUT_MS,
        }
        rendered = panel_text()
        self.assertIn(spec.caption, rendered)
        self.assertFalse(rendered.lstrip().startswith("{"), "raw protocol JSON became the normal UX")
        self.assertLessEqual(len(rendered), 64 * 1024 + 256)

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
            "command": {
                "action": spec.action,
                "id": spec.command_id,
                "session": self.session.config.name,
                "result_surface": "output.perllsp",
            },
            "assertions": {
                "active_view_session_selection": True,
                "advertised_command_gate": True,
                "workspace_execute_command": True,
                "bounded_structured_result": True,
                "no_destructive_binding": True,
            },
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
        if getattr(cls, "view", None) is not None and cls.view.is_valid():
            cls.view.set_scratch(True)
            cls.view.close()
        settings = sublime.load_settings("LSP-perllsp.sublime-settings")
        settings.erase("server_path")
        settings.erase("env")
        super().tearDownClass()
