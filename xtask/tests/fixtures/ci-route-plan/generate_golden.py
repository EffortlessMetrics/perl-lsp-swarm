#!/usr/bin/env python3
"""Independent reference-vector generator for `ci_route_plan.v1` (#10179).

This script is the non-Rust side of the golden-vector proof: it builds the
canonical semantic bytes and full payload bytes from the *specified*
encoding contract (not from the Rust encoder) and computes the
domain-separated SHA-256 fingerprints with Python's hashlib. The frozen
outputs live beside this script:

- `semantic-baseline.json`   canonical semantic projection bytes (baseline)
- `semantic-escaping.json`   canonical bytes exercising the escaping rules
- `payload-baseline.json`    complete canonical published artifact
- `digests.json`             independently computed SHA-256 fingerprints

Canonical encoding contract (mirrors the normative text in
xtask/src/ci_route_plan/canonical.rs):

- UTF-8, no whitespace, object keys sorted, arrays in projected order;
- minimal string escaping (short forms for \\b \\f \\n \\r \\t, \\u00XX for
  the remaining control characters, raw UTF-8 for everything else);
- plain decimal integers; omitted optional values;
- fingerprint = SHA-256(b"ci_route_plan.v1\\0" || canonical semantic bytes).

Regenerate the fixtures with:
    python3 xtask/tests/fixtures/ci-route-plan/generate_golden.py

Verify the checked-in fixtures without writing anything (drift check):
    python3 xtask/tests/fixtures/ci-route-plan/generate_golden.py --check
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

FINGERPRINT_DOMAIN = b"ci_route_plan.v1\0"

SHA_A = "a" * 40
SHA_B = "b" * 40
DIGEST_A = "a" * 64
DIGEST_B = "b" * 64
DIGEST_C = "c" * 64


def canonical_bytes(value: object) -> bytes:
    """Serialize per the canonical contract (independent of Rust)."""
    text = json.dumps(
        value,
        sort_keys=True,
        ensure_ascii=False,
        separators=(",", ":"),
        allow_nan=False,
    )
    return text.encode("utf-8")


def fingerprint(semantic: dict) -> str:
    return hashlib.sha256(FINGERPRINT_DOMAIN + canonical_bytes(semantic)).hexdigest()


def baseline_semantic(*, command: str = "run fmt_gate") -> dict:
    """The baseline semantic projection: one proof-backed run, one scoped
    noop. Tiers are stored in canonical (ascending) order; the Rust-side
    test supplies a different input order and must land on these bytes."""
    return {
        "schema": "ci_route_plan.v1",
        "producer": "xtask::ci_route_plan",
        "subject": {
            "kind": "pull_request",
            "head_sha": SHA_A,
            "base_sha": SHA_B,
            "subject_digest": DIGEST_A,
        },
        "requested_profile": "merge_gate",
        "included_native_tiers": ["merge_gate", "pr_fast"],
        "expansion_fingerprint": DIGEST_B,
        "policy_digest": DIGEST_C,
        "disposition_digest": DIGEST_B,
        "workflow_digest": DIGEST_C,
        "denominator": ["fmt_gate", "unit_gate"],
        "selection": {
            "base": SHA_B,
            "scope_ok": True,
            "fallback_used": False,
            "package_args": ["-p", "perl-parser"],
            "selector_digest": DIGEST_A,
        },
        "rows": [
            {
                "gate_id": "fmt_gate",
                "native_tier": "pr_fast",
                "policy_role": "required",
                "lifecycle": {"state": "active", "resolution": "current"},
                "selector_role": "always_on",
                "selector_placement": "selected",
                "applicability": "applicable",
                "outcome": {
                    "kind": "run",
                    "command": command,
                    "timeout_seconds": 60,
                    "reason": "selected by selector",
                },
            },
            {
                "gate_id": "unit_gate",
                "native_tier": "pr_fast",
                "policy_role": "advisory",
                "lifecycle": {"state": "active", "resolution": "current"},
                "selector_role": "rust_scoped",
                "selector_placement": "skipped",
                "applicability": "not_applicable",
                "outcome": {
                    "kind": "scoped_noop",
                    "reason": "scope selector decided",
                    "selector_digest": DIGEST_A,
                },
            },
        ],
    }


def escaping_semantic() -> dict:
    """Same plan with a command that exercises the escaping rules: quote,
    backslash, newline, tab, a raw control byte, and non-ASCII text."""
    return baseline_semantic(command='echo "é" \\ done\n\t\x01')


def summarize(rows: list) -> dict:
    counts = {"run": 0, "scoped_noop": 0, "quarantined": 0, "error": 0}
    by_role: dict[str, int] = {}
    for row in rows:
        counts[row["outcome"]["kind"]] += 1
        by_role[row["policy_role"]] = by_role.get(row["policy_role"], 0) + 1
    return {
        "governed": len(rows),
        **counts,
        "by_policy_role": by_role,
    }


def payload(semantic: dict) -> dict:
    """Complete published artifact: semantic projection plus the derived
    summary and the fingerprint field itself."""
    complete = dict(semantic)
    complete["summary"] = summarize(semantic["rows"])
    complete["semantic_fingerprint"] = fingerprint(semantic)
    return complete


def build_vectors() -> dict[str, bytes]:
    """Every fixture's expected byte content, keyed by file stem."""
    semantic = {
        "semantic-baseline": baseline_semantic(),
        "semantic-escaping": escaping_semantic(),
    }
    contents = {name: canonical_bytes(value) for name, value in semantic.items()}
    contents["payload-baseline"] = canonical_bytes(payload(baseline_semantic()))
    digests = {name: fingerprint(value) for name, value in semantic.items()}
    contents["digests"] = (
        json.dumps(digests, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")
    return contents


def check_vectors(here: Path, contents: dict[str, bytes]) -> int:
    """Non-mutating drift check: compare every fixture against the
    generator output and fail (exit 1) on any difference."""
    drifted = [
        name
        for name, expected in sorted(contents.items())
        if (here / f"{name}.json").read_bytes() != expected
    ]
    if drifted:
        for name in drifted:
            print(f"fixture drift: {name}.json differs from the generator output", file=sys.stderr)
        print(
            "regenerate with: python3 xtask/tests/fixtures/ci-route-plan/generate_golden.py",
            file=sys.stderr,
        )
        return 1
    print("golden fixtures match the generator output:", ", ".join(sorted(contents)))
    return 0


def main(argv: list[str]) -> int:
    here = Path(__file__).resolve().parent
    contents = build_vectors()
    if argv == ["--check"]:
        return check_vectors(here, contents)
    if argv:
        print(f"usage: {sys.argv[0]} [--check]", file=sys.stderr)
        return 2
    for name, expected in contents.items():
        (here / f"{name}.json").write_bytes(expected)
    print("wrote golden vectors:", ", ".join(sorted(contents)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
