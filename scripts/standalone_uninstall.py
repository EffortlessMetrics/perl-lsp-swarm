#!/usr/bin/env python3
"""POSIX standalone owned-state uninstall helper for scripts/install.sh (#8372).

Observes installer-owned state under INSTALL_DIR/.perl-lsp, records a bounded
standalone_owned_state.v1 manifest after install, and removes only manifest-
recorded owned rows plus PATH symlinks that point into the store.

Does not mutate profile/registry PATH persistence (#7832), Windows state, or
foreign/package-manager installations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "standalone_owned_state.v1"
MANIFEST_FILENAME = "owned_state.v1.json"
BIN_NAMES = ("perllsp", "perl-dap")
CLAIM_BOUNDARY = (
    "Records one first-party standalone install for bounded uninstall (#8372). "
    "Does not activate lifecycle claims or prove fresh-process absence (#10746)."
)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def symlink_content_digest(link: Path) -> str:
    target = os.readlink(link)
    return sha256_bytes(target.encode("utf-8"))


def directory_tree_digest(root: Path) -> str:
    if not root.is_dir():
        raise ValueError(f"not a directory: {root}")
    hasher = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file() and not p.is_symlink()):
        rel = path.relative_to(root).as_posix()
        hasher.update(rel.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
        hasher.update(b"\0")
    return hasher.hexdigest()


def install_root_path(install_dir: Path) -> Path:
    return install_dir.resolve() / ".perl-lsp"


def manifest_path(install_dir: Path) -> Path:
    return install_root_path(install_dir) / MANIFEST_FILENAME


def entry_path_under_store(store: Path, relative_path: str) -> Path | None:
    """Resolve a manifest relative_path strictly inside the install store."""
    rel = Path(relative_path)
    if rel.is_absolute() or ".." in rel.parts:
        return None
    store_resolved = store.resolve()
    candidate = store / rel
    try:
        candidate.resolve().relative_to(store_resolved)
    except (ValueError, OSError):
        return None
    return candidate


def safe_remove_owned_path(path: Path, store: Path) -> None:
    """Remove a file, symlink, or directory tree without following directory symlinks."""
    store_resolved = store.resolve()
    path_resolved = path.resolve()
    try:
        path_resolved.relative_to(store_resolved)
    except ValueError as exc:
        raise ValueError(f"path escapes install store: {path}") from exc

    if path.is_symlink() or path.is_file():
        path.unlink()
        return
    if not path.is_dir():
        return

    for child in sorted(path.iterdir(), reverse=True):
        if child.is_symlink() or child.is_file():
            child.unlink()
        elif child.is_dir():
            safe_remove_owned_path(child, store)
    path.rmdir()


def path_selector_is_owned(link: Path, store: Path) -> bool:
    if not link.is_symlink():
        return False
    target = Path(os.readlink(link))
    if not target.is_absolute():
        target = (link.parent / target).resolve()
    try:
        target.relative_to(store.resolve())
    except ValueError:
        return False
    return True


def emit_receipt(parts: dict[str, str]) -> None:
    body = " ".join(f"{key}={value}" for key, value in parts.items())
    print(f"standalone_uninstall_receipt {body}")


@dataclass
class AbsenceVerdict:
    already_absent: bool
    complete_evidence: bool
    reason: str = ""


def classify_absence(install_dir: Path) -> AbsenceVerdict:
    store = install_root_path(install_dir)
    manifest = manifest_path(install_dir)
    selectors = [install_dir / name for name in BIN_NAMES]
    any_selector = any(p.exists() or p.is_symlink() for p in selectors)
    if store.exists() or any_selector:
        if store.exists() and not manifest.is_file():
            return AbsenceVerdict(
                already_absent=False,
                complete_evidence=False,
                reason="legacy_install_without_owned_manifest",
            )
        return AbsenceVerdict(already_absent=False, complete_evidence=False, reason="owned_state_present")
    return AbsenceVerdict(already_absent=True, complete_evidence=True)


def entry_row(
    *,
    role: str,
    relative_path: str,
    ownership_class: str,
    retention: str,
    identity_kind: str,
    digest: str,
) -> dict[str, Any]:
    identity: dict[str, Any] = {"kind": identity_kind}
    if identity_kind != "unavailable":
        identity["sha256"] = digest
    return {
        "role": role,
        "relative_path": relative_path,
        "observed": "present",
        "ownership_class": ownership_class,
        "identity": identity,
        "process_refs": [],
        "user_modified": False,
        "retention": retention,
    }


def observe_manifest(install_dir: Path) -> dict[str, Any]:
    store = install_root_path(install_dir)
    if not store.is_dir():
        raise SystemExit("observe refused: install root store is absent")

    root_digest = sha256_bytes(str(store.resolve()).encode("utf-8"))
    entries: list[dict[str, Any]] = []

    current = store / "current"
    if current.is_symlink():
        entries.append(
            entry_row(
                role="selection_marker",
                relative_path="current",
                ownership_class="owned_required",
                retention="retain_required",
                identity_kind="sha256_content",
                digest=symlink_content_digest(current),
            )
        )

    previous = store / "previous"
    if previous.is_symlink():
        entries.append(
            entry_row(
                role="selection_marker",
                relative_path="previous",
                ownership_class="owned_retained_for_rollback",
                retention="retain_for_rollback",
                identity_kind="sha256_content",
                digest=symlink_content_digest(previous),
            )
        )

    candidates = store / "candidates"
    if candidates.is_dir():
        for candidate in sorted(p for p in candidates.iterdir() if p.is_dir()):
            rel = f"candidates/{candidate.name}"
            entries.append(
                entry_row(
                    role="immutable_candidate_dir",
                    relative_path=rel,
                    ownership_class="owned_required",
                    retention="retain_required",
                    identity_kind="directory_tree_digest",
                    digest=directory_tree_digest(candidate),
                )
            )

    current_id = ""
    if current.is_symlink():
        target = os.readlink(current)
        current_id = target.split("/")[-1]

    manifest: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "claim_boundary": CLAIM_BOUNDARY,
        "install_root": {
            "absolute_path": str(store.resolve()),
            "identity_digest_sha256": root_digest,
            "link_policy": "resolved_physical",
        },
        "installer": {"kind": "posix_installer", "schema_version": "posix_installer.v1"},
        "current_selection": {
            "candidate_id": current_id or "unknown",
            "version": "unknown",
            "artifact_sha256": "0" * 64,
            "selection_receipt_path": "receipts/current.json",
            "retained_for_rollback": False,
        },
        "previous_candidates": [],
        "other_roots_observed": [],
        "entries": entries,
        "transactions": [],
        "enumeration": {
            "complete": True,
            "instrument": "scripts/standalone_uninstall.py observe",
        },
        "redaction": {"policy": "none", "redacted_fields": []},
    }
    return manifest


def write_manifest(install_dir: Path) -> None:
    manifest = observe_manifest(install_dir)
    path = manifest_path(install_dir)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def verify_entry_present(store: Path, entry: dict[str, Any]) -> bool:
    rel = entry["relative_path"]
    path = store / rel
    if entry["identity"]["kind"] == "sha256_content":
        if not path.is_symlink() and not path.is_file():
            return False
        if path.is_symlink():
            return symlink_content_digest(path) == entry["identity"]["sha256"]
        return sha256_file(path) == entry["identity"]["sha256"]
    if entry["identity"]["kind"] == "directory_tree_digest":
        if not path.is_dir():
            return False
        return directory_tree_digest(path) == entry["identity"]["sha256"]
    return False


def removable_classes() -> set[str]:
    return {
        "owned_required",
        "owned_removable",
        "owned_retained_for_rollback",
    }


def prune_empty_dirs(root: Path) -> None:
    if not root.exists():
        return
    for path in sorted((p for p in root.rglob("*") if p.is_dir()), reverse=True):
        if not any(path.iterdir()):
            path.rmdir()
    if root.is_dir() and not any(root.iterdir()):
        root.rmdir()


def execute_uninstall(install_dir: Path) -> int:
    verdict = classify_absence(install_dir)
    if verdict.already_absent:
        emit_receipt(
            {
                "result": "already_absent_owned_state",
                "complete_evidence": "true",
                "route": "posix_installer",
            }
        )
        return 0

    if verdict.reason == "legacy_install_without_owned_manifest":
        emit_receipt(
            {
                "result": "not_proven",
                "reason": verdict.reason,
                "next_action": "manual_owned_state_resolution",
                "route": "posix_installer",
            }
        )
        return 2

    store = install_root_path(install_dir)
    manifest_file = manifest_path(install_dir)
    manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != SCHEMA_VERSION:
        emit_receipt(
            {
                "result": "root_or_manifest_mismatch",
                "reason": "unsupported_schema_version",
                "route": "posix_installer",
            }
        )
        return 2

    removed: list[str] = []
    preserved_foreign = 0

    for name in BIN_NAMES:
        link = install_dir / name
        if path_selector_is_owned(link, store):
            link.unlink()
            removed.append(f"path_selector:{name}")

    for entry in manifest.get("entries", []):
        cls = entry.get("ownership_class", "")
        if cls not in removable_classes():
            preserved_foreign += 1
            continue
        rel = entry["relative_path"]
        path = entry_path_under_store(store, rel)
        if path is None:
            emit_receipt(
                {
                    "result": "root_or_manifest_mismatch",
                    "reason": f"manifest_path_escape:{rel}",
                    "next_action": "reobserve_and_replan",
                    "route": "posix_installer",
                }
            )
            return 2
        if not path.exists() and not path.is_symlink():
            continue
        if not verify_entry_present(store, entry):
            emit_receipt(
                {
                    "result": "root_or_manifest_mismatch",
                    "reason": f"identity_drift:{rel}",
                    "next_action": "reobserve_and_replan",
                    "route": "posix_installer",
                }
            )
            return 2
        try:
            safe_remove_owned_path(path, store)
        except ValueError:
            emit_receipt(
                {
                    "result": "root_or_manifest_mismatch",
                    "reason": f"manifest_path_escape:{rel}",
                    "next_action": "reobserve_and_replan",
                    "route": "posix_installer",
                }
            )
            return 2
        removed.append(rel)

    if manifest_file.exists():
        manifest_file.unlink()
        removed.append(MANIFEST_FILENAME)

    prune_empty_dirs(store)

    emit_receipt(
        {
            "result": "removed",
            "removed_count": str(len(removed)),
            "preserved_foreign_rows": str(preserved_foreign),
            "route": "posix_installer",
            "fresh_process_proof": "not_established",
        }
    )
    return 0


def cmd_record(args: argparse.Namespace) -> int:
    write_manifest(Path(args.install_dir))
    emit_receipt({"result": "manifest_recorded", "route": "posix_installer"})
    return 0


def cmd_uninstall(args: argparse.Namespace) -> int:
    return execute_uninstall(Path(args.install_dir))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    record = sub.add_parser("record", help="Observe store and write owned_state.v1.json")
    record.add_argument("--install-dir", required=True)
    record.set_defaults(func=cmd_record)

    uninstall = sub.add_parser("uninstall", help="Remove installer-owned state for INSTALL_DIR")
    uninstall.add_argument("--install-dir", required=True)
    uninstall.set_defaults(func=cmd_uninstall)

    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
