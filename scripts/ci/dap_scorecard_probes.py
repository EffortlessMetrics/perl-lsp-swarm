"""Real stdio launch, attach, session, pagination, and memory probes."""

from __future__ import annotations

import queue
import socket
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Mapping

from dap_scorecard_model import DEFAULT_TIMEOUT_SECONDS, ScorecardError, metric_failure
from dap_scorecard_transport import DapProcess, InvocationCounter, frame_message


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


def probe_launch(
    binary: Path,
    script: Path,
    timeout_seconds: float,
    invocations: InvocationCounter,
) -> int:
    with DapProcess(binary, timeout_seconds, invocations) as dap:
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


def probe_attach(
    binary: Path,
    timeout_seconds: float,
    invocations: InvocationCounter,
) -> int:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = int(listener.getsockname()[1])
    errors: queue.Queue[BaseException] = queue.Queue()
    server = threading.Thread(target=_serve_attach, args=(listener, errors), daemon=True)
    server.start()

    with DapProcess(binary, timeout_seconds, invocations) as dap:
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


def _positive_int(value: Any) -> int | None:
    if isinstance(value, int) and not isinstance(value, bool) and value > 0:
        return value
    return None


def _fixture_name_matches(name: str, sigil: str, bare_name: str) -> bool:
    return name == f"{sigil}{bare_name}" or name.endswith(f"::{bare_name}")


def _query_scope_variables(
    dap: DapProcess,
    scopes: list[Any],
) -> tuple[dict[str, list[Mapping[str, Any]]], dict[str, str]]:
    scope_variables: dict[str, list[Mapping[str, Any]]] = {}
    scope_errors: dict[str, str] = {}
    for raw_scope in scopes:
        scope = _require_object(raw_scope, "scope")
        name = str(scope.get("name", ""))
        if name not in {"Package", "Globals", "Locals"}:
            continue
        variables_ref = _positive_int(scope.get("variablesReference"))
        if variables_ref is None:
            scope_errors[name] = "missing positive variablesReference"
            continue
        try:
            body = _require_object(
                dap.request("variables", {"variablesReference": variables_ref}),
                f"{name} variables body",
            )
            rows = _require_array(body.get("variables"), f"{name} variables")
            scope_variables[name] = [
                _require_object(row, f"{name} variable") for row in rows
            ]
        except ScorecardError as exc:
            scope_errors[name] = str(exc)
    return scope_variables, scope_errors


def _variable_samples(scope_variables: Mapping[str, list[Mapping[str, Any]]]) -> str:
    samples: list[str] = []
    for scope_name, rows in scope_variables.items():
        names = [str(row.get("name", "")) for row in rows[:8]]
        samples.append(f"{scope_name}={names!r}")
    return "; ".join(samples) or "<no scope rows>"


def _require_lexical_big(
    scope_variables: Mapping[str, list[Mapping[str, Any]]],
) -> Mapping[str, Any]:
    locals_rows = scope_variables.get("Locals", [])
    matches = [row for row in locals_rows if str(row.get("name", "")) == "@big"]
    if len(matches) != 1:
        names = [str(row.get("name", "")) for row in locals_rows]
        raise ScorecardError(
            f"expected exactly one Locals @big row, got {len(matches)}; "
            f"locals={names!r}"
        )
    return matches[0]


def _validate_unexpanded_lexical_big(row: Mapping[str, Any]) -> str:
    """Assert the adapter stays honest about not having observed `@big`.

    The lexical `B` query deliberately does not enumerate live aggregates, so the
    only truthful rendering is an opaque, non-expandable marker. Two things must
    therefore be absent: a fabricated element total, and a reference that would
    invite the client to page contents the adapter never read. Bounded lexical
    snapshots are owned by #7358; this cell fails the moment either appears.
    """
    value = str(row.get("value", ""))
    variables_ref = row.get("variablesReference")
    indexed = row.get("indexedVariables")
    named = row.get("namedVariables")

    if _positive_int(variables_ref) is not None:
        raise ScorecardError(
            "Locals @big advertised an expandable variablesReference "
            f"({variables_ref!r}) without a proven bounded snapshot (#7358)"
        )
    for label, count in (("indexedVariables", indexed), ("namedVariables", named)):
        if count is not None:
            raise ScorecardError(
                f"Locals @big fabricated {label}={count!r}; the lexical query "
                "never observed the aggregate contents (#7358)"
            )
    # The scalar renderer quotes string values, so the observed marker is
    # `"ARRAY(0x0)"` rather than a bare `ARRAY(0x0)`.
    if not value.strip('"').startswith("ARRAY("):
        raise ScorecardError(
            f"Locals @big must render as an opaque ARRAY marker, got value={value!r}"
        )
    return value




