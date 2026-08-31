#!/usr/bin/env python3
"""Deterministic key-composition tests for scripts/ci/scope_cache_key.py (#2908).

These are the falsifiers the scope-aware cache-key claim rests on:

- a force-push that does not move Cargo.lock or the crate set must produce a
  byte-identical hash (stability across order/duplicate/whitespace noise);
- adding an unrelated crate to one set must not change another set's hash;
- composition is pinned to an exact known vector so algorithm drift fails
  loudly instead of silently re-partitioning every cache lane.
"""

from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import os
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("scope_cache_key.py")
SPEC = importlib.util.spec_from_file_location("scope_cache_key", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
scope_key = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scope_key)

REPO_ROOT = Path(
    os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2])
)


def expected_hash(canonical: str, length: int = 16) -> str:
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()[:length]


class StabilityTests(unittest.TestCase):
    """Same crate set => byte-identical hash regardless of presentation."""

    def test_order_and_duplicates_and_whitespace_do_not_move_the_hash(self) -> None:
        baseline = scope_key.scope_cache_key("-p perl-uri -p perl-workspace")
        self.assertEqual("668db3399e4546be", baseline)
        for variant in (
            "-p perl-workspace -p perl-uri",
            "-p   perl-uri   -p perl-workspace",
            "-p perl-uri -p perl-workspace -p perl-uri",
            "-p perl-workspace -p perl-uri -p perl-workspace -p perl-uri",
            "  -p perl-uri -p perl-workspace  ",
        ):
            with self.subTest(variant=variant):
                self.assertEqual(baseline, scope_key.scope_cache_key(variant))

    def test_empty_set_is_a_deterministic_value(self) -> None:
        empty_hash = hashlib.sha256(b"").hexdigest()
        self.assertEqual(
            expected_hash(""),
            scope_key.scope_cache_key(""),
        )
        self.assertEqual(empty_hash[:16], scope_key.scope_cache_key(""))
        self.assertEqual(
            scope_key.scope_cache_key(""), scope_key.scope_cache_key("")
        )

    def test_single_flag_alias_is_accepted(self) -> None:
        self.assertEqual(
            scope_key.scope_cache_key("--package perl-uri"),
            scope_key.scope_cache_key("-p perl-uri"),
        )

    def test_known_vector_pins_composition(self) -> None:
        # Canonical sorted form: perl-lexer < perl-parser < perl-parser-core.
        self.assertEqual(
            "07e20081041e87f0",
            scope_key.scope_cache_key(
                "-p perl-parser -p perl-lexer -p perl-parser-core"
            ),
        )


class PartitionTests(unittest.TestCase):
    """Different crate sets => different hashes; unrelated growth stays local."""

    def test_superset_changes_only_its_own_hash(self) -> None:
        two_crate = scope_key.scope_cache_key("-p perl-uri -p perl-workspace")
        three_crate = scope_key.scope_cache_key(
            "-p perl-uri -p perl-workspace -p perl-dap"
        )
        self.assertNotEqual(two_crate, three_crate)

    def test_disjoint_sets_do_not_collide(self) -> None:
        left = scope_key.scope_cache_key("-p perl-parser")
        right = scope_key.scope_cache_key("-p perl-lsp-rs")
        self.assertNotEqual(left, right)

    def test_unrelated_addition_does_not_invalidate_original_scope(self) -> None:
        original = scope_key.scope_cache_key("-p perl-uri -p perl-workspace")
        recomputed = scope_key.scope_cache_key("-p perl-workspace -p perl-uri")
        self.assertEqual(original, recomputed)


class KnownVectorTests(unittest.TestCase):
    def test_exact_sha256_truncation(self) -> None:
        canonical = "perl-uri\nperl-workspace"
        self.assertEqual(
            expected_hash(canonical),
            scope_key.scope_cache_key("-p perl-workspace -p perl-uri"),
        )


class LengthTests(unittest.TestCase):
    def test_length_bounds_are_honored(self) -> None:
        for length in (8, 16, 32, 64):
            with self.subTest(length=length):
                value = scope_key.scope_cache_key("-p perl-uri", length=length)
                self.assertEqual(length, len(value))
                self.assertEqual(
                    hashlib.sha256(b"perl-uri").hexdigest()[:length], value
                )

    def test_out_of_bounds_lengths_fail_closed(self) -> None:
        for length in (0, 7, 65, -1):
            with self.subTest(length=length):
                with self.assertRaises(ValueError):
                    scope_key.scope_cache_key("-p perl-uri", length=length)


class MalformedInputTests(unittest.TestCase):
    def test_rejects_bare_names_unknown_flags_and_dangling_flags(self) -> None:
        for bad in (
            "perl-uri",
            "-p",
            "--package",
            "-p perl-uri extra-token",
            "--unknown perl-uri",
            "-p 'quoted name'",
            "-p ../escape",
            "-p perl uri",
        ):
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    scope_key.parse_package_args(bad)


