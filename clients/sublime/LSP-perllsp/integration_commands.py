from __future__ import annotations

import json
import urllib.request
from pathlib import Path
from typing import Any

import sublime
import sublime_plugin

from .debugger_adapter import registered_adapter_class
from .integration_status import (
    IntegrationStatusError,
    clear_invalid_managed_cache,
    collect_status,
    format_status,
    repair_managed_server,
)

_SETTINGS_FILE = "LSP-perllsp.sublime-settings"
_OUTPUT_PANEL = "perllsp-status"


def _trusted_settings() -> sublime.Settings:
    return sublime.load_settings(_SETTINGS_FILE)


def _storage_path() -> Path:
    # Imported lazily to avoid a module cycle while Sublime loads plugin.py.
    from .plugin import PerllspPlugin

    return Path(PerllspPlugin.plugin_storage_path)


def _write_output(window: sublime.Window, text: str) -> None:
    panel = window.create_output_panel(_OUTPUT_PANEL)
    panel.set_read_only(False)
    panel.run_command("select_all")
    panel.run_command("right_delete")
    panel.run_command(
        "append",
        {"characters": text, "force": True, "scroll_to_end": True},
    )
    panel.set_read_only(True)
    window.run_command("show_panel", {"panel": f"output.{_OUTPUT_PANEL}"})


def _collect() -> dict[str, Any]:
    settings = _trusted_settings()
    server_path = settings.get("server_path", "auto")
    dap_path = settings.get("dap_path", "auto")
    if not isinstance(server_path, str) or not server_path:
        server_path = "auto"
    if not isinstance(dap_path, str) or not dap_path:
        dap_path = "auto"
    return collect_status(
        _storage_path(),
        sublime.platform(),
        sublime.arch(),
        server_path=server_path,
        dap_path=dap_path,
        debugger_registered=registered_adapter_class() is not None,
    )


def _show_error(window: sublime.Window, action: str, error: Exception) -> None:
    _write_output(
        window,
        f"Perl LSP {action} failed\n"
        f"{'=' * (16 + len(action))}\n\n"
        f"{error}\n",
    )
    window.status_message(f"Perl LSP {action} failed; see the integration status panel.")


class PerllspIntegrationStatusCommand(sublime_plugin.WindowCommand):
    """Read-only structural status. It does not install, delete, or restart."""

    def run(self) -> None:
        sublime.set_timeout_async(self._run_async)

    def _run_async(self) -> None:
        try:
            payload = _collect()
            rendered = format_status(payload)
        except Exception as error:
            sublime.set_timeout(lambda: _show_error(self.window, "status", error))
            return
        sublime.set_timeout(lambda: _write_output(self.window, rendered))


class PerllspRepairManagedServerCommand(sublime_plugin.WindowCommand):
    """Explicitly repair a missing or invalid package-managed perllsp cache."""

    def is_enabled(self) -> bool:
        return _trusted_settings().get("server_path", "auto") == "auto"

    def run(self) -> None:
        if not self.is_enabled():
            self.window.status_message(
                "LSP-perllsp uses a user-owned external server; package repair is unavailable."
            )
            return
        if not sublime.ok_cancel_dialog(
            "Repair the package-managed perllsp binary?\n\n"
            "This may download the exact pinned artifact. It will not change project settings "
            "or restart an active LSP session."
        ):
            return
        sublime.set_timeout_async(self._run_async)

    def _run_async(self) -> None:
        try:
            receipt = repair_managed_server(
                _storage_path(),
                sublime.platform(),
                sublime.arch(),
                opener=urllib.request.urlopen,
            )
            payload = _collect()
            rendered = (
                "Managed perllsp repair\n"
                "======================\n\n"
                f"{json.dumps(receipt, indent=2, sort_keys=True)}\n\n"
                f"{format_status(payload)}"
            )
        except Exception as error:
            sublime.set_timeout(lambda: _show_error(self.window, "managed-server repair", error))
            return
        sublime.set_timeout(lambda: _write_output(self.window, rendered))


class PerllspClearInvalidCacheCommand(sublime_plugin.WindowCommand):
    """Explicitly remove only a known invalid managed cache directory."""

    def is_enabled(self) -> bool:
        return _trusted_settings().get("server_path", "auto") == "auto"

    def run(self) -> None:
        if not self.is_enabled():
            self.window.status_message(
                "LSP-perllsp uses a user-owned external server; package cache cleanup is unavailable."
            )
            return
        if not sublime.ok_cancel_dialog(
            "Remove the invalid package-managed perllsp cache?\n\n"
            "A verified cache is never removed by this command. The server is not reinstalled "
            "or restarted automatically."
        ):
            return
        sublime.set_timeout_async(self._run_async)

    def _run_async(self) -> None:
        try:
            receipt = clear_invalid_managed_cache(
                _storage_path(),
                sublime.platform(),
                sublime.arch(),
            )
            payload = _collect()
            rendered = (
                "Managed perllsp cache cleanup\n"
                "=============================\n\n"
                f"{json.dumps(receipt, indent=2, sort_keys=True)}\n\n"
                f"{format_status(payload)}"
            )
        except IntegrationStatusError as error:
            sublime.set_timeout(lambda: _show_error(self.window, "cache cleanup", error))
            return
        except Exception as error:
            sublime.set_timeout(lambda: _show_error(self.window, "cache cleanup", error))
            return
        sublime.set_timeout(lambda: _write_output(self.window, rendered))
