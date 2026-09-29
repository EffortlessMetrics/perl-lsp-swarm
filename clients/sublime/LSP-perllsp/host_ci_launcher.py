"""CI-only launcher for the UnitTesting host journey on headless runners.

Upstream's run-tests action drives the suite by dropping a `zzz_run_scheduler`
shim into the installed UnitTesting package and relying on Sublime's plugin
loader to import it. On the headless CI runners the shim file is present on
disk and every other plugin loads (proven by the former canary plugin), yet
the shim is never imported and no trace of the suite appears. Rather than
depend on that implicit load, this plugin explicitly imports UnitTesting's
scheduler at `plugin_loaded` and runs the recorded schedule, capturing any
traceback to a file the workflow runner dumps.

The launcher only acts when a schedule exists, so a normal editor install
never starts test runs. It is not collected by the suite itself (only
`host_tests/test_*.py` are).
"""

from __future__ import annotations

import os
import sys
import traceback

import sublime

_LAUNCHER_LOG = os.path.join(
    os.path.expanduser("~"), "perllsp_sublime_host_ci.log"
)


def _log(message: str) -> None:
    with open(_LAUNCHER_LOG, "a", encoding="utf-8") as handle:
        handle.write(message + "\n")


def _probe_host(delay_ms: int) -> None:
    """Observe the loaded host without importing LSP or changing its timing."""
    try:
        window = sublime.active_window()
        registry = sys.modules.get("LSP.plugin.core.registry")
        windows = getattr(registry, "windows", None)
        fields = {}
        if windows is not None:
            for key, value in vars(windows).items():
                if isinstance(value, (dict, list, tuple, set)):
                    fields[key] = f"{type(value).__name__}[{len(value)}]"
                else:
                    fields[key] = type(value).__name__
        _log(
            f"host probe {delay_ms}ms: python={sys.version!r} "
            f"sublime={sublime.version()} expected={os.environ.get('PERLLSP_EXPECTED_SUBLIME_BUILD')} "
            f"window_id={window.id() if window else None} "
            f"window_valid={window.is_valid() if window else None} "
            f"lsp_loaded={'LSP.plugin' in sys.modules} registry_loaded={registry is not None} "
            f"registry_fields={fields!r} "
            f"enabled={getattr(windows, '_enabled', None)!r} "
            f"registered_window_ids={list(getattr(windows, '_windows', {}))!r}"
        )
    except Exception:
        _log(f"host probe {delay_ms}ms failed\n{traceback.format_exc()}")


def plugin_loaded() -> None:
    packages_root = os.environ.get("SUBLIME_TEXT_PACKAGES")
    schedule = os.path.join(
        packages_root or os.path.expanduser("~/.config/sublime-text/Packages"),
        "User",
        "UnitTesting",
        "schedule.json",
    )
    if not os.path.isfile(schedule):
        return
    _log(f"launcher: schedule found at {schedule}")
    for delay_ms in (0, 1000, 5000, 15000):
        sublime.set_timeout(lambda delay_ms=delay_ms: _probe_host(delay_ms), delay_ms)
    try:
        from UnitTesting.unittesting import run_scheduler
    except Exception:
        _log("launcher: importing UnitTesting failed\n" + traceback.format_exc())
        return
    try:
        run_scheduler()
    except Exception:
        _log("launcher: run_scheduler failed\n" + traceback.format_exc())
