"""Safe archive inspection and exact-member extraction."""

from __future__ import annotations

import shutil
import stat
import tarfile
import zipfile
from pathlib import Path, PurePosixPath

from .common import ReceiptError, sha256_bytes, validate_relative_member


def _zip_is_symlink(info: zipfile.ZipInfo) -> bool:
    mode = (info.external_attr >> 16) & 0xFFFF
    return stat.S_IFMT(mode) == stat.S_IFLNK


def _candidate_executable(name: str) -> bool:
    return PurePosixPath(name.replace("\\", "/")).name.lower() in {
        "perllsp",
        "perllsp.exe",
        "perl-lsp",
        "perl-lsp.exe",
    }


def inspect_tar(path: Path, expected_member: str) -> list[str]:
    with tarfile.open(path, mode="r:gz") as archive:
        names: list[str] = []
        seen: set[str] = set()
        selected = False
        for member in archive.getmembers():
            normalized = str(validate_relative_member(member.name))
            if normalized in seen:
                raise ReceiptError(f"duplicate archive member: {normalized}")
            seen.add(normalized)
            names.append(normalized)
            if member.issym() or member.islnk():
                raise ReceiptError(f"archive links are not accepted: {normalized}")
            if _candidate_executable(normalized) and normalized != expected_member:
                raise ReceiptError(f"unexpected code-intelligence executable: {normalized}")
            if normalized == expected_member:
                if not member.isfile():
                    raise ReceiptError("required perllsp member is not a regular file")
                selected = True
        if not selected:
            raise ReceiptError(f"archive lacks required member {expected_member!r}")
        return names


def inspect_zip(path: Path, expected_member: str) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        names: list[str] = []
        seen: set[str] = set()
        selected = False
        for info in archive.infolist():
            normalized = str(validate_relative_member(info.filename.rstrip("/")))
            if normalized in seen:
                raise ReceiptError(f"duplicate archive member: {normalized}")
            seen.add(normalized)
            names.append(normalized)
            if _zip_is_symlink(info):
                raise ReceiptError(f"archive symlink is not accepted: {normalized}")
            if _candidate_executable(normalized) and normalized != expected_member:
                raise ReceiptError(f"unexpected code-intelligence executable: {normalized}")
            if normalized == expected_member:
                if info.is_dir():
                    raise ReceiptError("required perllsp member is a directory")
                selected = True
        if not selected:
            raise ReceiptError(f"archive lacks required member {expected_member!r}")
        return names


def extract_expected(
    archive_path: Path,
    archive_type: str,
    expected_member: str,
    destination: Path,
    make_executable: bool,
) -> tuple[Path, str]:
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / PurePosixPath(expected_member).name
    output.unlink(missing_ok=True)

    if archive_type == "tar.gz":
        names = inspect_tar(archive_path, expected_member)
        with tarfile.open(archive_path, mode="r:gz") as archive:
            source = archive.extractfile(archive.getmember(expected_member))
            if source is None:
                raise ReceiptError("required tar member could not be opened")
            with source, output.open("wb") as target:
                shutil.copyfileobj(source, target)
    elif archive_type == "zip":
        names = inspect_zip(archive_path, expected_member)
        with zipfile.ZipFile(archive_path) as archive:
            with archive.open(expected_member, "r") as source, output.open("wb") as target:
                shutil.copyfileobj(source, target)
    else:
        raise ReceiptError(f"unsupported archive type: {archive_type}")

    if make_executable:
        output.chmod(output.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    members_digest = sha256_bytes(("\n".join(sorted(names)) + "\n").encode("utf-8"))
    return output, members_digest
