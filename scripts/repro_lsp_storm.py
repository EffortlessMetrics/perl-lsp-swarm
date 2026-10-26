#!/usr/bin/env python3
"""Drive perl-lsp through open/change/close churn and record RSS samples."""

from __future__ import annotations

import argparse
import csv
import errno
import json
import os
import queue
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from urllib.parse import quote


def env_int(name: str, default: int) -> int:
    value = os.environ.get(name)
    if value is None:
        return default
    try:
        return int(value)
    except ValueError:
        raise SystemExit(f"{name} must be an integer, got {value!r}")


def file_uri(path: Path) -> str:
    return "file://" + quote(str(path.resolve()))


def rss_kb(pid: int) -> int:
    status = Path(f"/proc/{pid}/status")
    if status.exists():
        for line in status.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])

    out = subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)
    return int(out.strip() or "0")


def send_message(proc: subprocess.Popen[bytes], payload: dict) -> None:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    assert proc.stdin is not None
    proc.stdin.write(header + body)
    proc.stdin.flush()


def read_messages(proc: subprocess.Popen[bytes], out: "queue.Queue[dict]") -> None:
    assert proc.stdout is not None
    stream = proc.stdout
    while True:
        headers: dict[str, str] = {}
        while True:
            line = stream.readline()
            if not line:
                return
            if line == b"\r\n":
                break
            key, _, value = line.decode("ascii", errors="replace").partition(":")
            headers[key.lower()] = value.strip()
        length = int(headers.get("content-length", "0"))
        if length <= 0:
            continue
        raw = stream.read(length)
        if not raw:
            return
        try:
            out.put(json.loads(raw.decode("utf-8")))
        except json.JSONDecodeError:
            continue


