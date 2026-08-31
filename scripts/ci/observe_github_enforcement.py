#!/usr/bin/env python3
"""Capture GitHub's live enforcement surfaces as github_enforcement_snapshot.v1.

The observer is bounded and read-only. It issues GET requests only, retains no
credential material, and never mutates branch protection, rulesets, bypass
actors, or checked-in policy.

It does not reinterpret enforcement semantics.
`scripts/ci/reconcile_github_enforcement_snapshot.py` (#9152) remains the sole
authority for target applicability, union construction, and
`MATCH` / `DRIFT` / `NOT_PROVEN`. This module only observes, hashes the exact
evidence, and reports which surfaces it could and could not read.

An unreadable surface never becomes an empty surface: a surface that was not
definitively observed carries no rows and downgrades observation permission, so
a permission failure reaches the reconciler as incomplete evidence rather than
as proof that no enforcement exists.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Iterable

SNAPSHOT_VERSION = 1
CAPTURE_VERSION = 2
API_ROOT = "https://api.github.com"
API_VERSION = "2022-11-28"
LIVE_SOURCES = ("trusted_default_branch", "operator", "connector")
TOKEN_VARIABLES = ("GITHUB_TOKEN", "GH_TOKEN")

# Closed, private-safe limitation vocabulary. Raw host errors, response bodies,
# URLs, and credential material never reach a limitation string; only these
# codes, optionally suffixed with a public numeric ruleset id, are emitted.
CLASSIC_FORBIDDEN = "classic_branch_protection_forbidden"
CLASSIC_UNREADABLE = "classic_branch_protection_unreadable"
RULESET_LIST_FORBIDDEN = "ruleset_list_forbidden"
RULESET_LIST_UNREADABLE = "ruleset_list_unreadable"
RULESET_DETAIL_FORBIDDEN = "ruleset_detail_forbidden"
RULESET_DETAIL_UNREADABLE = "ruleset_detail_unreadable"
RULESET_DETAIL_UNREPRESENTABLE = "ruleset_detail_unrepresentable"
RULESET_LIST_INCOMPLETE = "ruleset_list_incomplete"
RULESET_LIST_TRUNCATED = "ruleset_list_truncated"

# The ruleset listing is requested at the maximum page size. A repository with
# more rulesets than one page would silently understate live enforcement, so a
# paginated listing is reported as truncated rather than followed: one bounded
# request per surface keeps the response digest single-valued.
RULESET_PAGE_SIZE = 100

# Classic branch protection uses -1 on a required check to mean "any source"
# rather than a specific app. It is a sentinel, not an app identity, so it is
# carried as an absent binding: the reconciler then reports a mismatch against
# any declared `classic_app_id`, which is the correct verdict — "any source"
# does not satisfy "must be app N".
ANY_SOURCE_APP_ID = -1


class ObserverError(RuntimeError):
    """Capture could not produce a well-formed observation."""


class ApiResult:
    """One bounded GET outcome: an HTTP status and the exact response bytes."""

    __slots__ = ("status", "body", "transport_failed", "link")

    def __init__(
        self,
        status: int | None,
        body: bytes = b"",
        *,
        transport_failed: bool = False,
        link: str = "",
    ) -> None:
        self.status = status
        self.body = body
        self.transport_failed = transport_failed
        # Only the RFC 5988 Link header is retained, and only to detect a
        # truncated listing. No other response header is captured.
        self.link = link

    @property
    def has_next_page(self) -> bool:
        return 'rel="next"' in self.link

    @property
    def ok(self) -> bool:
        return self.status == 200 and not self.transport_failed

    @property
    def forbidden(self) -> bool:
        return self.status in (401, 403)

    def json(self, field: str) -> Any:
        try:
            return json.loads(self.body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ObserverError(f"{field} response is not valid JSON") from error

    def sha256(self) -> str:
        return hashlib.sha256(self.body).hexdigest()


Transport = Callable[[str], ApiResult]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """Refuse every redirect.

    The default opener copies request headers onto a redirected request
    without checking that the host is unchanged, which would forward the
    bearer token to whatever host a 3xx names. These are idempotent GETs
    against a fixed API root and gain nothing from following redirects, so a
    3xx is surfaced as its own status and read as an unreadable surface.
    """

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def http_transport(token: str | None) -> Transport:
    """Build a read-only HTTPS transport. The token is used, never retained."""
    opener = urllib.request.build_opener(NoRedirect)

    def get(path: str) -> ApiResult:
        request = urllib.request.Request(  # noqa: S310 - fixed https API root
            f"{API_ROOT}/{path.lstrip('/')}",
            method="GET",
            headers=headers(token),
        )
        try:
            with opener.open(request, timeout=30) as response:
                return ApiResult(
                    response.status,
                    response.read(),
                    link=response.headers.get("Link", "") or "",
                )
        except urllib.error.HTTPError as error:
            return ApiResult(error.code, error.read())
        except (urllib.error.URLError, OSError, ValueError):
            # Host errors are deliberately not retained: an unreachable API is
            # an unreadable surface, and its raw text is not evidence.
            return ApiResult(None, b"", transport_failed=True)

    return get


def headers(token: str | None) -> dict[str, str]:
    """Request headers for one bounded read. The token is used, not stored."""
    value = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": API_VERSION,
        "User-Agent": "perl-lsp-enforcement-observer",
    }
    if token:
        value["Authorization"] = f"Bearer {token}"
    return value


def resolve_token(environ: dict[str, str] | None = None) -> str | None:
    """First non-empty token from the accepted environment variables."""
    environ = os.environ if environ is None else environ
    for name in TOKEN_VARIABLES:
        token = environ.get(name)
        if token:
            return token
    return None


def normalize_timestamp(value: str, field: str) -> str:
    """Normalize an ISO-8601 instant to UTC `...Z`, rejecting naive input."""
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise ObserverError(f"{field} must be an ISO-8601 timestamp") from error
    if parsed.tzinfo is None:
        raise ObserverError(f"{field} must carry a timezone")
    return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def utc_now() -> str:
    """Current UTC instant, second resolution, in the contract's format."""
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


