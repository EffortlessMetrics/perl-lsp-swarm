from __future__ import annotations

import os
import shutil
import threading
from pathlib import Path
from typing import Any

import sublime
import sublime_plugin
from LSP.plugin import LspPlugin, OnPreStartContext, PluginStartError
from LSP.plugin.core.open import open_file
from LSP.plugin.core.protocol import Error
from LSP.plugin.core.registry import LspTextCommand
from LSP.plugin.core.url import filename_to_uri

from .command_surface import (
    COMMAND_SPECS,
    CommandInvocation,
    CommandSurfaceError,
    format_error,
    format_result,
    navigation_target,
    prepare_invocation,
)
from .compatibility import CompatibilityError, assert_managed_install_allowed
from .debugger_adapter import register_debugger_adapter
from .release import install_server

_SETTINGS_FILE = "LSP-perllsp.sublime-settings"
_OUTPUT_PANEL = "perllsp"
_INSTALL_LOCK = threading.Lock()


def _trusted_settings() -> sublime.Settings:
    # Package/user settings are loaded independently of the project-merged
    # ClientConfig. A repository may tune ordinary LSP settings, but it cannot
    # replace the executable, transport, or process environment.
    return sublime.load_settings(_SETTINGS_FILE)


def _external_server_path(configured: str) -> Path:
    expanded = os.path.expandvars(os.path.expanduser(configured))
    candidate = Path(expanded)
    resolved = shutil.which(expanded) if not candidate.is_absolute() else None
    path = Path(resolved) if resolved else candidate
    if not path.is_file():
        raise PluginStartError(f"Configured perllsp binary was not found: {configured}")
    return path


def _advertised_commands(session: Any) -> set[str]:
    value = session.get_capability("executeCommandProvider.commands", [])
    if not isinstance(value, list):
        return set()
    return {command for command in value if isinstance(command, str)}


def _utf16_rowcol(view: sublime.View, point: int) -> tuple[int, int]:
    """Return the LSP position for a Sublime point.

    The server pins LSP positions to UTF-16 code units, while `View.rowcol`
    counts characters. Any astral character earlier on the line (an emoji, for
    example) makes the two disagree, which shifts the request onto a different
    symbol. Sublime 4132 added `rowcol_utf16`; older builds fall back to the
    character column, which is exact for the BMP-only lines they can report.
    """
    rowcol_utf16 = getattr(view, "rowcol_utf16", None)
    if callable(rowcol_utf16):
        row, column = rowcol_utf16(point)
        return row, column
    return view.rowcol(point)


def _write_output_panel(window: sublime.Window, text: str) -> None:
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


def _diag(message: str) -> None:
    """Record a palette-dispatch decision on the Sublime console.

    The command journey's silent failure modes (no session, a rejected
    invocation, a dispatched request whose response never arrives) all
    leave the output panel empty, so the real-host runner's failure
    artifact — the recorded console log — is the only surface that can
    distinguish them. One bounded line per decision keeps the console
    useful without spamming normal usage: these commands are
    user-initiated and low-frequency.
    """
    print(f"[perllsp-command] {message}")


# Sublime re-queries is_enabled frequently (palette opens, menu refreshes,
# capability changes), so its diagnostics deduplicate per action+reason: the
# first occurrence of each reason reaches the console, repeats stay silent.
_enabled_diag_seen: set[tuple[str, str]] = set()


def _diag_enabled_once(action: str, reason: str) -> None:
    key = (action, reason)
    if key in _enabled_diag_seen:
        return
    _enabled_diag_seen.add(key)
    _diag(f"is_enabled action={action!r} enabled=False reason={reason}")


class PerllspPlugin(LspPlugin):
    @classmethod
    def on_pre_start_async(cls, context: OnPreStartContext) -> None:
        settings = _trusted_settings()
        configured_path = settings.get("server_path", "auto")
        if not isinstance(configured_path, str) or not configured_path:
            raise PluginStartError("LSP-perllsp server_path must be a non-empty string")

        if configured_path == "auto":
            with _INSTALL_LOCK:
                server_path = install_server(
                    cls.plugin_storage_path,
                    sublime.platform(),
                    sublime.arch(),
                )
        else:
            server_path = _external_server_path(configured_path)

        trusted_env = settings.get("env", {})
        if not isinstance(trusted_env, dict):
            raise PluginStartError("LSP-perllsp env must be an object")

        context.variables["server_path"] = str(server_path)
        context.configuration.command = ["${server_path}", "--stdio"]
        context.configuration.tcp_port = None
        context.configuration.env = dict(trusted_env)