def perl_source(index: int, version: int) -> str:
    package = f"Storm::File{index:05d}"
    return (
        f"package {package};\n"
        "use strict;\n"
        "use warnings;\n\n"
        f"sub value_{index:05d} {{ return {index + version}; }}\n"
        f"sub call_{index:05d} {{ return value_{index:05d}(); }}\n\n"
        "1;\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out", type=Path)
    parser.add_argument("--csv-out", type=Path)
    parser.add_argument("--binary", default=os.environ.get("BINARY", "./target/release/perllsp"))
    parser.add_argument("--n-files", type=int, default=env_int("N_FILES", 500))
    parser.add_argument("--n-changes", type=int, default=env_int("N_CHANGES", 10))
    parser.add_argument(
        "--workspace-symbol",
        action="store_true",
        default=os.environ.get("DO_WORKSPACE_SYMBOL", "0") == "1",
    )
    parser.add_argument(
        "--delete-after-close",
        action="store_true",
        default=os.environ.get("DELETE_AFTER_CLOSE", "0") == "1",
        help="unlink each file after didClose and send a watched-file DELETED event",
    )
    parser.add_argument("--sample-every", type=int, default=env_int("SAMPLE_EVERY", 10))
    parser.add_argument("--settle-seconds", type=float, default=float(os.environ.get("SETTLE_SECONDS", "1.0")))
    parser.add_argument("--server-stderr", type=Path)
    args = parser.parse_args()

    binary = shutil.which(args.binary) or args.binary
    server_stderr = subprocess.DEVNULL
    stderr_handle = None
    if args.server_stderr:
        args.server_stderr.parent.mkdir(parents=True, exist_ok=True)
        stderr_handle = args.server_stderr.open("wb")
        server_stderr = stderr_handle

    with tempfile.TemporaryDirectory(prefix="perl-lsp-storm-") as tmp:
        root = Path(tmp)
        proc = subprocess.Popen(
            [binary, "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=server_stderr,
        )
        responses: "queue.Queue[dict]" = queue.Queue()
        reader = threading.Thread(target=read_messages, args=(proc, responses), daemon=True)
        reader.start()

        next_id = 1
        start = time.monotonic()
        samples: list[dict] = []

        def sample(phase: str, file_index: int) -> None:
            try:
                rss = rss_kb(proc.pid)
            except Exception as exc:  # pragma: no cover - platform fallback
                print(f"rss sample failed: {exc}", file=sys.stderr)
                rss = 0
            row = {
                "elapsed_s": round(time.monotonic() - start, 3),
                "phase": phase,
                "file_index": file_index,
                "rss_kb": rss,
            }
            samples.append(row)
            print(json.dumps(row, separators=(",", ":")), file=sys.stderr)

        def request(method: str, params: dict | None = None) -> int:
            nonlocal next_id
            req_id = next_id
            next_id += 1
            send_message(proc, {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}})
            return req_id

        def notify(method: str, params: dict | None = None) -> None:
            send_message(proc, {"jsonrpc": "2.0", "method": method, "params": params or {}})

        def wait_for_response(req_id: int, timeout: float = 10.0) -> dict:
            deadline = time.monotonic() + timeout
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"timed out waiting for response id {req_id}")
                try:
                    message = responses.get(timeout=min(remaining, 0.05))
                except queue.Empty:
                    if not reader.is_alive():
                        # EOF on the server's stdout: nothing else can arrive.
                        raise TimeoutError(
                            f"server closed stdout before responding to request id {req_id}"
                        ) from None
                    continue
                if message.get("id") == req_id:
                    return message

        try:
            initialize_id = request(
                "initialize",
                {
                    "processId": os.getpid(),
                    "rootUri": file_uri(root),
                    "capabilities": {
                        "textDocument": {
                            "synchronization": {"didSave": True},
                            "publishDiagnostics": {"relatedInformation": True},
                        },
                        "workspace": {"workspaceFolders": True},
                    },
                    "workspaceFolders": [{"uri": file_uri(root), "name": "storm"}],
                },
            )
            wait_for_response(initialize_id, timeout=10.0)
            notify("initialized", {})
            sample("initialized", 0)

            for i in range(args.n_files):
                path = root / f"File{i:05d}.pm"
                text = perl_source(i, 0)
                path.write_text(text, encoding="utf-8")
                uri = file_uri(path)
                notify(
                    "textDocument/didOpen",
                    {
                        "textDocument": {
                            "uri": uri,
                            "languageId": "perl",
                            "version": 1,
                            "text": text,
                        }
                    },
                )
                for change in range(args.n_changes):
                    text = perl_source(i, change + 1)
                    notify(
                        "textDocument/didChange",
                        {
                            "textDocument": {"uri": uri, "version": change + 2},
                            "contentChanges": [{"text": text}],
                        },
                    )
                if args.workspace_symbol:
                    request("workspace/symbol", {"query": f"value_{i:05d}"})
                notify("textDocument/didClose", {"textDocument": {"uri": uri}})
                if args.delete_after_close:
                    path.unlink(missing_ok=True)
                    notify(
                        "workspace/didChangeWatchedFiles",
                        {
                            "changes": [
                                {
                                    "uri": uri,
                                    "type": 3,
                                }
                            ]
                        },
                    )

                if i % max(args.sample_every, 1) == 0 or i == args.n_files - 1:
                    sample("churn", i + 1)

            time.sleep(args.settle_seconds)
            sample("settled", args.n_files)
            # #18349 review: a successful workload write is not evidence the
            # server consumed the workload — the OS pipe buffer can absorb
            # writes a dying or wedged server never reads, and zero-RSS rows
            # from a dead peer are discarded by the plateau assertion, so an
            # incomplete workload could pass as a memory plateau. This
            # in-order barrier request is that evidence: the server can only
            # answer it after dequeuing every preceding workload message. A
            # failed barrier write or a barrier timeout is fatal (raised
            # outside the tolerated teardown region below).
            barrier_id = request("workspace/symbol", {"query": "value_00000"})
            wait_for_response(barrier_id, timeout=10.0)
            teardown_race = None
            teardown_note = None
            try:
                shutdown_id = request("shutdown", {})
                wait_for_response(shutdown_id, timeout=10.0)
                notify("exit", {})
            except BrokenPipeError as exc:
                teardown_race = exc
            except TimeoutError as exc:
                # A server that consumed the barrier-confirmed workload and
                # then died during the shutdown exchange may close stdout
                # before answering `shutdown` — on Windows the intervening
                # write can still succeed into the pipe buffer, so this is
                # the same dead-peer race as the write-failure shapes and
                # surfaces here within seconds (the reader EOF fast-fail
                # above). Without the barrier response this timeout remains
                # fatal.
                teardown_race = exc
            except OSError as exc:
                # Windows reports the same dead-peer stdin write as EINVAL
                # rather than EPIPE (observed: `OSError(22, 'Invalid
                # argument')` on win32); only that exact shape is tolerated.
                if exc.errno != errno.EINVAL:
                    raise
                teardown_race = exc
            if teardown_race is not None:
                # #18349: the server's documented delivery-failure policy
                # closes ingress — over stdio ending the process — when the
                # teardown burst overflows the outbound channel. Tolerance is
                # conditional on the post-workload barrier response above
                # having confirmed the workload was consumed; a server that
                # died earlier fails the run at the barrier instead. Proceed
                # with the samples already collected so the plateau assertion
                # still runs (and still fails on a real memory regression). A
                # mid-workload server death is NOT tolerated here and still
                # fails the run, as does a timeout waiting for the shutdown
                # response.
                print(
                    f"teardown race tolerated ({type(teardown_race).__name__}: "
                    f"{teardown_race}): workload completion was confirmed by "
                    "the post-workload workspace/symbol barrier response; the "
                    "server closed stdio during teardown; proceeding to the "
                    "plateau assertion on the samples already collected",
                    file=sys.stderr,
                )
                teardown_note = {
                    "stage": "shutdown",
                    "error": type(teardown_race).__name__,
                    "errno": getattr(teardown_race, "errno", None),
                    "workload_confirmed_by": "workspace/symbol barrier response",
                }
        finally:
            try:
                if proc.stdin:
                    proc.stdin.close()
            except OSError:
                # Discarding the pipe is best-effort cleanup: a server that
                # already self-exited (#18349 teardown race) leaves the write
                # end dead — BrokenPipeError on Linux, EINVAL on Windows.
                pass
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
            if stderr_handle:
                stderr_handle.close()

        result = {
            "schema": 1,
            "binary": str(binary),
            "n_files": args.n_files,
            "n_changes": args.n_changes,
            "workspace_symbol": args.workspace_symbol,
            "delete_after_close": args.delete_after_close,
            "sample_every": args.sample_every,
            "settle_seconds": args.settle_seconds,
            "samples": samples,
            # #18349: None unless the post-workload shutdown hit the server's
            # documented delivery-failure self-exit and was tolerated; when
            # set, the plateau assertion ran on the samples already collected.
            "teardown_race": teardown_note,
        }

        if args.json_out:
            args.json_out.parent.mkdir(parents=True, exist_ok=True)
            args.json_out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        if args.csv_out:
            args.csv_out.parent.mkdir(parents=True, exist_ok=True)
            with args.csv_out.open("w", newline="", encoding="utf-8") as fh:
                writer = csv.DictWriter(fh, fieldnames=["elapsed_s", "phase", "file_index", "rss_kb"])
                writer.writeheader()
                writer.writerows(samples)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
