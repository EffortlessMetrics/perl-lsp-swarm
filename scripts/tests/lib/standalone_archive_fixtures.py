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


def _ustar_header(name: str, size: int, mode: int, typeflag: bytes) -> bytearray:
    """One 512-byte POSIX ustar header block with a correct checksum."""
    header = bytearray(512)
    encoded = name.encode()
    header[0 : len(encoded)] = encoded
    header[100:108] = f"{mode:07o}\0".encode()
    header[108:116] = b"0000000\0"
    header[116:124] = b"0000000\0"
    header[124:136] = f"{size:011o}\0".encode()
    header[136:148] = b"00000000000\0"
    header[156:157] = typeflag
    header[257:263] = b"ustar\0"
    header[263:265] = b"00"
    header[148:156] = b" " * 8
    header[148:156] = f"{sum(header) & 0o7777777:06o}\0 ".encode()
    return header


def extended_pax_header(dest: Path) -> None:
    """A PAX ``x`` record that renames the entry following it (#11508).

    The hazard is identity rewriting: an ``x`` record's ``path`` key overrides
    the ustar name of the next entry, so a classifier that skips the record
    inspects one name while ``tar`` extracts under another. Here the topology's
    ``SHA256SUMS.txt`` is delivered only through such an override, over a ustar
    header that names ``decoy``.

    The override is small and extraction-safe on purpose. An earlier version
    forced the ``x`` record with an oversized uid, which real ``tar`` refused
    during extraction — so the case passed even when the classifier was mutated
    to skip extended records, proving nothing about the classifier itself.
    """
    import gzip as _gzip

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        _add_dir(archive, PACKAGE)
        _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
        _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
        for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT"):
            _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])
    body = bytes(raw.getvalue()).rstrip(b"\0")
    body += b"\0" * ((-len(body)) % 512)

    # PAX record: "<len> path=<value>\n", where <len> counts itself.
    value = f"path={PACKAGE}/SHA256SUMS.txt\n"
    length = len(value) + len(str(len(value))) + 1
    if len(str(length)) != len(str(len(value))):
        length += 1
    record = f"{length} {value}".encode()

    out = bytearray(body)
    out += _ustar_header("PaxHeaders/SHA256SUMS.txt", len(record), 0o644, b"x")
    out += record + b"\0" * ((-len(record)) % 512)
    payload = POSIX_FILES["SHA256SUMS.txt"]
    out += _ustar_header(f"{PACKAGE}/decoy", len(payload), 0o644, b"0")
    out += payload + b"\0" * ((-len(payload)) % 512)
    out += b"\0" * 1024
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(_gzip.compress(bytes(out)))


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


def sized_directory_entry(dest: Path) -> None:
    """A directory entry declaring a nonzero size (#11508).

    POSIX requires a zero size on types that carry no data. A walker that
    trusts the typeflag alone lets this entry's phantom data block swallow the
    header that follows it, so the walker and a conformant tar reader disagree
    about which entries the archive holds.
    """
    import gzip as _gzip

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo(PACKAGE + "/")
        info.type = tarfile.DIRTYPE
        info.mode = 0o755
        info.size = 512
        archive.addfile(info, io.BytesIO(b"\0" * 512))
        _add_reg(archive, f"{PACKAGE}/perllsp", POSIX_FILES["perllsp"], 0o755)
        _add_reg(archive, f"{PACKAGE}/perl-dap", POSIX_FILES["perl-dap"], 0o755)
        for name in ("README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"):
            _add_reg(archive, f"{PACKAGE}/{name}", POSIX_FILES[name])
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(_gzip.compress(raw.getvalue()))


def sparse_entry(dest: Path) -> None:
    """A GNU sparse member (typeflag ``S``) carrying real stored data (#11508).

    A sparse header's size field counts only the stored bytes and a long
    sparse map continues into further blocks, so the entry's extent is not
    derivable from the header alone. It must be refused by name rather than
    treated as a dataless type that wrongly declared a size.
    """
    import gzip as _gzip

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        valid_posix(archive)
    blocks = bytearray(raw.getvalue())

    header = bytearray(512)
    name = f"{PACKAGE}/sparse".encode()
    header[0 : len(name)] = name
    header[100:108] = b"0000644\0"
    header[108:116] = b"0000000\0"
    header[116:124] = b"0000000\0"
    header[124:136] = b"00000003400\0"  # 1792 stored bytes
    header[136:148] = b"00000000000\0"
    header[156] = ord("S")
    header[257:265] = b"ustar  \0"  # GNU magic+version
    header[148:156] = b" " * 8
    checksum = sum(header) & 0o7777777
    header[148:156] = (f"{checksum:06o}\0 ").encode()

    # Splice the sparse header (plus its stored data blocks) ahead of the
    # end-of-archive marker so the walk reaches it.
    body = bytes(blocks).rstrip(b"\0")
    pad = (-len(body)) % 512
    out = bytearray(body + b"\0" * pad)
    out += header
    out += b"\0" * 1792
    out += b"\0" * 1024
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(_gzip.compress(bytes(out)))


def truncated_garbage(dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(b"not-an-archive\n")


def corrupt_header_checksum(dest: Path) -> None:
    """A well-formed gzip stream whose second tar header no longer checksums.

    Decompression succeeds, so this reaches the header walk rather than the
    gzip guard. The corrupted field is one the classifier never reads, so no
    path, type, or membership rule can catch it and only the header checksum
    can reject it (#11508).
    """
    import gzip as _gzip

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        valid_posix(archive)
    blocks = bytearray(raw.getvalue())
    # Flip a byte in the second header's mtime field (offset 136..147), which
    # the classifier never reads. Corrupting the name field instead would be
    # caught by the ordinary path-membership rule, so the fixture would pass
    # even with checksum verification disabled and would not isolate it.
    blocks[512 + 136] ^= 0x01
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
    if args.case == "sized_directory_entry":
        sized_directory_entry(dest)
        return 0
    if args.case == "sparse_entry":
        sparse_entry(dest)
        return 0
    if args.case == "extended_pax_header":
        extended_pax_header(dest)
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
