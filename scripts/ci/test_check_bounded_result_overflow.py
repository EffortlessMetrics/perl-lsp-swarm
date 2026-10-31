#!/usr/bin/env python3
"""Unit tests for bounded-result overflow arithmetic enforcement (#18320)."""

from __future__ import annotations

import unittest

from check_bounded_result_overflow import integer_count, overflow_errors


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


class IntegerCountTests(unittest.TestCase):
    def test_integer_valued_floats(self) -> None:
        self.assertEqual(integer_count(2.0), 2)
        self.assertEqual(integer_count(2e0), 2)
        self.assertEqual(integer_count(3), 3)

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


if __name__ == "__main__":
    unittest.main()