# --------------------------------------------------------------------------
# Capture
# --------------------------------------------------------------------------


class Capture:
    """The exact response evidence backing one observation.

    A capture is bound to the repository, branch, and acquisition time it was
    taken with. Those travel inside the bundle so an imported capture cannot
    be relabelled onto another branch or re-dated as fresh evidence.
    """

    def __init__(
        self,
        *,
        repository: str = "",
        branch: str = "",
        captured_at: str = "",
    ) -> None:
        self.entries: dict[str, ApiResult] = {}
        self.repository = repository
        self.branch = branch
        self.captured_at = captured_at

    def record(self, key: str, result: ApiResult) -> ApiResult:
        self.entries[key] = result
        return result

    def get(self, key: str) -> ApiResult:
        if key not in self.entries:
            raise ObserverError(f"capture is missing required entry: {key}")
        return self.entries[key]

    def to_bundle(self) -> dict[str, Any]:
        """Serialize the capture, preserving exact bytes and acquisition identity."""
        return {
            "schema_version": CAPTURE_VERSION,
            "repository": self.repository,
            "branch": self.branch,
            "captured_at": self.captured_at,
            "entries": [
                {
                    "key": key,
                    "status": result.status,
                    "transport_failed": result.transport_failed,
                    "link": result.link,
                    "body_base64": base64.b64encode(result.body).decode("ascii"),
                }
                for key, result in sorted(self.entries.items())
            ],
        }

    @classmethod
    def from_bundle(cls, bundle: Any) -> "Capture":
        """Rebuild a capture, rejecting any bundle a real capture could not produce."""
        if not isinstance(bundle, dict):
            raise ObserverError("capture bundle must be an object")
        if bundle.get("schema_version") != CAPTURE_VERSION:
            raise ObserverError(
                f"capture bundle schema_version must be {CAPTURE_VERSION}"
            )
        entries = bundle.get("entries")
        if not isinstance(entries, list):
            raise ObserverError("capture bundle entries must be a list")
        # Acquisition identity is required. A bundle without it could be
        # relabelled onto another branch or re-dated as fresh evidence, so a
        # legacy bundle fails closed rather than being adopted.
        repository = bundle.get("repository")
        branch = bundle.get("branch")
        captured_at = bundle.get("captured_at")
        for field, value in (
            ("repository", repository),
            ("branch", branch),
            ("captured_at", captured_at),
        ):
            if not isinstance(value, str) or not value:
                raise ObserverError(f"capture bundle is missing {field}")
        normalize_timestamp(captured_at, "capture bundle captured_at")
        capture = cls(
            repository=repository, branch=branch, captured_at=captured_at
        )
        for entry in entries:
            if not isinstance(entry, dict):
                raise ObserverError("capture bundle entry must be an object")
            missing = {"key", "status", "body_base64"} - set(entry)
            if missing:
                raise ObserverError(
                    f"capture bundle entry missing fields: {sorted(missing)}"
                )
            key = entry["key"]
            if not isinstance(key, str) or not key.strip():
                raise ObserverError("capture bundle entry key must be a string")
            status = entry["status"]
            if status is not None and (
                not isinstance(status, int) or isinstance(status, bool)
            ):
                raise ObserverError(
                    "capture bundle entry status must be an integer or null"
                )
            failed = entry.get("transport_failed", False)
            if not isinstance(failed, bool):
                raise ObserverError(
                    "capture bundle entry transport_failed must be a boolean"
                )
            # The live transport emits a status or a transport failure, never
            # both and never neither. Imported evidence may not represent a
            # state the observer itself cannot produce.
            if failed != (status is None):
                raise ObserverError(
                    "capture bundle entry must carry a status exactly when it "
                    "did not fail in transport"
                )
            try:
                body = base64.b64decode(entry["body_base64"], validate=True)
            except (ValueError, TypeError) as error:
                raise ObserverError(
                    "capture bundle entry body_base64 is not valid base64"
                ) from error
            link = entry.get("link", "")
            if not isinstance(link, str):
                raise ObserverError("capture bundle entry link must be a string")
            capture.record(
                key,
                ApiResult(status, body, transport_failed=failed, link=link),
            )
        return capture


