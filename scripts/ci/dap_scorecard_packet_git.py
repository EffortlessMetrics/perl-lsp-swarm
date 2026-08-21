"""Git-object and post-run tree binding for the DAP scorecard packet."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from dap_scorecard_packet_common import (
    GENERATED_STATUS_PATH,
    PacketError,
    as_object,
    expect_equal,
    run,
    run_bytes,
    run_text,
)


def verify_head(root: Path, repository_sha: str) -> None:
    actual = run_text(["git", "rev-parse", "HEAD"], cwd=root)
    if actual != repository_sha:
        raise PacketError(f"checked-out HEAD differs from candidate: {actual} != {repository_sha}")
    resolved = run_text(["git", "rev-parse", f"{repository_sha}^{{commit}}"], cwd=root)
    if resolved != repository_sha:
        raise PacketError(f"candidate SHA does not resolve exactly: {resolved} != {repository_sha}")


def repository_status(root: Path) -> list[str]:
    result = run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=root,
        text=True,
    )
    output = result.stdout.rstrip("\n")
    if result.returncode != 0:
        raise PacketError(
            f"git status failed with exit {result.returncode}: {output or '<no output>'}"
        )
    return [] if not output else output.splitlines()


def assert_repository_state(root: Path) -> list[str]:
    lines = repository_status(root)
    for line in lines:
        if len(line) < 4:
            raise PacketError(f"malformed git status line: {line!r}")
        status = line[:2]
        path = line[3:]
        if path != GENERATED_STATUS_PATH or status not in {" M", "M "}:
            raise PacketError(
                "candidate tree changed outside the generated DAP status: "
                f"status={status!r}, path={path!r}"
            )
        if status == "M ":
            raise PacketError("generated DAP status must not be staged during packet construction")
    return lines


def tracked_record(root: Path, relative: str, repository_sha: str) -> dict[str, str]:
    path = root / relative
    blob_sha = run_text(["git", "rev-parse", f"{repository_sha}:{relative}"], cwd=root)
    expected = run_bytes(["git", "show", f"{repository_sha}:{relative}"], cwd=root)
    try:
        actual = path.read_bytes()
    except OSError as exc:
        raise PacketError(f"cannot read tracked evidence subject {relative}: {exc}") from exc
    if actual != expected:
        raise PacketError(f"tracked evidence subject differs from candidate Git object: {relative}")
    return {
        "path": relative,
        "git_blob_sha1": blob_sha,
        "sha256": hashlib.sha256(actual).hexdigest(),
    }


def tracked_records(
    root: Path,
    paths: Iterable[str],
    repository_sha: str,
    *,
    expected: Sequence[str],
    context: str,
) -> list[dict[str, str]]:
    observed = list(paths)
    if len(observed) != len(set(observed)):
        raise PacketError(f"duplicate {context} identity")
    if set(observed) != set(expected):
        raise PacketError(
            f"{context} set mismatch; missing={sorted(set(expected) - set(observed))}, "
            f"extra={sorted(set(observed) - set(expected))}"
        )
    return sorted(
        (tracked_record(root, relative, repository_sha) for relative in observed),
        key=lambda record: record["path"],
    )


def validate_tracked_packet_records(
    root: Path,
    records: Any,
    repository_sha: str,
    expected: Sequence[str],
    context: str,
) -> None:
    if not isinstance(records, list):
        raise PacketError(f"packet.{context} must be an array")
    observed: dict[str, Mapping[str, Any]] = {}
    for index, raw_record in enumerate(records):
        record = as_object(raw_record, f"packet.{context}[{index}]")
        path = record.get("path")
        if not isinstance(path, str) or not path:
            raise PacketError(f"packet.{context}[{index}].path is missing")
        if path in observed:
            raise PacketError(f"duplicate {context} identity: {path}")
        observed[path] = record
    expect_equal(set(observed), set(expected), f"{context} identity set")
    for path, record in observed.items():
        expect_equal(record, tracked_record(root, path, repository_sha), f"tracked {context} {path}")
