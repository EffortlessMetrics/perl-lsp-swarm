#!/usr/bin/env python3
"""Project the canonical development/publication topology for contributors.

The projection is derived from the existing product-identity contract and sync
protocol. It performs no network access and never mutates repository, release,
or publication state. Optional captured observations make live SHAs and
publication stages explicit; missing or incomplete observations remain
NOT_PROVEN.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

SCHEMA = 1
PRODUCT_IDENTITY_PATH = "policy/product-identity.toml"
SYNC_PROTOCOL_PATH = "docs/swarm/sync-protocol.md"
PROMOTION_PROTOCOL = (
    "docs/swarm/sync-protocol.md"
    "#mechanics-history-preserving-complete-tree-merge"
)
EXPECTED_DEVELOPMENT_BRANCH = "main"
EXPECTED_PUBLICATION_BRANCH = "master"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
REPOSITORY_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
CHANNEL_STATES = {"AVAILABLE", "UNAVAILABLE", "NOT_PROVEN"}
OBSERVATION_KEYS = {
    "status",
    "source",
    "observed_at",
    "limitation",
    "development_repository",
    "development_branch",
    "development_sha",
    "publication_repository",
    "publication_branch",
    "publication_sha",
    "prepared_swarm_sha",
    "publication_join_sha",
    "public_release_tag",
    "channels",
}


class ContributorTopologyError(ValueError):
    """A static topology source or captured observation is invalid."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_digest(value: object) -> str:
    payload = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ContributorTopologyError(f"{label} must be a non-empty string")
    return value


def require_repository(value: object, label: str) -> str:
    repository = require_string(value, label)
    if not REPOSITORY_RE.fullmatch(repository):
        raise ContributorTopologyError(
            f"{label} must be an owner/name repository slug: {repository!r}"
        )
    return repository


