"""Isolated managed-DAP cache model with a known-good preservation suite.

The model mirrors the accepted managed-route boundary the Zed adapter
candidate implements: candidates stage privately in a `.tmp` sibling, promote
atomically only after the expected member exists with the expected digest,
record the accepted current selection in `current.json`, and clean up only
inside the exact `perl-dap-managed-` family. Every failure scenario below
must leave the prior known-good selection byte-identical.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import tarfile
import zipfile
from pathlib import Path
from typing import Any, Callable

from .common import ReceiptError, sha256_file
from .dap_archive import extract_expected_member

DAP_MANAGED_PREFIX = "perl-dap-managed-"
CURRENT_MANIFEST = "current.json"

#: The complete recovery-scenario denominator. The receipt validator
#: recomputes this set, so a receipt cannot claim a passing cache-recovery
#: suite while silently dropping scenarios.
EXPECTED_SCENARIOS: tuple[str, ...] = (
    "complete_candidate_becomes_current",
    "missing_asset_preserves_known_good",
    "duplicate_asset_preserves_known_good",
    "wrong_target_asset_preserves_known_good",
    "wrong_product_member_preserves_known_good",
    "digest_mismatch_preserves_known_good",
    "member_digest_mismatch_preserves_known_good",
    "unsafe_archive_path_preserves_known_good",
    "duplicate_member_preserves_known_good",
    "missing_member_preserves_known_good",
    "foreign_executable_member_preserves_known_good",
    "partial_download_preserves_known_good",
    "extraction_failure_preserves_known_good",
    "launch_failure_preserves_known_good",
    "protocol_impurity_preserves_known_good",
    "promote_failure_preserves_known_good",
    "manifest_failure_preserves_known_good",
    "cleanup_stays_inside_the_perl_dap_family",
)


def _digest_bytes(payload: bytes) -> str:
    return "sha256:" + hashlib.sha256(payload).hexdigest()


def build_tar(path: Path, members: dict[str, bytes], executable: set[str] = set()) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, "w:gz") as archive:
        for name, payload in sorted(members.items()):
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = 0o755 if name in executable else 0o644
            archive.addfile(info, io.BytesIO(payload))
    return path


def good_archive_members(version: str, target: str, perl_dap: bytes) -> dict[str, bytes]:
    package = f"perllsp-{version}-{target}"
    return {
        f"{package}/LICENSE-MIT": b"license",
        f"{package}/perllsp": b"language-server sibling",
        f"{package}/perl-dap": perl_dap,
    }


def good_row(version: str, target: str, archive: Path, member_sha256: str) -> dict[str, Any]:
    archive_type = "tar.gz" if archive.suffix == ".gz" else "zip"
    return {
        "target": target,
        "archive_type": archive_type,
        "asset_name": archive.name,
        "asset_digest": sha256_file(archive),
        "archive_member": f"perllsp-{version}-{target}/perl-dap",
        "member_sha256": member_sha256,
        "make_executable": True,
    }


def fingerprint(root: Path) -> dict[str, str]:
    """Digest every file under root so scenario suites prove byte-preservation."""
    snapshot: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file():
            snapshot[str(path.relative_to(root)).replace("\\", "/")] = hashlib.sha256(
                path.read_bytes()
            ).hexdigest()
    return snapshot


class ManagedDapCache:
    """The debugger-specific managed cache boundary for one root directory."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)

    # -- current selection ---------------------------------------------------

    def current_manifest_path(self) -> Path:
        return self.root / CURRENT_MANIFEST

    def current(self) -> dict[str, Any] | None:
        path = self.current_manifest_path()
        if not path.is_file():
            return None
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            raise ReceiptError(f"managed current selection is unreadable: {error}") from error
        if not isinstance(value, dict):
            raise ReceiptError("managed current selection is not an object")
        return value

    def _write_current(self, selection: dict[str, Any]) -> None:
        path = self.current_manifest_path()
        temporary = path.with_suffix(".json.tmp")
        temporary.write_text(
            json.dumps(selection, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        os.replace(temporary, path)

    # -- candidate installation ----------------------------------------------

    def install(
        self,
        row: dict[str, Any],
        assets_by_name: dict[str, list[Path]],
        version: str,
        verify_process: Callable[[Path], None] | None = None,
        simulate_promote_failure: bool = False,
        simulate_manifest_failure: bool = False,
    ) -> Path:
        """Install one candidate release row, preserving known-good on failure.

        The candidate is staged in a private `.tmp` sibling at the exact
        archive-member path, verified against the exact release/member
        digests, and committed through a retire-and-swap: the incumbent is
        moved aside, the staged tree is renamed into place, the selection
        manifest is written, and only then is the retired incumbent deleted.
        Any promote or manifest failure rolls the incumbent back before the
        error surfaces, so the durable directory tree and the current
        selection are never simultaneously inconsistent.
        """
        name = row.get("asset_name")
        matches = assets_by_name.get(str(name), [])
        if not matches:
            raise ReceiptError(f"missing_asset: release has no asset named {name!r}")
        if len(matches) > 1:
            raise ReceiptError(
                f"duplicate_asset: release has {len(matches)} assets named {name!r}"
            )
        archive = matches[0]
        if archive.name.endswith(".partial"):
            raise ReceiptError(f"partial_download: asset {name!r} never completed its download")

        target = str(row["target"])
        version_dir = self.root / f"{DAP_MANAGED_PREFIX}{version}-{target}"
        staging = self.root / f"{version_dir.name}.tmp"
        retired = self.root / f"{version_dir.name}.retired"
        if staging.exists():
            shutil.rmtree(staging)
        staging.mkdir(parents=True)

        try:
            archive_digest = sha256_file(archive)
            if archive_digest != row["asset_digest"]:
                raise ReceiptError(
                    f"digest_mismatch: downloaded {name!r} digest {archive_digest} does not "
                    f"match the retained digest {row['asset_digest']}"
                )
            binary, _members, _sums = extract_expected_member(
                archive,
                str(row["archive_type"]),
                str(row["archive_member"]),
                staging,
                bool(row["make_executable"]),
            )
            # Keep the exact archive-member path inside the managed tree so
            # the installed projection matches the contract layout instead
            # of a flattened basename.
            nested = staging / str(row["archive_member"])
            nested.parent.mkdir(parents=True, exist_ok=True)
            binary.rename(nested)
            binary = nested
            binary_digest = sha256_file(binary)
            if binary_digest != row["member_sha256"]:
                raise ReceiptError(
                    "member_digest_mismatch: extracted perl-dap member digest "
                    f"{binary_digest} does not match the contract digest {row['member_sha256']}"
                )
            if verify_process is not None:
                try:
                    verify_process(binary)
                except ReceiptError as error:
                    raise ReceiptError(f"process_gate: {error}") from error
                except OSError as error:
                    raise ReceiptError(f"launch_failure: {error}") from error

            incumbent_retired = False
            if version_dir.exists():
                if retired.exists():
                    shutil.rmtree(retired)
                os.rename(version_dir, retired)
                incumbent_retired = True
            try:
                if simulate_promote_failure:
                    raise OSError("simulated promote failure")
                os.rename(staging, version_dir)
                if simulate_manifest_failure:
                    raise OSError("simulated manifest write failure")
                self._write_current(
                    {
                        "version": version,
                        "target": target,
                        "asset_name": str(name),
                        "asset_digest": row["asset_digest"],
                        "archive_member": row["archive_member"],
                        "member_sha256": row["member_sha256"],
                        "installed_binary": str(
                            (version_dir / str(row["archive_member"])).relative_to(self.root)
                        ).replace("\\", "/"),
                    }
                )
            except BaseException as error:
                if incumbent_retired:
                    if version_dir.exists():
                        shutil.rmtree(version_dir, ignore_errors=True)
                    os.rename(retired, version_dir)
                self.current_manifest_path().with_suffix(".json.tmp").unlink(missing_ok=True)
                raise ReceiptError(f"commit_failure: {error}") from error
            if incumbent_retired:
                shutil.rmtree(retired, ignore_errors=True)
        except ReceiptError:
            shutil.rmtree(staging, ignore_errors=True)
            raise
        except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as error:
            shutil.rmtree(staging, ignore_errors=True)
            raise ReceiptError(f"extraction_failure: {error}") from error

        return version_dir / str(row["archive_member"])

    # -- cleanup ---------------------------------------------------------------

    def cleanup(self, keep_version_dir: str) -> list[str]:
        """Remove superseded managed-DAP directories inside the exact family.

        Only `perl-dap-managed-` prefixed directories (and their `.tmp`
        staging siblings) are ever removed; language-server caches, other
        debuggers, user binaries, and unrelated state are untouchable.
        """
        removed: list[str] = []
        try:
            entries = sorted(self.root.iterdir())
        except OSError as error:
            raise ReceiptError(f"managed cache cleanup could not list root: {error}") from error
        for entry in entries:
            if not entry.name.startswith(DAP_MANAGED_PREFIX):
                continue
            if entry.name == keep_version_dir:
                continue
            if entry.is_dir():
                shutil.rmtree(entry)
            else:
                entry.unlink()
            removed.append(entry.name)
        return removed


def run_recovery_scenarios(root: Path) -> dict[str, Any]:
    """Run the deterministic known-good preservation suite in isolation.

    Returns the receipt `cache_recovery` block. The suite is offline: every
    archive is synthetic, no network is touched, and the only mutable state
    lives under `root`.
    """
    version = "9.9.9"
    target = "x86_64-unknown-linux-musl"
    package = f"perllsp-{version}-{target}"
    scenario_root = root / "cache-recovery"
    shutil.rmtree(scenario_root, ignore_errors=True)

    results: list[dict[str, Any]] = []

    def record(scenario: str, preserved: bool, detail: str) -> None:
        results.append(
            {"scenario": scenario, "known_good_preserved": preserved, "detail": detail}
        )

    # -- a complete verified candidate may become current ---------------------
    cache = ManagedDapCache(scenario_root / "selection")
    perl_dap = b"#!/bin/sh\necho perl-dap 9.9.9\n"
    archive = build_tar(
        scenario_root / "good.tar.gz",
        good_archive_members(version, target, perl_dap),
        executable={f"{package}/perl-dap"},
    )
    row = good_row(version, target, archive, _digest_bytes(perl_dap))
    installed = cache.install(row, {archive.name: [archive]}, version)
    selected = cache.current()
    complete_ok = (
        installed.is_file()
        and selected is not None
        and selected["member_sha256"] == row["member_sha256"]
        and selected["archive_member"] == row["archive_member"]
    )
    record(
        "complete_candidate_becomes_current",
        complete_ok,
        "a fully verified candidate is selected",
    )
    known_good = cache.current()

    def expect_preserved(
        scenario: str,
        defect_row: dict[str, Any],
        assets: dict[str, list[Path]],
        expect_fragment: str,
        verify_process: Callable[[Path], None] | None = None,
    ) -> None:
        before_selection = cache.current()
        before_snapshot = fingerprint(cache.root)
        try:
            cache.install(defect_row, assets, version, verify_process=verify_process)
            preserved = False
            detail = "defective candidate was accepted"
        except ReceiptError as error:
            message = str(error)
            staging_gone = not any(
                entry.name.endswith(".tmp") for entry in cache.root.iterdir()
            )
            preserved = (
                cache.current() == before_selection
                and fingerprint(cache.root) == before_snapshot
                and staging_gone
                and expect_fragment in message
            )
            detail = (
                message
                if expect_fragment in message
                else f"error did not name the defect: {message}"
            )
        record(scenario, preserved, detail)

    # -- release-index defects -------------------------------------------------
    expect_preserved("missing_asset_preserves_known_good", row, {}, "missing_asset")
    expect_preserved(
        "duplicate_asset_preserves_known_good",
        row,
        {archive.name: [archive, archive]},
        "duplicate_asset",
    )

    # -- wrong-target member inside a matching asset name ----------------------
    other_target = "aarch64-unknown-linux-musl"
    wrong_target_archive = build_tar(
        scenario_root / "wrong-target.tar.gz",
        good_archive_members(version, other_target, perl_dap),
        executable={f"perllsp-{version}-{other_target}/perl-dap"},
    )
    wrong_target_row = good_row(version, target, wrong_target_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "wrong_target_asset_preserves_known_good",
        wrong_target_row,
        {wrong_target_archive.name: [wrong_target_archive]},
        "ambiguous perl-dap member",
    )

    # -- a member-free archive: no executable ships at all -------------------
    no_member_archive = build_tar(
        scenario_root / "no-member.tar.gz", {f"{package}/README.md": b"docs only"}
    )
    no_member_row = good_row(version, target, no_member_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "missing_member_preserves_known_good",
        no_member_row,
        {no_member_archive.name: [no_member_archive]},
        "lacks required perl-dap member",
    )

    # -- wrong-product archive: only the perllsp member ships ------------------
    wrong_product_archive = build_tar(
        scenario_root / "wrong-product.tar.gz", {f"{package}/perllsp": b"language server only"}
    )
    wrong_product_row = good_row(version, target, wrong_product_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "wrong_product_member_preserves_known_good",
        wrong_product_row,
        {wrong_product_archive.name: [wrong_product_archive]},
        "lacks required perl-dap member",
    )

    # -- archive byte-level defects ---------------------------------------------
    digest_row = dict(row)
    digest_row["asset_digest"] = "sha256:" + "1" * 64
    expect_preserved(
        "digest_mismatch_preserves_known_good",
        digest_row,
        {archive.name: [archive]},
        "digest_mismatch",
    )

    member_row = dict(row)
    member_row["member_sha256"] = "sha256:" + "2" * 64
    expect_preserved(
        "member_digest_mismatch_preserves_known_good",
        member_row,
        {archive.name: [archive]},
        "member_digest_mismatch",
    )

    unsafe_archive = build_tar(
        scenario_root / "unsafe.tar.gz",
        {f"{package}/perl-dap": perl_dap, "../evil": b"traversal"},
    )
    unsafe_row = good_row(version, target, unsafe_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "unsafe_archive_path_preserves_known_good",
        unsafe_row,
        {unsafe_archive.name: [unsafe_archive]},
        "unsafe archive member path",
    )

    foreign_archive = build_tar(
        scenario_root / "foreign-exec.tar.gz",
        {
            f"{package}/perl-dap": perl_dap,
            f"{package}/payload": b"#!/bin/sh\n",
        },
        executable={f"{package}/perl-dap", f"{package}/payload"},
    )
    foreign_row = good_row(version, target, foreign_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "foreign_executable_member_preserves_known_good",
        foreign_row,
        {foreign_archive.name: [foreign_archive]},
        "unexpected executable member",
    )

    duplicate_buffer = io.BytesIO()
    with tarfile.open(fileobj=duplicate_buffer, mode="w:gz") as tar_writer:
        for _ in range(2):
            info = tarfile.TarInfo(f"{package}/perl-dap")
            info.size = len(perl_dap)
            info.mode = 0o755
            tar_writer.addfile(info, io.BytesIO(perl_dap))
    duplicate_archive = scenario_root / "duplicate-member.tar.gz"
    duplicate_archive.write_bytes(duplicate_buffer.getvalue())
    duplicate_row = good_row(version, target, duplicate_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "duplicate_member_preserves_known_good",
        duplicate_row,
        {duplicate_archive.name: [duplicate_archive]},
        "duplicate archive member",
    )

    # -- partial download --------------------------------------------------------
    partial = scenario_root / "partial.tar.gz.partial"
    partial.write_bytes(b"truncated")
    partial_row = dict(row)
    partial_row["asset_name"] = partial.name
    expect_preserved(
        "partial_download_preserves_known_good",
        partial_row,
        {partial.name: [partial]},
        "partial_download",
    )

    # -- extraction failure: digest-consistent corrupt archive -------------------
    corrupt_archive = scenario_root / "corrupt.tar.gz"
    corrupt_archive.write_bytes(b"not a tar.gz at all")
    corrupt_row = good_row(version, target, corrupt_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "extraction_failure_preserves_known_good",
        corrupt_row,
        {corrupt_archive.name: [corrupt_archive]},
        "malformed tar.gz archive",
    )

    # -- launch and protocol gates (matching-host process boundary) ---------------
    def failing_launch(binary: Path) -> None:
        raise ReceiptError("launch_failure: perl-dap --version exited 1")

    def impure_protocol(binary: Path) -> None:
        from .framing import lsp_frame, parse_lsp_frames

        message = {"seq": 1, "type": "response", "command": "initialize", "success": True}
        try:
            parse_lsp_frames(lsp_frame(message) + b"stray stdout bytes")
        except ReceiptError as error:
            raise ReceiptError(f"protocol_impurity: {error}") from error

    gate_archive = build_tar(
        scenario_root / "gated.tar.gz",
        good_archive_members(version, target, perl_dap),
        executable={f"{package}/perl-dap"},
    )
    gate_row = good_row(version, target, gate_archive, _digest_bytes(perl_dap))
    expect_preserved(
        "launch_failure_preserves_known_good",
        gate_row,
        {gate_archive.name: [gate_archive]},
        "launch_failure",
        verify_process=failing_launch,
    )
    expect_preserved(
        "protocol_impurity_preserves_known_good",
        gate_row,
        {gate_archive.name: [gate_archive]},
        "protocol_impurity",
        verify_process=impure_protocol,
    )

    # -- promote and manifest commit failures: the incumbent must roll back --
    before_selection = cache.current()
    before_snapshot = fingerprint(cache.root)

    def expect_commit_preserved(scenario: str, manifest_failure: bool) -> None:
        try:
            cache.install(
                gate_row,
                {gate_archive.name: [gate_archive]},
                version,
                simulate_promote_failure=not manifest_failure,
                simulate_manifest_failure=manifest_failure,
            )
            preserved = False
            detail = "commit failure was accepted"
        except ReceiptError as error:
            message = str(error)
            incumbent_intact = (
                cache.current() == before_selection
                and fingerprint(cache.root) == before_snapshot
                and (scenario_root / "selection" / f"{DAP_MANAGED_PREFIX}{version}-{target}").is_dir()
            )
            preserved = "commit_failure" in message and incumbent_intact
            detail = message
        record(scenario, preserved, detail)

    expect_commit_preserved("promote_failure_preserves_known_good", manifest_failure=False)
    expect_commit_preserved("manifest_failure_preserves_known_good", manifest_failure=True)

    # -- cleanup boundary -----------------------------------------------------------
    cleanup_root = scenario_root / "cleanup"
    cleanup_root.mkdir(parents=True)
    keep = f"{DAP_MANAGED_PREFIX}{version}-{target}"
    protected = {
        "perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp": b"language-server cache",
        "perl-lsp-0.15.0/perl-lsp": b"retired language-server cache",
        "perl-debug-adapter-managed-0.9.0/perl-dap": b"another debugger family",
        "user-bin/perl-dap": b"user binary",
        "unrelated/state.json": b"unrelated cache state",
    }
    for relative, payload in protected.items():
        path = cleanup_root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
    for stale in (f"{DAP_MANAGED_PREFIX}0.16.9-{target}", f"{keep}.tmp"):
        (cleanup_root / stale).mkdir(parents=True, exist_ok=True)
        (cleanup_root / stale / "perl-dap").write_bytes(b"stale")
    (cleanup_root / keep).mkdir(parents=True, exist_ok=True)
    (cleanup_root / keep / "perl-dap").write_bytes(b"current")

    before_fingerprint = fingerprint(cleanup_root)
    removed = ManagedDapCache(cleanup_root).cleanup(keep)
    after_fingerprint = fingerprint(cleanup_root)
    expected_removed = {f"{DAP_MANAGED_PREFIX}0.16.9-{target}", f"{keep}.tmp"}
    protected_keys = {
        key for key in before_fingerprint if not key.startswith(DAP_MANAGED_PREFIX)
    }
    cleanup_ok = (
        set(removed) == expected_removed
        and all(key in after_fingerprint for key in protected_keys)
        and before_fingerprint[f"{keep}/perl-dap"] == after_fingerprint[f"{keep}/perl-dap"]
    )
    record(
        "cleanup_stays_inside_the_perl_dap_family",
        cleanup_ok,
        "cleanup removed exactly the superseded managed-DAP entries",
    )

    all_passed = all(entry["known_good_preserved"] for entry in results)
    return {
        "result": "pass" if all_passed else "fail",
        "known_good_before": known_good,
        "selected_after": cache.current(),
        "scenario_results": results,
        "limitations": [
            "Offline synthetic archives: the suite proves the managed-DAP cache "
            "boundary and preservation semantics, not live GitHub release state.",
        ],
    }
