#!/usr/bin/env python3
"""Deterministic adversarial tar.gz/zip fixtures for standalone archive safety.

Fixtures are generated at test time so accidental extraction outside the
harness stays small and non-payload. Expected verdicts are authored here,
not copied from the installer.
"""

from __future__ import annotations

import argparse
import io
import os
import stat
import tarfile
import zipfile
from pathlib import Path
from typing import Callable

PACKAGE = "perllsp-0.18.0-x86_64-unknown-linux-gnu"
WIN_PACKAGE = "perllsp-0.18.0-x86_64-pc-windows-msvc"

POSIX_FILES = {
    "perllsp": b"posix-server\n",
    "perl-dap": b"posix-dap\n",
    "README.md": b"readme\n",
    "LICENSE-APACHE": b"apache\n",
    "LICENSE-MIT": b"mit\n",
    "SHA256SUMS.txt": b"sums\n",
}
WIN_FILES = {
    "perllsp.exe": b"win-server\n",
    "perl-dap.exe": b"win-dap\n",
    "README.md": b"readme\n",
    "LICENSE-APACHE": b"apache\n",
    "LICENSE-MIT": b"mit\n",
    "SHA256SUMS.txt": b"sums\n",
}


def _posix_tar(builder: Callable[[tarfile.TarFile], None], dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(dest, mode="w:gz") as archive:
        builder(archive)


def _add_reg(
    archive: tarfile.TarFile,
    name: str,
    data: bytes,
    mode: int = 0o644,
) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = mode
    info.type = tarfile.REGTYPE
    archive.addfile(info, io.BytesIO(data))


def _add_dir(archive: tarfile.TarFile, name: str) -> None:
    info = tarfile.TarInfo(name if name.endswith("/") else name + "/")
    info.type = tarfile.DIRTYPE
    info.mode = 0o755
    archive.addfile(info)


def valid_posix(archive: tarfile.TarFile) -> None:
    _add_dir(archive, PACKAGE)
    _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
    _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
    for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])


def traversal_parent(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/../../sentinel_pwned", b"escaped\n")


def absolute_path(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, "/tmp/sentinel_pwned", b"escaped\n")


def windows_drive(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, "C:/Windows/sentinel_pwned", b"escaped\n")


def backslash_separator(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, rf"{PACKAGE}\extra.txt", b"alias\n")


def empty_component(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}//extra.txt", b"empty\n")


def symlink_entry(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    info = tarfile.TarInfo(f"{PACKAGE}/link")
    info.type = tarfile.SYMTYPE
    info.linkname = "perllsp"
    archive.addfile(info)


def hardlink_entry(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    info = tarfile.TarInfo(f"{PACKAGE}/hard")
    info.type = tarfile.LNKTYPE
    info.linkname = f"{PACKAGE}/perllsp"
    archive.addfile(info)


def hardlink_topology_member(archive: tarfile.TarFile) -> None:
    """A hardlink wearing an accepted topology name (#11508).

    ``hardlink_entry`` names its link ``hard``, so the unexpected-member rule
    rejects it before the link rule is ever consulted. This case supplies
    ``SHA256SUMS.txt`` itself as a hardlink, so only a real type check can
    reject it. BusyBox ``tar -tv`` renders a hardlink with a regular-file type
    char, which is why entry type must come from the header, not the listing.
    """
    _add_dir(archive, PACKAGE)
    _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
    _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
    for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])
    info = tarfile.TarInfo(f"{PACKAGE}/SHA256SUMS.txt")
    info.type = tarfile.LNKTYPE
    info.linkname = f"{PACKAGE}/perllsp"
    info.mode = 0o644
    archive.addfile(info)


def absolute_topology_member(archive: tarfile.TarFile) -> None:
    """The server binary delivered at an absolute archive path (#11508).

    ``absolute_path`` adds an escape member alongside a complete valid
    topology, so it is rejected as outside the package directory. Here the
    absolute member *is* the topology's ``perllsp``: BusyBox ``tar -t`` strips
    the leading ``/`` before printing, so a listing-derived name reads as the
    canonical member and stages substituted content.
    """
    _add_dir(archive, PACKAGE)
    _add_reg(archive, f"/{PACKAGE}/perllsp", b"smuggled-server\n", 0o755)
    _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
    for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])


