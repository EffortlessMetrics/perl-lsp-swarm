"""Reconcile the pinned DAP authority with production request and event names."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Mapping

from dap_authority_common import (
    DEBUG_ADAPTER_ROOT,
    DISPATCH_PATH,
    PEER_DISPATCH_PATHS,
    SEND_EVENT_CALL_RE,
    SEND_EVENT_LITERAL_RE,
    AuthorityError,
    array_value,
    extractor_identity,
    manifest_rows,
    object_value,
    parse_request_table,
    parse_peer_dispatch_routes,
    production_dispatch_sources,
    production_source_graph,
    read_text,
    string_value,
)


def _production_request_rows(root: Path) -> list[dict[str, str]]:
    text = read_text(root / DISPATCH_PATH, "DAP dispatch source")
    return parse_request_table(text)


def _production_events(root: Path) -> set[str]:
    source_root = root / DEBUG_ADAPTER_ROOT
    if not source_root.is_dir():
        raise AuthorityError(f"missing DAP production source root: {DEBUG_ADAPTER_ROOT}")
    events: set[str] = set()
    for path in sorted(source_root.rglob("*.rs")):
        text = read_text(path, "DAP production source")
        for call in SEND_EVENT_CALL_RE.finditer(text):
            literal = SEND_EVENT_LITERAL_RE.match(text, call.end())
            if literal is None:
                relative = path.relative_to(root)
                line = text.count("\n", 0, call.start()) + 1
                raise AuthorityError(
                    f"cannot derive self.send_event wire name at {relative}:{line}; "
                    "use a literal or extend the typed inventory"
                )
            events.add(literal.group(1))
    return events


def _request_routes(row: Mapping[str, str]) -> list[dict[str, str]]:
    routes = [
        {
            "route_id": f"{row['row_id']}.native",
            "frontend": "native",
            "syntax_owner": DISPATCH_PATH.as_posix(),
            "handler": row["handler"],
            "condition": "default (no external-peer runtime selector)",
            "disposition": "handler_present",
        }
    ]
    explicit = row["availability"] == "all_frontends"
    # A native-only row is still a known catalog route, so the pinned peer
    # fallback (EXPECTED_PEER_FALLBACKS, #9527/#9069) refuses it fail-closed
    # with success: false. The success-empty acknowledgement applies only to
    # commands outside the catalog, which have no row here; that policy is
    # projected separately under `fallback_policies`.
    for frontend, owner, selector in (
        ("external_peer", PEER_DISPATCH_PATHS[0], "--external-peer"),
        ("mirror_peer", PEER_DISPATCH_PATHS[1], "--external-peer-listen"),
    ):
        routes.append(
            {
                "route_id": f"{row['row_id']}.{frontend}",
                "frontend": frontend,
                "syntax_owner": owner.as_posix(),
                "handler": (
                    f"{'DapPeerBridge' if frontend == 'external_peer' else 'MirrorPeerBridge'}::dispatch"
                    if explicit
                    else "fail_closed_unavailable_in_frontend"
                ),
                "condition": selector,
                "disposition": "handler_present" if explicit else "fail_closed",
            }
        )
    return routes


def verify_inventory_binding(root: Path, receipt: Mapping[str, Any]) -> Mapping[str, Any]:
    """Reject a receipt whose extractor or source graph is no longer current.

    `check` always regenerates, so it cannot catch staleness; the risk is a
    receipt kept and consumed after the extractor or the governed sources
    moved. Recompute both identities from the tree in hand and refuse the
    receipt on any drift, including a receipt that predates the binding and
    therefore carries no identity to compare at all.
    """
    production = object_value(receipt.get("production"), "receipt.production")

    recorded_extractor = object_value(production.get("extractor"), "receipt.production.extractor")
    recorded_graph = object_value(
        production.get("source_graph"), "receipt.production.source_graph"
    )

    current_extractor = extractor_identity()
    current_graph = production_source_graph(root)

    recorded_extractor_digest = string_value(
        recorded_extractor.get("digest"), "receipt.production.extractor.digest"
    )
    if recorded_extractor_digest != current_extractor["digest"]:
        recorded_modules = {
            string_value(row.get("module"), "receipt extractor module"): row.get("git_blob_sha1")
            for row in array_value(
                recorded_extractor.get("modules"), "receipt.production.extractor.modules"
            )
        }
        current_modules = {row["module"]: row["git_blob_sha1"] for row in current_extractor["modules"]}
        changed = sorted(
            name
            for name in recorded_modules.keys() | current_modules.keys()
            if recorded_modules.get(name) != current_modules.get(name)
        )
        raise AuthorityError(
            "receipt was produced by a different DAP authority extractor: "
            f"recorded={recorded_extractor_digest}, current={current_extractor['digest']}, "
            f"changed modules={changed}"
        )

    recorded_graph_digest = string_value(
        recorded_graph.get("digest"), "receipt.production.source_graph.digest"
    )
    if recorded_graph_digest != current_graph["digest"]:
        raise AuthorityError(
            "receipt was produced from a different production source graph: "
            f"recorded={recorded_graph_digest} ({recorded_graph.get('file_count')} files), "
            f"current={current_graph['digest']} ({current_graph['file_count']} files)"
        )

    return {
        "extractor_digest": current_extractor["digest"],
        "source_graph_digest": current_graph["digest"],
        "source_graph_file_count": current_graph["file_count"],
    }


def validate_production_boundary(
    root: Path,
    manifest: Mapping[str, Any],
    observed: Mapping[str, Any],
) -> Mapping[str, Any]:
    standard_requests = set(
        array_value(observed.get("standard_requests"), "observed.standard_requests")
    )
    standard_events = set(
        array_value(observed.get("standard_events"), "observed.standard_events")
    )
    rows = _production_request_rows(root)
    commands = {row["command"] for row in rows}
    events = _production_events(root)

    expected_owners = {DISPATCH_PATH, *PEER_DISPATCH_PATHS}
    discovered_owners = production_dispatch_sources(root)
    if discovered_owners != expected_owners:
        raise AuthorityError(
            "production request-dispatch source graph changed: "
            f"expected={sorted(map(str, expected_owners))}, "
            f"discovered={sorted(map(str, discovered_owners))}"
        )
    expected_peer_variants = {
        row["variant"] for row in rows if row["availability"] == "all_frontends"
    }
    for path in PEER_DISPATCH_PATHS:
        actual = parse_peer_dispatch_routes(
            read_text(root / path, "DAP peer dispatch source"), path
        )
        if actual != expected_peer_variants:
            raise AuthorityError(
                f"{path} route/catalog mismatch: "
                f"missing={sorted(expected_peer_variants - actual)}, "
                f"unexpected={sorted(actual - expected_peer_variants)}"
            )

    # The class declared beside the executable route must agree with the
    # pinned upstream schema. A row cannot claim to be standard DAP that
    # upstream does not define, nor hide a standard request as a project
    # extension.
    misclassified = sorted(
        (row["command"], row["class"])
        for row in rows
        if (row["class"] == "standard") != (row["command"] in standard_requests)
    )
    if misclassified:
        raise AuthorityError(
            "request rows are misclassified against the pinned upstream schema: "
            f"{misclassified}"
        )

    production_extensions = {
        *(("request", name) for name in commands - standard_requests),
        *(("event", name) for name in events - standard_events),
    }
    declared_extensions: set[tuple[str, str]] = set()
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        declared_extensions.add(
            (
                string_value(extension.get("kind"), f"project_extensions[{index}].kind"),
                string_value(
                    extension.get("wire_name"), f"project_extensions[{index}].wire_name"
                ),
            )
        )

    missing = sorted(production_extensions - declared_extensions)
    stale = sorted(declared_extensions - production_extensions)
    if missing or stale:
        raise AuthorityError(
            "production/project extension inventory mismatch: "
            f"unclassified production={missing}, stale manifest={stale}"
        )

    # Versioned custom families: their request and event names are
    # registered transport surfaces, distinct from dispatched production
    # wire names. A family record that claims `dispatched: false` must have
    # no route in the production inventory (and vice versa), so a family
    # can never silently gain or lose runtime reachability.
    family_boundary: list[dict[str, object]] = []
    for index, family in enumerate(manifest_rows(manifest, "project_families")):
        where = f"project_families[{index}]"
        name = string_value(family.get("family"), f"{where}.family")
        request_name = string_value(family.get("request_name"), f"{where}.request_name")
        dispatched = family.get("dispatched")
        if not isinstance(dispatched, bool):
            raise AuthorityError(f"{where}.dispatched must be a boolean")
        if dispatched != (request_name in commands):
            raise AuthorityError(
                f"custom family {name!r} dispatch mismatch: dispatched={dispatched} but the "
                f"production inventory {'has' if request_name in commands else 'lacks'} a "
                f"route for {request_name!r}"
            )
        for event_entry in array_value(family.get("event_names"), f"{where}.event_names"):
            event = string_value(event_entry, f"{where}.event_names entry")
            if event in events:
                raise AuthorityError(
                    f"custom family {name!r} event {event!r} is emitted in production; "
                    "family events must be registered here, not dispatched around it"
                )
        family_boundary.append(
            {
                "family": name,
                "request_name": request_name,
                "event_names": list(
                    string_value(entry, f"{where}.event_names entry")
                    for entry in array_value(family.get("event_names"), f"{where}.event_names")
                ),
                "dispatched": dispatched,
            }
        )

    return {
        "dispatch_path": DISPATCH_PATH.as_posix(),
        "source_root": DEBUG_ADAPTER_ROOT.as_posix(),
        # Which extractor produced these rows, and from what source content.
        # Without this an inventory is self-describing only: the rows are
        # reported, but nothing says they were derived by the current
        # extractor from the current tree, so a receipt kept past either
        # change stays indistinguishable from a fresh one (#9527 falsifier 9).
        "extractor": extractor_identity(),
        "source_graph": production_source_graph(root),
        "request_rows": [
            {
                "row_id": row["row_id"],
                "command": row["command"],
                "class": row["class"],
                "availability": row["availability"],
                "variant": row["variant"],
                "handler": row["handler"],
                "routes": _request_routes(row),
            }
            for row in sorted(rows, key=lambda row: row["row_id"])
        ],
        "commands": sorted(commands),
        "events": sorted(events),
        "project_extensions": [
            {"kind": kind, "wire_name": name} for kind, name in sorted(production_extensions)
        ],
        "project_families": family_boundary,
        "dispatch_sources": sorted(path.as_posix() for path in expected_owners),
        "fallback_policies": [
            {
                "policy_id": "dap.fallback.external_peer.dynamic_compatibility_ack_success_empty",
                "frontend": "external_peer",
                "condition": "unknown command (not present in catalog)",
                "disposition": "not_proven",
            },
            {
                "policy_id": "dap.fallback.mirror_peer.dynamic_compatibility_ack_success_empty",
                "frontend": "mirror_peer",
                "condition": "unknown command (not present in catalog)",
                "disposition": "not_proven",
            },
        ],
    }
