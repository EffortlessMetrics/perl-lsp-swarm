from __future__ import annotations

import hashlib
import json
import os
import shutil
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Any, Callable, Dict

RELEASE_REPOSITORY = "EffortlessMetrics/perl-lsp"
MANIFEST_PATH = Path(__file__).with_name("server-manifest.json")
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024


class ManifestError(RuntimeError):
    pass


class UnsupportedPlatform(RuntimeError):
    pass


def load_manifest(path: Path = MANIFEST_PATH) -> Dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise ManifestError("unsupported server manifest schema")
    if data.get("repository") != RELEASE_REPOSITORY:
        raise ManifestError("server manifest repository is not canonical")
    version = data.get("version")
    tag = data.get("release_tag")
    assets = data.get("assets")
    if not isinstance(version, str) or not version:
        raise ManifestError("server manifest version is missing")
    if tag != f"v{version}":
        raise ManifestError("release tag does not match the pinned server version")
    if not isinstance(assets, dict) or not assets:
        raise ManifestError("server manifest assets are missing")
    return data


def platform_key(platform: str, arch: str) -> str:
    return f"{platform}-{arch}"


def select_asset(manifest: Dict[str, Any], platform: str, arch: str) -> Dict[str, str]:
    key = platform_key(platform, arch)
    asset = manifest["assets"].get(key)
    if not isinstance(asset, dict):
        raise UnsupportedPlatform(f"unsupported Sublime platform/architecture: {key}")
    required = ("target", "asset", "sha256", "binary")
    if any(not isinstance(asset.get(field), str) or not asset[field] for field in required):
        raise ManifestError(f"incomplete asset record for {key}")
    digest = asset["sha256"]
    if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
        raise ManifestError(f"invalid SHA-256 for {key}")
    return asset


def release_url(manifest: Dict[str, Any], asset: Dict[str, str]) -> str:
    return (
        f"https://github.com/{RELEASE_REPOSITORY}/releases/download/"
        f"{manifest['release_tag']}/{asset['asset']}"
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def verify_sha256(path: Path, expected: str) -> None:
    actual = sha256_file(path)
    if actual != expected:
        raise ManifestError(f"SHA-256 mismatch for {path.name}: expected {expected}, got {actual}")


def download_verified(
    url: str,
    destination: Path,
    expected_sha256: str,
    opener: Callable[..., Any] = urllib.request.urlopen,
) -> None:
    partial = destination.with_suffix(destination.suffix + ".part")
    total = 0
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "LSP-perllsp"})
        with opener(request, timeout=30) as response, partial.open("wb") as output:
            while True:
                block = response.read(1024 * 1024)
                if not block:
                    break
                total += len(block)
                if total > MAX_ARCHIVE_BYTES:
                    raise ManifestError("release archive exceeds the configured size limit")
                output.write(block)
        verify_sha256(partial, expected_sha256)
        os.replace(partial, destination)
    finally:
        partial.unlink(missing_ok=True)


def _matching_archive_names(names: list[str], binary_name: str) -> list[str]:
    suffix = "/" + binary_name
    return [name for name in names if name == binary_name or name.endswith(suffix)]


def extract_binary(archive: Path, binary_name: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".part")
    temporary.unlink(missing_ok=True)
    try:
        if archive.name.endswith(".zip"):
            with zipfile.ZipFile(archive) as package:
                matches = _matching_archive_names(package.namelist(), binary_name)
                if len(matches) != 1:
                    raise ManifestError(
                        f"expected exactly one {binary_name} in {archive.name}, found {len(matches)}"
                    )
                with package.open(matches[0]) as source, temporary.open("wb") as output:
                    shutil.copyfileobj(source, output)
        elif archive.name.endswith(".tar.gz"):
            with tarfile.open(archive, mode="r:gz") as package:
                members = [
                    member
                    for member in package.getmembers()
                    if member.isfile()
                    and (member.name == binary_name or member.name.endswith("/" + binary_name))
                ]
                if len(members) != 1:
                    raise ManifestError(
                        f"expected exactly one {binary_name} in {archive.name}, found {len(members)}"
                    )
                source = package.extractfile(members[0])
                if source is None:
                    raise ManifestError(f"could not read {binary_name} from {archive.name}")
                with source, temporary.open("wb") as output:
                    shutil.copyfileobj(source, output)
        else:
            raise ManifestError(f"unsupported release archive: {archive.name}")
        if temporary.stat().st_size == 0:
            raise ManifestError("extracted perllsp binary is empty")
        temporary.chmod(0o755)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def _metadata_path(binary_path: Path) -> Path:
    return binary_path.with_name("install.json")


def installed_binary_is_current(binary_path: Path, asset: Dict[str, str]) -> bool:
    metadata_path = _metadata_path(binary_path)
    if not binary_path.is_file() or not metadata_path.is_file():
        return False
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    if metadata.get("archive_sha256") != asset["sha256"]:
        return False
    expected_binary_sha = metadata.get("binary_sha256")
    return isinstance(expected_binary_sha, str) and sha256_file(binary_path) == expected_binary_sha


def install_server(
    storage_path: Path,
    platform: str,
    arch: str,
    opener: Callable[..., Any] = urllib.request.urlopen,
) -> Path:
    manifest = load_manifest()
    asset = select_asset(manifest, platform, arch)
    install_dir = storage_path / manifest["version"] / platform_key(platform, arch)
    binary_path = install_dir / asset["binary"]
    if installed_binary_is_current(binary_path, asset):
        return binary_path

    storage_path.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="perllsp-", dir=str(storage_path)) as temporary_dir:
        temporary_root = Path(temporary_dir)
        archive = temporary_root / asset["asset"]
        extracted = temporary_root / asset["binary"]
        download_verified(release_url(manifest, asset), archive, asset["sha256"], opener)
        extract_binary(archive, asset["binary"], extracted)

        install_dir.mkdir(parents=True, exist_ok=True)
        staged_binary = install_dir / (asset["binary"] + ".part")
        shutil.copyfile(extracted, staged_binary)
        staged_binary.chmod(0o755)
        os.replace(staged_binary, binary_path)
        metadata = {
            "schema_version": 1,
            "version": manifest["version"],
            "target": asset["target"],
            "asset": asset["asset"],
            "archive_sha256": asset["sha256"],
            "binary_sha256": sha256_file(binary_path),
        }
        metadata_path = _metadata_path(binary_path)
        metadata_part = metadata_path.with_suffix(".json.part")
        metadata_part.write_text(json.dumps(metadata, sort_keys=True) + "\n", encoding="utf-8")
        os.replace(metadata_part, metadata_path)
    return binary_path
