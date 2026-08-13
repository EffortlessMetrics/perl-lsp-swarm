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


class PerllspPlugin(LspPlugin):
    @classmethod
    def on_pre_start_async(cls, context: OnPreStartContext) -> None:
        settings = _trusted_settings()
        configured_path = settings.get("server_path", "auto")
        if not isinstance(configured_path, str) or not configured_path:
            raise PluginStartError("LSP-perllsp server_path must be a non-empty string")

        if configured_path == "auto":
            try:
                assert_managed_install_allowed()
            except CompatibilityError as error:
                raise PluginStartError(str(error)) from error
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
        return self.view.match_selector(0, "source.perl")

    def is_enabled(
        self,
        action: str = "",
        event: dict[str, Any] | None = None,
        point: int | None = None,
    ) -> bool:
        del event, point
        session = self.session_by_name(self.session_name)
        if session is None:
            return False
        try:
            self._prepare(action, session)
        except CommandSurfaceError:
            return False
        return True

    def run(self, edit: sublime.Edit, action: str = "") -> None:
        del edit
        session = self.session_by_name(self.session_name)
        if session is None:
            self._status("No active LSP-perllsp session owns the current Perl view.")
            return

        try:
            invocation = self._prepare(action, session)
        except CommandSurfaceError as error:
            self._status(str(error))
            return

        params: dict[str, Any] = {"command": invocation.spec.command_id}
        if invocation.arguments:
            params["arguments"] = invocation.arguments

        def handle_response(response: Any) -> None:
            if isinstance(response, Error):
                sublime.set_timeout(lambda: self._show_error(invocation, response))
                return
            sublime.set_timeout(lambda: self._show_success(invocation, response))

        session.execute_command(params, progress=True, view=self.view).then(handle_response)

    def _prepare(self, action: str, session: Any) -> CommandInvocation:
        selection = self.view.sel()
        point = selection[0].b if selection else 0
        line, character = self.view.rowcol(point)
        folders = [folder.path for folder in session.get_workspace_folders()]
        return prepare_invocation(
            action,
            _advertised_commands(session),
            file_path=self.view.file_name(),
            workspace_folders=folders,
            line=line,
            character=character,
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
