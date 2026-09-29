#!/usr/bin/env python3
"""Workflow contract for the shared RIPR cache seed (issue #15055)."""

from __future__ import annotations

import ast
import os
import re
import unittest
from pathlib import Path

REPO_ROOT = Path(os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2]))
WORKFLOW = REPO_ROOT / ".github/workflows/ripr.yml"
SELF_TEST_WORKFLOW = REPO_ROOT / ".github/workflows/ci-gate-self-tests.yml"
ACTION = "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6"
# #16126: ripr's own fact cache rides actions/cache, pinned to the SHA
# .ci/policies/action-pin-provenance.toml already approves for this family.
FACT_CACHE_PIN = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
FACT_CACHE_RESTORE = f"actions/cache/restore@{FACT_CACHE_PIN}"
FACT_CACHE_SAVE = f"actions/cache/save@{FACT_CACHE_PIN}"
SEED_SCAN = "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json"
# #16126: the fact-cache path is spelled twice -- once for the shell that
# exports it and once for the cache action's `path:` -- because `runner` is a
# step-level context. These must name the same directory, or a lane restores
# into somewhere ripr never reads and stays cold while every other rule here
# still passes.
FACT_CACHE_DIRNAME = "ripr-facts"
FACT_CACHE_DECLARATION = (
    'echo "RIPR_CACHE_DIR=$RUNNER_TEMP/' + FACT_CACHE_DIRNAME + '" >> "$GITHUB_ENV"'
)
FACT_CACHE_PATH = "path: ${{ runner.temp }}/" + FACT_CACHE_DIRNAME
# #16431: exact-head memoization of the repo-wide RIPR+ receipt. Same pinned
# actions/cache family, but a separate cache with its own contract: the key
# binds the toolchain version, the exact head sha, and the suppression-ledger
# hash; only an exact-key hit may be consumed (a `restore-keys` prefix hit
# restores a different head's receipt and must be refused); and only a
# freshly produced receipt may be saved.
RECEIPT_CACHE_RESTORE = f"actions/cache/restore@{FACT_CACHE_PIN}"
RECEIPT_CACHE_SAVE = f"actions/cache/save@{FACT_CACHE_PIN}"
RECEIPT_CACHE_RESTORE_ID = "ripr-plus-receipt-cache"
RECEIPT_CACHE_KEY = (
    "ripr-plus-receipt-v1-${{ env.RIPR_VERSION }}-${{ github.sha }}-"
    "${{ hashFiles('policy/ripr-suppressions.toml') }}"
)
RECEIPT_CACHE_RESTORE_PREFIX = "ripr-plus-receipt-v1-${{ env.RIPR_VERSION }}-"
RECEIPT_CACHE_PATH = "path: target/receipts/quality/ripr-plus.json"
RECEIPT_CACHE_MARKER_GUARD = (
    "success() && hashFiles('target/receipts/quality/.ripr-plus-fresh') != ''"
)
RECEIPT_CACHE_ADJUDICATOR = "Adjudicate receipt cache exactness"
RECEIPT_CACHE_EXACT_GUARD = ' && [ "${RIPR_RECEIPT_CACHE_HIT:-}" = "true" ]'
RECEIPT_SCAN = "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json"
RECEIPT_MARKER_NAME = ".ripr-plus-fresh"
RECEIPT_CACHE_JOBS = ("ripr-github", "ripr-fallback")
SELFHOSTED_CACHE_DIR = "RIPR_CACHE_DIR: /mnt/ci-cache/" + FACT_CACHE_DIRNAME
# #16209: the job-level RIPR_CACHE_DIR above is a host-side declaration only.
# docker does not inherit arbitrary host environment variables, so the
# analysis container needs its own explicit forward plus a bind mount to the
# same host directory -- without both, the container falls back to its
# workspace-local default and loses the cache during per-run cleanup.
SELFHOSTED_CONTAINER_CACHE_DIR = "/ripr-facts"
SELFHOSTED_CACHE_FORWARD_LINE = (
    "            -e RIPR_CACHE_DIR=" + SELFHOSTED_CONTAINER_CACHE_DIR + " \\\n"
)
SELFHOSTED_CACHE_MOUNT_LINE = (
    '            -v "$RIPR_CACHE_DIR:' + SELFHOSTED_CONTAINER_CACHE_DIR + '" \\\n'
)
HOSTED_FACT_CACHE_JOBS = ("ripr-github", "ripr-fallback", "seed-cache")
CANONICAL_REFS = "github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main'"
SAVE_GUARD = (
    "(github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && "
    "inputs.seed_cache == true)) && (" + CANONICAL_REFS + ")"
)
EXPECTED_ROUTER = (
    "(github.event.pull_request.draft != true || github.event_name != 'pull_request') && "
    "github.event_name != 'schedule' && (github.event_name != 'workflow_dispatch' || "
    "inputs.seed_cache != true)"
)
ATOM = re.compile(
    r"(?:github\.(?:event_name|ref)\s*(?:==|!=)\s*'[^']+'|"
    r"inputs\.seed_cache\s*(?:==|!=)\s*(?:true|false))"
)


