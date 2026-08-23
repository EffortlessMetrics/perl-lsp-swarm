"""Identity, hashing, redaction, and local-command helpers for Zed host receipts."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
from pathlib import Path
from typing import Any, Iterable


class HostReceiptError(RuntimeError):
    """A bounded exact-source host preparation or execution failure."""


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise HostReceiptError(f"{path} must contain a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(value, indent=2, sort_keys=True) + "\n"
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(encoded, encoding="utf-8")
    os.replace(temporary, path)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def sha256_tree(root: Path, ignored: Iterable[str] = (".git", "target")) -> str:
    ignored_names = set(ignored)
    digest = hashlib.sha256()
    files: list[Path] = []
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if any(part in ignored_names for part in relative.parts):
            continue
        if path.is_symlink():
            raise HostReceiptError(f"tree hashing rejects symlink: {relative}")
        if path.is_file():
            files.append(path)
    for path in sorted(files, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        data = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return "sha256:" + digest.hexdigest()


def run_checked(command: list[str], cwd: Path | None = None) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
    )
    if completed.returncode != 0:
        raise HostReceiptError(
            f"command failed ({completed.returncode}): {command!r}\n{completed.stderr.strip()}"
        )
    return (completed.stdout or completed.stderr).strip()


def require_clean_git_checkout(
    checkout: Path,
    expected_head: str,
    expected_base: str,
) -> None:
    actual_head = run_checked(["git", "rev-parse", "HEAD"], cwd=checkout)
    if actual_head != expected_head:
        raise HostReceiptError(
            f"extension checkout head {actual_head} does not match {expected_head}"
        )
    status = run_checked(["git", "status", "--porcelain"], cwd=checkout)
    if status:
        raise HostReceiptError("extension checkout must be clean")
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", expected_base, expected_head],
        cwd=checkout,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if ancestor.returncode != 0:
        raise HostReceiptError(
            f"extension base {expected_base} is not an ancestor of {expected_head}"
        )


def canonical_file(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve(strict=True)
    if not resolved.is_file():
        raise HostReceiptError(f"{label} is not a regular file: {resolved}")
    return resolved


def canonical_dir(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve(strict=True)
    if not resolved.is_dir():
        raise HostReceiptError(f"{label} is not a directory: {resolved}")
    return resolved


def normalize_architecture(machine: str) -> str:
    normalized = machine.strip().lower()
    if normalized in {"x86_64", "amd64"}:
        return "x86_64"
    if normalized in {"aarch64", "arm64"}:
        return "aarch64"
    return normalized or "unknown"


def platform_identity() -> dict[str, str]:
    system = platform.system().strip().lower()
    if system == "darwin":
        system = "macos"
    return {
        "os": system or "unknown",
        "version": platform.version(),
        "architecture": normalize_architecture(platform.machine()),
    }


def redactions(manifest: dict[str, Any], run_dir: Path) -> list[tuple[str, str]]:
    replacements = [
        (str(run_dir), "<run-dir>"),
        (manifest["zed"]["cli"], "<zed-cli>"),
        (manifest["zed"]["app"], "<zed-app>"),
        (manifest["extension"]["directory"], "<extension-dir>"),
        (manifest["perllsp"]["command"], "<perllsp>"),
        (manifest["workspace"]["directory"], "<workspace>"),
        (manifest["profile"]["directory"], "<profile>"),
    ]
    return sorted(replacements, key=lambda item: len(item[0]), reverse=True)


def redact_text(text: str, replacements: list[tuple[str, str]]) -> str:
    redacted = text
    for source, replacement in replacements:
        redacted = redacted.replace(source, replacement)
    return redacted


def copy_redacted_text(
    source: Path,
    destination: Path,
    replacements: list[tuple[str, str]],
) -> str:
    text = source.read_text(encoding="utf-8", errors="replace")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(redact_text(text, replacements), encoding="utf-8")
    return sha256_file(destination)


def artifact_reference(path: Path, run_dir: Path) -> str:
    return f"{path.relative_to(run_dir).as_posix()}#{sha256_file(path)}"


def verify_artifact_reference(
    path: Path,
    run_dir: Path,
    reference: object,
    label: str,
) -> None:
    if not isinstance(reference, str) or artifact_reference(path, run_dir) != reference:
        raise HostReceiptError(f"{label} artifact binding does not match its bytes")
