#!/usr/bin/env python3
"""Fail-closed Policy Validators aggregate decision (#15854).

The required check identity is `Validate CI policy ledgers`. Trusted
`em-ci-nano` is an optimization, not proof semantics: the portable
stdlib-only validators already ran on `ubuntu-24.04`. This helper is the
single decision table for that required identity.

- trusted success + hosted success → pass (agreement)
- trusted success + hosted non-success → fail (divergence)
- trusted non-success + hosted success → pass (failover)
- any other pair → fail (fail-closed)

Trusted non-success includes failure, skip, cancel, and an outage that
never produced a conclusion. Hosted failure never passes.
"""

from __future__ import annotations

import argparse
import sys

AGREE_MESSAGE = "Policy Validators trusted result: both lanes agree."
DIVERGENCE_PREFIX = "lane divergence: trusted lane passed but fallback is "
FAILOVER_MESSAGE = "Policy Validators failover result: trusted lane {nano}, hosted passed."
FAILED_MESSAGE = "Policy Validators failed (trusted {nano}, hosted {hosted})"


def decide(nano: str, hosted: str) -> tuple[int, str]:
    """Return (exit_code, message) for one trusted/hosted pair."""
    trusted = nano.strip() or "pending"
    fallback = hosted.strip() or "pending"
    if trusted == "success":
        if fallback == "success":
            return 0, AGREE_MESSAGE
        return 1, f"{DIVERGENCE_PREFIX}{fallback}"
    if fallback == "success":
        return 0, FAILOVER_MESSAGE.format(nano=trusted)
    return 1, FAILED_MESSAGE.format(nano=trusted, hosted=fallback)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Fail-closed Policy Validators aggregate decision."
    )
    parser.add_argument("nano", help="Trusted-lane conclusion or pending")
    parser.add_argument("hosted", help="validate-hosted job result")
    args = parser.parse_args(argv)
    code, message = decide(args.nano, args.hosted)
    if code == 0:
        print(message)
    else:
        print(f"::error::{message}")
    return code


if __name__ == "__main__":
    sys.exit(main())
