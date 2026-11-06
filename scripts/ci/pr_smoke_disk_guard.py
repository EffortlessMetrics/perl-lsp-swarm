#!/usr/bin/env python3
"""PR Smoke runner disk guard (#11943).

Two modes backing the two acceptance arms of issue #11943:

``preflight``
    Run BEFORE any PR-fast build starts. Fails the job immediately, naming the
    path, when the filesystems holding the workspace and the cargo target
    directory have less free headroom than the measured worst-case need of the
    tier. A degraded runner therefore reports an explicit resource-exhaustion
    diagnostic naming the filled path instead of dying 38 minutes later inside
    ``ld`` with SIGBUS disguised as a compile failure (run 32697324730).
    Preflight also clears cache-restored ``*.log`` files from the receipt-log
    directory first (the shared cargo cache restores all of ``target``, so a
    fresh job can otherwise start with an unrelated run's logs — the same
    misattribution class #12085 fixed for gate receipts): after this sweep,
    every scanned log was produced by the current run.

    ``classify``
    Run ``if: always()`` AFTER the gate runner exits. Scans the per-gate logs
    under ``target/receipts/logs`` for exhaustion signatures (ENOSPC /
    ``os error 28``, ``ld terminated with signal``, LLVM IO failure), emits
    ``::error`` annotations that name the offending log and the filled target
    directory, and appends a decision-grade verdict plus a fresh free-bytes
    snapshot to ``pr-fast-disk-pressure.log`` (composing with #11977's raw
    capture). Only ENOSPC / os-error-28 matches earn the resource-exhaustion
    verdict, which names the matched logs and leaves other failure causes
    unattributed. A linker SIGBUS or a lone rustc-LLVM
    output-stream failure is recorded as corroborating evidence under a
    distinct ``not_proven_io_failure`` verdict that does not exonerate the
    candidate: signal 7 identifies a bus error, not its cause (truncated or
    concurrently replaced mmap inputs, storage I/O faults, corrupt objects,
    and hardware faults produce it without any ENOSPC condition), so the
    definitive classification additionally requires an ENOSPC / os-error-28
    match in the same run's logs.
    Advisory only: never changes the gate outcome.

Measured basis (2026-08-24 receipt artifacts, runs 32697324730 and
32697144932): the single PR Smoke ``CARGO_TARGET_DIR`` grows to ~83 GiB on a
cold-cache branch while the hosted runner offers ~83-84 GiB free at start on a
145 GiB root volume, so unbounded builds fill the disk mid-link. Run
37195503421 (2026-10-04) measured 85 GiB after the debuginfo trim (#17231).
The preflight floor is derived from that declared footprint bound plus a
reserve, so a runner that cannot hold the tier is rejected before any build
instead of being accepted into a mid-link ENOSPC; the bound is declared in
the workflow beside the trim that produces it (``PR_SMOKE_FOOTPRINT_GIB``).
"""

from __future__ import annotations

import argparse
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

GIB = 1024**3

# Declared footprint bound for the PR Smoke tier (#17231): the single
# CARGO_TARGET_DIR measured 85 GiB in run 37195503421 (2026-10-04 receipt,
# pr-fast-disk-pressure.log), after the line-tables-only debuginfo trim.
# The preflight floor is DERIVED from this bound (footprint + reserve), so
# the two cannot drift apart the way the former fixed 32 GiB floor did:
# that floor's rejection band was empty and it accepted runners the lane
# then filled to 100% (ENOSPC -> ld SIGBUS). Update the bound only with a
# fresh measured receipt; the workflow declares the same number beside the
# trim that bounds it (PR_SMOKE_FOOTPRINT_GIB in ci.yml).
MEASURED_FOOTPRINT_BYTES = 85 * GIB
RESERVE_BYTES = 5 * GIB


def effective_min_free_bytes(
    requested: int | None,
    footprint_bytes: int = MEASURED_FOOTPRINT_BYTES,
    reserve_bytes: int = RESERVE_BYTES,
) -> int:
    """Floor for preflight: always exceeds the declared footprint.

    An explicit override at or below the footprint is a configuration
    error, not a smaller floor: it would re-open the empty rejection
    band #17231 measured. Larger overrides are honoured.
    """
    floor = footprint_bytes + reserve_bytes
    if requested is None:
        return floor
    if requested <= footprint_bytes:
        raise ValueError(
            f"min-free override {requested} bytes does not exceed the "
            f"declared PR Smoke footprint {footprint_bytes} bytes; the "
            "floor must exceed the footprint or it cannot reject any "
            "runner the lane would exhaust (issue #17231)"
        )
    return max(requested, floor)

