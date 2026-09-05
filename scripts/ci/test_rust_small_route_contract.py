#!/usr/bin/env python3
"""Route-shape contract for the canonical Rust Small proof lane (#8407).

Every required Rust Small route (`rust-small-cx53`, `rust-small-cx43`,
`rust-small-github`, `rust-small-fallback`) must invoke exactly one shared,
candidate/toolchain/profile-honest command:

    cargo run -p xtask --locked -- rust-small-proof

The semantic step list (fetch, workspace check, parser smokes, LSP smoke,
references scorecard census + execution, diff hygiene) lives in the xtask task
(`xtask/src/tasks/rust_small_proof.rs`) so the aggregate gate means one proof
on every route. Duplicated per-runner shell lists previously drifted: the
scorecard census counted `awk "/: test$/{...}"` on CX routes and
`grep -c -F ": test"` on hosted routes — two different proofs behind one
required check. The consolidation keeps the stricter suffix-marker semantics
in exactly one place.

Red-first contract (issue #10064 slice): mutating ANY single lane's copy of
the canonical invocation — argument drift, commenting out, echo decoy, shadow
duplicate — must fail this contract WITH THE SITE NAMED, so a silent revert
fails the required "Perl LSP Rust Small Result" check instead of drifting
back to per-runner copies.

`cargo fmt --all -- --check` intentionally remains pinned as a literal yml
line by scripts/ci/test_rustfmt_required_workflow.py (#9127/#12320); that
claim owns its placement and this file does not move it.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"

RUST_SMALL_LANE_JOBS = (
    "rust-small-cx53",
    "rust-small-cx43",
    "rust-small-github",
    "rust-small-fallback",
)
RUST_SMALL_RESULT_JOB = "rust-small-result"
CANONICAL_INVOCATION = "cargo run -p xtask --locked -- rust-small-proof"
CONTRACT_TEST_FILE = "scripts/ci/test_rust_small_route_contract.py"

# Inline fragments whose presence in an active (non-comment) line of a lane job
# means the duplicated route-command list or its drifted counter came back.
LEGACY_INLINE_FRAGMENTS = (
    'awk "/: test',  # cx53/cx43 census drift (#8407 receipt)
    "grep -c -F",  # hosted/fallback census drift (#8407 receipt)
    "-p perl-parser --test semantic_smoke_tests",
    "-p perl-parser --test parser_accuracy_e2e",
    "-p perl-lsp-rs --test lsp_smoke",
    "references_tier_scorecard_tests",
    "cargo fetch --locked",
    "cargo check --workspace --locked",
    "INSTA_UPDATE=no",
)

# Per-site field mutations that each independently prove the pin is red-first.
INVOCATION_MUTATIONS = (
    ("argument drift (--locked dropped)", CANONICAL_INVOCATION.replace(" --locked", "")),
    ("commented out", f"# {CANONICAL_INVOCATION}"),
    ("echo decoy", f'echo "{CANONICAL_INVOCATION}"'),
)

# The only cargo commands a lane may run besides the canonical invocation:
# runner instrumentation and the fmt literal owned by
# test_rustfmt_required_workflow.py. Anything else (an extra semantic test, a
# second cargo run, a clippy/check/build side gate) would make the route
# choice change what the required status means (#8408 negative control).
ALLOWED_CARGO_COMMANDS = (
    "cargo --version",
    "cargo fmt --all -- --check",
    CANONICAL_INVOCATION,
)


def load_workflow_text() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def job_start_index(workflow_text: str, job_id: str) -> int:
    return workflow_text.index(f"\n  {job_id}:\n")


def mutate_first_in_job(workflow_text: str, job_id: str, old: str, new: str) -> str:
    """Replace the first `old` at-or-after `job_id`'s header with `new`.

    Anchoring at the job header makes every mutation test site-specific even
    when several lanes contain byte-identical copies.
    """
    start = job_start_index(workflow_text, job_id)
    position = workflow_text.index(old, start)
    return workflow_text[:position] + new + workflow_text[position + len(old) :]


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for line in workflow_text.splitlines():
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            in_jobs = False
            current = None
            continue
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.rstrip().endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def active_code_lines(text: str) -> list[str]:
    lines: list[str] = []
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(raw)
    return lines


def canonical_invocation_count(job_body: str) -> int:
    return sum(
        1
        for line in active_code_lines(job_body)
        if line.strip().strip("'\"") == CANONICAL_INVOCATION
    )


def validate_rust_small_route_contract(workflow_text: str) -> None:
    jobs = job_bodies(workflow_text)

    missing = [job_id for job_id in RUST_SMALL_LANE_JOBS if job_id not in jobs]
    if missing:
        raise AssertionError(f"required Rust Small lane jobs missing: {missing}")

    for job_id in RUST_SMALL_LANE_JOBS:
        body = jobs[job_id]
        active = "\n".join(active_code_lines(body))
        invocations = canonical_invocation_count(body)
        if invocations != 1:
            raise AssertionError(
                f"{job_id} (field: canonical-invocation) must invoke the "
                f"canonical Rust Small proof exactly once ({CANONICAL_INVOCATION!r}); "
                f"found {invocations}. Route drift between runners made one "
                "aggregate check mean two different proofs (#8407)."
            )
        for fragment in LEGACY_INLINE_FRAGMENTS:
            if fragment in active:
                raise AssertionError(
                    f"{job_id} (field: semantic-step-body) reintroduced an inline "
                    f"duplicated lane step or drifted counter ({fragment!r}). The "
                    "semantic Rust Small proof is owned by "
                    "`cargo xtask rust-small-proof`; fix the task, not a "
                    "per-route copy (#8407)."
                )
        for cargo_line in (
            line.strip() for line in active_code_lines(body) if line.strip().startswith("cargo ")
        ):
            if cargo_line not in ALLOWED_CARGO_COMMANDS:
                raise AssertionError(
                    f"{job_id} (field: cargo-allowlist) runs unowned cargo "
                    f"command {cargo_line!r}. A route may add only runner "
                    "instrumentation around the canonical invocation; an extra "
                    "semantic command makes the route choice change what the "
                    "required status means (#8408 negative control)."
                )

    result_job = jobs.get(RUST_SMALL_RESULT_JOB)
    if not isinstance(result_job, str):
        raise AssertionError(
            f"{RUST_SMALL_RESULT_JOB} (field: aggregate-job) is missing"
        )
    result_active = "\n".join(active_code_lines(result_job))
    if CONTRACT_TEST_FILE not in result_active:
        raise AssertionError(
            f"{RUST_SMALL_RESULT_JOB} (field: contract-suite-list) must run "
            f"{CONTRACT_TEST_FILE} so a silent revert of the canonical-route "
            "consolidation fails a required check (#8407)"
        )


class RustSmallRouteContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow_text = load_workflow_text()

    # ── green path ──────────────────────────────────────────────────────────

    def test_checked_in_workflow_matches_canonical_route(self) -> None:
        validate_rust_small_route_contract(self.workflow_text)

    def test_every_route_invocation_is_byte_identical(self) -> None:
        jobs = job_bodies(self.workflow_text)
        seen: set[str] = set()
        for job_id in RUST_SMALL_LANE_JOBS:
            occurrences = [
                line.strip()
                for line in active_code_lines(jobs[job_id])
                if line.strip().strip("'\"") == CANONICAL_INVOCATION
            ]
            self.assertEqual(len(occurrences), 1, f"{job_id}: {occurrences}")
            seen.add(occurrences[0])
        self.assertEqual(seen, {CANONICAL_INVOCATION})

    # ── red-first per-site pins: any single mutated copy fails, named ───────

    def test_any_single_lane_invocation_mutation_fails_named_site(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            for mutation_name, replacement in INVOCATION_MUTATIONS:
                with self.subTest(
                    site=job_id, field="canonical-invocation", mutation=mutation_name
                ):
                    broken = mutate_first_in_job(
                        self.workflow_text, job_id, CANONICAL_INVOCATION, replacement
                    )
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    message = str(context.exception)
                    self.assertIn(job_id, message, message)

    def test_shadow_duplicate_in_any_single_lane_fails_named_site(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            with self.subTest(site=job_id, field="canonical-invocation"):
                broken = mutate_first_in_job(
                    self.workflow_text,
                    job_id,
                    CANONICAL_INVOCATION,
                    f"{CANONICAL_INVOCATION}\n              {CANONICAL_INVOCATION}",
                )
                with self.assertRaises(AssertionError) as context:
                    validate_rust_small_route_contract(broken)
                message = str(context.exception)
                self.assertIn(job_id, message, message)
                self.assertIn("exactly once", message, message)

    def test_reintroduced_drifted_counter_fails_named_site_per_fragment(self) -> None:
        legacy_counters = (
            'scorecard_tests=$(cargo test -p perl-lsp-rs --lib --features '
            'workspace --profile agent --locked references_tier_scorecard_tests '
            '-- --list | awk "/: test\\$/{count++} END {print count+0}")',
            "scorecard_tests=$(cargo test -p perl-lsp-rs --lib --features "
            "workspace --profile agent --locked references_tier_scorecard_tests "
            "-- --list | grep -c -F ': test' || true)",
        )
        for job_id in RUST_SMALL_LANE_JOBS:
            for legacy in legacy_counters:
                with self.subTest(
                    site=job_id, field="semantic-step-body", legacy=legacy[:60]
                ):
                    broken = mutate_first_in_job(
                        self.workflow_text,
                        job_id,
                        CANONICAL_INVOCATION,
                        f"{legacy}\n              {CANONICAL_INVOCATION}",
                    )
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    message = str(context.exception)
                    self.assertIn(job_id, message, message)

    def test_reintroduced_semantic_step_body_fails_named_site(self) -> None:
        legacy_steps = (
            "cargo fetch --locked",
            "cargo check --workspace --locked",
            "cargo test --locked -p perl-parser --test parser_accuracy_e2e -- --nocapture",
            "INSTA_UPDATE=no cargo test -p perl-lsp-rs --lib",
        )
        for job_id in RUST_SMALL_LANE_JOBS:
            for legacy in legacy_steps:
                with self.subTest(
                    site=job_id, field="semantic-step-body", legacy=legacy[:50]
                ):
                    broken = mutate_first_in_job(
                        self.workflow_text,
                        job_id,
                        CANONICAL_INVOCATION,
                        f"{legacy}\n              {CANONICAL_INVOCATION}",
                    )
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    message = str(context.exception)
                    self.assertIn(job_id, message, message)

    def test_extra_semantic_cargo_command_in_any_lane_fails_named_site(self) -> None:
        # #8408 negative control: a hosted route running an extra semantic
        # test (or any unowned cargo command) must fail with the site named,
        # not silently broaden what one route's green means.
        extras = (
            "cargo test --locked -p perl-lsp-rs --test route_parity_extra",
            "cargo clippy --workspace --locked -- -D warnings",
            "cargo run -p xtask --locked -- some-other-proof",
        )
        for job_id in RUST_SMALL_LANE_JOBS:
            for extra in extras:
                with self.subTest(site=job_id, field="cargo-allowlist", extra=extra[:50]):
                    broken = mutate_first_in_job(
                        self.workflow_text,
                        job_id,
                        CANONICAL_INVOCATION,
                        f"{CANONICAL_INVOCATION}\n              {extra}",
                    )
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    message = str(context.exception)
                    self.assertIn(job_id, message, message)
                    self.assertIn("cargo-allowlist", message, message)
                    self.assertIn(extra, message, message)

    def test_missing_lane_job_fails_closed(self) -> None:
        lines = [
            line
            for line in self.workflow_text.splitlines()
            if not line.startswith(("  rust-small-fallback:", "  rust-small-cx53:"))
        ]
        broken = "\n".join(lines)
        with self.assertRaisesRegex(AssertionError, "lane jobs missing"):
            validate_rust_small_route_contract(broken)

    def test_result_job_without_contract_reference_fails_closed(self) -> None:
        broken = self.workflow_text.replace(CONTRACT_TEST_FILE, "scripts/ci/removed.py")
        with self.assertRaisesRegex(AssertionError, "must run"):
            validate_rust_small_route_contract(broken)


if __name__ == "__main__":
    unittest.main()
