"""Run the Sublime UnitTesting suite for LSP-perllsp with an honest wall clock.

Adapted from SublimeText/UnitTesting ``actions/run-tests/run_tests.py`` (MIT),
pinned in the workflow at the same commit the setup action installs.  The
upstream action waits 30 seconds for the in-Sublime result file to become
non-empty and retries by restarting Sublime Text three times.  That wall is
structurally too small for this package's deferred host journey: cold-starting
Sublime, loading the pinned LSP package, spawning the current-source perllsp
binary, and completing the LSP round trips legitimately needs minutes, and
every condition in the suite is individually bounded by ``condition_timeout``
in ``unittesting.json``.  Restarting Sublime mid-journey cannot make it
faster; it only aborts a suite that was making progress.

This runner keeps the upstream schedule mechanics (schedule.json plus the
scheduler shim inside the installed UnitTesting package) but polls for the
result once, with a generous default wall that the CI job timeout still
bounds, and dumps the recorded Sublime log and result file on failure so a
hang is diagnosable from the job log alone.  SIGTERM/SIGINT (the observed
ubuntu failure mode is an external kill long before any timeout fires) gets
the same treatment: a handler dumps the process tree, this process' wait
channel, and the recorded logs before re-raising with the original exit
semantics.

Usage:

    python scripts/sublime/run_unit_tests.py LSP-perllsp
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import threading
import time

PACKAGES_DIR_PATH = os.path.realpath(
    os.environ.get(
        "SUBLIME_TEXT_PACKAGES",
        os.path.join(os.path.dirname(__file__), "..", ".."),
    )
)
UT_OUTPUT_DIR_PATH = os.path.realpath(
    os.path.join(PACKAGES_DIR_PATH, "User", "UnitTesting")
)
SCHEDULE_FILE_PATH = os.path.join(UT_OUTPUT_DIR_PATH, "schedule.json")
SCHEDULE_RUNNER_TARGET = os.path.join(
    PACKAGES_DIR_PATH, "UnitTesting", "zzz_run_scheduler.py"
)
RX_RESULT = re.compile(r"^(?P<result>OK|FAILED|ERROR)", re.MULTILINE)
RX_DONE = re.compile(r"^UnitTesting: Done\.$", re.MULTILINE)

WAIT_TIMEOUT_SECONDS = int(
    os.environ.get("PERLLSP_UNITTESTING_WAIT_TIMEOUT", "900")
)
OUTPUT_IDLE_TIMEOUT_SECONDS = int(
    os.environ.get("PERLLSP_UNITTESTING_OUTPUT_IDLE_TIMEOUT", "300")
)

IS_WINDOWS = sys.platform == "win32"

_START = time.monotonic()


def _elapsed() -> str:
    return f"T+{time.monotonic() - _START:.1f}s"


def _dump_process_state() -> None:
    """Snapshot the process tree and this process' kernel wait channel.

    The observed ubuntu failure mode is an external SIGTERM ~10s after Sublime
    launches, long before any poll timeout can fire, so the normal failure
    dumps never run.  This snapshot exists to identify the killer's context:
    whether Xvfb/Sublime/plugin_host were alive, and what this process was
    doing when the signal landed.
    """
    identity = [f"pid={os.getpid()}"]
    if hasattr(os, "getpgrp"):
        try:
            identity.append(f"pgid={os.getpgrp()}")
        except OSError:
            pass
    if hasattr(os, "getsid"):
        try:
            identity.append(f"sid={os.getsid(0)}")
        except OSError:
            pass
    print(" ".join(identity))
    try:
        with open("/proc/self/wchan", encoding="utf-8") as handle:
            print(f"wchan: {handle.read().strip()}")
    except OSError:
        pass
    # Targeted first: the full axjf forest is dominated by kernel threads and
    # truncates before the user tree that identifies the signal's sender.
    for probe in (
        ["ps", "-eo", "pid,ppid,pgid,sid,stat,etime,args", "--sort=pid"],
        ["ps", "axjf"],
    ):
        try:
            result = subprocess.run(probe, capture_output=True, text=True)
        except OSError:
            continue
        if result.returncode == 0:
            print(f"===== {' '.join(probe)} =====")
            print(result.stdout[:6000])
            break
    for name in ("Xvfb", "sublime_text", "plugin_host", "perllsp"):
        try:
            result = subprocess.run(
                ["pgrep", "-a", name], capture_output=True, text=True
            )
        except OSError:
            continue
        state = result.stdout.strip() if result.returncode == 0 else "not running"
        print(f"===== {name}: {state or 'not running'} =====")


def _install_signal_diagnostics(package: str) -> None:
    def _handler(signum: int, _frame) -> None:
        print(
            f"\nreceived signal {signum} at {_elapsed()}; "
            "dumping state before re-raising",
            flush=True,
        )
        try:
            _dump_process_state()
            _diagnose(package)
        finally:
            # Block-buffered stdout would otherwise lose the dump.
            sys.stdout.flush()
            sys.stderr.flush()
            # Preserve the original exit semantics (128 + signum, e.g. 143).
            signal.signal(signum, signal.SIG_DFL)
            os.kill(os.getpid(), signum)

    for signum in (signal.SIGTERM, signal.SIGINT):
        try:
            signal.signal(signum, _handler)
        except (OSError, ValueError):
            pass


def _remove(path: str) -> None:
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass


def _diagnose(package: str) -> None:
    """Print launch evidence: shim logs at both known locations, the installed
    package layout, and whether a Sublime process is still alive."""
    _dump("UnitTesting/unittesting.log",
          os.path.join(PACKAGES_DIR_PATH, "UnitTesting", "unittesting.log"))
    _dump(f"{package}/unittesting.log",
          os.path.join(PACKAGES_DIR_PATH, package, "unittesting.log"))
    _dump("schedule.json", SCHEDULE_FILE_PATH)
    listing = subprocess.run(
        ["ls", "-la", PACKAGES_DIR_PATH],
        capture_output=True,
        text=True,
    )
    if listing.returncode == 0:
        print(f"===== {PACKAGES_DIR_PATH} =====")
        print(listing.stdout)
    user_dir = os.path.join(PACKAGES_DIR_PATH, "User")
    user_listing = subprocess.run(
        ["ls", "-la", user_dir], capture_output=True, text=True
    )
    if user_listing.returncode == 0:
        print(f"===== {user_dir} =====")
        print(user_listing.stdout)
    for probe in [
        ["ps", "-ax", "-o", "pid,command"],
        ["pwsh", "-command",
         "Get-Process | Where-Object {$_.Name -match 'sublime|plugin_host'}"
         " | Format-Table Id,Name,Path -AutoSize | Out-String"],
    ]:
        try:
            alive = subprocess.run(probe, capture_output=True, text=True)
        except OSError:
            continue
        if alive.returncode == 0:
            hits = "\n".join(
                line
                for line in alive.stdout.splitlines()
                if "sublime" in line.lower() or "plugin_host" in line.lower()
            )
            if hits:
                print("===== sublime processes (with command lines) =====")
                print(hits)
                break
    shim = os.path.join(PACKAGES_DIR_PATH, "UnitTesting", "zzz_run_scheduler.py")
    print(f"===== shim present: {os.path.isfile(shim)} ({shim}) =====")
    _dump("launcher log", os.path.expanduser("~/perllsp_sublime_host_ci.log"))
    home = os.path.expanduser("~")
    for crash_dir in (
        os.path.join(home, "Library", "Caches", "Sublime Text", "Crash Reports"),
        os.path.join(home, ".config", "sublime-text", "Crash Reports"),
    ):
        listing = subprocess.run(
            ["ls", "-laR", crash_dir], capture_output=True, text=True
        )
        if listing.returncode == 0 and listing.stdout.strip():
            print(f"===== {crash_dir} =====")
            print(listing.stdout[:2000])


def _dump(label: str, path: str) -> None:
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            content = handle.read()
    except OSError:
        return
    if content.strip():
        print(f"===== {label} ({path}) =====")
        print(content)
        print(f"===== end {label} =====")


def _wait_for_output(path: str, timeout: int) -> None:
    deadline = time.monotonic() + timeout
    last_heartbeat = time.monotonic()
    while True:
        try:
            if os.stat(path).st_size != 0:
                return
        except OSError:
            pass
        if time.monotonic() > deadline:
            raise ValueError(f"result file {path} stayed empty for {timeout}s")
        if time.monotonic() - last_heartbeat > 15:
            print(f" still waiting ({_elapsed()})", flush=True)
            last_heartbeat = time.monotonic()
        time.sleep(1)


def _read_output(path: str, idle_timeout: int) -> bool | None:
    success: bool | None = None
    done = False
    lines: list[str] = []
    partial = [""]

    def reader() -> None:
        with open(path, encoding="utf-8", errors="replace") as handle:
            while not done:
                offset = handle.tell()
                line = handle.readline()
                if line.endswith("\n"):
                    lines.append(line)
                else:
                    partial[0] = line
                    handle.seek(offset)
                    time.sleep(0.2)

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    last_update = time.monotonic()

    while not done:
        if time.monotonic() - last_update > idle_timeout:
            print(partial[0])
            raise TimeoutError(f"output stalled for {idle_timeout}s")
        if lines:
            last_update = time.monotonic()
            line = lines.pop(0)
        else:
            time.sleep(0.2)
            continue
        print(line, end="")
        match = RX_RESULT.search(line)
        if match is not None:
            success = match.group("result") == "OK"
        if RX_DONE.search(line) is not None:
            done = True
    return success


def _start_sublime_text() -> None:
    if IS_WINDOWS:
        subprocess.Popen(["sublime_text.exe"])
    else:
        subprocess.Popen(["subl", "--stay"], start_new_session=True)


def _kill_sublime_text() -> None:
    if IS_WINDOWS:
        subprocess.run(
            [
                "pwsh",
                "-command",
                "stop-process -force -processname sublime_text -ea silentlycontinue",
            ],
            check=False,
        )
    else:
        # Match process NAMES, never full command lines: the runner's own
        # invocation carries "sublime" in its path, so a -f pattern like
        # '[Ss]ubl' signals the runner itself during cleanup (observed as the
        # T+8s exit-143 on the Linux DAP leg after a green journey).
        subprocess.run(
            "pkill -x sublime_text || true; pkill -x 'plugin_host-3.3'"
            " || true; pkill -x 'plugin_host-3.8' || true",
            shell=True,
            check=False,
        )


def main() -> int:
    package = sys.argv[1] if len(sys.argv) > 1 else "LSP-perllsp"
    _install_signal_diagnostics(package)
    output_dir = os.path.join(UT_OUTPUT_DIR_PATH, package)
    output_file = os.path.join(output_dir, "result")
    log_file = os.path.join(PACKAGES_DIR_PATH, package, "unittesting.log")

    os.makedirs(output_dir, exist_ok=True)
    _remove(output_file)
    # The upstream shim-copy dance is replaced by the package's own
    # host_ci_launcher plugin, which imports UnitTesting's scheduler directly
    # at plugin_loaded; nothing is installed into the UnitTesting package.
    _remove(SCHEDULE_RUNNER_TARGET)

    schedule: list[dict] = []
    try:
        with open(SCHEDULE_FILE_PATH, encoding="utf-8") as handle:
            schedule = json.load(handle)
    except (OSError, ValueError):
        pass
    if not any(entry.get("package") == package for entry in schedule):
        schedule.append(
            {
                "package": package,
                "syntax_test": False,
                "syntax_compatibility": False,
                "color_scheme_test": False,
                "coverage": False,
                "output": output_file,
            }
        )
    with open(SCHEDULE_FILE_PATH, "w", encoding="utf-8") as handle:
        json.dump(schedule, handle, ensure_ascii=False, indent=True)

    _start_sublime_text()
    print(f"Wait for tests output (wall {WAIT_TIMEOUT_SECONDS}s)...", end="", flush=True)
    try:
        _wait_for_output(output_file, WAIT_TIMEOUT_SECONDS)
    except ValueError as error:
        print()
        print(f"Timeout: {error}")
        print(
            "The in-Sublime suite did not report within the wall clock; "
            "dumping recorded logs for diagnosis."
        )
        _diagnose(package)
        _dump("result", output_file)
        _remove(SCHEDULE_RUNNER_TARGET)
        _kill_sublime_text()
        return 1

    print()
    print("Start to read output...")
    try:
        success = _read_output(output_file, OUTPUT_IDLE_TIMEOUT_SECONDS)
    except TimeoutError as error:
        print(f"Timeout: {error}")
        _diagnose(package)
        _remove(SCHEDULE_RUNNER_TARGET)
        _kill_sublime_text()
        return 1
    _remove(SCHEDULE_RUNNER_TARGET)
    _kill_sublime_text()
    return 0 if success is True else 1


if __name__ == "__main__":
    sys.exit(main())