PRESSURE_LOG_NAME = "pr-fast-disk-pressure.log"

# Signatures taken verbatim from the confirmed exhaustion receipts (runs
# 32697324730 / 32697144932): collect2 SIGBUS during linking, cargo/rustc
# ENOSPC errors, and the rustc-LLVM output-stream failure. Ordered most to
# least specific so the first match names the strongest evidence, and each
# log contributes at most that one finding. The linker death is pinned to
# signal 7 (SIGBUS): a timeout or OOM kill reports signal 15/9, which is not
# disk-exhaustion evidence and must not earn the definitive verdict.
EXHAUSTION_SIGNATURES: tuple[tuple[str, str], ...] = (
    ("enospc", "No space left on device"),
    ("enospc", "os error 28"),
    ("link_sigbus", "ld terminated with signal 7"),
    ("llvm_io_failure", "IO failure on output stream"),
)

# Classes whose match alone proves disk exhaustion: only the ENOSPC family.
# ``link_sigbus`` and ``llvm_io_failure`` are deliberately absent from this
# set: `ld terminated with signal 7` identifies a bus error, not its cause —
# truncated or concurrently replaced mmap inputs, storage I/O faults, corrupt
# objects, and hardware faults produce it without any ENOSPC condition — and
# "IO failure on output stream" names no errno at all. A lone match of either
# class is recorded and annotated as corroborating evidence under the
# non-exonerating ``not_proven_io_failure`` verdict; the resource-exhaustion
# classification requires an ENOSPC / os-error-28 match and applies only to
# the matched logs (review #12183).
STRONG_EXHAUSTION_CLASSES = frozenset({"enospc"})

Probe = Callable[[str], shutil._ntuple_diskusage]


def _nearest_existing_ancestor(path: str) -> str:
    """Return the closest existing ancestor of ``path`` (or ``path`` itself)."""
    candidate = Path(path)
    while not candidate.exists():
        parent = candidate.parent
        if parent == candidate:
            return path
        candidate = parent
    return str(candidate)


@dataclass(frozen=True)
class Finding:
    """One exhaustion signature found in one gate log."""

    log_path: str
    signature_class: str
    signature_text: str

    def annotation(self, target_dir: str) -> str:
        label = (
            "resource-exhaustion"
            if self.signature_class in STRONG_EXHAUSTION_CLASSES
            else "not-proven-io-failure"
        )
        return (
            f"{label} [{self.signature_class}] in {self.log_path}: "
            f"{self.signature_text!r}; filled path family: {target_dir} "
            "(see pr-fast-disk-pressure.log for the df/du snapshot)"
        )


def parse_min_free_bytes(raw: str) -> int:
    """Accept plain byte counts or a GiB-suffixed value (``32GiB``)."""
    text = raw.strip()
    if text.lower().endswith("gib"):
        number = text[: -len("gib")].strip()
        try:
            value = float(number)
        except ValueError as error:
            raise argparse.ArgumentTypeError(
                f"invalid GiB value: {raw!r}"
            ) from error
        if value < 0:
            raise argparse.ArgumentTypeError(
                f"min-free value must be non-negative: {raw!r}"
            )
        return int(value * GIB)
    try:
        value = int(text)
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            f"min-free value must be bytes or '<n>GiB': {raw!r}"
        ) from error
    if value < 0:
        raise argparse.ArgumentTypeError(
            f"min-free value must be non-negative: {raw!r}"
        )
    return value


def _probe_path(probe: Probe, path: str) -> shutil._ntuple_diskusage:
    """Probe ``path``, falling back to its nearest existing ancestor.

    The cargo target directory may legitimately not exist yet at pre-flight
    time; the filesystem that will hold it is the one its first existing
    ancestor already lives on. Any other probe failure propagates so callers
    can fail closed.
    """
    try:
        return probe(path)
    except OSError as error:
        if isinstance(error, (FileNotFoundError, NotADirectoryError)):
            return probe(_nearest_existing_ancestor(path))
        raise


