#!/usr/bin/env python3
"""Unit tests for bounded-result overflow arithmetic enforcement (#18320)."""

from __future__ import annotations

import json
import sys
import unittest
from decimal import Decimal
from pathlib import Path


_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from check_bounded_result_overflow import integer_count, overflow_errors  # noqa: E402


def overflow_packet(
    *,
    truncated: bool,
    selected: object,
    omitted: object,
    total: object,
) -> dict[str, object]:
    return {
        "overflow": [
            {
                "truncated": truncated,
                "counts": {
                    "selected": selected,
                    "omitted": omitted,
                    "total": total,
                },
            }
        ]
    }


# Discriminators for #18320 (wave 5 uses this module instead of expanding fixtures.json).
PUBLIC_CHECKER_DISCRIMINATORS: tuple[tuple[dict[str, object], bool], ...] = (
    (overflow_packet(truncated=False, selected=2, omitted=0, total=9), True),
    (overflow_packet(truncated=True, selected=2.0, omitted=1.0, total=9.0), True),
    (overflow_packet(truncated=True, selected=2e0, omitted=1e0, total=9e0), True),
    (overflow_packet(truncated=True, selected=2.0, omitted=1.0, total=3.0), False),
    (
        overflow_packet(
            truncated=True,
            selected=9007199254740992.0,
            omitted=1,
            total=9007199254740992.0,
        ),
        True,
    ),
)


class IntegerCountTests(unittest.TestCase):
    def test_integer_valued_floats(self) -> None:
        self.assertEqual(integer_count(2.0), 2)
        self.assertEqual(integer_count(2e0), 2)
        self.assertEqual(integer_count(3), 3)

    def test_decimal_integral_values(self) -> None:
        self.assertEqual(integer_count(Decimal("2.0")), 2)
        self.assertEqual(integer_count(Decimal("9007199254740993")), 9007199254740993)

    def test_non_integer_float_returns_none(self) -> None:
        self.assertIsNone(integer_count(2.5))

    def test_bool_returns_none(self) -> None:
        self.assertIsNone(integer_count(True))


class OverflowErrorsTests(unittest.TestCase):
    def test_nontruncated_sum_mismatch(self) -> None:
        packet = overflow_packet(truncated=False, selected=2, omitted=0, total=9)
        errors = overflow_errors(packet)
        self.assertTrue(any("must equal total" in error for error in errors))

    def test_decimal_encoded_sum_mismatch(self) -> None:
        packet = overflow_packet(truncated=True, selected=2.0, omitted=1.0, total=9.0)
        errors = overflow_errors(packet)
        self.assertTrue(any("must equal total" in error for error in errors))

    def test_exponent_encoded_sum_mismatch(self) -> None:
        packet = overflow_packet(truncated=True, selected=2e0, omitted=1e0, total=9e0)
        errors = overflow_errors(packet)
        self.assertTrue(any("must equal total" in error for error in errors))

    def test_integer_valued_float_matching_sum_accepted(self) -> None:
        packet = overflow_packet(truncated=True, selected=2.0, omitted=1.0, total=3.0)
        self.assertEqual(overflow_errors(packet), [])

    def test_non_integer_fraction_skips_sum_without_crash(self) -> None:
        packet = overflow_packet(truncated=True, selected=2.5, omitted=1, total=3)
        errors = overflow_errors(packet)
        self.assertFalse(any("must equal total" in error for error in errors))

    def test_large_float_encoded_mismatch_not_lost_to_rounding(self) -> None:
        packet = overflow_packet(
            truncated=True,
            selected=9007199254740992.0,
            omitted=1,
            total=9007199254740992.0,
        )
        errors = overflow_errors(packet)
        self.assertTrue(any("must equal total" in error for error in errors))

    def test_fixture_json_decimal_preserves_precision(self) -> None:
        decoded = json.loads(
            '{"selected":9007199254740993.0,"omitted":1,"total":9007199254740993.0}',
            parse_float=Decimal,
        )
        selected = integer_count(decoded["selected"])
        omitted = integer_count(decoded["omitted"])
        total = integer_count(decoded["total"])
        self.assertIsNotNone(selected)
        self.assertIsNotNone(omitted)
        self.assertIsNotNone(total)
        assert selected is not None and omitted is not None and total is not None
        self.assertNotEqual(selected + omitted, total)


class PublicCheckerDiscriminatorTests(unittest.TestCase):
    def test_issue_18320_controls_rejected_by_overflow_errors(self) -> None:
        for packet, expect_error in PUBLIC_CHECKER_DISCRIMINATORS:
            errors = overflow_errors(packet)
            has_sum_error = any("must equal total" in error for error in errors)
            self.assertEqual(
                has_sum_error,
                expect_error,
                msg=(packet, errors),
            )


if __name__ == "__main__":
    unittest.main()
