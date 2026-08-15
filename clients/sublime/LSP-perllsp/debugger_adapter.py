from __future__ import annotations

from typing import Any

import sublime

from .dap_support import DapPathError, dap_command, resolve_dap_path

_REGISTERED_CLASS: type | None = None
_IMPORT_FAILURE_REPORTED = False
_FOREIGN_OWNER_REPORTED = False


def _settings() -> sublime.Settings:
    # Package/user settings are intentionally separate from project-merged
    # Debugger configuration. A repository may provide launch arguments, but it
    # cannot select the adapter executable.
    return sublime.load_settings("LSP-perllsp.sublime-settings")


def resolve_configured_dap_path():
    settings = _settings()
    configured = settings.get("dap_path", "auto")
    server_path = settings.get("server_path", "auto")
    if not isinstance(configured, str) or not configured:
        raise DapPathError("LSP-perllsp dap_path must be a non-empty string.")
    if not isinstance(server_path, str) or not server_path:
        server_path = "auto"
    return resolve_dap_path(configured, server_path=server_path)


def _registry(AdapterBase: type) -> dict[str, Any] | None:
    registered = getattr(AdapterBase, "registered_types", None)
    if isinstance(registered, dict):
        return registered
    try:
        from Debugger.modules.dap.adapter import AdapterConfigurationRegistery
    except ImportError:
        return None
    registered = getattr(AdapterConfigurationRegistery, "registered_types", None)
    return registered if isinstance(registered, dict) else None


def _is_our_adapter(adapter: Any) -> bool:
    adapter_type = adapter if isinstance(adapter, type) else type(adapter)
    return adapter_type.__module__.endswith("debugger_adapter")


def register_debugger_adapter() -> bool:
    global _FOREIGN_OWNER_REPORTED, _IMPORT_FAILURE_REPORTED, _REGISTERED_CLASS

    try:
        from Debugger.modules import dap
    except ImportError as error:
        if not _IMPORT_FAILURE_REPORTED:
            print(
                "[LSP-perllsp] Sublime Debugger is not loaded; "
                "the Perl DAP adapter will register when Debugger becomes available: "
                f"{error}"
            )
            _IMPORT_FAILURE_REPORTED = True
        return False

    AdapterBase = getattr(dap, "AdapterConfiguration", None) or getattr(dap, "Adapter", None)
    StdioTransport = getattr(dap, "StdioTransport", None)
    if not isinstance(AdapterBase, type) or StdioTransport is None:
        print("[LSP-perllsp] Unsupported Sublime Debugger adapter API; Perl DAP was not registered.")
        return False

    registered = _registry(AdapterBase)
    if registered is not None and "perl" in registered:
        existing = registered["perl"]
        if _is_our_adapter(existing):
            _REGISTERED_CLASS = type(existing) if not isinstance(existing, type) else existing
            return True
        if not _FOREIGN_OWNER_REPORTED:
            print(
                "[LSP-perllsp] A different Sublime Debugger adapter already owns type 'perl'; "
                "LSP-perllsp will not replace it."
            )
            _FOREIGN_OWNER_REPORTED = True
        return False

    class PerlDapAdapter(AdapterBase):
        type = "perl"
        docs = (
            "https://github.com/EffortlessMetrics/perl-lsp/blob/main/"
            "docs/tutorials/DAP_USER_GUIDE.md"
        )

        @property
        def configuration_snippets(self) -> list[dict[str, Any]]:
            return [
                {
                    "label": "Perl: Debug current file",
                    "body": {
                        "name": "Perl: Debug current file",
                        "type": "perl",
                        "request": "launch",
                        "program": r"\${file}",
                        "cwd": r"\${workspaceFolder}",
                        "perlPath": "perl",
                        "args": [],
                        "includePaths": [r"\${workspaceFolder}/lib"],
                        "env": {},
                    },
                }
            ]

        @property
        def configuration_schema(self) -> dict[str, Any]:
            return {
                "type": "object",
                "required": ["program"],
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "Absolute path to the Perl program to debug.",
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory for the debuggee.",
                    },
                    "perlPath": {
                        "type": "string",
                        "default": "perl",
                        "description": "Perl interpreter used by perl-dap.",
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "default": [],
                    },
                    "includePaths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "default": [],
                    },
                    "env": {
                        "type": "object",
                        "additionalProperties": {"type": "string"},
                        "default": {},
                    },
                    "stopOnEntry": {
                        "type": "boolean",
                        "default": False,
                    },
                },
            }

        async def start(self, log: Any, configuration: Any):
            try:
                binary = resolve_configured_dap_path()
            except DapPathError as error:
                core_error = getattr(dap, "Error", RuntimeError)
                raise core_error(str(error)) from error

            command = dap_command(binary)
            if hasattr(log, "info"):
                log.info(f"Using perl-dap `{binary}` over stdio")
            cwd = configuration.get("cwd") if hasattr(configuration, "get") else None
            return StdioTransport(command=command, cwd=cwd)

    _REGISTERED_CLASS = PerlDapAdapter
    _IMPORT_FAILURE_REPORTED = False
    _FOREIGN_OWNER_REPORTED = False

    registered = _registry(AdapterBase)
    if registered is not None and "perl" not in registered:
        print("[LSP-perllsp] Perl DAP adapter class loaded but Debugger did not register type 'perl'.")
        return False
    print("[LSP-perllsp] Registered Sublime Debugger adapter type 'perl'.")
    return True


def unregister_debugger_adapter() -> bool:
    """Drop our adapter from Debugger's registry when the package unloads.

    Registration is process-global inside Debugger, so without this the class
    object created by a disabled, uninstalled, or previous revision of this
    package stays reachable. `register_debugger_adapter` would then find a
    `perl` entry, see `_is_our_adapter` return True for that stale class, and
    adopt it instead of installing the current implementation — leaving old code
    callable until Debugger or Sublime restarts. Ownership is re-checked here so
    a foreign `perl` adapter is never removed.
    """
    global _FOREIGN_OWNER_REPORTED, _IMPORT_FAILURE_REPORTED, _REGISTERED_CLASS

    _REGISTERED_CLASS = None
    _IMPORT_FAILURE_REPORTED = False
    _FOREIGN_OWNER_REPORTED = False

    try:
        from Debugger.modules import dap
    except ImportError:
        return False

    AdapterBase = getattr(dap, "AdapterConfiguration", None) or getattr(dap, "Adapter", None)
    if not isinstance(AdapterBase, type):
        return False

    registered = _registry(AdapterBase)
    if registered is None or "perl" not in registered:
        return False
    if not _is_our_adapter(registered["perl"]):
        return False

    del registered["perl"]
    return True


def registered_adapter_class() -> type | None:
    return _REGISTERED_CLASS
