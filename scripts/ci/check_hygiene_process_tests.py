#!/usr/bin/env python3
"""Require executed process tests in the Windows perl-ci-hygiene smoke log.

Cargo/libtest exit zero for an empty filter. This check consumes the pretty,
colorless output of the single binary target; Cargo's status must also pass
(the workflow uses pipefail). It does not establish native-platform coverage.
"""

from pathlib import Path
import re
import sys


def validate(output: str) -> int:
    expected = None
    records: dict[str, str] = {}
    completed = False
    for line in output.splitlines():
        start = re.fullmatch(r"running (\d+) tests?", line)
        record = re.fullmatch(
            r"test (\S+)(?: - should panic)? \.\.\. (ok|ignored|FAILED)(?:, .*)?", line
        )
        summary = re.fullmatch(
            r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
            r"(\d+) measured; \d+ filtered out; finished in .+", line,
        )
        if completed and (start or record or line.startswith("test result:")):
            raise ValueError("unexpected libtest output after completed outer run")
        if start:
            if expected is None:
                expected = int(start[1])
            # Raw-status helpers inherit nested fixture harness output. Their
            # starts/summaries are not the outer suite's completion evidence.
            elif len(records) >= expected:
                raise ValueError("unexpected additional libtest run")
        if record:
            name, status = record.groups()
            if (
                expected is None or not name.startswith("process::tests::")
                or status == "FAILED" or records.get(name, status) != status
            ):
                raise ValueError("unexpected or failed process test record")
            records[name] = status
        if summary and expected is not None and len(records) == expected:
            passed, failed, ignored, measured = map(int, summary.groups())
            if (
                passed > 0 and failed == 0 and measured == 0
                and passed + ignored == expected
                and sum(status == "ok" for status in records.values()) == passed
                and sum(status == "ignored" for status in records.values()) == ignored
            ):
                completed = True
    if not completed:
        raise ValueError("expected completed nonzero process::tests:: execution with consistent counts")
    return sum(status == "ok" for status in records.values())


def main() -> int:
    try:
        if len(sys.argv) != 2:
            raise ValueError("usage: check_hygiene_process_tests.py LOG")
        passed = validate(Path(sys.argv[1]).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        print(f"hygiene process smoke NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    print(f"hygiene process smoke: {passed} intended tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
