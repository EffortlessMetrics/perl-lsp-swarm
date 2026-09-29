#!/usr/bin/env python3
"""Fail-closed classification of live required and advisory PR checks.

Input is a JSON object with pr, rules, classic, required_checks, and evidence.
The shell guard obtains the first four values from GitHub. Evidence is a
maintainer-reviewed, head-bound JSON document, never a substitute for policy.
"""

import json
import sys


GOOD = {"SUCCESS", "NEUTRAL", "SKIPPED"}
RED = {"FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED", "CANCELLED"}
ALLOWED = {"inherited", "instrument", "environment", "not_proven_nonmaterial"}


def fail(message):
    print(f"FAIL pre-merge status: {message}", file=sys.stderr)
    return 1


def identity(check):
    return (check.get("name", check.get("context")), check.get("detailsUrl", check.get("targetUrl")))


def result(check):
    return check.get("conclusion", check.get("state", check.get("status", ""))).upper()


def main():
    try:
        data = json.load(sys.stdin)
        pr = data["pr"]
        head = pr["headRefOid"]
        checks = pr["statusCheckRollup"]
        rules = data["rules"]
        classic = data["classic"]
        required_checks = data["required_checks"]
        if not isinstance(checks, list) or not isinstance(rules, list):
            return fail("GitHub check or ruleset response has the wrong shape")
        required = set(classic.get("contexts", []))
        required.update(item["context"] for item in classic.get("checks", []))
        for rule in rules:
            if rule.get("type") == "required_status_checks":
                required.update(item["context"] for item in rule["parameters"]["required_status_checks"])
        if not required:
            return fail("live policy supplied no required contexts")
        observed_required = {item["name"] for item in required_checks}
        if observed_required != required:
            return fail(f"required policy and gh pr checks disagree: policy={sorted(required)}, checks={sorted(observed_required)}")
        by_name = {}
        for check in checks:
            by_name.setdefault(identity(check)[0], []).append(check)
        for name in sorted(required):
            matches = by_name.get(name, [])
            if not matches:
                return fail(f"required context missing: {name}")
            if not any(result(check) == "SUCCESS" for check in matches):
                return fail(f"required context is not successful: {name}")
            if not any(item["name"] == name and item["state"] == "SUCCESS" for item in required_checks):
                return fail(f"gh pr checks does not report required success: {name}")
        reds = {identity(check) for check in checks if identity(check)[0] not in required and result(check) in RED}
        pending = {identity(check) for check in checks if identity(check)[0] not in required and result(check) not in GOOD | RED}
        if pending:
            return fail(f"advisory result pending or unknown: {sorted(pending)}")
        if not reds:
            print("OK pre-merge status: required contexts green and no advisory reds")
            return 0
        evidence = data.get("evidence")
        if not isinstance(evidence, dict) or evidence.get("headRefOid") != head:
            return fail("advisory reds require evidence bound to the current PR head")
        entries = evidence.get("advisories")
        if not isinstance(entries, list):
            return fail("advisory evidence must contain an advisories list")
        mapped = {}
        for entry in entries:
            key = (entry.get("name"), entry.get("detailsUrl"))
            if key in mapped:
                return fail(f"duplicate advisory evidence: {key}")
            mapped[key] = entry
        if set(mapped) != reds:
            return fail(f"advisory evidence identities differ from live reds: missing={sorted(reds - set(mapped))}, extra={sorted(set(mapped) - reds)}")
        for key, entry in mapped.items():
            classification = entry.get("classification")
            if classification not in ALLOWED:
                return fail(f"candidate-owned or unclassified advisory blocks: {key} ({classification})")
            if not entry.get("discriminator") or not entry.get("evidenceUrl"):
                return fail(f"advisory requires a named discriminator and evidence URL: {key}")
            if classification == "inherited" and not entry.get("mergeBaseRunUrl"):
                return fail(f"inherited advisory requires matching merge-base run evidence: {key}")
            if classification == "not_proven_nonmaterial" and not entry.get("nonmaterialReason"):
                return fail(f"NOT_PROVEN advisory requires an explicit nonmaterial reason: {key}")
        print(f"OK pre-merge status: {len(required)} required contexts green; {len(reds)} advisory reds explicitly classified")
        return 0
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        return fail(f"invalid GitHub or evidence data: {exc}")


if __name__ == "__main__":
    sys.exit(main())
