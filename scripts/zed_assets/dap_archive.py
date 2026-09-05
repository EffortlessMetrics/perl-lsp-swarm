"""Safe shared-family archive inspection and exact perl-dap member extraction.

The canonical release archives legitimately carry both products (`perllsp` and
`perl-dap`) inside one `perllsp-{version}-{triple}` directory, so the DAP scan
accepts exactly those known binary names and rejects any other executable
member, any ambiguous second `perl-dap` location, and any unsafe member shape.
"""

from __future__ import annotations

import shutil
import stat
import tarfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any

from .common import ReceiptError, sha256_bytes, validate_relative_member

SHARED_BINARY_NAMES = {"perllsp", "perllsp.exe", "perl-dap", "perl-dap.exe"}
ARCHIVE_SUMS_MEMBER_SUFFIX = "/SHA256SUMS.txt"
WINDOWS_EXECUTABLE_SUFFIXES = (".exe", ".bat", ".cmd")


def _is_known_binary(name: str) -> bool:
    return PurePosixPath(name.replace("\\", "/")).name.lower() in SHARED_BINARY_NAMES


def _reject_foreign_executable(normalized: str, mode: int | None) -> None:
    """Reject any executable member outside the permitted shared-family binaries.

    A foreign payload that merely carries a different name is not enough to
    trust it: an archive is only `safe` when nothing executable ships except
    the two known products. On tar members the executable mode bits are the
    authority; on zip members the Windows executable suffixes and the stored
    external mode both count.
    """
    basename = PurePosixPath(normalized.replace("\\", "/")).name.lower()
    suffix_executable = basename.endswith(WINDOWS_EXECUTABLE_SUFFIXES)
    mode_executable = mode is not None and mode & 0o111 != 0
    if (suffix_executable or mode_executable) and basename not in SHARED_BINARY_NAMES:
        raise ReceiptError(
            f"unexpected executable member: {normalized!r} "
            f"(mode {mode:o}) is outside the shared perllsp/perl-dap family"
        )


def _zip_is_symlink(info: zipfile.ZipInfo) -> bool:
    mode = (info.external_attr >> 16) & 0xFFFF
    return stat.S_IFMT(mode) == stat.S_IFLNK


def _check_member(
    normalized: str,
    seen: set[str],
    expected_member: str,
) -> None:
    if normalized in seen:
        raise ReceiptError(f"duplicate archive member: {normalized}")
    seen.add(normalized)
    if _is_known_binary(normalized) and normalized != expected_member:
        if PurePosixPath(normalized).name.lower() in {"perl-dap", "perl-dap.exe"}:
            raise ReceiptError(
                f"ambiguous perl-dap member: {normalized!r} does not match the expected "
                f"member {expected_member!r}"
            )
        # The perllsp sibling member is legitimate in the shared family; any
        # other known-binary position is not. Both binaries live in the
        # perllsp-{version}-{triple} directory only.
        prefix = expected_member.rsplit("/", 1)[0]
        if not normalized.startswith(f"{prefix}/"):
            raise ReceiptError(f"unexpected shared-family binary location: {normalized}")


def inspect_tar(path: Path, expected_member: str) -> list[str]:
    try:
        with tarfile.open(path, mode="r:gz") as archive:
            names: list[str] = []
            seen: set[str] = set()
            selected = False
            for member in archive.getmembers():
                normalized = str(validate_relative_member(member.name))
                _check_member(normalized, seen, expected_member)
                if member.issym() or member.islnk():
                    raise ReceiptError(f"archive links are not accepted: {normalized}")
                if member.isfile():
                    _reject_foreign_executable(normalized, member.mode)
                if normalized == expected_member:
                    if member.name != expected_member:
                        raise ReceiptError(
                            f"required member has a noncanonical archive name: {member.name!r}"
                        )
                    if not member.isfile():
                        raise ReceiptError("required perl-dap member is not a regular file")
                    selected = True
                names.append(normalized)
            if not selected:
                raise ReceiptError(f"archive lacks required perl-dap member {expected_member!r}")
            return names
    except tarfile.TarError as error:
        raise ReceiptError(f"malformed tar.gz archive {path.name}: {error}") from error


def inspect_zip(path: Path, expected_member: str) -> list[str]:
    try:
        with zipfile.ZipFile(path) as archive:
            names: list[str] = []
            seen: set[str] = set()
            selected = False
            for info in archive.infolist():
                normalized = str(validate_relative_member(info.filename.rstrip("/")))
                _check_member(normalized, seen, expected_member)
                if _zip_is_symlink(info):
                    raise ReceiptError(f"archive symlink is not accepted: {normalized}")
                if not info.is_dir():
                    _reject_foreign_executable(normalized, (info.external_attr >> 16) & 0xFFFF)
                if normalized == expected_member:
                    if info.filename.rstrip("/") != expected_member:
                        raise ReceiptError(
                            f"required member has a noncanonical archive name: {info.filename!r}"
                        )
                    if info.is_dir():
                        raise ReceiptError("required perl-dap member is a directory")
                    selected = True
                names.append(normalized)
            if not selected:
                raise ReceiptError(f"archive lacks required perl-dap member {expected_member!r}")
            return names
    except zipfile.BadZipFile as error:
        raise ReceiptError(f"malformed zip archive {path.name}: {error}") from error