class PerllspExecuteCommand(LspTextCommand):
    """Route the curated Sublime surface through the active perllsp session."""

    session_name = "LSP-perllsp"

    def is_visible(
        self,
        action: str = "",
        event: dict[str, Any] | None = None,
        point: int | None = None,
    ) -> bool:
        del event, point
        if action not in COMMAND_SPECS:
            return False
        selector_point = min(max(self.view.size() - 1, 0), 0)
        return self.view.match_selector(selector_point, "source.perl")

    def is_enabled(
        self,
        action: str = "",
        event: dict[str, Any] | None = None,
        point: int | None = None,
    ) -> bool:
        del event, point
        session = self.session_by_name(self.session_name)
        if session is None:
            _diag_enabled_once(action, "session_none")
            return False
        try:
            self._prepare(action, session)
        except CommandSurfaceError as error:
            _diag_enabled_once(action, f"{type(error).__name__}: {error}")
            return False
        return True

    def run(self, edit: sublime.Edit, action: str = "") -> None:
        del edit
        session = self.session_by_name(self.session_name)
        if session is None:
            _diag(f"run action={action!r} dispatched=False reason=session_none")
            self._status("No active LSP-perllsp session owns the current Perl view.")
            return

        try:
            invocation = self._prepare(action, session)
        except CommandSurfaceError as error:
            _diag(
                f"run action={action!r} dispatched=False "
                f"reason={type(error).__name__}: {error}"
            )
            self._status(str(error))
            return

        params: dict[str, Any] = {"command": invocation.spec.command_id}
        if invocation.arguments:
            params["arguments"] = invocation.arguments

        def handle_response(response: Any) -> None:
            _diag(
                f"response action={action!r} command={invocation.spec.command_id!r} "
                f"kind={'error' if isinstance(response, Error) else 'result'}"
            )
            if isinstance(response, Error):
                sublime.set_timeout(lambda: self._show_error(invocation, response))
                return
            sublime.set_timeout(lambda: self._show_success(invocation, response))

        _diag(
            f"run action={action!r} dispatched command={invocation.spec.command_id!r} "
            f"arguments={invocation.arguments!r}"
        )
        session.execute_command(params, progress=True, view=self.view).then(handle_response)

    def _prepare(self, action: str, session: Any) -> CommandInvocation:
        selection = self.view.sel()
        point = selection[0].b if selection else 0
        line, character = _utf16_rowcol(self.view, point)
        folders = [folder.path for folder in session.get_workspace_folders()]
        return prepare_invocation(
            action,
            _advertised_commands(session),
            file_path=self.view.file_name(),
            workspace_folders=folders,
            line=line,
            character=character,
            is_dirty=self.view.is_dirty(),
        )

    def _show_success(self, invocation: CommandInvocation, result: Any) -> None:
        window = self.view.window()
        if window is None:
            return

        if invocation.spec.result_kind == "navigation":
            target = navigation_target(result)
            if target:
                if "://" in target:
                    uri = target
                else:
                    path = Path(target)
                    if not path.is_absolute() and invocation.workspace_path:
                        path = Path(invocation.workspace_path) / path
                    uri = filename_to_uri(str(path.resolve()))

                def opened(view: sublime.View | None) -> None:
                    if view is None:
                        _write_output_panel(
                            window,
                            format_result(invocation.spec.caption, result),
                        )
                        window.status_message(
                            f"{invocation.spec.caption}: the returned target could not be opened."
                        )
                    else:
                        window.status_message(f"{invocation.spec.caption}: opened target.")

                open_file(window, uri).then(opened)
                return

        _write_output_panel(window, format_result(invocation.spec.caption, result))
        window.status_message(f"{invocation.spec.caption}: completed.")

    def _show_error(self, invocation: CommandInvocation, error: Error) -> None:
        window = self.view.window()
        if window is None:
            return
        _write_output_panel(
            window,
            format_error(invocation.spec.caption, invocation.spec.command_id, error),
        )
        window.status_message(
            f"{invocation.spec.caption}: server command failed; see the Perl LSP output panel."
        )

    def _status(self, message: str) -> None:
        if window := self.view.window():
            window.status_message(message)
        else:
            sublime.status_message(message)


class PerllspDebuggerRegistrationListener(sublime_plugin.EventListener):
    """Register after Debugger loads, including when it is installed later."""

    def on_activated_async(self, view: sublime.View) -> None:
        del view
        register_debugger_adapter()


def plugin_loaded() -> None:
    PerllspPlugin.register()
    register_debugger_adapter()
    sublime.set_timeout_async(register_debugger_adapter, 1000)


def plugin_unloaded() -> None:
    PerllspPlugin.unregister()
