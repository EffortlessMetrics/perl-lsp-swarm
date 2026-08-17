from __future__ import annotations

import os
import shutil
import threading
from pathlib import Path

import sublime
from LSP.plugin import LspPlugin, OnPreStartContext, PluginStartError

from .release import install_server

_SETTINGS_FILE = "LSP-perllsp.sublime-settings"
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


def plugin_loaded() -> None:
    PerllspPlugin.register()


def plugin_unloaded() -> None:
    PerllspPlugin.unregister()