def evaluate_preflight(
    paths: dict[str, str],
    min_free_bytes: int,
    probe: Probe = shutil.disk_usage,
) -> list[str]:
    """Return one human-readable breach message per path below the floor.

    A path whose usage cannot be probed counts as a breach: headroom that
    cannot be proven is treated exactly like headroom that is not there.
    """
    breaches: list[str] = []
    for label, path in paths.items():
        try:
            usage = _probe_path(probe, path)
        except OSError as error:
            breaches.append(
                f"PR Smoke pre-flight disk budget could not probe {label} "
                f"path {path}: {error}. Failing closed rather than building "
                "into an unverifiable filesystem (issue #11943)."
            )
            continue
        if usage.free < min_free_bytes:
            breaches.append(
                f"PR Smoke pre-flight disk budget breached for {label} "
                f"path {path}: {usage.free} bytes free "
                f"(budget {min_free_bytes} bytes). Building would exhaust this "
                f"filesystem mid-link (ENOSPC -> ld SIGBUS, issue #11943); "
                "failing before any build so the filled path is named."
            )
    return breaches


def render_snapshot(paths: dict[str, str], probe: Probe = shutil.disk_usage) -> str:
    """Render the df-style free-bytes block appended to the pressure log."""
    lines = []
    for label, path in paths.items():
        try:
            usage = _probe_path(probe, path)
        except OSError as error:
            lines.append(f"{label} path {path}: unavailable ({error})")
            continue
        lines.append(
            f"{label} path {path}: total={usage.total} "
            f"used={usage.used} free={usage.free}"
        )
    return "\n".join(lines)


def scan_logs(logs_dir: Path) -> list[Finding]:
    """Scan every ``*.log`` directly under ``logs_dir`` for signatures."""
    findings: list[Finding] = []
    if not logs_dir.is_dir():
        return findings
    for log_path in sorted(logs_dir.glob("*.log")):
        try:
            text = log_path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for signature_class, signature_text in EXHAUSTION_SIGNATURES:
            if signature_text in text:
                findings.append(
                    Finding(
                        log_path=str(log_path),
                        signature_class=signature_class,
                        signature_text=signature_text,
                    )
                )
                break
    return findings


def _emit(message: str) -> None:
    print(message, flush=True)


def _clear_restored_gate_logs(logs_dir: Path) -> list[str]:
    """Remove cache-restored ``*.log`` files before the current gate run.

    The workflow's shared cargo cache restores all of ``target``, so a fresh
    job can start with exhaustion logs produced by an unrelated SHA or run;
    classifying them as current-run evidence would exonerate a real candidate
    (the misattribution class #12085 fixed for gate receipts). Mirrors
    ``run_gate_shard._clean_restored_receipt_state`` for this advisory lane:
    best-effort, never raises, and refuses to follow a symlinked log
    directory. Returns the names of removed files for provenance.
    """
    try:
        if logs_dir.is_symlink():
            _emit(
                f"::warning::refusing symlinked receipt-log directory during "
                f"cleanup: {logs_dir}"
            )
            return []
        if not logs_dir.is_dir():
            return []
        removed: list[str] = []
        for log_path in sorted(logs_dir.glob("*.log")):
            if log_path.is_symlink():
                _emit(
                    f"::warning::refusing symlinked receipt log during "
                    f"cleanup: {log_path}"
                )
                continue
            try:
                log_path.unlink()
                removed.append(log_path.name)
            except OSError as error:
                _emit(f"::warning::could not clear restored log {log_path}: {error}")
        return removed
    except OSError as error:
        _emit(f"::warning::could not sweep restored receipt logs: {error}")
        return []


def run_preflight(args: argparse.Namespace) -> int:
    paths = {"workspace": args.workspace_path}
    target_dir = args.target_dir or str(Path(args.workspace_path) / "target")
    paths["cargo-target-dir"] = target_dir
    try:
        min_free_bytes = effective_min_free_bytes(
            args.min_free_bytes,
            footprint_bytes=getattr(args, "footprint_bytes", MEASURED_FOOTPRINT_BYTES),
            reserve_bytes=getattr(args, "reserve_bytes", RESERVE_BYTES),
        )
    except ValueError as error:
        _emit(f"::error::PR Smoke pre-flight disk guard misconfigured: {error}")
        return 2
    breaches = evaluate_preflight(paths, min_free_bytes)
    pressure_log = Path(args.workspace_path) / "target/receipts/logs"
    # Current-run evidence binding (#11943 review): sweep cache-restored logs
    # before anything writes today's records, so the post-run classify step
    # can only ever see logs produced by this run.
    removed_logs = _clear_restored_gate_logs(pressure_log)
    removal_note = (
        [f"cleared {len(removed_logs)} restored log(s): {', '.join(removed_logs)}"]
        if removed_logs
        else []
    )
    if breaches:
        _append_pressure_record(
            pressure_log / PRESSURE_LOG_NAME,
            header="== PR Smoke pre-flight disk budget REJECTED the runner ==",
            body_lines=[*removal_note, *breaches, render_snapshot(paths)],
        )
        for message in breaches:
            _emit(f"::error::{message}")
        return 1
    _append_pressure_record(
        pressure_log / PRESSURE_LOG_NAME,
        header="== PR Smoke pre-flight disk budget accepted the runner ==",
        body_lines=[*removal_note, render_snapshot(paths)],
    )
    _emit(f"PR Smoke pre-flight disk budget OK (>= {min_free_bytes} bytes free)")
    return 0


