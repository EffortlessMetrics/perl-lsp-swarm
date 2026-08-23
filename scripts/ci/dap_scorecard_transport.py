"""Bounded DAP framing and exact-binary stdio client for the scorecard."""

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

MAX_FRAME_HEADER_BYTES = 8 * 1024
MAX_FRAME_BODY_BYTES = 1024 * 1024
MESSAGE_QUEUE_CAPACITY = 64
MAX_RETAINED_MESSAGES = 256
MAX_RETAINED_MESSAGE_BYTES = 4 * 1024 * 1024
MAX_STDERR_TAIL_BYTES = 64 * 1024
STDERR_READ_CHUNK_BYTES = 4 * 1024
OBSERVED_LABEL_TAIL = 32
READER_POLL_SECONDS = 0.05

BufferedMessage = tuple[Mapping[str, Any], int]
QueueItem = BufferedMessage | BaseException


class InvocationCounter:
    """Thread-safe count of exact DAP processes successfully spawned."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._count = 0

    def record_spawn(self) -> None:
        with self._lock:
            self._count += 1

    @property
    def count(self) -> int:
        with self._lock:
            return self._count


def frame_message(message: Mapping[str, Any]) -> bytes:
    body = json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    if len(body) > MAX_FRAME_BODY_BYTES:
        raise ScorecardError(
            f"DAP message body exceeds {MAX_FRAME_BODY_BYTES} byte limit: {len(body)} bytes"
        )
    return f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body


def _read_exact(stream: BinaryIO, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise ScorecardError(
                f"unexpected EOF while reading DAP frame body: {remaining} bytes missing"
            )
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_framed_message(stream: BinaryIO) -> Mapping[str, Any]:
    header = bytearray()
    while not header.endswith(b"\r\n\r\n"):
        byte = stream.read(1)
        if not byte:
            raise EOFError("DAP stream closed")
        header.extend(byte)
        if len(header) > MAX_FRAME_HEADER_BYTES:
            raise ScorecardError(
                f"DAP frame header exceeds {MAX_FRAME_HEADER_BYTES} byte limit"
            )

    content_lengths: list[str] = []
    for raw_line in bytes(header[:-4]).split(b"\r\n"):
        name, separator, value = raw_line.partition(b":")
        if separator and name.strip().lower() == b"content-length":
            content_lengths.append(value.decode("ascii", errors="strict").strip())
    if len(content_lengths) != 1:
        raise ScorecardError(
            "DAP frame must contain exactly one Content-Length header, "
            f"got {len(content_lengths)}"
        )
    try:
        length = int(content_lengths[0])
    except ValueError as exc:
        raise ScorecardError(f"invalid DAP Content-Length: {content_lengths[0]!r}") from exc
    if length < 0:
        raise ScorecardError(f"negative DAP Content-Length: {length}")
    if length > MAX_FRAME_BODY_BYTES:
        raise ScorecardError(
            f"DAP frame body exceeds {MAX_FRAME_BODY_BYTES} byte limit: {length} bytes"
        )

    body = _read_exact(stream, length)
    try:
        message = json.loads(body)
    except json.JSONDecodeError as exc:
        raise ScorecardError(f"malformed DAP JSON body: {exc}") from exc
    if not isinstance(message, dict):
        raise ScorecardError("DAP frame body must be a JSON object")
    return message


def _message_size(message: Mapping[str, Any]) -> int:
    return len(json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode("utf-8"))


class DapProcess:
    """Drive one exact ``perl-dap --stdio`` candidate process."""

    def __init__(
        self,
        binary: Path,
        timeout_seconds: float,
        invocations: InvocationCounter | None = None,
    ) -> None:
        self.timeout_seconds = timeout_seconds
        self.process = subprocess.Popen(
            [str(binary), "--stdio", "--log-level", "error"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if invocations is not None:
            invocations.record_spawn()
        if self.process.stdin is None or self.process.stdout is None or self.process.stderr is None:
            self.close()
            raise ScorecardError("exact perl-dap process did not expose stdio pipes")

        self._messages: queue.Queue[QueueItem] = queue.Queue(maxsize=MESSAGE_QUEUE_CAPACITY)
        self._pending: deque[BufferedMessage] = deque()
        self._stderr = bytearray()
        self._stderr_lock = threading.Lock()
        self._next_seq = 1
        self._retained_lock = threading.Lock()
        self._retained_messages = 0
        self._retained_bytes = 0
        self._reader_error: BaseException | None = None
        self._reader = threading.Thread(target=self._reader_loop, daemon=True)
        self._stderr_reader = threading.Thread(target=self._stderr_loop, daemon=True)
        self._reader.start()
        self._stderr_reader.start()

    def __enter__(self) -> DapProcess:
        return self

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        self.close()

    def _record_reader_error(self, error: BaseException, *, terminate: bool) -> None:
        with self._retained_lock:
            if self._reader_error is None:
                self._reader_error = error
        if terminate and self.process.poll() is None:
            try:
                self.process.kill()
            except OSError:
                pass
        try:
            self._messages.put_nowait(error)
        except queue.Full:
            # The consumer polls _reader_error whenever the bounded queue empties.
            pass

    def _retain_message(self, size: int) -> None:
        with self._retained_lock:
            next_messages = self._retained_messages + 1
            next_bytes = self._retained_bytes + size
            if next_messages > MAX_RETAINED_MESSAGES:
                raise ScorecardError(
                    "DAP retained-message envelope exceeded: "
                    f"{next_messages} messages > {MAX_RETAINED_MESSAGES}"
                )
            if next_bytes > MAX_RETAINED_MESSAGE_BYTES:
                raise ScorecardError(
                    "DAP retained-message byte envelope exceeded: "
                    f"{next_bytes} bytes > {MAX_RETAINED_MESSAGE_BYTES}"
                )
            self._retained_messages = next_messages
            self._retained_bytes = next_bytes

    def _release_message(self, size: int) -> None:
        with self._retained_lock:
            self._retained_messages = max(0, self._retained_messages - 1)
            self._retained_bytes = max(0, self._retained_bytes - size)

    def _reader_loop(self) -> None:
        assert self.process.stdout is not None
        try:
            while True:
                message = read_framed_message(self.process.stdout)
                size = _message_size(message)
                self._retain_message(size)
                try:
                    self._messages.put_nowait((message, size))
                except queue.Full as exc:
                    self._release_message(size)
                    raise ScorecardError(
                        "DAP inbound queue exceeded its bounded capacity: "
                        f"{MESSAGE_QUEUE_CAPACITY} messages"
                    ) from exc
        except EOFError as exc:
            self._record_reader_error(exc, terminate=False)
        except BaseException as exc:  # propagate framing/reader failures to the driver
            self._record_reader_error(exc, terminate=True)

    def _stderr_loop(self) -> None:
        assert self.process.stderr is not None
        read_chunk = getattr(self.process.stderr, "read1", self.process.stderr.read)
        while raw := read_chunk(STDERR_READ_CHUNK_BYTES):
            with self._stderr_lock:
                self._stderr.extend(raw)
                overflow = len(self._stderr) - MAX_STDERR_TAIL_BYTES
                if overflow > 0:
                    del self._stderr[:overflow]

    def stderr_tail(self) -> str:
        with self._stderr_lock:
            raw = bytes(self._stderr)
        return raw.decode("utf-8", errors="replace").rstrip()

    @staticmethod
    def _discard_if_unmatched(message: Mapping[str, Any]) -> bool:
        # The adapter can emit one output event per debugger line. The scorecard
        # never waits on those events, so retaining an array dump would turn
        # legitimate debuggee output into client-side memory growth. Other
        # events and all responses remain available for later request/event waits.
        return message.get("type") == "event" and message.get("event") == "output"

    @staticmethod
    def _label(message: Mapping[str, Any]) -> str:
        message_type = str(message.get("type", "<missing-type>"))
        if message_type == "response":
            return f"response:{message.get('command', '<missing-command>')}"
        if message_type == "event":
            return f"event:{message.get('event', '<missing-event>')}"
        return message_type

    def _wait(
        self,
        predicate: Callable[[Mapping[str, Any]], bool],
        description: str,
    ) -> Mapping[str, Any]:
        deadline = time.monotonic() + self.timeout_seconds
        observed: deque[str] = deque(maxlen=OBSERVED_LABEL_TAIL)
        observed_count = 0

        pending_count = len(self._pending)
        for _ in range(pending_count):
            message, size = self._pending.popleft()
            if predicate(message):
                self._release_message(size)
                return message
            observed.append(self._label(message))
            observed_count += 1
            if self._discard_if_unmatched(message):
                self._release_message(size)
            else:
                self._pending.append((message, size))

        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                stderr = self.stderr_tail()
                raise ScorecardError(
                    f"timed out waiting for {description}; observed_tail={list(observed)!r}; "
                    f"observed_total={observed_count}; stderr={stderr or '<empty>'}"
                )
            try:
                item = self._messages.get(timeout=min(remaining, READER_POLL_SECONDS))
            except queue.Empty:
                with self._retained_lock:
                    reader_error = self._reader_error
                if reader_error is not None:
                    raise ScorecardError(
                        f"DAP reader failed while waiting for {description}: {reader_error}; "
                        f"stderr={self.stderr_tail() or '<empty>'}"
                    ) from reader_error
                if self.process.poll() is not None:
                    raise ScorecardError(
                        f"perl-dap exited with {self.process.returncode} while waiting for "
                        f"{description}; stderr={self.stderr_tail() or '<empty>'}"
                    )
                continue
            if isinstance(item, BaseException):
                raise ScorecardError(
                    f"DAP reader failed while waiting for {description}: {item}; "
                    f"stderr={self.stderr_tail() or '<empty>'}"
                ) from item
            message, size = item
            if predicate(message):
                self._release_message(size)
                return message
            observed.append(self._label(message))
            observed_count += 1
            if self._discard_if_unmatched(message):
                self._release_message(size)
            else:
                self._pending.append((message, size))

    def send_request(self, command: str, arguments: Mapping[str, Any] | None = None) -> int:
        seq = self._next_seq
        self._next_seq += 1
        message: dict[str, Any] = {"seq": seq, "type": "request", "command": command}
        if arguments is not None:
            message["arguments"] = dict(arguments)
        payload = frame_message(message)
        assert self.process.stdin is not None
        try:
            self.process.stdin.write(payload)
            self.process.stdin.flush()
        except OSError as exc:
            raise ScorecardError(
                f"cannot write DAP request {command!r}: {exc}; "
                f"stderr={self.stderr_tail() or '<empty>'}"
            ) from exc
        return seq

    def request(
        self,
        command: str,
        arguments: Mapping[str, Any] | None = None,
    ) -> Mapping[str, Any]:
        request_seq = self.send_request(command, arguments)
        response = self._wait(
            lambda message: message.get("type") == "response"
            and message.get("request_seq") == request_seq,
            f"response to {command!r} request_seq={request_seq}",
        )
        if response.get("command") != command:
            raise ScorecardError(
                f"response command mismatch for request_seq={request_seq}: "
                f"expected {command!r}, got {response.get('command')!r}"
            )
        if response.get("success") is not True:
            raise ScorecardError(
                f"DAP command {command!r} failed: {response.get('message', '<no message>')}"
            )
        body = response.get("body")
        if body is None:
            return {}
        if not isinstance(body, dict):
            raise ScorecardError(f"DAP command {command!r} returned a non-object body")
        return body

    def wait_event(self, name: str) -> Mapping[str, Any]:
        event = self._wait(
            lambda message: message.get("type") == "event" and message.get("event") == name,
            f"event {name!r}",
        )
        body = event.get("body")
        if body is None:
            return {}
        if not isinstance(body, dict):
            raise ScorecardError(f"DAP event {name!r} returned a non-object body")
        return body

    def initialize(self) -> None:
        self.request(
            "initialize",
            {
                "clientID": "perl-lsp-dap-scorecard",
                "adapterID": "perl-dap",
                "pathFormat": "path",
                "linesStartAt1": True,
                "columnsStartAt1": True,
            },
        )
        self.wait_event("initialized")

    def rss_kb(self) -> int:
        status_path = Path(f"/proc/{self.process.pid}/status")
        try:
            status = status_path.read_text(encoding="utf-8")
        except OSError as exc:
            raise ScorecardError(
                f"cannot read exact perl-dap process memory from {status_path}: {exc}"
            ) from exc
        for line in status.splitlines():
            if line.startswith("VmRSS:"):
                parts = line.split()
                if len(parts) >= 2:
                    try:
                        return int(parts[1])
                    except ValueError as exc:
                        raise ScorecardError(f"malformed VmRSS line: {line!r}") from exc
        raise ScorecardError(f"VmRSS was absent from {status_path}")

    def disconnect(self) -> None:
        self.request("disconnect", {})
        self.wait_event("terminated")
        assert self.process.stdin is not None
        try:
            self.process.stdin.close()
        except OSError as exc:
            raise ScorecardError(f"failed to close perl-dap stdin after disconnect: {exc}") from exc
        try:
            return_code = self.process.wait(timeout=self.timeout_seconds)
        except subprocess.TimeoutExpired as exc:
            raise ScorecardError(
                "perl-dap did not exit after successful disconnect and terminated event"
            ) from exc
        if return_code != 0:
            raise ScorecardError(
                f"perl-dap exited with {return_code} after disconnect; "
                f"stderr={self.stderr_tail() or '<empty>'}"
            )

    def close(self) -> None:
        stdin = self.process.stdin
        if stdin is not None and not stdin.closed:
            try:
                stdin.close()
            except OSError:
                pass
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)