def inspect_archive(path: Path, archive_type: str, expected_member: str) -> list[str]:
    if archive_type == "tar.gz":
        return inspect_tar(path, expected_member)
    if archive_type == "zip":
        return inspect_zip(path, expected_member)
    raise ReceiptError(f"unsupported archive type: {archive_type}")


def _archive_sums(
    path: Path,
    archive_type: str,
    names: list[str],
    package_dir: str,
) -> dict[str, str] | None:
    """Parse the in-archive SHA256SUMS.txt when the family carries one.

    The sums file lists bare binary names with their digests; it is an
    authority inside the downloaded bytes themselves, independent of both the
    checked contract and the release-level consolidated SHA256SUMS asset.
    """
    sums_member = f"{package_dir}/SHA256SUMS.txt"
    if sums_member not in names:
        return None
    try:
        if archive_type == "tar.gz":
            with tarfile.open(path, mode="r:gz") as archive:
                payload = archive.extractfile(sums_member)
                if payload is None:
                    return None
                text = payload.read().decode("utf-8", errors="strict")
        else:
            with zipfile.ZipFile(path) as archive:
                text = archive.read(sums_member).decode("utf-8", errors="strict")
    except (tarfile.TarError, zipfile.BadZipFile, UnicodeDecodeError, OSError) as error:
        raise ReceiptError(f"archive checksum manifest is unreadable: {error}") from error
    sums: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        digest, separator, name = line.replace("*", " ").partition(" ")
        if not separator or not name.strip():
            raise ReceiptError(f"archive checksum manifest has a malformed line: {line!r}")
        sums[name.strip()] = f"sha256:{digest.strip()}"
    return sums or None


def extract_expected_member(
    archive_path: Path,
    archive_type: str,
    expected_member: str,
    destination: Path,
    make_executable: bool,
) -> tuple[Path, str, dict[str, str] | None]:
    """Extract exactly the expected perl-dap member; never the whole archive.

    Returns the extracted binary, the sorted member-list digest, and the
    in-archive binary checksum map when present.
    """
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / PurePosixPath(expected_member).name
    output.unlink(missing_ok=True)

    names = inspect_archive(archive_path, archive_type, expected_member)
    package_dir = expected_member.rsplit("/", 1)[0]
    sums = _archive_sums(archive_path, archive_type, names, package_dir)

    try:
        if archive_type == "tar.gz":
            with tarfile.open(archive_path, mode="r:gz") as archive:
                source = archive.extractfile(archive.getmember(expected_member))
                if source is None:
                    raise ReceiptError("required tar member could not be opened")
                with source, output.open("wb") as target:
                    shutil.copyfileobj(source, target)
        else:
            with zipfile.ZipFile(archive_path) as archive:
                with archive.open(expected_member, "r") as source, output.open("wb") as target:
                    shutil.copyfileobj(source, target)
    except tarfile.TarError as error:
        raise ReceiptError(f"malformed tar.gz archive {archive_path.name}: {error}") from error
    except zipfile.BadZipFile as error:
        raise ReceiptError(f"malformed zip archive {archive_path.name}: {error}") from error

    if make_executable:
        output.chmod(output.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    members_digest = sha256_bytes(("\n".join(sorted(names)) + "\n").encode("utf-8"))
    return output, members_digest, sums


def verify_member_against_archive_sums(
    sums: dict[str, str] | None,
    binary: Path,
    member_sha256: str,
) -> str | None:
    """Cross-check the extracted member digest against the in-archive sums.

    Returns the in-archive recorded digest for the receipt when the archive
    carries one; a disagreement with the independently computed digest fails
    closed.
    """
    if sums is None:
        return None
    from .common import sha256_file

    recorded = sums.get(binary.name)
    if recorded is None:
        raise ReceiptError(
            f"archive checksum manifest does not list the required member {binary.name!r}"
        )
    actual = sha256_file(binary)
    if actual != recorded:
        raise ReceiptError(
            "extracted perl-dap member disagrees with the in-archive checksum manifest: "
            f"manifest {recorded}, extracted {actual}"
        )
    if recorded != member_sha256:
        raise ReceiptError(
            "in-archive checksum manifest disagrees with the checked contract member digest: "
            f"manifest {recorded}, contract {member_sha256}"
        )
    return recorded
