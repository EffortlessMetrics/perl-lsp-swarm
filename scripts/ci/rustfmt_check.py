#!/usr/bin/env python3
"""Produce candidate-bound rustfmt evidence without compiling repository Rust code.

The instrument uses ``cargo metadata --no-deps`` to discover the exact workspace
members and then runs ``cargo fmt --manifest-path ... -- --check`` for each
member. Child output is captured through bounded temporary files, so an
untrusted or broken formatter cannot allocate unbounded memory in the wrapper.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import signal
import stat
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

SCHEMA_VERSION = "rustfmt_check.v1"
RECEIPT_KIND = "rustfmt_check"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
DIFF_RE = re.compile(r"^Diff in (.+):(\d+):\s*$", re.MULTILINE)
RUSTFMT_ERROR_RE = re.compile(
    r"^(?:error(?:\[[^\]]+\])?:|Error writing files:)",
    re.IGNORECASE | re.MULTILINE,
)
DEFAULT_TIMEOUT_SECONDS = 300.0
DEFAULT_MAX_OUTPUT_BYTES = 256 * 1024
DEFAULT_MAX_METADATA_BYTES = 16 * 1024 * 1024
DEFAULT_MAX_MANIFESTS = 512
DEFAULT_MAX_TARGETS = 4096
DEFAULT_MAX_FINDINGS = 1024
DEFAULT_RECEIPT = Path("target/receipts/rustfmt/rustfmt-check.json")


@dataclass(frozen=True)
class ProcessResult:
    command: tuple[str, ...]
    exit_code: int | None
    signal: int | None
    timed_out: bool
    stdout: str
    stderr: str
    stdout_truncated: bool
    stderr_truncated: bool
    spawn_error: str | None = None

    @property
    def output(self) -> str:
        parts = [part for part in (self.stdout, self.stderr) if part]
        return "\n".join(parts)


class EvidenceError(RuntimeError):
    """The instrument cannot establish a complete candidate-bound result."""


class InstrumentError(RuntimeError):
    """A required subprocess or tool failed before producing a formatter verdict."""

    def __init__(self, message: str, record: dict[str, object] | None = None) -> None:
        super().__init__(message)
        self.record = record


def bounded_text(value: object, limit: int = 2048) -> str:
    text = re.sub(r"[\x00-\x1f\x7f]+", " ", str(value)).strip()
    return (text or "unknown error")[:limit]


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def file_digest(path: Path, *, required: bool = True) -> str | None:
    try:
        metadata = path.lstat()
    except OSError as error:
        if required:
            raise EvidenceError(f"required input is unavailable: {path}: {error}") from error
        return None
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise EvidenceError(f"required input is not a regular file: {path}")
    return "sha256:" + sha256_bytes(path.read_bytes())


def _read_bounded(file_object: Any, limit: int) -> tuple[str, bool]:
    file_object.flush()
    size = file_object.seek(0, os.SEEK_END)
    file_object.seek(0)
    payload = file_object.read(limit + 1)
    truncated = size > limit
    if len(payload) > limit:
        payload = payload[:limit]
        truncated = True
    return payload.decode("utf-8", errors="replace"), truncated


def _terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        if os.name == "nt":
            subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=10,
            )
        else:
            os.killpg(process.pid, signal.SIGKILL)
    except (OSError, subprocess.SubprocessError):
        process.kill()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def run_bounded(
    command: Sequence[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
    max_output_bytes: int,
) -> ProcessResult:
    if timeout_seconds <= 0:
        return ProcessResult(
            command=tuple(command),
            exit_code=None,
            signal=None,
            timed_out=True,
            stdout="",
            stderr="total timeout exhausted before process start",
            stdout_truncated=False,
            stderr_truncated=False,
        )

    with tempfile.TemporaryFile() as stdout_file, tempfile.TemporaryFile() as stderr_file:
        try:
            process = subprocess.Popen(
                list(command),
                cwd=cwd,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=stdout_file,
                stderr=stderr_file,
                start_new_session=os.name != "nt",
            )
        except OSError as error:
            return ProcessResult(
                command=tuple(command),
                exit_code=None,
                signal=None,
                timed_out=False,
                stdout="",
                stderr="",
                stdout_truncated=False,
                stderr_truncated=False,
                spawn_error=bounded_text(error),
            )

        timed_out = False
        output_exceeded = False
        deadline = time.monotonic() + timeout_seconds
        while process.poll() is None:
            if (
                os.fstat(stdout_file.fileno()).st_size > max_output_bytes
                or os.fstat(stderr_file.fileno()).st_size > max_output_bytes
            ):
                output_exceeded = True
                _terminate_process_tree(process)
                break
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                _terminate_process_tree(process)
                break
            time.sleep(min(0.02, remaining))

        stdout, stdout_truncated = _read_bounded(stdout_file, max_output_bytes)
        stderr, stderr_truncated = _read_bounded(stderr_file, max_output_bytes)
        if output_exceeded:
            stdout_truncated = (
                stdout_truncated
                or os.fstat(stdout_file.fileno()).st_size > max_output_bytes
            )
            stderr_truncated = (
                stderr_truncated
                or os.fstat(stderr_file.fileno()).st_size > max_output_bytes
            )
        return_code = process.returncode
        return ProcessResult(
            command=tuple(command),
            exit_code=return_code if return_code is not None and return_code >= 0 else None,
            signal=-return_code if return_code is not None and return_code < 0 else None,
            timed_out=timed_out,
            stdout=stdout,
            stderr=stderr,
            stdout_truncated=stdout_truncated,
            stderr_truncated=stderr_truncated,
        )


def command_record(result: ProcessResult) -> dict[str, object]:
    return {
        "command": list(result.command),
        "exit_code": result.exit_code,
        "signal": result.signal,
        "timed_out": result.timed_out,
        "spawn_error": result.spawn_error,
        "stdout_truncated": result.stdout_truncated,
        "stderr_truncated": result.stderr_truncated,
    }


def require_success(result: ProcessResult, label: str) -> None:
    record = command_record(result)
    if result.spawn_error:
        raise InstrumentError(f"{label} could not start: {result.spawn_error}", record)
    if result.timed_out:
        raise InstrumentError(f"{label} timed out", record)
    if result.signal is not None:
        raise InstrumentError(f"{label} terminated by signal {result.signal}", record)
    if result.stdout_truncated or result.stderr_truncated:
        raise InstrumentError(f"{label} output exceeded its configured bound", record)
    if result.exit_code != 0:
        detail = bounded_text(result.output)
        raise InstrumentError(f"{label} exited {result.exit_code}: {detail}", record)


def validate_sha(value: str, label: str) -> str:
    normalized = value.strip().lower()
    if not SHA_RE.fullmatch(normalized):
        raise EvidenceError(f"{label} must be a full lowercase 40-character commit SHA")
    return normalized


def resolve_subject(
    root: Path,
    env: dict[str, str],
    runner: "Runner",
    candidate_sha: str | None,
    candidate_tree_sha: str | None,
) -> tuple[str, str, list[dict[str, object]]]:
    if bool(candidate_sha) != bool(candidate_tree_sha):
        raise EvidenceError("--candidate-sha and --candidate-tree-sha must be provided together")
    records: list[dict[str, object]] = []
    head = runner.run(["git", "rev-parse", "HEAD^{commit}"], cwd=root, env=env)
    records.append(command_record(head))
    require_success(head, "git candidate resolution")
    tree = runner.run(["git", "rev-parse", "HEAD^{tree}"], cwd=root, env=env)
    records.append(command_record(tree))
    require_success(tree, "git tree resolution")
    status = runner.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=root,
        env=env,
    )
    records.append(command_record(status))
    require_success(status, "git worktree status")

    resolved_sha = validate_sha(head.stdout.strip(), "resolved candidate SHA")
    resolved_tree_sha = validate_sha(tree.stdout.strip(), "resolved candidate tree SHA")
    if status.stdout:
        raise EvidenceError(
            "candidate worktree is not clean: " + bounded_text(status.stdout)
        )
    if candidate_sha:
        expected_sha = validate_sha(candidate_sha, "candidate SHA")
        expected_tree_sha = validate_sha(candidate_tree_sha or "", "candidate tree SHA")
        if expected_sha != resolved_sha or expected_tree_sha != resolved_tree_sha:
            raise EvidenceError(
                "supplied candidate identity does not match the checked-out commit and tree"
            )
    return resolved_sha, resolved_tree_sha, records


@dataclass
class Runner:
    deadline: float
    max_output_bytes: int

    def remaining(self) -> float:
        return max(0.0, self.deadline - time.monotonic())

    def run(
        self,
        command: Sequence[str],
        *,
        cwd: Path,
        env: dict[str, str],
        max_output_bytes: int | None = None,
    ) -> ProcessResult:
        return run_bounded(
            command,
            cwd=cwd,
            env=env,
            timeout_seconds=self.remaining(),
            max_output_bytes=max_output_bytes or self.max_output_bytes,
        )


def resolve_inside(root: Path, path_value: str, label: str) -> Path:
    candidate = Path(path_value)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        candidate_metadata = candidate.lstat()
        if stat.S_ISLNK(candidate_metadata.st_mode):
            raise EvidenceError(f"{label} must not be a symbolic link: {candidate}")
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise EvidenceError(f"{label} is unavailable: {candidate}: {error}") from error
    try:
        without_windows_extended_prefix(resolved).relative_to(
            without_windows_extended_prefix(root)
        )
    except ValueError as error:
        raise EvidenceError(f"{label} escapes the repository: {resolved}") from error
    metadata = resolved.lstat()
    if not stat.S_ISREG(metadata.st_mode):
        raise EvidenceError(f"{label} is not a regular file: {resolved}")
    return resolved


def relative_posix(root: Path, path: Path) -> str:
    return without_windows_extended_prefix(path).relative_to(
        without_windows_extended_prefix(root)
    ).as_posix()


def without_windows_extended_prefix(path: Path) -> Path:
    text = str(path)
    if os.name != "nt":
        return path
    if text.startswith("\\\\?\\UNC\\"):
        return Path("\\\\" + text[8:])
    if text.startswith("\\\\?\\"):
        return Path(text[4:])
    return path


def parse_metadata(
    root: Path,
    payload: str,
    *,
    max_manifests: int = DEFAULT_MAX_MANIFESTS,
    max_targets: int = DEFAULT_MAX_TARGETS,
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    try:
        metadata = json.loads(payload)
    except json.JSONDecodeError as error:
        raise EvidenceError(f"cargo metadata returned malformed JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise EvidenceError("cargo metadata root must be an object")
    packages = metadata.get("packages")
    workspace_members = metadata.get("workspace_members")
    workspace_root = metadata.get("workspace_root")
    if not isinstance(packages, list) or not isinstance(workspace_members, list):
        raise EvidenceError("cargo metadata omitted packages or workspace_members")
    if not isinstance(workspace_root, str):
        raise EvidenceError("cargo metadata omitted workspace_root")
    try:
        observed_root = Path(workspace_root).resolve(strict=True)
    except OSError as error:
        raise EvidenceError(f"cargo metadata workspace_root is unavailable: {error}") from error
    if observed_root != root:
        raise EvidenceError(
            f"cargo metadata workspace_root {observed_root} does not match requested root {root}"
        )

    if not all(isinstance(member, str) and member for member in workspace_members):
        raise EvidenceError("cargo metadata workspace_members must contain non-empty strings")
    member_ids = set(workspace_members)
    if len(member_ids) != len(workspace_members):
        raise EvidenceError("cargo metadata contains duplicate workspace member identities")
    manifests: list[dict[str, object]] = []
    targets: list[dict[str, object]] = []
    seen_manifests: set[Path] = set()
    seen_targets: set[tuple[str, str, tuple[str, ...]]] = set()
    seen_member_ids: set[object] = set()

    for package in packages:
        if not isinstance(package, dict):
            raise EvidenceError("cargo metadata contains a malformed package record")
        package_id = package.get("id")
        if not isinstance(package_id, str) or not package_id:
            raise EvidenceError("cargo metadata package id must be a non-empty string")
        if package_id not in member_ids:
            continue
        if package_id in seen_member_ids:
            raise EvidenceError(f"cargo metadata duplicates workspace package record: {package_id}")
        seen_member_ids.add(package_id)
        name = package.get("name")
        manifest_path = package.get("manifest_path")
        if not isinstance(name, str) or not name or not isinstance(manifest_path, str):
            raise EvidenceError("workspace package has invalid name or manifest_path")
        manifest = resolve_inside(root, manifest_path, f"manifest for {name}")
        if manifest in seen_manifests:
            raise EvidenceError(f"duplicate workspace manifest: {manifest}")
        seen_manifests.add(manifest)
        manifest_relative = relative_posix(root, manifest)
        manifests.append({"package": name, "manifest": manifest_relative})

        package_targets = package.get("targets")
        if not isinstance(package_targets, list):
            raise EvidenceError(f"workspace package {name} omitted targets")
        for target in package_targets:
            if not isinstance(target, dict):
                raise EvidenceError(f"workspace package {name} contains a malformed target")
            target_name = target.get("name")
            kinds = target.get("kind")
            source_path = target.get("src_path")
            if (
                not isinstance(target_name, str)
                or not isinstance(kinds, list)
                or not all(isinstance(kind, str) for kind in kinds)
                or not isinstance(source_path, str)
            ):
                raise EvidenceError(f"workspace package {name} contains an incomplete target")
            source = resolve_inside(root, source_path, f"target {name}::{target_name}")
            key = (name, target_name, tuple(sorted(kinds)))
            if key in seen_targets:
                raise EvidenceError(f"duplicate workspace target identity: {key}")
            seen_targets.add(key)
            targets.append(
                {
                    "package": name,
                    "name": target_name,
                    "kind": sorted(kinds),
                    "source": relative_posix(root, source),
                    "manifest": manifest_relative,
                }
            )

    missing_members = member_ids - seen_member_ids
    if missing_members:
        raise EvidenceError(
            "cargo metadata omitted workspace package records: "
            + ", ".join(sorted(str(member) for member in missing_members))
        )

    manifests.sort(key=lambda item: (str(item["manifest"]), str(item["package"])))
    targets.sort(
        key=lambda item: (
            str(item["manifest"]),
            str(item["source"]),
            str(item["name"]),
        )
    )
    if not manifests:
        raise EvidenceError("cargo metadata selected no workspace manifests")
    if len(manifests) > max_manifests:
        raise EvidenceError(
            f"workspace manifest count {len(manifests)} exceeds limit {max_manifests}"
        )
    if len(targets) > max_targets:
        raise EvidenceError(f"workspace target count {len(targets)} exceeds limit {max_targets}")
    return manifests, targets


def parse_diff_locations(output: str, root: Path) -> list[tuple[str, int]]:
    findings: set[tuple[str, int]] = set()
    for match in DIFF_RE.finditer(output):
        raw_path = match.group(1).strip()
        try:
            line = int(match.group(2))
        except ValueError as error:
            raise EvidenceError(
                f"rustfmt emitted an invalid diff line: {match.group(0)}"
            ) from error
        candidate = Path(raw_path)
        if not candidate.is_absolute():
            candidate = root / candidate
        try:
            resolved = without_windows_extended_prefix(candidate.resolve(strict=False))
            comparison_root = without_windows_extended_prefix(root.resolve(strict=True))
            relative = resolved.relative_to(comparison_root).as_posix()
        except (OSError, ValueError) as error:
            raise EvidenceError(f"rustfmt diff path escapes the repository: {raw_path}") from error
        findings.add((relative, line))
    return sorted(findings)


def classify_fmt_run(
    result: ProcessResult,
    *,
    root: Path,
    manifest: dict[str, object],
) -> tuple[dict[str, object], list[dict[str, object]]]:
    command = command_record(result)
    base: dict[str, object] = {
        "package": manifest["package"],
        "manifest": manifest["manifest"],
        **command,
    }
    if result.spawn_error:
        return ({**base, "status": "instrument_failure", "reason": result.spawn_error}, [])
    if result.timed_out:
        return ({**base, "status": "instrument_failure", "reason": "formatter timed out"}, [])
    if result.signal is not None:
        return (
            {**base, "status": "instrument_failure", "reason": f"formatter signal {result.signal}"},
            [],
        )
    if result.stdout_truncated or result.stderr_truncated:
        return (
            {**base, "status": "instrument_failure", "reason": "formatter output exceeded limit"},
            [],
        )
    if result.exit_code == 0:
        return ({**base, "status": "pass"}, [])

    try:
        locations = parse_diff_locations(result.output, root)
    except EvidenceError as error:
        return ({**base, "status": "instrument_failure", "reason": bounded_text(error)}, [])
    if result.exit_code == 1 and locations and not RUSTFMT_ERROR_RE.search(result.output):
        reproduce = [
            "cargo",
            "fmt",
            "--manifest-path",
            str(manifest["manifest"]),
            "--",
            "--check",
        ]
        findings = [
            {
                "package": manifest["package"],
                "manifest": manifest["manifest"],
                "path": path_value,
                "line": line,
                "reproduce": reproduce,
            }
            for path_value, line in locations
        ]
        return ({**base, "status": "format_failure", "finding_count": len(findings)}, findings)

    return (
        {
            **base,
            "status": "instrument_failure",
            "reason": (
                f"formatter exited {result.exit_code} without a trustworthy diff marker: "
                f"{bounded_text(result.output)}"
            ),
        },
        [],
    )


def evidence_payload(receipt: dict[str, object]) -> dict[str, object]:
    return {
        key: value
        for key, value in receipt.items()
        if key not in {"evidence_sha256"}
    }


def write_json_atomic(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        os.chmod(temporary, 0o600)
        with os.fdopen(descriptor, "wb") as destination:
            destination.write(canonical_json(payload))
            destination.flush()
            os.fsync(destination.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            temporary.unlink(missing_ok=True)
        finally:
            raise


def build_failure_receipt(
    result: str,
    reason: object,
    *,
    candidate_sha: str | None = None,
    candidate_tree_sha: str | None = None,
    commands: list[dict[str, object]] | None = None,
    instrument_failures: list[dict[str, object]] | None = None,
    limits: dict[str, object] | None = None,
) -> dict[str, object]:
    receipt: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "receipt_kind": RECEIPT_KIND,
        "result": result,
        "reason": bounded_text(reason),
        "subject": (
            {
                "repository_sha": candidate_sha,
                "repository_tree_sha": candidate_tree_sha,
            }
            if candidate_sha and candidate_tree_sha
            else None
        ),
        "inputs": {},
        "workspace": {"manifests": [], "targets": []},
        "commands": commands or [],
        "runs": [],
        "findings": [],
        "instrument_failures": instrument_failures or [],
        "findings_truncated": False,
        "limits": limits or {},
        "claim_boundary": (
            "No formatter verdict was established because the instrument or candidate/workspace "
            "evidence did not reach a complete trustworthy result."
        ),
    }
    receipt["evidence_sha256"] = "sha256:" + sha256_bytes(canonical_json(evidence_payload(receipt)))
    return receipt


def execute(args: argparse.Namespace) -> tuple[dict[str, object], int]:
    root = args.root.resolve(strict=True)
    if not root.is_dir():
        raise EvidenceError(f"repository root is not a directory: {root}")
    receipt_path = args.receipt if args.receipt.is_absolute() else root / args.receipt
    env = os.environ.copy()
    env["CARGO_TERM_COLOR"] = "never"
    env["RUSTFMT"] = str(args.rustfmt)
    runner = Runner(
        deadline=time.monotonic() + args.timeout_seconds,
        max_output_bytes=args.max_output_bytes,
    )

    candidate_sha, tree_sha, subject_commands = resolve_subject(
        root,
        env,
        runner,
        args.candidate_sha,
        args.candidate_tree_sha,
    )

    probe_commands = list(subject_commands)
    try:
        rustfmt_version_result = runner.run([str(args.rustfmt), "--version"], cwd=root, env=env)
        probe_commands.append(command_record(rustfmt_version_result))
        require_success(rustfmt_version_result, "rustfmt version probe")
        cargo_version_result = runner.run([str(args.cargo), "--version"], cwd=root, env=env)
        probe_commands.append(command_record(cargo_version_result))
        require_success(cargo_version_result, "cargo version probe")
        rustc_version_result = runner.run([str(args.rustc), "-Vv"], cwd=root, env=env)
        probe_commands.append(command_record(rustc_version_result))
        require_success(rustc_version_result, "rustc version probe")
        metadata_result = runner.run(
            [
                str(args.cargo),
                "metadata",
                "--no-deps",
                "--locked",
                "--format-version",
                "1",
                "--manifest-path",
                "Cargo.toml",
            ],
            cwd=root,
            env=env,
            max_output_bytes=args.max_metadata_bytes,
        )
        probe_commands.append(command_record(metadata_result))
        require_success(metadata_result, "cargo metadata")
    except InstrumentError as error:
        failure_row = {
            **(error.record or {}),
            "status": "instrument_failure",
            "reason": bounded_text(error),
        }
        receipt = build_failure_receipt(
            "instrument_failure",
            error,
            candidate_sha=candidate_sha,
            candidate_tree_sha=tree_sha,
            commands=probe_commands,
            instrument_failures=[failure_row],
            limits={
                "total_timeout_seconds": args.timeout_seconds,
                "max_output_bytes_per_stream": args.max_output_bytes,
                "max_metadata_bytes_per_stream": args.max_metadata_bytes,
                "max_manifests": args.max_manifests,
                "max_targets": args.max_targets,
                "max_findings": args.max_findings,
            },
        )
        write_json_atomic(receipt_path, receipt)
        return receipt, 2

    manifests, targets = parse_metadata(
        root,
        metadata_result.stdout,
        max_manifests=args.max_manifests,
        max_targets=args.max_targets,
    )

    inputs = {
        "cargo_toml_sha256": file_digest(root / "Cargo.toml"),
        "cargo_lock_sha256": file_digest(root / "Cargo.lock", required=False),
        "rust_toolchain_sha256": file_digest(root / "rust-toolchain.toml"),
        "rustfmt_toml_sha256": file_digest(root / "rustfmt.toml"),
        "producer_sha256": file_digest(Path(__file__).resolve()),
        "cargo_version": cargo_version_result.stdout.strip(),
        "rustfmt_version": rustfmt_version_result.stdout.strip(),
        "rustc_version_verbose": rustc_version_result.stdout.strip(),
    }

    runs: list[dict[str, object]] = []
    findings: list[dict[str, object]] = []
    instrument_failures: list[dict[str, object]] = []
    findings_truncated = False
    for manifest in manifests:
        if runner.remaining() <= 0:
            row = {
                "package": manifest["package"],
                "manifest": manifest["manifest"],
                "status": "instrument_failure",
                "reason": "total timeout exhausted before formatter run",
                "command": [],
                "exit_code": None,
                "signal": None,
                "timed_out": True,
                "spawn_error": None,
                "stdout_truncated": False,
                "stderr_truncated": False,
            }
            runs.append(row)
            instrument_failures.append(row)
            continue
        command = [
            str(args.cargo),
            "fmt",
            "--manifest-path",
            str(manifest["manifest"]),
            "--",
            "--check",
        ]
        result = runner.run(command, cwd=root, env=env)
        row, row_findings = classify_fmt_run(result, root=root, manifest=manifest)
        runs.append(row)
        if row_findings and len(findings) + len(row_findings) > args.max_findings:
            if not findings_truncated:
                limit_failure = {
                **row,
                "status": "instrument_failure",
                "reason": f"formatter findings exceed limit {args.max_findings}",
                }
                instrument_failures.append(limit_failure)
                findings_truncated = True
            remaining = max(0, args.max_findings - len(findings))
            findings.extend(row_findings[:remaining])
        else:
            findings.extend(row_findings)
            if row["status"] == "instrument_failure":
                instrument_failures.append(row)

    final_head = runner.run(["git", "rev-parse", "HEAD^{commit}"], cwd=root, env=env)
    probe_commands.append(command_record(final_head))
    final_tree = runner.run(["git", "rev-parse", "HEAD^{tree}"], cwd=root, env=env)
    probe_commands.append(command_record(final_tree))
    final_status = runner.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=root,
        env=env,
    )
    probe_commands.append(command_record(final_status))
    final_status_row = command_record(final_status)
    try:
        require_success(final_head, "final git candidate resolution")
        require_success(final_tree, "final git tree resolution")
        require_success(final_status, "final git worktree status")
    except InstrumentError as error:
        instrument_failures.append(
            {
                **(error.record or final_status_row),
                "status": "instrument_failure",
                "reason": bounded_text(error),
            }
        )
    else:
        final_sha = validate_sha(final_head.stdout.strip(), "final candidate SHA")
        final_tree_sha = validate_sha(final_tree.stdout.strip(), "final candidate tree SHA")
        if final_sha != candidate_sha or final_tree_sha != tree_sha:
            instrument_failures.append(
                {
                    **final_status_row,
                    "status": "instrument_failure",
                    "reason": "candidate commit or tree changed while formatter evidence was running",
                }
            )
        elif final_status.stdout:
            instrument_failures.append(
                {
                    **final_status_row,
                    "status": "instrument_failure",
                    "reason": (
                        "candidate worktree changed while formatter evidence was running: "
                        + bounded_text(final_status.stdout)
                    ),
                }
            )

    findings.sort(key=lambda item: (str(item["path"]), int(item["line"]), str(item["package"])))
    if instrument_failures:
        result_name = "instrument_failure"
        exit_code = 2
    elif findings:
        result_name = "format_failure"
        exit_code = 1
    else:
        result_name = "pass"
        exit_code = 0

    receipt: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "receipt_kind": RECEIPT_KIND,
        "result": result_name,
        "subject": {
            "repository_sha": candidate_sha,
            "repository_tree_sha": tree_sha,
        },
        "inputs": inputs,
        "workspace": {
            "root": ".",
            "manifest_count": len(manifests),
            "target_count": len(targets),
            "manifests": manifests,
            "targets": targets,
        },
        "commands": probe_commands,
        "runs": runs,
        "findings": findings,
        "instrument_failures": instrument_failures,
        "findings_truncated": findings_truncated,
        "limits": {
            "total_timeout_seconds": args.timeout_seconds,
            "max_output_bytes_per_stream": args.max_output_bytes,
            "max_metadata_bytes_per_stream": args.max_metadata_bytes,
            "max_manifests": args.max_manifests,
            "max_targets": args.max_targets,
            "max_findings": args.max_findings,
        },
        "claim_boundary": (
            "Proves rustfmt --check over every Cargo workspace member and discovered "
            "target identity. It does not compile or execute repository Rust code and "
            "does not authorize merge policy."
        ),
    }
    receipt["evidence_sha256"] = "sha256:" + sha256_bytes(canonical_json(evidence_payload(receipt)))
    write_json_atomic(receipt_path, receipt)
    return receipt, exit_code


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--receipt", type=Path, default=DEFAULT_RECEIPT)
    parser.add_argument("--cargo", type=Path, default=Path(os.environ.get("CARGO", "cargo")))
    parser.add_argument("--rustfmt", type=Path, default=Path(os.environ.get("RUSTFMT", "rustfmt")))
    parser.add_argument("--rustc", type=Path, default=Path(os.environ.get("RUSTC", "rustc")))
    parser.add_argument("--candidate-sha")
    parser.add_argument("--candidate-tree-sha")
    parser.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--max-output-bytes", type=int, default=DEFAULT_MAX_OUTPUT_BYTES)
    parser.add_argument("--max-metadata-bytes", type=int, default=DEFAULT_MAX_METADATA_BYTES)
    parser.add_argument("--max-manifests", type=int, default=DEFAULT_MAX_MANIFESTS)
    parser.add_argument("--max-targets", type=int, default=DEFAULT_MAX_TARGETS)
    parser.add_argument("--max-findings", type=int, default=DEFAULT_MAX_FINDINGS)
    args = parser.parse_args(argv)
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.max_output_bytes <= 0:
        parser.error("--max-output-bytes must be positive")
    if args.max_metadata_bytes <= 0:
        parser.error("--max-metadata-bytes must be positive")
    if args.max_manifests <= 0 or args.max_targets <= 0 or args.max_findings <= 0:
        parser.error("count limits must be positive")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    receipt_path = args.receipt if args.receipt.is_absolute() else root / args.receipt
    try:
        receipt, exit_code = execute(args)
    except (EvidenceError, InstrumentError, OSError, TypeError, ValueError) as error:
        result = "instrument_failure" if isinstance(error, InstrumentError) else "not_proven"
        failure_record = error.record if isinstance(error, InstrumentError) else None
        receipt = build_failure_receipt(
            result,
            error,
            commands=[failure_record] if failure_record else None,
            instrument_failures=(
                [{**failure_record, "status": "instrument_failure", "reason": bounded_text(error)}]
                if failure_record
                else None
            ),
            limits={
                "total_timeout_seconds": args.timeout_seconds,
                "max_output_bytes_per_stream": args.max_output_bytes,
                "max_metadata_bytes_per_stream": args.max_metadata_bytes,
                "max_manifests": args.max_manifests,
                "max_targets": args.max_targets,
                "max_findings": args.max_findings,
            },
        )
        try:
            write_json_atomic(receipt_path, receipt)
        except OSError as write_error:
            print(
                f"rustfmt check could not persist receipt: {bounded_text(write_error)}; "
                f"original error: {bounded_text(error)}",
                file=sys.stderr,
            )
            return 2
        print(bounded_text(error), file=sys.stderr)
        return 2
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
