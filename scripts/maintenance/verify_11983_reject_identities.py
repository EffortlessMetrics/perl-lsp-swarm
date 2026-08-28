#!/usr/bin/env python3
"""Verify current-main reject artifacts against the reviewed 11983 patch."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class RejectIdentity:
    hunk: str
    old_anchor: str


# Authored re-ownership table for the widened current-main denominator. A
# reject may be deleted only when both its patch hunk header and an old-side
# anchor match the entry for that exact path.
EXPECTED_REJECT_IDENTITIES: dict[str, tuple[RejectIdentity, ...]] = {
    "crates/perl-dap/features_sot.toml": (
        RejectIdentity('@@ -103,27 +103,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs-core/features_sot.toml": (
        RejectIdentity('@@ -141,27 +141,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs/features_sot.toml": (
        RejectIdentity('@@ -106,27 +106,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-parser/features_sot.toml": (
        RejectIdentity('@@ -103,27 +103,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs/src/runtime/language/formatting_policy/tests.rs": (
        RejectIdentity(
            "@@ -555,7 +555,7 @@ fn stale_unknown_range_decision_preserves_unknown_receipt_engine()",
            "fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
        ),
        RejectIdentity(
            "@@ -564,14 +564,9 @@ fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
            "let cases = [",
        ),
        RejectIdentity(
            "@@ -611,65 +606,53 @@ fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
            "for (offset, (method, params)) in cases.into_iter().enumerate() {",
        ),
        RejectIdentity(
            "@@ -688,13 +671,16 @@ fn live_external_partial_range_returns_typed_refusal_not_native_edits()",
            'assert!(response.error.is_none(), "range formatting should return a typed refusal");',
        ),
    ),
    "crates/perl-lsp-rs/src/runtime/text_sync.rs": (
        RejectIdentity(
            "@@ -11,9 +11,8 @@",
            "Arc, AtomicBool, AtomicU32, CodeFormatter, DocumentState, FormattingOptions, HashMap,",
        ),
    ),
    "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs": (
        RejectIdentity("@@ -1,7 +1,4 @@", "use super::{"),
        RejectIdentity(
            "@@ -242,77 +239,17 @@ impl LspServer {",
            "params: Option<Value>,",
        ),
    ),
    "crates/perl-lsp-rs/tests/lsp_batteries_e2e_workflow_test.rs": (
        RejectIdentity(
            "@@ -149,6 +149,8 @@ my$result=calculate(5,3);",
            "let formatted = apply_text_edits(messy_code, edits);",
        ),
        RejectIdentity(
            "@@ -158,7 +160,6 @@ my$result=calculate(5,3);",
            '"\\n",',
        ),
    ),
    "crates/perl-lsp-rs/tests/lsp_formatting_e2e.rs": (
        RejectIdentity(
            "@@ -48,8 +48,14 @@ fn native_default_document_formatting() -> Result<(), Box<dyn std::error::Error>",
            "fn native_default_range_formatting() -> Result<(), Box<dyn std::error::Error>> {",
        ),
        RejectIdentity(
            "@@ -70,18 +76,31 @@ fn native_default_range_formatting() -> Result<(), Box<dyn std::error::Error>> {",
            "let edits =",
        ),
        RejectIdentity(
            "@@ -167,23 +186,26 @@ fn native_default_formatting_honors_lsp_tab_size() -> Result<(), Box<dyn std::er",
            "fn native_default_ranges_formatting_formats_selected_ranges()",
        ),
        RejectIdentity(
            "@@ -205,23 +227,9 @@ sub third{my$c=3;return$c;}",
            "let edits =",
        ),
    ),
    "docs/specs/lsp-318-conformance-matrix.md": (
        RejectIdentity(
            "@@ -19,7 +19,7 @@ Status vocabulary:",
            "| Multi-range formatting | LSP 3.18 | range-formatting client support | `documentRangeFormattingProvider.rangesSupport` | `textDocument/rangesFormatting` | implemented+tested+documented | `lsp_caps_contract_shapes`; `lsp_disabled_features_tests`; `lsp_formatting_e2e`; `lsp_capabilities_snapshot`; `lsp_cap_snap` | `crates/perl-lsp-rs/src/runtime/language/formatting.rs`; `crates/perl-lsp-rs-core/src/protocol/capabilities.rs` | P0 | `documentRangesFormattingProvider` is not a valid capability and remains forbidden. |",
        ),
    ),
    "features.toml": (
        RejectIdentity('@@ -172,30 +172,30 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "xtask/src/tasks/lsp_318_matrix.rs": (
        RejectIdentity(
            "@@ -63,11 +63,11 @@ const ROWS: &[MatrixRow] = &[",
            'status: "implemented+tested+documented",',
        ),
    ),
}


def hunk_segments(text: str) -> list[str]:
    segments: list[list[str]] = []
    current: list[str] | None = None
    for line in text.splitlines():
        if line.startswith("@@ "):
            if current is not None:
                segments.append(current)
            current = [line]
        elif current is not None:
            current.append(line)
    if current is not None:
        segments.append(current)
    return ["\n".join(segment) for segment in segments]


def identity_for(segment: str) -> RejectIdentity:
    lines = segment.splitlines()
    if not lines:
        raise ValueError("empty reject hunk")
    old_side = [
        line[1:]
        for line in lines[1:]
        if line.startswith((" ", "-")) and not line.startswith("---")
    ]
    return RejectIdentity(lines[0], "\n".join(old_side))


def _retain_rejects(
    rejects: set[Path],
    reject_scope: Path,
    evidence_dir: Path,
    reason: str,
) -> None:
    _copy_rejects(rejects, reject_scope, evidence_dir)
    raise ValueError(f"unverified rejected hunks retained under {evidence_dir}: {reason}")


def _copy_rejects(
    rejects: set[Path],
    reject_scope: Path,
    evidence_dir: Path,
) -> None:
    evidence_dir.mkdir(parents=True, exist_ok=True)
    for reject in sorted(rejects):
        try:
            relative = reject.relative_to(reject_scope)
        except ValueError:
            relative = Path(reject.name)
        retained = evidence_dir / relative
        retained.parent.mkdir(parents=True, exist_ok=True)
        retained.write_bytes(reject.read_bytes())


def _retain(
    reject: Path,
    reject_scope: Path,
    evidence_dir: Path,
    reason: str,
) -> None:
    _retain_rejects(
        {reject.resolve()} if reject.exists() else set(),
        reject_scope,
        evidence_dir,
        f"{reject}: {reason}",
    )


def _scoped_reject(path: Path, reject_scope: Path) -> Path:
    resolved = path.resolve()
    try:
        resolved.relative_to(reject_scope)
    except ValueError as error:
        raise ValueError(f"reject artifact escapes canonical scope: {path}") from error
    return resolved


def _is_under(path: Path, directory: Path) -> bool:
    try:
        path.relative_to(directory)
    except ValueError:
        return False
    return True


def _verify_patch_path(path: str, patch_file: Path) -> None:
    lines = patch_file.read_text(encoding="utf-8").splitlines()
    expected_header = f"diff --git a/{path} b/{path}"
    expected_old = f"--- a/{path}"
    expected_new = f"+++ b/{path}"
    if (
        not lines
        or lines[0] != expected_header
        or expected_old not in lines[:4]
        or expected_new not in lines[:4]
    ):
        raise ValueError(f"{path}: patch artifact path mismatch")


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _verify_provenance(
    provenance_path: Path,
    entries: list[list[str]],
    *,
    source_parent: str | None,
    source_commit: str | None,
) -> None:
    try:
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid reject-artifact provenance: {provenance_path}") from error
    if source_parent is not None and provenance.get("source_parent") != source_parent:
        raise ValueError("reject-artifact provenance source parent mismatch")
    if source_commit is not None and provenance.get("source_commit") != source_commit:
        raise ValueError("reject-artifact provenance source commit mismatch")
    records = provenance.get("artifacts")
    if not isinstance(records, list) or len(records) != len(entries):
        raise ValueError("reject-artifact provenance coverage mismatch")
    by_path = {record.get("path"): record for record in records if isinstance(record, dict)}
    if set(by_path) != {entry[0] for entry in entries}:
        raise ValueError("reject-artifact provenance path mismatch")
    for path, patch_file, apply_log_file, _ in entries:
        record = by_path[path]
        if Path(record.get("patch", "")).resolve() != Path(patch_file).resolve():
            raise ValueError(f"{path}: patch provenance path mismatch")
        if Path(record.get("apply_log", "")).resolve() != Path(apply_log_file).resolve():
            raise ValueError(f"{path}: apply-log provenance path mismatch")
        if record.get("patch_sha256") != _sha256(Path(patch_file)):
            raise ValueError(f"{path}: patch provenance digest mismatch")
        if record.get("apply_log_sha256") != _sha256(Path(apply_log_file)):
            raise ValueError(f"{path}: apply-log provenance digest mismatch")


def validate_manifest(
    manifest_path: Path,
    evidence_dir: Path,
    *,
    reject_scope: Path,
    delete_verified: bool = False,
    require_complete_table: bool = True,
    provenance_path: Path | None = None,
    source_parent: str | None = None,
    source_commit: str | None = None,
) -> None:
    entries = [line.split("\t") for line in manifest_path.read_text(encoding="utf-8").splitlines()]
    if any(len(entry) != 4 for entry in entries):
        raise ValueError("reject manifest entries must have four tab-separated fields")
    if provenance_path is not None:
        _verify_provenance(
            provenance_path,
            entries,
            source_parent=source_parent,
            source_commit=source_commit,
        )
    paths = {entry[0] for entry in entries}
    if len(paths) != len(entries):
        raise ValueError("reject manifest contains duplicate file identities")
    if require_complete_table and paths != set(EXPECTED_REJECT_IDENTITIES):
        missing = sorted(set(EXPECTED_REJECT_IDENTITIES) - paths)
        extra = sorted(paths - set(EXPECTED_REJECT_IDENTITIES))
        raise ValueError(f"reject table coverage mismatch: missing={missing}, extra={extra}")

    scope = reject_scope.resolve()
    evidence_root = evidence_dir.resolve()
    manifest_rejects = [_scoped_reject(Path(entry[3]), scope) for entry in entries]
    if len(set(manifest_rejects)) != len(manifest_rejects):
        raise ValueError("reject manifest contains duplicate artifact identities")
    discovered_rejects = {
        candidate.resolve()
        for candidate in scope.rglob("*.rej")
        if not _is_under(candidate.resolve(), evidence_root)
    }
    manifest_reject_set = set(manifest_rejects)
    if discovered_rejects != manifest_reject_set:
        missing = sorted(str(path) for path in manifest_reject_set - discovered_rejects)
        extra = sorted(str(path) for path in discovered_rejects - manifest_reject_set)
        _retain_rejects(
            discovered_rejects,
            scope,
            evidence_dir,
            f"canonical artifact scope mismatch: missing={missing}, extra={extra}",
        )

    verified: list[Path] = []
    for (path, patch_file, apply_log_file, _), reject in zip(entries, manifest_rejects):
        expected = EXPECTED_REJECT_IDENTITIES.get(path)
        if expected is None:
            raise ValueError(f"{path}: no authored reject-identity table entry")
        log_text = Path(apply_log_file).read_text(encoding="utf-8")
        rejected_hunks = [int(number) for number in re.findall(r"Rejected hunk #(\d+)\.", log_text)]
        if not rejected_hunks:
            if reject.exists():
                _retain(
                    reject,
                    scope,
                    evidence_dir,
                    "apply reported no rejection but a reject artifact exists",
                )
            raise ValueError(f"{path}: authored reject identities have no matching apply rejection")
        if not reject.exists():
            _retain(
                reject,
                scope,
                evidence_dir,
                f"apply recorded rejected hunks {rejected_hunks} but no reject file exists",
            )

        patch_path = Path(patch_file)
        try:
            _verify_patch_path(path, patch_path)
        except ValueError as error:
            _retain(reject, scope, evidence_dir, str(error))
        patch_segments = hunk_segments(patch_path.read_text(encoding="utf-8"))
        reject_segments = hunk_segments(reject.read_text(encoding="utf-8"))
        if len(reject_segments) != len(rejected_hunks):
            _retain(
                reject,
                scope,
                evidence_dir,
                f"expected {len(rejected_hunks)} rejected hunks, found {len(reject_segments)}",
            )

        patch_indices: list[int] = []
        for segment in reject_segments:
            matches = [index for index, candidate in enumerate(patch_segments) if candidate == segment]
            if len(matches) != 1:
                _retain(
                    reject,
                    scope,
                    evidence_dir,
                    "reject hunk is absent from or ambiguous within the reviewed patch",
                )
            patch_indices.append(matches[0] + 1)
        if patch_indices != rejected_hunks:
            _retain(
                reject,
                scope,
                evidence_dir,
                f"reject hunk ordinals {patch_indices} do not match apply log {rejected_hunks}",
            )

        actual = []
        for segment in reject_segments:
            identity = identity_for(segment)
            old_lines = {line.strip() for line in identity.old_anchor.splitlines()}
            matches = [
                candidate
                for candidate in expected
                if candidate.hunk == identity.hunk and candidate.old_anchor.strip() in old_lines
            ]
            if len(matches) != 1:
                _retain(
                    reject,
                    scope,
                    evidence_dir,
                    f"hunk identity omitted or reused: {identity.hunk}",
                )
            actual.append(matches[0])
        if actual != list(expected):
            _retain(
                reject,
                scope,
                evidence_dir,
                "reject identities do not exactly match the authored per-file table",
            )
        verified.append(reject)

    if delete_verified:
        snapshots = {}
        for reject in verified:
            stat = reject.stat()
            snapshots[reject] = (
                (stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns),
                reject.read_bytes(),
            )
        try:
            for reject in verified:
                expected_identity, expected_contents = snapshots[reject]
                current_stat = reject.stat()
                current_identity = (
                    current_stat.st_dev,
                    current_stat.st_ino,
                    current_stat.st_size,
                    current_stat.st_mtime_ns,
                )
                if current_identity != expected_identity or reject.read_bytes() != expected_contents:
                    raise ValueError(f"verified reject cleanup target changed: {reject}")
                reject.unlink()
        except (OSError, ValueError) as error:
            for reject, (_, contents) in snapshots.items():
                if not reject.exists():
                    reject.write_bytes(contents)
            _copy_rejects(set(verified), scope, evidence_dir)
            raise ValueError(
                f"verified reject cleanup failed; artifacts retained under {evidence_dir}"
            ) from error
        deletion_receipt = evidence_dir / "verified-deletions.txt"
        evidence_dir.mkdir(parents=True, exist_ok=True)
        deletion_receipt.write_text(
            "verified reject artifacts deleted:\n"
            + "\n".join(str(reject) for reject in verified)
            + "\n",
            encoding="utf-8",
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--reject-scope", type=Path, default=Path("."))
    parser.add_argument("--delete-verified", action="store_true")
    parser.add_argument("--provenance", type=Path)
    parser.add_argument("--source-parent")
    parser.add_argument("--source-commit")
    args = parser.parse_args()
    try:
        validate_manifest(
            args.manifest,
            args.evidence_dir,
            reject_scope=args.reject_scope,
            delete_verified=args.delete_verified,
            provenance_path=args.provenance,
            source_parent=args.source_parent,
            source_commit=args.source_commit,
        )
    except (OSError, ValueError) as error:
        print(error, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