def ruleset_key(ruleset_id: int) -> str:
    """Capture key holding one ruleset's detail response."""
    return f"ruleset:{ruleset_id}"


def encode_segment(value: str, field: str) -> str:
    """Percent-encode one path segment.

    A branch name may legally contain characters that are reserved in a URL
    path or query. Interpolating them raw would let a valid branch address a
    different endpoint entirely.
    """
    if not isinstance(value, str) or not value:
        raise ObserverError(f"{field} must be a non-empty string")
    return urllib.parse.quote(value, safe="")


def encode_repository(repository: str) -> str:
    """Encode `owner/name`, rejecting anything that is not exactly two parts."""
    if not isinstance(repository, str):
        raise ObserverError("repository must be a string")
    parts = repository.split("/")
    if len(parts) != 2 or not all(parts):
        raise ObserverError("repository must be exactly owner/name")
    owner, name = parts
    return f"{encode_segment(owner, 'repository owner')}/{encode_segment(name, 'repository name')}"


def capture_live(repository: str, branch: str, transport: Transport) -> Capture:
    """Perform the bounded read-only GET sequence against the live API."""
    repo = encode_repository(repository)
    ref = encode_segment(branch, "branch")
    # Stamped before the first request: a long capture must never make its
    # earliest responses look newer than they are.
    capture = Capture(repository=repository, branch=branch, captured_at=utc_now())
    capture.record("repository", transport(f"repos/{repo}"))
    capture.record("branch_head", transport(f"repos/{repo}/git/ref/heads/{ref}"))
    capture.record(
        "classic_branch_protection",
        transport(f"repos/{repo}/branches/{ref}/protection"),
    )
    listing = capture.record(
        "ruleset_list",
        transport(
            f"repos/{repo}/rulesets"
            f"?includes_parents=true&per_page={RULESET_PAGE_SIZE}"
        ),
    )
    if listing.ok:
        try:
            listed = branch_ruleset_ids(listing)
        except ObserverError:
            # A malformed listing is an unreadable ruleset surface, decided in
            # `ruleset_surface`. Capture must not abort: the identity already
            # captured is still bindable evidence.
            listed = []
        for ruleset_id in listed:
            capture.record(
                ruleset_key(ruleset_id),
                transport(f"repos/{repo}/rulesets/{ruleset_id}"),
            )
    return capture


def branch_ruleset_ids(listing: ApiResult) -> list[int]:
    """Ids of branch-target rulesets in the listing, in ascending order."""
    payload = listing.json("ruleset_list")
    if not isinstance(payload, list):
        raise ObserverError("ruleset listing must be a JSON array")
    ids: list[int] = []
    for item in payload:
        if not isinstance(item, dict):
            raise ObserverError("ruleset listing entry must be an object")
        if item.get("target") != "branch":
            # Tag and push rulesets cannot enforce a branch status context and
            # are not representable in github_enforcement_snapshot.v1.
            continue
        ruleset_id = item.get("id")
        if not isinstance(ruleset_id, int) or isinstance(ruleset_id, bool):
            raise ObserverError("ruleset listing entry id must be an integer")
        ids.append(ruleset_id)
    return sorted(set(ids))


# --------------------------------------------------------------------------
# Snapshot assembly
# --------------------------------------------------------------------------


