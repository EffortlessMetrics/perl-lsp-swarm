"""Observation and projection model for contributor topology."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from contributor_topology_sources import (
    PRODUCT_IDENTITY_PATH,
    SCHEMA,
    SYNC_PROTOCOL_PATH,
    ContributorTopologyError,
    canonical_digest,
    load_static_topology,
    optional_sha,
    require_string,
)

CHANNEL_STATES = {"AVAILABLE", "UNAVAILABLE", "NOT_PROVEN"}
OBSERVATION_KEYS = {
    "status", "source", "observed_at", "limitation",
    "development_repository", "development_branch", "development_sha",
    "publication_repository", "publication_branch", "publication_sha",
    "prepared_swarm_sha", "publication_join_sha", "public_release_tag",
    "channels",
}


def empty_observation() -> dict[str, Any]:
    return {
        "status": "NOT_PROVEN",
        "source": None,
        "observed_at": None,
        "limitation": "live topology observation was not supplied",
        "development_sha": None,
        "publication_sha": None,
        "prepared_swarm_sha": None,
        "publication_join_sha": None,
        "public_release_tag": None,
        "stage": "not_proven",
        "channels": {},
    }


def normalize_channels(value: object) -> dict[str, str]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ContributorTopologyError("observation.channels must be an object")
    channels: dict[str, str] = {}
    for name, state in value.items():
        name = require_string(name, "channel name")
        if state not in CHANNEL_STATES:
            raise ContributorTopologyError(
                f"channel {name!r} has invalid state {state!r}"
            )
        channels[name] = state
    return dict(sorted(channels.items()))


def normalize_observation(raw: object, static: dict[str, str]) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise ContributorTopologyError("observation must be a JSON object")
    unknown = set(raw) - OBSERVATION_KEYS
    if unknown:
        raise ContributorTopologyError(f"unknown observation fields: {sorted(unknown)}")

    status = require_string(raw.get("status"), "observation.status")
    if status not in {"PROVEN", "NOT_PROVEN"}:
        raise ContributorTopologyError("status must be PROVEN or NOT_PROVEN")
    source = require_string(raw.get("source"), "observation.source")
    observed_at = require_string(raw.get("observed_at"), "observation.observed_at")
    limitation = raw.get("limitation")
    if limitation is not None:
        limitation = require_string(limitation, "observation.limitation")
    if status == "NOT_PROVEN" and limitation is None:
        raise ContributorTopologyError("NOT_PROVEN observation requires a limitation")
    if status == "PROVEN" and limitation is not None:
        raise ContributorTopologyError("PROVEN observation cannot carry a limitation")

    pairs = (
        ("development_repository", "development_repository"),
        ("development_branch", "development_default_branch"),
        ("publication_repository", "publication_repository"),
        ("publication_branch", "publication_branch"),
    )
    for observed_key, static_key in pairs:
        if raw.get(observed_key) != static[static_key]:
            raise ContributorTopologyError(
                f"observation {observed_key} disagrees with static topology"
            )

    development_sha = optional_sha(raw.get("development_sha"), "development_sha")
    publication_sha = optional_sha(raw.get("publication_sha"), "publication_sha")
    prepared_sha = optional_sha(raw.get("prepared_swarm_sha"), "prepared_swarm_sha")
    join_sha = optional_sha(raw.get("publication_join_sha"), "publication_join_sha")
    release_tag = raw.get("public_release_tag")
    if release_tag is not None:
        release_tag = require_string(release_tag, "public_release_tag")
    channels = normalize_channels(raw.get("channels"))

    if status == "PROVEN" and (development_sha is None or publication_sha is None):
        raise ContributorTopologyError("PROVEN observation requires both repository SHAs")
    if join_sha is not None and prepared_sha is None:
        raise ContributorTopologyError("publication join requires prepared swarm SHA")
    if release_tag is not None and join_sha is None:
        raise ContributorTopologyError("public release requires publication join SHA")
    if any(state == "AVAILABLE" for state in channels.values()) and release_tag is None:
        raise ContributorTopologyError("AVAILABLE channel requires public release tag")

    if status == "NOT_PROVEN":
        stage = "not_proven"
    elif release_tag is not None:
        stage = "public_release"
    elif join_sha is not None:
        stage = "post_join_pre_release"
    elif prepared_sha is not None:
        stage = "prepared_candidate"
    else:
        stage = "development_only"

    return {
        "status": status,
        "source": source,
        "observed_at": observed_at,
        "limitation": limitation,
        "development_sha": development_sha,
        "publication_sha": publication_sha,
        "prepared_swarm_sha": prepared_sha,
        "publication_join_sha": join_sha,
        "public_release_tag": release_tag,
        "stage": stage,
        "channels": channels,
    }


def load_observation(path: Path | None, static: dict[str, str]) -> dict[str, Any]:
    if path is None:
        return empty_observation()
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContributorTopologyError(f"cannot load observation {path}: {error}") from error
    return normalize_observation(raw, static)


def build_projection(root: Path, observation_path: Path | None = None) -> dict[str, Any]:
    static, sources = load_static_topology(root)
    body: dict[str, Any] = {
        "schema": SCHEMA,
        "static": static,
        "observation": load_observation(observation_path, static),
        "sources": sources,
    }
    body["projection_digest"] = canonical_digest(body)
    return body


def validate_projection(projection: object, root: Path) -> None:
    if not isinstance(projection, dict):
        raise ContributorTopologyError("projection must be a JSON object")
    keys = {"schema", "static", "observation", "sources", "projection_digest"}
    if set(projection) != keys or projection.get("schema") != SCHEMA:
        raise ContributorTopologyError("projection does not match contributor topology v1")

    static, sources = load_static_topology(root)
    if projection.get("static") != static:
        raise ContributorTopologyError("projection static topology is stale")
    if projection.get("sources") != sources:
        raise ContributorTopologyError("projection source digests are stale")

    observation = projection.get("observation")
    if not isinstance(observation, dict):
        raise ContributorTopologyError("projection observation must be an object")
    if observation.get("source") is None:
        canonical = empty_observation()
    else:
        canonical = normalize_observation(
            {
                "status": observation.get("status"),
                "source": observation.get("source"),
                "observed_at": observation.get("observed_at"),
                "limitation": observation.get("limitation"),
                "development_repository": static["development_repository"],
                "development_branch": static["development_default_branch"],
                "development_sha": observation.get("development_sha"),
                "publication_repository": static["publication_repository"],
                "publication_branch": static["publication_branch"],
                "publication_sha": observation.get("publication_sha"),
                "prepared_swarm_sha": observation.get("prepared_swarm_sha"),
                "publication_join_sha": observation.get("publication_join_sha"),
                "public_release_tag": observation.get("public_release_tag"),
                "channels": observation.get("channels"),
            },
            static,
        )
    if observation != canonical:
        raise ContributorTopologyError("projection observation is not canonical")
    body = {key: projection[key] for key in ("schema", "static", "observation", "sources")}
    if projection.get("projection_digest") != canonical_digest(body):
        raise ContributorTopologyError("projection digest does not match its content")


def render_human(projection: dict[str, Any]) -> str:
    static = projection["static"]
    observation = projection["observation"]
    lines = [
        f"contributor-topology: {observation['status']}",
        f"development: {static['development_repository']}/{static['development_default_branch']} @ {observation['development_sha'] or 'NOT_PROVEN'}",
        f"publication: {static['publication_repository']}/{static['publication_branch']} @ {observation['publication_sha'] or 'NOT_PROVEN'}",
        f"issues/prs: {static['issue_repository']}",
        f"promotion: {static['promotion_protocol']}",
        f"stage: {observation['stage']}",
    ]
    if observation["limitation"]:
        lines.append(f"limitation: {observation['limitation']}")
    lines.extend(
        f"channel {name}: {state}" for name, state in observation["channels"].items()
    )
    return "\n".join(lines)
