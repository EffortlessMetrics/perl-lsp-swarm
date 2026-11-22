#!/usr/bin/env python3
"""Read the ci-contract receipt status, refusing an envelope this consumer cannot read.

The producer is `xtask ci-contract` (`xtask/src/tasks/ci_contract.rs`), which
writes `schema_version: "ci-contract.v1"` next to the contract status.

A consumer that reads `.status` without checking the envelope version reads
the empty string when a later envelope renames the field, so the
`POLICY_FINDING` comparison falls through and the step exits non-zero with no
diagnostic naming the envelope mismatch. The `// empty` fallback is what makes
that failure silent. This module makes the envelope check explicit: an unknown
`schema_version` is refused, loudly, before any other field is read (#15310).

Exit codes:
  0  the envelope was understood; the contract status is printed on stdout
  1  the receipt is missing, unreadable, not a JSON object, or carries a
     `schema_version` this consumer does not understand. The reason is printed
     on stderr as a GitHub Actions workflow error.

Usage: ci_contract_receipt_status.py <receipt.json>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# The envelope version this consumer understands. The producer declares the same
# constant in `xtask/src/tasks/ci_contract.rs`; the self-test asserts the two
# agree, so a producer bump cannot silently desynchronise the pair.
EXPECTED_SCHEMA_VERSION = "ci-contract.v1"


def fail(reason: str) -> tuple[int, str]:
    """Return the refusal contract: a non-zero code and a workflow annotation."""
    print(f"::error::{reason}", file=sys.stderr)
    return 1, ""


def read_status(receipt_path: Path) -> tuple[int, str]:
    """Return `(exit_code, contract_status)` for the receipt at `receipt_path`.

    A zero exit code means the envelope was understood and `contract_status`
    is the receipt's `status` field. A non-zero exit code means the receipt
    could not be read as this envelope, and the reason has already been
    reported as a workflow error.
    """
    try:
        raw = receipt_path.read_text(encoding="utf-8")
    except OSError as exc:
        return fail(f"ci-contract receipt unreadable at {receipt_path}: {exc}")

    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        return fail(f"ci-contract receipt is not valid JSON at {receipt_path}: {exc}")

    if not isinstance(payload, dict):
        return fail(f"ci-contract receipt at {receipt_path} is not a JSON object")

    actual = payload.get("schema_version")
    if actual != EXPECTED_SCHEMA_VERSION:
        return fail(
            f"unsupported ci-contract schema_version={actual!r}, "
            f"expected {EXPECTED_SCHEMA_VERSION!r} (producer/consumer out of sync)"
        )

    status = payload.get("status")
    # A v1 envelope always carries `status`. An absent field is read as the
    # empty string, which is what the shell consumer did; keeping that here
    # means this change only alters behaviour for an envelope this consumer
    # cannot read, and nothing else.
    return 0, "" if status is None else str(status)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        return fail("usage: ci_contract_receipt_status.py <receipt.json>")[0]

    code, status = read_status(Path(argv[1]))
    if code != 0:
        return code
    print(status)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
