"""Read-only GitHub release metadata and asset download helpers."""

from __future__ import annotations

import json
import os
import shutil
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from .common import ReceiptError

USER_AGENT = "perl-lsp-swarm-zed-asset-receipts/1"
DOWNLOAD_TIMEOUT_SECONDS = 120


def github_request(url: str, token: str | None, accept: str) -> urllib.request.Request:
    headers = {
        "Accept": accept,
        "User-Agent": USER_AGENT,
        "X-GitHub-Api-Version": "2022-11-28",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return urllib.request.Request(url, headers=headers)


def fetch_json(url: str, token: str | None) -> dict[str, Any]:
    request = github_request(url, token, "application/vnd.github+json")
    try:
        with urllib.request.urlopen(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            payload = response.read()
    except (urllib.error.URLError, TimeoutError) as error:
        raise ReceiptError(f"GitHub metadata request failed: {error}") from error
    value = json.loads(payload)
    if not isinstance(value, dict):
        raise ReceiptError("GitHub metadata response is not an object")
    return value


def download_asset(url: str, destination: Path, token: str | None) -> None:
    request = github_request(url, token, "application/octet-stream")
    temporary = destination.with_suffix(destination.suffix + ".partial")
    temporary.unlink(missing_ok=True)
    try:
        with urllib.request.urlopen(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            with temporary.open("wb") as output:
                shutil.copyfileobj(response, output)
        os.replace(temporary, destination)
    except (urllib.error.URLError, OSError, TimeoutError) as error:
        temporary.unlink(missing_ok=True)
        raise ReceiptError(f"asset download failed for {destination.name}: {error}") from error


def release_version(tag: str) -> str:
    return tag[1:] if tag.startswith("v") else tag


def asset_index(
    release: dict[str, Any],
) -> tuple[dict[int, dict[str, Any]], dict[str, list[dict[str, Any]]]]:
    by_id: dict[int, dict[str, Any]] = {}
    by_name: dict[str, list[dict[str, Any]]] = {}
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise ReceiptError("release assets are missing")
    for asset in assets:
        if not isinstance(asset, dict):
            continue
        asset_id = asset.get("id")
        name = asset.get("name")
        if isinstance(asset_id, int):
            by_id[asset_id] = asset
        if isinstance(name, str):
            by_name.setdefault(name, []).append(asset)
    return by_id, by_name