def probe_session_metrics(
    binary: Path,
    timeout_seconds: float,
    invocations: InvocationCounter,
) -> tuple[dict[str, str], dict[str, str], dict[str, str], dict[str, str]]:
    script_text = """use strict;
use warnings;
my $x = 41;
my @big = (1..500);
my %meta = (name => \"dap-scorecard\");
my $marker = $x + 1;
print \"marker=$marker\\n\";
"""
    with tempfile.TemporaryDirectory(prefix="dap-scorecard-") as temp_dir:
        script = Path(temp_dir) / "scorecard_session.pl"
        script.write_text(script_text, encoding="utf-8")
        with DapProcess(binary, timeout_seconds, invocations) as dap:
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
            scope_variables, scope_errors = _query_scope_variables(dap, scopes)
            all_variables = [row for rows in scope_variables.values() for row in rows]
            names = [str(row.get("name", "")) for row in all_variables]
            expected = {
                "$x": any(_fixture_name_matches(name, "$", "x") for name in names),
                "@big": any(_fixture_name_matches(name, "@", "big") for name in names),
                "%meta": any(_fixture_name_matches(name, "%", "meta") for name in names),
            }
            missing = [name for name, present in expected.items() if not present]
            if missing:
                variables_metric = metric_failure(
                    "stdio variable scopes did not expose fixture package variables "
                    f"{missing!r}; samples={_variable_samples(scope_variables)}; "
                    f"scope_errors={scope_errors!r}"
                )
            elif any(not name for name in names):
                variables_metric = metric_failure(
                    "stdio variable scopes contained unnamed rows; "
                    f"samples={_variable_samples(scope_variables)}"
                )
            else:
                variables_metric = {
                    "status": "PASS",
                    "detail": (
                        "stdio Package/Globals/Locals scopes exposed $x, @big, and %meta "
                        f"across {len(all_variables)} named variables"
                    ),
                }

            deep_setup_error: str | None = None
            value_before = ""
            try:
                value_before = _validate_unexpanded_lexical_big(
                    _require_lexical_big(scope_variables)
                )
            except ScorecardError as exc:
                deep_setup_error = str(exc)

            try:
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
                    raise ScorecardError(
                        f"stdio evaluate result did not contain 42: {result!r}"
                    )
                evaluate_metric = {
                    "status": "PASS",
                    "detail": "stdio evaluate($x + 1) returns 42",
                }
            except ScorecardError as exc:
                evaluate_metric = metric_failure(str(exc))

            if deep_setup_error is not None:
                deep_metric = metric_failure(deep_setup_error)
            else:
                try:
                    after_scopes, _after_errors = _query_scope_variables(dap, scopes)
                    value_after = _validate_unexpanded_lexical_big(
                        _require_lexical_big(after_scopes)
                    )
                    if value_after != value_before:
                        raise ScorecardError(
                            "Locals @big rendering drifted across evaluate: "
                            f"{value_before!r} then {value_after!r}"
                        )
                    deep_metric = {
                        "status": "NOT_PROVEN",
                        "detail": (
                            "stdio Locals @big stays a non-expandable marker "
                            f"({value_after}) with no variablesReference and no "
                            "element counts before and after evaluate; bounded "
                            "lexical collection snapshots and their deep-pagination "
                            "proof are owned by issue 7358"
                        ),
                    }
                except ScorecardError as exc:
                    deep_metric = metric_failure(str(exc))

            try:
                memory_metric = {
                    "status": "MEASURED",
                    "detail": f"exact perl-dap stdio process VmRSS={dap.rss_kb()} KiB",
                }
            except ScorecardError as exc:
                memory_metric = metric_failure(str(exc))

            # A probe is not successful until the disconnect response, terminated
            # event, stdin closure, and clean adapter exit are all observed.
            dap.disconnect()
            return variables_metric, evaluate_metric, deep_metric, memory_metric
