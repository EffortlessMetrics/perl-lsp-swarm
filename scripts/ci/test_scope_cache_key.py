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
import re
import shlex
import unittest
from pathlib import Path

import yaml

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
    TOKMD_WORKFLOW = REPO_ROOT / ".github/workflows/tokmd.yml"

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

    def test_tokmd_pr_cache_restores_without_candidate_writes(self) -> None:
        if not self.TOKMD_WORKFLOW.is_file():
            self.skipTest("tokmd.yml not present in this checkout")

        text = self.TOKMD_WORKFLOW.read_text(encoding="utf-8")
        cache_marker = "- name: Cache Cargo dependencies"
        cache_start = text.index(cache_marker)
        cache_end = text.index("\n      - name:", cache_start + len(cache_marker))
        cache_segment = text[cache_start:cache_end]
        save_if_lines = [
            line.strip()
            for line in cache_segment.splitlines()
            if line.strip().startswith("save-if:")
        ]

        self.assertEqual(
            1,
            text.count("uses: Swatinem/rust-cache@"),
            "tokmd analyze must declare exactly one rust-cache step",
        )
        self.assertIn(
            "cache-on-failure: true",
            cache_segment,
            "tokmd cache should remain reusable after canonical failures",
        )
        self.assertIn(
            "shared-key: tokmd-review-1.10.0",
            cache_segment,
            "tokmd cache namespace must remain version-scoped",
        )
        self.assertEqual(
            1,
            len(save_if_lines),
            "tokmd cache must declare exactly one save boundary",
        )
        save_if = save_if_lines[0].removeprefix("save-if:").strip()
        match = re.fullmatch(
            r"\$\{\{\s*github\.ref\s*==\s*'([^']+)'\s*\|\|\s*"
            r"github\.ref\s*==\s*'([^']+)'\s*\}\}",
            save_if,
        )
        if match is None:
            self.fail(
                "tokmd cache save-if must be an exact two-ref canonical boundary"
            )

        self.assertEqual(
            {"refs/heads/master", "refs/heads/main"},
            set(match.groups()),
            "tokmd cache writes must be limited to canonical branches",
        )
        allowed_refs = set(match.groups())
        for event_ref, expected_write in (
            ("refs/heads/main", True),
            ("refs/heads/master", True),
            ("refs/pull/13101/merge", False),
            ("refs/heads/feature/cache", False),
            ("refs/tags/v1.10.0", False),
        ):
            with self.subTest(event_ref=event_ref):
                self.assertEqual(
                    expected_write,
                    event_ref in allowed_refs,
                    "cache writes must be limited to canonical branch refs",
        )


