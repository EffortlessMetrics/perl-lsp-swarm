"""Shared constants, process helpers, and status validation for DAP packets."""

from __future__ import annotations

import hashlib
import json
import math
import subprocess
from pathlib import Path
from typing import Any, Mapping, Sequence

SCHEMA_VERSION = "dap_scorecard_packet.v2"
RUNTIME_SCHEMA_VERSION = "dap_runtime_scorecard.v2"
REQUIRED_THRESHOLD_PCT = 80
REQUIRED_PROCESS_INVOCATIONS = 11
REQUIRED_BINARY_STATUSES = {
    "variables": "PASS",
    "evaluate": "PASS",
    "deep_pagination": "PASS",
    "memory": "MEASURED",
}
REQUIRED_LAUNCH_FIXTURE_NAMES = ("hello", "loops", "eval", "args", "begin_end")
REQUIRED_ATTACH_NAMES = ("tcp_loopback",) * 5
REQUIRED_FIXTURES = (
    "crates/perl-dap/tests/fixtures/hello.pl",
    "crates/perl-dap/tests/fixtures/loops.pl",
    "crates/perl-dap/tests/fixtures/eval.pl",
    "crates/perl-dap/tests/fixtures/args.pl",
    "crates/perl-dap/tests/fixtures/breakpoints_begin_end.pl",
)
REQUIRED_SOURCE_SUBJECTS = (
    ".github/workflows/dap-scorecard.yml",
    "scripts/ci/dap_scorecard_model.py",
    "scripts/ci/dap_scorecard_packet.py",
    "scripts/ci/dap_scorecard_packet_common.py",
    "scripts/ci/dap_scorecard_packet_git.py",
    "scripts/ci/dap_scorecard_packet_policy.py",
    "scripts/ci/dap_scorecard_probes.py",
    "scripts/ci/dap_scorecard_runtime.py",
    "scripts/ci/dap_scorecard_transport.py",
    "scripts/tests/test_dap_scorecard_packet.py",
    "scripts/tests/test_dap_scorecard_runtime.py",
    "xtask/src/tasks/update_status/dap.rs",
)
GENERATED_STATUS_PATH = "docs/project/status/dap.md"
STATUS_MARKERS = (
    ("<!-- BEGIN: DAP_LAUNCH_SCORECARD -->", "<!-- END: DAP_LAUNCH_SCORECARD -->"),
    ("<!-- BEGIN: DAP_SESSION_SCORECARD -->", "<!-- END: DAP_SESSION_SCORECARD -->"),
)


class PacketError(RuntimeError):
    """A fail-closed scorecard packet validation error."""


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise PacketError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise PacketError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read {path}: {exc}") from exc


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except FileNotFoundError as exc:
        raise PacketError(f"missing evidence subject: {path}") from exc
    except OSError as exc:
        raise PacketError(f"cannot hash {path}: {exc}") from exc
    return digest.hexdigest()


def run(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    timeout: int = 20,
    text: bool = True,
) -> subprocess.CompletedProcess[Any]:
    try:
        return subprocess.run(
            list(argv),
            cwd=cwd,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=text,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PacketError(f"cannot execute {' '.join(argv)!r}: {exc}") from exc


def run_text(argv: Sequence[str], *, cwd: Path | None = None) -> str:
    result = run(argv, cwd=cwd, text=True)
    output = result.stdout.strip()
    if result.returncode != 0:
        raise PacketError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{output or '<no output>'}"
        )
    return output


def run_bytes(argv: Sequence[str], *, cwd: Path | None = None) -> bytes:
    result = run(argv, cwd=cwd, text=False)
    output = bytes(result.stdout)
    if result.returncode != 0:
        rendered = output.decode("utf-8", errors="replace").strip()
        raise PacketError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{rendered or '<no output>'}"
        )
    return output


def as_object(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise PacketError(f"{context} must be a JSON object")
    return value


def as_int(value: Any, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PacketError(f"{context} must be an integer")
    return value


def as_nonnegative_int(value: Any, context: str) -> int:
    result = as_int(value, context)
    if result < 0:
        raise PacketError(f"{context} must be nonnegative")
    return result


def percentile(values: Sequence[int], pct: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    rank = math.ceil((pct / 100.0) * len(ordered))
    return ordered[max(0, min(rank - 1, len(ordered) - 1))]


def relative_path(root: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as exc:
        raise PacketError(f"evidence path escapes repository root: {path}") from exc


def expect_equal(actual: Any, expected: Any, context: str) -> None:
    if actual != expected:
        raise PacketError(f"{context} mismatch: expected {expected!r}, got {actual!r}")


def validate_subject_hash(root: Path, subject: Mapping[str, Any], context: str) -> None:
    path_value = subject.get("path")
    digest_value = subject.get("sha256")
    if not isinstance(path_value, str) or not isinstance(digest_value, str):
        raise PacketError(f"{context} path/hash fields are missing")
    expect_equal(sha256(root / path_value), digest_value, f"{context} SHA-256")


def validate_generated_status(status_path: Path) -> None:
    try:
        text = status_path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise PacketError(f"missing generated DAP status: {status_path}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read generated DAP status {status_path}: {exc}") from exc
    for begin, end in STATUS_MARKERS:
        start = text.find(begin)
        stop = text.find(end)
        if start < 0 or stop < 0 or stop <= start:
            raise PacketError(f"generated status is missing marker pair {begin!r} / {end!r}")
        block = text[start : stop + len(end)]
        if "receipt missing" in block:
            raise PacketError("generated DAP status still reports a missing receipt")
        if "| SKIP |" in block:
            raise PacketError("generated DAP status contains SKIP in a required scorecard block")
        if "| FAIL |" in block:
            raise PacketError("generated DAP status contains FAIL in a required scorecard block")