def build_snapshot(
    capture: Capture,
    *,
    source: str,
    static_receipt: dict[str, Any],
) -> dict[str, Any]:
    """Assemble github_enforcement_snapshot.v1 from captured evidence.

    The branch and observation time come from the capture itself. There is
    deliberately no caller override for either: an argument that could replace
    the acquisition time would let any caller present an old capture as fresh,
    which is the freshness bound this contract exists to enforce.
    """
    if source not in LIVE_SOURCES:
        raise ObserverError(
            f"source must be one of {sorted(LIVE_SOURCES)}; "
            "the observer never emits a fixture observation"
        )
    branch = capture.branch
    if not branch:
        raise ObserverError("capture is not bound to a branch")
    if not capture.captured_at:
        raise ObserverError("capture is not bound to an acquisition time")

    static_contract = static_binding(static_receipt)
    identity = repository_identity(capture)
    limitations: list[str] = []

    classic = classic_surface(capture, branch, limitations)
    rulesets = ruleset_surface(capture, limitations)

    snapshot = {
        "schema_version": SNAPSHOT_VERSION,
        "repository": {
            "full_name": identity["full_name"],
            "repository_id": identity["repository_id"],
            "default_branch": identity["default_branch"],
            "branch_sha": branch_head_sha(capture),
            "observed_at": normalize_timestamp(
                capture.captured_at, "capture captured_at"
            ),
        },
        "observation": {
            "source": source,
            "permission": permission_for(classic, rulesets, limitations),
            "limitations": sorted(set(limitations)),
        },
        "static_contract": static_contract,
        "classic_branch_protection": classic,
        "rulesets": rulesets,
    }
    enforce_no_empty_surface_claim(snapshot)
    return snapshot


def static_binding(receipt: dict[str, Any]) -> dict[str, Any]:
    """Bind the snapshot to the exact static contract subject it reconciles."""
    if not isinstance(receipt, dict):
        raise ObserverError("static receipt must be an object")
    if receipt.get("status") != "SUCCESS":
        raise ObserverError(
            "static receipt status must be SUCCESS before a live observation "
            "can be bound to it"
        )
    subjects = receipt.get("subjects")
    if not isinstance(subjects, dict):
        raise ObserverError("static receipt is missing subjects")
    policy = subjects.get("policy")
    if not isinstance(policy, dict):
        raise ObserverError("static receipt is missing subjects.policy")
    binding = {
        "subject_sha256": receipt.get("subject_sha256"),
        "exact_source_sha256": receipt.get("exact_source_sha256"),
        "policy_sha256": policy.get("sha256"),
        "repository_sha": subjects.get("repository_sha"),
    }
    for field, value in binding.items():
        if not isinstance(value, str) or not value:
            raise ObserverError(f"static receipt is missing {field}")
    return binding


def repository_identity(capture: Capture) -> dict[str, Any]:
    """Repository identity from the capture, or fail closed if unreadable."""
    result = capture.get("repository")
    if not result.ok:
        # Without repository identity there is no subject to bind, so the
        # observer fails closed rather than emitting an unbound snapshot.
        raise ObserverError(
            "repository identity is unreadable; no snapshot can be bound "
            f"(status={result.status})"
        )
    payload = result.json("repository")
    if not isinstance(payload, dict):
        raise ObserverError("repository response must be an object")
    full_name = payload.get("full_name")
    repository_id = payload.get("id")
    default_branch = payload.get("default_branch")
    if not isinstance(full_name, str) or "/" not in full_name:
        raise ObserverError("repository response is missing full_name")
    if not isinstance(repository_id, int) or isinstance(repository_id, bool):
        raise ObserverError("repository response is missing a numeric id")
    if not isinstance(default_branch, str) or not default_branch:
        raise ObserverError("repository response is missing default_branch")
    return {
        "full_name": full_name,
        "repository_id": repository_id,
        "default_branch": default_branch,
    }


def branch_head_sha(capture: Capture) -> str:
    """Exact branch head SHA, checked against the branch the capture claims."""
    result = capture.get("branch_head")
    if not result.ok:
        raise ObserverError(
            "branch head is unreadable; the observation cannot be bound to an "
            f"exact branch SHA (status={result.status})"
        )
    payload = result.json("branch_head")
    if not isinstance(payload, dict):
        raise ObserverError("branch head response must be an object")
    # The captured ref must be the branch this capture claims. Otherwise one
    # branch's protection could be assembled under another branch's name, and
    # two branches sharing a commit would reconcile without complaint.
    ref = payload.get("ref")
    expected = f"refs/heads/{capture.branch}"
    if ref != expected:
        raise ObserverError(
            f"branch head response is for {ref!r}, not {expected!r}"
        )
    sha = (payload.get("object") or {}).get("sha")
    if not isinstance(sha, str) or len(sha) != 40:
        raise ObserverError("branch head response is missing object.sha")
    return sha


