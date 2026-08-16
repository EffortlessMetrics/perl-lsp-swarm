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
hang is diagnosable from the job log alone.

Usage:

    python scripts/sublime/run_unit_tests.py LSP-perllsp
"""

from __future__ import annotations

import json
import os
import re
import shutil
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
SCHEDULE_RUNNER_SOURCE = os.path.join(
    PACKAGES_DIR_PATH, "UnitTesting", "actions", "run-tests", "run_scheduler.py"
)
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


def _remove(path: str) -> None:
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass


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
    while True:
        try:
            if os.stat(path).st_size != 0:
                return
        except OSError:
            pass
        if time.monotonic() > deadline:
            raise ValueError(f"result file {path} stayed empty for {timeout}s")
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
        subprocess.run(
            "pkill -f '[Ss]ubl' || true; pkill plugin_host || true",
            shell=True,
            check=False,
        )


def main() -> int:
    package = sys.argv[1] if len(sys.argv) > 1 else "LSP-perllsp"
    output_dir = os.path.join(UT_OUTPUT_DIR_PATH, package)
    output_file = os.path.join(output_dir, "result")
    log_file = os.path.join(PACKAGES_DIR_PATH, package, "unittesting.log")

    os.makedirs(output_dir, exist_ok=True)
    _remove(output_file)
    _remove(SCHEDULE_RUNNER_TARGET)
    if not os.path.isfile(SCHEDULE_RUNNER_SOURCE):
        print(
            f"UnitTesting scheduler shim missing: {SCHEDULE_RUNNER_SOURCE}",
            file=sys.stderr,
        )
        return 1
    shutil.copyfile(SCHEDULE_RUNNER_SOURCE, SCHEDULE_RUNNER_TARGET)

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
        _dump("unittesting.log", log_file)
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
        _dump("unittesting.log", log_file)
        _remove(SCHEDULE_RUNNER_TARGET)
        _kill_sublime_text()
        return 1
    _remove(SCHEDULE_RUNNER_TARGET)
    _kill_sublime_text()
    return 0 if success is True else 1


if __name__ == "__main__":
    sys.exit(main())