def evaluate_guard(expression: str, *, event: str, ref: str, seed: bool) -> bool:
    """Evaluate the small, explicitly supported GitHub guard grammar."""
    atoms: dict[str, bool] = {}

    def replace(match: re.Match[str]) -> str:
        atom = match.group(0)
        if atom in atoms:
            return str(atoms[atom])
        lhs, op, quoted, bare = re.fullmatch(
            r"(.+?)\s*(==|!=)\s*(?:'([^']+)'|(true|false))", atom
        ).groups()
        actual = {"github.event_name": event, "github.ref": ref,
                  "inputs.seed_cache": seed}[lhs]
        expected = quoted if quoted is not None else bare
        if expected in {"true", "false"}:
            expected_value = expected == "true"
        else:
            expected_value = expected
        atoms[atom] = (actual == expected_value) if op == "==" else (actual != expected_value)
        return str(atoms[atom])

    normalized = ATOM.sub(replace, expression).replace("||", " or ").replace("&&", " and ")
    if re.search(r"[^()\sA-Za-z']|\b(?:github|inputs)\b", normalized):
        raise ValueError(f"unsupported guard syntax: {expression}")
    if not re.fullmatch(r"(?:True|False|and|or|\(|\)|\s)+", normalized):
        raise ValueError(f"unsupported guard syntax: {expression}")
    tree = ast.parse(normalized, mode="eval")

    def visit(node: ast.AST) -> bool:
        if isinstance(node, ast.Expression):
            return visit(node.body)
        if isinstance(node, ast.Constant) and isinstance(node.value, bool):
            return node.value
        if isinstance(node, ast.BoolOp) and isinstance(node.op, (ast.And, ast.Or)):
            values = [visit(value) for value in node.values]
            return all(values) if isinstance(node.op, ast.And) else any(values)
        raise ValueError(f"unsupported guard syntax: {expression}")

    return visit(tree)


def lines() -> list[str]:
    if not WORKFLOW.is_file():
        raise FileNotFoundError(WORKFLOW)
    return WORKFLOW.read_text(encoding="utf-8").splitlines()


def self_test_lines() -> list[str]:
    if not SELF_TEST_WORKFLOW.is_file():
        raise FileNotFoundError(SELF_TEST_WORKFLOW)
    return SELF_TEST_WORKFLOW.read_text(encoding="utf-8").splitlines()


def block(source: list[str], marker: str, indent: int) -> list[str]:
    prefix = " " * indent + marker + ":"
    starts = [i for i, line in enumerate(source) if line == prefix]
    if len(starts) != 1:
        raise AssertionError(f"expected one {marker!r}, found {len(starts)}")
    start = starts[0]
    end = next(
        (i for i in range(start + 1, len(source))
         if source[i] and not source[i].startswith(" " * (indent + 1))),
        len(source),
    )
    return source[start:end]