def classic_surface(
    capture: Capture, branch: str, limitations: list[str]
) -> dict[str, Any]:
    """Normalize the classic branch-protection surface and its instrument state."""
    result = capture.get("classic_branch_protection")
    surface: dict[str, Any] = {
        "instrument_state": "observed",
        "response_sha256": None,
        "branch": branch,
        "strict": None,
        "required_status_checks": [],
    }
    if result.ok:
        try:
            payload = result.json("classic_branch_protection")
            if not isinstance(payload, dict):
                raise ObserverError(
                    "classic protection response must be an object"
                )
            checks = payload.get("required_status_checks")
            if checks is not None and not isinstance(checks, dict):
                raise ObserverError(
                    "classic protection required_status_checks must be an object"
                )
            strict = (checks or {}).get("strict")
            if strict is not None and not isinstance(strict, bool):
                raise ObserverError("classic protection strict must be a boolean")
            rows = classic_checks(checks or {})
        except ObserverError:
            # A 200 we cannot parse is an unreadable instrument, not an absent
            # one. Reporting it keeps the rest of the observation bindable
            # while still forcing NOT_PROVEN downstream.
            surface["instrument_state"] = "unreadable"
            limitations.append(CLASSIC_UNREADABLE)
            return surface
        surface["response_sha256"] = result.sha256()
        surface["strict"] = strict
        surface["required_status_checks"] = rows
        return surface

    if result.status == 404 and not result.transport_failed:
        # GitHub reports an unprotected branch as 404. That is a definitive
        # absence of the classic instrument, not a permission failure, so the
        # surface is `missing` and access remains complete.
        surface["instrument_state"] = "missing"
        return surface

    if result.forbidden:
        surface["instrument_state"] = "unreadable"
        limitations.append(CLASSIC_FORBIDDEN)
        return surface

    surface["instrument_state"] = "error" if result.transport_failed else "unreadable"
    limitations.append(CLASSIC_UNREADABLE)
    return surface


def classic_checks(required: dict[str, Any]) -> list[dict[str, Any]]:
    """Normalize classic `checks`/`contexts` into {context, app_id} rows."""
    rows: dict[str, int | None] = {}
    contexts = required.get("contexts")
    if contexts is not None:
        if not isinstance(contexts, list):
            raise ObserverError("classic protection contexts must be a list")
        for context in contexts:
            if not isinstance(context, str) or not context:
                raise ObserverError("classic protection context must be a string")
            rows.setdefault(context, None)
    checks = required.get("checks")
    if checks is not None:
        if not isinstance(checks, list):
            raise ObserverError("classic protection checks must be a list")
        for check in checks:
            if not isinstance(check, dict):
                raise ObserverError("classic protection check must be an object")
            context = check.get("context")
            if not isinstance(context, str) or not context:
                raise ObserverError("classic protection check is missing context")
            raw = check.get("app_id")
            if raw == ANY_SOURCE_APP_ID:
                raw = None
            rows[context] = app_identity(raw, "classic app_id")
    return [
        {"context": context, "app_id": rows[context]} for context in sorted(rows)
    ]


def app_identity(value: Any, field: str) -> int | None:
    """Validate an app/integration id: a positive integer, or absent."""
    if value is None:
        return None
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ObserverError(f"{field} must be a positive integer or null")
    return value


def ruleset_surface(capture: Capture, limitations: list[str]) -> dict[str, Any]:
    """Normalize the ruleset surface, reporting every ruleset it could not read."""
    result = capture.get("ruleset_list")
    surface: dict[str, Any] = {
        "instrument_state": "observed",
        "list_response_sha256": None,
        "items": [],
    }
    if not result.ok:
        if result.forbidden:
            surface["instrument_state"] = "unreadable"
            limitations.append(RULESET_LIST_FORBIDDEN)
            return surface
        surface["instrument_state"] = (
            "error" if result.transport_failed else "unreadable"
        )
        limitations.append(RULESET_LIST_UNREADABLE)
        return surface

    try:
        listed = branch_ruleset_ids(result)
    except ObserverError:
        surface["instrument_state"] = "unreadable"
        limitations.append(RULESET_LIST_UNREADABLE)
        return surface

    surface["list_response_sha256"] = result.sha256()
    if result.has_next_page:
        # A second page exists that this bounded capture did not read, so the
        # observed ruleset set is a subset of live enforcement.
        limitations.append(RULESET_LIST_TRUNCATED)
    items: list[dict[str, Any]] = []
    for ruleset_id in listed:
        key = ruleset_key(ruleset_id)
        if key not in capture.entries:
            limitations.append(f"{RULESET_LIST_INCOMPLETE}:{ruleset_id}")
            continue
        detail = capture.get(key)
        if not detail.ok:
            code = (
                RULESET_DETAIL_FORBIDDEN
                if detail.forbidden
                else RULESET_DETAIL_UNREADABLE
            )
            limitations.append(f"{code}:{ruleset_id}")
            continue
        try:
            item = ruleset_item(ruleset_id, detail)
        except ObserverError:
            # A detail response we cannot parse leaves this ruleset's
            # contribution unknown; it is reported, never assumed empty.
            limitations.append(f"{RULESET_DETAIL_UNREADABLE}:{ruleset_id}")
            continue
        if item is None:
            limitations.append(f"{RULESET_DETAIL_UNREPRESENTABLE}:{ruleset_id}")
            continue
        items.append(item)
    surface["items"] = sorted(items, key=lambda item: item["id"])
    return surface


