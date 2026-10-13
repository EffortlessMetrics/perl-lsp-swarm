"""Fail-closed, streaming access to one regular member of a release archive."""

from __future__ import annotations

import hashlib
import stat
import tarfile
import zipfile
from contextlib import contextmanager
from pathlib import Path
from typing import BinaryIO


CHUNK_SIZE = 1 << 20


class ArchiveMemberError(ValueError):
    """The selected archive member is missing, ambiguous, or unsafe."""


def _regular_zip(info: zipfile.ZipInfo, name: str) -> None:
    if info.filename.endswith("/"):
        raise ArchiveMemberError(f"archive member is not a regular file: {name}")
    if info.external_attr & 0x10:
        raise ArchiveMemberError(f"archive member is not a regular file: {name}")
    mode = (info.external_attr >> 16) & 0xFFFF
    if stat.S_IFMT(mode) not in (0, stat.S_IFREG):
        raise ArchiveMemberError(f"archive member is not a regular file: {name}")


def _digest(handle: BinaryIO) -> str:
    value = hashlib.sha256()
    for chunk in iter(lambda: handle.read(CHUNK_SIZE), b""):
        value.update(chunk)
    return value.hexdigest()


@contextmanager
def _selected_handle(archive: Path, name: str):
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            matches = [info for info in bundle.infolist() if info.filename == name]
            if not matches:
                raise ArchiveMemberError(f"archive member is missing: {name}")
            if len(matches) != 1:
                raise ArchiveMemberError(f"archive has duplicate members: {name}")
            _regular_zip(matches[0], name)
            with bundle.open(matches[0]) as handle:
                yield handle
                return

    with tarfile.open(archive, "r:gz") as bundle:
        matches = [member for member in bundle.getmembers() if member.name == name]
        if not matches:
            raise ArchiveMemberError(f"archive member is missing: {name}")
        if len(matches) != 1:
            raise ArchiveMemberError(f"archive has duplicate members: {name}")
        member = matches[0]
        if not member.isfile():
            raise ArchiveMemberError(f"archive member is not a regular file: {name}")
        handle = bundle.extractfile(member)
        if handle is None:
            raise ArchiveMemberError(f"archive member is not a regular file: {name}")
        with handle:
            yield handle


def selected_member_digest(archive: Path, name: str) -> str:
    """Digest exactly one regular archive member without materializing it."""
    with _selected_handle(archive, name) as handle:
        return _digest(handle)


def copy_selected_member(archive: Path, name: str, output: Path) -> str:
    """Stream one validated regular member to a new file and return its digest."""
    value = hashlib.sha256()
    with _selected_handle(archive, name) as source, output.open("wb") as target:
        for chunk in iter(lambda: source.read(CHUNK_SIZE), b""):
            value.update(chunk)
            target.write(chunk)
    return value.hexdigest()