class CliTests(unittest.TestCase):
    def test_main_prints_hash_and_exits_zero(self) -> None:
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            status = scope_key.main(["--package-args", "-p perl-uri"])
        self.assertEqual(0, status)
        self.assertEqual(
            scope_key.scope_cache_key("-p perl-uri") + "\n",
            buffer.getvalue(),
        )

    def test_main_fails_closed_with_status_two_on_malformed_input(self) -> None:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = scope_key.main(["--package-args", "not-a-flag"])
        self.assertEqual(2, status)
        self.assertIn("error:", stderr.getvalue())

    def test_main_requires_a_non_empty_scope_when_the_consumer_requests_it(self) -> None:
        for package_args in ("", "   \t"):
            with self.subTest(package_args=package_args):
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    status = scope_key.main(
                        ["--require-non-empty", "--package-args", package_args]
                    )
                self.assertEqual(2, status)
                self.assertIn("must not be empty", stderr.getvalue())

        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            status = scope_key.main(
                [
                    "--require-non-empty",
                    "--package-args",
                    "-p perl-uri",
                ]
            )
        self.assertEqual(0, status)
        self.assertRegex(stdout.getvalue().strip(), r"^[0-9a-f]{16}$")

    def test_workflow_invocation_shape_is_supported(self) -> None:
        """The exact invocation ci.yml uses must parse and print one hex line."""
        buffer = io.StringIO()
        argv = [
            "--package-args",
            "-p perl-uri -p perl-workspace",
            "--length",
            "16",
        ]
        with contextlib.redirect_stdout(buffer):
            status = scope_key.main(argv)
        self.assertEqual(0, status)
        printed = buffer.getvalue().strip()
        self.assertRegex(printed, r"^[0-9a-f]{16}$")


class WorkflowContractTests(unittest.TestCase):
    """The consuming workflow step must stay aligned with the helper (#2908)."""

    WORKFLOW = REPO_ROOT / ".github/workflows/ci.yml"

    @classmethod
    def setUpClass(cls) -> None:
        if not cls.WORKFLOW.is_file():
            raise unittest.SkipTest("ci.yml not present in this checkout")

    def test_windows_lane_hashes_the_scope_before_the_cache_step(self) -> None:
        text = self.WORKFLOW.read_text(encoding="utf-8")
        derive_index = text.index("Derive scope-aware cache key component")
        cache_index = text.index("Cache cargo dependencies", derive_index)
        derive_segment = text[derive_index:cache_index]
        shared_key_segment = text[cache_index : cache_index + 600]
        self.assertIn(
            "steps.windows-scope-key.outputs.scope-hash",
            shared_key_segment,
            "windows-platform-smoke must hash the scope set into its shared-key",
        )
        self.assertIn(
            "hashFiles('Cargo.lock')",
            shared_key_segment,
            "the lockfile component must remain part of the scoped key",
        )
        self.assertIn(
            "scripts/ci/scope_cache_key.py --require-non-empty --package-args",
            derive_segment,
            "the derivation step must call the canonical helper fail-closed",
        )
        self.assertIn(
            "save-if: ${{ github.ref == 'refs/heads/master' || "
            "github.ref == 'refs/heads/main' }}",
            shared_key_segment,
            "the scoped cache must remain restore-only on pull requests",
        )
        self.assertLess(
            derive_index,
            cache_index,
            "the scope hash must exist before rust-cache consumes it",
        )

    def test_windows_lane_rejects_a_missing_or_empty_scope_before_hashing(self) -> None:
        text = self.WORKFLOW.read_text(encoding="utf-8")
        derive_index = text.index("Derive scope-aware cache key component")
        cache_index = text.index("Cache cargo dependencies", derive_index)
        derive_segment = text[derive_index:cache_index]
        self.assertIn(
            "scripts/ci/scope_cache_key.py --require-non-empty --package-args",
            derive_segment,
            "the Windows helper invocation must require a non-empty scope",
        )

    def test_platform_overrides_exports_the_crate_set_its_consumers_read(self) -> None:
        """Producer→consumer binding: a needs.* read of windows_test_crates
        resolves empty unless the platform-overrides job exports it."""
        text = self.WORKFLOW.read_text(encoding="utf-8")
        job_start = text.index("platform-overrides:")
        job_end = text.index("windows-platform-smoke:", job_start)
        job_segment = text[job_start:job_end]
        outputs_start = job_segment.index("outputs:")
        outputs_end = job_segment.index("steps:", outputs_start)
        self.assertIn(
            "windows_test_crates: "
            "${{ steps.scope.outputs.windows_test_crates }}",
            job_segment[outputs_start:outputs_end],
            "platform-overrides must export windows_test_crates; an "
            "unexported step output makes every needs.* consumer resolve "
            "the empty set",
        )
        self.assertGreaterEqual(
            text.count("needs.platform-overrides.outputs.windows_test_crates"),
            1,
            "the scope-hash and smoke steps must keep reading the exported "
            "job output, not re-derive it",
        )

    def test_ready_tier_keys_stay_byte_identical_to_workspace_wide_form(self) -> None:
        text = self.WORKFLOW.read_text(encoding="utf-8")
        for lane_marker in (
            "shared-key: ci-gate-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-ux-tests-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-all-targets-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-lsp-memory-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-platform-scope-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-contract-${{ hashFiles('Cargo.lock') }}",
        ):
            self.assertIn(
                lane_marker,
                text,
                f"ready-tier/workspace-wide keys must remain exactly {lane_marker!r}",
            )
        # Exactly ONE scoped key may carry the scope-hash suffix.
        self.assertEqual(
            1,
            text.count("${{ steps.windows-scope-key.outputs.scope-hash }}"),
            "only windows-platform-smoke may consume the scope-hash output",
        )


if __name__ == "__main__":
    unittest.main()