def ruleset_item(ruleset_id: int, detail: ApiResult) -> dict[str, Any] | None:
    """Normalize one ruleset detail response, or None when unrepresentable."""
    payload = detail.json(f"ruleset:{ruleset_id}")
    if not isinstance(payload, dict):
        raise ObserverError("ruleset detail response must be an object")
    # The detail must be for the ruleset the listing named. A swapped entry
    # would attribute one ruleset's enforcement, conditions, and bypass actors
    # to another; `ruleset_surface` turns this into an unreadable-detail
    # limitation rather than accepting the misattribution.
    if payload.get("id") != ruleset_id:
        raise ObserverError(
            f"ruleset detail response is not for ruleset {ruleset_id}"
        )
    if payload.get("target") != "branch":
        return None
    include, exclude = ref_name_conditions(payload)
    if include is None:
        # An empty or absent include selector cannot be represented, and
        # silently dropping the ruleset would understate live enforcement.
        return None
    name = payload.get("name")
    source_type = payload.get("source_type")
    source = payload.get("source")
    enforcement = payload.get("enforcement")
    for field, value in (
        ("name", name),
        ("source_type", source_type),
        ("source", source),
        ("enforcement", enforcement),
    ):
        if not isinstance(value, str) or not value:
            raise ObserverError(f"ruleset {ruleset_id} is missing {field}")
    strict, do_not_enforce, checks = ruleset_status_checks(payload, ruleset_id)
    return {
        "id": ruleset_id,
        "name": name,
        "target": "branch",
        "source_type": source_type,
        "source": source,
        "enforcement": enforcement,
        "detail_response_sha256": detail.sha256(),
        "conditions": {"ref_name": {"include": include, "exclude": exclude}},
        "bypass_actors": bypass_actors(payload, ruleset_id),
        "strict_required_status_checks_policy": strict,
        "do_not_enforce_on_create": do_not_enforce,
        "required_status_checks": checks,
    }


def ref_name_conditions(
    payload: dict[str, Any],
) -> tuple[list[str] | None, list[str]]:
    """Ref-name selectors; include is None when the ruleset is unrepresentable."""
    ref_name = ((payload.get("conditions") or {}).get("ref_name")) or {}
    if not isinstance(ref_name, dict):
        raise ObserverError("ruleset conditions.ref_name must be an object")
    include = selector_list(ref_name.get("include"), "include")
    exclude = selector_list(ref_name.get("exclude"), "exclude")
    if not include:
        return None, exclude
    return include, exclude


def selector_list(value: Any, field: str) -> list[str]:
    """Sorted unique ref-name selectors, as the snapshot contract requires."""
    if value is None:
        return []
    if not isinstance(value, list):
        raise ObserverError(f"ruleset ref_name.{field} must be a list")
    selectors = set()
    for selector in value:
        if not isinstance(selector, str) or not selector:
            raise ObserverError(f"ruleset ref_name.{field} entry must be a string")
        selectors.add(selector)
    return sorted(selectors)


def bypass_actors(payload: dict[str, Any], ruleset_id: int) -> list[dict[str, Any]]:
    """Normalized, deduplicated bypass actors for one ruleset."""
    actors = payload.get("bypass_actors")
    if actors is None:
        return []
    if not isinstance(actors, list):
        raise ObserverError(f"ruleset {ruleset_id} bypass_actors must be a list")
    rows = []
    for actor in actors:
        if not isinstance(actor, dict):
            raise ObserverError(f"ruleset {ruleset_id} bypass actor must be object")
        actor_type = actor.get("actor_type")
        bypass_mode = actor.get("bypass_mode")
        if not isinstance(actor_type, str) or not actor_type:
            raise ObserverError(f"ruleset {ruleset_id} actor_type is missing")
        if not isinstance(bypass_mode, str) or not bypass_mode:
            raise ObserverError(f"ruleset {ruleset_id} bypass_mode is missing")
        rows.append(
            {
                "actor_type": actor_type,
                "actor_id": app_identity(actor.get("actor_id"), "actor_id"),
                "bypass_mode": bypass_mode,
            }
        )
    # Identical rows carry no additional information, and the reconciler
    # rejects a duplicate bypass identity outright, so collapse them here
    # rather than losing the whole snapshot to a repeated row.
    unique = {
        (row["actor_type"], row["actor_id"], row["bypass_mode"]): row
        for row in rows
    }
    return sorted(
        unique.values(),
        key=lambda row: (
            row["actor_type"],
            -1 if row["actor_id"] is None else row["actor_id"],
            row["bypass_mode"],
        ),
    )


