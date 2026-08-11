"""Real stdio launch, attach, session, pagination, and memory probes."""

from __future__ import annotations

import queue
import socket
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Mapping

from dap_scorecard_model import DEFAULT_TIMEOUT_SECONDS, ScorecardError
from dap_scorecard_transport import DapProcess, frame_message


def _launch_arguments(script: Path, *, stop_on_entry: bool) -> dict[str, Any]:
    return {
        "program": str(script.resolve()),
        "args": [],
        "stopOnEntry": stop_on_entry,
        "env": {
            "PERL_PERTURB_KEYS": "0",
            "PERL_HASH_SEED": "0",
            "LC_ALL": "C",
            "TZ": "UTC",
        },
    }


def probe_launch(binary: Path, script: Path, timeout_seconds: float) -> int:
    with DapProcess(binary, timeout_seconds) as dap:
        dap.initialize()
        started = time.monotonic()
        dap.request("launch", _launch_arguments(script, stop_on_entry=True))
        dap.wait_event("stopped")
        elapsed_ms = int((time.monotonic() - started) * 1000)
        dap.disconnect()
        return elapsed_ms


def _serve_attach(listener: socket.socket, errors: queue.Queue[BaseException]) -> None:
    try:
        listener.settimeout(DEFAULT_TIMEOUT_SECONDS)
        connection, _address = listener.accept()
        with connection:
            connection.sendall(
                frame_message(
                    {
                        "type": "event",
                        "seq": 1,
                        "event": "stopped",
                        "body": {
                            "reason": "breakpoint",
                            "threadId": 7,
                            "allThreadsStopped": True,
                        },
                    }
                )
            )
            connection.settimeout(1.0)
            while True:
                try:
                    if not connection.recv(4096):
                        break
                except socket.timeout:
                    continue
    except BaseException as exc:
        errors.put(exc)
    finally:
        listener.close()


def probe_attach(binary: Path, timeout_seconds: float) -> int:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = int(listener.getsockname()[1])
    errors: queue.Queue[BaseException] = queue.Queue()
    server = threading.Thread(target=_serve_attach, args=(listener, errors), daemon=True)
    server.start()

    with DapProcess(binary, timeout_seconds) as dap:
        dap.initialize()
        started = time.monotonic()
        dap.request("attach", {"host": "127.0.0.1", "port": port, "timeout": 2000})
        dap.wait_event("stopped")
        elapsed_ms = int((time.monotonic() - started) * 1000)
        dap.disconnect()

    server.join(timeout=timeout_seconds)
    if server.is_alive():
        raise ScorecardError("fake TCP debugger did not stop after adapter disconnect")
    if not errors.empty():
        error = errors.get_nowait()
        raise ScorecardError(f"fake TCP debugger failed: {error}") from error
    return elapsed_ms


