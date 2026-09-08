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
    commands = [field(step, "run", 8) for step in workflow_steps(seed)]
    for command in ('cargo install ripr --version "$RIPR_VERSION" --locked',
                    'cargo build -p xtask --locked'):
        if command not in commands:
            raise AssertionError(f"seed useful work is missing: {command}")


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
                        'cargo build -p xtask --locked'):
            mutations[f"seed omits {command}"] = text.replace(
                seed_text, seed_text.replace(f"run: {command}", "run: true", 1), 1
            )
        for name, mutated in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(text, mutated, "mutation must alter the real workflow")
                with self.assertRaises(AssertionError):
                    validate_static_contract(mutated.splitlines())


if __name__ == "__main__":
    unittest.main()