def ruleset_status_checks(
    payload: dict[str, Any], ruleset_id: int
) -> tuple[bool | None, bool | None, list[dict[str, Any]]]:
    """Required-status-check settings and contexts from one ruleset's rules."""
    rules = payload.get("rules")
    if rules is None:
        return None, None, []
    if not isinstance(rules, list):
        raise ObserverError(f"ruleset {ruleset_id} rules must be a list")
    strict: bool | None = None
    do_not_enforce: bool | None = None
    rows: dict[str, int | None] = {}
    for rule in rules:
        if not isinstance(rule, dict):
            raise ObserverError(f"ruleset {ruleset_id} rule must be an object")
        if rule.get("type") != "required_status_checks":
            continue
        parameters = rule.get("parameters") or {}
        if not isinstance(parameters, dict):
            raise ObserverError(f"ruleset {ruleset_id} rule parameters invalid")
        strict = optional_bool(
            parameters.get("strict_required_status_checks_policy"), strict
        )
        do_not_enforce = optional_bool(
            parameters.get("do_not_enforce_on_create"), do_not_enforce
        )
        checks = parameters.get("required_status_checks")
        if checks is None:
            continue
        if not isinstance(checks, list):
            raise ObserverError(
                f"ruleset {ruleset_id} required_status_checks must be a list"
            )
        for check in checks:
            if not isinstance(check, dict):
                raise ObserverError(f"ruleset {ruleset_id} check must be object")
            context = check.get("context")
            if not isinstance(context, str) or not context:
                raise ObserverError(f"ruleset {ruleset_id} check missing context")
            # GitHub names the ruleset binding `integration_id`; the snapshot
            # contract carries it in `app_id` per enforcement source, which the
            # reconciler compares only against `ruleset_integration_id`.
            rows[context] = app_identity(
                check.get("integration_id"), "ruleset integration_id"
            )
    return (
        strict,
        do_not_enforce,
        [{"context": context, "app_id": rows[context]} for context in sorted(rows)],
    )


def optional_bool(value: Any, current: bool | None) -> bool | None:
    """Keep the current value when a rule omits the flag; reject a non-boolean."""
    if value is None:
        return current
    if not isinstance(value, bool):
        raise ObserverError("ruleset status-check flag must be a boolean")
    return value


def permission_for(
    classic: dict[str, Any],
    rulesets: dict[str, Any],
    limitations: Iterable[str],
) -> str:
    """Access completeness — never surface presence.

    `complete` means every surface was read to a definitive answer. A branch
    with no classic protection is still a complete observation; the reconciler
    decides what an absent instrument means for the verdict.
    """
    if list(limitations):
        classic_reached = classic["instrument_state"] in ("observed", "missing")
        rulesets_reached = rulesets["instrument_state"] == "observed"
        if classic_reached or rulesets_reached:
            return "partial"
        return "unknown"
    return "complete"


def enforce_no_empty_surface_claim(snapshot: dict[str, Any]) -> None:
    """A surface that was not observed must carry no rows and no digest.

    This is the observer's load-bearing fail-closed invariant: a permission or
    transport failure must never be presentable as "nothing is enforced".
    """
    classic = snapshot["classic_branch_protection"]
    rulesets = snapshot["rulesets"]
    if classic["instrument_state"] != "observed" and (
        classic["required_status_checks"] or classic["response_sha256"]
    ):
        raise ObserverError(
            "unobserved classic branch protection cannot carry rows or a digest"
        )
    if rulesets["instrument_state"] != "observed" and (
        rulesets["items"] or rulesets["list_response_sha256"]
    ):
        raise ObserverError(
            "unobserved ruleset surface cannot carry rows or a digest"
        )
    if snapshot["observation"]["permission"] == "complete" and (
        snapshot["observation"]["limitations"]
        or classic["instrument_state"] not in ("observed", "missing")
        or rulesets["instrument_state"] != "observed"
    ):
        raise ObserverError(
            "complete observation permission requires every surface to have "
            "been read to a definitive answer"
        )


# --------------------------------------------------------------------------
# Authority
# --------------------------------------------------------------------------