def run_classify(args: argparse.Namespace) -> int:
    findings = scan_logs(Path(args.logs_dir))
    strong = [
        finding
        for finding in findings
        if finding.signature_class in STRONG_EXHAUSTION_CLASSES
    ]
    corroborating = [
        finding
        for finding in findings
        if finding.signature_class not in STRONG_EXHAUSTION_CLASSES
    ]
    target_dir = args.target_dir or "target"
    body_lines = [render_snapshot({"cargo-target-dir": target_dir})]
    if strong:
        body_lines.append(
            "VERDICT: resource-exhaustion detected in gate logs "
            "(ENOSPC / os-error-28 class); matched logs: "
            f"{', '.join(finding.log_path for finding in strong)}. "
            "Other gate failure causes remain unattributed."
        )
        for finding in (*strong, *corroborating):
            annotation = finding.annotation(target_dir)
            body_lines.append(annotation)
            _emit(f"::error::{annotation}")
    elif corroborating:
        body_lines.append(
            "VERDICT: not_proven_io_failure: a lone linker SIGBUS or a lone "
            "rustc-LLVM 'IO failure on output stream' line identifies a bus "
            "error or a failed write, not its cause (truncated/replaced mmap "
            "inputs, broken pipe, closed output, filesystem I/O fault, or "
            "permission/path failure produce the same text without ENOSPC), "
            "so this is not proof of disk exhaustion and does not exonerate "
            "the candidate. Treat it as corroborating evidence only unless an "
            "ENOSPC / os-error-28 match appears in this run's logs."
        )
        for finding in corroborating:
            annotation = finding.annotation(target_dir)
            body_lines.append(annotation)
            _emit(f"::error::{annotation}")
    else:
        body_lines.append(
            "VERDICT: no resource-exhaustion signatures found in gate logs."
        )
        _emit(
            "PR-fast disk-exhaustion classification: no exhaustion signatures "
            f"found under {args.logs_dir}"
        )
    _append_pressure_record(
        Path(args.pressure_log),
        header="== PR-fast disk-exhaustion classification ==",
        body_lines=body_lines,
    )
    return 0


def _append_pressure_record(pressure_log: Path, header: str, body_lines: list[str]) -> None:
    """Best-effort append; diagnostics must never break the lane."""
    try:
        pressure_log.parent.mkdir(parents=True, exist_ok=True)
        with pressure_log.open("a", encoding="utf-8") as handle:
            handle.write(header + "\n")
            for line in body_lines:
                handle.write(line + "\n")
    except OSError as error:
        _emit(f"::warning::could not update {pressure_log}: {error}")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    subparsers = parser.add_subparsers(dest="mode", required=True)

    preflight = subparsers.add_parser(
        "preflight", help="fail fast when free headroom cannot sustain the tier"
    )
    preflight.add_argument("--workspace-path", default=".")
    preflight.add_argument(
        "--target-dir",
        default="",
        help="cargo target directory (defaults to <workspace>/target)",
    )
    preflight.add_argument(
        "--min-free-bytes",
        type=parse_min_free_bytes,
        default=None,
        help="free-byte floor override, bytes or GiB suffix; must exceed "
        "--footprint-bytes (default: footprint + reserve)",
    )
    preflight.add_argument(
        "--footprint-bytes",
        type=parse_min_free_bytes,
        default=MEASURED_FOOTPRINT_BYTES,
        help="declared tier footprint bound, bytes or GiB suffix "
        "(default: measured 85GiB, issue #17231)",
    )
    preflight.set_defaults(handler=run_preflight)

    classify = subparsers.add_parser(
        "classify", help="annotate exhaustion signatures found in gate logs"
    )
    classify.add_argument("--logs-dir", default="target/receipts/logs")
    classify.add_argument("--target-dir", default="target")
    classify.add_argument(
        "--pressure-log",
        default=f"target/receipts/logs/{PRESSURE_LOG_NAME}",
    )
    classify.set_defaults(handler=run_classify)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    handler: Callable[[argparse.Namespace], int] = args.handler
    return handler(args)


if __name__ == "__main__":
    sys.exit(main())