def _require_object(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise ScorecardError(f"{context} must be an object")
    return value


def _require_array(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise ScorecardError(f"{context} must be an array")
    return value


def probe_session_metrics(
    binary: Path, timeout_seconds: float
) -> tuple[dict[str, str], dict[str, str], dict[str, str], dict[str, str]]:
    script_text = """use strict;
use warnings;
our $x = 41;
our @big = (1..500);
our %meta = (name => \"dap-scorecard\");
my $marker = $x + 1;
print \"marker=$marker\\n\";
"""
    with tempfile.TemporaryDirectory(prefix="dap-scorecard-") as temp_dir:
        script = Path(temp_dir) / "scorecard_session.pl"
        script.write_text(script_text, encoding="utf-8")
        with DapProcess(binary, timeout_seconds) as dap:
            dap.initialize()
            dap.request("launch", _launch_arguments(script, stop_on_entry=False))
            breakpoints = dap.request(
                "setBreakpoints",
                {"source": {"path": str(script)}, "breakpoints": [{"line": 6}]},
            )
            rows = _require_array(
                _require_object(breakpoints, "setBreakpoints body").get("breakpoints"),
                "setBreakpoints.breakpoints",
            )
            if len(rows) != 1 or not _require_object(rows[0], "breakpoint").get("verified"):
                raise ScorecardError(f"scorecard breakpoint was not verified: {rows!r}")
            dap.request("configurationDone")
            stopped = _require_object(dap.wait_event("stopped"), "stopped body")
            thread_id = stopped.get("threadId")
            if isinstance(thread_id, bool) or not isinstance(thread_id, int):
                raise ScorecardError("stopped event omitted an integer threadId")

            stack = _require_object(
                dap.request("stackTrace", {"threadId": thread_id, "startFrame": 0, "levels": 1}),
                "stackTrace body",
            )
            frames = _require_array(stack.get("stackFrames"), "stackTrace.stackFrames")
            if not frames:
                raise ScorecardError("stackTrace returned no frames")
            frame_id = _require_object(frames[0], "top stack frame").get("id")
            if isinstance(frame_id, bool) or not isinstance(frame_id, int):
                raise ScorecardError("top stack frame omitted an integer id")

            scopes = _require_array(
                _require_object(dap.request("scopes", {"frameId": frame_id}), "scopes body").get(
                    "scopes"
                ),
                "scopes.scopes",
            )
            globals_scope = next(
                (
                    _require_object(scope, "scope")
                    for scope in scopes
                    if isinstance(scope, dict)
                    and str(scope.get("name", "")).lower() == "globals"
                ),
                None,
            )
            if globals_scope is None:
                raise ScorecardError(f"Globals scope was absent: {scopes!r}")
            globals_ref = globals_scope.get("variablesReference")
            if isinstance(globals_ref, bool) or not isinstance(globals_ref, int) or globals_ref <= 0:
                raise ScorecardError("Globals scope omitted a positive variablesReference")

            variables = _require_array(
                _require_object(
                    dap.request("variables", {"variablesReference": globals_ref}),
                    "variables body",
                ).get("variables"),
                "variables.variables",
            )
            names = [str(_require_object(variable, "variable").get("name", "")) for variable in variables]
            if not variables or any(not name for name in names):
                raise ScorecardError(f"Globals variables were empty or unnamed: {names!r}")
            variables_metric = {
                "status": "PASS",
                "detail": f"stdio Globals scope returned {len(variables)} named variables",
            }

            evaluate = _require_object(
                dap.request(
                    "evaluate",
                    {
                        "expression": "$x + 1",
                        "frameId": frame_id,
                        "context": "watch",
                        "allowSideEffects": False,
                    },
                ),
                "evaluate body",
            )
            result = str(evaluate.get("result", ""))
            if "42" not in result:
                raise ScorecardError(f"stdio evaluate result did not contain 42: {result!r}")
            evaluate_metric = {"status": "PASS", "detail": "stdio evaluate($x + 1) returns 42"}

            expandable: Mapping[str, Any] | None = None
            for variable in variables:
                row = _require_object(variable, "variable")
                variables_ref = row.get("variablesReference")
                indexed = row.get("indexedVariables")
                if (
                    isinstance(variables_ref, int)
                    and not isinstance(variables_ref, bool)
                    and variables_ref > 0
                    and isinstance(indexed, int)
                    and not isinstance(indexed, bool)
                    and indexed >= 200
                ):
                    expandable = row
                    break
            if expandable is None:
                raise ScorecardError(
                    "stdio Globals scope exposed no indexed variable with indexedVariables >= 200"
                )
            indexed_ref = int(expandable["variablesReference"])
            indexed_count = int(expandable["indexedVariables"])
            page = _require_array(
                _require_object(
                    dap.request(
                        "variables",
                        {"variablesReference": indexed_ref, "start": 250, "count": 25},
                    ),
                    "paged variables body",
                ).get("variables"),
                "paged variables",
            )
            if len(page) != 25:
                raise ScorecardError(f"expected 25 paged variables, got {len(page)}")
            first = str(_require_object(page[0], "paged variable[0]").get("name", ""))
            last = str(_require_object(page[-1], "paged variable[-1]").get("name", ""))
            if first != "[250]" or last != "[274]":
                raise ScorecardError(f"unexpected stdio page bounds: first={first!r}, last={last!r}")
            deep_metric = {
                "status": "PASS",
                "detail": (
                    "stdio pagination returned [250]..[274] over "
                    f"indexedVariables={indexed_count}"
                ),
            }
            memory_metric = {
                "status": "MEASURED",
                "detail": f"exact perl-dap stdio process VmRSS={dap.rss_kb()} KiB",
            }
            dap.disconnect()
            return variables_metric, evaluate_metric, deep_metric, memory_metric
