"""Common identity and serialization helpers for Zed asset receipts."""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import os
import platform
import re
from pathlib import Path, PurePosixPath
from typing import Any

CONTRACT_SCHEMA = "zed_perllsp_managed_downloads.v1"
RECEIPT_SCHEMA = "zed_managed_asset_receipt.v1"


class ReceiptError(RuntimeError):
    """A bounded, user-actionable receipt failure."""


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ReceiptError(f"{path} must contain a JSON object")
    return value


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def now_utc() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def normalize_architecture(machine: str) -> str:
    normalized = machine.strip().lower()
    if normalized in {"x86_64", "amd64"}:
        return "x86_64"
    if normalized in {"aarch64", "arm64"}:
        return "aarch64"
    return normalized or "unknown"


def verifier_identity() -> dict[str, str]:
    return {
        "os": platform.system().lower() or "unknown",
        "version": platform.version(),
        "architecture": normalize_architecture(platform.machine()),
        "python": platform.python_version(),
    }


def parse_digest(value: Any, context: str) -> str:
    if not isinstance(value, str) or re.fullmatch(r"sha256:[0-9a-f]{64}", value) is None:
        raise ReceiptError(f"{context} must be a sha256:<64 lowercase hex> digest")
    return value


def validate_relative_member(name: str) -> PurePosixPath:
    normalized = name.replace("\\", "/")
    if re.match(r"^[A-Za-z]:", normalized):
        raise ReceiptError(f"archive member uses a drive prefix: {name!r}")
    path = PurePosixPath(normalized)
    if (
        not path.parts
        or path.is_absolute()
        or any(part in {"", ".", ".."} for part in path.parts)
    ):
        raise ReceiptError(f"unsafe archive member path: {name!r}")
    return path


def expected_host(row: dict[str, Any], verifier: dict[str, str]) -> bool:
    os_name = str(row.get("os", "")).lower()
    verifier_os = verifier["os"]
    if verifier_os == "darwin":
        verifier_os = "macos"
    return os_name == verifier_os and row.get("architecture") == verifier["architecture"]


def write_receipt(path: Path, receipt: dict[str, Any], exit_code: int) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(encoded, encoding="utf-8")
    os.replace(temporary, path)
    return exit_code