def newline_in_member_name(archive: tarfile.TarFile) -> None:
    """A stored name carrying a raw newline (#11508).

    A line-oriented listing splits this member across two lines, desynchronizing
    any pairing of `tar -t` names with `tar -tv` type chars. Reading the header
    name field makes it a single nonportable name instead.
    """
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/ev\nil", b"split\n")


def extended_pax_header(archive: tarfile.TarFile) -> None:
    """A PAX extended header record ahead of a topology member (#11508).

    ``x`` and ``g`` records can rewrite the path of the entry that follows
    them, so the header walk must fail closed on them rather than skip them.
    """
    _add_dir(archive, PACKAGE)
    info = tarfile.TarInfo(f"{PACKAGE}/perllsp")
    info.size = len(POSIX_FILES["perllsp"])
    info.mode = 0o755
    info.type = tarfile.REGTYPE
    # An oversized uid cannot be represented in the base header, so tarfile
    # emits a preceding `x` record for it.
    info.uid = 0o77777777777
    archive.addfile(info, io.BytesIO(POSIX_FILES["perllsp"]))
    _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
    for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])


def fifo_entry(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    info = tarfile.TarInfo(f"{PACKAGE}/pipe")
    info.type = tarfile.FIFOTYPE
    info.mode = 0o644
    archive.addfile(info)


def duplicate_path(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/README.md", b"second\n")


def case_collision(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/Readme.md", b"case\n")


def missing_dap(archive: tarfile.TarFile) -> None:
    _add_dir(archive, PACKAGE)
    _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
    for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])


def duplicate_server(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/perl-lsp", b"alias\n", 0o755)


def extra_executable(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/helper.sh", b"#!/bin/sh\n", 0o755)


def reserved_device_name(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/CON.txt", b"reserved\n")


def trailing_dot(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    _add_reg(archive, f"{PACKAGE}/README.md.", b"trailing\n")


def too_many_entries(archive: tarfile.TarFile) -> None:
    valid_posix(archive)
    for index in range(40):
        _add_reg(archive, f"{PACKAGE}/extra-{index}.txt", b"x\n")


def oversized_entry(archive: tarfile.TarFile) -> None:
    # Unique topology: one required member over the test ceiling. Do not
    # append a second README.md — that is duplicate_path, and a GNU-biased
    # tar -tv size parse used to fail closed on uid/nlink instead.
    _add_dir(archive, PACKAGE)
    _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
    _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
    _add_reg(archive, f"{PACKAGE}/README.md", b"x" * 64)
    for name in ("LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
        _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])


def truncated_garbage(dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(b"not-an-archive\n")


def corrupt_header_checksum(dest: Path) -> None:
    """A well-formed gzip stream whose second tar header no longer checksums.

    Decompression succeeds, so this reaches the header walk rather than the
    gzip guard, and only the header checksum can reject it (#11508).
    """
    import gzip as _gzip

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        valid_posix(archive)
    blocks = bytearray(raw.getvalue())
    # Flip a byte inside the second header's name field, leaving its recorded
    # checksum stale.
    blocks[512] ^= 0x20
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(_gzip.compress(bytes(blocks)))


def _zip_write(dest: Path, entries: list[tuple[str, bytes, bool]]) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dest, mode="w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data, executable in entries:
            info = zipfile.ZipInfo(filename=name)
            mode = 0o755 if executable else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            archive.writestr(info, data)


def valid_windows_flat(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe")) for name, data in WIN_FILES.items()
    ]
    _zip_write(dest, entries)


def valid_windows_nested(dest: Path) -> None:
    entries = [
        (f"{WIN_PACKAGE}/{name}", data, name.endswith(".exe"))
        for name, data in WIN_FILES.items()
    ]
    _zip_write(dest, entries)


def windows_traversal(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe")) for name, data in WIN_FILES.items()
    ]
    entries.append(("../sentinel_pwned", b"escaped\n", False))
    _zip_write(dest, entries)


def windows_absolute(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe")) for name, data in WIN_FILES.items()
    ]
    entries.append(("/tmp/sentinel_pwned", b"escaped\n", False))
    _zip_write(dest, entries)


def windows_symlink(dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dest, mode="w") as archive:
        for name, data in WIN_FILES.items():
            info = zipfile.ZipInfo(filename=name)
            mode = 0o755 if name.endswith(".exe") else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            archive.writestr(info, data)
        link = zipfile.ZipInfo(filename="link")
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        archive.writestr(link, "perllsp.exe")


def windows_duplicate(dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dest, mode="w") as archive:
        for name, data in WIN_FILES.items():
            info = zipfile.ZipInfo(filename=name)
            mode = 0o755 if name.endswith(".exe") else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            archive.writestr(info, data)
        extra = zipfile.ZipInfo(filename="README.md")
        extra.external_attr = (stat.S_IFREG | 0o644) << 16
        archive.writestr(extra, b"second\n")


def windows_case_collision(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe")) for name, data in WIN_FILES.items()
    ]
    entries.append(("Readme.md", b"case\n", False))
    _zip_write(dest, entries)


def windows_missing_dap(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe"))
        for name, data in WIN_FILES.items()
        if name != "perl-dap.exe"
    ]
    _zip_write(dest, entries)


def windows_extra_executable(dest: Path) -> None:
    entries = [
        (name, data, name.endswith(".exe")) for name, data in WIN_FILES.items()
    ]
    entries.append(("helper.bat", b"echo hi\n", True))
    _zip_write(dest, entries)


TAR_CASES: dict[str, Callable[[tarfile.TarFile], None]] = {
    "valid_posix": valid_posix,
    "traversal_parent": traversal_parent,
    "absolute_path": absolute_path,
    "windows_drive": windows_drive,
    "backslash_separator": backslash_separator,
    "empty_component": empty_component,
    "symlink_entry": symlink_entry,
    "hardlink_entry": hardlink_entry,
    "hardlink_topology_member": hardlink_topology_member,
    "absolute_topology_member": absolute_topology_member,
    "newline_in_member_name": newline_in_member_name,
    "extended_pax_header": extended_pax_header,
    "fifo_entry": fifo_entry,
    "duplicate_path": duplicate_path,
    "case_collision": case_collision,
    "missing_dap": missing_dap,
    "duplicate_server": duplicate_server,
    "extra_executable": extra_executable,
    "reserved_device_name": reserved_device_name,
    "trailing_dot": trailing_dot,
    "too_many_entries": too_many_entries,
    "oversized_entry": oversized_entry,
}

ZIP_CASES: dict[str, Callable[[Path], None]] = {
    "valid_windows_flat": valid_windows_flat,
    "valid_windows_nested": valid_windows_nested,
    "windows_traversal": windows_traversal,
    "windows_absolute": windows_absolute,
    "windows_symlink": windows_symlink,
    "windows_duplicate": windows_duplicate,
    "windows_case_collision": windows_case_collision,
    "windows_missing_dap": windows_missing_dap,
    "windows_extra_executable": windows_extra_executable,
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    dest = Path(args.out)
    if args.case == "truncated_garbage":
        truncated_garbage(dest)
        return 0
    if args.case == "corrupt_header_checksum":
        corrupt_header_checksum(dest)
        return 0
    if args.case in TAR_CASES:
        _posix_tar(TAR_CASES[args.case], dest)
        return 0
    if args.case in ZIP_CASES:
        ZIP_CASES[args.case](dest)
        return 0
    raise SystemExit(f"unknown fixture case: {args.case}")


if __name__ == "__main__":
    os.umask(0o022)
    raise SystemExit(main())