def build_authority(
    *,
    producer: str,
    declared_repository: str,
    declared_repository_id: int,
    declared_branch: str,
    max_age_seconds: int,
    max_future_skew_seconds: int,
    evaluated_at: str | None = None,
) -> dict[str, Any]:
    """Emit the reconciliation authority from OPERATOR-DECLARED identity.

    The authority exists so a snapshot cannot authenticate itself, which means
    it must never be derived from the observation. Every identity value here
    comes from what the caller declared up front; the reconciler is what
    compares those declarations against what was actually observed, and a
    disagreement is its to report.
    """
    # Validated here rather than left to the reconciler: an authority that
    # fails the contract downstream is a wasted capture, and the operator sees
    # the reason at the point they supplied the value.
    for field, value in (
        ("producer", producer),
        ("declared branch", declared_branch),
        ("declared repository", declared_repository),
    ):
        if not isinstance(value, str) or not value.strip():
            raise ObserverError(f"{field} must be a non-empty string")
    if len(declared_repository.split("/")) != 2 or not all(
        declared_repository.split("/")
    ):
        raise ObserverError("declared repository must be exactly owner/name")
    for field, value in (
        ("declared repository id", declared_repository_id),
        ("max observation age seconds", max_age_seconds),
        ("max future skew seconds", max_future_skew_seconds),
    ):
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            raise ObserverError(f"{field} must be a positive integer")
    return {
        "schema_version": 1,
        "producer": producer,
        "repository": {
            "full_name": declared_repository,
            "repository_id": declared_repository_id,
            "default_branch": declared_branch,
        },
        "evaluated_at": normalize_timestamp(evaluated_at, "evaluated_at")
        if evaluated_at
        else utc_now(),
        "max_observation_age_seconds": max_age_seconds,
        "max_future_skew_seconds": max_future_skew_seconds,
    }


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def write_json(path: Path, payload: Any) -> None:
    """Write deterministic, key-sorted JSON, creating parent directories."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def load_json(path: Path, field: str) -> Any:
    """Read one JSON input, reporting a missing or malformed file as typed."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ObserverError(f"{field} not found: {path}") from error
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise ObserverError(f"{field} is not valid JSON: {path}") from error


def emit(args: argparse.Namespace, capture: Capture) -> int:
    """Assemble and write the snapshot, returning the observation exit code."""
    static_receipt = load_json(args.static_receipt, "static receipt")
    snapshot = build_snapshot(
        capture,
        source=args.source,
        static_receipt=static_receipt,
    )
    write_json(args.snapshot, snapshot)
    if args.authority:
        if args.authority_repository_id is None:
            raise ObserverError(
                "--authority requires --authority-repository-id: the authority "
                "must state repository identity independently of the "
                "observation, so it cannot be taken from the capture"
            )
        write_json(
            args.authority,
            build_authority(
                producer=args.producer,
                declared_repository=args.repository,
                declared_repository_id=args.authority_repository_id,
                declared_branch=args.branch,
                max_age_seconds=args.max_observation_age_seconds,
                max_future_skew_seconds=args.max_future_skew_seconds,
            ),
        )
    if args.capture_bundle:
        write_json(args.capture_bundle, capture.to_bundle())

    observation = snapshot["observation"]
    print(
        "GitHub enforcement observation: "
        f"permission={observation['permission']} "
        f"classic={snapshot['classic_branch_protection']['instrument_state']} "
        f"rulesets={snapshot['rulesets']['instrument_state']}"
    )
    for limitation in observation["limitations"]:
        print(f"- limitation: {limitation}")
    print(
        "- reconcile with "
        "scripts/ci/reconcile_github_enforcement_snapshot.py; this observer "
        "asserts no verdict"
    )
    return 0 if observation["permission"] == "complete" else 2


def add_common_arguments(parser: argparse.ArgumentParser) -> None:
    """Arguments shared by `capture` and `assemble`."""
    # Declared by the operator, never derived from the observation: these are
    # what the reconciliation authority states independently.
    parser.add_argument("--repository", required=True)
    parser.add_argument("--authority-repository-id", type=int)
    parser.add_argument("--branch", default="main")
    parser.add_argument("--source", choices=list(LIVE_SOURCES), required=True)
    parser.add_argument("--static-receipt", type=Path, required=True)
    parser.add_argument("--snapshot", type=Path, required=True)
    parser.add_argument("--authority", type=Path)
    parser.add_argument("--capture-bundle", type=Path)
    parser.add_argument("--producer", default="github-enforcement-observer")
    parser.add_argument("--max-observation-age-seconds", type=int, default=3600)
    parser.add_argument("--max-future-skew-seconds", type=int, default=300)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    capture = commands.add_parser(
        "capture", help="read both live surfaces and emit a snapshot"
    )
    add_common_arguments(capture)

    assemble = commands.add_parser(
        "assemble", help="emit a snapshot from a previously captured bundle"
    )
    assemble.add_argument("--capture", type=Path, required=True)
    add_common_arguments(assemble)

    args = parser.parse_args(list(argv) if argv is not None else None)
    try:
        if args.command == "capture":
            evidence = capture_live(
                args.repository, args.branch, http_transport(resolve_token())
            )
        else:
            evidence = Capture.from_bundle(load_json(args.capture, "capture bundle"))
        return emit(args, evidence)
    except ObserverError as error:
        print(f"github enforcement observation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
