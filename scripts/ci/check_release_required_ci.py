#!/usr/bin/env python3
"""Read-only, exact-subject required CI validation for release dispatch (#6059).

Enforcement observation and the additive context/app union remain owned by
observe_github_enforcement and reconcile_github_enforcement_snapshot. This
caller evaluates current check runs and legacy statuses; it never dispatches,
reruns, bypasses protection, or accepts a combined-status shortcut.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import time

if __package__:
    from . import observe_github_enforcement as observer
    from . import reconcile_github_enforcement_snapshot as enforcement
else:
    import observe_github_enforcement as observer
    import reconcile_github_enforcement_snapshot as enforcement


PAGE_SIZE = 100
MAX_PAGES = 100
REFERENCE_SUITE_LIMIT = 1000


class NotProven(RuntimeError):
    """An incomplete or inconsistent observation cannot authorize dispatch."""


def required_policy(get, repository: str, branch: str, sha: str):
    capture = observer.capture_live(repository, branch, get)
    identity = observer.repository_identity(capture)
    if identity["full_name"].lower() != repository.lower():
        raise NotProven("repository identity differs from release repository")
    if identity["default_branch"] != branch:
        raise NotProven("release branch is not the observed default branch")
    if observer.branch_head_sha(capture) != sha:
        raise NotProven("release SHA is not the current default-branch tip")
    # A generic/auth-masking 404 is not evidence that classic protection is
    # absent. Retain only GitHub's explicit unprotected-branch disposition.
    classic_response = capture.get("classic_branch_protection")
    if classic_response.status == 404:
        payload = classic_response.json("classic protection absence")
        if not isinstance(payload, dict) or payload.get("message") != "Branch not protected":
            raise NotProven("classic protection absence is not corroborated")
    limitations = []
    classic = observer.classic_surface(capture, branch, limitations)
    rulesets = observer.ruleset_surface(capture, limitations)
    if (limitations or classic["instrument_state"] not in {"observed", "missing"}
            or rulesets["instrument_state"] != "observed"):
        raise NotProven("effective required policy is unreadable or incomplete")
    for ruleset in rulesets["items"]:
        ruleset["targeting"] = enforcement.derive_targeting(
            ruleset["conditions"]["ref_name"], default_branch=branch)
    union, _, limitations = enforcement.live_union({
        "classic_branch_protection": classic, "rulesets": rulesets})
    if limitations or not union:
        raise NotProven("effective required context/app union is unproven or empty")
    return union


def positive_id(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def check_reference_population(get, repository, sha):
    # GitHub limits the ref-level check-runs API to the most recent 1000
    # suites. Pagination alone cannot prove an older active run is absent.
    path = f"repos/{observer.encode_repository(repository)}/commits/{sha}/check-suites?per_page=100"
    response = get(path)
    if not response.ok:
        raise NotProven("check-suite population is unreadable")
    payload = response.json("check suites")
    count = payload.get("total_count") if isinstance(payload, dict) else None
    if (not isinstance(count, int) or isinstance(count, bool)
            or not 0 <= count < REFERENCE_SUITE_LIMIT):
        raise NotProven("check-suite population is invalid or at the ref API limit")
    return count


def collect_pages(get, repository: str, sha: str, kind: str, *, filter="latest"):
    rows = []
    seen = set()
    expected_total = None
    for page in range(1, MAX_PAGES + 1):
        query = f"filter={filter}&" if kind == "check-runs" else ""
        path = (f"repos/{observer.encode_repository(repository)}/commits/{sha}/{kind}"
                f"?{query}per_page={PAGE_SIZE}&page={page}")
        response = get(path)
        if not response.ok:
            raise NotProven(f"{kind} page is unreadable")
        payload = response.json(kind)
        if kind == "check-runs":
            if not isinstance(payload, dict):
                raise NotProven("check-runs response is not an object")
            total = payload.get("total_count")
            if not isinstance(total, int) or isinstance(total, bool) or total < 0:
                raise NotProven("check-runs total is invalid")
            if expected_total is not None and total != expected_total:
                raise NotProven("check-runs population changed during pagination")
            expected_total = total
            items = payload.get("check_runs")
        else:
            items = payload
        if not isinstance(items, list) or len(items) > PAGE_SIZE:
            raise NotProven(f"{kind} page is malformed")
        for item in items:
            field = "name" if kind == "check-runs" else "context"
            if (not isinstance(item, dict) or not positive_id(item.get("id"))
                    or not isinstance(item.get(field), str) or not item[field]):
                raise NotProven(f"{kind} identity is malformed")
            if item["id"] in seen:
                raise NotProven(f"{kind} pagination contains duplicate identities")
            if kind == "check-runs" and (not isinstance(item.get("app"), dict)
                                         or not positive_id(item["app"].get("id"))):
                raise NotProven("check-run app identity is malformed")
            seen.add(item["id"])
            rows.append(item)
        if expected_total is not None and len(rows) > expected_total:
            raise NotProven("check-runs population exceeds its declared total")
        if expected_total == len(rows) and not response.has_next_page:
            return rows
        if len(items) < PAGE_SIZE and not response.has_next_page:
            if expected_total is not None and len(rows) != expected_total:
                raise NotProven("check-runs population is incomplete")
            return rows
    raise NotProven(f"{kind} pagination exceeded its bounded page limit")


def check_result(run, sha):
    if run.get("head_sha") != sha:
        return "STALE"
    status = run.get("status")
    if not isinstance(status, str):
        return "NOT_PROVEN"
    if status in {"queued", "in_progress", "waiting", "pending", "requested"}:
        return "PENDING"
    if status != "completed" or not isinstance(run.get("conclusion"), str):
        return "NOT_PROVEN"
    return {
        "success": "SUCCESS", "neutral": "SUCCESS", "failure": "FAILURE",
        "timed_out": "FAILURE", "action_required": "FAILURE",
        "cancelled": "CANCELLED", "skipped": "SKIPPED", "stale": "STALE",
    }.get(run.get("conclusion"), "NOT_PROVEN")


def evaluate_required(union, runs, all_runs, statuses, sha):
    results = []
    for requirement in union:
        for app_id in requirement["app_ids"]:
            matching = [run for run in runs if run["name"] == requirement["context"]
                        and (app_id is None or (isinstance(run.get("app"), dict)
                             and run["app"].get("id") == app_id))]
            # GitHub owns latest selection; IDs and timestamps are not an
            # attempt-order contract. Ambiguous current identities refuse.
            run = matching[0] if len(matching) == 1 else None
            observed = [row for row in all_runs if row["name"] == requirement["context"]
                        and (app_id is None or row["app"]["id"] == app_id)]
            legacy = [row for row in statuses if row["context"] == requirement["context"]]
            # The statuses API documents reverse chronological order, unlike
            # the numeric identifiers. Preserve that order across pages.
            status = legacy[0] if legacy and app_id is None else None
            values = []
            if len(matching) > 1:
                values.append("NOT_PROVEN")
            if observed and not matching:
                # A legacy status cannot replace a check identity that vanished
                # between the all/latest observations. Legacy-only contexts
                # remain valid when neither view contains a matching check.
                values.append("NOT_PROVEN")
            # latest is completed_at-based. An unfinished rerun must veto an
            # older green even when that green is the latest completion.
            for row in observed:
                if row.get("head_sha") != sha or row.get("status") != "completed":
                    values.append(check_result(row, sha))
            if run is not None:
                values.append(check_result(run, sha))
                if not any(row["id"] == run["id"] for row in observed):
                    values.append("PENDING")  # population moved between reads
            if status is not None:
                state = status.get("state")
                values.append({"success": "SUCCESS", "pending": "PENDING",
                               "failure": "FAILURE", "error": "FAILURE"}.get(state, "NOT_PROVEN")
                              if isinstance(state, str) else "NOT_PROVEN")
            result = "MISSING" if not values else "SUCCESS"
            for blocked in ("NOT_PROVEN", "STALE", "FAILURE", "CANCELLED", "PENDING", "SKIPPED"):
                if blocked in values:
                    result = blocked
                    break
            results.append({"context": requirement["context"], "app_id": app_id,
                            "result": result, "check_run_id": run["id"] if run else None,
                            "status_id": status["id"] if status else None})
    return results


def wait_for_required_ci(repository, branch, sha, *, timeout, interval,
                         transport=None, clock=time.monotonic, sleep=time.sleep):
    deadline = clock() + timeout
    token = observer.resolve_token()

    def get(path):
        remaining = deadline - clock()
        if remaining <= 0:
            raise NotProven("required CI observation deadline expired")
        fetch = transport or observer.http_transport(token, timeout=min(30, remaining))
        result = fetch(path)
        if clock() >= deadline:
            raise NotProven("required CI observation deadline expired")
        return result

    policy = required_policy(get, repository, branch, sha)
    while True:
        suite_count = check_reference_population(get, repository, sha)
        all_runs = collect_pages(get, repository, sha, "check-runs", filter="all")
        runs = collect_pages(get, repository, sha, "check-runs")
        if suite_count == 0 and (all_runs or runs):
            raise NotProven("check-suite and check-run populations are inconsistent")
        statuses = collect_pages(get, repository, sha, "statuses")
        results = evaluate_required(policy, runs, all_runs, statuses, sha)
        packet = {"repository": repository, "subject_sha": sha, "checks": results}
        if all(row["result"] == "SUCCESS" for row in results):
            # Re-read live subject and policy after check collection. A moved
            # branch or changed context/app requirement invalidates this join.
            if required_policy(get, repository, branch, sha) != policy:
                raise NotProven("required policy changed during CI observation")
            return dict(packet, verdict="PASS")
        if any(row["result"] == "FAILURE" for row in results):
            return dict(packet, verdict="FAIL")
        if any(row["result"] in {"NOT_PROVEN", "STALE", "CANCELLED", "SKIPPED"} for row in results):
            return dict(packet, verdict="NOT_PROVEN")
        remaining = deadline - clock()
        if remaining <= 0:
            return dict(packet, verdict="NOT_PROVEN", reason="required checks never completed")
        if remaining <= interval:
            sleep(remaining)
            return dict(packet, verdict="NOT_PROVEN", reason="required checks never completed")
        sleep(min(interval, remaining))


def main(argv=None, *, transport=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--branch", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=300)
    parser.add_argument("--poll-interval-seconds", type=float, default=30)
    args = parser.parse_args(argv)
    try:
        if not observer.is_hex(args.sha, 40) or args.sha != args.sha.lower():
            raise NotProven("release subject must be an immutable lowercase commit SHA")
        if (not math.isfinite(args.timeout_seconds) or not 0 < args.timeout_seconds <= 600
                or not math.isfinite(args.poll_interval_seconds)
                or not 0 < args.poll_interval_seconds <= args.timeout_seconds):
            raise NotProven("CI polling bounds must be finite, positive, and at most 600 seconds")
        packet = wait_for_required_ci(args.repository, args.branch, args.sha,
                                     timeout=args.timeout_seconds, interval=args.poll_interval_seconds,
                                     transport=transport)
    except (NotProven, observer.ObserverError, enforcement.ContractError) as error:
        packet = {"verdict": "NOT_PROVEN", "reason": str(error)}
    print(json.dumps(packet, sort_keys=True))
    return 0 if packet["verdict"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