def require_optional_sha(value: object, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not SHA_RE.fullmatch(value):
        raise ContributorTopologyError(
            f"{label} must be null or a full lowercase commit SHA"
        )
    return value


def repository_branch(sync_protocol: str, repository: str) -> str:
    basename = repository.split("/", 1)[1]
    pattern = re.compile(
        rf"^\|\s*`{re.escape(basename)}/([^`]+)`\s*\|\s*[^|]+\|\s*$",
        re.MULTILINE,
    )
    matches = pattern.findall(sync_protocol)
    if len(matches) != 1:
        raise ContributorTopologyError(
            f"sync protocol must contain exactly one authority row for {repository}; "
            f"found {len(matches)}"
        )
    branch = matches[0].strip()
    if not branch or branch.startswith("-") or any(character.isspace() for character in branch):
        raise ContributorTopologyError(
            f"sync protocol has an invalid branch for {repository}: {branch!r}"
        )
    return branch


def load_static_topology(root: Path) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    identity_path = root / PRODUCT_IDENTITY_PATH
    sync_path = root / SYNC_PROTOCOL_PATH
    try:
        identity = tomllib.loads(identity_path.read_text(encoding="utf-8"))
        sync_protocol = sync_path.read_text(encoding="utf-8")
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ContributorTopologyError(f"cannot load topology authority: {error}") from error

    if identity.get("schema_version") != 1:
        raise ContributorTopologyError(
            "product identity schema must be 1 for contributor topology v1"
        )
    product = identity.get("product")
    if not isinstance(product, dict):
        raise ContributorTopologyError("product identity [product] table is missing")
    development_repository = require_repository(
        product.get("development_repository"), "product.development_repository"
    )
    publication_repository = require_repository(
        product.get("public_repository"), "product.public_repository"
    )
    if development_repository == publication_repository:
        raise ContributorTopologyError(
            "development and publication repositories must remain distinct"
        )

    development_branch = repository_branch(sync_protocol, development_repository)
    publication_branch = repository_branch(sync_protocol, publication_repository)
    if development_branch != EXPECTED_DEVELOPMENT_BRANCH:
        raise ContributorTopologyError(
            "development branch contradicts the current topology contract: "
            f"expected {EXPECTED_DEVELOPMENT_BRANCH!r}, found {development_branch!r}"
        )
    if publication_branch != EXPECTED_PUBLICATION_BRANCH:
        raise ContributorTopologyError(
            "publication branch contradicts the current topology contract: "
            f"expected {EXPECTED_PUBLICATION_BRANCH!r}, found {publication_branch!r}"
        )

    development_name = development_repository.split("/", 1)[1]
    publication_name = publication_repository.split("/", 1)[1]
    role_patterns = (
        (
            rf"`{re.escape(development_name)}` is the active development "
            r"source of truth\.",
            "development authority",
        ),
        (
            rf"`{re.escape(publication_name)}` is the\s+release, history, "
            r"and canonical package-lineage repo\.",
            "publication authority",
        ),
    )
    for pattern, label in role_patterns:
        if re.search(pattern, sync_protocol) is None:
            raise ContributorTopologyError(
                f"sync protocol is missing the {label} statement"
            )

    required_sync_markers = (
        "#### Mechanics: history-preserving complete-tree merge",
        f"git merge -s ours --no-commit swarm/{development_branch}",
        f"git read-tree -u --reset swarm/{development_branch}",
    )
    for marker in required_sync_markers:
        if marker not in sync_protocol:
            raise ContributorTopologyError(
                f"sync protocol is missing required promotion marker: {marker!r}"
            )

    static = {
        "development_repository": development_repository,
        "development_default_branch": development_branch,
        "publication_repository": publication_repository,
        "publication_branch": publication_branch,
        "issue_repository": development_repository,
        "pull_request_repository": development_repository,
        "promotion_protocol": PROMOTION_PROTOCOL,
    }
    sources = {
        PRODUCT_IDENTITY_PATH: {
            "path": PRODUCT_IDENTITY_PATH,
            "sha256": sha256(identity_path),
        },
        SYNC_PROTOCOL_PATH: {
            "path": SYNC_PROTOCOL_PATH,
            "sha256": sha256(sync_path),
        },
    }
    return static, sources


def empty_observation() -> dict[str, Any]:
    return {
        "status": "NOT_PROVEN",
        "source": NІИ="25ќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њX›XШ][Ы—Ъ›Ъ[—ЬЪH™\]Z\™\И™\\™YЬЭШ\›WЬЪH‚€
B€Y€X›XЧЬ™[X\ЩWЭYИ\И›Э›Ы™H[™X›XШ][Ы—Ъ›Ъ[—ЬЪH\И›Ы™N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њX›XЧЬ™[X\ЩWЭYИ™\]Z\™\ИX›XШ][Ы—Ъ›Ъ[—ЬЪH‚€
B€Y€[ћJЭ]HOHђUђRSP“H€›Ь€Э]H[€Ъ[›™[Лќ[Y\К
JH[™X›XШЧЬ™[X\ЩWЭYИ\И›Ы™N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€[€UђRSP“HЪ[›™[™\]Z\™\ИX›XЧЬ™[X\ЩWЭYИ‚€
B‚€Y€X›XЧЬ™[X\ЩWЭYИ\И›Э›Ы™N‚€ЭYЩHHњX›XЧЬ™[X\ЩH‚€[Y€X›XШ][Ы—Ъ›Ъ[—ЬЪH\И›Э›Ы™N‚€ЭYЩHHњЬЭЪ›Ъ[—Ь™WЬ™[X\ЩH‚€[Y€™\\™YЬЭШ\›WЬЪH\И›Э›Ы™N‚€ЭYЩHHњ™\\™YШШ[™Y]H‚€[ЩN‚€ЭYЩHH™]™[ЬY[ќЫЫ›H‚‚€™]\›€В€њЭ]\ИЋ€Э]\Л€њЫЭ\ЩHЋ€ЫЭ\ЩK€›ШњЩ\ќ™YШ]Ћ€ШњЩ\ќ™YШ]€›[Z]][Ы€Ћ€›Ы™K€™]™[ЬY[ќЬЪHЋ€]™[ЬY[ќЬЪK€њX›XШ][Ы—ЬЪHЋ€X›XШ][Ы—ЬЪK€њ™\\™YЬЭШ\›WЬЪHЋ€™\\™YЬЭШ\›WЬЪK€њX›XШ][Ы—Ъ›Ъ[—ЬЪHЋ€X›XШ][Ы—Ъ›Ъ[—ЬЪK€њX›XЧЬ™[X\ЩWЭYИЋ€X›XЧЬ™[X\ЩWЭYЛ€њЭYЩHЋ€ЭYЩK€Ъ[›™[ИЋ€Ъ[›™[Л€B‚‚™Y€ШYЫШњЩ\ќ][ЫЉ]€]›Ы™KЭ]XО€XЭЬЭ‹Э—JHO€XЭЬЭ‹[ћWN‚€Y€]\И›Ы™N‚€™]\›€[\WЫШњЩ\ќ][ЫЉ
B€ћN‚€]ИHњЫЫ‹›ШYК]њ™XYЭ^
[ЫЩ[™ПHќ]‹NЉJB€^Щ\
ФС\њ›Ь‹њЫЫ‹’”УУ‘XЫЩQ\њ›ЬЉH\И\њ›ЬЋ‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€€Ш[››ЭШYШ\\™YШњЩ\ќ][Ы€Ь]N€Щ\њ›ЬџH‚€
Hњ›ЫH\њ›Ь‚€™]\›€›Ь›X[^™WЫШњЩ\ќ][ЫЉ]ЛЭ]XКB‚‚™Y€ќZ[Ь›Ъ™XЭ[ЫЉ›ЫЭ€]ШњЩ\ќ][Ы—Ь]€]›Ы™HH›Ы™JHO€XЭЬЭ‹[ћWN‚€Э]XЛЫЭ\Щ\ИHШYЬЭ]XЧЭЬЫЩЮJ›ЫЭ
B€ШњЩ\ќ][Ы€HШYЫШњЩ\ќ][ЫЉШњЩ\ќ][Ы—Ь]Э]XКB€›ЩHHВ€њШЪ[XHЋ€РТSPK€њЭ]XИЋ€Э]XЛ€›ШњЩ\ќ][Ы€Ћ€ШњЩ\ќ][Ы‹€њЫЭ\Щ\ИЋ€ЫЭ\Щ\Л€B€›ЩVИњ›Ъ™XЭ[Ы—ЩYЩ\Э—HHШ[›ЫљXШ[ЩYЩ\Э
›ЩJB€™]\›€›ЩB‚‚™Y€[Y]WЬ›Ъ™XЭ[ЫЉ›Ъ™XЭ[ЫЋ€Шљ™XЭ›ЫЭ€]
HO€›Ы™N‚€Y€›Э\Ъ[њЭ[ЩJ›Ъ™XЭ[Ы‹XЭ
N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉњ›Ъ™XЭ[Ы€]\Э™HH”УУ€Шљ™XЭЉB€^XЭYЪЩ^\ИHВ€њШЪ[XH‹€њЭ]XИ‹€›ШњЩ\ќ][Ы€‹€њЫЭ\Щ\И‹€њ›Ъ™XЭ[Ы—ЩYЩ\Э‹€B€Y€Щ]
›Ъ™XЭ[ЫЉHOH^XЭYЪЩ^\О‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њ›Ъ™XЭ[Ы€љY[ИY™™\€њ›ЫHЫЫќљXќ]Ь—ЭЬЫЩЮKќЊN€‚€€™^XЭY^ЬЫЬќY
^XЭYЪЩ^\К_K›Э[™^ЬЫЬќY
›Ъ™XЭ[ЫЉ_H‚€
B€Y€›Ъ™XЭ[Ы‹™Щ]
њШЪ[XHЉHOHРТSPN‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њ›Ъ™XЭ[Ы€ШЪ[XH]\Э™HФРТSP_HЉB‚€Э]XЛЫЭ\Щ\ИHШYЬЭ]XЧЭЬЫЩЮJ›ЫЭ
B€Y€›Ъ™XЭ[Ы‹™Щ]
њЭ]XИЉHOHЭ]XО‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њ›Ъ™XЭ[Ы€Э]XИЬЫЩЮH\ИЭ[HЬ€ЫЫќYXЭЬћH‚€
B€Y€›Ъ™XЭ[Ы‹™Щ]
њЫЭ\Щ\ИЉHOHЫЭ\Щ\О‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉњ›Ъ™XЭ[Ы€ЫЭ\ЩHYЩ\ЭИ\™HЭ[HЉB‚€ШњЩ\ќ][Ы€H›Ъ™XЭ[Ы‹™Щ]
›ШњЩ\ќ][Ы€ЉB€Y€›Э\Ъ[њЭ[ЩJШњЩ\ќ][Ы‹XЭ
N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉњ›Ъ™XЭ[Ы€ШњЩ\ќ][Ы€]\Э™H[€Шљ™XЭЉB€›Ь›X[^™YЪ[њ]HВ€њЭ]\ИЋ€ШњЩ\ќ][Ы‹™Щ]
њЭ]\ИЉK€њЫЭ\ЩHЋ€ШњЩ\ќ][Ы‹™Щ]
њЫЭ\ЩHЉK€›ШњЩ\ќ™YШ]Ћ€ШњЩ\ќ][Ы‹™Щ]
›ШњЩ\ќ™YШ]ЉK€›[Z]][Ы€Ћ€ШњЩ\ќ][Ы‹™Щ]
›[Z]][Ы€ЉK€™]™[ЬY[ќЬ™\ЬЪ]ЬћHЋ€Э]XЦИ™]™[ЬY[ќЬ™\ЬЪ]ЬћH—K€™]™[ЬY[ќШњ[ЪЋ€Э]XЦИ™]™[ЬY[ќЩY][Шњ[Ъ—K€™]™[ЬY[ќЬЪHЋ€ШњЩ\ќ][Ы‹™Щ]
™]™[ЬY[ќЬЪHЉK€њX›XШ][Ы—Ь™\ЬЪ]ЬћHЋ€Э]XЦИњX›XШ][Ы—Ь™\ЬЪ]ЬћH—K€њX›XШ][Ы—Шњ[ЪЋ€Э]XЦИњX›XШ][Ы—Шњ[Ъ—K€њX›XШ][Ы—ЬЪHЋ€ШњЩ\ќ][Ы‹™Щ]
њX›XШ][Ы—ЬЪHЉK€њ™\\™YЬЭШ\›WЬЪHЋ€ШњЩ\ќ][Ы‹™Щ]
њ™\\™YЬЭШ\›WЬЪHЉK€њX›XШ][Ы—Ъ›Ъ[—ЬЪHЋ€ШњЩ\ќ][Ы‹™Щ]
њX›XШ][Ы—Ъ›Ъ[—ЬЪHЉK€њX›XЧЬ™[X\ЩWЭYИЋ€ШњЩ\ќ][Ы‹™Щ]
њX›XЧЬ™[X\ЩWЭYИЉK€Ъ[›™[ИЋ€ШњЩ\ќ][Ы‹™Щ]
Ъ[›™[ИЉK€B€›Ь›X[^™YЫШњЩ\ќ][Ы€H›Ь›X[^™WЫШњЩ\ќ][ЫЉ›Ь›X[^™YЪ[њ]Э]XКB€Y€ШњЩ\ќ][Ы€OH›Ь›X[^™YЫШњЩ\ќ][ЫЋ‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ€њ›Ъ™XЭ[Ы€ШњЩ\ќ][Ы€\И›ЭШ[›ЫљXШ[Ь€\И[€[ќ[YЭYЩH‚€
B‚€YЩ\ЭЪ[њ]HВ€њШЪ[XHЋ€›Ъ™XЭ[Ы–ИњШЪ[XH—K€њЭ]XИЋ€›Ъ™XЭ[Ы–ИњЭ]XИ—K€›ШњЩ\ќ][Ы€Ћ€›Ъ™XЭ[Ы–И›ШњЩ\ќ][Ы€—K€њЫЭ\Щ\ИЋ€›Ъ™XЭ[Ы–ИњЫЭ\Щ\И—K€B€Y€›Ъ™XЭ[Ы‹™Щ]
њ›Ъ™XЭ[Ы—ЩYЩ\ЭЉHOHШ[›ЫљXШ[ЩYЩ\Э
YЩ\ЭЪ[њ]
N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉњ›Ъ™XЭ[Ы€YЩ\ЭЩ\И›ЭX]Ъ]ИЫЫќ[ќЉB‚‚™Y€™[™\—Ъ[X[Љ›Ъ™XЭ[ЫЋ€XЭЬЭ‹[ћWJHO€ЭЋ‚€Э]XИH›Ъ™XЭ[Ы–ИњЭ]XИ—B€ШњЩ\ќ][Ы€H›Ъ™XЭ[Ы–И›ШњЩ\ќ][Ы€—B€]™[ЬY[ќЬЪHHШњЩ\ќ][Ы–И™]™[ЬY[ќЬЪH—HЬ€““ХФ“Х‘S€‚€X›XШ][Ы—ЬЪHHШњЩ\ќ][Ы–ИњX›XШ][Ы—ЬЪH—HЬ€““ХФ“Х‘S€‚€[™\ИHВ€€ЫЫќљXќ]Ь‹]ЬЫЩЮN€ЫШњЩ\ќ][Ы–ЙЬЭ]\ЙЧ_H‹€
€™]™[ЬY[ќ€‚€€ћЬЭ]XЦЙЩ]™[ЬY[ќЬ™\ЬЪ]ЬћIЧ_KИ‚€€ћЬЭ]XЦЙЩ]™[ЬY[ќЩY][Шњ[Ъ	Ч_HЩ]™[ЬY[ќЬЪ_H‚€
K€
€њX›XШ][ЫЋ€‚€€ћЬЭ]XЦЙЬX›XШ][Ы—Ь™\ЬЪ]ЬћIЧ_KИ‚€€ћЬЭ]XЦЙЬX›XШ][Ы—Шњ[Ъ	Ч_HЬX›XШ][Ы—ЬЪ_H‚€
K€€љ\ЬЭY\ЛЬњО€ЬЭ]XЦЙЪ\ЬЭYWЬ™\ЬЪ]ЬћIЧ_H‹€€њ›Ы[Э[ЫЋ€ЬЭ]XЦЙЬ›Ы[Э[Ы—Ь›ЭШЫЫ	Ч_H‹€€њЭYЩN€ЫШњЩ\ќ][Ы–ЙЬЭYЩIЧ_H‹€B€Y€ШњЩ\ќ][Ы–И›[Z]][Ы€—H\И›Э›Ы™N‚€[™\Л\[™
€›[Z]][ЫЋ€ЫШњЩ\ќ][Ы–ЙЫ[Z]][Ы‰Ч_HЉB€›Ь€Ъ[›™[Э]H[€ШњЩ\ќ][Ы–ИЪ[›™[И—Kљ][\К
N‚€[™\Л\[™
€Ъ[›™[ШЪ[›™[N€ЬЭ]_HЉB€™]\›€—€‹љ›Ъ[Љ[™\КB‚‚™Y€XZ[Љ
HO€[ќ‚€\њЩ\€H\™Ь\њЩKђ\™Э[Y[ќ\њЩ\Љ\ШЬљ\[ЫЏWЧЩШЧЧКB€\њЩ\‹YШ\™Э[Y[ќ
€‹K[ШњЩ\ќ][Ы€‹€\OT]€[HШ\\™Y™XY[Ы›H]™HШњЩ\ќ][Ы€”УУЋИЫZ]›Ь€“ХФ“Х‘S€‹€
B€\њЩ\‹YШ\™Э[Y[ќ
‹K[Э]]‹\OT]
B€\њЩ\‹YШ\™Э[Y[ќ
€‹KXЪXЪИ‹€XЭ[ЫЏHњЭЬ™WЭќYH‹€[Hќ[Y]HH^\Э[™ИK[Э]]YШZ[њЭЭ\њ™[ќЭ]XИ]]Ьљ]Y\И‹€
B€\њЩ\‹YШ\™Э[Y[ќ
€‹KZњЫЫ€‹XЭ[ЫЏHњЭЬ™WЭќYH‹[Hњљ[ќШ[›ЫљXШ[”УУ€[њЭXYЩ€[X[€Э]]‚€
B€\™ЬИH\њЩ\‹њ\њЩWШ\™ЬК
B€›ЫЭH]
ЧЩљ[WЧКKњ™\ЫЫ™J
Kњ\™[ќЦМWB‚€ћN‚€Y€\™ЬЛЪXЪО‚€Y€\™ЬЛ›Э]]\И›Ы™N‚€Z\ЩHЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉ‹KXЪXЪИ™\]Z\™\ИK[Э]]ЉB€›Ъ™XЭ[Ы€HњЫЫ‹›ШYК\™ЬЛ›Э]]њ™XYЭ^
[ЫЩ[™ПHќ]‹NЉJB€[Y]WЬ›Ъ™XЭ[ЫЉ›Ъ™XЭ[Ы‹›ЫЭ
B€[ЩN‚€›Ъ™XЭ[Ы€HќZ[Ь›Ъ™XЭ[ЫЉ›ЫЭ\™ЬЛ›ШњЩ\ќ][ЫЉB€[Y]WЬ›Ъ™XЭ[ЫЉ›Ъ™XЭ[Ы‹›ЫЭ
B€Y€\™ЬЛ›Э]]\И›Э›Ы™N‚€\™ЬЛ›Э]]њ\™[ќ›ZЩ\Љ\™[ќПUќYK^\ЭЫЪПUќYJB€\™ЬЛ›Э]]ќЬљ]WЭ^
€њЫЫ‹™[\К›Ъ™XЭ[Ы‹[™[ќL‹ЫЬќЪЩ^\ПUќYJH
И—€‹€[ЫЩ[™ПHќ]‹N‹€
B€^Щ\
ФС\њ›Ь‹њЫЫ‹’”УУ‘XЫЩQ\њ›Ь‹ЫЫќљXќ]Ь•ЬЫЩЮQ\њ›ЬЉH\И\њ›ЬЋ‚€љ[ќ
€ЫЫќљXќ]Ь‹]ЬЫЩЮN€“ХФ“Х‘SЋ€Щ\њ›ЬџH‹љ[O\Ю\ЛњЭ\њЉB€™]\›€‚‚€Y€\™ЬЛљњЫЫЋ‚€љ[ќ
њЫЫ‹™[\К›Ъ™XЭ[Ы‹[™[ќL‹ЫЬќЪЩ^\ПUќYJJB€[ЩN‚€љ[ќ
™[™\—Ъ[X[Љ›Ъ™XЭ[ЫЉJB€™]\›€‚‚љY€ЧЫ[YWЧИOH—ЧЫXZ[—ЧИЋ‚€Z\ЩHЮ\Э[Q^]
XZ[Љ
JB