def field(source: list[str], name: str, indent: int) -> str | None:
    prefix = " " * indent + name + ":"
    matches = [i for i, line in enumerate(source) if line.startswith(prefix)]
    if len(matches) > 1:
        raise AssertionError(f"duplicate {name!r} field")
    if not matches:
        return None
    start = matches[0]
    value = source[start][len(prefix):].strip()
    if value not in {">", ">-", "|", "|-"}:
        return value
    continuation = []
    for line in source[start + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        if line.strip() and not line.lstrip().startswith("#"):
            continuation.append(line.strip())
    return " ".join(continuation)


def workflow_steps(source: list[str]) -> list[list[str]]:
    starts = [i for i, line in enumerate(source) if line.startswith("      - ")]
    return [source[start:starts[pos + 1] if pos + 1 < len(starts) else len(source)]
            for pos, start in enumerate(starts)]


def action_reference(step: list[str]) -> str:
    for line in step:
        declaration = line.strip().removeprefix("- ")
        if declaration.startswith("uses:"):
            return declaration.partition(":")[2].split(" #", 1)[0].strip().strip("\"'")
    return ""


def cache_steps(source: list[str]) -> list[list[str]]:
    return [step for step in workflow_steps(source)
            if action_reference(step).lower().startswith("swatinem/rust-cache@")]


def fact_cache_steps(source: list[str]) -> list[list[str]]:
    """`actions/cache`-family steps keyed `ripr-facts-...` (#16126).

    Kept separate from `cache_steps` because that helper is the rust-cache
    cardinality contract; a fact-cache step must not change those counts, and
    a rust-cache step must not satisfy the writer rules below. Scoped to the
    `ripr-facts-` key family so the #16431 receipt cache -- a separate
    actions/cache consumer with its own contract -- cannot be confused with
    it; `validate_cache_family_accounting` still fails closed on any
    actions/cache step belonging to neither family.
    """
    return [step for step in workflow_steps(source)
            if action_reference(step).lower().startswith("actions/cache")
            and "key: ripr-facts-" in "\n".join(step)]


def receipt_cache_steps(source: list[str]) -> list[list[str]]:
    """`actions/cache` steps keyed `ripr-plus-receipt-v1-...` (#16431)."""
    return [step for step in workflow_steps(source)
            if action_reference(step).lower().startswith("actions/cache")
            and "key: ripr-plus-receipt-v1-" in "\n".join(step)]


def validate_static_contract(source: list[str]) -> None:
    """Fail closed on the workflow mutations this contract is meant to catch."""
    text = "\n".join(source)
    router = field(block(source, "route-ripr", 2), "if", 4)
    if router != EXPECTED_ROUTER:
        raise AssertionError("router dispatch boundary drifted")
    if len(cache_steps(source)) != 3 or "hashFiles('Cargo.lock')" in text:
        raise AssertionError("cache identity or seed cardinality drifted")
    if "ripr-fallback-" in text or text.count("seed-cache:") != 1:
        raise AssertionError("legacy key or duplicate seed survived")
    for step in cache_steps(source):
        if action_reference(step) != ACTION:
            raise AssertionError("cache action pin is not approved")
        if "shared-key: ripr-${{ env.RIPR_VERSION }}" not in "\n".join(step):
            raise AssertionError("cache consumers do not share the stable key")
        if "cache-workspace-crates: true" not in "\n".join(step):
            raise AssertionError("workspace crate cache retention is missing")
    jobs = {name: block(source, name, 2) for name in ("ripr-github", "ripr-fallback")}
    if any("save-if: ${{ false }}" not in "\n".join(cache_steps(job)[0]) for job in jobs.values()):
        raise AssertionError("analysis cache writer became reachable")
    seed = block(source, "seed-cache", 2)
    if field(seed, "if", 4) != SAVE_GUARD:
        raise AssertionError("seed condition is unsafe")
    seed_cache = cache_steps(seed)
    if len(seed_cache) != 1 or f"save-if: ${{{{ {SAVE_GUARD} }}}}" not in "\n".join(seed_cache[0]):
        raise AssertionError("seed writer guard is unsafe")
    # Substring, not equality: the scan shares a `run:` block with the status
    # capture that keeps a gap finding from costing the whole warm cache, so
    # its step's command text is longer than the command. `field` already
    # drops comment lines, so a command named only in prose still fails here.
    commands = [field(step, "run", 8) or "" for step in workflow_steps(seed)]
    for command in ('cargo install ripr --version "$RIPR_VERSION" --locked',
                    'cargo build -p xtask --locked',
                    # #16126: the seed must actually produce the fact cache it
                    # saves. Without this the save step stores an empty
                    # directory and every analysis lane still pays the cold
                    # repo-wide scan, silently.
                    SEED_SCAN):
        if not any(command in text for text in commands):
            raise AssertionError(f"seed useful work is missing: {command}")
    validate_fact_cache_contract(source)


def validate_fact_cache_contract(source: list[str]) -> None:
    """#16126: the fact cache keeps the same one-writer boundary as the rust cache.

    A restored fact cache cannot change a verdict — ripr keys every entry on
    content hashes plus analyzer and schema version — but a candidate-reachable
    *writer* would still burn the shared quota that docs/ci/cache-policy.md
    reserves to the canonical seed. That is what these rules hold.
    """
    for job in ("ripr-github", "ripr-fallback"):
        steps = fact_cache_steps(block(source, job, 2))
        if len(steps) != 1:
            raise AssertionError(f"{job} must carry exactly one fact-cache step")
        reference = action_reference(steps[0])
        if reference != FACT_CACHE_RESTORE:
            raise AssertionError(f"{job} fact cache is not the approved restore action")
    seed_steps = fact_cache_steps(block(source, "seed-cache", 2))
    if len(seed_steps) != 1 or action_reference(seed_steps[0]) != FACT_CACHE_SAVE:
        raise AssertionError("seed must be the one fact-cache writer")
    # docs/ci/cache-policy.md § Direct cache-save actions: an explicit writer
    # states its own event/ref authority instead of inheriting the job's.
    if field(seed_steps[0], "if", 8) != SAVE_GUARD:
        raise AssertionError("fact-cache writer does not carry its own save guard")
    if len(fact_cache_steps(source)) != 3:
        raise AssertionError("an unaccounted fact-cache step appeared")
    for step in fact_cache_steps(source):
        text = "\n".join(step)
        if "key: ripr-facts-${{ env.RIPR_VERSION }}-${{ github.sha }}" not in text:
            raise AssertionError("fact cache key is not bound to the analyzer version")
        # Whole line, not substring: `ripr-facts-cache` contains `ripr-facts`,
        # so a containment test accepts a path ripr never reads.
        if not any(line.strip() == FACT_CACHE_PATH for line in step):
            raise AssertionError("fact cache path is not the runner temp directory")
    validate_fact_cache_path_agreement(source)
    validate_receipt_cache_contract(source)
    validate_cache_family_accounting(source)


def validate_receipt_cache_contract(source: list[str]) -> None:
    """#16431: exact-head memoization of the repo-wide RIPR+ receipt.

    The receipt is a function of the checked-out HEAD tree -- it embeds
    `head: <current_head>` and every consumer head-binds it (`ripr-plus
    --check` byte-compares; the quality gate marks a receipt `stale` unless
    its head matches) -- so a cache entry keyed on the exact head can only
    change how long the answer takes, never which answer it is. Three rules
    hold that boundary:

    1. the key binds the toolchain version, the exact head sha, and the
       suppression-ledger hash, so only byte-identical inputs can collide;
    2. only an exact-key hit is consumed: a `restore-keys` prefix hit
       restores a DIFFERENT head's receipt, which the adjudicator refuses
       and the produce branch overwrites;
    3. only a freshly produced receipt is saved: the marker file exists only
       on the produce branch, so a restored receipt is never re-published.
    """
    for job in RECEIPT_CACHE_JOBS:
        steps = workflow_steps(block(source, job, 2))
        names = [(i, "\n".join(step)) for i, step in enumerate(steps)]
        # Scope by key, not by action alone: the fact-cache restore uses the
        # same pinned actions/cache/restore ref in the same job.
        restores = [i for i, text in names
                    if action_reference(text.splitlines()) == RECEIPT_CACHE_RESTORE
                    and "key: ripr-plus-receipt-v1-" in text]
        saves = [i for i, text in names
                 if action_reference(text.splitlines()) == RECEIPT_CACHE_SAVE
                 and "key: ripr-plus-receipt-v1-" in text]
        if len(restores) != 1 or len(saves) != 1:
            raise AssertionError(
                f"{job} must carry exactly one receipt-cache restore and one save"
            )
        restore, save = restores[0], saves[0]
        restore_text = names[restore][1]
        save_text = names[save][1]
        for text in (restore_text, save_text):
            if f"key: {RECEIPT_CACHE_KEY}" not in text:
                raise AssertionError(f"{job} receipt cache key is not bound to the exact head")
            if not any(line.strip() == RECEIPT_CACHE_PATH for line in text.splitlines()):
                raise AssertionError(f"{job} receipt cache path is not the receipt file")
        # Exact-consumption machinery: the restore carries the prefix for
        # shape parity with the fact cache, the adjudicator bridges the
        # action's exact-match-only output into the environment, and the
        # receipt step guards on it.
        if f"id: {RECEIPT_CACHE_RESTORE_ID}" not in restore_text:
            raise AssertionError(f"{job} receipt-cache restore lost its step id")
        if not any(line.strip() == f"restore-keys: {RECEIPT_CACHE_RESTORE_PREFIX}" or
                   line.strip() == "restore-keys: |" for line in restore_text.splitlines()):
            raise AssertionError(f"{job} receipt-cache restore-keys drifted")
        if RECEIPT_CACHE_RESTORE_PREFIX not in restore_text:
            raise AssertionError(f"{job} receipt-cache restore prefix drifted")
        adjudicators = [i for i, text in names if f"name: {RECEIPT_CACHE_ADJUDICATOR}" in text]
        if len(adjudicators) != 1:
            raise AssertionError(f"{job} must adjudicate receipt-cache exactness exactly once")
        adjudicator = names[adjudicators[0]][1]
        if f"steps.{RECEIPT_CACHE_RESTORE_ID}.outputs.cache-hit" not in adjudicator:
            raise AssertionError(f"{job} adjudicator ignores the exact-match output")
        # Whole lines, not a substring: the guard reads the literal value
        # `true`, so an export that emits any other spelling silently
        # disables consumption and would never be caught by the guard check.
        if not any(line.strip() == 'echo "RIPR_RECEIPT_CACHE_HIT=true" >> "$GITHUB_ENV"'
                   for line in adjudicator.splitlines()):
            raise AssertionError(f"{job} adjudicator does not export the exact-hit decision")
        if not any(line.strip() == 'echo "RIPR_RECEIPT_CACHE_HIT=false" >> "$GITHUB_ENV"'
                   for line in adjudicator.splitlines()):
            raise AssertionError(f"{job} adjudicator does not export the no-hit decision")
        # Whole line, not substring: the validate step's `--check` invocation
        # contains the produce command as a prefix, so a containment test
        # would count it as a second producer.
        receipt_steps = [i for i, text in names
                         if any(line.strip() == RECEIPT_SCAN for line in text.splitlines())]
        if len(receipt_steps) != 1:
            raise AssertionError(f"{job} must run the repo-wide receipt scan exactly once")
        receipt = names[receipt_steps[0]][1]
        if RECEIPT_CACHE_EXACT_GUARD not in receipt:
            raise AssertionError(
                f"{job} receipt step consumes receipts without the exact-head guard"
            )
        if RECEIPT_MARKER_NAME not in receipt:
            raise AssertionError(f"{job} receipt step does not record produce-branch freshness")
        if not (restore < adjudicators[0] < receipt_steps[0] < save):
            raise AssertionError(
                f"{job} receipt-cache restore/adjudicate/produce/save order drifted"
            )
        # Save side: only a fully succeeding run may publish, and only when
        # this run produced the receipt itself (freshness marker present).
        if field(save_text.splitlines(), "if", 8) != RECEIPT_CACHE_MARKER_GUARD:
            raise AssertionError(f"{job} receipt-cache save guard is not the exact fresh rule")
        if "restore-keys" in save_text:
            raise AssertionError(f"{job} receipt-cache save must not carry restore-keys")
        if save != len(steps) - 1:
            raise AssertionError(f"{job} receipt-cache save is not the job's last step")


def validate_cache_family_accounting(source: list[str]) -> None:
    """Every `actions/cache` step must belong to a contracted family.

    The fact-cache contract historically failed closed on ANY unaccounted
    actions/cache step workflow-wide. The #16431 receipt cache is a second
    contracted family, so accounting is now by family membership: an
    actions/cache step in neither family still fails here. Both sides are
    derived from the same whole-source step listing, because a job's last
    step absorbs the following job's header lines in that listing and would
    never compare equal to a job-block-scoped derivation.
    """
    cache = [step for step in workflow_steps(source)
             if action_reference(step).lower().startswith("actions/cache")]
    known = fact_cache_steps(source) + receipt_cache_steps(source)
    unknown = [step for step in cache if step not in known]
    if unknown:
        raise AssertionError(
            "an unaccounted actions/cache step appeared: "
            + "; ".join(action_reference(step) or step[0] for step in unknown)
        )
    if len(cache) != 3 + 2 * len(RECEIPT_CACHE_JOBS):
        raise AssertionError("cache family cardinality drifted")


def validate_fact_cache_path_agreement(source: list[str]) -> None:
    """The exported path and the cached path must name the same directory.

    They cannot share a spelling: `runner` is a step-level context, so the
    shell export reads `$RUNNER_TEMP` while the cache action's `path:` reads
    `${{ runner.temp }}`.
    """
    for job in HOSTED_FACT_CACHE_JOBS:
        steps = workflow_steps(block(source, job, 2))
        declared = [i for i, step in enumerate(steps)
                    if FACT_CACHE_DECLARATION in "\n".join(step)]
        if len(declared) != 1:
            raise AssertionError(f"{job} does not declare the fact cache path exactly once")
        consumers = [i for i, step in enumerate(steps)
                     if action_reference(step).lower().startswith("actions/cache")
                     or SEED_SCAN in (field(step, "run", 8) or "")]
        if not consumers:
            raise AssertionError(f"{job} declares a fact cache path that nothing reads")
        if declared[0] > min(consumers):
            raise AssertionError(f"{job} reads the fact cache path before declaring it")
    # The self-hosted lane keeps durable state on the mount beside CARGO_HOME,
    # so it needs no action and no export -- but it must still point ripr
    # outside the per-run checkout.
    if SELFHOSTED_CACHE_DIR not in "\n".join(block(source, "ripr-selfhosted", 2)):
        raise AssertionError("the self-hosted lane lost its durable fact cache path")
    validate_selfhosted_container_boundary(source)
    # The mistake this rule exists to catch, made while writing this change:
    # `runner` is a step-level context, and no other job-level `env:` in this
    # repository reaches for it.
    for job in ("ripr-selfhosted",) + HOSTED_FACT_CACHE_JOBS:
        for line in block(source, job, 2):
            stripped = line.lstrip()
            if (line.startswith(" " * 6) and not line.startswith(" " * 7)
                    and "runner." in line and not stripped.startswith(("#", "-"))):
                raise AssertionError(f"{job} uses the runner context in a job-level field")


def validate_selfhosted_container_boundary(source: list[str]) -> None:
    """#16209: the job-level env is not a container-visible one.

    `RIPR_CACHE_DIR` set in the job's `env:` block only sets a host-side shell
    variable. `docker run` does not inherit arbitrary host environment
    variables into the container, so the analysis container needs its own
    explicit `-e RIPR_CACHE_DIR=...` forward *and* a `-v` bind mount to the
    same host directory the job-level env names -- either one missing means
    every ripr command inside the container reads/writes its workspace-local
    default instead, which is deleted by the next run's workspace cleanup.
    """
    job_text = "\n".join(block(source, "ripr-selfhosted", 2))
    if SELFHOSTED_CACHE_FORWARD_LINE not in job_text:
        raise AssertionError(
            "self-hosted docker run does not forward RIPR_CACHE_DIR into the container"
        )
    if SELFHOSTED_CACHE_MOUNT_LINE not in job_text:
        raise AssertionError(
            "self-hosted docker run does not mount the host fact-cache directory"
        )


class RiprCacheAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = lines()

    def test_dispatch_declares_opt_in_cache_seed(self) -> None:
        trigger = block(self.source, "on", 0)
        self.assertIn("  workflow_dispatch:", trigger)
        self.assertIn("    inputs:", trigger)
        self.assertIn("      seed_cache:", trigger)
        dispatch = block(trigger, "workflow_dispatch", 2)
        seed = block(dispatch, "seed_cache", 6)
        self.assertIn("        type: boolean", seed)
        self.assertIn("        default: false", seed)

    def test_router_skips_only_opt_in_seed_dispatch(self) -> None:
        router = block(self.source, "route-ripr", 2)
        condition = field(router, "if", 4) or ""
        self.assertEqual(EXPECTED_ROUTER, condition)

    def test_self_test_trigger_reaches_this_contract(self) -> None:
        trigger = block(self_test_lines(), "on", 0)
        pull_request = block(trigger, "pull_request", 2)
        paths = block(pull_request, "paths", 4)
        self.assertIn("      - 'scripts/ci/test_ripr_cache_authority_15055.py'", paths)

    def test_seed_is_one_canonical_writer_and_analysis_is_restore_only(self) -> None:
        steps = cache_steps(self.source)
        self.assertEqual(3, len(steps), "primary, fallback, and one seed cache step")
        seed = block(self.source, "seed-cache", 2)
        seed_condition = field(seed, "if", 4) or ""
        self.assertIn("github.event_name == 'schedule'", seed_condition)
        self.assertIn("inputs.seed_cache == true", seed_condition)
        self.assertIn(CANONICAL_REFS, seed_condition)
        self.assertEqual(SAVE_GUARD, seed_condition)
        for step in steps:
            with_text = "\n".join(step)
            self.assertNotIn("hashFiles('Cargo.lock')", with_text)
            self.assertIn("shared-key: ripr-${{ env.RIPR_VERSION }}", with_text)
            self.assertIn("cache-workspace-crates: true", with_text)
        self.assertIn("save-if: ${{ false }}", "\n".join(cache_steps(block(self.source, "ripr-github", 2))[0]))
        self.assertIn("save-if: ${{ false }}", "\n".join(cache_steps(block(self.source, "ripr-fallback", 2))[0]))
        self.assertIn(f"save-if: ${{{{ {SAVE_GUARD} }}}}", "\n".join(steps[-1]))

    def test_selfhosted_container_receives_forwarded_cache_and_mount(self) -> None:
        job_text = "\n".join(block(self.source, "ripr-selfhosted", 2))
        self.assertIn(SELFHOSTED_CACHE_FORWARD_LINE, job_text)
        self.assertIn(SELFHOSTED_CACHE_MOUNT_LINE, job_text)
        # forwarded env and mount must name the same container path so the
        # process ripr runs actually reads from where the mount lands.
        self.assertIn(SELFHOSTED_CONTAINER_CACHE_DIR, SELFHOSTED_CACHE_FORWARD_LINE)
        self.assertIn(SELFHOSTED_CONTAINER_CACHE_DIR, SELFHOSTED_CACHE_MOUNT_LINE)

    def test_negative_controls_do_not_authorize_saves(self) -> None:
        seed_condition = field(block(self.source, "seed-cache", 2), "if", 4) or ""
        cases = {
            ("schedule", "refs/heads/main", False): True,
            ("workflow_dispatch", "refs/heads/main", True): True,
            ("workflow_dispatch", "refs/heads/main", False): False,
            ("workflow_dispatch", "refs/pull/1/merge", True): False,
            ("workflow_dispatch", "refs/heads/feature", True): False,
            ("push", "refs/heads/main", True): False,
            ("schedule", "refs/tags/v0.18.0", False): False,
        }
        for (event, ref, seed), expected in cases.items():
            self.assertEqual(expected, evaluate_guard(seed_condition, event=event, ref=ref, seed=seed))
        text = "\n".join(self.source)
        self.assertNotIn("hashFiles('Cargo.lock')", text)
        self.assertNotIn("ripr-fallback-", text, "old lock/key prefix must not survive")
        self.assertEqual(1, sum("seed-cache:" in line for line in self.source),
                         "duplicate seed jobs are not allowed")
        self.assertNotIn("pull_request_target", text)
        save_values = re.findall(r"^\s+save-if: (.+)$", text, re.MULTILINE)
        self.assertEqual(3, len(save_values))
        self.assertEqual(2, save_values.count("${{ false }}"))
        self.assertEqual(1, sum(SAVE_GUARD in value for value in save_values))
        self.assertNotIn("save-if: ${{ github.event_name == 'workflow_dispatch' }}", text)

    def test_guard_parser_rejects_unsafe_mutations(self) -> None:
        with self.assertRaises(ValueError):
            evaluate_guard("true || github.ref == 'refs/heads/main'", event="push", ref="refs/heads/main", seed=False)
        with self.assertRaises(ValueError):
            evaluate_guard("github.ref == env.DEFAULT_BRANCH", event="push", ref="refs/heads/main", seed=False)

    def test_workflow_mutation_controls_fail_closed(self) -> None:
        source = self.source
        validate_static_contract(source)
        text = "\n".join(source)
        mutations = {
            "disabled normal manual analysis": text.replace(
                "inputs.seed_cache != true)", "inputs.seed_cache != true) && false", 1
            ),
            "manual dispatch clause narrowed incorrectly": text.replace(
                "github.event_name != 'workflow_dispatch' || inputs.seed_cache != true",
                "github.event_name != 'workflow_dispatch' && inputs.seed_cache != true", 1
            ),
            "weakened analysis writer": text.replace("save-if: ${{ false }}", "save-if: ${{ true }}", 1),
            "removed seed ref guard": text.replace(SAVE_GUARD, "true", 1),
            "old lock prefix": text.replace("ripr-${{ env.RIPR_VERSION }}", "ripr-${{ env.RIPR_VERSION }}-${{ hashFiles('Cargo.lock') }}", 1),
            "workspace retention disabled": text.replace("cache-workspace-crates: true", "cache-workspace-crates: false", 1),
            "duplicate seed": text.replace("  seed-cache:", "  seed-cache:\n  seed-cache:", 1),
        }
        for name, mutated in mutations.items():
            with self.subTest(mutation=name):
                with self.assertRaises(AssertionError):
                    validate_static_contract(mutated.splitlines())

    def test_cache_discovery_and_seed_work_mutations(self) -> None:
        text = "\n".join(self.source)
        seed_text = "\n".join(block(self.source, "seed-cache", 2))
        other_pin = "Swatinem/rust-cache@" + "a" * 40
        mutations = {
            "unapproved replacement pin": text.replace(ACTION, other_pin, 1),
            "additional cache with implicit save": text.replace(
                "      - name: Install ripr\n",
                f"      - uses: {other_pin}\n\n      - name: Install ripr\n", 1
            ),
            "seed job ref guard removed": text.replace(
                seed_text,
                re.sub(r"(?m)^    if: >-\n(?:      .*\n)+", "    if: true\n", seed_text, count=1),
                1,
            ),
        }
        for command in ('cargo install ripr --version "$RIPR_VERSION" --locked',
                        'cargo build -p xtask --locked',
                        SEED_SCAN):
            mutations[f"seed omits {command}"] = text.replace(
                seed_text, seed_text.replace(command, "true", 1), 1
            )
        # #16126 negative controls for the fact cache's one-writer boundary.
        mutations["analysis job becomes a fact-cache writer"] = text.replace(
            FACT_CACHE_RESTORE, FACT_CACHE_SAVE, 1
        )
        mutations["seed stops writing the fact cache"] = text.replace(
            seed_text, seed_text.replace(FACT_CACHE_SAVE, FACT_CACHE_RESTORE, 1), 1
        )
        mutations["fact cache pin is unapproved"] = text.replace(
            FACT_CACHE_RESTORE, "actions/cache/restore@" + "b" * 40, 1
        )
        mutations["fact cache key drops the analyzer version"] = text.replace(
            "key: ripr-facts-${{ env.RIPR_VERSION }}-${{ github.sha }}",
            "key: ripr-facts-${{ github.sha }}", 1
        )
        mutations["cached path drifts from the exported path"] = text.replace(
            FACT_CACHE_PATH, "path: ${{ runner.temp }}/ripr-facts-cache", 1
        )
        mutations["lane reads the fact cache path before declaring it"] = text.replace(
            "      - name: Point ripr's fact cache outside the workspace\n"
            f"        run: {FACT_CACHE_DECLARATION}\n\n",
            "", 1
        )
        mutations["fact cache path returns to a job-level env"] = text.replace(
            "    env:\n      RIPR_VERSION: \"0.10.0\"\n",
            "    env:\n      RIPR_VERSION: \"0.10.0\"\n"
            "      RIPR_CACHE_DIR: ${{ runner.temp }}/ripr-facts\n", 1
        )
        mutations["self-hosted lane loses its durable fact cache path"] = text.replace(
            SELFHOSTED_CACHE_DIR, "RIPR_CACHE_DIR: target/ripr/cache", 1
        )
        # #16209: the job-level declaration alone is not enough -- docker run
        # must also forward the env var and mount the host directory into the
        # container, or the container silently uses its own default cache.
        mutations["self-hosted container never receives the forwarded cache env"] = text.replace(
            SELFHOSTED_CACHE_FORWARD_LINE, "", 1
        )
        mutations["self-hosted container never receives the cache mount"] = text.replace(
            SELFHOSTED_CACHE_MOUNT_LINE, "", 1
        )
        mutations["fact-cache writer inherits instead of stating its guard"] = text.replace(
            seed_text,
            re.sub(
                r"(?m)^      - name: Save ripr fact cache\n        if: >-\n(?:          .*\n)+",
                "      - name: Save ripr fact cache\n",
                seed_text,
                count=1,
            ),
            1,
        )
        for name, mutated in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(text, mutated, "mutation must alter the real workflow")
                with self.assertRaises(AssertionError):
                    validate_static_contract(mutated.splitlines())

    def test_receipt_cache_is_exact_head_and_fresh_only(self) -> None:
        validate_receipt_cache_contract(self.source)
        validate_cache_family_accounting(self.source)

    def test_receipt_cache_mutations_fail_closed(self) -> None:
        """#16431 negative controls for the exact-head receipt memoization."""
        text = "\n".join(self.source)
        key_without_head = "key: ripr-plus-receipt-v1-${{ env.RIPR_VERSION }}-${{ hashFiles('policy/ripr-suppressions.toml') }}"
        key_without_suppressions = "key: ripr-plus-receipt-v1-${{ env.RIPR_VERSION }}-${{ github.sha }}"
        restore_keys_block = (
            "          restore-keys: |\n"
            f"            {RECEIPT_CACHE_RESTORE_PREFIX}\n"
        )
        mutations = {
            "save publishes without the fresh marker": text.replace(
                RECEIPT_CACHE_MARKER_GUARD, "success()", 1
            ),
            "save publishes without requiring success": text.replace(
                RECEIPT_CACHE_MARKER_GUARD,
                "hashFiles('target/receipts/quality/.ripr-plus-fresh') != ''", 1
            ),
            "receipt consumed without the exact-head guard": text.replace(
                RECEIPT_CACHE_EXACT_GUARD, "", 1
            ),
            "receipt key drops the head binding": text.replace(
                f"key: {RECEIPT_CACHE_KEY}", key_without_head, 1
            ),
            "receipt key drops the suppression hash": text.replace(
                f"key: {RECEIPT_CACHE_KEY}", key_without_suppressions, 1
            ),
            "adjudicator reads the wrong step output": text.replace(
                f"steps.{RECEIPT_CACHE_RESTORE_ID}.outputs.cache-hit",
                "steps.wrong-step.outputs.cache-hit", 1
            ),
            "exactness export renamed under the guard": text.replace(
                "RIPR_RECEIPT_CACHE_HIT=true", "RIPR_RECEIPT_CACHE_HIT=yes", 1
            ),
            "prefix restore keys removed from the contract shape": text.replace(
                restore_keys_block, "", 1
            ),
            "unapproved restore pin": text.replace(
                RECEIPT_CACHE_RESTORE, "actions/cache/restore@" + "c" * 40, 1
            ),
            "unapproved save pin": text.replace(
                RECEIPT_CACHE_SAVE, "actions/cache/save@" + "d" * 40, 1
            ),
            "receipt save step removed": re.sub(
                r"(?m)^      - name: Save memoized ripr-plus receipt\n(?:        .*\n)+",
                "", text, count=1
            ),
            "receipt restore step removed": re.sub(
                r"(?m)^      - name: Restore memoized ripr-plus receipt\n(?:        .*\n|          .*\n)+\n",
                "", text, count=1
            ),
            "produce branch stops recording freshness": text.replace(
                "target/receipts/quality/.ripr-plus-fresh", "target/receipts/quality/stale-marker", 2
            ),
        }
        for name, mutated in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(text, mutated, "mutation must alter the real workflow")
                with self.assertRaises(AssertionError):
                    validate_static_contract(mutated.splitlines())


if __name__ == "__main__":
    unittest.main()
