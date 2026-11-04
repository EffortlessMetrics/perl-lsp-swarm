#!/usr/bin/env python3
"""Route-shape contract for the canonical Rust Small proof lane (#8407).

Every required Rust Small route (`rust-small-cx53`, `rust-small-cx43`,
`rust-small-github`, `rust-small-fallback`) must invoke exactly one shared,
candidate/toolchain/profile-honest command:

    cargo run -p xtask --profile agent --locked -- rust-small-proof

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

import shlex
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"

RUST_SMALL_LANE_JOBS = (
    "rust-small-cx53",
    "rust-small-cx43",
    "rust-small-github",
    "rust-small-fallback",
)
RUST_SMALL_RESULT_JOB = "rust-small-result"
CANONICAL_INVOCATION = "cargo run -p xtask --profile agent --locked -- rust-small-proof"
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
    ("profile missing", CANONICAL_INVOCATION.replace(" --profile agent", "")),
    ("profile changed", CANONICAL_INVOCATION.replace("--profile agent", "--profile dev")),
    ("release profile", CANONICAL_INVOCATION.replace("--profile agent", "--profile release")),
    ("profile forwarded to xtask", "cargo run -p xtask --locked -- rust-small-proof --profile agent"),
    ("cached debug executable substituted", "target/debug/xtask rust-small-proof"),
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

# This is deliberately a bounded shell-shape contract, not a Bash interpreter.
# Parse the actual YAML run step, then admit only the existing straight-line
# hosted body or Docker launch/payload. Counting text anywhere in the job cannot
# distinguish a command from quoted data, a skipped branch or a shell function.
# New shell structure needs an explicit contract update and negative controls.
# Other run steps conservatively exclude literal proof-command references, even
# quoted data. This is not universal execution-count proof: dynamically built
# commands, action internals and environment-supplied code are outside the check.
DOCKER_RESOURCES = {
    "rust-small-cx53": ("14", "28g"),
    "rust-small-cx43": ("8", "16g"),
}
DOCKER_ARGUMENT_LINES = r'''
-e CARGO_HOME=/cargo-home \
-e SCCACHE_DIR=/sccache \
-e SCCACHE_CACHE_SIZE="$SCCACHE_CACHE_SIZE" \
-e RUSTC_WRAPPER=/usr/local/cargo/bin/sccache \
-e CARGO_INCREMENTAL="$CARGO_INCREMENTAL" \
-e CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" \
-e CARGO_TARGET_DIR=/target \
-e RUST_TEST_THREADS="$RUST_TEST_THREADS" \
-e RUST_BACKTRACE=1 \
-e CARGO_TERM_COLOR=always \
-e TMPDIR=/tmp/jobtmp \
-e GITHUB_JOB="$GITHUB_JOB" \
-v "$GITHUB_WORKSPACE:/workspace" \
-v "$CARGO_HOME:/cargo-home" \
-v "$SCCACHE_DIR:/sccache" \
-v "$CARGO_TARGET_DIR:/target" \
-v "$TMPDIR:/tmp/jobtmp" \
-w /workspace \
em-ci-rust:1.95 \
bash -eu -o pipefail -c '
'''.strip().splitlines()


def validate_proof_step(job_id: str, job: dict) -> None:
    name = "Run Rust small lane"
    if job_id == "rust-small-fallback":
        name += " (fallback)"
    steps = job.get("steps", [])
    if not isinstance(steps, list):
        raise AssertionError(f"{job_id} (field: proof-step) steps must be a YAML sequence")
    matches = [step for step in steps if isinstance(step, dict) and step.get("name") == name]
    if len(matches) != 1:
        raise AssertionError(f"{job_id} (field: proof-step) must contain exactly one {name!r}")
    step = matches[0]
    for other in steps:
        if other is step or not isinstance(other, dict):
            continue
        script = other.get("run")
        if isinstance(script, str) and any(
            "rust-small-proof" in line
            and (
                not line.lstrip(" \t").startswith("#")
                or "$(" in line
                or "`" in line
            )
            for line in script.split("\n")
        ):
            raise AssertionError(
                f"{job_id} (field: other-run-step) literal rust-small-proof references "
                "are supported only in the canonical proof step or standalone comments"
            )
    if set(step) != {"name", "shell", "run"} or step.get("shell") != "bash":
        raise AssertionError(
            f"{job_id} (field: proof-step) must be an unconditional bash run step "
            "without an error override or additional execution settings"
        )
    if not isinstance(step["run"], str):
        raise AssertionError(f"{job_id} (field: proof-step) run must be a shell script")

    expected = ["set -euo pipefail"]
    if job_id in DOCKER_RESOURCES:
        cpus, memory = DOCKER_RESOURCES[job_id]
        docker_launcher = [
            "docker run --rm \\",
            f'--cpus="{cpus}" \\',
            f'--memory="{memory}" \\',
            *DOCKER_ARGUMENT_LINES,
        ]
        expected += [
            *docker_launcher,
            "rustc --version",
            "cargo --version",
            "sccache --version || true",
            "git config --global --add safe.directory /workspace",
        ]
    expected += ["cargo fmt --all -- --check", CANONICAL_INVOCATION]
    if job_id in DOCKER_RESOURCES:
        expected += ["sccache --show-stats || true", "'"]
    # Use Bash's physical newline and indentation characters, not splitlines()
    # or strip()'s additional Unicode/control-character separators.
    physical_lines = step["run"].split("\n")
    lines = [line.strip(" \t") for line in physical_lines]
    actual = [line for line in lines if line and not line.startswith("#")]
    if actual != expected:
        raise AssertionError(
            f"{job_id} (field: proof-shell-body) must use the supported executable "
            "proof body; quoting, control flow, indirection or extra commands "
            "cannot stand in for one canonical invocation"
        )
    if job_id in DOCKER_RESOURCES:
        launch_start = lines.index("docker run --rm \\")
        opening = lines.index("bash -eu -o pipefail -c '")
        # A blank/comment line ends the continued Docker command, and spaces
        # after a backslash escape whitespace instead of the physical newline.
        # Preserve both adjacency and literal line endings in this region.
        launcher_lines = [line.lstrip(" \t") for line in physical_lines[launch_start:opening + 1]]
        if launcher_lines != docker_launcher:
            raise AssertionError(
                f"{job_id} (field: proof-shell-body) must retain contiguous Docker "
                "continuations with no whitespace after a continuation backslash"
            )
        closing = lines.index("'", opening + 1)
        # A # inside this outer single-quoted argument is data until the inner
        # Bash runs. An apostrophe in a purported comment would close the outer
        # quote first, so it cannot be discarded as an inert shell comment.
        if any("'" in line for line in lines[opening + 1:closing]):
            raise AssertionError(
                f"{job_id} (field: proof-shell-body) must retain the literal "
                "single-quoted Docker proof payload"
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

    try:
        workflow = yaml.safe_load(workflow_text)
    except yaml.YAMLError as error:
        raise AssertionError(f"Rust Small workflow must be valid YAML: {error}") from error
    parsed_jobs = workflow.get("jobs", {}) if isinstance(workflow, dict) else {}
    if not isinstance(parsed_jobs, dict):
        raise AssertionError("Rust Small jobs must be a YAML mapping")
    for job_id in RUST_SMALL_LANE_JOBS:
        job = parsed_jobs.get(job_id)
        if not isinstance(job, dict):
            raise AssertionError(f"{job_id} (field: proof-step) must be a YAML job mapping")
        validate_proof_step(job_id, job)

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

    def test_bootstrap_profile_and_executable_selection_are_pinned(self) -> None:
        # A shared-constant edit plus matching workflow edits must not silently
        # restore a different profile or forward Cargo's flag to xtask.
        args = shlex.split(CANONICAL_INVOCATION)
        separator = args.index("--")
        self.assertEqual(
            args[:separator],
            ["cargo", "run", "-p", "xtask", "--profile", "agent", "--locked"],
        )
        self.assertEqual(args[separator + 1 :], ["rust-small-proof"])

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

    def test_shell_context_cannot_replace_or_duplicate_proof(self) -> None:
        # These inherited false acceptances preserve a canonical-looking line,
        # but Bash either never invokes Cargo or invokes it more than once.
        command = CANONICAL_INVOCATION
        mutations = {
            "multiline echo": f'echo "\n{command}\n"',
            "heredoc data": f"cat << DECOY\n{command}\nDECOY",
            "heredoc comment": f": << DECOY\n{command}\nDECOY",
            "command duplicate": f"{command}\ncommand {command}",
            "env duplicate": f"{command}\nenv {command}",
            "compound duplicate": f"{command}\necho proof; {command}",
            "function shadow": f'cargo() {{ : target/debug/xtask "$@"; }}\n{command}',
            "skipped conditional": f"if false; then\n{command}\nfi",
            "quoted executable": f'"{command}"',
            "swallowed failure": f"{command} || true",
        }
        for job_id in RUST_SMALL_LANE_JOBS:
            line = next(
                line for line in job_bodies(self.workflow_text)[job_id].splitlines()
                if line.strip() == command
            )
            indent = line[: len(line) - len(line.lstrip())]
            for label, replacement in mutations.items():
                with self.subTest(site=job_id, mutation=label):
                    broken = mutate_first_in_job(
                        self.workflow_text, job_id, command,
                        replacement.replace("\n", "\n" + indent),
                    )
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    self.assertIn(job_id, str(context.exception))

    def test_proof_must_be_an_unconditional_failing_bash_run_step(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            for field, replacement in (
                ("if", "        if: false\n        shell: bash"),
                ("continue-on-error", "        continue-on-error: true\n        shell: bash"),
                ("shell", "        shell: python"),
                ("run", "        run: ':'\n        env:\n          PROOF_TEXT: |"),
            ):
                with self.subTest(site=job_id, field=field):
                    body = job_bodies(self.workflow_text)[job_id]
                    start = body.index("      - name: Run Rust small lane")
                    old = "        run: |" if field == "run" else "        shell: bash"
                    position = body.index(old, start)
                    tail = body[position + len(old):]
                    if field == "run":
                        # Move the entire former script into valid YAML env data,
                        # leaving an executable no-op in its place. Do not rely
                        # on a YAML syntax error to reject the decoy.
                        lines = tail.splitlines(keepends=True)
                        boundary = next(
                            (i for i, line in enumerate(lines)
                             if line.strip() and len(line) - len(line.lstrip()) <= 8),
                            len(lines),
                        )
                        tail = "".join("  " + line for line in lines[:boundary]) + "".join(lines[boundary:])
                    changed = body[:position] + replacement + tail
                    broken = self.workflow_text.replace(body, changed, 1)
                    yaml.safe_load(broken)
                    with self.assertRaises(AssertionError) as context:
                        validate_rust_small_route_contract(broken)
                    self.assertIn(job_id, str(context.exception))

    def test_other_run_steps_cannot_reference_the_proof_command(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            body = job_bodies(self.workflow_text)[job_id]
            position = body.index("      - name: Run Rust small lane")
            for command in (
                "command " + CANONICAL_INVOCATION,
                "env " + CANONICAL_INVOCATION,
                "echo setup; " + CANONICAL_INVOCATION,
                'echo "' + CANONICAL_INVOCATION + '"',
            ):
                with self.subTest(site=job_id, command=command):
                    extra = (
                        "      - name: Extra proof\n"
                        "        shell: bash\n"
                        "        run: |\n"
                        f"          {command}\n\n"
                    )
                    changed = body[:position] + extra + body[position:]
                    with self.assertRaisesRegex(AssertionError, job_id):
                        validate_rust_small_route_contract(self.workflow_text.replace(body, changed, 1))
            with self.subTest(site=job_id, command="quoted-substitution"):
                extra = (
                    "      - name: Extra proof\n"
                    "        shell: bash\n"
                    "        run: |\n"
                    '          echo "\n'
                    "          # $(" + CANONICAL_INVOCATION + ")\n"
                    '          "\n\n'
                )
                changed = body[:position] + extra + body[position:]
                with self.assertRaisesRegex(AssertionError, job_id):
                    validate_rust_small_route_contract(self.workflow_text.replace(body, changed, 1))

    def test_unrelated_step_metadata_and_standalone_comments_are_allowed(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            with self.subTest(site=job_id):
                body = job_bodies(self.workflow_text)[job_id]
                position = body.index("      - name: Run Rust small lane")
                extra = (
                    "      - name: Setup for rust-small-proof\n"
                    "        if: false\n"
                    "        continue-on-error: true\n"
                    "        shell: python\n"
                    "        env:\n"
                    "          TASK: rust-small-proof\n"
                    "        run: |\n"
                    "          # The rust-small-proof command belongs to its own step.\n"
                    "          print('unrelated setup')\n\n"
                )
                changed = body[:position] + extra + body[position:]
                validate_rust_small_route_contract(self.workflow_text.replace(body, changed, 1))

    def test_proof_step_allows_standalone_comments_and_blank_lines(self) -> None:
        for job_id in RUST_SMALL_LANE_JOBS:
            with self.subTest(site=job_id):
                line = next(
                    line for line in job_bodies(self.workflow_text)[job_id].splitlines()
                    if line.strip() == CANONICAL_INVOCATION
                )
                indent = line[: len(line) - len(line.lstrip())]
                changed = mutate_first_in_job(
                    self.workflow_text, job_id, CANONICAL_INVOCATION,
                    '# Explanation with "quotes" and $literal shell text\n\n'
                    + indent + CANONICAL_INVOCATION,
                )
                validate_rust_small_route_contract(changed)

    def test_docker_payload_comment_cannot_escape_the_outer_quote(self) -> None:
        for job_id in ("rust-small-cx53", "rust-small-cx43"):
            with self.subTest(site=job_id):
                broken = mutate_first_in_job(
                    self.workflow_text, job_id, CANONICAL_INVOCATION,
                    "# ' ; false; echo '\n              " + CANONICAL_INVOCATION,
                )
                with self.assertRaisesRegex(AssertionError, job_id):
                    validate_rust_small_route_contract(broken)

    def test_docker_continuations_cannot_have_gaps_or_trailing_whitespace(self) -> None:
        for job_id in DOCKER_RESOURCES:
            body = job_bodies(self.workflow_text)[job_id]
            proof_start = body.index("      - name: Run Rust small lane")
            launch_start = body.index("          docker run --rm", proof_start)
            payload_start = body.index("            bash -eu -o pipefail -c '", launch_start)
            continued_lines = body[launch_start:payload_start].split("\n")[:-1]
            self.assertTrue(continued_lines)
            for line in continued_lines:
                self.assertTrue(line.endswith("\\"), line)
                indent = line[: len(line) - len(line.lstrip(" \t"))]
                for label, suffix in (
                    ("blank line", "\n"),
                    ("comment line", "\n" + indent + "# continuation is interrupted"),
                    ("trailing space", " "),
                    ("trailing tab", "\t"),
                ):
                    with self.subTest(site=job_id, line=line.strip(), mutation=label):
                        position = body.index(line, launch_start)
                        changed = body[:position] + line + suffix + body[position + len(line):]
                        broken = self.workflow_text.replace(body, changed, 1)
                        with self.assertRaisesRegex(AssertionError, job_id):
                            validate_rust_small_route_contract(broken)

    def test_docker_comments_and_blank_lines_at_command_boundaries_are_allowed(self) -> None:
        for job_id in DOCKER_RESOURCES:
            body = job_bodies(self.workflow_text)[job_id]
            proof_start = body.index("      - name: Run Rust small lane")
            launch_start = body.index("          docker run --rm", proof_start)
            payload_start = body.index("            bash -eu -o pipefail -c '", launch_start)
            payload_end = body.index("\n            '\n", payload_start) + len("\n            '")
            for label, position in (
                ("before Docker", launch_start),
                ("inside payload", body.index("\n", payload_start) + 1),
                ("after Docker", payload_end),
            ):
                with self.subTest(site=job_id, boundary=label):
                    changed = body[:position] + "\n          # safe command boundary\n\n" + body[position:]
                    validate_rust_small_route_contract(self.workflow_text.replace(body, changed, 1))

    def test_result_job_without_contract_reference_fails_closed(self) -> None:
        broken = self.workflow_text.replace(CONTRACT_TEST_FILE, "scripts/ci/removed.py")
        with self.assertRaisesRegex(AssertionError, "must run"):
            validate_rust_small_route_contract(broken)


if __name__ == "__main__":
    unittest.main()
