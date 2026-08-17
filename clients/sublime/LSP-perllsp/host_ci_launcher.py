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
import traceback

_LAUNCHER_LOG = os.path.join(
    os.path.expanduser("~"), "perllsp_sublime_host_ci.log"
)


def _log(message: str) -> None:
    with open(_LAUNCHER_LOG, "a", encoding="utf-8") as handle:
        handle.write(message + "\n")


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
    try:
        from UnitTesting.unittesting import run_scheduler
    except Exception:
        _log("launcher: importing UnitTesting failed\n" + traceback.format_exc())
        return
    try:
        run_scheduler()
    except Exception:
        _log("launcher: run_scheduler failed\n" + traceback.format_exc())
