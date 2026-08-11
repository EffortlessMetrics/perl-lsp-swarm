"""Bounded stdio transport client for exact-binary DAP scorecard sessions."""

from __future__ import annotations

import json
import queue
import subprocess
import threading
import time
from collections import deque
from pathlib import Path
from typing import Any, BinaryIO, Callable, Mapping

from dap_scorecard_model import ScorecardError

MAX_FRAME_HEADER_BYTES = 8_192


def frame_message(message: Mapping[str, Any]) -> bytes:
    payload = json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii") + payload


def _read_exact(stream: BinaryIO, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise ScorecardError(
                f"DAP transport ended with {remaining} bytes still required from the frame"
            )
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_framed_message(stream: BinaryIO) -> Mapping[str, Any]:
    header = bytearray()
    while not header.endswith(b"\r\n\r\n"):
        byte = stream.read(1)
        if not byte:
            raise EOFError("DAP stdout closed")
        header.extend(byte)
        if len(header) > MAX_FRAME_HEADER_BYTES:
            raise ScorecardError("DAP frame header exceeded the bounded limit")

    try:
        header_text = header.decode("ascii")
    except UnicodeDecodeError as exc:
        raise ScorecardError("DAP frame header was not ASCII") from exc

    content_length: int | None = None
    for line in header_text.split("\r\n"):
        name, separator, raw_value = line.partition(":")
        if separator and name.strip().lower() == "content-length":
            try:
                content_length = int(raw_value.strip())
            except ValueError as exc:
                raise ScorecardError(f"invalid DAP Content-Length: {raw_value!r}") from exc
            break
    if content_length is None or content_length < 0:
        raise ScorecardError(f"DAP frame omitted a valid Content-Length: {header_text!r}")

    body = _read_exact(stream, content_length)
    try:
        value = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ScorecardError(f"DAP frame body was not valid UTF-8 JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ScorecardError("DAP frame body must be a JSON object")
    return value


def _message_label(message: Mapping[str, Any]) -> str:
    message_type = message.get("type")
    if message_type == "response":
        return (
            f"response:{message.get('command')}#{message.get('request_seq')}:"
            f"success={message.get('success')}"
        )
    if message_type == "event":
        return f"event:{message.get('event')}"
    if message_type == "request":
        return f"request:{message.get('command')}"
    return f"unknown:{message_type!r}"


class DapProcess:
    """One exact-binary stdio DAP session with bounded message waits."""

    def __init__(self, binary: Path, timeout_seconds: float) -> None:
        self.binary = binary.resolve()
        self.timeout_seconds = timeout_seconds
        self._messages: queue.Queue[Mapping[str, Any] | BaseException] = queue.Queue()
        self._pending: deque[Mapping[str, Any]] = deque()
        self._stderr: deque[str] = deque(maxlen=80)
        self._seq = 0
        try:
            self.process = subprocess.Popen(
                [str(self.binary), "--stdio", "--log-level", "error"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
            )
        except OSError as exc:
            raise ScorecardError(f"cannot spawn exact perl-dap binary {self.binary}: {exc}") from exc
        if self.process.stdin is None or self.process.stdout is None or self.process.stderr is None:
            self.close()
            raise ScorecardError("perl-dap stdio pipes were not created")
        self._reader = threading.Thread(target=self._reader_loop, daemon=True)
        self._stderr_reader = threading.Thread(target=self._stderr_loop, daemon=True)
        self._reader.start()
        self._stderr_reader.start()

    def __enter__(self) -> "DapProcess":
        return self

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()

    @property
    def pid(self) -> int:
        return self.process.pid

    def _reader_loop(self) -> None:
        assert self.process.stdout is not None
        try:
            while True:
                self._messages.put(read_framed_message(self.process.stdout))
        except EOFError as exc:
            self._messages.put(exc)
        except BaseException as exc:
            self._messages.put(exc)

    def _stderr_loop(self) -> None:
        assert self.process.stderr is not None
        try:
            for raw_line in iter(self.process.stderr.readline, b""):
                self._stderr.append(raw_line.decode("utf-8", errors="replace").rstrip())
        except OSError as exc:
            self._stderr.append(f"<stderr read failed: {exc}>")

    def stderr_tail(self) -> str:
        return "\n".join(self._stderr) or "<no stderr>"

    def close(self) -> None:
        process = getattr(self, "process", None)
        if process is None:
            return
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)

    def send_request(self, command: str, arguments: Mapping[str, Any] | None = None) -> int:
        self._seq += 1
        request = {
            "type": "request",
            "seq": self._seq,
            "command": command,
            "arguments": arguments,
        }
        assert self.process.stdin is not None
        try:
            self.process.stdin.write(frame_message(request))
            self.process.stdin.flush()
        except (BrokenPipeError, OSError) as exc:
            raise ScorecardError(
                f"failed to send {command!r} to perl-dap: {exc}; stderr:\n{self.stderr_tail()}"
            ) from exc
        return self._seq

    def _wait(
        self,
        predicate: Callable[[Mapping[str, Any]], bool],
        description: str,
    ) -> Mapping[str, Any]:
        deadline = time.monotonic() + self.timeout_seconds
        observed: list[str] = []
        for _ in range(len(self._pending)):
            message = self._pending.popleft()
            if predicate(message):
                return message
            observed.append(_message_label(message))
            self._pending.append(message)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ScorecardError(
                    f"timeout waiting for {description}; observed={observed}; "
                    f"stderr:\n{self.stderr_tail()}"
                )
            try:
                item = self._messages.get(timeout=remaining)
            except queue.Empty as exc:
                raise ScorecardError(
                    f"timeout waiting for {description}; observed={observed}; "
                    f"stderr:\n{self.stderr_tail()}"
                ) from exc
            if isinstance(item, BaseException):
                raise ScorecardError(
                    f"DAP reader ended while waiting for {description}: {item}; "
                    f"stderr:\n{self.stderr_tail()}"
                ) from item
            if predicate(item):
                return item
            observed.append(_message_label(item))
            self._pending.append(item)

    def wait_response(self, request_seq: int, command: str) -> Mapping[str, Any] | None:
        response = self._wait(
            lambda message: message.get("type") == "response"
            and message.get("request_seq") == request_seq
            and message.get("command") == command,
            f"response {command!r} for request {request_seq}",
        )
        if response.get("success") is not True:
            raise ScorecardError(
                f"DAP command {command!r} failed: {response.get('message', '<no message>')}"
            )
        body = response.get("body")
        if body is not None and not isinstance(body, dict):
            raise ScorecardError(f"DAP response {command!r} body was not an object or null")
        return body

    def request(
        self, command: str, arguments: Mapping[str, Any] | None = None
    ) -> Mapping[str, Any] | None:
        return self.wait_response(self.send_request(command, arguments), command)

    def wait_event(self, event: str) -> Mapping[str, Any] | None:
        message = self._wait(
            lambda candidate: candidate.get("type") == "event"
            and candidate.get("event") == event,
            f"event {event!r}",
        )
        body = message.get("body")
        if body is not None and not isinstance(body, dict):
            raise ScorecardError(f"DAP event {event!r} body was not an object or null")
        return body

    def initialize(self) -> None:
        self.request(
            "initialize",
            {
                "clientID": "perl-lsp-swarm-scorecard",
                "adapterID": "perl-dap",
                "pathFormat": "path",
                "linesStartAt1": True,
                "columnsStartAt1": True,
                "supportsVariablePaging": True,
                "supportsVariableType": True,
            },
        )
        self.wait_event("initialized")

    def disconnect(self) -> None:
        try:
            self.request("disconnect", {})
        except ScorecardError:
            pass

    def rss_kb(self) -> int:
        status = Path(f"/proc/{self.pid}/status")
        try:
            text = status.read_text(encoding="utf-8")
        except OSError as exc:
            raise ScorecardError(f"cannot read exact adapter RSS from {status}: {exc}") from exc
        for line in text.splitlines():
            if line.startswith("VmRSS:"):
                fields = line.split()
                if len(fields) >= 2 and fields[1].isdigit():
                    return int(fields[1])
        raise ScorecardError(f"exact adapter RSS was absent from {status}")