class PrFastCacheContractTests(unittest.TestCase):
    """Protect the #15528/#19229 source contracts, not artifact compatibility.

    PR Smoke can save a compile-free entry after preflight fails. The two jobs
    need distinct families and must build in rust-cache's default target tree.
    The meta shard provisions the pinned PyYAML reader before these tests.
    """

    JOBS = ("pr-smoke", "pr-fast-unblocked")

    def workflow(self) -> dict:
        return yaml.safe_load(
            (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        )

    def cache_step(self, job: dict) -> dict:
        caches = [
            step for step in job["steps"]
            if step.get("uses", "").startswith("Swatinem/rust-cache@")
        ]
        self.assertEqual(1, len(caches), "missing or duplicate rust-cache step")
        return caches[0]

    def warm_step(self, job: dict) -> dict:
        steps = [step for step in job["steps"] if step.get("name") == "Warm xtask"]
        self.assertEqual(1, len(steps), "missing or duplicate Warm xtask step")
        self.assertIn("cargo build", steps[0]["run"])
        return steps[0]

    def check_cache_contract(self, text: str) -> None:
        workflow = yaml.safe_load(text)
        for job_name in self.JOBS:
            job = workflow["jobs"][job_name]
            cache = self.cache_step(job)
            self.warm_step(job)
            options = cache.get("with", {})
            self.assertEqual(
                job_name, options.get("shared-key"),
                f"{job_name} needs its own stable family without a lockfile prefix",
            )
            self.assertNotIn(
                "workspaces", options,
                "review target alignment if rust-cache's default target changes",
            )
            self.assertIn(options.get("cache-targets", True), (True, "true"))
            # Actions resolves env from workflow to job to step. In particular,
            # a cache-step override cannot repair a run step's inherited path.
            job_env = {**workflow.get("env", {}), **job.get("env", {})}
            for step in job["steps"]:
                if "run" not in step and step is not cache:
                    continue
                effective_env = {**job_env, **step.get("env", {})}
                self.assertEqual(
                    "target", effective_env.get("CARGO_TARGET_DIR"),
                    f"{job_name}/{step.get('name')} must use the stable cached target tree",
                )
                # This bounded guard catches explicit inline/export/GITHUB_ENV
                # assignments, not arbitrary shell or called-script behavior.
                # The diagnostic is harmless only as an unredirected command.
                for line in step.get("run", "").splitlines():
                    command = line.strip()
                    if command.startswith("#"):
                        continue
                    if "CARGO_TARGET_DIR=" in command:
                        self.assertEqual(
                            'echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-<unset>}"',
                            command,
                            f"{job_name} must not override the cached target in a run step",
                        )
                    # Cargo's explicit CLI target overrides even a correct env.
                    # Read the next shell word, accepting the existing helpers'
                    # inherited-target spelling and equivalent literal paths.
                    for flag in re.finditer(r"--target-dir(?:\s+|=)", command):
                        words = shlex.shlex(command[flag.end():], posix=True, punctuation_chars=True)
                        words.whitespace_split = True
                        self.assertIn(
                            words.get_token(),
                            ("target", "./target", "$CARGO_TARGET_DIR",
                             "${CARGO_TARGET_DIR}", "${CARGO_TARGET_DIR:-target}"),
                            f"{job_name} CLI must use the stable cached target tree",
                        )

    def check_document(self, workflow: dict) -> None:
        self.check_cache_contract(yaml.safe_dump(workflow, sort_keys=False))

    def test_pr_fast_cache_families_remain_separate_and_target_aligned(self) -> None:
        self.check_cache_contract(
            (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        )

    def test_each_job_rejects_colliding_and_lockfile_prefixed_families(self) -> None:
        for name, other in (self.JOBS, self.JOBS[::-1]):
            for key in (other, name + "-${{ hashFiles('Cargo.lock') }}"):
                with self.subTest(job=name, key=key):
                    workflow = self.workflow()
                    self.cache_step(workflow["jobs"][name])["with"]["shared-key"] = key
                    with self.assertRaisesRegex(AssertionError, "own stable family"):
                        self.check_document(workflow)

    def test_each_job_rejects_wrong_effective_target_at_every_env_scope(self) -> None:
        for name in self.JOBS:
            for scope in ("workflow", "job", "step", "cache-only"):
                with self.subTest(job=name, scope=scope):
                    workflow = self.workflow()
                    job = workflow["jobs"][name]
                    bad = "/mnt/run-${{ github.run_id }}"
                    if scope in ("workflow", "cache-only"):
                        del job["env"]["CARGO_TARGET_DIR"]
                        workflow["env"]["CARGO_TARGET_DIR"] = bad
                        if scope == "cache-only":
                            self.cache_step(job)["env"] = {"CARGO_TARGET_DIR": "target"}
                    elif scope == "job":
                        job["env"]["CARGO_TARGET_DIR"] = bad
                    else:
                        self.warm_step(job)["env"] = {"CARGO_TARGET_DIR": bad}
                    with self.assertRaisesRegex(AssertionError, "stable cached target"):
                        self.check_document(workflow)

    def test_equivalent_env_scopes_and_shadowed_workflow_target_are_accepted(self) -> None:
        for name in self.JOBS:
            for variant in ("workflow", "step", "cache", "shadowed"):
                with self.subTest(job=name, variant=variant):
                    workflow = self.workflow()
                    job = workflow["jobs"][name]
                    if variant == "workflow":
                        del job["env"]["CARGO_TARGET_DIR"]
                        workflow["env"]["CARGO_TARGET_DIR"] = "target"
                    elif variant == "shadowed":
                        workflow["env"]["CARGO_TARGET_DIR"] = "/mnt/run-${{ github.run_id }}"
                    else:
                        step = self.warm_step(job) if variant == "step" else self.cache_step(job)
                        step["env"] = {"CARGO_TARGET_DIR": "target"}
                    self.check_document(workflow)

    def test_quoted_values_and_reformatted_yaml_are_accepted(self) -> None:
        text = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        for quote in ("'", '"'):
            with self.subTest(quote=quote):
                mutant = text.replace("CARGO_TARGET_DIR: target", f"CARGO_TARGET_DIR: {quote}target{quote} # stable")
                for name in self.JOBS:
                    mutant = mutant.replace(f"shared-key: {name}\n", f"shared-key: {quote}{name}{quote}\n")
                self.assertNotEqual(text, mutant)
                self.check_cache_contract(mutant)
        self.check_document(self.workflow())

    def test_each_job_rejects_explicit_shell_target_overrides(self) -> None:
        for name in self.JOBS:
            for command in (
                'echo "CARGO_TARGET_DIR=/mnt/run-${{ github.run_id }}" >> "$GITHUB_ENV"',
                'CARGO_TARGET_DIR=/mnt/run-${{ github.run_id }} cargo build -p xtask --locked',
                'export CARGO_TARGET_DIR=/mnt/run-${{ github.run_id }}',
                'echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-<unset>}" >> "$GITHUB_ENV"',
            ):
                with self.subTest(job=name, command=command):
                    workflow = self.workflow()
                    self.warm_step(workflow["jobs"][name])["run"] = "cargo build -p xtask --locked\n" + command
                    with self.assertRaisesRegex(AssertionError, "must not override"):
                        self.check_document(workflow)

    def test_missing_warm_step_cannot_borrow_the_sibling_job(self) -> None:
        for name in self.JOBS:
            with self.subTest(job=name):
                workflow = self.workflow()
                job = workflow["jobs"][name]
                job["steps"].remove(self.warm_step(job))
                with self.assertRaisesRegex(AssertionError, "Warm xtask"):
                    self.check_document(workflow)

    def test_explicit_target_flags_reject_redirection_and_accept_equivalents(self) -> None:
        for name in self.JOBS:
            for separator in (" ", "="):
                for target in ('"target"', "./target", '"${CARGO_TARGET_DIR:-target}"', "/mnt/not-cached"):
                    with self.subTest(job=name, separator=separator, target=target):
                        workflow = self.workflow()
                        self.warm_step(workflow["jobs"][name])["run"] += f" --target-dir{separator}{target}"
                        if target == "/mnt/not-cached":
                            with self.assertRaisesRegex(AssertionError, "CLI must use the stable cached target"):
                                self.check_document(workflow)
                        else:
                            self.check_document(workflow)

if __name__ == "__main__":
    unittest.main()
